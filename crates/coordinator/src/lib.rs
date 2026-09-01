//! Coordinator 프로세스 stub — coordinator/agent 최소 핸드셰이크.
//!
//! `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
//! 단계 3·4. **"완전한 Coordinator" 가 아니다** — `ExecutionGrant` 를
//! 서명해 발급하고, Agent 가 돌려준 `AgentGrantAck` 를 검증해
//! `grant_id`/`attempt_id`/`agent_device_id` 를 대조하는 것까지만 한다.
//! lease 발급 · 스케줄링 · 여러 Agent 동시 처리는 범위 밖(계획서 "Out" 절).
//!
//! # 키 배분 — 왜 파일 기반 `PersistentKeyring` 을 쓰지 않는가
//!
//! 계획서의 "확인 안 됨" 1번 — `PersistentKeyring` 은 등록된 개인키를
//! 나중에 다시 꺼내는 공개 API가 없다(`crates/crypto/src/keyring.rs`).
//! 이 stub 은 실제 키 프로비저닝을 증명하는 것이 목적이 아니므로,
//! 호출자가 직접 준 32바이트 시드로 [`SigningKey`] 를 만들어 이 프로세스
//! 메모리에만 들고, 상대의 공개키는 [`InMemoryKeyring`] 에 담아 검증에만
//! 쓴다 — `crates/cli/src/selftest.rs:540-541` 이 이미 쓰는 패턴 그대로다.

use std::io::{ErrorKind, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Duration;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, InMemoryKeyring, InMemoryReplayGuard,
    IngressMessage, KeyDirectorySource, SigningKey, SystemClock, VerifyingKey,
};
use gputeer_protocol::canonical::blake3_256;
use gputeer_protocol::pb;
use gputeer_protocol::signing::signing_input;
use prost::Message;

pub mod attempt_report_store;
pub mod checkpoint_manifest_store;
pub mod inventory_store;
pub mod job_store;
pub mod lease_store;
pub mod neighbor_report_store;
#[allow(dead_code)]
pub mod node_liveness_store;
pub mod multi_agent;
mod orchestrate;
pub mod replica_ack_store;
pub mod reservation_release;
pub mod staging_store;
use lease_store::{
    CoordinatorLeaseStore, LeaseStoreError, RenewDecision, ResumeDecision, ResumeRequestIdentity,
    StoredLease,
};

const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct CoordinatorConfig {
    /// `"127.0.0.1:0"` 처럼 포트 0 을 주면 커널이 임시 포트를 고른다.
    pub listen: String,
    pub own_seed: [u8; 32],
    pub agent_verifying_key: VerifyingKey,
    pub coordinator_device_id: String,
    pub agent_device_id: String,
    pub grant_id: String,
    pub attempt_id: String,
    /// ★ 테스트 전용 — 단계 5 거부 경로 검증(`coordinator-agent-selftest`).
    ///   서명 직후 `coordinator_signature` 의 마지막 바이트를 뒤집어
    ///   전송한다. 정직한 Coordinator 는 절대 자기 서명을 위조하지
    ///   않는다 — 이 플래그는 "위조된 Grant 가 도착했을 때 Agent 가
    ///   실제로 거부하는가" 를 프로세스 경계에서 확인하기 위한
    ///   자기 타락(self-corruption) 주입이다.
    pub corrupt_own_signature: bool,
    /// ★ 테스트 전용 — 같은 Grant wire bytes 를 같은 연결에 두 번
    ///   보낸다. 두 번째 전송은 `grant.encode_to_vec()` 을 다시 부르지
    ///   않고 **첫 번째와 동일한 `frame` 바이트**를 재사용한다 — 그래야
    ///   "논리적으로 같은 재발급" 이 아니라 "같은 wire bytes 의 replay"
    ///   를 시험한다. 이 모드에서는 Agent 의 두 번째 ACK 를 기다리지
    ///   않는다 — 대신 replay 가 확실히 거부됐는지 확인한 뒤 `Err` 로
    ///   끝난다(정상 `RESULT ok=true` 를 절대 찍지 않는다).
    pub send_grant_twice: bool,
    /// ★ 테스트 전용 — Agent의 ACK를 검증한 직후 연결을 의도적으로
    ///   닫는다. 재접속 조각의 첫 번째 프로세스 쌍이 실제로 끊긴
    ///   뒤 종료되는지 확인하기 위한 주입이며, 운영 재시도는 만들지
    ///   않는다.
    pub disconnect_after_ack: bool,
    pub drop_connection_after_ack_once: bool,
    pub max_connections: u32,
    pub accept_timeout_ms: u64,
    pub revoke_before_drop: bool,
    pub pause_before_next_accept_ms: u64,

    // ── Lease (2026-08-18, 코덱스 설계 · `p67` 프롬프트) ──────────
    //
    // `ExecutionGrant.lease` 는 proto 에 이미 있었지만 지금까지 이
    // stub 은 채우지 않았다. `Lease` 는 이미 `Signable`(독립 서명
    // 대상)이므로, 여기서 채우고 Agent 가 독립적으로 검증하는 것까지가
    // "Grant 가 유효한 Lease 를 운반한다" 는 최소 한 걸음이다 — lease
    // 발급·갱신 전체나 다중 Agent 는 여전히 범위 밖(계획 문서 "Out" 절).
    pub lease_id: String,
    pub job_id: String,
    pub fence_epoch: u64,
    /// Grant 에 실을 **이미 서명된** `JobManifest` 파일 경로.
    ///
    /// ★ `None` 이면 `manifest` 필드를 비운 채 보낸다 — 기존 시나리오가
    ///   전부 그 경로다.
    ///
    /// ★ **Coordinator 는 제출자 개인키를 갖지 않는다.** `gputeer submit`
    ///   이 만든 서명된 파일을 읽어서 실어 나르기만 한다.
    pub manifest_file: Option<PathBuf>,
    /// 테스트 전용 — nested Manifest 서명을 망가뜨린다.
    pub corrupt_manifest_signature: bool,
    /// 테스트 전용 — `manifest_hash` 를 실제 Manifest 와 다르게 채운다.
    pub corrupt_manifest_hash: bool,
    /// ★ 테스트 전용 — nested `Lease` 서명 직후 마지막 바이트를
    ///   뒤집는다. outer `Grant` 서명은 정상이므로, 이 시나리오는
    ///   "outer 검증만으로는 안 잡히고 Agent 가 nested Lease 를
    ///   **독립적으로** 검증해야만 잡히는가"를 시험한다.
    pub corrupt_lease_signature: bool,
    /// ★ 테스트 전용 — `Lease.expires_at_unix_ms` 를 발급 시각보다
    ///   과거로 만든다. `Lease::LIFETIME == LongLived` 라 `verify()`
    ///   가 만료 시각을 검사해야 거부된다.
    pub expire_lease: bool,

    // ── Lease 갱신 (2026-08-19, `docs/plans/2026-08-19_0500_...`) ──
    //
    // 정상 handshake(Grant/ACK) 뒤, **같은 TCP 연결**에 이어서
    // Agent 가 보낸 `RenewLeaseRequest` 를 받고 서명된
    // `RenewLeaseResult` 로 응답한다. `do_renew == false` 면 이 단계
    // 전체를 건너뛴다 — 기존 핸드셰이크 전용 시나리오와 완전히 같게
    // 동작한다(회귀 없음).
    /// 갱신 왕복을 수행할지 여부.
    pub do_renew: bool,
    /// 정상 갱신 시 새로 발급할 Lease 의 fence_epoch.
    ///
    /// ★ 기본 시나리오는 요청의 `fence_epoch` 와 같은 값을 준다
    ///   (`same_epoch_reuse_is_allowed_by_design` 과 일관). 이 값을
    ///   요청보다 **낮게** 주면 "낮은 fence_epoch" 강등 시나리오를
    ///   만들 수 있다 — Agent 쪽 `FenceWatermark` 가 거부해야 한다.
    pub renewed_fence_epoch: u64,
    /// ★ 테스트 전용 — `RenewOutcome` 을 강제로 주입한다(예:
    ///   `SUPERSEDED`=2, `QUARANTINED`=3). `None` 이면 정상 판정
    ///   (`RENEWED`=1, 새 Lease 를 담아 응답)을 쓴다.
    ///
    ///   실제 Coordinator 가 언제 SUPERSEDED/QUARANTINED 를 내리는지
    ///   정하는 정책은 범위 밖이다(계획서 "Out" 절) — 여기서는
    ///   "서명된 정책 거부가 전송·검증·분류되는가" 만 증명한다.
    pub renew_outcome_override: Option<i32>,
    /// ★ 테스트 전용 — `RenewLeaseResult.coordinator_signature` 의
    ///   마지막 바이트를 뒤집는다. Agent 가 결과 서명 검증으로
    ///   거부해야 한다.
    pub corrupt_renew_result_signature: bool,
    /// ★ 테스트 전용 — `RenewLeaseResult.lease` 안의 nested
    ///   `Lease.coordinator_signature` 만 뒤집는다. outer 결과
    ///   서명은 정상이므로, Agent 가 nested Lease 를 **독립
    ///   검증**해야만 잡히는 시나리오다.
    pub corrupt_renewed_lease_signature: bool,
    /// ★ 테스트 전용 — `RenewLeaseResult.request_nonce` 를 요청의
    ///   nonce 를 echo 하지 않고 다른 값으로 채운다(서명은 정상).
    ///   Agent 가 이 값을 자신이 보낸 요청의 nonce 와 대조해 거부해야
    ///   한다 — 안 그러면 다른 갱신 요청에 대한 결과가 재사용될 수
    ///   있다(계획서 "In" 절 — `request_nonce` 의 목적).
    pub corrupt_renew_result_nonce: bool,
    /// Test-only: after `build_renew_result()` has durably committed a renewed
    /// Lease, drop the first connection before encoding or sending the result.
    pub drop_after_renew_commit_before_result_once: bool,
    /// Renewal lifetime used by the stub.  The default remains 60 seconds;
    /// short values make expiry-after-ambiguous-commit tests deterministic.
    pub renew_extension_ms: u64,

    // ── 반복 Lease 갱신 (2026-08-19, `docs/plans/2026-08-19_2330_...`) ──
    /// 같은 연결에서 갱신 왕복을 이 횟수만큼 반복한다. `do_renew ==
    /// false` 면 무시된다. 기본값 1은 기존(단일 왕복) 시나리오와
    /// 완전히 같게 동작한다.
    pub renew_rounds: u32,

    // ── Coordinator 영속 Lease 저장소 (2026-08-19, `docs/plans/2026-08-19_2300_...`) ──
    /// SQLite 파일에 발급한 Lease 의 신원(identity)과 epoch 를
    /// 영속한다 — 재시작 후에도 자신이 무엇을 발급했는지 기억한다.
    /// `None` 이면(기존 시나리오 전부) **이 조각 이전과 완전히 같은
    /// 동작** — `config.fence_epoch`/`config.lease_id` 등을 그 실행
    /// 동안만 쓰는 기존 레거시 경로를 그대로 쓴다(회귀 없음).
    pub lease_db_path: Option<PathBuf>,
    /// `lease_db_path == None` 인 레거시 경로를 의도적으로 선택했음을
    /// 호출자가 확인했는지 여부. 기본값은 `false`이며, 위험을 명시적으로
    /// 수락하지 않으면 Coordinator는 시작하지 않는다.
    pub allow_unsafe_legacy_mode: bool,

    // ── max_total_duration_seconds 갱신 차단 (2026-08-19, `docs/plans/2026-08-19_2350_...`) ──
    /// 최초 발급 시 후보값으로만 쓰인다 — 이미 저장소에 있는 Lease 의
    /// 값은 저장소가 권위를 갖는다(`get_or_issue()` 와 같은 원칙).
    /// `lease_store` 가 `None`(레거시)이면 이 값은 저장은 되지만 갱신
    /// 판정에는 쓰이지 않는다 — 그 경로는 매 요청마다 `issued_at_unix_ms`
    /// 를 즉석에서 재구성해 실제 경과시간을 추적하지 못한다.
    pub max_total_duration_seconds: u64,

    // ── Lease revoke (2026-08-19) ───────────────────────────────────
    /// 완료된 갱신 회차가 이 값에 도달하면 같은 연결로
    /// `RevokeLeaseNotice` 를 보낸다. `Some(0)` 은 Grant/ACK 직후다.
    pub revoke_after_round: Option<u32>,
    /// ★ 테스트 전용 — ACK 직후 저장소에만 revoke를 확정하고 통지는
    /// 보내지 않는다. Agent가 같은 연결에서 갱신 요청을 보내면
    /// Coordinator의 signed `REVOKED` outcome 경로를 직접 시험한다.
    pub revoke_before_renew: bool,
    /// ACK 뒤에 받을 `NodeHeartbeat` 개수. 0 이면 이 구간이 없다.
    pub expect_heartbeats: u32,
    /// heartbeat 관측을 남길 SQLite 경로.
    ///
    /// ★ `None` 이면 관측이 **메모리에도 안 남는다** — 로그로 찍고
    ///   버린다. 그러면 Coordinator 가 재시작하는 순간 모든 노드가
    ///   "한 번도 못 봤다" 가 된다. `ADR-033` §7 이 지목한
    ///   `NodeRecord.last_heartbeat_unix_ms` 공백이 정확히 그것이다.
    pub liveness_db_path: Option<String>,
    /// ACK·heartbeat 뒤에 받을 `NeighborUnreachableReport` 개수.
    /// 0 이면 이 구간이 없다.
    pub expect_neighbor_reports: u32,
    /// 이웃 신고 관측을 남길 SQLite 경로.
    ///
    /// ★ `expect_neighbor_reports > 0` 인데 이 값이 `None` 이면 **아예
    ///   시작하지 않는다**(`run()` 이 bind 전에 막는다). 받아 놓고 안
    ///   남기면 `ADR-033` §7 의 "Broker 가 그 보고를 모아" 가 성립하지
    ///   않는데, 로그에는 받은 것처럼 찍히기 때문이다.
    ///
    ///   `:memory:` 와 빈 경로도 같은 이유로 막는다.
    pub neighbor_report_db_path: Option<String>,
    /// 다중 Agent lane 을 켜고 추가 신원을 등록한다.
    ///
    /// 형식: `id=pubkeyhex;id2=pubkeyhex2`
    pub extra_agents: Option<String>,
    /// ★ 테스트 전용 — 각 세션이 이만큼의 세션이 **동시에** 열릴
    ///   때까지 기다렸다가 진행한다. 0(기본값)이면 안 기다린다.
    ///
    ///   운영 경로는 이걸 켜지 않는다. 이건 "동시에 처리한다" 는
    ///   주장을 확률이 아니라 구조로 증명하기 위한 관문이다 —
    ///   순차 서버는 통과할 수가 없다.
    pub require_concurrent_sessions: u32,
    /// 다중 Agent lane 을 쓴다. 기본값은 기존 순차 경로다.
    pub multi_agent: bool,
    /// ★ 테스트 전용 — 통지의 lease_id 를 바꿔 Agent identity 검증을
    ///   확인한다. 서명은 바뀐 payload 에 대해 다시 만든다.
    pub revoke_lease_id_override: Option<String>,
    /// ★ 테스트 전용 — 통지의 fence_epoch 을 바꿔 Agent fencing 검증을
    ///   확인한다. 서명은 바뀐 payload 에 대해 다시 만든다.
    pub revoke_fence_epoch_override: Option<u64>,
    /// ★ 테스트 전용 — 정상 서명 뒤 coordinator_signature 를 변조한다.
    pub corrupt_revoke_signature: bool,
    /// ★ 테스트 전용 — Grant 안의 Lease 만료시각을 짧게 만든다.
    pub lease_ttl_ms: u64,
    /// ★ 테스트 전용 — revoke 전 실제 시간을 흘려보낸다.
    pub revoke_delay_ms: u64,
    /// Test-only delay after Grant/ACK and before reading a renewal request.
    pub renew_delay_ms: u64,
    /// Explicit opt-in Hello-first Resume lane. The default remains the
    /// historical server-first Grant/ACK lane.
    pub resume_protocol: bool,
    pub session_id: String,
}

/// A failure isolated to one accepted Coordinator session.
///
/// Transport and protocol failures are connection-scoped: the dispatcher logs
/// them and returns to `accept()`. A storage failure means the Coordinator can
/// no longer make a trustworthy lease decision, so `run()` fails closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorSessionError {
    Transport(String),
    Protocol(String),
    Storage(String),
}

impl std::fmt::Display for CoordinatorSessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(message) => write!(formatter, "transport: {message}"),
            Self::Protocol(message) => write!(formatter, "protocol: {message}"),
            Self::Storage(message) => write!(formatter, "storage: {message}"),
        }
    }
}

impl std::error::Error for CoordinatorSessionError {}

fn transport_error(context: &str, error: impl std::fmt::Display) -> CoordinatorSessionError {
    CoordinatorSessionError::Transport(format!("{context}: {error}"))
}

fn protocol_error(context: &str, error: impl std::fmt::Display) -> CoordinatorSessionError {
    CoordinatorSessionError::Protocol(format!("{context}: {error}"))
}

fn storage_error(context: &str, error: impl std::fmt::Display) -> CoordinatorSessionError {
    CoordinatorSessionError::Storage(format!("{context}: {error}"))
}

/// The legacy coordinator handlers still produce string errors for their
/// transport/protocol paths. Resume storage failures are different: they must
/// reach the dispatcher as a typed `Storage` error instead of being converted
/// into a signed UNAVAILABLE result or reclassified by message text.
enum SessionHandlerError {
    Legacy(String),
    Classified(CoordinatorSessionError),
}

impl From<String> for SessionHandlerError {
    fn from(message: String) -> Self {
        Self::Legacy(message)
    }
}

fn classify_resume_store_error(error: LeaseStoreError) -> CoordinatorSessionError {
    match error {
        // These are the only store failures classify_resume currently emits.
        LeaseStoreError::Io(_) | LeaseStoreError::LockTimeout => {
            storage_error("resume lease classification", error)
        }
        // These variants are policy rejections in the issue/renew APIs, not
        // valid classify_resume errors (ResumeDecision carries those policy
        // outcomes). If one ever crosses this boundary, fail closed.
        LeaseStoreError::NotFound
        | LeaseStoreError::IdentityConflict { .. }
        | LeaseStoreError::Revoked { .. }
        | LeaseStoreError::Expired { .. } => {
            storage_error("unexpected resume lease-store error", error)
        }
    }
}

/// 이 Coordinator 를 실행한다 — lane 선택도 여기서 한다.
///
/// ★ `multi_agent` 가 켜져 있으면 그 lane 으로 분기한다(독립 검수 8라운드에
///   `run_from_args` 에서 옮겼다). 그 lane 은 연결마다 스레드를 만들어
///   **동시 처리를 허용**한다(`max_connections` 는 총 accept 수이지 동시치가
///   아니고, 실제 중첩을 보장하지도 않는다 — 12라운드 정정). 아래 설명은
///   기본(순차) lane 의 것이다.
///
/// 리스닝을 시작하면 즉시 `stdout` 에 `READY <addr>` 한 줄을 찍는다 —
/// 호출자(주로 `coordinator-agent-selftest`)가 이 줄로 실제 바인딩된
/// 주소를 얻는다. `port 0` 요청은 커널이 포트를 고르므로 미리 알 수 없다.
///
/// 성공하면 `stdout` 에 `RESULT ok=true ...` 를 찍고 `Ok(())`,
/// 실패하면 그 이유를 담아 `Err` 를 반환한다(호출자가 exit code 로 매핑).
pub fn run(config: CoordinatorConfig) -> Result<(), String> {
    // ★ 검사를 **파서가 아니라 실행 진입점에** 둔다(2026-08-30 독립
    //   검수 2라운드 지적). 초안은 `run_from_args()` 에서만 불러서,
    //   라이브러리로 `run(config)` 를 직접 부르는 호출자는 검사를 통째로
    //   우회했다. CLI 는 이 저장소가 쓰는 한 가지 진입 방법일 뿐이다.
    validate_device_id(&config.agent_device_id)?;
    validate_device_id(&config.coordinator_device_id)?;

    // ★ lane 선택을 **여기서** 한다(독립 검수 6라운드 지적).
    //
    //   전에는 `run_from_args()` 만 분기해서, 라이브러리로
    //   `run(config)` 를 직접 부르면 `multi_agent` 설정이 **조용히**
    //   무시되고 순차 lane 이 돌았다. Agent 쪽 `run()` 은 이미 여기서
    //   분기하고 있었다 — 비대칭 자체가 결함이었다.
    if config.multi_agent {
        return multi_agent::run_multi_agent(config);
    }

    if config.lease_db_path.is_none() && !config.allow_unsafe_legacy_mode {
        return Err(
            "--lease-db 없이 실행하는 레거시 모드는 revoke/만료/max-duration 보호가 없다; \
             의도적으로 이 위험한 호환 모드를 사용하려면 \
             --i-understand-legacy-mode-is-unsafe true 를 지정하라"
                .to_string(),
        );
    }
    if config.lease_db_path.is_none() {
        eprintln!(
            "경고: 레거시 모드(--lease-db 없음)를 사용한다 — revoke/만료/max-duration 보호가 없다"
        );
    }

    // ★ fail closed — lease store 를 **listener bind 보다 먼저** 연다.
    //   `--lease-db` 를 안 주면(기존 전부) `None` 이라 이 단계는
    //   아무것도 하지 않는다(`docs/plans/2026-08-19_2300_...v1.md`).
    let mut lease_store = match &config.lease_db_path {
        Some(path) => {
            let store = match CoordinatorLeaseStore::open(path) {
                Ok(store) => store,
                Err(error) => {
                    let message = format!("lease store 저장소 열기 실패: {error}");
                    eprintln!(
                        "SESSION_ERROR peer=<startup> connection_attempt=<none> kind=storage error={message}"
                    );
                    return Err(message);
                }
            };
            if !store.is_durable() {
                let message = format!(
                    "lease store 저장소가 영속이 아니다(lease_db_path={path:?}) — \
                     재시작을 넘는 Lease 복원이 조용히 무력화된다"
                );
                eprintln!(
                    "SESSION_ERROR peer=<startup> connection_attempt=<none> kind=storage error={message}"
                );
                return Err(message);
            }
            Some(store)
        }
        None => None,
    };

    // ★ 이 lane 이 이웃 신고를 실제로 다루는가 — 아니면 시작하지 않는다.
    // ★ `kind=storage` 로 찍지 않는다(독립 검수 11라운드) — 이건 저장소
    //   연산이 아니라 **구성 충돌**이다. 원인이 다르면 이름도 달라야 한다.
    if let Some(message) = unsupported_neighbor_report_lane(&config, lane_from_config(&config)) {
        eprintln!("STARTUP_REFUSED reason=lane error={message}");
        return Err(message);
    }

    // ★ **이웃 신고 저장소를 listener bind 보다 먼저 연다**(독립 검수
    //   4·5라운드 지적). 4라운드 수정은 이 블록을 bind **뒤에** 두어
    //   주석과 코드가 어긋나 있었다 — 소켓이 열린 뒤 죽으면 그 사이에
    //   들어온 연결이 있을 수 있다. lease store 가 이미 바로 위에서
    //   같은 방식으로 검증된다.
    if config.expect_neighbor_reports > 0 && config.neighbor_report_db_path.is_none() {
        let message =
            "--expect-neighbor-reports 를 켰으면 --neighbor-report-db 가 있어야 한다".to_string();
        eprintln!(
            "SESSION_ERROR peer=<startup> connection_attempt=<none> kind=storage error={message}"
        );
        return Err(message);
    }
    let mut neighbor_store = if let (true, Some(path)) = (
        config.expect_neighbor_reports > 0,
        config.neighbor_report_db_path.as_ref(),
    ) {
        let store = match crate::neighbor_report_store::CoordinatorNeighborReportStore::open(
            path,
            &config.coordinator_device_id,
        ) {
            Ok(store) => store,
            Err(error) => {
                let message = format!("이웃 신고 저장소 열기 실패: {error}");
                eprintln!(
                    "SESSION_ERROR peer=<startup> connection_attempt=<none> kind=storage error={message}"
                );
                return Err(message);
            }
        };
        if !store.is_durable() {
            let message = format!(
                "이웃 신고 저장소가 영속이 아니다(neighbor_report_db_path={path:?}) — \
                 프로세스가 죽으면 모아 둔 관측이 통째로 사라지는데 로그에는 저장한 것처럼 찍힌다"
            );
            eprintln!(
                "SESSION_ERROR peer=<startup> connection_attempt=<none> kind=storage error={message}"
            );
            return Err(message);
        }
        Some(store)
    } else {
        None
    };

    let listener = TcpListener::bind(&config.listen).map_err(|e| format!("bind 실패: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;


    println!("READY {address}");
    std::io::stdout().flush().map_err(|e| e.to_string())?;

    let signing_key = SigningKey::from_bytes(&config.own_seed);
    let mut agent_keys = InMemoryKeyring::new();
    agent_keys.insert(config.agent_device_id.clone(), config.agent_verifying_key);

    let mut replay = InMemoryReplayGuard::new();
    let clock = SystemClock;

    listener
        .set_nonblocking(true)
        .map_err(|e| format!("listener nonblocking failed: {e}"))?;
    let mut connection_count = 0u32;
    loop {
        if connection_count >= config.max_connections {
            return Err("max-connections reached before completed session".into());
        }
        if connection_count > 0 && config.pause_before_next_accept_ms != 0 {
            std::thread::sleep(Duration::from_millis(config.pause_before_next_accept_ms));
        }
        let (mut stream, peer) =
            accept_with_deadline(&listener, Duration::from_millis(config.accept_timeout_ms))?;
        let connection_attempt = connection_count;
        connection_count += 1;
        println!("CONNECTION_ATTEMPT {connection_attempt} peer={peer}");

        match serve_one_connection(
            &config,
            &mut stream,
            &mut lease_store,
            &mut neighbor_store,
            &signing_key,
            &agent_keys,
            &mut replay,
            &clock,
            connection_attempt,
        ) {
            Ok(()) => {
                if connection_count >= config.max_connections {
                    return Ok(());
                }
            }
            Err(error @ CoordinatorSessionError::Transport(_))
            | Err(error @ CoordinatorSessionError::Protocol(_)) => {
                eprintln!(
                    "SESSION_ERROR peer={peer} connection_attempt={connection_attempt} kind={} error={error}",
                    session_error_kind(&error)
                );
                if connection_count >= config.max_connections {
                    return Err(error.to_string());
                }
            }
            Err(error @ CoordinatorSessionError::Storage(_)) => {
                eprintln!(
                    "SESSION_ERROR peer={peer} connection_attempt={connection_attempt} kind=storage error={error}"
                );
                return Err(error.to_string());
            }
        }
    }
}

/// **어느 lane 을 실제로 시작하는가** — 설정이 아니라 진입점이 말한다.
///
/// ★ 독립 검수 11라운드 지적. 전에는 관문이 `config.multi_agent` 를 읽어서
///   판정했는데, **`run_multi_agent()` 자체가 그 lane 이다.** 라이브러리
///   호출자가 그 함수를 `multi_agent: false` 로 부르면 관문이 "순차 lane
///   이구나" 하고 통과시켰다 — 관문이 자기가 어디 있는지를 남에게 물어본 셈.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NeighborReportLane {
    /// `run()` 이 직접 도는 경로 — **유일하게 신고를 받는다.**
    Sequential,
    /// `multi_agent::run_multi_agent()` — 수신 루프가 없다.
    MultiAgent,
}

/// 설정만 보고 lane 을 고른다 — `run()`/`run_from_args()` 처럼 **아직 분기하지
/// 않은** 자리에서만 쓴다. lane 진입점 안에서는 쓰지 않는다(그 자리는 답을
/// 이미 안다).
pub(crate) fn lane_from_config(config: &CoordinatorConfig) -> NeighborReportLane {
    if config.multi_agent {
        NeighborReportLane::MultiAgent
    } else {
        NeighborReportLane::Sequential
    }
}

/// 이웃 신고 옵션이 **이 lane 에서 실제로 동작하는가**를 본다.
///
/// ★ 구현하지 않은 조합은 거부한다 — 받아 놓고 안 하는 것이 가장 나쁘다.
///   운영자는 신고가 모이는 줄 안다.
pub(crate) fn unsupported_neighbor_report_lane(
    config: &CoordinatorConfig,
    lane: NeighborReportLane,
) -> Option<String> {
    if config.expect_neighbor_reports == 0 {
        return None;
    }
    if lane == NeighborReportLane::MultiAgent {
        return Some(
            "multi-agent lane 은 이웃 신고 수신을 구현하지 않았다 — --expect-neighbor-reports 와 함께 쓸 수 없다"
                .to_string(),
        );
    }
    // ★ 순차 lane 안에도 **신고 수신 루프에 닿지 못할 수 있는** 구성이 있다
    //   (독립 검수 11라운드). resume 경로는 그보다 먼저 반환하고, 세 test
    //   hook 은 ACK 직후 세션을 끝낼 수 있다 — 셋 중 둘은 항상 끝내고,
    //   `--drop-connection-after-ack-once` 는 `max_connections > 1` 이고 첫
    //   연결일 때만 끝낸다(13라운드 정정 — 전에 "세 hook 은 ACK 직후 끝낸다"
    //   고 뭉뚱그렸다). 닿는다고 **보장할 수 없으면** 받지 않는다.
    if config.resume_protocol {
        return Some(
            "resume 경로는 이웃 신고 수신에 닿기 전에 반환한다 — --expect-neighbor-reports 와 --resume-protocol 을 함께 줄 수 없다"
                .to_string(),
        );
    }
    for (enabled, flag) in [
        (config.send_grant_twice, "--send-grant-twice"),
        (config.disconnect_after_ack, "--disconnect-after-ack"),
        (
            config.drop_connection_after_ack_once,
            "--drop-connection-after-ack-once",
        ),
    ] {
        if enabled {
            // ★ 조건 없이 거부한다. `--drop-connection-after-ack-once` 는
            //   `max_connections > 1 && connection_attempt == 0` 일 때만
            //   실제로 끊지만(12라운드 정정 — 전에 "항상 끝낸다" 고 썼다),
            //   **닿을 수도 있고 아닐 수도 있는 구성**을 받아 주는 것이
            //   조용한 무시보다 낫지 않다.
            return Some(format!(
                "{flag} 는 ACK 직후 세션을 끝낼 수 있어 이웃 신고 수신에 닿는다고 보장할 수 없다 — --expect-neighbor-reports 와 함께 줄 수 없다"
            ));
        }
    }
    None
}

fn session_error_kind(error: &CoordinatorSessionError) -> &'static str {
    match error {
        CoordinatorSessionError::Transport(_) => "transport",
        CoordinatorSessionError::Protocol(_) => "protocol",
        CoordinatorSessionError::Storage(_) => "storage",
    }
}

fn classify_legacy_session_error(
    message: String,
    durable_store_enabled: bool,
) -> CoordinatorSessionError {
    let lower = message.to_ascii_lowercase();
    let transport = lower.contains("truncated")
        || lower.contains("stream")
        || lower.contains("connection reset")
        || lower.contains("connection aborted")
        || lower.contains("broken pipe")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || message.contains("스트림이 끊겼다")
        || message.contains("스트림 읽기 실패")
        || message.contains("연결이 끊겼다")
        || message.contains("전송 실패")
        || message.contains("flush 실패")
        || message.contains("연결에 더 이상 아무것도 오지 않았다");
    if message.contains("lease store") || message.contains("저장소") {
        storage_error("session lease operation", message)
    } else if transport {
        transport_error("session I/O", message)
    } else if durable_store_enabled && message.contains("Lease") && message.contains("실패") {
        storage_error("session durable lease operation", message)
    } else {
        protocol_error("session protocol", message)
    }
}

/// Dispatch and serve exactly one accepted connection. The implementation is
/// deliberately sequential; the outer loop owns accept/count/error isolation.
#[allow(clippy::too_many_arguments)]
fn serve_one_connection(
    config: &CoordinatorConfig,
    stream: &mut std::net::TcpStream,
    lease_store: &mut Option<CoordinatorLeaseStore>,
    neighbor_store: &mut Option<crate::neighbor_report_store::CoordinatorNeighborReportStore>,
    signing_key: &SigningKey,
    agent_keys: &InMemoryKeyring,
    replay: &mut InMemoryReplayGuard,
    clock: &SystemClock,
    connection_attempt: u32,
) -> Result<(), CoordinatorSessionError> {
    stream
        .set_nonblocking(false)
        .map_err(|e| transport_error("accepted stream blocking mode failed", e))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| transport_error("read timeout setup failed", e))?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| transport_error("write timeout setup failed", e))?;

    serve_one_connection_impl(
        config,
        stream,
        lease_store,
        neighbor_store,
        signing_key,
        agent_keys,
        replay,
        clock,
        connection_attempt,
    )
    .map_err(|error| match error {
        SessionHandlerError::Legacy(message) => {
            classify_legacy_session_error(message, lease_store.is_some())
        }
        SessionHandlerError::Classified(error) => error,
    })
}

#[allow(clippy::too_many_arguments)]
fn serve_one_connection_impl(
    config: &CoordinatorConfig,
    stream: &mut std::net::TcpStream,
    lease_store: &mut Option<CoordinatorLeaseStore>,
    neighbor_store: &mut Option<crate::neighbor_report_store::CoordinatorNeighborReportStore>,
    signing_key: &SigningKey,
    agent_keys: &InMemoryKeyring,
    replay: &mut InMemoryReplayGuard,
    clock: &SystemClock,
    connection_attempt: u32,
) -> Result<(), SessionHandlerError> {
    let now = clock.now_unix_ms();
    if config.resume_protocol {
        return serve_resume_connection(
            config,
            stream,
            lease_store,
            signing_key,
            agent_keys,
            replay,
            clock,
            connection_attempt,
        );
    }
    let mut grant = issue_grant(config, lease_store, signing_key, now, connection_attempt)?;

    if config.corrupt_own_signature {
        let last = grant
            .coordinator_signature
            .last_mut()
            .ok_or_else(|| "coordinator_signature 가 비어 있다".to_string())?;
        *last ^= 0x01;
    }

    let frame = write_frame(FrameType::Grant, &grant.encode_to_vec())
        .map_err(|e| format!("Grant 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .map_err(|e| format!("Grant 전송 실패: {e}"))?;

    // ★ replay 시나리오 — **똑같은 wire bytes** 를 다시 쓴다. `grant` 를
    //   다시 인코딩하지 않는다 — 그러면 "논리적으로 같은 재발급" 이지
    //   "같은 프레임의 replay" 가 아니게 된다. 검증 대상은 "같은 서명
    //   바이트가 두 번 오면 두 번째가 거부되는가" 다.
    if config.send_grant_twice {
        stream
            .write_all(&frame)
            .map_err(|e| format!("replay Grant 전송 실패: {e}"))?;
    }
    stream.flush().map_err(|e| e.to_string())?;

    let received = read_frame(
        stream,
        1,
        KeyDirectorySource::Provided(agent_keys),
        replay,
        clock,
    )
    .map_err(|e| format!("ACK 프레임 읽기/검증 실패: {e}"))?;

    // ★ `require_replay_checked()` 를 반드시 거친다 — replay 상태가
    //   `permits_side_effects()` 를 만족하지 못한 `Verified<M>` 로
    //   grant_id 대조 같은 부작용을 실행하지 않는다(§10).
    let ack = match &received {
        IngressMessage::GrantAck(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("ACK replay 검사 실패: {e:?}"))?,
        other => return Err(format!("예상하지 못한 응답 타입: {other:?}").into()),
    };

    if !ack.accepted {
        return Err("Agent 가 Grant 를 accepted=false 로 응답했다"
            .to_string()
            .into());
    }
    if ack.grant_id != grant.grant_id {
        return Err(format!(
            "grant_id 상관관계 불일치: 보낸 값 {} != ACK 값 {}",
            grant.grant_id, ack.grant_id
        )
        .into());
    }
    if ack.attempt_id != grant.attempt_id {
        return Err(format!(
            "attempt_id 상관관계 불일치: 보낸 값 {} != ACK 값 {}",
            grant.attempt_id, ack.attempt_id
        )
        .into());
    }
    if ack.agent_device_id != config.agent_device_id {
        return Err(format!(
            "agent_device_id 불일치: 기대값 {} != ACK 값 {}",
            config.agent_device_id, ack.agent_device_id
        )
        .into());
    }
    if ack.nonce != derive_nonce("grant-ack", &grant.grant_id, connection_attempt) {
        return Err("ACK nonce does not match connection attempt"
            .to_string()
            .into());
    }

    if config.drop_connection_after_ack_once
        && config.max_connections > 1
        && connection_attempt == 0
    {
        if config.revoke_before_drop {
            if let Some(store) = lease_store.as_mut() {
                store
                    .mark_revoked(
                        &grant.lease.as_ref().expect("Grant lease").lease_id,
                        clock.now_unix_ms(),
                    )
                    .map_err(|e| format!("lease store revoke before drop failed: {e}"))?;
            }
        }
        println!(
            "DROP_CONNECTION_AFTER_ACK_ONCE coordinator_acknowledged=true grant_id={}",
            grant.grant_id
        );
        return Ok(());
    }

    if config.disconnect_after_ack {
        println!(
            "DISCONNECT_AFTER_ACK coordinator_acknowledged=true grant_id={}",
            grant.grant_id
        );
        return Ok(());
    }

    // ★ replay 시나리오는 여기서 끝낸다 — **절대 `RESULT ok=true` 를
    //   찍지 않는다.** Agent 는 첫 번째(정상) Grant 에만 ACK 를 보내고
    //   두 번째(replay) Grant 는 거부해야 하므로, 이 연결에 더 이상
    //   올 것이 없다는 사실 자체가 검증 대상이다. `REPLAY_TIMEOUT`
    //   짧은 타임아웃으로 "그 이상 아무것도 안 온다" 를 빠르게 확정한다.
    if config.send_grant_twice {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .map_err(|e| e.to_string())?;
        return match read_frame(
                stream,
                1,
                KeyDirectorySource::Provided(agent_keys),
                replay,
                clock,
            ) {
                Err(error) => Err(format!(
                    "REPLAY_SCENARIO_NO_EXTRA_MESSAGE: 연결에 더 이상 아무것도 오지 않았다(기대한 결과) — {error}"
                ).into()),
                Ok(message) => Err(format!(
                    "REPLAY_SCENARIO_UNEXPECTED_EXTRA_MESSAGE: {message:?}"
                ).into()),
            };
    }

    // ★ 노드 생존 보고 (2026-08-29, ADR-033 §7 앞 단계).
    //
    //   이 stub 프로토콜에는 비동기 multiplexing 이 없으므로, ACK 뒤·
    //   갱신 앞의 **고정 위치**로 받는다. 실제 운영에서는 주기적으로
    //   비동기로 와야 하지만, 그러려면 프레임 다중화가 먼저 필요하다 —
    //   그 배선을 흉내 내지 않고 지금 표현 가능한 것만 한다.
    //
    //   `expect_heartbeats == 0`(기본값)이면 이 구간은 통째로 없다 —
    //   기존 시나리오와 바이트 단위로 같게 동작한다.
    // ★ 관측을 남길 곳을 **루프 밖에서** 한 번 연다. 회차마다 열면
    //   heartbeat 하나에 파일 열기가 하나씩 붙는다.
    //
    //   저장소 열기 실패는 `Storage` 로 분류해 fail-closed 한다
    //   (`DoD-37` 이 세운 규칙) — 관측을 못 남기는 채로 "살아 있다" 를
    //   계속 받아들이면, 그 사실이 어디에도 안 남는다.
    let mut liveness_store = match &config.liveness_db_path {
        Some(path) if config.expect_heartbeats > 0 => Some(
            crate::node_liveness_store::CoordinatorNodeLivenessStore::open(path).map_err(
                |error| {
                    SessionHandlerError::Classified(storage_error(
                        "liveness store open",
                        error,
                    ))
                },
            )?,
        ),
        _ => None,
    };

    for _ in 0..config.expect_heartbeats {
        let message = read_frame(
            stream,
            1,
            KeyDirectorySource::Provided(agent_keys),
            replay,
            clock,
        )
        .map_err(|e| format!("NodeHeartbeat 프레임 읽기/검증 실패: {e}"))?;

        // ★ `require_replay_checked()` — ACK·갱신과 같은 이유(§10).
        //   heartbeat 를 재생할 수 있으면 이미 죽은 노드를 살아 있는
        //   것처럼 보이게 만들 수 있다.
        let heartbeat = match &message {
            IngressMessage::NodeHeartbeat(verified) => verified
                .require_replay_checked()
                .map_err(|e| format!("NodeHeartbeat replay 검사 실패: {e:?}"))?,
            other => return Err(format!("예상하지 못한 heartbeat 타입: {other:?}").into()),
        };

        // 이 연결의 상대가 맞는가. 서명은 "이 장치가 보냈다" 를 증명할
        // 뿐이므로, 그 장치가 **이 연결의 그 장치인지**는 따로 본다.
        // ★ 이 대조는 **지금 도달 불가다** — 사실대로 적는다
        //   (2026-08-30 독립 검수 지적을 확인하다 알아냈다).
        //
        //   `NodeHeartbeat::signer_id()` 가 `device_id` 라, 다른 이름을
        //   실으면 서명 검증이 그 이름의 키를 못 찾아 먼저 막는다
        //   (`UnknownSigner`). 여기까지 오려면 등록된 **다른** Agent 가
        //   자기 이름·자기 키로 정상 서명해 보내야 하는데, 이 lane 은
        //   Agent 키를 하나만 등록한다.
        //
        //   그래도 지우지 않는다. keyring 에 신원이 둘 이상 들어가는
        //   순간(다중 Agent lane 이 그 방향이다) 이 대조가 유일한
        //   방어가 된다 — 등록된 B 가 A 를 대신해 "살아 있다" 고
        //   보고하는 것을 서명 검증은 막지 못한다.
        if heartbeat.device_id != config.agent_device_id {
            return Err(format!(
                "HEARTBEAT_REJECTED: device_id 불일치 — 기대값 {} != {}",
                config.agent_device_id, heartbeat.device_id
            )
            .into());
        }
        // 다른 Coordinator 로 보낸 heartbeat 를 이쪽으로 돌려쓸 수 없다.
        if heartbeat.coordinator_device_id != config.coordinator_device_id {
            return Err(format!(
                "HEARTBEAT_REJECTED: coordinator_device_id 불일치 — 기대값 {} != {}",
                config.coordinator_device_id, heartbeat.coordinator_device_id
            )
            .into());
        }
        // 어느 세대의 작업을 들고 살아 있는가. 발급한 Lease 와 다르면
        // 오래된 세대의 보고이므로 최신 상태로 오인하지 않는다.
        // ★ Lease 가 없으면 **거부한다.** 예전에는 `if let Some` 으로
        //   조용히 건너뛰었는데, 그러면 "3종을 항상 대조한다" 는
        //   불변식이 조건부가 된다(2026-08-29 독립 검수 지적).
        //   지금 이 Coordinator 는 항상 Lease 를 실어 보내므로 이
        //   가지는 도달하지 않지만, 나중에 그 가정이 깨졌을 때
        //   조용히 검사를 건너뛰기보다 멈추는 편이 낛다.
        let Some(lease) = grant.lease.as_ref() else {
            return Err(
                "HEARTBEAT_REJECTED: 이 연결의 Grant 에 Lease 가 없어 세대를 대조할 수 없다"
                    .to_string()
                    .into(),
            );
        };
        {
            if heartbeat.fence_epoch != lease.fence_epoch {
                return Err(format!(
                    "HEARTBEAT_REJECTED: fence_epoch 불일치 — 발급 {} != 보고 {}",
                    lease.fence_epoch, heartbeat.fence_epoch
                )
                .into());
            }
        }

        // ★ 세 대조를 **전부 통과한 뒤에만** 남긴다. 먼저 저장하면
        //   거부될 관측이 사실로 기록된다.
        //
        //   판정은 여전히 여기서 하지 않는다 — `classify_node_liveness`
        //   가 하고, 재배정은 `ADR-033` §8 의 여섯 조건이 필요하다.
        //   이 자리는 **사실만** 남긴다.
        if let Some(store) = liveness_store.as_mut() {
            let verified = match &message {
                IngressMessage::NodeHeartbeat(verified) => verified,
                other => return Err(format!("예상하지 못한 heartbeat 타입: {other:?}").into()),
            };
            let observed = store.observe(verified).map_err(|error| match error {
                // ★ 신원 충돌은 **저장소 장애가 아니다**(2026-08-30 독립
                //   검수 2라운드 지적). `Storage` 로 포장하면 이 세션만
                //   거부하는 게 아니라 Coordinator accept loop 전체가
                //   종료된다 — 장치 하나를 잘못 신고한 것 때문에 다른
                //   Agent 들의 작업까지 끊긴다.
                crate::node_liveness_store::NodeLivenessStoreError::DeviceChanged {
                    ..
                }
                | crate::node_liveness_store::NodeLivenessStoreError::InvalidHeartbeat {
                    ..
                }
                | crate::node_liveness_store::NodeLivenessStoreError::SignerIsNotTheDevice {
                    ..
                } => SessionHandlerError::Legacy(format!("HEARTBEAT_REJECTED: {error}")),
                // 진짜 저장소 장애는 fail-closed 다(`DoD-37` 규칙).
                other => {
                    SessionHandlerError::Classified(storage_error("liveness observe", other))
                }
            })?;
            println!(
                "HEARTBEAT_STORED node_id={} last_heartbeat_unix_ms={} advanced={}",
                observed.stored.node_id, observed.stored.last_heartbeat_unix_ms, observed.advanced
            );
        }

        println!(
            "HEARTBEAT_ACCEPTED node_id={} device_id={} fence_epoch={} running_attempts={} issued_at_unix_ms={}",
            heartbeat.node_id,
            heartbeat.device_id,
            heartbeat.fence_epoch,
            heartbeat.running_attempts,
            heartbeat.issued_at_unix_ms
        );
    }

    // ── 이웃 신고(`ADR-033` §7 의 관측 층) ───────────────────────────
    //
    // ★ **저장은 판정이 아니다.** §7 이 금지한 것은 관측을 판정으로
    //   승격하는 것이고, 이 구간은 관측을 남기기만 한다 — 세지 않고,
    //   임계값도 없고, "죽었다" 고 결론짓지 않는다. 그래서 멤버십 해소가
    //   없어도 안전하게 연결할 수 있다.
    //
    //   재배정은 여전히 `ADR-033` §8 의 여섯 조건이 필요하고, 그 관문
    //   (`crates/scheduler/src/reassignment.rs`)은 오늘 어떤 정직한
    //   호출부도 통과시키지 않는다.
    //
    // ★ 저장소는 heartbeat 와 같은 이유로 **루프 밖에서** 한 번 연다.
    //   열기 실패는 `Storage` 로 분류해 fail-closed 한다(`DoD-37` 규칙) —
    //   관측을 못 남기는 채로 신고를 계속 받아들이면 그 사실이 어디에도
    //   안 남는다.
    // ★ 저장소는 `run()` 이 **bind 보다 먼저** 열고 검증했다(독립 검수
    //   4라운드 정정) — 여기서 열면 Grant/ACK 를 다 지난 뒤에야 잘못된
    //   구성이 드러나고, 그때는 이미 Lease 를 영속 발급한 뒤다.
    //
    //   그러므로 이 자리에는 검사가 없다. `expect_neighbor_reports > 0`
    //   인데 `neighbor_store` 가 `None` 인 상태는 `run()` 이 만들지 않는다.

    for _ in 0..config.expect_neighbor_reports {
        let message = read_frame(
            stream,
            1,
            KeyDirectorySource::Provided(agent_keys),
            replay,
            clock,
        )
        .map_err(|e| format!("NeighborUnreachableReport 프레임 읽기/검증 실패: {e}"))?;

        // ★ `require_replay_checked()` — 이 메시지는 `ShortLived` 다.
        //   재생을 허용하면 **오래된 관측을 지금 것처럼** 보이게 만들 수
        //   있다(신선도 위조). 정족수를 혼자 채우는 것은 막지 못한다 —
        //   저장소가 기계별로 한 행만 두므로 N 번 넣어도 한 행이다.
        let verified = match &message {
            IngressMessage::NeighborUnreachableReport(verified) => {
                verified.require_replay_checked().map_err(|e| {
                    format!("NeighborUnreachableReport replay 검사 실패: {e:?}")
                })?;
                verified
            }
            other => {
                return Err(format!("예상하지 못한 이웃 신고 타입: {other:?}").into())
            }
        };
        let report = verified.get();

        // 이 연결의 상대가 맞는가. 서명은 "이 장치가 보냈다" 를 증명할
        // 뿐이므로, 그 장치가 **이 연결의 그 장치인지**는 따로 본다.
        //
        // ★ 이 대조는 **지금 도달 불가다** — heartbeat 경로와 같은 이유로
        //   사실대로 적는다. `NeighborUnreachableReport::signer_id()` 가
        //   `reporter_device_id` 라, 다른 이름을 실으면 서명 검증이 그
        //   이름의 키를 못 찾아 먼저 막는다(`UnknownSigner`). 여기까지
        //   오려면 등록된 **다른** Agent 가 자기 이름·자기 키로 정상
        //   서명해 보내야 하는데, 이 lane 은 Agent 키를 하나만 등록한다.
        //
        //   그래도 지우지 않는다. keyring 에 신원이 둘 이상 들어가는
        //   순간(다중 Agent lane 이 그 방향이다) 이 대조가 유일한 방어가
        //   된다 — 등록된 B 가 A 를 사칭해 남의 기계 이름으로 신고하는
        //   것을 서명 검증은 막지 못한다.
        if report.reporter_device_id != config.agent_device_id {
            return Err(format!(
                "NEIGHBOR_REPORT_REJECTED: reporter_device_id 불일치 — 기대값 {} != {}",
                config.agent_device_id, report.reporter_device_id
            )
            .into());
        }
        // 다른 Coordinator 로 보낸 신고를 이쪽으로 돌려쓸 수 없다.
        //
        // ★ **저장소도 같은 값을 대조한다** — 이 배선에서는 저장소를 열 때
        //   `config.coordinator_device_id` 를 그대로 넘기므로, 이 검사가
        //   없어도 신고는 저장소에서 거부된다(초안 주석은 "다른 것을 본다"
        //   고 썼는데 이 배선에서는 **같은 값**이다 — 독립 검수 1라운드
        //   뮤테이션이 그 사실을 드러냈다).
        //
        //   그래도 둔다. 얻는 것은 두 가지다 — 저장소를 **건드리기 전에**
        //   끝내는 것과, 거부 사유를 이 계층의 말로 분명히 하는 것.
        //   그리고 저장소가 다른 ID 로 열리는 배선이 생기면 그때는 둘이
        //   실제로 다른 것을 보게 된다.
        if report.coordinator_device_id != config.coordinator_device_id {
            return Err(format!(
                "NEIGHBOR_REPORT_REJECTED: coordinator_device_id 불일치 — 기대값 {} != {}",
                config.coordinator_device_id, report.coordinator_device_id
            )
            .into());
        }
        // ★ 세대(`fence_epoch`) 대조는 **없다** — 이 메시지에 그 필드가
        //   없기 때문이다. 없는 것을 만들어 넣지 않는다. 이웃 신고는
        //   신고자가 어느 세대를 들고 있는지와 무관한 관측이다.

        // ★ 대조를 전부 통과한 뒤에만 남긴다. 먼저 저장하면 거부될 관측이
        //   사실로 기록된다.
        if let Some(store) = neighbor_store.as_mut() {
            let outcome = store.record_verified_report(verified).map_err(|error| {
                match error {
                    // ★ 아래는 전부 **들어온 신고가 유발한** 문제다 — 입력이
                    //   잘못됐거나, 서명자가 신고 안의 장치와 다르거나, 그
                    //   기계가 이미 다른 장치에 묶여 있거나, 그 장치의 저장
                    //   상한이 찼다.
                    //
                    //   ("남의 Coordinator 앞" 은 여기서 뺐다 — 2라운드에서
                    //    fail-closed 로 옮겼는데 이 예시만 낡아 있었다.)
                    //
                    //   `Storage` 로 포장하면 accept loop 전체가 끝나 다른
                    //   Agent 들의 작업까지 끊긴다 — 2026-08-30 독립 검수가
                    //   heartbeat 경로에서 정확히 이 지적을 했다.
                    crate::neighbor_report_store::NeighborReportStoreError::InvalidInput(_)
                    | crate::neighbor_report_store::NeighborReportStoreError::SignerIsNotTheReporterDevice { .. }
                    | crate::neighbor_report_store::NeighborReportStoreError::ConflictingReporterDevice { .. }
                    | crate::neighbor_report_store::NeighborReportStoreError::ReporterDeviceQuotaExhausted { .. } => {
                        SessionHandlerError::Legacy(format!(
                            "NEIGHBOR_REPORT_REJECTED: {error}"
                        ))
                    }
                    // ★ **`AddressedToAnotherCoordinator` 도 여기가 아니다**
                    //   (독립 검수 2라운드 정정).
                    //
                    //   이 오류는 **읽기 경로에서도** 난다 — 저장된 행을
                    //   디코드할 때 그 행의 수신자가 우리와 다르면 같은
                    //   오류다(`decode_row`). 그리고 들어온 신고 쪽은 바로
                    //   위에서 이미 걸렀으므로, 저장소가 이 오류를 내면
                    //   그건 **이미 들어 있는 행이 남의 것**이라는 뜻이다
                    //   — 파일 복사·이관·설정 오류로 생긴 저장 상태의
                    //   불일치이지 이 신고의 문제가 아니다.
                    //
                    //   ★ 이 판단은 **위 세션 대조가 있어야 성립한다.**
                    //     그것이 없으면 두 원인을 구분할 수 없다 — 그
                    //     대조를 남겨 둔 진짜 이유가 여기에 있다.
                    //
                    // ★ **`Corrupt` 는 여기가 아니다**(독립 검수 1라운드 정정).
                    //
                    //   초안은 "손상된 행은 한 기계의 것이니 풀 전체를 멈추는
                    //   건 과하다" 며 세션 거부로 뒀는데, **그 근거가 사실과
                    //   다르다** — 저장소는 상한·축출을 처리하며 **같은 장치가
                    //   주장한 다른 기계의 행까지** 읽으므로 손상은 지금 신고와
                    //   무관한 행에서도 나온다.
                    //
                    //   그리고 손상은 들어온 입력의 문제가 아니라 **이미
                    //   영속된 데이터가 깨졌다**는 뜻이다. 그 상태로 계속
                    //   기록하면 깨진 DB 에 계속 쓰게 된다. heartbeat 경로도
                    //   손상을 `Storage` 로 fail-closed 한다.
                    //
                    // 진짜 저장소 장애와 손상은 fail-closed 다(`DoD-37` 규칙).
                    other => SessionHandlerError::Classified(storage_error(
                        "neighbor report record",
                        other,
                    )),
                }
            })?;
            match outcome {
                crate::neighbor_report_store::RecordOutcome::Recorded {
                    stored,
                    evicted,
                } => {
                    println!(
                        "NEIGHBOR_REPORT_STORED reporter_node_id={} unreachable_node_id={} observed_at_unix_ms={} evicted={}",
                        stored.reporter_node_id,
                        stored.unreachable_node_id,
                        stored.observed_at_unix_ms,
                        evicted.len()
                    );
                }
                crate::neighbor_report_store::RecordOutcome::NotNewer(stored) => {
                    // ★ 오류가 아니다 — 재전송·지연은 정상이다. 그러나
                    //   조용히 넘기지 않는다(`CLAUDE.md` §3).
                    println!(
                        "NEIGHBOR_REPORT_NOT_NEWER reporter_node_id={} unreachable_node_id={} kept_observed_at_unix_ms={}",
                        stored.reporter_node_id,
                        stored.unreachable_node_id,
                        stored.observed_at_unix_ms
                    );
                }
            }
        }

        println!(
            "NEIGHBOR_REPORT_ACCEPTED reporter_node_id={} reporter_device_id={} unreachable_node_id={} observed_at_unix_ms={}",
            report.reporter_node_id,
            report.reporter_device_id,
            report.unreachable_node_id,
            report.observed_at_unix_ms
        );
    }

    if config.revoke_before_renew {
        let lease = grant
            .lease
            .as_ref()
            .ok_or_else(|| "갱신 전 revoke 대상 Grant에 Lease가 없다".to_string())?;
        let store = lease_store
            .as_mut()
            .ok_or_else(|| "갱신 전 revoke 시나리오에는 --lease-db가 필요하다".to_string())?;
        let revoked_at = SystemClock.now_unix_ms();
        store
            .mark_revoked(&lease.lease_id, revoked_at)
            .map_err(|e| format!("갱신 전 revoke 저장 실패: {e}"))?;
        println!(
            "REVOKE_STORE ok=true lease_id={} revoked_at_unix_ms={revoked_at}",
            lease.lease_id
        );
    }

    // 현재 stub 프로토콜에는 비동기 이벤트 multiplexing 이 없으므로,
    // Agent가 이 옵션을 알고 있는 고정 순차 경로로 revoke를 받는다.
    let revoked_after_grant = if config.revoke_after_round == Some(0) {
        send_revoke_notice(config, &grant, lease_store, signing_key, stream)?;
        true
    } else {
        false
    };

    // ★ Lease 갱신 (2026-08-19) — 같은 연결에 이어서 Agent 가 보낸
    //   `RenewLeaseRequest` 를 받고 서명된 `RenewLeaseResult` 로
    //   응답한다. `do_renew == false` 면 건너뛴다(기존 핸드셰이크
    //   전용 시나리오와 완전히 같게 동작). 반복 갱신(2026-08-19,
    //   `docs/plans/2026-08-19_2330_...`)이 추가되면서 왕복을
    //   `renew_rounds` 만큼 반복한다 — 기본값 1이면 기존 단일
    //   왕복과 동일하다. Coordinator 는 매 회차 요청의 nonce 를
    //   그대로 echo 할 뿐 스스로 회차를 유도하지 않는다 — 회차별
    //   nonce 분리는 Agent 가 요청을 만들 때 책임진다.
    if !revoked_after_grant {
        if config.do_renew && config.renew_delay_ms != 0 {
            std::thread::sleep(Duration::from_millis(config.renew_delay_ms));
        }
        for _round in if config.do_renew {
            0..config.renew_rounds
        } else {
            0..0
        } {
            let renew_msg = read_frame(
                stream,
                1,
                KeyDirectorySource::Provided(agent_keys),
                replay,
                clock,
            )
            .map_err(|e| format!("RenewLeaseRequest 프레임 읽기/검증 실패: {e}"))?;

            // ★ `require_replay_checked()` — ACK 와 같은 이유(§10).
            let renew_req = match &renew_msg {
                IngressMessage::LeaseRenew(verified) => verified
                    .require_replay_checked()
                    .map_err(|e| format!("RenewLeaseRequest replay 검사 실패: {e:?}"))?,
                other => return Err(format!("예상하지 못한 갱신 요청 타입: {other:?}").into()),
            };

            if renew_req.node_id != config.agent_device_id {
                return Err(format!(
                    "RenewLeaseRequest.node_id 불일치: 기대값 {} != {}",
                    config.agent_device_id, renew_req.node_id
                )
                .into());
            }
            if renew_req.lease_id != config.lease_id {
                return Err(format!(
                    "RenewLeaseRequest.lease_id 불일치: 기대값 {} != {}",
                    config.lease_id, renew_req.lease_id
                )
                .into());
            }
            // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — 이전에는
            //   `renew_req.fence_epoch` 를 아무것도와 대조하지 않았다.
            //   `lease_store` 가 있으면(2026-08-19,
            //   `docs/plans/2026-08-19_2300_...`) 저장된 fence_epoch 와
            //   대조한다 — 재시작을 넘어도 정확한 값이다. 없으면(레거시
            //   경로) 처음 발급한 `config.fence_epoch` 를 그 실행 동안만
            //   "Coordinator 가 기억하는 현재 epoch" 로 삼는다.
            let expected_epoch = match &lease_store {
                Some(store) => {
                    let stored = store
                        .get(&renew_req.lease_id)
                        .map_err(|e| format!("lease store 조회 실패: {e}"))?
                        .ok_or_else(|| {
                            format!(
                                "RenewLeaseRequest.lease_id({}) 가 lease store 에 없다",
                                renew_req.lease_id
                            )
                        })?;
                    stored.fence_epoch
                }
                None => config.fence_epoch,
            };
            let renew_now = clock.now_unix_ms();
            if renew_req.fence_epoch != expected_epoch {
                // 영속 저장소가 있는 경우에만 저장된 더 높은 epoch의 존재를
                // 재시작을 넘어 확인할 수 있다. 낮은 epoch는 정상적인
                // failover 경합에서 도착할 수 있으므로 연결을 끊지 않고,
                // 서명된 SUPERSEDED 정책 결과로 Agent가 스스로 물러나게
                // 한다. 레거시(None)와 높은 epoch는 기존 hard error를
                // 유지한다 — 새 epoch 발급 정책을 이 조각에서 만들지 않는다.
                if renew_req.fence_epoch < expected_epoch && lease_store.is_some() {
                    let result = build_signed_policy_renew_result(
                        config,
                        signing_key,
                        renew_now,
                        2, // RENEW_OUTCOME_SUPERSEDED
                        "a higher fence epoch already exists",
                        renew_req.nonce.clone(),
                    )?;
                    let frame = write_frame(FrameType::LeaseRenewResult, &result.encode_to_vec())
                        .map_err(|e| format!("RenewLeaseResult 프레임 인코딩 실패: {e}"))?;
                    stream
                        .write_all(&frame)
                        .map_err(|e| format!("RenewLeaseResult 전송 실패: {e}"))?;
                    stream.flush().map_err(|e| e.to_string())?;
                    println!(
                        "RENEW_RESULT ok=true outcome={} lease_id={}",
                        result.outcome, config.lease_id
                    );
                    break;
                }
                return Err(format!(
                    "RenewLeaseRequest.fence_epoch 불일치: 기대값 {} != {}",
                    expected_epoch, renew_req.fence_epoch
                )
                .into());
            }

            let result = build_renew_result(
                config,
                lease_store,
                signing_key,
                renew_now,
                &renew_req.lease_id,
                renew_req.nonce.clone(),
            )?;

            if config.drop_after_renew_commit_before_result_once && connection_attempt == 0 {
                let committed_lease = result.lease.as_ref().ok_or_else(|| {
                    "drop-after-renew-commit hook requires a RENEWED result with Lease".to_string()
                })?;
                if lease_store.is_none() {
                    return Err("drop-after-renew-commit hook requires durable --lease-db"
                        .to_string()
                        .into());
                }
                if config.revoke_before_drop {
                    lease_store
                        .as_mut()
                        .expect("durable store checked above")
                        .mark_revoked(&renew_req.lease_id, clock.now_unix_ms())
                        .map_err(|e| {
                            format!("lease store revoke after renew commit failed: {e}")
                        })?;
                }
                println!(
                    "DROP_AFTER_RENEW_COMMIT_BEFORE_RESULT_ONCE durable_commit=true lease_id={} expires_at_unix_ms={} request_nonce={}",
                    renew_req.lease_id,
                    committed_lease.expires_at_unix_ms,
                    hex_bytes(&renew_req.nonce)
                );
                return Ok(());
            }

            let frame = write_frame(FrameType::LeaseRenewResult, &result.encode_to_vec())
                .map_err(|e| format!("RenewLeaseResult 프레임 인코딩 실패: {e}"))?;
            stream
                .write_all(&frame)
                .map_err(|e| format!("RenewLeaseResult 전송 실패: {e}"))?;
            stream.flush().map_err(|e| e.to_string())?;

            println!(
                "RENEW_RESULT ok=true outcome={} lease_id={} request_nonce={}",
                result.outcome,
                config.lease_id,
                hex_bytes(&result.request_nonce)
            );

            // Agent는 정상 정책 거부(outcome=2/3/6/8)를 받으면 즉시
            // 갱신 함수를 종료하므로, 다음 회차의 요청을 기다리지 않는다.
            // Coordinator도 같은 회차에서 갱신 루프를 끝내야 교착/EOF 오류를
            // 만들지 않는다.
            if matches!(result.outcome, 2 | 3 | 6 | 8) {
                break;
            }

            if config.revoke_after_round == Some(_round + 1) {
                send_revoke_notice(config, &grant, lease_store, signing_key, stream)?;
                break;
            }
        }
    }

    println!(
        "RESULT ok=true grant_id={} attempt_id={} agent_device_id={}",
        grant.grant_id, grant.attempt_id, ack.agent_device_id
    );
    // Give an ACK-only Agent enough time to distinguish a completed session
    // from the deliberate drop-after-ACK test hook.
    std::thread::sleep(Duration::from_millis(100));
    return Ok(());
}

/// 이미 발급한 Grant 안의 Lease를 대상으로 revoke 통지를 만들고
/// 서명해 같은 연결로 보낸다.
///
/// `RevokeLeaseNotice::signer_id()`는 현재 프로토콜 계약상 `lease_id`를
/// 반환한다(V-08). 따라서 테스트용 target override도 바뀐 payload에
/// 대해 정상 서명해, Agent의 identity 검증과 서명 검증을 분리한다.
fn accept_with_deadline(
    listener: &TcpListener,
    timeout: Duration,
) -> Result<(std::net::TcpStream, std::net::SocketAddr), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok(connection) => return Ok(connection),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err("accept timeout exceeded".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(format!("accept failed: {error}")),
        }
    }
}

/// Hello-first dispatcher for the explicit Resume lane. This is deliberately
/// separate from the legacy Grant-first path so a caller cannot accidentally
/// make the two wire orderings ambiguous on one port.
fn serve_resume_connection(
    config: &CoordinatorConfig,
    stream: &mut std::net::TcpStream,
    lease_store: &mut Option<CoordinatorLeaseStore>,
    signing_key: &SigningKey,
    agent_keys: &InMemoryKeyring,
    replay: &mut InMemoryReplayGuard,
    clock: &SystemClock,
    connection_attempt: u32,
) -> Result<(), SessionHandlerError> {
    let hello_message = read_frame(
        stream,
        1,
        KeyDirectorySource::Provided(agent_keys),
        replay,
        clock,
    )
    .map_err(|e| format!("AgentSessionHello 프레임 읽기/검증 실패: {e}"))?;
    let hello = match hello_message {
        IngressMessage::SessionHello(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("AgentSessionHello replay 검사 실패: {e:?}"))?
            .clone(),
        other => {
            return Err(SessionHandlerError::Legacy(format!(
                "Resume lane에서 Hello가 아닌 프레임 수신: {other:?}"
            )))
        }
    };
    // ★ 리터럴 2 대신 공용 상수(2026-08-30 독립 검수 지적).
    if hello.mode != gputeer_protocol::constants::MODE_RESUME {
        return Err(SessionHandlerError::Legacy(format!(
            "AgentSessionHello.mode must be RESUME, got {}",
            hello.mode
        )));
    }
    if hello.node_id != config.agent_device_id {
        return Err(SessionHandlerError::Legacy(format!(
            "AgentSessionHello.node_id 불일치: 기대값 {} != {}",
            config.agent_device_id, hello.node_id
        )));
    }
    if hello.connection_attempt != connection_attempt {
        return Err(SessionHandlerError::Legacy(format!(
            "AgentSessionHello.connection_attempt 불일치: 기대값 {} != {}",
            connection_attempt, hello.connection_attempt
        )));
    }
    if hello.session_id.is_empty() {
        return Err(SessionHandlerError::Legacy(
            "AgentSessionHello.session_id가 비어 있다".into(),
        ));
    }

    let request_message = read_frame(
        stream,
        1,
        KeyDirectorySource::Provided(agent_keys),
        replay,
        clock,
    )
    .map_err(|e| format!("ResumeLeaseRequest 프레임 읽기/검증 실패: {e}"))?;
    let request = match request_message {
        IngressMessage::LeaseResume(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("ResumeLeaseRequest replay 검사 실패: {e:?}"))?
            .clone(),
        other => {
            return Err(SessionHandlerError::Legacy(format!(
                "Resume lane에서 ResumeLeaseRequest가 아닌 프레임 수신: {other:?}"
            )))
        }
    };
    if request.node_id != hello.node_id
        || request.session_id != hello.session_id
        || request.connection_attempt != hello.connection_attempt
    {
        return Err(SessionHandlerError::Legacy(
            "ResumeLeaseRequest와 AgentSessionHello 상관관계가 일치하지 않는다".into(),
        ));
    }

    let now = clock.now_unix_ms();
    let mut result = pb::ResumeLeaseResult {
        schema_version: 1,
        coordinator_id: config.coordinator_device_id.clone(),
        issued_at_unix_ms: now,
        request_nonce: request.request_nonce.clone(),
        ..Default::default()
    };
    let decision = match lease_store.as_ref() {
        Some(store) => store.classify_resume(
            &ResumeRequestIdentity {
                lease_id: request.lease_id.clone(),
                node_id: request.node_id.clone(),
                job_id: request.job_id.clone(),
                attempt_id: request.attempt_id.clone(),
                fence_epoch: request.fence_epoch,
            },
            now,
        ),
        None => Err(LeaseStoreError::Io(
            "resume requires a durable lease store".into(),
        )),
    };
    match decision {
        Ok(ResumeDecision::Resumed(stored)) => {
            result.outcome = 1;
            result.detail = "resume accepted; lease expiry was not extended".into();
            result.lease = Some(stored_to_signed_lease(config, signing_key, stored)?);
        }
        Ok(ResumeDecision::UnknownLease) => {
            result.outcome = 5;
            result.detail = "lease_id is not present in the durable store".into();
        }
        Ok(ResumeDecision::IdentityConflict {
            field,
            stored,
            requested,
        }) => {
            result.outcome = 6;
            result.detail =
                format!("identity conflict in {field}: stored={stored} requested={requested}");
        }
        Ok(ResumeDecision::Revoked { stored }) => {
            result.outcome = 2;
            result.detail = format!("lease revoked at {:?}", stored.revoked_at_unix_ms);
        }
        Ok(ResumeDecision::Expired { stored }) => {
            result.outcome = 3;
            result.detail = format!("lease expired at {}", stored.expires_at_unix_ms);
        }
        Ok(ResumeDecision::Superseded { stored }) => {
            result.outcome = 4;
            result.detail = format!("request epoch is below stored epoch {}", stored.fence_epoch);
        }
        Ok(ResumeDecision::EpochAhead { stored }) => {
            result.outcome = 8;
            result.detail = format!("request epoch is above stored epoch {}", stored.fence_epoch);
        }
        Err(error) => {
            return Err(SessionHandlerError::Classified(
                classify_resume_store_error(error),
            ));
        }
    }
    result.coordinator_signature = sign(signing_key, &result).to_vec();
    let frame = write_frame(FrameType::LeaseResumeResult, &result.encode_to_vec())
        .map_err(|e| format!("ResumeLeaseResult 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .map_err(|e| format!("ResumeLeaseResult 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;
    println!(
        "RESUME_RESULT ok=true outcome={} lease_id={} connection_attempt={}",
        result.outcome, request.lease_id, connection_attempt
    );
    println!(
        "RESULT ok=true resume_outcome={} lease_id={}",
        result.outcome, request.lease_id
    );
    Ok(())
}

fn send_revoke_notice(
    config: &CoordinatorConfig,
    grant: &pb::ExecutionGrant,
    lease_store: &mut Option<CoordinatorLeaseStore>,
    key: &SigningKey,
    stream: &mut std::net::TcpStream,
) -> Result<(), String> {
    let lease = grant
        .lease
        .as_ref()
        .ok_or_else(|| "revoke 대상 Grant에 Lease가 없다".to_string())?;

    // Persist the actual Grant lease id before constructing or sending any
    // wire notice. Test-only notice overrides must never change this key.
    if let Some(store) = lease_store {
        store
            .mark_revoked(&lease.lease_id, SystemClock.now_unix_ms())
            .map_err(|e| format!("lease store revoke 저장 실패: {e}"))?;
    }

    if config.revoke_delay_ms != 0 {
        std::thread::sleep(Duration::from_millis(config.revoke_delay_ms));
    }

    let notice = build_revoke_notice(
        lease,
        config.revoke_lease_id_override.as_deref(),
        config.revoke_fence_epoch_override,
        key,
        SystemClock.now_unix_ms(),
        config.corrupt_revoke_signature,
    )?;

    let frame = write_frame(FrameType::LeaseRevoke, &notice.encode_to_vec())
        .map_err(|e| format!("RevokeLeaseNotice 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .map_err(|e| format!("RevokeLeaseNotice 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;
    println!(
        "REVOKE_RESULT ok=true lease_id={} fence_epoch={} cause={}",
        notice.lease_id, notice.fence_epoch, notice.cause
    );
    Ok(())
}

fn build_revoke_notice(
    lease: &pb::Lease,
    lease_id_override: Option<&str>,
    fence_epoch_override: Option<u64>,
    key: &SigningKey,
    now_unix_ms: u64,
    corrupt_signature: bool,
) -> Result<pb::RevokeLeaseNotice, String> {
    let mut notice = pb::RevokeLeaseNotice {
        schema_version: 1,
        lease_id: lease_id_override
            .map(str::to_owned)
            .unwrap_or_else(|| lease.lease_id.clone()),
        fence_epoch: fence_epoch_override.unwrap_or(lease.fence_epoch),
        cause: pb::RevokeCause::Quarantine as i32,
        issued_at_unix_ms: now_unix_ms,
        ..Default::default()
    };
    notice.coordinator_signature = sign(key, &notice).to_vec();

    if corrupt_signature {
        let last = notice
            .coordinator_signature
            .last_mut()
            .ok_or_else(|| "RevokeLeaseNotice coordinator_signature가 비어 있다".to_string())?;
        *last ^= 0x01;
    }
    Ok(notice)
}

/// 저장된 더 높은 epoch에 의해 갱신이 대체된 경우의 정책 결과를 만든다.
///
/// 이 결과도 일반 갱신 결과와 같은 domain/signature 경로를 사용한다.
/// 특히 `lease=None`인 정책 거부는 nested Lease 검증으로 대체할 수 없으므로
/// Coordinator 서명이 없으면 Agent가 정상 failover 경합과 위조된 강제 중단을
/// 구별할 수 없다.
fn build_signed_policy_renew_result(
    config: &CoordinatorConfig,
    key: &SigningKey,
    now: u64,
    outcome: i32,
    detail: &str,
    request_nonce: Vec<u8>,
) -> Result<pb::RenewLeaseResult, String> {
    let request_nonce = if config.corrupt_renew_result_nonce {
        request_nonce.iter().map(|b| b ^ 0xFF).collect()
    } else {
        request_nonce
    };
    let mut result = pb::RenewLeaseResult {
        outcome,
        detail: detail.into(),
        schema_version: 1,
        coordinator_id: config.coordinator_device_id.clone(),
        issued_at_unix_ms: now,
        request_nonce,
        ..Default::default()
    };
    result.coordinator_signature = sign(key, &result).to_vec();

    if config.corrupt_renew_result_signature {
        let last = result
            .coordinator_signature
            .last_mut()
            .ok_or_else(|| "RenewLeaseResult coordinator_signature가 비어 있다".to_string())?;
        *last ^= 0x01;
    }

    Ok(result)
}

/// `RenewLeaseRequest` 에 대한 응답을 만들어 서명한다.
///
/// `renew_outcome_override` 가 있으면 그 값을 그대로 쓰고 새 Lease 를
/// 담지 않는다(서명된 정책 거부 시나리오). 없으면 `RENEW_OUTCOME_RENEWED`
/// 와 함께 새 Lease 를 독립적으로 서명해 담는다(규칙 i — nested 서명은
/// outer 서명과 별개다).
///
/// ★ Coordinator 영속 Lease 저장소(2026-08-19) — `lease_store` 가
///   `Some` 이면 `renew_existing_within_duration()` 으로 **저장된**
///   fence_epoch 를 그대로 쓰고 `expires_at`/`renew_after` 만 갱신한다
///   (`renewed_fence_epoch` CLI 값은 store 모드에서는 쓰이지 않는다 —
///   저장소가 epoch 의 권위를 갖는다). `None` 이면(레거시 경로) 기존과
///   동일하게 `config.renewed_fence_epoch` 를 쓴다.
///
/// ★ `max_total_duration_seconds` 갱신 차단(2026-08-19,
///   `docs/plans/2026-08-19_2350_...`) — `lease_store` 가 `Some` 일
///   때만 저장된 `issued_at_unix_ms` 기준 누적 시간을 판정한다.
///   초과했으면 `renew_outcome_override` 와 **무관하게** 서명된
///   `RENEW_OUTCOME_MAX_DURATION_EXCEEDED` 를 반환한다 — 실제 만료
///   정책이 테스트용 override 보다 우선해야, override 가 이 정책
///   검증을 가릴 수 없다. `lease_store` 가 `None` 이면(레거시) 이
///   판정 자체를 하지 않는다 — 그 경로는 매 요청마다
///   `issued_at_unix_ms` 를 즉석에서 재구성해 실제 경과시간을 추적
///   하지 못하기 때문이다(legacy enforcement bypass).
///
/// ★ 코덱스 독립 검수(2026-08-19, p114) 지적 — 처음 구현은
///   `lease_store=Some` 이고 override 도 있을 때 초과 여부와 무관하게
///   먼저 `renew_existing_within_duration()` 을 호출해 만료시각을
///   연장한 **뒤에** override 를 적용했다. 그러면 "거부 응답인데
///   저장소는 갱신됨" 이라는 상태 불일치가 생긴다(override 는
///   `lease_store=None` 경로에서 저장소를 전혀 안 건드리는 것과
///   대칭이어야 한다). 고친 뒤에는 override 가 있으면 읽기 전용
///   `get()` 으로 초과 여부만 먼저 확인하고(초과 시엔 여전히
///   outcome=6 이 이긴다), 저장소를 바꾸는 건 override 가 없을 때
///   `renew_existing_within_duration()` 하나뿐이다.
fn build_renew_result(
    config: &CoordinatorConfig,
    lease_store: &mut Option<CoordinatorLeaseStore>,
    key: &SigningKey,
    now: u64,
    lease_id: &str,
    request_nonce: Vec<u8>,
) -> Result<pb::RenewLeaseResult, String> {
    let request_nonce = if config.corrupt_renew_result_nonce {
        request_nonce.iter().map(|b| b ^ 0xFF).collect()
    } else {
        request_nonce
    };

    let policy_override = |outcome: i32, request_nonce: Vec<u8>| pb::RenewLeaseResult {
        outcome,
        detail: "policy override".into(),
        schema_version: 1,
        coordinator_id: config.coordinator_device_id.clone(),
        issued_at_unix_ms: now,
        request_nonce,
        ..Default::default()
    };

    let max_duration_exceeded_result = |request_nonce: Vec<u8>| pb::RenewLeaseResult {
        outcome: 6, // RENEW_OUTCOME_MAX_DURATION_EXCEEDED
        detail: "max total duration exceeded".into(),
        schema_version: 1,
        coordinator_id: config.coordinator_device_id.clone(),
        issued_at_unix_ms: now,
        request_nonce,
        ..Default::default()
    };

    let revoked_result = |request_nonce: Vec<u8>, revoked_at_unix_ms: u64| {
        pb::RenewLeaseResult {
            outcome: 8, // RENEW_OUTCOME_REVOKED
            detail: format!("lease revoked at unix ms: {revoked_at_unix_ms}"),
            schema_version: 1,
            coordinator_id: config.coordinator_device_id.clone(),
            issued_at_unix_ms: now,
            request_nonce,
            ..Default::default()
        }
    };

    let mut result = match lease_store {
        // ★ 코덱스 독립 검수(2026-08-19, p114) 지적 — override 가
        //   있으면 저장소를 **전혀 건드리지 않는다**(레거시 `None`
        //   경로와 같은 계약: "거부 응답인데 저장소는 갱신됨" 이라는
        //   상태 불일치를 만들지 않는다). 다만 실제 초과는 override
        //   보다 여전히 우선해야 하므로, 읽기 전용 `get()` 으로
        //   먼저 확인만 한다 — 저장소를 바꾸는 건 override 가 없을
        //   때 `renew_existing_within_duration()` 하나뿐이다.
        Some(store) => match config.renew_outcome_override {
            Some(outcome) => {
                let stored = store
                    .get(lease_id)
                    .map_err(|e| format!("lease store 조회 실패: {e}"))?
                    .ok_or_else(|| {
                        format!("RenewLeaseRequest.lease_id({lease_id}) 가 lease store 에 없다")
                    })?;
                if let Some(revoked_at_unix_ms) = stored.revoked_at_unix_ms {
                    revoked_result(request_nonce, revoked_at_unix_ms)
                } else if stored.expires_at_unix_ms <= now {
                    return Err(format!(
                        "lease store expired during renewal: {}",
                        LeaseStoreError::Expired {
                            expires_at_unix_ms: stored.expires_at_unix_ms,
                        }
                    ));
                } else if stored.is_max_duration_exceeded(now) {
                    max_duration_exceeded_result(request_nonce)
                } else {
                    policy_override(outcome, request_nonce)
                }
            }
            None => match store.renew_existing_within_duration(
                lease_id,
                now,
                now + config.renew_extension_ms,
                now + config.renew_extension_ms / 2,
            ) {
                Err(LeaseStoreError::Revoked { revoked_at_unix_ms }) => {
                    revoked_result(request_nonce, revoked_at_unix_ms)
                }
                Err(LeaseStoreError::Expired { expires_at_unix_ms }) => {
                    return Err(format!(
                        "lease store expired during renewal: {}",
                        LeaseStoreError::Expired { expires_at_unix_ms }
                    ));
                }
                Err(error) => return Err(format!("lease store 갱신 실패: {error}")),
                Ok(RenewDecision::MaxDurationExceeded(_)) => {
                    max_duration_exceeded_result(request_nonce)
                }
                Ok(RenewDecision::Renewed(resolved)) => {
                    build_renewed_lease_result(config, key, now, resolved, request_nonce)?
                }
            },
        },
        None => match config.renew_outcome_override {
            Some(outcome) => policy_override(outcome, request_nonce),
            None => {
                let resolved = StoredLease {
                    lease_id: config.lease_id.clone(),
                    job_id: config.job_id.clone(),
                    attempt_id: config.attempt_id.clone(),
                    holder_node_id: config.agent_device_id.clone(),
                    fence_epoch: config.renewed_fence_epoch,
                    expires_at_unix_ms: now + 60_000,
                    issuing_coordinator_id: config.coordinator_device_id.clone(),
                    coordinator_term: 1,
                    issued_at_unix_ms: now,
                    renew_after_unix_ms: now + 30_000,
                    max_total_duration_seconds: config.max_total_duration_seconds,
                    revoked_at_unix_ms: None,
                };
                build_renewed_lease_result(config, key, now, resolved, request_nonce)?
            }
        },
    };

    result.coordinator_signature = sign(key, &result).to_vec();

    if config.corrupt_renew_result_signature {
        let last = result
            .coordinator_signature
            .last_mut()
            .expect("RenewLeaseResult coordinator_signature 는 비어 있지 않다");
        *last ^= 0x01;
    }

    Ok(result)
}

/// 정상 갱신(`RENEW_OUTCOME_RENEWED`) 결과를 만든다 — 새 `Lease` 를
/// 독립적으로 서명해 담는다(규칙 i, nested 서명은 outer 서명과 별개).
/// `build_renew_result` 의 두 정상 경로(레거시·저장소 갱신 성공)가
/// 공유한다.
fn stored_to_signed_lease(
    _config: &CoordinatorConfig,
    key: &SigningKey,
    resolved: StoredLease,
) -> Result<pb::Lease, String> {
    let max_total_duration_seconds = u32_from_stored(
        resolved.max_total_duration_seconds,
        "max_total_duration_seconds",
    )?;
    let mut lease = pb::Lease {
        schema_version: 1,
        lease_id: resolved.lease_id,
        job_id: resolved.job_id,
        attempt_id: resolved.attempt_id,
        fence_epoch: resolved.fence_epoch,
        coordinator_term: resolved.coordinator_term,
        holder_node_id: resolved.holder_node_id.clone(),
        member_node_ids: vec![resolved.holder_node_id],
        issuing_coordinator_id: resolved.issuing_coordinator_id,
        issued_at_unix_ms: resolved.issued_at_unix_ms,
        expires_at_unix_ms: resolved.expires_at_unix_ms,
        renew_after_unix_ms: resolved.renew_after_unix_ms,
        max_total_duration_seconds,
        ..Default::default()
    };
    lease.coordinator_signature = sign(key, &lease).to_vec();
    Ok(lease)
}

fn build_renewed_lease_result(
    config: &CoordinatorConfig,
    key: &SigningKey,
    now: u64,
    resolved: StoredLease,
    request_nonce: Vec<u8>,
) -> Result<pb::RenewLeaseResult, String> {
    let max_total_duration_seconds = u32_from_stored(
        resolved.max_total_duration_seconds,
        "max_total_duration_seconds",
    )?;
    let mut lease = pb::Lease {
        schema_version: 1,
        lease_id: resolved.lease_id,
        job_id: resolved.job_id,
        attempt_id: resolved.attempt_id,
        fence_epoch: resolved.fence_epoch,
        coordinator_term: resolved.coordinator_term,
        holder_node_id: resolved.holder_node_id.clone(),
        member_node_ids: vec![resolved.holder_node_id],
        issuing_coordinator_id: resolved.issuing_coordinator_id,
        issued_at_unix_ms: resolved.issued_at_unix_ms,
        expires_at_unix_ms: resolved.expires_at_unix_ms,
        renew_after_unix_ms: resolved.renew_after_unix_ms,
        max_total_duration_seconds,
        ..Default::default()
    };
    lease.coordinator_signature = sign(key, &lease).to_vec();

    if config.corrupt_renewed_lease_signature {
        let last = lease
            .coordinator_signature
            .last_mut()
            .expect("Lease coordinator_signature 는 비어 있지 않다");
        *last ^= 0x01;
    }

    Ok(pb::RenewLeaseResult {
        outcome: 1, // RENEW_OUTCOME_RENEWED
        lease: Some(lease),
        detail: "ok".into(),
        schema_version: 1,
        coordinator_id: config.coordinator_device_id.clone(),
        issued_at_unix_ms: now,
        request_nonce,
        ..Default::default()
    })
}

fn issue_grant(
    config: &CoordinatorConfig,
    lease_store: &mut Option<CoordinatorLeaseStore>,
    key: &SigningKey,
    now: u64,
    connection_attempt: u32,
) -> Result<pb::ExecutionGrant, String> {
    let lease = issue_lease(config, lease_store, key, now)?;

    let mut grant = pb::ExecutionGrant {
        schema_version: 2,
        grant_id: config.grant_id.clone(),
        attempt_id: config.attempt_id.clone(),
        coordinator_device_id: config.coordinator_device_id.clone(),
        coordinator_term: 1,
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_nonce("grant", &config.grant_id, connection_attempt),
        // This bit is part of the v2 Grant signature.  It reports the store
        // actually used for this issue_lease() call, not the legacy opt-in.
        lease_from_durable_store: lease_store.is_some(),
        lease: Some(lease),
        ..Default::default()
    };

    // ★ Manifest 는 **제출자가 서명한 별도 메시지**다. Coordinator 는
    //   그것을 실어 나르기만 한다 — 그래서 여기서 outer Grant 서명
    //   **전에** nested 서명을 먼저 완성해야 한다(그래야 Grant 서명이
    //   최종 nested 바이트를 덮는다).
    if let Some(manifest) = load_signed_manifest(config)? {
        // `manifest_hash` 는 Agent 가 재계산해 대조한다(`CLAUDE.md` §0.2).
        // 여기서는 그 대조 대상을 정직하게 채운다.
        let mut digest = blake3_256(&signing_input(&manifest)).to_vec();
        if config.corrupt_manifest_hash {
            digest[0] ^= 0x01;
        }
        grant.manifest_hash = Some(pb::Digest {
            algo: 1, // HASH_ALGORITHM_BLAKE3_256
            value: digest,
        });
        grant.manifest = Some(manifest);
    }

    grant.coordinator_signature = sign(key, &grant).to_vec();
    Ok(grant)
}

/// 제출자가 서명해 둔 Manifest 파일을 읽는다.
///
/// ★ **Coordinator 는 이 Manifest 를 만들지도, 서명하지도 않는다.**
///   읽어서 그대로 실어 나른다. 서명 검증도 하지 않는다 — 그것은
///   Agent 가 제출자 공개키로 독립적으로 할 일이다. Coordinator 가
///   "검증했다" 고 대신 말해 주면 Agent 가 그 말을 믿게 되고, 그
///   순간 독립 검증이 사라진다.
///
/// ★ 처음 배선에서는 `--submitter-seed` 로 **제출자 개인키를 받아
///   여기서 직접 서명**했다. 테스트 편의였지만 그 구조가 남으면
///   같은 주체가 서명하고 실어 나르는 셈이라 Agent 의 독립 검증이
///   아무것도 증명하지 못한다. `gputeer submit` 으로 키를 밖으로
///   꺼내고 이 함수는 읽기만 한다.
fn load_signed_manifest(config: &CoordinatorConfig) -> Result<Option<pb::JobManifest>, String> {
    let Some(path) = config.manifest_file.as_ref() else {
        return Ok(None);
    };
    let bytes = std::fs::read(path)
        .map_err(|e| format!("Manifest 파일 읽기 실패({}): {e}", path.display()))?;
    let mut manifest = pb::JobManifest::decode(bytes.as_slice())
        .map_err(|e| format!("Manifest 디코딩 실패({}): {e}", path.display()))?;

    // 실어 나르는 도중 위조가 일어나는 상황을 재현한다. 서명은 파일에
    // 이미 들어 있으므로 Coordinator 는 그것을 **깨뜨릴 수만** 있고
    // 새로 만들 수는 없다 — 그것이 이 구조의 요점이다.
    if config.corrupt_manifest_signature {
        let last = manifest
            .submitter_signature
            .last_mut()
            .ok_or_else(|| "submitter_signature 가 비어 있어 손상시킬 수 없다".to_string())?;
        *last ^= 0x01;
    }
    Ok(Some(manifest))
}

/// `ExecutionGrant.lease` 에 실어 보낼 `Lease` 를 만들어 서명한다.
///
/// ★ **독립적으로 서명한다** — `Lease::coordinator_signature` 는
///   outer `Grant::coordinator_signature` 의 계산에 들어가지 않는
///   서명 대상 필드(§6 규칙 i, 중첩 메시지는 각자 서명된다)이므로,
///   여기서 위조하면 outer Grant 서명은 여전히 유효한 채로 남는다 —
///   `corrupt_lease_signature` 시나리오가 정확히 이 성질을 시험한다.
///
/// ★ Coordinator 영속 Lease 저장소(2026-08-19,
///   `docs/plans/2026-08-19_2300_...`) — `lease_store` 가 `Some` 이면
///   CLI 값을 후보로 저장소에 `get_or_issue()` 한다. `lease_id` 가
///   저장소에 **없으면** 후보가 그대로 최초 발급이 되고, **있으면**
///   identity 가 일치하는 한 **저장된 값이 CLI 값을 덮는다** — 재시작
///   후에도 같은 `lease_id` 는 항상 같은 epoch/expires_at 를 받는다.
///   `lease_store` 가 `None` 이면(기존 시나리오) CLI 값을 그대로
///   쓰는 레거시 경로 그대로다.
fn issue_lease(
    config: &CoordinatorConfig,
    lease_store: &mut Option<CoordinatorLeaseStore>,
    key: &SigningKey,
    now: u64,
) -> Result<pb::Lease, String> {
    let expires_at = if config.expire_lease {
        now.saturating_sub(1)
    } else {
        now + config.lease_ttl_ms
    };

    let resolved = match lease_store {
        None => StoredLease {
            lease_id: config.lease_id.clone(),
            job_id: config.job_id.clone(),
            attempt_id: config.attempt_id.clone(),
            holder_node_id: config.agent_device_id.clone(),
            fence_epoch: config.fence_epoch,
            expires_at_unix_ms: expires_at,
            issuing_coordinator_id: config.coordinator_device_id.clone(),
            coordinator_term: 1,
            issued_at_unix_ms: now,
            renew_after_unix_ms: now + 30_000,
            max_total_duration_seconds: config.max_total_duration_seconds,
            revoked_at_unix_ms: None,
        },
        Some(store) => {
            let candidate = StoredLease {
                lease_id: config.lease_id.clone(),
                job_id: config.job_id.clone(),
                attempt_id: config.attempt_id.clone(),
                holder_node_id: config.agent_device_id.clone(),
                fence_epoch: config.fence_epoch,
                expires_at_unix_ms: expires_at,
                issuing_coordinator_id: config.coordinator_device_id.clone(),
                coordinator_term: 1,
                issued_at_unix_ms: now,
                renew_after_unix_ms: now + 30_000,
                max_total_duration_seconds: config.max_total_duration_seconds,
                revoked_at_unix_ms: None,
            };
            store
                .get_or_issue(&candidate, now)
                .map_err(|e| format!("lease store 최초 발급 실패: {e}"))?
        }
    };

    let max_total_duration_seconds = u32_from_stored(
        resolved.max_total_duration_seconds,
        "max_total_duration_seconds",
    )?;
    let mut lease = pb::Lease {
        schema_version: 1,
        lease_id: resolved.lease_id,
        job_id: resolved.job_id,
        attempt_id: resolved.attempt_id,
        fence_epoch: resolved.fence_epoch,
        coordinator_term: resolved.coordinator_term,
        holder_node_id: resolved.holder_node_id.clone(),
        member_node_ids: vec![resolved.holder_node_id],
        issuing_coordinator_id: resolved.issuing_coordinator_id,
        issued_at_unix_ms: resolved.issued_at_unix_ms,
        expires_at_unix_ms: resolved.expires_at_unix_ms,
        renew_after_unix_ms: resolved.renew_after_unix_ms,
        max_total_duration_seconds,
        ..Default::default()
    };

    lease.coordinator_signature = sign(key, &lease).to_vec();

    if config.corrupt_lease_signature {
        let last = lease
            .coordinator_signature
            .last_mut()
            .expect("Lease coordinator_signature 는 비어 있지 않다");
        *last ^= 0x01;
    }

    Ok(lease)
}

/// `grant_id`(호출자가 시나리오마다 다르게 준다)에서 16바이트 nonce 를
/// 결정적으로 뽑는다. 실제 운영에서는 CSPRNG 를 쓰지만, 이 stub 은
/// 재현 가능한 selftest 시나리오가 목적이라 결정적 유도로 충분하다 —
/// 서로 다른 시나리오는 서로 다른 `grant_id` 를 쓰므로 nonce 도 갈린다.
///
/// ★ **운영 코드는 이 패턴을 쓰면 안 된다.** 같은 `grant_id` 로 다시
/// 부르면 같은 nonce 가 나온다. 이 stub 이 안전한 이유는 매 실행이
/// 새 OS 프로세스·새 `InMemoryReplayGuard` 를 쓰기 때문이다(실행 간
/// replay 상태가 없다) — `DurableReplayGuard` 처럼 재시작을 견디는
/// 저장소와 함께 쓰면, 재시작 후 같은 `grant_id` 를 다시 발급했을 때
/// 정당한 새 Grant 가 예전 nonce 와 충돌해 `Duplicate` 로 오판될 수
/// 있다(코덱스 독립 검수 2026-08-18 지적).
/// 저장소의 `u64` 값을 `pb::Lease.max_total_duration_seconds`(`u32`)
/// 로 변환한다.
///
/// ★ 코덱스 독립 검수(2026-08-19, p108) 지적 — 전에는 `as u32` 로
///   무검사 캐스팅했다. `CoordinatorLeaseStore` 는 스키마상 임의의
///   `u64` 를 저장할 수 있으므로(지금 발급 경로는 항상 `86_400` 을
///   쓰지만, 그것은 호출부의 우연한 사실이지 저장소의 계약이 아니다),
///   `u32::MAX` 를 넘는 값이 들어오면 조용히 잘려 다른 의미의 값이
///   전송될 수 있었다 — 진짜 데이터 무결성 결함이다. fail closed 로
///   바꾼다.
fn u32_from_stored(value: u64, field: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| {
        format!("lease store 의 {field} 값({value})이 u32 범위를 넘는다 — 저장소 손상 의심")
    })
}

/// nonce 유도 — 모든 성분을 **길이 접두사와 함께** 넣는다.
///
/// # 왜 그냥 이어 붙이면 안 되는가
///
/// ★ 2026-08-29 독립 검수가 찾았다. 초안은 `tag || 0x00 || id` 뒤에
///   `connection_attempt` 를 **0 이 아닐 때만** 붙였다. 그래서 서로 다른
///   입력이 같은 바이트가 됐다.
///
///   ```text
///   id = "L:51234", attempt = 0            -> "...\0L:51234"
///   id = "L:5",     attempt = 0x31323334   -> "...\0L:5" + "1234"
///   ```
///
///   두 번째의 4바이트가 ASCII `"1234"` 라 첫 번째와 완전히 같아진다.
///   지금 bounded reconnect 범위에서는 도달하지 않지만, **함수가 단사가
///   아니면** 언젠가 두 다른 요청이 같은 nonce 를 갖고 하나가 replay 로
///   거부된다 — 그때 원인을 찾기가 매우 어렵다.
///
///   이 저장소는 같은 교훈을 이미 배웠다. `start_checkpoint_id()` 는
///   "길이-프리픽스된 job_id/attempt_id/grant_id — canonical encoding
///   결함 방지" 라고 주석까지 달아 뒀는데, 이 함수는 그러지 않았다.
///
/// 이제 모든 성분이 고정 폭 길이 접두사를 갖고, `connection_attempt` 는
/// 값과 무관하게 **항상** 고정 8바이트로 들어간다.
fn derive_nonce(tag: &str, id: &str, connection_attempt: u32) -> Vec<u8> {
    // ★ 계산은 `crates/protocol` 에 **한 군데만** 있다. 예전에는
    //   이 계산이 agent·coordinator·selftest 세 군데에 복사돼 있었고,
    //   실제로 둘만 고치고 셋째를 놓쳐 selftest 가 깨졌다.
    gputeer_protocol::nonce::derive_replay_nonce(tag, id, connection_attempt)
}


fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `--flag value` 쌍으로 이루어진 CLI 인자를 설정으로 바꾸기만 한다 —
/// lane 선택도 관문도 여기서 하지 않는다.
///
/// ★ `run_from_args` 에서 떼어냈다(독립 검수 7라운드). 관문이 실제로
///   각 진입점에 있는지 재려면, 라이브러리 호출자처럼 설정을 만들어
///   진입점을 **직접** 부를 수 있어야 한다 — `CoordinatorConfig` 는
///   손으로 만들기에 필드가 너무 많다.
pub fn parse_config_from_args(args: &[String]) -> Result<CoordinatorConfig, String> {
    let flags = parse_flags(args)?;

    let config = CoordinatorConfig {
        listen: flags.require("--listen")?,
        own_seed: hex_to_seed(&flags.require("--own-seed")?)?,
        agent_verifying_key: hex_to_verifying_key(&flags.require("--peer-pubkey")?)?,
        coordinator_device_id: flags.require("--coordinator-device-id")?,
        agent_device_id: flags.require("--agent-device-id")?,
        grant_id: flags.require("--grant-id")?,
        attempt_id: flags.require("--attempt-id")?,
        corrupt_own_signature: flags.bool_flag("--corrupt-own-signature"),
        send_grant_twice: flags.bool_flag("--send-grant-twice"),
        disconnect_after_ack: flags.bool_flag("--disconnect-after-ack"),
        drop_connection_after_ack_once: flags.bool_flag("--drop-connection-after-ack-once"),
        max_connections: flags.u32_flag_with_default("--max-connections", 1)?,
        accept_timeout_ms: flags.u64_flag_with_default("--accept-timeout-ms", 30_000)?,
        revoke_before_drop: flags.bool_flag("--revoke-before-drop"),
        pause_before_next_accept_ms: flags
            .u64_flag_with_default("--pause-before-next-accept-ms", 0)?,
        lease_id: flags.require("--lease-id")?,
        job_id: flags.require("--job-id")?,
        fence_epoch: flags.u64_flag("--fence-epoch")?,
        // Manifest 배선 — 안 주면 기존 경로 그대로(manifest 필드 없음).
        manifest_file: flags.0.get("--manifest-file").map(PathBuf::from),
        corrupt_manifest_signature: flags.bool_flag("--corrupt-manifest-signature"),
        corrupt_manifest_hash: flags.bool_flag("--corrupt-manifest-hash"),
        corrupt_lease_signature: flags.bool_flag("--corrupt-lease-signature"),
        expire_lease: flags.bool_flag("--expire-lease"),
        do_renew: flags.bool_flag("--do-renew"),
        renewed_fence_epoch: flags.u64_flag("--renewed-fence-epoch")?,
        renew_outcome_override: flags.i32_opt_flag("--renew-outcome-override")?,
        corrupt_renew_result_signature: flags.bool_flag("--corrupt-renew-result-signature"),
        corrupt_renewed_lease_signature: flags.bool_flag("--corrupt-renewed-lease-signature"),
        corrupt_renew_result_nonce: flags.bool_flag("--corrupt-renew-result-nonce"),
        drop_after_renew_commit_before_result_once: flags
            .bool_flag("--drop-after-renew-commit-before-result-once"),
        renew_extension_ms: flags.u64_flag_with_default("--renew-extension-ms", 60_000)?,
        renew_rounds: flags.u32_flag_with_default("--renew-rounds", 1)?,
        lease_db_path: flags.0.get("--lease-db").map(PathBuf::from),
        allow_unsafe_legacy_mode: flags.bool_flag("--i-understand-legacy-mode-is-unsafe"),
        max_total_duration_seconds: flags
            .u64_flag_with_default("--max-total-duration-seconds", 86_400)?,
        revoke_after_round: flags.u32_opt_flag("--revoke-after-round")?,
        revoke_before_renew: flags.bool_flag("--revoke-before-renew"),
        expect_heartbeats: flags.u32_flag_with_default("--expect-heartbeats", 0)?,
        liveness_db_path: flags.0.get("--liveness-db").cloned(),
        expect_neighbor_reports: flags
            .u32_flag_with_default("--expect-neighbor-reports", 0)?,
        neighbor_report_db_path: flags.0.get("--neighbor-report-db").cloned(),
        extra_agents: flags.0.get("--extra-agents").cloned(),
        require_concurrent_sessions: flags.u32_flag_with_default("--require-concurrent-sessions", 0)?,
        multi_agent: flags.bool_flag("--multi-agent"),
        revoke_lease_id_override: flags.0.get("--revoke-lease-id").cloned(),
        revoke_fence_epoch_override: flags.u64_opt_flag("--revoke-fence-epoch")?,
        corrupt_revoke_signature: flags.bool_flag("--corrupt-revoke-signature"),
        lease_ttl_ms: flags.u64_flag_with_default("--lease-ttl-ms", 60_000)?,
        revoke_delay_ms: flags.u64_flag_with_default("--revoke-delay-ms", 0)?,
        renew_delay_ms: flags.u64_flag_with_default("--renew-delay-ms", 0)?,
        resume_protocol: flags.bool_flag("--resume-protocol"),
        session_id: flags
            .0
            .get("--session-id")
            .cloned()
            .unwrap_or_else(|| "legacy-session".into()),
    };
    Ok(config)
}

/// CLI 인자를 [`parse_config_from_args`] 로 설정으로 바꾼 뒤 실행한다.
/// 파싱 자체는 이 함수가 하지 않는다.
///
/// `crates/cli` 는 이 함수를 호출하기만 하고 인자 의미는 여기
/// (Coordinator 스트림)가 정의한다(`docs/contracts/01_스트림_소유권.md`).
///
/// ★ 여기에도 lane 관문이 있다 — CLI 로 들어오면 이것이 먼저 걸리므로,
///   각 lane 진입점의 관문은 **라이브러리 호출자에게만** 보인다.
///   그래서 그쪽은 `tests/neighbor_report_lane_guard.rs` 가 따로 잰다.
pub fn run_from_args(args: &[String]) -> Result<(), String> {
    let config = parse_config_from_args(args)?;

    // ★ **multi-agent lane 은 이웃 신고를 다루지 않는다**(독립 검수 5라운드
    //   지적). 그 lane 에는 수신 루프가 없는데 플래그는 받아들여져서
    //   **조용히 무시**됐다.
    //
    //   ★ 8라운드 정정 — 그 lane 은 이제 `run()` 의 분기를 거친다.
    //     이 관문은 그보다 앞이라 CLI 경로에서 먼저 걸리고, lane 진입점
    //     자체에도 같은 관문이 따로 있다.
    //
    //   구현하지 않은 조합은 **거부한다.** 받아 놓고 안 하는 것이 가장
    //   나쁘다 — 운영자는 신고가 모이는 줄 안다.
    // ★ `kind=storage` 로 찍지 않는다(독립 검수 11라운드) — 이건 저장소
    //   연산이 아니라 **구성 충돌**이다. 원인이 다르면 이름도 달라야 한다.
    if let Some(message) = unsupported_neighbor_report_lane(&config, lane_from_config(&config)) {
        eprintln!("STARTUP_REFUSED reason=lane error={message}");
        return Err(message);
    }
    run(config)
}

/// Agent 식별자로 쓸 수 있는 모양인가.
///
/// # 왜 검사하는가
///
/// ★ 2026-08-30 독립 검수 지적. `scoped_id()` 가 이 값을 식별자 네 곳
///   (`lease_id`·`job_id`·`attempt_id`·`grant_id`)에 그대로 복제한다.
///   "사람이 정한 짧은 이름" 이라는 전제로 축약을 없앴는데, 그 전제를
///   코드가 강제하지 않으면 전제가 아니라 희망이다.
///
/// ```text
/// `;` `=`   --extra-agents 문법과 충돌한다(id=key;id2=key2)
/// 개행      로그 한 줄을 여러 줄로 쪼개 위조·파싱 혼동을 만든다
/// 매우 긴 값 네 곳에 복제돼 프레임 상한을 넘기고 저장소를 부풀린다
/// 경로 문자 checkpoint 디렉터리 이름으로 흘러 들어간다
/// ```
///
/// 영숫자와 `-`·`_`·`.` 만 허용한다. 이 저장소가 쓰는 ULID 계열
/// 식별자는 전부 이 안에 들어간다.
pub const MAX_DEVICE_ID_LEN: usize = 64;

pub fn validate_device_id(device_id: &str) -> Result<(), String> {
    if device_id.is_empty() {
        return Err("DEVICE_ID_REJECTED: 비었다".to_string());
    }
    if device_id.len() > MAX_DEVICE_ID_LEN {
        return Err(format!(
            "DEVICE_ID_REJECTED: {}자다 — 상한 {MAX_DEVICE_ID_LEN}자.              이 값은 식별자 네 곳에 복제된다",
            device_id.len()
        ));
    }
    if let Some(bad) = device_id
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.'))
    {
        return Err(format!(
            "DEVICE_ID_REJECTED: {bad:?} 는 쓸 수 없다({device_id:?}) —              영숫자와 - _ . 만 허용한다"
        ));
    }
    Ok(())
}

struct Flags(std::collections::HashMap<String, String>);

impl Flags {
    fn require(&self, key: &str) -> Result<String, String> {
        self.0
            .get(key)
            .cloned()
            .ok_or_else(|| format!("필수 인자 누락: {key}"))
    }

    /// 값이 있는 boolean 플래그(`--flag true`). 안 주면 `false`.
    /// 테스트 전용 거부 경로 플래그(단계 5)에만 쓴다 — 다른 모든
    /// 플래그는 여전히 필수 값을 가진다(`require`).
    fn bool_flag(&self, key: &str) -> bool {
        self.0.get(key).map(|v| v == "true").unwrap_or(false)
    }

    /// 정수 플래그. 안 주면 `0`(fence_epoch 의 첫 발급 기본값 —
    /// `FenceWatermark` 는 0 에서 시작하므로 `0` 은 항상 유효한 첫
    /// epoch 다).
    fn u64_flag(&self, key: &str) -> Result<u64, String> {
        match self.0.get(key) {
            None => Ok(0),
            Some(v) => v
                .parse::<u64>()
                .map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }

    fn u64_opt_flag(&self, key: &str) -> Result<Option<u64>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<u64>()
                .map(Some)
                .map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }

    /// 반복 Lease 갱신(2026-08-19) — 안 주면 `default`(왕복 횟수).
    /// 기본값 1은 기존 단일 왕복 시나리오와 동일하게 동작한다.
    fn u32_flag_with_default(&self, key: &str, default: u32) -> Result<u32, String> {
        match self.0.get(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<u32>()
                .map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }

    /// `max_total_duration_seconds`(2026-08-19) — 안 주면 `default`
    /// (기존 하드코딩 값 86,400초 = 24시간과 동일, 회귀 없음).
    fn u64_flag_with_default(&self, key: &str, default: u64) -> Result<u64, String> {
        match self.0.get(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<u64>()
                .map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }

    /// ★ 테스트 전용 — `RenewOutcome` 강제 주입(단계 5). 안 주면 `None`
    ///   (정상 판정 사용).
    fn i32_opt_flag(&self, key: &str) -> Result<Option<i32>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<i32>()
                .map(Some)
                .map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }

    fn u32_opt_flag(&self, key: &str) -> Result<Option<u32>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<u32>()
                .map(Some)
                .map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }
}

fn parse_flags(args: &[String]) -> Result<Flags, String> {
    let mut map = std::collections::HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = &args[i];
        if !key.starts_with("--") {
            return Err(format!("플래그가 아닌 인자: {key}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} 뒤에 값이 없다"))?;
        map.insert(key.clone(), value.clone());
        i += 2;
    }
    Ok(Flags(map))
}

fn hex_to_seed(hex: &str) -> Result<[u8; 32], String> {
    let bytes = hex_decode(hex)?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| "seed 는 정확히 32바이트여야 한다".into())
}

fn hex_to_verifying_key(hex: &str) -> Result<VerifyingKey, String> {
    let seed = hex_to_seed(hex)?;
    VerifyingKey::from_bytes(&seed).map_err(|e| format!("유효하지 않은 공개키: {e}"))
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("hex 문자열 길이가 홀수다".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 코덱스 독립 검수(2026-08-19, p108) 지적 — `u32_from_stored()`
    /// 가 실제로 fail closed 하는지(무검사 `as u32` 캐스팅으로
    /// 되돌아가지 않는지) 경계값으로 고정한다.
    #[test]
    fn u32_from_stored_accepts_max_total_duration_seconds_default() {
        assert_eq!(
            u32_from_stored(86_400, "max_total_duration_seconds"),
            Ok(86_400)
        );
    }

    #[test]
    fn u32_from_stored_accepts_u32_max() {
        assert_eq!(u32_from_stored(u64::from(u32::MAX), "x"), Ok(u32::MAX));
    }

    #[test]
    fn u32_from_stored_rejects_values_above_u32_max_instead_of_truncating() {
        let result = u32_from_stored(u64::from(u32::MAX) + 1, "x");
        assert!(
            result.is_err(),
            "u32::MAX 를 넘는 값이 조용히 잘리지 않고 거부돼야 한다: {result:?}"
        );
    }

    #[test]
    fn revoke_notice_uses_issued_lease_identity_and_epoch() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let lease = pb::Lease {
            lease_id: "lease-a".into(),
            fence_epoch: 7,
            ..Default::default()
        };
        let notice = build_revoke_notice(&lease, None, None, &key, 123, false)
            .expect("revoke notice가 만들어져야 한다");
        assert_eq!(notice.lease_id, "lease-a");
        assert_eq!(notice.fence_epoch, 7);
        assert_eq!(notice.issued_at_unix_ms, 123);
        assert_eq!(notice.cause, pb::RevokeCause::Quarantine as i32);
        assert!(!notice.coordinator_signature.is_empty());
    }

    #[test]
    fn revoke_notice_test_overrides_are_signed_payload_values() {
        let key = SigningKey::from_bytes(&[8u8; 32]);
        let lease = pb::Lease {
            lease_id: "lease-a".into(),
            fence_epoch: 7,
            ..Default::default()
        };
        let notice = build_revoke_notice(&lease, Some("lease-b"), Some(6), &key, 456, false)
            .expect("override revoke notice가 만들어져야 한다");
        assert_eq!(notice.lease_id, "lease-b");
        assert_eq!(notice.fence_epoch, 6);
        assert_eq!(
            notice.coordinator_signature,
            sign(&key, &notice).to_vec(),
            "override가 적용된 최종 payload에 대한 서명이어야 한다"
        );
    }
}

#[cfg(test)]
mod device_id_validation_tests {
    use super::{validate_device_id, MAX_DEVICE_ID_LEN};

    /// ★ 2026-08-30 독립 검수가 "새 validator 의 금지 문자·길이 부정
    ///   테스트가 없다" 고 지적했다. 검사를 만들어 놓고 그 검사가 실제로
    ///   막는지 확인하지 않으면, 나중에 조건이 뒤집혀도 아무도 모른다.
    #[test]
    fn the_forbidden_shapes_are_actually_refused() {
        let cases: &[(&str, &str)] = &[
            ("", "빈 값"),
            ("a;b", "--extra-agents 항목 구분자"),
            ("a=b", "--extra-agents key 구분자"),
            ("a\nb", "개행 — 로그 한 줄을 쪼갠다"),
            ("a b", "공백"),
            ("a/b", "경로 구분자"),
            ("../escape", "경로 탈출"),
            ("노드", "비ASCII"),
            ("a\0b", "NUL"),
        ];
        for (value, why) in cases {
            assert!(
                validate_device_id(value).is_err(),
                "{value:?} 가 통과했다 — {why}"
            );
        }
        let too_long = "a".repeat(MAX_DEVICE_ID_LEN + 1);
        assert!(validate_device_id(&too_long).is_err(), "상한을 넘겼는데 통과했다");
    }

    /// 정상 값은 통과하는가.
    ///
    /// ★ 위 테스트만 있으면 "전부 거부" 로도 통과한다.
    #[test]
    fn ordinary_identifiers_pass() {
        for value in [
            "01JAGENTSELFTEST00000000001",
            "node-1",
            "node_1",
            "node.1",
            &"a".repeat(MAX_DEVICE_ID_LEN),
        ] {
            assert!(validate_device_id(value).is_ok(), "{value:?} 가 거부됐다");
        }
    }
}
