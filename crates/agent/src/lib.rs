//! Agent 프로세스 stub — coordinator/agent 최소 핸드셰이크.
//!
//! `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
//! 단계 3·4. **"완전한 Agent" 가 아니다** — Coordinator 가 보낸
//! `ExecutionGrant` 를 검증하고, 서명된 `AgentGrantAck` 를 돌려주는
//! 것까지만 한다. Job 실행 · GPU 할당 · CUDA 실행은 범위 밖(계획서 "Out" 절).
//!
//! # 키 배분
//!
//! `crates/coordinator/src/lib.rs` 모듈 문서의 "키 배분" 절과 동일한
//! 이유로 `PersistentKeyring` 대신 호출자가 준 시드로 만든 [`SigningKey`]
//! 를 메모리에만 들고, Coordinator 공개키는 [`InMemoryKeyring`] 에 담아
//! 검증에만 쓴다.

use std::fs;
use std::io::{ErrorKind, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use gputeer_checkpoint::durability::record_initial_state;
use gputeer_checkpoint::writer::{manifest_for, write_checkpoint_phased, WritePhase};
pub mod exec;
pub mod multi_agent;
pub mod owner_panel;
pub mod report;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, Ed25519Verifier, FrameType, FramingError,
    InMemoryKeyring, InMemoryReplayGuard, IngressMessage, KeyDirectorySource, SigningKey,
    SystemClock, VerifyingKey,
};
use gputeer_protocol::{pb, verify};
use gputeer_runtime_policy::{DurableFenceError, DurableFenceWatermark};
use prost::Message;

const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
enum SessionError {
    Retryable(String),
    Fatal(String),
    AmbiguousRenew(String),
}

impl From<String> for SessionError {
    fn from(value: String) -> Self {
        if value.contains("RETRYABLE_CONNECTION") || value.contains("RETRYABLE_RESUME") {
            Self::Retryable(value)
        } else if value.contains("AMBIGUOUS_RENEW") {
            Self::AmbiguousRenew(value)
        } else {
            Self::Fatal(value)
        }
    }
}

impl From<&str> for SessionError {
    fn from(value: &str) -> Self {
        Self::Fatal(value.to_owned())
    }
}

struct RetryPolicy {
    max_attempts: u32,
    max_duration: Duration,
    base_delay: Duration,
    cap_delay: Duration,
    connect_timeout: Duration,
    safety_margin_ms: u64,
}

struct RetryBudget {
    started_at: std::time::Instant,
    deadline: std::time::Instant,
    lease_expires_at_unix_ms: Option<u64>,
}

impl RetryBudget {
    fn new(policy: &RetryPolicy) -> Self {
        let started_at = std::time::Instant::now();
        Self {
            started_at,
            deadline: started_at + policy.max_duration,
            lease_expires_at_unix_ms: None,
        }
    }

    fn update_lease_deadline(
        &mut self,
        expires_at_unix_ms: u64,
        now_unix_ms: u64,
        policy: &RetryPolicy,
    ) {
        self.lease_expires_at_unix_ms = Some(expires_at_unix_ms);
        let remaining_ms = expires_at_unix_ms.saturating_sub(now_unix_ms);
        let lease_duration =
            Duration::from_millis(remaining_ms.saturating_sub(policy.safety_margin_ms));
        self.deadline = self
            .started_at
            .checked_add(policy.max_duration)
            .unwrap_or(self.deadline)
            .min(std::time::Instant::now() + lease_duration);
    }

    fn exhausted(&self) -> bool {
        std::time::Instant::now() >= self.deadline
    }
}

#[derive(Clone)]
pub struct AgentConfig {
    pub coordinator_addr: String,
    pub own_seed: [u8; 32],
    pub coordinator_verifying_key: VerifyingKey,
    /// nested `JobManifest` 를 **독립 검증**할 제출자 공개키.
    ///
    /// ★ `None` 이면 Grant 에 Manifest 가 실려 오는 것 자체를 거부한다 —
    ///   검증할 수 없는 실행 지시를 받아들이지 않는다(fail closed).
    ///   Manifest 가 없는 Grant 는 기존대로 통과한다.
    ///
    /// ★ 이 키의 authoritative 출처는 membership 이며 아직 없다. 지금은
    ///   CLI 로 받는다 — `coordinator_verifying_key` 가 같은 처지인 것과
    ///   같은 단계다.
    pub submitter_verifying_key: Option<VerifyingKey>,
    /// ★ **명시적 opt-in.** 이걸 켜지 않으면 Agent 는 실행 지시를
    ///   뽑기만 하고 **프로세스를 띄우지 않는다.**
    ///
    ///   `DoD-29` 의 `--i-understand-legacy-mode-is-unsafe` 와 같은
    ///   패턴이다 — 위험한 기본값을 실수로 켜는 것을 막는다. 다만
    ///   이건 더 위험하다: 남의 코드를 실제로 실행한다.
    pub execute_workload: bool,
    /// 워크로드가 끝난 뒤 서명된 `AttemptReport` 를 Coordinator 에 보낸다.
    ///
    /// ★ 기본값 `false` 다 — 이 값이 꺼져 있으면 이 조각 **이전과
    ///   바이트 단위로 같게** 동작한다(기존 시나리오 회귀 없음).
    ///
    /// ★ 켰는데 **관측된 종료가 없으면 오류로 끝난다.** 워크로드를
    ///   실행하지 않았거나(`--i-understand-this-executes-untrusted-code`
    ///   가 꺼져 있음) Grant 에 Manifest 가 없었다면 보고할 사실이 없다 —
    ///   그때 빈 보고를 만들어 보내면 그것이 곧 지어낸 값이다.
    pub send_attempt_report: bool,
    /// ACK 뒤에 보낼 `NodeHeartbeat` 개수. 0 이면 안 보낸다.
    pub heartbeat_rounds: u32,
    /// heartbeat 뒤에 보낼 `NeighborUnreachableReport` 개수. 0 이면 안 보낸다.
    ///
    /// ★ 이건 **관측 보고**다 — "저 노드에 연락이 안 된다" 를 서명해 알릴
    ///   뿐, "죽었다" 고 판정하지 않는다(`ADR-033` §7).
    pub neighbor_report_rounds: u32,
    /// 이웃 신고 회차 사이 간격(밀리초).
    ///
    /// ★ heartbeat 와 같은 이유로 필요하다 — 간격이 없으면 두 회차가 같은
    ///   밀리초에 나가고, 저장소가 두 번째를 "진행 없음" 으로 **옳게**
    ///   판정한다(`DoD-60` 이 실측으로 배운 것).
    pub neighbor_report_interval_ms: u64,
    /// 연락이 안 된다고 신고할 대상 노드.
    ///
    /// ★ `neighbor_report_rounds > 0` 인데 이 값이 없으면 시작하지 않는다 —
    ///   대상 없는 신고는 만들 수 없다.
    pub neighbor_report_target_node_id: Option<String>,
    /// 테스트 전용 — 신고의 수신 Coordinator 를 일부러 틀리게 적는다.
    pub corrupt_neighbor_report_coordinator: bool,
    /// heartbeat 회차 사이에 둘 간격(밀리초).
    ///
    /// ★ 2026-08-30 selftest 가 실제를 잡아 생겼다. 간격이 없으면 두
    ///   회차가 **같은 밀리초**에 나가고, 그러면 저장소가 두 번째를
    ///   "진행 없음" 으로 옳게 판정한다 — 코드는 맞는데 테스트의 전제가
    ///   현실과 달랐던 것이다. 실제 시스템은 초 단위로 보낸다.
    pub heartbeat_interval_ms: u64,
    /// 다중 Agent lane 을 쓴다. Hello 를 먼저 보내고 Grant 를 받는다.
    pub multi_agent: bool,
    /// ★ 테스트 전용 — heartbeat 의 `fence_epoch` 을 보유 Lease 와
    ///   다르게 보낸다. Coordinator 가 그 대조를 실제로 하는지
    ///   확인하기 위해서다 — 대조를 지우고도 통과하는 검사는
    ///   아무것도 증명하지 않는다(2026-08-29 실제로 그랬다).
    pub corrupt_heartbeat_fence: bool,
    /// ★ 테스트 전용 — heartbeat 의 `coordinator_device_id` 를 다른
    ///   Coordinator 것으로 보낸다. 그 대조가 실제로 걸리는지
    ///   확인하기 위해서다 — 지워도 통과하는 검사는 아무것도
    ///   증명하지 않는다(2026-08-29 독립 검수 지적).
    pub corrupt_heartbeat_coordinator: bool,
    /// ★ 테스트 전용 — heartbeat 의 `device_id` 를 다른 Agent 것으로
    ///   바꿔 보낸다.
    ///
    ///   2026-08-30 독립 검수 지적으로 생겼다. Coordinator 는
    ///   `device_id` 와 `coordinator_device_id` **둘 다** 대조하는데,
    ///   시나리오 89 는 뒤엣것만 손상시켰다 — 앞엣것 대조를 지워도
    ///   아무 테스트도 실패하지 않았다. 검사가 있다는 것과 그 검사가
    ///   지켜지는 것을 증명하는 것은 다르다.
    pub corrupt_heartbeat_device: bool,
    /// ★ 테스트 전용 — 다중 Agent Hello 의 `mode` 를 Resume lane 값으로
    ///   바꿔 보낸다.
    ///
    ///   2026-08-30 독립 검수 지적. Coordinator 가 `mode` 를 아예 안 봐서,
    ///   등록된 Agent 가 Resume lane 용으로 서명한 Hello 를 보내도 다중
    ///   Agent Grant 를 받았다 — 서명은 유효하므로 서명 검증은 이것을
    ///   절대 못 잡는다.
    pub corrupt_hello_mode: bool,
    /// Linux 에서 작업 cgroup 을 만들 부모(위임받은 subtree).
    ///
    /// ★ 보통의 배포 환경에서는 Agent 자신이 자기 cgroup 안에 있어서
    ///   거기에는 컨트롤러를 위임할 수 없다. 운영자가 Agent 몫으로
    ///   위임한 subtree 를 여기 지정한다. 없으면 "내 cgroup" 을 쓰고,
    ///   위임이 없으면 실행을 거부한다 — 상한 없이 띄우지 않는다.
    pub workload_cgroup_parent: Option<std::path::PathBuf>,
    /// 실행에 걸 Job Object 커밋 상한(바이트). 0 이면 실행하지 않는다.
    pub workload_commit_limit_bytes: u64,
    /// Owner Panel 이 쓸 상태. Agent 가 작업을 시작하면 여기 등록하고
    /// 끝나면 뺀다.
    ///
    /// ★ `Option` 이 아니다. 패널을 안 띄우더라도 상태는 항상 갱신한다 —
    ///   나중에 패널이 붙었을 때 이미 도는 작업이 안 보이는 일이 없도록.
    pub owner_panel_state: owner_panel::OwnerPanelState,
    /// Owner Panel 을 띄울 포트. `None` 이면 안 띄운다.
    ///
    /// ★ 주소는 받지 않는다 — `127.0.0.1` 고정이다(`CLAUDE.md` §0.1).
    ///   0 을 주면 OS 가 포트를 고르고, 실제 주소를 표준 출력에 찍는다.
    pub owner_panel_port: Option<u16>,
    pub coordinator_device_id: String,
    pub agent_device_id: String,
    /// ★ 테스트 전용 — `crates/coordinator/src/lib.rs::CoordinatorConfig::corrupt_own_signature`
    ///   와 대칭. 서명 직후 `agent_signature` 의 마지막 바이트를 뒤집어
    ///   전송한다 — "위조된 ACK 가 도착했을 때 Coordinator 가 실제로
    ///   거부하는가" 를 프로세스 경계에서 확인한다.
    pub corrupt_own_signature: bool,
    /// ★ 테스트 전용 — 첫 ACK 를 보낸 뒤 같은 연결에서 프레임을 하나 더
    ///   읽는다. Coordinator 가 같은 Grant wire bytes 를 두 번 보낸
    ///   경우(`CoordinatorConfig::send_grant_twice`), 이 두 번째 읽기는
    ///   그 replay 된 Grant 다 — `verify()` 가 `Duplicate` 를 만나면
    ///   `Err` 를 직접 반환한다(`crates/protocol/src/signing.rs:826`).
    ///   여기서 `Err` 가 나오는 것이 **기대한 결과**다.
    pub expect_replay: bool,

    // ── Lease 갱신 (2026-08-19, `docs/plans/2026-08-19_0500_...`) ──
    /// ACK 를 보낸 뒤 같은 연결에 이어서 `RenewLeaseRequest` 를 보내고
    /// `RenewLeaseResult` 를 기다린다. `false` 면 이 단계를 건너뛴다
    /// (기존 핸드셰이크 전용 시나리오와 완전히 같게 동작).
    pub do_renew: bool,
    /// ★ 테스트 전용 — `RenewLeaseRequest.node_signature` 의 마지막
    ///   바이트를 뒤집는다. Coordinator 가 ingress 단계에서 거부해야
    ///   한다(`CoordinatorConfig::corrupt_own_signature` 와 대칭).
    pub corrupt_renew_request_signature: bool,
    /// ★ 테스트 전용 — `RenewLeaseRequest.fence_epoch` 을 보유 중인
    ///   Lease 의 실제 epoch 대신 이 값으로 채운다. Coordinator 가
    ///   자신이 기억하는 epoch(`CoordinatorConfig::fence_epoch`)와
    ///   대조해 거부해야 한다.
    pub renew_request_epoch_override: Option<u64>,

    // ── durable FenceWatermark (2026-08-19, `docs/plans/2026-08-19_2200_...`) ──
    /// `FenceWatermark` 를 SQLite 파일에 영속한다 — 재시작을 넘어
    /// epoch 강등 방어를 유지한다(`docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md`).
    /// 최초 Grant 의 Lease 검증과 Lease 갱신 검증 **둘 다** 같은 파일을
    /// 쓴다. 안 주면(selftest 기본 경로) 매 프로세스마다 고유한 임시
    /// 파일을 새로 만든다 — 기존 시나리오와 동일하게 항상 빈
    /// watermark 에서 시작한다.
    pub fence_db_path: PathBuf,

    /// Job 시작 `WRITING` 마커를 기록할 checkpoint root.
    pub checkpoint_root: PathBuf,

    // ── 반복 Lease 갱신 (2026-08-19, `docs/plans/2026-08-19_2330_...`) ──
    /// 같은 연결에서 `RenewLeaseRequest`/`RenewLeaseResult` 왕복을 이
    /// 횟수만큼 반복한다. `do_renew == false` 면 무시된다. 기본값 1은
    /// 기존(단일 왕복) 시나리오와 완전히 같게 동작한다.
    pub renew_rounds: u32,
    /// Test-only delay before constructing each renewal request. This makes
    /// short-TTL expiry deterministic in the process-boundary selftest.
    pub renew_delay_ms: u64,
    /// B+E 구현 단계 5b — 워크로드가 도는 **동안** 이 간격(밀리초)마다 새 연결(RENEW 세션)로 Lease 를 갱신한다.
    /// 0 이면 끈다(기본값). 켜려면 Coordinator 가 `--lease-db` 로 RENEW 세션을 받아야 한다.
    pub renew_during_execution_ms: u64,

    // ── Lease revoke (2026-08-19) ───────────────────────────────────
    /// 이 회차가 끝난 뒤 Coordinator가 보내는 revoke frame을 기다린다.
    /// `Some(0)`은 Grant/ACK 직후다. 현재 stub에는 비동기 이벤트
    /// multiplexing이 없으므로 순차 selftest 경로가 명시적으로 지정한다.
    pub expect_revoke_after_round: Option<u32>,
    /// ★ 테스트 전용 — V-08(`signer_id() == lease_id`) 때문에 잘못된
    ///   target lease_id에도 검증 키를 등록해 identity 검증까지 도달한다.
    pub revoke_signer_id_override: Option<String>,
    /// Bounded reconnect controls. Defaults preserve the production policy;
    /// selftests may shorten them to make exhaustion deterministic.
    pub max_reconnect_attempts: u32,
    pub max_reconnect_duration_seconds: u64,
    pub retry_base_ms: u64,
    pub retry_cap_ms: u64,
    pub connection_attempt: u32,
    pub reconnect_enabled: bool,
    /// Local opt-in for ambiguous renewal recovery.  This flag alone is not
    /// authority: the recovery connection must also carry a signed v2 Grant
    /// with `lease_from_durable_store == true`.
    pub recover_ambiguous_renew_from_durable_lease: bool,
    /// Test-only: close this Agent's first connection immediately after the
    /// first renewal request is flushed, before reading its result.
    pub drop_after_renew_request_once: bool,
    /// Test-only mutation of nonce derivation: reuse connection attempt zero
    /// after reconnect so the Coordinator's replay guard must reject it.
    pub reuse_renew_nonce_after_reconnect: bool,
    /// Explicit opt-in Resume lane; false preserves Grant-first behavior.
    pub resume_protocol: bool,
    pub session_id: String,
    pub resume_lease_id: String,
    pub resume_job_id: String,
    pub resume_attempt_id: String,
    pub resume_fence_epoch: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NeighborReportLane {
    /// `run()` 이 직접 도는 경로 — **유일하게 신고를 보낸다.**
    Sequential,
    /// `multi_agent::run_multi_agent_session()` — 송신 루프가 없다.
    MultiAgent,
}

/// 설정만 보고 lane 을 고른다 — 아직 분기하지 않은 자리에서만 쓴다.
pub(crate) fn lane_from_config(config: &AgentConfig) -> NeighborReportLane {
    if config.multi_agent {
        NeighborReportLane::MultiAgent
    } else {
        NeighborReportLane::Sequential
    }
}

pub(crate) fn unsupported_neighbor_report_lane(
    config: &AgentConfig,
    lane: NeighborReportLane,
) -> Option<String> {
    if config.neighbor_report_rounds == 0 {
        return None;
    }
    if lane == NeighborReportLane::MultiAgent {
        return Some(
            "NEIGHBOR_REPORT_REFUSED: multi-agent lane 은 이웃 신고를 보내지 않는다 — --neighbor-report-rounds 와 함께 쓸 수 없다"
                .to_string(),
        );
    }
    if config.resume_protocol {
        return Some(
            "NEIGHBOR_REPORT_REFUSED: resume 경로는 이웃 신고 송신에 닿기 전에 반환한다 — --neighbor-report-rounds 와 --resume-protocol 을 함께 줄 수 없다"
                .to_string(),
        );
    }
    None
}

/// 이 Agent 를 실행한다 — lane 선택도 여기서 한다.
///
/// ★ 기본은 정상 handshake 한 번이지만, `multi_agent` 가 켜져 있으면
///   그 lane 으로 분기한다. 아래 설명은 기본 lane 의 것이다.
///
/// Coordinator 에 연결해 `ExecutionGrant` 를 받아 검증하고, 서명된
/// `AgentGrantAck` 를 돌려준다. 성공하면 `stdout` 에
/// `JOB_STARTED ... state=WRITING` 및 `RESULT ok=true ...` 를 찍고
/// `Ok(())`, 실패하면 그 이유를 담아
/// `Err` 를 반환한다(호출자가 exit code 로 매핑).
///
/// `RESULT ok=true` 는 Job 완료가 아니다. 이 stub은 entrypoint를 실행하지
/// 않으며, 시작 디렉터리에는 데이터 파일과 `manifest.json`도 없으므로
/// 이 마커만으로 resume 후보나 `COMMITTED` 근거를 만들 수 없다.
/// 이웃 신고 옵션이 **이 lane 에서 실제로 동작하는가**를 본다.
///
/// ★ 관문을 CLI 진입점에만 두면 라이브러리 호출자가 지나쳐 간다(독립 검수
///   6라운드 지적). 그래서 **각 lane 이 실제로 시작하는 자리**마다 둔다.
///
/// ★ 구현하지 않은 조합은 거부한다 — 받아 놓고 안 하는 것이 가장 나쁘다.
///   운영자는 신고가 모이는 줄 안다.
pub fn run(config: AgentConfig) -> Result<(), String> {
    // ★ 결함 97 (재검수 60) — 실행 중 갱신(RENEW 세션)은 FRESH 연결이 ACK 뒤 닫히는 구성에서만 성립한다. 순차 Coordinator 는 한
    //   연결을 끝내야 다음 연결을 받으므로, FRESH 연결을 붙잡는 설정과 함께 켜면 RENEW 가 처리되지 않아 갱신 시한을 넘긴다.
    //   구성 오류는 연결하기 전에 드러낸다. 종료 보고와 함께 쓰려면 REPORT 세션(다음 단계)이 먼저다.
    if config.renew_during_execution_ms > 0 {
        let holding = [
            (config.send_attempt_report, "--send-attempt-report"),
            (config.heartbeat_rounds > 0, "--heartbeat-rounds"),
            (config.do_renew, "--do-renew"),
            (config.expect_revoke_after_round.is_some(), "--expect-revoke-after-round"),
            (config.neighbor_report_rounds > 0, "--neighbor-report-rounds"),
        ];
        if let Some((_, flag)) = holding.iter().find(|(enabled, _)| *enabled) {
            return Err(format!(
                "RENEW_DURING_EXECUTION_REFUSED: {flag} 는 FRESH 연결을 붙잡아 RENEW 세션이 처리되지 않는다 — \
                 --renew-during-execution-ms 와 함께 켤 수 없다(결함 97)"
            ));
        }
    }
    // ★ **신고 대상은 연결하기 전에 확인한다**(독립 검수 4라운드 지적).
    //
    //   전에는 이 검사가 송신 루프 안에 있었다 — Grant/ACK 를 다 마치고
    //   Lease 를 받아들인 뒤에야 "대상이 없다" 로 죽었다. 구성 오류는
    //   아무것도 시작하기 전에 드러나야 한다.
    //
    //   빈 문자열·공백도 막는다. 그것들은 저장소가 `InvalidInput` 으로
    //   거부하지만, 여기까지 오면 이미 왕복을 한 뒤다.
    if config.neighbor_report_rounds > 0 {
        match config.neighbor_report_target_node_id.as_deref() {
            None => {
                return Err(
                    "NEIGHBOR_REPORT_REFUSED: --neighbor-report-rounds 를 켰으면                      --neighbor-report-target 이 있어야 한다"
                        .to_string(),
                )
            }
            Some(target) if target.trim().is_empty() => {
                return Err(
                    "NEIGHBOR_REPORT_REFUSED: --neighbor-report-target 이 비어 있다".to_string(),
                )
            }
            Some(_) => {}
        }
    }
    // ★ **multi-agent lane 은 이웃 신고를 보내지 않는다**(독립 검수 5라운드
    //   지적). 그 lane 은 `Hello -> Grant -> ACK` 을 하고 끝나므로(10라운드
    //   정정 — 전에 "Grant/ACK 만" 이라 썼다), 신고 옵션을 줘도 한 건도
    //   안 보내고 **성공으로 끝난다** — 조용한 무시다.
    if let Some(message) = unsupported_neighbor_report_lane(&config, lane_from_config(&config)) {
        return Err(message);
    }
    if config.multi_agent {
        return multi_agent::run_multi_agent_session(&config);
    }
    let policy = RetryPolicy {
        max_attempts: config.max_reconnect_attempts,
        max_duration: Duration::from_secs(config.max_reconnect_duration_seconds),
        base_delay: Duration::from_millis(config.retry_base_ms),
        cap_delay: Duration::from_millis(config.retry_cap_ms),
        connect_timeout: Duration::from_secs(3),
        safety_margin_ms: 1_000,
    };
    // ★ 실행을 켰으면 소유자 패널이 **반드시** 있어야 한다
    //   (2026-08-29, 독립 검수 지적).
    //
    //   전에는 `--owner-panel-port` 를 안 주면 패널 없이 그냥 실행됐다.
    //   그러면 남의 코드가 남의 PC 에서 도는데 소유자는 그것을 볼
    //   방법도 멈출 방법도 없다 — `CLAUDE.md` §0.1 이 가장 앞에서
    //   요구하는 것이 정확히 그 두 가지다.
    //
    //   "포트를 안 줬으니 안 띄운다" 는 편의였지, 안전한 기본값이
    //   아니었다. 실행이 켜져 있으면 포트를 안 줘도 **0 으로 자동
    //   기동**한다 — OS 가 고른 포트를 표준 출력에 찍으므로 소유자는
    //   거기로 들어가면 된다. 실행이 꺼져 있으면(기본값) 멈출 대상이
    //   없으므로 명시했을 때만 띄운다.
    let panel_port = match (config.owner_panel_port, config.execute_workload) {
        (Some(port), _) => Some(port),
        // 실행하는데 포트를 안 줬다 — 조용히 넘어가지 않고 자동으로 연다.
        (None, true) => Some(0),
        (None, false) => None,
    };

    if let Some(port) = panel_port {
        let token = derive_owner_panel_token(&config.own_seed);
        let panel = owner_panel::OwnerPanel::bind(port, config.owner_panel_state.clone(), token)
            .map_err(|error| {
                format!(
                    "OWNER_PANEL_REFUSED: 소유자 패널을 127.0.0.1:{port} 에 띄우지 못했다 \
                     — 멈출 수 없는 작업을 시작하지 않는다: {error}"
                )
            })?;
        let addr = panel
            .local_addr()
            .map_err(|error| format!("OWNER_PANEL_REFUSED: 주소를 읽지 못했다: {error}"))?;
        println!("OWNER_PANEL listening=http://{addr}/");
        std::thread::Builder::new()
            .name("gputeer-owner-panel".into())
            .spawn(move || panel.serve_forever())
            .map_err(|error| format!("OWNER_PANEL_REFUSED: 스레드 기동 실패: {error}"))?;
    }

    let signing_key = SigningKey::from_bytes(&config.own_seed);
    let mut coordinator_keys = InMemoryKeyring::new();
    coordinator_keys.insert(
        config.coordinator_device_id.clone(),
        config.coordinator_verifying_key,
    );
    let mut replay = InMemoryReplayGuard::new();
    let clock = SystemClock;
    let mut fence_watermark = DurableFenceWatermark::open(&config.fence_db_path)
        .map_err(|e| format!("fence watermark storage open failed: {e}"))?;
    if !fence_watermark.is_durable() {
        return Err(format!(
            "fence watermark 저장소가 영속이 아니다: fence_db_path={:?}",
            config.fence_db_path
        ));
    }
    let mut budget = RetryBudget::new(&policy);
    if !config.reconnect_enabled {
        let stream = connect_with_timeout(&config.coordinator_addr, policy.connect_timeout)
            .map_err(|e| format!("Coordinator connect failed: {e}"))?;
        return run_one_connection(
            config,
            stream,
            false,
            &signing_key,
            &mut coordinator_keys,
            &mut replay,
            &clock,
            &mut fence_watermark,
            &mut budget,
            &policy,
        );
    }

    let mut connection_attempt = 0u32;
    let mut recovering_ambiguous_renew = false;
    for attempt in 0..config.max_reconnect_attempts {
        if attempt > 0 {
            let exponent = (attempt - 1).min(31);
            let raw = policy
                .base_delay
                .checked_mul(1u32 << exponent)
                .unwrap_or(policy.cap_delay)
                .min(policy.cap_delay);
            let jitter = if raw.is_zero() {
                Duration::ZERO
            } else {
                let n = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as u128)
                    .unwrap_or(0);
                Duration::from_nanos((n % raw.as_nanos()) as u64)
            };
            if budget.exhausted() || std::time::Instant::now() + jitter >= budget.deadline {
                return Err(format!("ReconnectExhausted: attempt={attempt}"));
            }
            std::thread::sleep(jitter);
        }

        let connection_result =
            match connect_with_timeout(&config.coordinator_addr, policy.connect_timeout) {
                Ok(stream) => {
                    let mut attempt_config = config.clone();
                    // Only successful TCP connects consume nonce attempts. A
                    // refused/timed-out connect cannot have reached accept().
                    attempt_config.connection_attempt =
                        next_connection_attempt(&mut connection_attempt);
                    run_one_connection(
                        attempt_config,
                        stream,
                        recovering_ambiguous_renew,
                        &signing_key,
                        &mut coordinator_keys,
                        &mut replay,
                        &clock,
                        &mut fence_watermark,
                        &mut budget,
                        &policy,
                    )
                }
                Err(error) => Err(format!("Coordinator connect failed: {error}")),
            };
        match connection_result {
            Ok(()) => return Ok(()),
            Err(error) => match SessionError::from(error) {
                SessionError::Retryable(reason) => {
                    if attempt + 1 >= policy.max_attempts {
                        return Err(format!("ReconnectExhausted: {reason}"));
                    }
                    continue;
                }
                SessionError::Fatal(reason) => return Err(reason),
                SessionError::AmbiguousRenew(reason) => {
                    if !config.recover_ambiguous_renew_from_durable_lease {
                        return Err(format!(
                            "AMBIGUOUS_RENEW_RECOVERY_DISABLED: durable Lease recovery was not enabled: {reason}"
                        ));
                    }
                    if attempt + 1 >= policy.max_attempts {
                        return Err(format!("ReconnectExhausted: {reason}"));
                    }
                    recovering_ambiguous_renew = true;
                    continue;
                }
            },
        }
    }
    Err("ReconnectExhausted: no attempts configured".into())
}

fn run_resume_connection(
    config: &AgentConfig,
    mut stream: TcpStream,
    signing_key: &SigningKey,
    coordinator_keys: &mut InMemoryKeyring,
    replay: &mut InMemoryReplayGuard,
    clock: &SystemClock,
    fence_watermark: &mut DurableFenceWatermark,
    budget: &mut RetryBudget,
    policy: &RetryPolicy,
) -> Result<(), String> {
    if config.session_id.is_empty()
        || config.resume_lease_id.is_empty()
        || config.resume_job_id.is_empty()
        || config.resume_attempt_id.is_empty()
    {
        return Err("resume protocol requires session_id, lease_id, job_id, and attempt_id".into());
    }
    let now = clock.now_unix_ms();
    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        // ★ 리터럴 2 대신 공용 상수를 쓴다(2026-08-30 독립 검수 지적).
        //   다중 Agent lane 만 상수로 옮기고 Resume 경로는 리터럴로
        //   남겨 두면, 두 값이 갈라졌을 때 아무도 모른다.
        mode: gputeer_protocol::constants::MODE_RESUME,
        session_id: config.session_id.clone(),
        node_id: config.agent_device_id.clone(),
        connection_attempt: config.connection_attempt,
        issued_at_unix_ms: now,
        nonce: fresh_nonce()?,
        ..Default::default()
    };
    hello.node_signature = sign(signing_key, &hello).to_vec();
    let hello_frame = write_frame(FrameType::SessionHello, &hello.encode_to_vec())
        .map_err(|e| format!("AgentSessionHello 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&hello_frame)
        .map_err(|e| format!("AgentSessionHello 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;

    let mut request = pb::ResumeLeaseRequest {
        schema_version: 1,
        lease_id: config.resume_lease_id.clone(),
        job_id: config.resume_job_id.clone(),
        attempt_id: config.resume_attempt_id.clone(),
        node_id: config.agent_device_id.clone(),
        fence_epoch: config.resume_fence_epoch,
        session_id: config.session_id.clone(),
        connection_attempt: config.connection_attempt,
        issued_at_unix_ms: clock.now_unix_ms(),
        request_nonce: fresh_nonce()?,
        ..Default::default()
    };
    request.node_signature = sign(signing_key, &request).to_vec();
    let request_frame = write_frame(FrameType::LeaseResume, &request.encode_to_vec())
        .map_err(|e| format!("ResumeLeaseRequest 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&request_frame)
        .map_err(|e| format!("ResumeLeaseRequest 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;

    let result_message = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(coordinator_keys),
        replay,
        clock,
    )
    .map_err(|e| format!("ResumeLeaseResult 프레임 읽기/검증 실패: {e}"))?;
    let result = match result_message {
        IngressMessage::LeaseResumeResult(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("ResumeLeaseResult replay 검사 실패: {e:?}"))?
            .clone(),
        other => return Err(format!("ResumeLeaseResult가 아닌 프레임 수신: {other:?}")),
    };
    if result.request_nonce != request.request_nonce {
        return Err("RESUME_REJECTED: request_nonce가 echo되지 않았다".into());
    }
    if result.coordinator_id != config.coordinator_device_id {
        return Err(format!(
            "RESUME_REJECTED: coordinator_id 불일치 {} != {}",
            config.coordinator_device_id, result.coordinator_id
        ));
    }

    match result.outcome {
        1 => {
            let lease = result
                .lease
                .clone()
                .ok_or_else(|| "RESUME_REJECTED: RESUMED 결과에 Lease가 없다".to_string())?;
            let verified_lease = verify(
                &lease,
                1,
                &Ed25519Verifier::new(&*coordinator_keys),
                clock.now_unix_ms(),
                replay,
            )
            .map_err(|e| format!("RESUME_REJECTED: Lease 서명 검증 실패: {e:?}"))?;
            if verified_lease.get().lease_id != request.lease_id
                || verified_lease.get().job_id != request.job_id
                || verified_lease.get().attempt_id != request.attempt_id
                || verified_lease.get().holder_node_id != config.agent_device_id
                || verified_lease.get().fence_epoch != request.fence_epoch
            {
                return Err("RESUME_REJECTED: returned Lease identity/epoch mismatch".into());
            }

            // ★ Resume 경로도 durable fence watermark 를 거친다.
            //
            //   위의 identity/epoch 검사는 "돌아온 Lease 가 **내가
            //   요청한** epoch 인가" 만 본다. 그 요청값은 CLI
            //   설정(`--resume-fence-epoch`)에서 온다 — 이 Agent 가
            //   과거에 이미 더 높은 epoch 를 봤는지와 무관하다.
            //
            //   Coordinator 쪽 `classify_resume()` 가 낮은 epoch 를
            //   `SUPERSEDED` 로 거부하긴 하지만, 그건 **Coordinator 의
            //   저장소가 온전할 때만** 유효한 방어다. 이
            //   저장소가 오래된 백업으로 되돌려지면 더 낮은 epoch 를
            //   정상으로 서명해 `RESUMED` 로 돌려준다. Agent 가
            //   그걸 그대로 받아들이면 이미 상위 epoch 를 가진
            //   다른 보유자와 동시에 살아있게 된다.
            //
            //   `CLAUDE.md` §0.2 — "Coordinator 가 보낸 값이라고
            //   신뢰하지 않는다." Grant 경로와 갱신 경로는 이미
            //   같은 검사를 하고 있었고, Resume 경로만 빠져 있었다.
            //
            //   Resume 은 같은 세대를 이어받는 것이므로 같은 epoch 가
            //   정상이다. `check_and_advance()` 는 같은 값을 통과시키고
            //   낮은 값만 거부한다(`durable_lease_scope.rs`).
            fence_watermark
                .check_and_advance(
                    &verified_lease.get().job_id,
                    verified_lease.get().fence_epoch,
                )
                .map_err(|e| fence_error_message("RESUME_REJECTED", e))?;

            budget.update_lease_deadline(
                verified_lease.get().expires_at_unix_ms,
                clock.now_unix_ms(),
                policy,
            );
            println!(
                "RESUME_RESULT ok=true outcome=1 lease_id={} connection_attempt={}",
                request.lease_id, request.connection_attempt
            );
            println!(
                "RESULT ok=true resume_outcome=1 lease_id={}",
                request.lease_id
            );
            Ok(())
        }
        7 => Err(format!(
            "RETRYABLE_RESUME: coordinator unavailable retry_after_ms={} detail={}",
            result.retry_after_ms, result.detail
        )),
        2 => Err("RESUME_REFUSED:REVOKED".into()),
        3 => Err("RESUME_REFUSED:EXPIRED".into()),
        4 => Err("RESUME_REFUSED:SUPERSEDED".into()),
        5 => Err("RESUME_REFUSED:UNKNOWN_LEASE".into()),
        6 => Err("RESUME_REFUSED:IDENTITY_CONFLICT".into()),
        8 => Err("RESUME_REFUSED:EPOCH_AHEAD".into()),
        outcome => Err(format!(
            "RESUME_REJECTED: outcome={} detail={}",
            outcome, result.detail
        )),
    }
}

fn fresh_nonce() -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|e| format!("CSPRNG nonce 생성 실패: {e}"))?;
    Ok(nonce.to_vec())
}

/// ★ 결함 ㊺ (구현 검수 49) — 이 연결에서 워크로드를 띄웠으면 **어떤 오류도** 재접속 · 재조회 사유로
///   돌려보내지 않는다. 바깥 루프(`run()`)는 재접속하면 같은 Grant 로 처음부터 다시 돌고, 그러면
///   워크로드가 두 번 실행된다(`docs/reports/debugs/2026-09-14_1055_재접속이_워크로드를_다시_돌린다.md`).
///   첫 커밋(391e49d)은 peek 경로 하나만 막았고 Revoke 읽기 실패 · 갱신 응답 유실 복구가 남아 있었다.
///   ★ 결함 ㊽ (재검수 50) — 기준은 **실행을 시도했는가**다(두 번째 관문 거부 · 기동 실패도 포함). 기동
///     지금 두 번째 관문 거부 · 기동 실패는 원래도 Fatal 이다(EXEC_REFUSED:* · EXEC_FAILED:SPAWN — 재접속 표지가 없다).
///     이 기준은 **실행 시도 이후에 재시도 가능 오류가 반환되더라도** 재진입을 막으려는 보수적 억제다(결함 56 — 전에는
///     "기동 성공만 세면 그 사이 실패가 재접속으로 이어진다" 고 단정했다). 전에는 "워크로드를 이미 띄웠다" 고 적었다.
#[allow(clippy::too_many_arguments)]
fn run_one_connection(
    config: AgentConfig,
    stream: TcpStream,
    recovering_ambiguous_renew: bool,
    signing_key: &SigningKey,
    coordinator_keys: &mut InMemoryKeyring,
    replay: &mut InMemoryReplayGuard,
    clock: &SystemClock,
    fence_watermark: &mut DurableFenceWatermark,
    budget: &mut RetryBudget,
    policy: &RetryPolicy,
) -> Result<(), String> {
    let mut execution_attempted = false;
    let result = run_one_connection_inner(
        config,
        stream,
        recovering_ambiguous_renew,
        signing_key,
        coordinator_keys,
        replay,
        clock,
        fence_watermark,
        budget,
        policy,
        &mut execution_attempted,
    );
    result.map_err(|error| {
        if execution_attempted {
            not_retried_after_workload(&error)
        } else {
            error
        }
    })
}

/// 재접속 · 재조회 표지를 바꿔 `SessionError::from` 이 Fatal 로 분류하게 한다(결함 ㊺).
fn not_retried_after_workload(error: &str) -> String {
    format!(
        "WORKLOAD_EXECUTION_ATTEMPTED: 이 연결에서 워크로드 실행을 시도했다(기동 성공 여부와 무관) — 재접속하면 같은 Grant 로 \
         다시 실행할 수 있으므로 재시도하지 않는다: {}",
        error
            .replace("RETRYABLE_", "RETRYABLE(억제)_")
            .replace("AMBIGUOUS_RENEW", "AMBIGUOUS(억제)_RENEW")
    )
}

#[allow(clippy::too_many_arguments)]
fn run_one_connection_inner(
    config: AgentConfig,
    mut stream: TcpStream,
    recovering_ambiguous_renew: bool,
    signing_key: &SigningKey,
    coordinator_keys: &mut InMemoryKeyring,
    mut replay: &mut InMemoryReplayGuard,
    clock: &SystemClock,
    mut fence_watermark: &mut DurableFenceWatermark,
    budget: &mut RetryBudget,
    policy: &RetryPolicy,
    execution_attempted: &mut bool,
) -> Result<(), String> {
    // ★ fail closed — fence watermark 저장소를 **네트워크 연결보다
    //   먼저** 연다. 열 수 없는 저장소로 epoch 를 검증하는 척하지
    //   않는다(`docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md`).
    /* let mut fence_watermark = DurableFenceWatermark::open(&config.fence_db_path)
        .map_err(|e| format!("fence watermark 저장소 열기 실패: {e}"))?;

    // ★ 코덱스 독립 검수(2026-08-19, p102) 지적 — `--fence-db :memory:`
    //   같은 비영속 경로를 아무 검사 없이 받아들이고 있었다.
    //   `is_durable() == false` 인 채로 계속 진행하면, 이 조각 전체의
    //   목적(재시작을 넘는 방어)이 조용히 거짓이 된다 — 겉으로는
    //   `DurableFenceWatermark` 를 쓰는 것처럼 보이지만 실제로는
    //   재시작 한 번에 사라진다. 열 수 없는 경우와 같은 이유로
    //   fail closed 한다.
    if !fence_watermark.is_durable() {
        return Err(format!(
            "fence watermark 저장소가 영속이 아니다(fence_db_path={:?}) — \
             재시작을 넘는 epoch 강등 방어가 조용히 무력화된다",
            config.fence_db_path
        ));
    }

    */
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;

    if config.resume_protocol {
        return run_resume_connection(
            &config,
            stream,
            signing_key,
            coordinator_keys,
            replay,
            clock,
            fence_watermark,
            budget,
            policy,
        );
    }

    // ★ D2 (B+E 구현 단계 4, 2026-09-14) — **모든 연결은 Agent 의 Hello 로 시작한다.** 순차 lane 도 다중 Agent lane 과
    //   같은 Hello(FRESH) -> Grant 순서다. 한 리스너가 FRESH · RESUME · RENEW · REPORT 를 가르려면 Agent 가 먼저 말해야 한다
    //   (제안서 결정 D2 — 설정 분기 없음). 옛 Coordinator 는 Grant 를 먼저 쓰고 ACK 자리에서 이 Hello 를 받아 명시적으로 실패한다.
    {
        let mut hello = pb::AgentSessionHello {
            schema_version: 1,
            mode: gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT,
            session_id: config.session_id.clone(),
            node_id: config.agent_device_id.clone(),
            connection_attempt: config.connection_attempt,
            issued_at_unix_ms: clock.now_unix_ms(),
            nonce: fresh_nonce()?,
            ..Default::default()
        };
        hello.node_signature = sign(&signing_key, &hello).to_vec();
        let frame = write_frame(FrameType::SessionHello, &hello.encode_to_vec())
            .map_err(|e| format!("AgentSessionHello(FRESH) 프레임 인코딩 실패: {e}"))?;
        stream
            .write_all(&frame)
            .map_err(|e| format!("AgentSessionHello(FRESH) 전송 실패: {e}"))?;
        stream.flush().map_err(|e| e.to_string())?;
    }

    let received = read_frame(
        &mut stream,
        2,
        KeyDirectorySource::Provided(coordinator_keys),
        replay,
        clock,
    )
    .map_err(|e| classify_framing_error(e, "Grant"))?;

    // ★ Grant 를 replay 검사까지 통과한 뒤에만 그 내용으로 ACK 를
    //   만든다 — 부작용(ACK 발급)이 검증되지 않은 값에서 나오지 않는다.
    let grant: pb::ExecutionGrant = match &received {
        IngressMessage::Grant(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("Grant replay 검사 실패: {e:?}"))?
            .clone(),
        other => return Err(format!("예상하지 못한 요청 타입: {other:?}")),
    };
    // The local recovery flag only expresses operator intent.  A legacy
    // Coordinator can mint a fresh in-memory Lease on reconnect, so recovery
    // is safe only when the verified Grant itself attests that issue_lease()
    // resolved the Lease through the durable store.  Missing v2 fields decode
    // as false and therefore fail closed before ACK/checkpoint/new Renew.
    if recovering_ambiguous_renew && !grant.lease_from_durable_store {
        return Err(
            "DURABLE_LEASE_RECOVERY_REFUSED: signed ExecutionGrant does not attest a durable Lease store"
                .into(),
        );
    }
    if grant.nonce != derive_nonce("grant", &grant.grant_id, config.connection_attempt) {
        return Err("GRANT_REJECTED: nonce does not match connection attempt".into());
    }

    // ★ `Lease` 는 outer `Grant` 와 **별도로 서명된** 메시지다(§6 규칙
    //   i — 중첩 메시지는 각자 서명된다). outer Grant 서명이 유효해도
    //   nested Lease 서명이 위조됐을 수 있으므로 반드시 독립적으로
    //   검증한다 — `to_fields.rs` 의 "manifest 와 lease 는 각자
    //   독립적으로 검증해야 한다(MUST)" 를 실제로 이행한다
    //   (2026-08-18, 코덱스 설계 · `p67` 프롬프트).
    let mut held_lease = verify_and_record_lease(
        &grant,
        &config,
        &coordinator_keys,
        clock.now_unix_ms(),
        &mut replay,
        &mut fence_watermark,
    )?;
    budget.update_lease_deadline(held_lease.expires_at_unix_ms, clock.now_unix_ms(), policy);
    println!(
        "LEASE_ACCEPTED connection_attempt={} lease_id={} issued_at_unix_ms={} expires_at_unix_ms={}",
        config.connection_attempt,
        held_lease.lease_id,
        held_lease.issued_at_unix_ms,
        held_lease.expires_at_unix_ms
    );

    // ★ nested `JobManifest` 도 `Lease` 와 **똑같이** 독립 검증한다.
    //   outer Grant 서명이 유효해도 nested Manifest 서명은 위조됐을 수
    //   있다(`to_fields.rs` — "manifest 와 lease 는 각자 독립적으로
    //   검증해야 한다(MUST)").
    //
    //   ACK·checkpoint **전에** 한다 — 실행 지시를 신뢰할 수 없으면
    //   시작 사실조차 남기지 않는다.
    let workload = verify_nested_manifest(&grant, &config, clock.now_unix_ms(), &mut replay)?;
    if let Some(spec) = workload.as_ref().map(|w| &w.spec) {
        println!(
            "MANIFEST_ACCEPTED job_id={} entrypoint={} args={} env_vars={}",
            spec.job_id,
            spec.entrypoint,
            spec.args.len(),
            spec.env_vars.len()
        );
    }

    // Grant/Lease 검증을 모두 통과한 뒤, ACK를 만들거나 보내기 전에
    // 시작 사실을 durable artifact로 남긴다. 디렉터리 생성 또는
    // record_initial_state()가 실패하면 여기서 fail-closed하여 ACK를
    // 보내지 않는다. record_initial_state()는 내부 write_once()의
    // 동일 내용 Ok(false) 멱등 동작을 그대로 상속한다.
    let checkpoint_id = record_start_checkpoint(
        &config.checkpoint_root,
        &held_lease.job_id,
        &grant.attempt_id,
        &grant.grant_id,
    )?;
    println!(
        "JOB_STARTED checkpoint_id={} job_id={} attempt_id={} grant_id={} state=WRITING",
        checkpoint_id, held_lease.job_id, grant.attempt_id, grant.grant_id
    );

    // ★ 실제 실행은 **시작 마커 뒤**에 온다.
    //
    //   순서를 바꾸면 실행 중 이 프로세스가 죽었을 때 남은 것이
    //   아무것도 없다 — 남의 PC 에서 남의 코드를 돌렸는데 그
    //   사실을 기록한 데가 없는 상태다. 마커가 먼저 있어야 부팅 시
    //   `startup_gc()` 가 그 PARTIAL 디렉터리를 보고 정리한다(`DoD-33`).

    // ★ 관측한 종료를 **여기서 보내지 않는다.** 이 지점은 아직 ACK 전이고,
    //   Coordinator 는 ACK 를 먼저 기다린다 — 여기서 보내면 프레임 순서가
    //   어긋나 Coordinator 가 `AttemptReport` 를 ACK 로 읽으려다 실패한다.
    //   사실만 들고 있다가 ACK·heartbeat·이웃 신고 뒤에 보낸다.
    let mut terminal_observation: Option<crate::report::TerminalObservation> = None;
    // ★ 결함 82 — 작업 디렉터리 삭제 실패. 종료 보고를 보낸 **뒤에** 오류로 알린다.
    let mut pending_cleanup_failure: Option<String> = None;
    // ★ 결함 ⑱ — 워크로드가 있으면 ACK 는 실행 **전**에 간다(아래). 보낸 것을 여기 담는다.
    let mut sent_ack: Option<pb::AgentGrantAck> = None;
    // ★ `execution_attempted`(인자) — 이 연결에서 워크로드를 띄우려 했는가. 바깥 `run_one_connection` 이
    //   그 뒤의 **모든** 오류를 재접속 불가로 바꾼다(결함 ㊺).
    if let Some(loaded) = workload.as_ref() {
        let spec = &loaded.spec;
        // 자식의 출력을 받을 별도 작업 디렉터리.
        //
        // ★ **체크포인트 루트 안에 두지 않는다.** 두 가지 이유다.
        //   1) 그 네임스페이스는 `write_once()` 가 소유한다(`DoD-21`) —
        //      남의 프로세스가 직접 쓰게 하면 그 계약이 깨진다.
        //   2) `startup_gc()` 는 루트 밑 모든 디렉터리를 체크포인트로
        //      보고 `gc_partial()` 을 돌린다. 지금은 그 함수가 파일만
        //      지워서 `.run` 이 **우연히** 살아남는데, 우연에 기대는
        //      설계를 두지 않는다.
        //
        //   루트의 **형제 디렉터리**를 쓴다 — 같은 볼륨·같은 권한이라
        //   새 설정 없이 동작하고, GC 가 스캔하는 범위 밖이다.
        let run_root = workload_run_root(&config.checkpoint_root)?;
        let run_dir = run_root.join(&checkpoint_id);
        // 이전 실행이 죽으면서 남긴 것이 있으면 먼저 치운다 —
        // 남은 `stdout.log` 에 자식이 이어서 쓰면 지난번 출력과
        // 섞인다.
        remove_dir_if_present(&run_dir)?;
        fs::create_dir_all(&run_dir).map_err(|error| {
            format!("작업 출력 디렉터리 생성 실패({run_dir:?}): {error}")
        })?;

        // ★★ **GPU 요구는 Coordinator 가 확정해 내려준 것만 쓴다**
        //   (2026-09-07 신설).
        //
        //   `proto/job.proto` 가 `GrantedExecutionPlan` 에 적어 뒀다 —
        //   "Agent 가 스스로 계산하지 않는다(노드마다 다른 결론이 나오면
        //   안 되므로)". 그래서 여기서 추론하지 않고 **서명된 Grant 의
        //   `plan.assigned_gpu_uuids` 를 그대로** 쓴다.
        //
        //   ★ 비어 있으면 `None` 이다 — 확인하지 않는다. 이유 둘:
        //     1. GPU 를 안 쓰는 Job 까지 NVML 을 요구하면 NVIDIA 없는
        //        노드가 CPU 작업조차 못 받는다
        //     2. **오늘 Coordinator 는 이 칸을 채우지 않는다.**
        //        `issue_grant.rs:40`·`grant_from_stored.rs:33` 이 "안
        //        한다" 고 명시했다(GPU scope 의 authoritative provenance
        //        가 없어서다). 즉 이 관문은 **자리만 잡은 상태**이고,
        //        그 칸이 채워지는 순간부터 실제로 돈다.
        //
        //   ★ 최소 VRAM 은 **요구하지 않는다**(0). Grant 에 그 값을 담을
        //     칸이 아직 없기 때문이다. 없는 값을 지어내지 않는다
        //     (`CLAUDE.md` §1). 칸이 생기면 그때 채운다.
        let gpu_requirements = grant
            .plan
            .as_ref()
            .filter(|plan| !plan.assigned_gpu_uuids.is_empty())
            .map(|plan| gputeer_runtime_nvml::preflight::GpuRequirements {
                required_gpu_count: plan.assigned_gpu_uuids.len() as u32,
                minimum_free_vram_bytes_per_gpu: 0,
                selected_gpu_uuids: plan.assigned_gpu_uuids.clone(),
            });

        let policy = exec::ExecutionPolicy {
            opted_in: config.execute_workload,
            commit_limit_bytes: config.workload_commit_limit_bytes,
            gpu_requirements,
            capture_dir: Some(run_dir.clone()),
            // ★ attempt 별로 갈라야 한다 — 같은 Job 의 두 attempt 가 같은
            //   격리 이름을 받으면 하나를 멈출 때 다른 하나도 죽는다.
            isolation: exec::IsolationIdentity {
                grant_id: grant.grant_id.clone(),
                attempt_id: grant.attempt_id.clone(),
            },
            cgroup_parent: config.workload_cgroup_parent.clone(),
        };
        // ★★ 결함 ⑱ (설계 A, 2026-09-14) — **사전 관문을 보고 ACK 를 실행 전에 보낸다.**
        //   전에는 워크로드를 끝까지 돌린 뒤에 ACK 를 보내, 10초보다 긴 작업이면 Coordinator 가
        //   ACK 읽기 시한에 먼저 걸렸다(실측 `docs/evidence/_raw/결함18_ACK_시한_실측_2026-09-10.txt`).
        //   **이 첫 검사**가 거부하면 지금처럼 ACK 없이 끝낸다 — 받아들이지 못할 Grant 에 "받았다" 고
        //   답하지 않는다. opt-in 이 꺼져 있으면 실행만 건너뛰고 ACK 는 보낸다(전과 같다).
        let will_execute = match exec::preflight(&policy) {
            Ok(()) => true,
            Err(exec::ExecutionError::NotOptedIn) => false,
            // ★ 결함 ㊸ — 정리까지 실패해도 거부 사유를 잃지 않는다.
            Err(refused) => return Err(fail_after_cleanup(refused.to_string(), &run_dir)),
        };
        // ★ 결함 ㊷ — ACK 전송이 실패해도 작업 디렉터리를 치운 뒤 보고한다.
        sent_ack = Some(
            send_grant_ack(&mut stream, signing_key, &grant, &config, clock)
                .map_err(|ack_error| fail_after_cleanup(ack_error, &run_dir))?,
        );
        *execution_attempted = will_execute;
        // ★ 실행부터 산출물 확정까지를 한 덩어리로 묶고, 그 **밖에서**
        //   작업 디렉터리를 지운다.
        //
        //   초안은 사이사이에 `?` 를 둔 평범한 직선 코드였는데,
        //   중간에서 실패하면 삭제에 **도달하지 못해** 제출자의
        //   stdout 이 남의 PC 에 남았다(2026-08-29, 독립 검수 지적).
        //   `CLAUDE.md` §0.5 는 성공했을 때만 치우라고 하지 않는다 —
        //   오히려 실패했을 때 남는 것이 더 위험하다.
        // ★ B+E 구현 단계 5b — 실행하는 동안만 새 연결로 갱신한다(설정으로 켤 때만).
        let renewer = if will_execute {
            start_renew_during_execution(&config, signing_key, &held_lease)
        } else {
            None
        };
        let outcome = run_and_capture_workload(
            spec,
            policy,
            &config.checkpoint_root,
            &checkpoint_id,
            &held_lease,
            &grant.attempt_id,
            &run_dir,
            &config.owner_panel_state,
            &loaded.submitter_device_id,
            clock.now_unix_ms(),
            clock,
        );
        if let Some(renewer) = renewer {
            renewer.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            match renewer.handle.join() {
                // 스레드가 검증한 최신 Lease — 세대는 올라가지 않는다(상승은 verify_renew_result 가 거부한다).
                Ok(renewed) => held_lease = renewed,
                Err(_) => println!("RENEW_SESSION_THREAD_PANICKED — 갱신 스레드가 비정상 종료했다(보유 Lease 는 실행 전 것)"),
            }
        }
        // 삭제는 성공·실패 관계없이 한다. 두 오류가 동시에 나면
        // 둘 다 보고한다 — 한쪽을 묵으면 진짜 원인을 놓친다.
        let cleanup = remove_dir_if_present(&run_dir);
        let (outcome, cleanup_failure) = merge_run_and_cleanup(outcome, cleanup)?;
        if let Some(error) = &cleanup_failure {
            println!("WORKLOAD_CLEANUP_FAILED job_id={} detail={error}", spec.job_id);
        }
        pending_cleanup_failure = cleanup_failure;

        match outcome {
            Some(report) => {
                println!(
                    "WORKLOAD_ARTIFACTS checkpoint_id={} files={} bytes={}",
                    checkpoint_id, report.file_count, report.total_bytes
                );
                // ★ 종료 코드 0 과 그 외를 **구분해서** 보고한다.
                //   `state-machines.md` §3 이 WORKLOAD_EXITED_OK 와
                //   WORKLOAD_EXITED_ERROR 를 다른 전이로 두는 이유다.
                //   (전이 자체는 아직 구현하지 않는다.)
                match report.exit.code() {
                    // 결함 ⑲ — 코드 0 이어도 산출물을 확정하지 못했으면 성공이 아니다.
                    Some(0) if report.finalization_failure.is_some() => println!(
                        "WORKLOAD_RESULT ok=false job_id={} exit_code=0 finalization_failed=true",
                        spec.job_id
                    ),
                    Some(0) => println!("WORKLOAD_RESULT ok=true job_id={}", spec.job_id),
                    Some(code) => println!(
                        "WORKLOAD_RESULT ok=false job_id={} exit_code={}",
                        spec.job_id, code
                    ),
                    // 종료는 관측했지만 코드가 없다(신호 종료 · 코드 조회 실패) — 성공으로 세지 않는다(결함 69).
                    None => println!(
                        "WORKLOAD_RESULT ok=false job_id={} exit_code=none",
                        spec.job_id
                    ),
                }
                // ★ **관측한 것만** 담는다. `job_id`·`fence_epoch` 은
                //   Coordinator 가 서명해 준 Lease 에서, `attempt_id` 는
                //   같은 Grant 에서 온다 — 여기서 새로 만들지 않는다.
                //   `issued_at_unix_ms` 는 실제로 보낼 때 읽는다.
                terminal_observation = Some(crate::report::TerminalObservation {
                    job_id: held_lease.job_id.clone(),
                    attempt_id: grant.attempt_id.clone(),
                    node_id: config.agent_device_id.clone(),
                    fence_epoch: held_lease.fence_epoch,
                    exit_code: report.exit.code(),
                    finalization_failure: report.finalization_failure.as_ref().map(|(stage, _)| *stage),
                    started_at_unix_ms: report.started_at_unix_ms,
                    finished_at_unix_ms: report.finished_at_unix_ms,
                    issued_at_unix_ms: 0,
                });
            }
            None => {
                println!(
                    "WORKLOAD_SKIPPED job_id={} reason=not_opted_in",
                    spec.job_id
                );
            }
        }
    }

    // RevokeLeaseNotice는 coordinator_device_id가 아닌 lease_id를
    // signer_id로 쓰는 기존 계약을 따른다(V-08). 정상 통지는 현재
    // 보유 Lease id 아래에서 같은 Coordinator 키로 검증한다.
    coordinator_keys.insert(
        held_lease.lease_id.clone(),
        config.coordinator_verifying_key,
    );
    if let Some(test_signer_id) = &config.revoke_signer_id_override {
        coordinator_keys.insert(test_signer_id.clone(), config.coordinator_verifying_key);
    }

    // ★ 워크로드가 있으면 ACK 는 이미 실행 **전**에 갔다(결함 ⑱). 없으면 여기서 보낸다.
    let ack = match sent_ack {
        Some(sent) => sent,
        None => send_grant_ack(&mut stream, signing_key, &grant, &config, clock)?,
    };

    // ★ 노드 생존 보고 (2026-08-29, ADR-033 §7 앞 단계).
    //
    //   ACK 뒤·갱신 앞의 고정 위치로 보낸다. 이 stub 프로토콜에는
    //   비동기 multiplexing 이 없어 주기적 전송을 표현할 수 없다 —
    //   할 수 없는 것을 하는 척하지 않고, 표현 가능한 순차 위치에 둔다.
    //
    //   `heartbeat_rounds == 0`(기본값)이면 이 구간이 통째로 없다.
    for round in 0..config.heartbeat_rounds {
        // 첫 회차는 기다리지 않는다 — 간격은 회차 **사이**의 것이다.
        if round > 0 && config.heartbeat_interval_ms > 0 {
            std::thread::sleep(Duration::from_millis(config.heartbeat_interval_ms));
        }
        let now = clock.now_unix_ms();
        let mut heartbeat = pb::NodeHeartbeat {
            schema_version: 1,
            node_id: config.agent_device_id.clone(),
            device_id: if config.corrupt_heartbeat_device {
                format!("{}-OTHER", config.agent_device_id)
            } else {
                config.agent_device_id.clone()
            },
            coordinator_device_id: if config.corrupt_heartbeat_coordinator {
                format!("{}-OTHER", config.coordinator_device_id)
            } else {
                config.coordinator_device_id.clone()
            },
            issued_at_unix_ms: now,
            // 지금 들고 있는 Lease 의 세대. 이게 있어야 Coordinator 가
            // "살아 있다" 뿐 아니라 "어느 세대를 들고 살아 있다" 를 안다.
            fence_epoch: if config.corrupt_heartbeat_fence {
                held_lease.fence_epoch.wrapping_add(999)
            } else {
                held_lease.fence_epoch
            },
            // ★ 관측값이다. 지금 이 stub 은 한 번에 한 작업만 돌리므로
            //   0 또는 1 이다. 나중 값을 미리 만들어 넣지 않는다.
            running_attempts: u32::from(workload.is_some()),
            // ★ 회차별로 nonce 를 나눈다. 같은 nonce 를 두 번 쓰면
            //   두 번째가 replay 로 거부된다 — 갱신 경로가 이미 같은
            //   이유로 `derive_renew_nonce(lease_id, round)` 를 쓴다.
            request_nonce: derive_nonce(
                "node-heartbeat",
                &format!("{}:{}", held_lease.lease_id, round),
                config.connection_attempt,
            ),
            ..Default::default()
        };
        heartbeat.node_signature = sign(&signing_key, &heartbeat).to_vec();
        let frame = write_frame(FrameType::NodeHeartbeat, &heartbeat.encode_to_vec())
            .map_err(|e| format!("NodeHeartbeat 프레임 인코딩 실패(round={round}): {e}"))?;
        stream
            .write_all(&frame)
            .map_err(|e| format!("NodeHeartbeat 전송 실패(round={round}): {e}"))?;
        stream.flush().map_err(|e| e.to_string())?;
        println!(
            "HEARTBEAT_SENT round={round} fence_epoch={} running_attempts={}",
            heartbeat.fence_epoch, heartbeat.running_attempts
        );
    }

    // ── 이웃 신고(`ADR-033` §7 의 관측 층) ───────────────────────────
    //
    // ★ 이 stub 은 **이웃이 아니다.** 자기 자신이 연결된 Coordinator 에게
    //   "다른 어떤 노드에 연락이 안 된다" 고 보고할 뿐이고, 그 노드가
    //   실제로 존재하는지도 모른다 — 이 lane 은 네트워크를 갈라 볼 수단이
    //   없다. 그러므로 이 경로가 증명하는 것은 **"서명된 관측이 wire 를
    //   건너 저장소에 도달한다"** 이지 "연락 두절을 실제로 감지한다" 가
    //   아니다.
    //
    //   `neighbor_report_rounds == 0`(기본값)이면 이 구간이 통째로 없다.
    for round in 0..config.neighbor_report_rounds {
        // ★ 이 분기는 **도달 불가다** — `run()` 이 연결 전에 이미 막는다
        //   (독립 검수 4라운드 정정). 그래도 `Option` 을 여는 자리는
        //   필요하므로 남기고, 사실을 적어 둔다.
        let Some(target) = config.neighbor_report_target_node_id.as_ref() else {
            return Err(
                "NEIGHBOR_REPORT_REFUSED: --neighbor-report-target 없이 신고할 수 없다"
                    .to_string()
                    .into(),
            );
        };
        if round > 0 && config.neighbor_report_interval_ms > 0 {
            std::thread::sleep(Duration::from_millis(config.neighbor_report_interval_ms));
        }
        let now = clock.now_unix_ms();
        let mut report = pb::NeighborUnreachableReport {
            schema_version: 1,
            // ★ 이 stub 은 기계 ID 와 장치 ID 를 같은 값으로 쓴다 —
            //   heartbeat 경로가 이미 그렇게 한다. 둘이 다른 실제 배치에서는
            //   저장소의 기계->장치 결합 검사가 그 차이를 본다.
            reporter_node_id: config.agent_device_id.clone(),
            reporter_device_id: config.agent_device_id.clone(),
            unreachable_node_id: target.clone(),
            coordinator_device_id: if config.corrupt_neighbor_report_coordinator {
                format!("{}-OTHER", config.coordinator_device_id)
            } else {
                config.coordinator_device_id.clone()
            },
            observed_at_unix_ms: now,
            // ★ 회차별 nonce. 같은 nonce 를 두 번 쓰면 두 번째가 replay 로
            //   거부된다 — heartbeat·갱신과 같은 이유다.
            request_nonce: derive_nonce(
                "neighbor-unreachable",
                &format!("{}:{}", held_lease.lease_id, round),
                config.connection_attempt,
            ),
            ..Default::default()
        };
        report.reporter_signature = sign(&signing_key, &report).to_vec();
        let frame = write_frame(
            FrameType::NeighborUnreachableReport,
            &report.encode_to_vec(),
        )
        .map_err(|e| {
            format!("NeighborUnreachableReport 프레임 인코딩 실패(round={round}): {e}")
        })?;
        stream
            .write_all(&frame)
            .map_err(|e| format!("NeighborUnreachableReport 전송 실패(round={round}): {e}"))?;
        stream.flush().map_err(|e| e.to_string())?;
        println!(
            "NEIGHBOR_REPORT_SENT round={round} unreachable_node_id={} observed_at_unix_ms={}",
            report.unreachable_node_id, report.observed_at_unix_ms
        );
    }

    // ── 종료 보고(`AttemptReport`) ───────────────────────────────────
    //
    // ★ **관측한 종료가 없으면 보내지 않는다.** 실행이 꺼져 있거나
    //   Grant 에 Manifest 가 없었으면 `terminal_observation` 이 `None`
    //   이고, 그때 빈 보고를 만들어 보내면 그것이 지어낸 값이다
    //   (`CLAUDE.md` §1). 켜 놓고 보낼 것이 없으면 **오류로 끝낸다** —
    //   조용히 건너뛰면 운영자는 보고가 간 줄 안다.
    //
    // ★ 자리는 heartbeat·이웃 신고와 같은 이유로 **고정 순차 위치**다.
    //   이 stub 프로토콜에는 비동기 다중화가 없어 "끝나는 즉시" 를
    //   표현할 수 없다 — 표현할 수 없는 것을 하는 척하지 않는다.
    if config.send_attempt_report {
        let Some(observed) = terminal_observation.as_ref() else {
            return Err(
                "ATTEMPT_REPORT_REFUSED: 관측된 워크로드 종료가 없다 — \
                 보고할 사실이 없으면 보내지 않는다"
                    .to_string(),
            );
        };
        // 발행 시각만 지금 읽는다. 시작·종료는 그때 관측한 값 그대로다.
        let attempt_report = report::build_signed_attempt_report(
            &signing_key,
            &crate::report::TerminalObservation {
                issued_at_unix_ms: clock.now_unix_ms(),
                ..observed.clone()
            },
        )
        .map_err(|error| error.to_string())?;
        let frame =
            report::attempt_report_frame(&attempt_report).map_err(|error| error.to_string())?;
        stream
            .write_all(&frame)
            .map_err(|e| format!("AttemptReport 전송 실패: {e}"))?;
        stream.flush().map_err(|e| e.to_string())?;
        println!(
            "ATTEMPT_REPORT_SENT job_id={} attempt_id={} node_id={} fence_epoch={} \
             outcome={} exit_code={} exit_observation={} schema_version={} \
             started_at_unix_ms={} finished_at_unix_ms={} \
             issued_at_unix_ms={}",
            attempt_report.job_id,
            attempt_report.attempt_id,
            attempt_report.node_id,
            attempt_report.fence_epoch,
            attempt_report.outcome,
            // 결함 80 — 코드가 없으면 none 이다(WORKLOAD_EXITED 와 같은 표현). 기본값 0 을 코드처럼 찍지 않는다.
            if attempt_report.exit_observation == pb::ExitObservation::ObservedWithCode as i32 {
                attempt_report.exit_code.to_string()
            } else {
                "none".to_string()
            },
            attempt_report.exit_observation,
            attempt_report.schema_version,
            attempt_report.started_at_unix_ms,
            attempt_report.finished_at_unix_ms,
            attempt_report.issued_at_unix_ms
        );
    }

    // ★ 결함 82 — 작업 디렉터리 삭제 실패는 종료 보고를 처리한 **뒤에** 알린다. 남의 PC 에 남은 출력을 조용히 넘기지 않는다(§0.5).
    if let Some(error) = pending_cleanup_failure.take() {
        return Err(format!(
            "WORKLOAD_CLEANUP_FAILED: 작업 디렉터리를 지우지 못했다({}): {error}",
            // 결함 91 — 보내는 설정일 때만 "보냈다" 다. 보낼 관측이 없으면 이 자리에 오기 전에 ATTEMPT_REPORT_REFUSED 로 끝난다.
            if config.send_attempt_report {
                "종료 보고는 보냈다"
            } else {
                "종료 보고를 보내는 설정이 아니다 — 보고 없음"
            }
        ));
    }

    // ★ 워크로드를 띄운 연결에서는 끊김을 재접속 사유로 쓰지 않는다 — 재접속은 같은 Grant 로
    //   처음부터 다시 돌고, 그러면 워크로드가 **두 번** 실행된다. 설계 A 로 ACK 가 실행 전에 가면
    //   기본 Coordinator 는 ACK 직후 닫으므로 실행이 끝날 즈음엔 닫혀 있을 수 있다(빈도는 재지 않았다 — 결함 52)
    //   (`docs/reports/debugs/2026-09-14_1055_재접속이_워크로드를_다시_돌린다.md`).
    if config.reconnect_enabled
        && !config.do_renew
        && !config.expect_replay
        && config.expect_revoke_after_round.is_none()
        && !*execution_attempted
        && peer_closed_after_ack(&stream)?
    {
        return Err("RETRYABLE_CONNECTION: coordinator disconnected after ACK".into());
    }

    let mut revoked = false;
    if config.expect_revoke_after_round == Some(0) {
        let notice = receive_and_validate_revoke(
            &mut stream,
            &coordinator_keys,
            &mut replay,
            &clock,
            &held_lease,
        )?;
        println!(
            "REVOKE_RESULT ok=true lease_id={} fence_epoch={} cause={}",
            notice.lease_id, notice.fence_epoch, notice.cause
        );
        revoked = true;
    }

    // ★ replay 시나리오는 여기서 끝낸다 — **절대 `RESULT ok=true` 를
    //   찍지 않는다.** 두 번째로 도착하는 프레임은 Coordinator 가
    //   `send_grant_twice` 로 다시 보낸 같은 Grant wire bytes 다.
    //   `require_replay_checked()` 까지 갈 필요도 없다 — `verify()`
    //   자체가 `Duplicate` 를 만나면 `read_frame` 단계에서 이미 `Err`
    //   를 반환한다.
    if config.expect_replay {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        return match read_frame(
            &mut stream,
            2,
            KeyDirectorySource::Provided(coordinator_keys),
            replay,
            clock,
        ) {
            Err(error) => Err(format!("REPLAY_REJECTED: {error}")),
            Ok(message) => Err(format!(
                "REPLAY_NOT_REJECTED: 두 번째 Grant 가 거부되지 않고 {message:?} 로 검증됐다"
            )),
        };
    }

    // ★ Lease 갱신 (2026-08-19) — ACK 를 보낸 뒤 같은 연결에 이어서
    //   `RenewLeaseRequest` 를 보내고 서명된 `RenewLeaseResult` 를
    //   기다린다. `do_renew == false` 면 건너뛴다. 반복 갱신
    //   (2026-08-19, `docs/plans/2026-08-19_2330_...`)이 추가되면서
    //   왕복을 `renew_rounds` 만큼 반복한다 — 기본값 1이면 기존
    //   단일 왕복과 동일하다.
    for round in if config.do_renew {
        0..config.renew_rounds
    } else {
        0..0
    } {
        if revoked {
            // revoke 뒤에는 request 생성·서명·전송보다 먼저 로컬에서
            // 멈춘다. Coordinator가 다음 frame을 기다리지 않도록
            // 이 연결의 작업도 여기서 정상 종료한다.
            println!(
                "RENEW_BLOCKED: lease revoked lease_id={}",
                held_lease.lease_id
            );
            break;
        }

        if config.renew_delay_ms != 0 {
            std::thread::sleep(Duration::from_millis(config.renew_delay_ms));
        }
        let renew_now = clock.now_unix_ms();
        if lease_is_expired(&held_lease, renew_now) {
            return Err(format!(
                "RENEW_REFUSED:LOCAL_EXPIRED: expires_at_unix_ms={} now={renew_now}",
                held_lease.expires_at_unix_ms
            ));
        }
        let mut renew_req = pb::RenewLeaseRequest {
            schema_version: 1,
            lease_id: held_lease.lease_id.clone(),
            fence_epoch: config
                .renew_request_epoch_override
                .unwrap_or(held_lease.fence_epoch),
            node_id: config.agent_device_id.clone(),
            issued_at_unix_ms: renew_now,
            // ★ round 를 nonce 입력에 섞는다 — lease_id 만으로는
            //   회차마다 같은 nonce 가 나와 두 번째 요청부터
            //   replay guard 가 Duplicate 로 거부한다(설계 `p105`
            //   가 코드 경로로 확정한 결함).
            nonce: derive_renew_nonce(
                &held_lease.lease_id,
                if config.reuse_renew_nonce_after_reconnect && config.connection_attempt > 0 {
                    0
                } else {
                    config.connection_attempt
                },
                round as u64,
            ),
            ..Default::default()
        };
        renew_req.node_signature = sign(&signing_key, &renew_req).to_vec();

        if config.corrupt_renew_request_signature {
            let last = renew_req
                .node_signature
                .last_mut()
                .ok_or_else(|| "node_signature 가 비어 있다".to_string())?;
            *last ^= 0x01;
        }

        let frame = write_frame(FrameType::LeaseRenew, &renew_req.encode_to_vec())
            .map_err(|e| format!("RenewLeaseRequest 프레임 인코딩 실패: {e}"))?;
        stream
            .write_all(&frame)
            .map_err(|e| format!("AMBIGUOUS_RENEW: RenewLeaseRequest 전송 실패: {e}"))?;
        stream
            .flush()
            .map_err(|e| format!("AMBIGUOUS_RENEW: RenewLeaseRequest flush 실패: {e}"))?;
        if config.drop_after_renew_request_once && config.connection_attempt == 0 && round == 0 {
            return Err(
                "AMBIGUOUS_RENEW: test hook dropped connection after RenewLeaseRequest flush"
                    .into(),
            );
        }

        let result_msg = read_frame(
            &mut stream,
            1,
            KeyDirectorySource::Provided(coordinator_keys),
            replay,
            clock,
        )
        .map_err(|e| classify_renew_result_error(e))?;

        // ★ `require_replay_checked()` — Grant 와 같은 이유(§10).
        let result = match &result_msg {
            IngressMessage::LeaseRenewResult(verified) => verified
                .require_replay_checked()
                .map_err(|e| format!("RenewLeaseResult replay 검사 실패: {e:?}"))?
                .clone(),
            other => return Err(format!("예상하지 못한 갱신 응답 타입: {other:?}")),
        };

        // ★ 결과 검증은 RENEW 세션(단계 5b)과 **같은 함수**다 — 두 벌을 두지 않는다.
        held_lease = verify_renew_result(
            &result,
            &renew_req.nonce,
            &held_lease,
            &config,
            &*coordinator_keys,
            &mut *replay,
            &clock,
            &mut *fence_watermark,
        )?;
        println!(
            "RENEW_RESULT ok=true outcome=RENEWED lease_id={} fence_epoch={}",
            held_lease.lease_id, held_lease.fence_epoch
        );

        if config.expect_revoke_after_round == Some(round + 1) {
            let notice = receive_and_validate_revoke(
                &mut stream,
                &coordinator_keys,
                &mut replay,
                &clock,
                &held_lease,
            )?;
            println!(
                "REVOKE_RESULT ok=true lease_id={} fence_epoch={} cause={}",
                notice.lease_id, notice.fence_epoch, notice.cause
            );
            revoked = true;
        }
    }

    println!(
        "RESULT ok=true grant_id={} attempt_id={} agent_device_id={}",
        grant.grant_id, grant.attempt_id, ack.agent_device_id
    );
    Ok(())
}

/// 갱신 결과를 검증하고 RENEWED 면 새 Lease 를 돌려준다 — FRESH 연결 안의 갱신 루프와 RENEW 세션(B+E 구현 단계 5b)이 같이 쓴다.
///
/// 순서: 요청 nonce echo -> coordinator_id -> outcome. RENEWED 면 중첩 Lease 를 **독립적으로** 서명 검증하고(규칙 i) 신원 ·
/// 세대 상승 거부 · fence watermark 를 본다. 그 밖의 outcome 은 `RENEW_REFUSED:<이름>` 오류다(전에 루프 안에 있던 코드 그대로).
#[allow(clippy::too_many_arguments)]
fn verify_renew_result(
    result: &pb::RenewLeaseResult,
    request_nonce: &[u8],
    held_lease: &pb::Lease,
    config: &AgentConfig,
    coordinator_keys: &InMemoryKeyring,
    replay: &mut InMemoryReplayGuard,
    clock: &SystemClock,
    fence_watermark: &mut DurableFenceWatermark,
) -> Result<pb::Lease, String> {
    // ★ 서명은 이미 검증됐다 — 그 뒤에 상관관계를 확인한다.
    //   request_nonce 가 우리가 보낸 요청과 다르면, 이 결과가 다른
    //   갱신 요청에 대한 응답이 재사용되고 있다는 뜻이다.
    if result.request_nonce != request_nonce {
        return Err("RENEW_REJECTED: request_nonce 가 우리가 보낸 요청과 다르다".into());
    }
    if result.coordinator_id != config.coordinator_device_id {
        return Err(format!(
            "RENEW_REJECTED: coordinator_id 불일치: 기대값 {} != {}",
            config.coordinator_device_id, result.coordinator_id
        ));
    }
    match result.outcome {
        1 => {
            // RENEW_OUTCOME_RENEWED — nested Lease 는 outer 결과 서명과 **무관하게** 독립적으로 검증한다(규칙 i).
            let new_lease = result
                .lease
                .clone()
                .ok_or_else(|| "RENEW_REJECTED: outcome=RENEWED 인데 Lease 가 없다".to_string())?;
            let verifier = Ed25519Verifier::new(coordinator_keys);
            let verified_lease = verify(&new_lease, 1, &verifier, clock.now_unix_ms(), replay)
                .map_err(|e| format!("RENEW_REJECTED: 갱신된 Lease 서명 검증 실패: {e:?}"))?;
            let new_lease = verified_lease.get();
            if new_lease.lease_id != held_lease.lease_id {
                return Err("RENEW_REJECTED: 갱신된 Lease.lease_id 가 기존과 다르다".into());
            }
            if new_lease.job_id != held_lease.job_id {
                return Err("RENEW_REJECTED: 갱신된 Lease.job_id 가 기존과 다르다".into());
            }
            if new_lease.attempt_id != held_lease.attempt_id {
                return Err("RENEW_REJECTED: 갱신된 Lease.attempt_id 가 기존과 다르다".into());
            }
            if new_lease.issuing_coordinator_id != config.coordinator_device_id {
                return Err("RENEW_REJECTED: 갱신된 Lease.issuing_coordinator_id 가 기대값과 다르다".into());
            }
            if new_lease.holder_node_id != config.agent_device_id {
                return Err("RENEW_REJECTED: 갱신된 Lease.holder_node_id 가 이 Agent 가 아니다".into());
            }
            // ★ 코덱스 독립 검수(2026-08-19, p99) — epoch **상승**은 정책상 거부한다. `check_and_advance()` 는 `<` 만 거부한다.
            if new_lease.fence_epoch > held_lease.fence_epoch {
                return Err(format!(
                    "RENEW_REJECTED: 갱신된 Lease.fence_epoch({}) 이 기존({}) 보다 높다 \
                     — epoch 상승은 이 조각의 범위 밖이라 정책상 거부한다",
                    new_lease.fence_epoch, held_lease.fence_epoch
                ));
            }
            // ★ 같은 job_id 를 resource key 로 재사용한다 — lease_id 를 새 키로 쓰면 강등 방어가 깨진다.
            fence_watermark
                .check_and_advance(&new_lease.job_id, new_lease.fence_epoch)
                .map_err(|e| fence_error_message("RENEW_REJECTED", e))?;
            Ok(new_lease.clone())
        }
        2 => Err("RENEW_REFUSED:SUPERSEDED".into()),
        3 => Err("RENEW_REFUSED:QUARANTINED".into()),
        // max_total_duration_seconds 갱신 차단 — 새 lease_id 재발급은 범위 밖이다.
        6 => Err("RENEW_REFUSED:MAX_DURATION_EXCEEDED".into()),
        8 => Err("RENEW_REFUSED:REVOKED".into()),
        other => Err(format!("RENEW_REJECTED: 알 수 없는 outcome {other}")),
    }
}

/// B+E 구현 단계 5b — RENEW 세션 한 번: 새 연결 -> Hello(RENEW) -> RenewLeaseRequest -> 서명된 RenewLeaseResult 검증.
///
/// ★ 이 연결은 FRESH 연결과 **따로** 연다 — 실행 중에는 FRESH 연결이 이미 닫혔을 수 있고(설계 A 뒤 ACK 직후 닫힌다),
///   Coordinator 는 Hello 의 mode 로 이 연결을 갱신 전용으로 가른다(단계 5a).
/// ★ 검증 도구는 이 호출 안에서 새로 만든다 — 키는 설정의 Coordinator 키, replay 는 이 연결용, fence watermark 는 같은 DB 파일을
///   따로 연다(SQLite 연결은 스레드 사이에 옮기지 않는다). 규칙은 FRESH 갱신과 같은 `verify_renew_result` 다.
fn renew_once_over_new_connection(
    config: &AgentConfig,
    signing_key: &SigningKey,
    held_lease: &pb::Lease,
) -> Result<pb::Lease, String> {
    let clock = SystemClock;
    let now = clock.now_unix_ms();
    if lease_is_expired(held_lease, now) {
        return Err(format!(
            "RENEW_REFUSED:LOCAL_EXPIRED: expires_at_unix_ms={} now={now}",
            held_lease.expires_at_unix_ms
        ));
    }
    let mut stream = connect_with_timeout(&config.coordinator_addr, IO_TIMEOUT)?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;

    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        mode: gputeer_protocol::constants::MODE_RENEW,
        session_id: config.session_id.clone(),
        node_id: config.agent_device_id.clone(),
        // RENEW 는 Coordinator 가 연결 번호를 대조하지 않는다(단계 5a) — 실행 중에는 그 번호를 알 수 없다.
        connection_attempt: 0,
        issued_at_unix_ms: now,
        nonce: fresh_nonce()?,
        ..Default::default()
    };
    hello.node_signature = sign(signing_key, &hello).to_vec();
    let mut request = pb::RenewLeaseRequest {
        schema_version: 1,
        lease_id: held_lease.lease_id.clone(),
        fence_epoch: held_lease.fence_epoch,
        node_id: config.agent_device_id.clone(),
        issued_at_unix_ms: now,
        nonce: fresh_nonce()?,
        ..Default::default()
    };
    request.node_signature = sign(signing_key, &request).to_vec();
    for (frame_type, body, what) in [
        (FrameType::SessionHello, hello.encode_to_vec(), "Hello(RENEW)"),
        (FrameType::LeaseRenew, request.encode_to_vec(), "RenewLeaseRequest"),
    ] {
        let frame =
            write_frame(frame_type, &body).map_err(|e| format!("{what} 프레임 인코딩 실패: {e}"))?;
        stream
            .write_all(&frame)
            .map_err(|e| format!("{what} 전송 실패: {e}"))?;
    }
    stream.flush().map_err(|e| e.to_string())?;

    let mut keys = InMemoryKeyring::new();
    keys.insert(config.coordinator_device_id.clone(), config.coordinator_verifying_key);
    let mut replay = InMemoryReplayGuard::new();
    let message = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&keys),
        &mut replay,
        &clock,
    )
    .map_err(|e| format!("RenewLeaseResult 읽기/검증 실패: {e}"))?;
    let result = match message {
        IngressMessage::LeaseRenewResult(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("RENEW_REJECTED: RenewLeaseResult replay 검사 실패: {e:?}"))?
            .clone(),
        _ => return Err("RENEW_REJECTED: 갱신 결과가 아닌 프레임이다".into()),
    };
    let mut fence = DurableFenceWatermark::open(&config.fence_db_path)
        .map_err(|e| format!("fence watermark 저장소 열기 실패: {e}"))?;
    verify_renew_result(
        &result,
        &request.nonce,
        held_lease,
        config,
        &keys,
        &mut replay,
        &clock,
        &mut fence,
    )
}

/// B+E 구현 단계 5b — 워크로드가 도는 동안 갱신하는 스레드의 손잡이.
struct RenewDuringExecution {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: std::thread::JoinHandle<pb::Lease>,
}

/// `renew_during_execution_ms` 간격으로 RENEW 세션을 연다. 0 이면 시작하지 않는다.
///
/// ```text
/// 갱신 성공        RENEW_SESSION_RESULT ok=true — 스레드가 든 Lease 를 새것으로 바꾼다
/// 거부 · 검증 실패  RENEW_SESSION_STOPPED — RENEW_REFUSED(SUPERSEDED · REVOKED · 만료 …) 나 RENEW_REJECTED 면 더 갱신하지 않는다
/// 그 밖의 실패     RENEW_SESSION_FAILED — 연결 · 전송 실패. 다음 주기에 다시 연다
/// ```
///
/// ★ 워크로드를 **멈추지는 않는다** — Lease 를 잃었을 때 워크로드를 어떻게 할지는 정책(규범 §3 LEASE_EXPIRED · STALE)이라
///   이 조각이 정하지 않는다. 사실을 알리는 데까지다.
fn start_renew_during_execution(
    config: &AgentConfig,
    signing_key: &SigningKey,
    held_lease: &pb::Lease,
) -> Option<RenewDuringExecution> {
    if config.renew_during_execution_ms == 0 {
        return None;
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let thread_stop = std::sync::Arc::clone(&stop);
    let config = config.clone();
    let key = signing_key.clone();
    let mut lease = held_lease.clone();
    let handle = std::thread::spawn(move || {
        let interval = Duration::from_millis(config.renew_during_execution_ms);
        let mut round: u64 = 0;
        'renew: loop {
            let due = std::time::Instant::now() + interval;
            while std::time::Instant::now() < due {
                if thread_stop.load(std::sync::atomic::Ordering::SeqCst) {
                    break 'renew;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            match renew_once_over_new_connection(&config, &key, &lease) {
                Ok(renewed) => {
                    println!(
                        "RENEW_SESSION_RESULT ok=true round={round} lease_id={} fence_epoch={} expires_at_unix_ms={}",
                        renewed.lease_id, renewed.fence_epoch, renewed.expires_at_unix_ms
                    );
                    lease = renewed;
                }
                Err(error) if error.starts_with("RENEW_REFUSED") || error.starts_with("RENEW_REJECTED") => {
                    println!("RENEW_SESSION_STOPPED round={round} detail={error}");
                    break 'renew;
                }
                Err(error) => println!("RENEW_SESSION_FAILED round={round} detail={error}"),
            }
            round += 1;
        }
        lease
    });
    Some(RenewDuringExecution { stop, handle })
}

/// revoke 프레임을 서명 검증한 뒤 현재 보유 Lease에 적용한다.
///
/// `read_frame()`이 반환한 `Verified` 내부 값만 이 함수에 들어오므로,
/// identity·만료 판정은 서명 검증 이후에만 수행된다. 반환된 통지는
/// 호출자가 `revoked` 상태를 세우고 다음 갱신 회차를 차단하는 데 쓴다.
fn receive_and_validate_revoke(
    stream: &mut TcpStream,
    coordinator_keys: &InMemoryKeyring,
    replay: &mut InMemoryReplayGuard,
    clock: &SystemClock,
    held_lease: &pb::Lease,
) -> Result<pb::RevokeLeaseNotice, String> {
    let message = read_frame(
        stream,
        1,
        KeyDirectorySource::Provided(coordinator_keys),
        replay,
        clock,
    )
    .map_err(|e| classify_framing_error(e, "RevokeLeaseNotice"))?;
    let notice = match message {
        IngressMessage::LeaseRevoke(verified) => verified.get().clone(),
        other => return Err(format!("예상하지 못한 revoke 응답 타입: {other:?}")),
    };
    validate_revoke_notice(held_lease, &notice, clock.now_unix_ms())?;
    Ok(notice)
}

/// 검증된 revoke 통지가 현재 보유 Lease에 적용 가능한지 판정한다.
fn validate_revoke_notice(
    held_lease: &pb::Lease,
    notice: &pb::RevokeLeaseNotice,
    now_unix_ms: u64,
) -> Result<(), String> {
    if notice.lease_id != held_lease.lease_id {
        return Err(format!(
            "REVOKE_REJECTED: lease_id 불일치: 기대값 {} != {}",
            held_lease.lease_id, notice.lease_id
        ));
    }
    if notice.fence_epoch != held_lease.fence_epoch {
        return Err(format!(
            "REVOKE_REJECTED: fence_epoch 불일치: 기대값 {} != {}",
            held_lease.fence_epoch, notice.fence_epoch
        ));
    }
    if lease_is_expired(held_lease, now_unix_ms) {
        return Err(format!(
            "REVOKE_REJECTED: held Lease가 이미 만료됐다: expires_at_unix_ms={} now={now_unix_ms}",
            held_lease.expires_at_unix_ms
        ));
    }
    Ok(())
}

fn lease_is_expired(lease: &pb::Lease, now_unix_ms: u64) -> bool {
    lease.expires_at_unix_ms <= now_unix_ms
}

/// `ExecutionGrant.lease` 에 실린 `Lease` 를 독립적으로 검증하고
/// `fence_epoch` 를 `watermark` 에 기록한다.
///
/// 서명 검증(`verify()`) 전에 필드 값을 신뢰하지 않는다 — 상관관계
/// 검사(attempt_id·issuing_coordinator·holder_node_id·job_id)는
/// **서명 검증 뒤에** 한다. 서명 안 된 필드를 먼저 믿고 분기하면
/// 위조된 Lease 로도 조기 반환을 유도할 수 있다.
/// Grant 에 실려 온 nested `JobManifest` 를 **독립 검증**하고 실행 지시를 뽑는다.
///
/// `Lease` 검증(`verify_and_record_lease`)과 같은 이유로 존재한다 —
/// outer Grant 서명이 유효해도 nested 메시지 서명은 따로 위조될 수 있다.
///
/// # 검사 순서
///
/// ```text
/// 1  Manifest 가 없다                 -> Ok(None). 기존 경로 그대로
/// 2  Manifest 는 있는데 제출자 키가 없다 -> 거부. 검증 못 하는 지시는 안 받는다
/// 3  제출자 서명 검증 (Verified<M> 획득)
/// 4  job_id 가 Lease 의 job_id 와 같은가
/// 5  실행 지시 도출 (derive_execution_spec)
/// ```
///
/// `manifest_hash` 대조는 이 함수가 아니라 프로토콜 계층이 한다 —
/// 아래 본문 주석 참조.
fn verify_nested_manifest(
    grant: &pb::ExecutionGrant,
    config: &AgentConfig,
    now_unix_ms: u64,
    replay: &mut InMemoryReplayGuard,
) -> Result<Option<VerifiedWorkload>, String> {
    let Some(manifest) = grant.manifest.as_ref() else {
        return Ok(None);
    };

    // 검증할 수단이 없으면 받아들이지 않는다. 조용히 무시하면 "실행
    // 지시가 없는 것" 과 "검증 못 한 실행 지시가 온 것" 이 구분되지 않는다.
    let Some(submitter_key) = config.submitter_verifying_key else {
        return Err(
            "MANIFEST_REJECTED: Grant 에 Manifest 가 있는데 제출자 공개키가 설정되지 않았다".into(),
        );
    };

    let mut submitter_keyring = InMemoryKeyring::new();
    submitter_keyring.insert(manifest.submitter_device_id.clone(), submitter_key);
    let verifier = Ed25519Verifier::new(&submitter_keyring);
    let verified = verify(manifest, 1, &verifier, now_unix_ms, replay)
        .map_err(|e| format!("MANIFEST_REJECTED: Manifest 서명 검증 실패: {e:?}"))?;

    // ★ `manifest_hash` 재계산 대조는 **여기서 하지 않는다.**
    //
    //   `CLAUDE.md` §0.2 가 요구하는 그 검사는 이미 프로토콜 계층에 있다 —
    //   `crates/protocol/src/signable.rs` 의
    //   `ExecutionGrant::check_derived_consistency()` 가 Grant 검증 중에
    //   `BLAKE3_256(sig_input_of(JobManifest))` 를 재계산해 대조하고,
    //   다르면 `DerivedMismatch` 로 Grant 자체를 거부한다(2026-08-16
    //   독립 검수가 "규범은 요구하는데 코드가 없다" 고 지적해 추가된 것).
    //
    //   ★ 처음엔 여기에도 같은 대조를 넣었다가 뺐다. 뮤테이션으로
    //   확인해 보니 그 코드는 **도달하지 않았다** — 프로토콜 계층이
    //   먼저 거부하기 때문이다. 도달하지 않는 방어를 남겨 두면
    //   "구현했다" 와 "강제한다" 를 혼동하게 된다.
    //
    //   같은 이유로 "hash 가 반드시 있어야 한다" 도 넣지 않는다.
    //   규범은 `manifest 있음 + hash 없음 -> 통과(주장을 안 했으므로)`
    //   라고 정했다. 그보다 엄격한 규칙을 여기서 발명하지 않는다.

    // 같은 Grant 안의 Lease 와 같은 Job 이어야 한다.
    if let Some(lease) = grant.lease.as_ref() {
        if verified.get().job_id != lease.job_id {
            return Err(format!(
                "MANIFEST_REJECTED: Manifest job_id({})가 Lease job_id({})와 다르다",
                verified.get().job_id,
                lease.job_id
            ));
        }
    }

    let spec = gputeer_protocol::execution_spec::derive_execution_spec(&verified)
        .map_err(|e| format!("MANIFEST_REJECTED: 실행 지시를 만들 수 없다: {e}"))?;
    // ★ 제출자 신원을 여기서 같이 꺼낸다. `ExecutionSpec` 에는 없는데,
    //   Owner Panel 은 소유자에게 **누가** 내 GPU 를 쓰는지 보여야
    //   한다(`CLAUDE.md` §0.1). 검증을 통과한 뒤에만 읽는다.
    Ok(Some(VerifiedWorkload {
        submitter_device_id: verified.get().submitter_device_id.clone(),
        spec,
    }))
}

/// 검증을 통과한 실행 지시와, 그것을 낸 사람.
struct VerifiedWorkload {
    spec: gputeer_protocol::execution_spec::ExecutionSpec,
    /// 소유자 화면에 보여줄 제출자. `ExecutionSpec` 에는 없다 —
    /// 실행에는 필요 없지만 **소유자에게는 필요한** 사실이다.
    submitter_device_id: String,
}

fn verify_and_record_lease(
    grant: &pb::ExecutionGrant,
    config: &AgentConfig,
    coordinator_keys: &InMemoryKeyring,
    now: u64,
    replay: &mut InMemoryReplayGuard,
    watermark: &mut DurableFenceWatermark,
) -> Result<pb::Lease, String> {
    let lease = grant
        .lease
        .clone()
        .ok_or_else(|| "LEASE_REJECTED: Grant 에 Lease 가 없다".to_string())?;

    let verifier = Ed25519Verifier::new(coordinator_keys);
    let verified = verify(&lease, 1, &verifier, now, replay)
        .map_err(|e| format!("LEASE_REJECTED: Lease 서명 검증 실패: {e:?}"))?;
    let lease = verified.get();

    // ★ 서명이 유효하다고 확인한 **뒤에만** 상관관계를 검사한다.
    if lease.attempt_id != grant.attempt_id {
        return Err("LEASE_REJECTED: attempt_id 가 Grant 와 다르다".into());
    }
    if lease.issuing_coordinator_id != grant.coordinator_device_id {
        return Err("LEASE_REJECTED: issuing_coordinator_id 가 Grant 와 다르다".into());
    }
    if lease.holder_node_id != config.agent_device_id {
        return Err("LEASE_REJECTED: holder_node_id 가 이 Agent 가 아니다".into());
    }
    if lease.job_id.is_empty() {
        return Err("LEASE_REJECTED: job_id 가 비어 있다".into());
    }

    watermark
        .check_and_advance(&lease.job_id, lease.fence_epoch)
        .map_err(|e| fence_error_message("LEASE_REJECTED", e))?;

    Ok(lease.clone())
}

/// 시작 마커 전용 checkpoint 디렉터리를 만들고 `WRITING`을 기록한다.
///
/// ID는 Job ID를 경로 성분으로 직접 사용하지 않는다. 대신 domain tag와
/// 세 문자열을 각각 u64 big-endian 길이 접두사로 구분한 바이트열에
/// BLAKE3-256을 적용하고, 64자리 hex digest 앞에 `start-`를 붙인다.
/// 길이 접두사는 필드 경계가 모호해지는 충돌을 막고, 전체 digest는
/// 예측 가능한 Job ID 경로와 임의 문자열 충돌을 피한다.
pub fn start_checkpoint_id(job_id: &str, attempt_id: &str, grant_id: &str) -> String {
    let mut input = Vec::with_capacity(
        b"gputeer/job-start-checkpoint/v1\0".len()
            + 3 * std::mem::size_of::<u64>()
            + job_id.len()
            + attempt_id.len()
            + grant_id.len(),
    );
    input.extend_from_slice(b"gputeer/job-start-checkpoint/v1\0");
    for value in [job_id, attempt_id, grant_id] {
        input.extend_from_slice(&(value.len() as u64).to_be_bytes());
        input.extend_from_slice(value.as_bytes());
    }

    let digest = gputeer_protocol::canonical::blake3_256(&input);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("start-{hex}")
}

/// 실행 후 보고할 것들.
struct WorkloadReport {
    /// 종료 관측 — 코드가 없을 수 있다(신호 종료 · 코드 조회 실패, 결함 69).
    exit: exec::ExitObserved,
    /// 종료 뒤 산출물 확정이 실패했으면 그 단계와 사유(결함 ⑲). 있으면 `file_count` · `total_bytes` 는 0 이다.
    finalization_failure: Option<(pb::FinalizationFailureStage, String)>,
    file_count: usize,
    total_bytes: usize,
    /// 자식을 띄우기 **직전에** 읽은 시계. 호출부가 준 값을 그대로
    /// 돌려준다 — 여기서 다시 계산하지 않는다.
    started_at_unix_ms: u64,
    /// 자식을 **거둔 직후**에 읽은 시계.
    ///
    /// ★ `started + 걸린시간` 이 아니다(`CLAUDE.md` §1). 산출물 수집과
    ///   체크포인트 확정은 프로세스가 끝난 **뒤** 일이므로, 그 뒤에 시계를
    ///   읽으면 "언제 끝났나" 가 아니라 "언제 정리가 끝났나" 가 된다.
    finished_at_unix_ms: u64,
}

/// 실행 -> 산출물 수집 -> 체크포인트 확정까지.
///
/// `Ok(None)` 은 **오류가 아니라 기본값**이다 — 운영자가 실행을
/// 명시적으로 켜지 않았다는 뜻이다. 위험한 동작은 기본으로 켜져
/// 있지 않다(`DoD-29` 와 같은 원칙).
///
/// ★ 이 함수는 작업 디렉터리를 **지우지 않는다.** 삭제는 호출부가
///   성공·실패 관계없이 한다 — 여기서 지우면 중간에 `?` 로 나가는
///   경로가 삭제를 건너뛰게 된다.
#[allow(clippy::too_many_arguments)]
fn run_and_capture_workload(
    spec: &gputeer_protocol::execution_spec::ExecutionSpec,
    policy: exec::ExecutionPolicy,
    checkpoint_root: &std::path::Path,
    checkpoint_id: &str,
    lease: &pb::Lease,
    attempt_id: &str,
    run_dir: &std::path::Path,
    panel: &owner_panel::OwnerPanelState,
    submitter_device_id: &str,
    started_at_unix_ms: u64,
    clock: &SystemClock,
) -> Result<Option<WorkloadReport>, String> {
    // ★ 자식이 뜨는 **즉시** 소유자 화면에 올린다. `execute()` 가
    //   돌아온 뒤에 등록하면 그건 이미 끝난 뒤라 아무 의미가 없다 —
    //   소유자는 도는 동안 멈출 수 있어야 한다(`CLAUDE.md` §0.1).
    // 결함 84 — 캡처를 켰으면 실행기가 출력 파일을 만든다. 없으면 잃은 것이다.
    let outputs_captured = policy.capture_dir.is_some();
    let outcome = match exec::execute_with_control(spec, policy, |stopper| {
        // ★ 자식이 **막 떴다**(결함 ㊻ — ACK_SENT 와 순서를 비교하는 표지).
        println!("WORKLOAD_SPAWNED job_id={} attempt_id={}", spec.job_id, attempt_id);
        panel.register(owner_panel::RunningWorkload {
            job_id: spec.job_id.clone(),
            attempt_id: attempt_id.to_string(),
            submitter_device_id: submitter_device_id.to_string(),
            started_at_unix_ms,
            entrypoint: spec.entrypoint.clone(),
            // 이 조각에는 실행 중 체크포인트가 없다 — 산출물은 끝난 뒤에
            // 확정된다. 그러므로 도는 동안은 "확정된 것이 하나도 없다"
            // 가 사실이고, 화면도 그렇게 보여야 한다.
            last_checkpoint_at_unix_ms: None,
            stopper,
        });
    }) {
        Ok(outcome) => outcome,
        Err(exec::ExecutionError::NotOptedIn) => return Ok(None),
        // ★ 여기서도 종료 시각을 읽지 않는다. 실행 자체가 **일어나지
        //   않았거나**(NotOptedIn·UnsupportedPlatform·LimitNotApplied·
        //   SpawnFailed) 종료를 **관측하지 못한** 경우(WaitFailed)이므로,
        //   보고할 종료가 없다.
        //   ★ 종료를 관측했지만 코드가 없는 경우는 여기로 오지 않는다 — `Ok` 로 돌아와
        //     OBSERVED_NO_CODE 로 보고된다(결함 69).
        Err(other) => {
            // 등록됐을 수도 있으니 반드시 뺀다. 안 빼면 끝난 작업이
            // 소유자 화면에 영원히 남는다.
            panel.unregister(attempt_id);
            return Err(other.to_string());
        }
    };
    // ★ 종료 시각을 **여기서** 읽는다 — `execute_with_control()` 가
    //   돌아온 직후이자 산출물 수집 전이다. 아래 `collect_...` 와
    //   `finalize_...` 뒤로 미루면 그 값은 "언제 끝났나" 가 아니라
    //   "언제 정리까지 끝났나" 가 된다.
    let finished_at_unix_ms = clock.now_unix_ms();
    // 프로세스는 끝났다. 산출물 확정이 남았지만 **멈출 대상은 이미
    // 없으므로** 화면에서 뺀다 — 못 멈추는 정지 버튼을 보이지 않는다.
    panel.unregister(attempt_id);
    // ★ 코드가 없으면 `exit_code=none` 이다 — 0 이나 -1 로 채우지 않는다(결함 69). 최대 메모리도 모르면 `unknown`.
    println!(
        "WORKLOAD_EXITED job_id={} exit_code={} commit_limit_bytes={} peak_commit_bytes={}",
        spec.job_id,
        outcome
            .exit
            .code()
            .map_or_else(|| "none".to_string(), |code| code.to_string()),
        outcome.commit_limit_bytes,
        outcome
            .peak_commit_bytes
            .map_or_else(|| "unknown".to_string(), |peak| peak.to_string())
    );
    if let exec::ExitObserved::NoCode { detail } = &outcome.exit {
        println!("WORKLOAD_EXIT_CODE_UNAVAILABLE job_id={} detail={detail}", spec.job_id);
    }
    if let Some(error) = &outcome.memory_observation_error {
        println!("WORKLOAD_MEMORY_OBSERVATION_FAILED job_id={} detail={error}", spec.job_id);
    }

    // ★ 결함 ⑲ — 여기부터의 실패는 **종료를 관측한 뒤**의 일이다. 전에는 `?` 로 돌려 Agent 오류가 됐고,
    //   그러면 종료 보고가 만들어지지 않아 관측한 종료까지 사라졌다. 이제 실패 단계를 보고에 싣는다.
    //   ★ 상위 확정 작업(finalize_workload_outputs)은 다시 부르지 않는다 — 포인터 rename 은 최대 5회 반복한다
    //     (checkpoint atomic.rs — 임시 파일 생성 · 쓰기 · sync 실패는 그 반복 전에 반환한다, 결함 91). D3 "재시도를 끝낸 뒤" 는 그 뒤다(결함 86 — 전에는 "재시도 0회" 로 넓게 적었다).
    let (file_count, total_bytes, finalization_failure) = match finalize_workload_outputs(
        run_dir,
        spec,
        &outcome,
        outputs_captured,
        checkpoint_root,
        checkpoint_id,
        lease,
        attempt_id,
    ) {
        Ok((files, publish_failure)) => {
            // 결함 83 · 90 — 파일 해시 검증 뒤의 실패는 확정 실패가 아니다(exit 0 이면 COMPLETED, 아니면 FAILED — 결함 91). 그래도 알린다.
            if let Some(detail) = publish_failure {
                println!("WORKLOAD_CHECKPOINT_PUBLISH_FAILED job_id={} detail={detail}", spec.job_id);
            }
            (files.len(), files.iter().map(|(_, data)| data.len()).sum(), None)
        }
        Err((stage, detail)) => {
            println!(
                "WORKLOAD_FINALIZATION_FAILED job_id={} stage={} detail={detail}",
                spec.job_id,
                stage.as_str_name()
            );
            (0, 0, Some((stage, detail)))
        }
    };
    Ok(Some(WorkloadReport {
        exit: outcome.exit.clone(),
        finalization_failure,
        file_count,
        total_bytes,
        started_at_unix_ms,
        finished_at_unix_ms,
    }))
}

/// 실행 결과와 작업 디렉터리 삭제 결과를 합친다(결함 82, 재검수 58).
///
/// ★ 종료를 관측한 결과(`Ok(..)`)는 삭제가 실패해도 **버리지 않는다** — 삭제 실패는 따로 돌려주고, 호출부가 종료 보고를
///   처리한 **뒤에** 오류로 끝낸다. 전에는 삭제 실패가 먼저 반환돼 ⑲ 가 막으려던 "종료 보고 소실" 이 다시 생겼다.
fn merge_run_and_cleanup(
    outcome: Result<Option<WorkloadReport>, String>,
    cleanup: Result<(), String>,
) -> Result<(Option<WorkloadReport>, Option<String>), String> {
    match (outcome, cleanup) {
        (Ok(value), Ok(())) => Ok((value, None)),
        (Ok(value), Err(cleanup_error)) => Ok((value, Some(cleanup_error))),
        (Err(run_error), Ok(())) => Err(run_error),
        (Err(run_error), Err(cleanup_error)) => Err(format!("{run_error} / 그리고 {cleanup_error}")),
    }
}

/// 산출물 수집 -> 체크포인트 확정. 실패하면 **어느 단계에서** 실패했는지 돌려준다(결함 ⑲ · 계획서 §5.7 (1)).
///
/// ```text
/// READ_OUTPUTS       작업 출력(stdout · stderr) 읽기 실패
/// ENCODE_RESULT      작업 결과를 JSON 으로 바꾸기 실패
/// COMMIT_CHECKPOINT  데이터 파일 · 매니페스트 확정 실패
/// ```
fn finalize_workload_outputs(
    run_dir: &std::path::Path,
    spec: &gputeer_protocol::execution_spec::ExecutionSpec,
    outcome: &exec::ExecutionOutcome,
    outputs_captured: bool,
    checkpoint_root: &std::path::Path,
    checkpoint_id: &str,
    lease: &pb::Lease,
    attempt_id: &str,
) -> Result<(Vec<(String, Vec<u8>)>, Option<String>), (pb::FinalizationFailureStage, String)> {
    let files = collect_workload_artifacts(run_dir, spec, outcome, outputs_captured)?;
    // Ok(Some(..)) 는 해시 검증 뒤의 공개 실패다(결함 83) — 확정 실패가 아니다.
    let publish_failure = finalize_workload_checkpoint(checkpoint_root, checkpoint_id, lease, attempt_id, &files)
        .map_err(|detail| (pb::FinalizationFailureStage::CommitCheckpoint, detail))?;
    Ok((files, publish_failure))
}

/// 작업 출력을 받는 루트. 체크포인트 루트의 **형제** 디렉터리다.
///
/// 예: `C:/gputeer/checkpoints` -> `C:/gputeer/checkpoints.workload-run`
///
/// ★ 루트 안에 두면 `startup_gc()` 가 그것을 체크포인트로 보고
///   `gc_partial()` 을 돌린다. GC 의 스캔 범위 밖으로 빼낸다.
///
/// ★ **문자열을 그냥 이어 붙이지 않는다**(2026-08-29, 독립 검수 지적).
///   초안은 경로 문자열 끝에 접미사를 붙였는데, 그러면 이런 값들이
///   **루트 안**으로 떨어졌다 — 정확히 막으려던 것이 다시 생긴다.
///
///   ```text
///   "C:/data/cp/"  ->  "C:/data/cp/.workload-run"   루트 안 (결함)
///   "."            ->  "..workload-run"             현재 디렉터리 안 (결함)
///   "C:/"          ->  "C:/.workload-run"           루트 안 (결함)
///   ```
///
///   그래서 절대 경로로 정규화한 뒤 **부모 + 이름** 으로 계산한다.
///   `Path::file_name()` 은 후행 구분자를 무시하므로 첫 번째 반례도
///   `cp.workload-run` 으로 제대로 떨어진다.
///
/// # 파일시스템 루트는 거부한다
///
/// `C:/` 같이 부모가 없는 경로는 형제 디렉터리를 만들 수 없다.
/// 그럴때 적당히 루트 안에 두는 대신 오류를 낸다 — 애매하면
/// 실행하지 않는다가 이 저장소의 기본값이다.
fn workload_run_root(checkpoint_root: &std::path::Path) -> Result<PathBuf, String> {
    let absolute = std::path::absolute(checkpoint_root).map_err(|error| {
        format!("checkpoint root 를 절대 경로로 바꿀 수 없다({checkpoint_root:?}): {error}")
    })?;
    let parent = absolute.parent().ok_or_else(|| {
        format!(
            "checkpoint root 가 파일시스템 루트라 작업 디렉터리를 밖에 둘 수 없다({absolute:?})              — 하위 디렉터리를 지정하라"
        )
    })?;
    let name = absolute.file_name().ok_or_else(|| {
        format!("checkpoint root 에서 이름을 얻을 수 없다({absolute:?})")
    })?;
    let mut sibling = name.to_os_string();
    sibling.push(".workload-run");
    Ok(parent.join(sibling))
}

/// 있으면 지우고, 없으면 조용히 넘어간다.
///
/// ★ 삭제 실패를 `let _ =` 로 버리지 않는다(`CLAUDE.md` §3).
///   남의 출력을 못 지우면 그건 알아야 할 사실이다.
/// ★ 결함 ㊷ ㊸ — 앞선 실패를 보고하기 전에 작업 디렉터리를 치우고, 정리까지 실패하면 **둘 다** 남긴다.
fn fail_after_cleanup(primary: String, run_dir: &std::path::Path) -> String {
    match remove_dir_if_present(run_dir) {
        Ok(()) => primary,
        Err(cleanup_error) => format!("{primary} / 그리고 {cleanup_error}"),
    }
}

fn remove_dir_if_present(dir: &std::path::Path) -> Result<(), String> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("작업 출력 디렉터리 삭제 실패({dir:?}): {error}")),
    }
}

/// 자식이 남긴 출력과 실행 결과를 체크포인트에 넣을 바이트로 모은다.
///
/// # 없는 파일을 빈 파일로 둔갑시키지 않는다
///
/// 자식이 아무것도 안 출력하면 `CreateFileW(CREATE_ALWAYS)` 가 만든
/// **빈 파일**이 있다. 그건 "출력이 없었다" 라는 사실이므로 그대로
/// 남긴다. 반면 파일 자체가 **없으면** 캡처를 안 한 경우이므로 목록에서
/// 뺀다 — 둘을 같은 것으로 만들면 관측 결과와 미관측을 구분할 수 없다
/// (`CLAUDE.md` §1 — 모르면 비워 둔다).
fn collect_workload_artifacts(
    run_dir: &std::path::Path,
    spec: &gputeer_protocol::execution_spec::ExecutionSpec,
    outcome: &exec::ExecutionOutcome,
    outputs_captured: bool,
) -> Result<Vec<(String, Vec<u8>)>, (pb::FinalizationFailureStage, String)> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    for name in [exec::STDOUT_FILENAME, exec::STDERR_FILENAME] {
        let path = run_dir.join(name);
        match fs::read(&path) {
            Ok(data) => files.push((name.to_string(), data)),
            // ★ 결함 84 — 캡처를 켰으면 실행기가 이 파일을 만들었다. 없으면 **잃은 것**이다(READ_OUTPUTS).
            //   캡처를 안 켰을 때만 "관측하지 않았다" 로 건너뛴다.
            Err(error) if error.kind() == ErrorKind::NotFound && !outputs_captured => {}
            Err(error) => {
                return Err((
                    pb::FinalizationFailureStage::ReadOutputs,
                    format!("작업 출력 읽기 실패({path:?}): {error}"),
                ));
            }
        }
    }

    // ★ 관측된 사실만 적는다. `peak_commit_bytes` 가
    //   `commit_limit_bytes` 를 넘을 수 있는 것은 결함이 아니라
    //   Job Object 가 소프트 제한이기 때문이다(`ADR-027` 실측).
    //   그래서 값을 가공하지 않고 그대로 남긴다.
    let result = serde_json::json!({
        "job_id": spec.job_id,
        "entrypoint": spec.entrypoint,
        // 코드가 없으면 null — 0 으로 채우지 않는다. 그 사유는 exit_code_unavailable 에(결함 69).
        "exit_code": outcome.exit.code(),
        "exit_code_unavailable": match &outcome.exit {
            exec::ExitObserved::NoCode { detail } => Some(detail.as_str()),
            exec::ExitObserved::Code(_) => None,
        },
        "commit_limit_bytes": outcome.commit_limit_bytes,
        "peak_commit_bytes": outcome.peak_commit_bytes,
        "memory_observation_error": outcome.memory_observation_error,
    });
    let mut result_bytes = serde_json::to_vec_pretty(&result)
        .map_err(|error| {
            (
                pb::FinalizationFailureStage::EncodeResult,
                format!("작업 결과를 JSON 으로 바꾸지 못했다: {error}"),
            )
        })?;
    result_bytes.push(b'\n');
    files.push((WORKLOAD_RESULT_FILENAME.to_string(), result_bytes));

    // 매니페스트 해시는 목록 순서에 의존하므로 정렬해 고정한다.
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

/// 데이터 파일을 쓰고 **마지막에** 매니페스트를 확정한다.
///
/// 순서는 `write_checkpoint()` 가 갖고 있다 — `CLAUDE.md` §0.3 의
/// "매니페스트는 모든 데이터 파일이 확정된 뒤 마지막에 쓴다" 를
/// 이미 구현해둔 경로라 여기서 손으로 다시 짜지 않는다.
///
/// ★ **이 경로가 도달하는 `COMMITTED` 는 복제본 수를 뜻하지 않는다.**
///   `write_checkpoint()` 는 로컬 단일 본을 쓰고 그 상태로 옮긴다.
///   `CLAUDE.md` §0.3 은 durability 정책이 요구하는 replica 수를
///   채워야 `COMMITTED` 라고 정하며, 그 판정은
///   `evaluate_effective_replicas()`(`DoD-54`)가 하고 그 입력을 만드는
///   `ReplicaAck` 전송 경로는 아직 없다. 즉 지금 이 값은
///   **로컬 확정**일 뿐이며, 그 이상을 주장하지 않는다.
fn finalize_workload_checkpoint(
    checkpoint_root: &std::path::Path,
    checkpoint_id: &str,
    lease: &pb::Lease,
    attempt_id: &str,
    files: &[(String, Vec<u8>)],
) -> Result<Option<String>, String> {
    let manifest = manifest_for(
        checkpoint_id,
        &lease.job_id,
        attempt_id,
        0,
        lease.fence_epoch,
        files,
    );
    match write_checkpoint_phased(checkpoint_root, &manifest, files, 0) {
        Ok(_) => Ok(None),
        // ★ 결함 83 · 90 — 파일 해시 검증 뒤의 실패(검증 상태 마커 · LATEST 교체 · COMMITTED 기록)는 산출물 확정 실패가 아니다.
        //   규범의 정상 완료 조건은 HASH_VERIFIED 다. 공개 실패는 checkpoint 쪽 `.publication-failed` 가 재개 후보에서 뺀다.
        Err((WritePhase::Publish, error)) => Ok(Some(format!(
            "체크포인트 공개 실패(checkpoint_id={checkpoint_id}): {error}"
        ))),
        Err((WritePhase::StoreAndVerify, error)) => Err(format!(
            "작업 결과 체크포인트 확정 실패(checkpoint_id={checkpoint_id}): {error}"
        )),
    }
}

/// 작업 실행 결과를 적는 파일 이름.
const WORKLOAD_RESULT_FILENAME: &str = "workload-result.json";

/// Owner Panel 의 CSRF 방어 토큰을 이 Agent 의 씨앨에서 파생한다.
///
/// # 왜 난수가 아니라 파생인가
///
/// 같은 Agent 가 재시작해도 토큰이 같아야 브라우저에 열어둔 패널이
/// 계속 돌아간다. 난수면 재시작마다 소유자가 새로고침해야 하고,
/// 그 사이에 정지 버튼이 안 듣는다.
///
/// ★ **씨씸을 그대로 쓰지 않는다.** 토큰은 화면·HTTP 응답에
///   노출되므로, 그것으로부터 서명키를 역산할 수 없어야 한다.
///   BLAKE3 에 용도 라벨을 붙여 파생한다.
///
/// ★ 이건 **인증이 아니다.** 같은 기계에 로그인한 다른 사용자는
///   이 토큰을 읽을 수 있다. 막려는 것은 브라우저로 열린 남의
///   웹페이지가 소유자 몰래 정지를 누르는 것이다.
fn derive_owner_panel_token(own_seed: &[u8; 32]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gputeer/v1/owner-panel-token");
    hasher.update(own_seed);
    hasher.finalize().to_hex().to_string()
}

fn record_start_checkpoint(
    checkpoint_root: &std::path::Path,
    job_id: &str,
    attempt_id: &str,
    grant_id: &str,
) -> Result<String, String> {
    let checkpoint_id = start_checkpoint_id(job_id, attempt_id, grant_id);
    let checkpoint_dir = checkpoint_root.join(&checkpoint_id);

    fs::create_dir_all(&checkpoint_dir).map_err(|error| {
        format!(
            "Grant 실패: 시작 checkpoint 디렉터리 생성 실패(root={:?}, checkpoint_id={}): {}",
            checkpoint_root, checkpoint_id, error
        )
    })?;
    record_initial_state(&checkpoint_dir).map_err(|error| {
        format!(
            "Grant 실패: WRITING 시작 마커 기록 실패(checkpoint_id={}): {}",
            checkpoint_id, error
        )
    })?;

    Ok(checkpoint_id)
}

/// `DurableFenceWatermark::check_and_advance()` 의 오류를 사람이 읽는
/// 문자열로 바꾼다.
///
/// ★ 코덱스 독립 검수(2026-08-19, p102) 지적 — 전에는 `{e:?}` 하나로
///   뭉뚱그렸다. `Stale`(정책상 정상 거부)과 `Io`/`LockTimeout`(저장소
///   장애)이 **다른 접두사**를 갖지 않으면, negative test 가 진짜
///   epoch 거부를 확인하는지 우연한 저장소 장애를 확인하는지 구분할
///   수 없다 — 후자로도 문자열이 우연히 일치해 시험이 "통과"할 수
///   있다. `Stale` 은 `{prefix}: fence_epoch 검사 실패(정책 거부)` 로
///   시작해 기존 negative test 의 접두사 문자열과 **호환**되고,
///   저장소 장애는 `FENCE_STORAGE_ERROR` 로 명확히 갈라 그 접두사와
///   절대 겹치지 않는다.
fn fence_error_message(prefix: &str, error: DurableFenceError) -> String {
    match error {
        DurableFenceError::Stale(violation) => {
            format!("{prefix}: fence_epoch 검사 실패(정책 거부) — {violation}")
        }
        other => format!("{prefix}: FENCE_STORAGE_ERROR: fence watermark 저장소 오류 — {other}"),
    }
}

fn next_connection_attempt(counter: &mut u32) -> u32 {
    let attempt = *counter;
    *counter = counter.saturating_add(1);
    attempt
}

fn connect_with_timeout(address: &str, timeout: Duration) -> Result<TcpStream, String> {
    let socket = address
        .to_socket_addrs()
        .map_err(|e| format!("Coordinator address parse failed: {e}"))?
        .next()
        .ok_or_else(|| "Coordinator address resolved to no socket addresses".to_string())?;
    TcpStream::connect_timeout(&socket, timeout).map_err(|e| {
        if matches!(
            e.kind(),
            ErrorKind::ConnectionRefused
                | ErrorKind::TimedOut
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::BrokenPipe
                | ErrorKind::NotConnected
        ) {
            format!("RETRYABLE_CONNECTION: Coordinator connect failed: {e}")
        } else {
            format!("Coordinator connect failed: {e}")
        }
    })
}

fn classify_framing_error(error: FramingError, phase: &str) -> String {
    let retryable = match &error {
        FramingError::Truncated => true,
        FramingError::Io(io) => matches!(
            io.kind(),
            ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::BrokenPipe
                | ErrorKind::NotConnected
                | ErrorKind::TimedOut
                | ErrorKind::UnexpectedEof
        ),
        FramingError::FrameTooLarge { .. }
        | FramingError::UnknownFrameType(_)
        | FramingError::Verify(_) => false,
    };
    if retryable {
        format!("RETRYABLE_CONNECTION: {phase} 프레임 읽기/검증 실패: {error}")
    } else {
        format!("{phase} 프레임 읽기/검증 실패: {error}")
    }
}

fn classify_renew_result_error(error: FramingError) -> String {
    let detail = error.to_string();
    match error {
        FramingError::Truncated | FramingError::Io(_) => {
            format!("AMBIGUOUS_RENEW: RenewLeaseResult was not received: {detail}")
        }
        other => format!("RenewLeaseResult 프레임 읽기/검증 실패: {other}"),
    }
}

/// 서명된 `AgentGrantAck` 를 만들어 보낸다.
///
/// ★ 결함 ⑱ (설계 A) — 워크로드가 있으면 **사전 관문 뒤 · 실행 전**에, 없으면 Grant 처리 끝에
///   부른다. 한 연결에서 한 번만 부른다 — 두 번 보내면 Coordinator 가 두 번째를 다음 프레임으로
///   읽으려다 실패한다.
fn send_grant_ack(
    stream: &mut TcpStream,
    signing_key: &SigningKey,
    grant: &pb::ExecutionGrant,
    config: &AgentConfig,
    clock: &SystemClock,
) -> Result<pb::AgentGrantAck, String> {
    let now = clock.now_unix_ms();
    let mut ack = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: grant.grant_id.clone(),
        attempt_id: grant.attempt_id.clone(),
        agent_device_id: config.agent_device_id.clone(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_nonce("grant-ack", &grant.grant_id, config.connection_attempt),
        accepted: true,
        ..Default::default()
    };
    ack.agent_signature = sign(signing_key, &ack).to_vec();

    if config.corrupt_own_signature {
        let last = ack
            .agent_signature
            .last_mut()
            .ok_or_else(|| "agent_signature 가 비어 있다".to_string())?;
        *last ^= 0x01;
    }

    let frame = write_frame(FrameType::GrantAck, &ack.encode_to_vec())
        .map_err(|e| format!("ACK 프레임 인코딩 실패: {e}"))?;
    stream
        .write_all(&frame)
        .map_err(|e| format!("ACK 전송 실패: {e}"))?;
    stream.flush().map_err(|e| e.to_string())?;
    // ★ 기동 전 ACK 를 테스트가 직접 관측하는 표지(결함 ㊻) — WORKLOAD_SPAWNED 보다 먼저 찍혀야 한다.
    println!("ACK_SENT grant_id={} attempt_id={}", ack.grant_id, ack.attempt_id);
    Ok(ack)
}

/// A coordinator that deliberately drops the transport after ACK is detected
/// without adding a protocol message. A normal coordinator remains readable,
/// so the short peek times out and the completed handshake is preserved.
fn peer_closed_after_ack(stream: &TcpStream) -> Result<bool, String> {
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|e| e.to_string())?;
    let mut byte = [0u8; 1];
    match stream.peek(&mut byte) {
        Ok(0) => Ok(true),
        Ok(_) => Ok(false),
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
            Ok(false)
        }
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::BrokenPipe
                    | ErrorKind::NotConnected
            ) =>
        {
            Ok(true)
        }
        Err(error) => Err(error.to_string()),
    }
}

/// `crates/coordinator/src/lib.rs::derive_nonce` 와 같은 방식 —
/// 결정적 유도로 재현 가능한 selftest 시나리오를 만든다. `tag` 로
/// Grant nonce 와 네임스페이스를 분리한다(같은 grant_id 라도
/// Coordinator->Agent 방향과 Agent->Coordinator 방향의 nonce 가 같아지면
/// `nonce_namespace_is_per_device` 가 보장하는 sender 별 분리에 기대게
/// 되어 이 stub 자체의 nonce 선택이 우연히 안전해 보일 수 있다).
///
/// ★ **운영 코드는 이 패턴을 쓰면 안 된다.** 같은 `grant_id` 로 다시
/// 부르면 같은 nonce 가 나온다 — CSPRNG 가 아니라 결정적 해시이기
/// 때문이다. 이 stub 이 안전한 이유는 매 selftest 실행이 새 OS
/// 프로세스·새 `InMemoryReplayGuard` 를 쓰기 때문이다(실행 간 replay
/// 상태가 없다). `DurableReplayGuard` 처럼 재시작을 견디는 저장소와
/// 이 nonce 선택을 같이 쓰면, 재시작 후 같은 `grant_id` 를 다시
/// 발급했을 때 정당한 새 Grant 가 예전 nonce 와 충돌해 `Duplicate`
/// 로 오판될 수 있다(코덱스 독립 검수 2026-08-18 지적).
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


/// 반복 Lease 갱신(2026-08-19,
/// `docs/plans/2026-08-19_2330_같은_연결_반복_lease_갱신_v1.md`) 전용
/// nonce 유도 — `lease_id` 만으로는 회차마다 같은 값이 나와
/// replay guard 가 두 번째 요청을 `Duplicate` 로 거부한다(코덱스
/// 설계 `p105` 가 코드 경로로 확정한 결함). `round` 를 입력에 섞어
/// 회차별로 분리한다.
///
/// ★ `derive_nonce()` 와 별도 함수로 둔다 — 기존 호출부(grant-ack 등)
///   의 nonce 유도 방식을 바꾸지 않기 위해서다.
fn derive_renew_nonce(lease_id: &str, connection_attempt: u32, round: u64) -> Vec<u8> {
    let mut input = Vec::with_capacity(b"lease-renew".len() + 1 + lease_id.len() + 1 + 4 + 1 + 8);
    input.extend_from_slice(b"lease-renew");
    input.push(0);
    input.extend_from_slice(lease_id.as_bytes());
    input.push(0);
    if connection_attempt != 0 {
        input.extend_from_slice(&connection_attempt.to_be_bytes());
        input.push(0);
    }
    input.extend_from_slice(&round.to_be_bytes());
    gputeer_protocol::canonical::blake3_256(&input)[..16].to_vec()
}

/// `--flag value` 쌍으로 이루어진 CLI 인자를 설정으로 바꾸기만 한다 —
/// lane 선택도 관문도 여기서 하지 않는다.
///
/// ★ `run_from_args` 에서 떼어냈다(독립 검수 7라운드). 관문이 실제로
///   각 진입점에 있는지 재려면, 라이브러리 호출자처럼 설정을 만들어
///   진입점을 **직접** 부를 수 있어야 한다.
pub fn parse_config_from_args(args: &[String]) -> Result<AgentConfig, String> {
    let flags = parse_flags(args)?;

    let config = AgentConfig {
        coordinator_addr: flags.require("--connect")?,
        own_seed: hex_to_seed(&flags.require("--own-seed")?)?,
        coordinator_verifying_key: hex_to_verifying_key(&flags.require("--peer-pubkey")?)?,
        // 안 주면 None — Manifest 가 실려 오면 fail closed 로 거부한다.
        submitter_verifying_key: match flags.get("--submitter-pubkey") {
            Some(hex) => Some(hex_to_verifying_key(hex)?),
            None => None,
        },
        execute_workload: flags.bool_flag("--i-understand-this-executes-untrusted-code"),
        send_attempt_report: flags.bool_flag("--send-attempt-report"),
        corrupt_heartbeat_fence: flags.bool_flag("--corrupt-heartbeat-fence"),
        corrupt_heartbeat_coordinator: flags.bool_flag("--corrupt-heartbeat-coordinator"),
        corrupt_heartbeat_device: flags.bool_flag("--corrupt-heartbeat-device"),
        corrupt_hello_mode: flags.bool_flag("--corrupt-hello-mode"),
        workload_cgroup_parent: flags.get("--workload-cgroup-parent")
            .map(std::path::PathBuf::from),
        multi_agent: flags.bool_flag("--multi-agent"),
        // ★ 파싱 실패를 0 으로 접지 않는다(2026-08-30 독립 검수 3라운드).
        //   잘못 쓴 값이 "간격 없음" 으로 조용히 바뀌면, 운영자는 간격을
        //   줬다고 믿는데 실제로는 안 준 상태가 된다.
        heartbeat_interval_ms: match flags.checked_get::<u64>("--heartbeat-interval-ms")? {
            Some(raw) => raw.parse::<u64>().map_err(|_| {
                format!("--heartbeat-interval-ms 를 숫자로 읽지 못했다: {raw:?}")
            })?,
            None => 0,
        },
        heartbeat_rounds: match flags.checked_get::<u32>("--heartbeat-rounds")? {
            Some(v) => v
                .parse()
                .map_err(|e| format!("--heartbeat-rounds 파싱 실패: {e}"))?,
            None => 0,
        },
        neighbor_report_rounds: match flags.checked_get::<u32>("--neighbor-report-rounds")? {
            Some(v) => v
                .parse()
                .map_err(|e| format!("--neighbor-report-rounds 파싱 실패: {e}"))?,
            None => 0,
        },
        // ★ heartbeat 간격과 같은 이유로 **조용히 0 으로 떨어뜨리지 않는다** —
        //   잘못 쓴 값이 "간격 없음" 이 되면 운영자는 간격을 줬다고 믿는데
        //   실제로는 안 준 상태가 된다.
        neighbor_report_interval_ms: match flags.checked_get::<u64>("--neighbor-report-interval-ms")? {
            Some(raw) => raw.parse::<u64>().map_err(|_| {
                format!("--neighbor-report-interval-ms 를 숫자로 읽지 못했다: {raw:?}")
            })?,
            None => 0,
        },
        neighbor_report_target_node_id: flags.get("--neighbor-report-target").cloned(),
        corrupt_neighbor_report_coordinator: flags
            .bool_flag("--corrupt-neighbor-report-coordinator"),
        owner_panel_state: owner_panel::OwnerPanelState::new(),
        owner_panel_port: flags.checked_get::<u16>("--owner-panel-port")?.map(|v| v.parse::<u16>()).transpose().map_err(|e| format!("--owner-panel-port 파싱 실패: {e}"))?,
        // 기본 256MiB. Job Object 커밋 상한이라 VRAM 은 대략
        // `RAM 상한 - 2000MiB` 로 간접 제한된다(ADR-027) — 이 값은
        // 실행 자체를 증명하기 위한 최소값이고 정책이 아니다.
        workload_commit_limit_bytes: flags
            .u64_opt_flag("--workload-commit-limit-bytes")?
            .unwrap_or(256 * 1024 * 1024),
        coordinator_device_id: flags.require("--coordinator-device-id")?,
        agent_device_id: flags.require("--agent-device-id")?,
        corrupt_own_signature: flags.bool_flag("--corrupt-own-signature"),
        expect_replay: flags.bool_flag("--expect-replay"),
        do_renew: flags.bool_flag("--do-renew"),
        renew_request_epoch_override: flags.u64_opt_flag("--renew-request-epoch-override")?,
        corrupt_renew_request_signature: flags.bool_flag("--corrupt-renew-request-signature"),
        fence_db_path: match flags.get("--fence-db") {
            Some(v) => PathBuf::from(v),
            None => default_fence_db_path(),
        },
        checkpoint_root: match flags.get("--checkpoint-root") {
            Some(v) => PathBuf::from(v),
            None => default_checkpoint_root(),
        },
        renew_rounds: flags.u32_flag_with_default("--renew-rounds", 1)?,
        renew_delay_ms: flags.u64_flag_with_default("--renew-delay-ms", 0)?,
        renew_during_execution_ms: flags.u64_flag_with_default("--renew-during-execution-ms", 0)?,
        expect_revoke_after_round: flags.u32_opt_flag("--expect-revoke-after-round")?,
        revoke_signer_id_override: flags.get("--revoke-signer-id").cloned(),
        max_reconnect_attempts: flags.u32_flag_with_default("--max-reconnect-attempts", 8)?,
        max_reconnect_duration_seconds: flags
            .u64_flag_with_default("--max-reconnect-duration-seconds", 60)?,
        retry_base_ms: flags.u64_flag_with_default("--retry-base-ms", 250)?,
        retry_cap_ms: flags.u64_flag_with_default("--retry-cap-ms", 5_000)?,
        connection_attempt: 0,
        reconnect_enabled: !flags.bool_flag("--disable-reconnect"),
        recover_ambiguous_renew_from_durable_lease: flags
            .bool_flag("--recover-ambiguous-renew-from-durable-lease"),
        drop_after_renew_request_once: flags.bool_flag("--drop-after-renew-request-once"),
        reuse_renew_nonce_after_reconnect: flags.bool_flag("--reuse-renew-nonce-after-reconnect"),
        resume_protocol: flags.bool_flag("--resume-protocol"),
        session_id: flags.get("--session-id")
            .cloned()
            .unwrap_or_else(|| "resume-session".into()),
        resume_lease_id: flags.get("--resume-lease-id")
            .cloned()
            .unwrap_or_default(),
        resume_job_id: flags.get("--resume-job-id").cloned().unwrap_or_default(),
        resume_attempt_id: flags.get("--resume-attempt-id")
            .cloned()
            .unwrap_or_default(),
        resume_fence_epoch: flags.u64_flag_with_default("--resume-fence-epoch", 0)?,
    };

    // ★ 결함 ⑯ 확장(2026-09-10) — 모르는 이름을 받아 두지 않는다.
    let bad_bools = flags.bad_bools();
    if !bad_bools.is_empty() {
        return Err(format!(
            "STARTUP_REFUSED: INVALID_BOOL — {bad_bools:?} 는 true 도 false 도 아니다. \
             조용히 false 로 읽으면 켠 줄 안 것이 안 켜진다"
        ));
    }
    let unread = flags.unread();
    if !unread.is_empty() {
        return Err(format!(
            "STARTUP_REFUSED: UNKNOWN_FLAGS — 이 명령이 읽지 않는 플래그 {unread:?}. \
             오타이거나 없는 설정이다 — 받아 두면 말없이 버려진다"
        ));
    }
    Ok(config)
}

/// CLI 인자를 [`parse_config_from_args`] 로 설정으로 바꾼 뒤 실행한다.
/// 파싱 자체는 이 함수가 하지 않는다.
///
/// `crates/coordinator/src/lib.rs::run_from_args` 와 같은 이유로
/// `crates/cli` 대신 여기(Agent 스트림)가 인자 의미를 정의한다.
///
/// ★ 이 함수 자체에는 관문이 없다 — 설정을 [`run`] 에 넘길 뿐이고
///   관문은 거기에 있다. 따라서 selftest 도 `run()` 의 관문은 실제로
///   실행한다. 라이브러리 호출자에게만 보이는 것은 multi-agent lane
///   진입점의 관문이다.
pub fn run_from_args(args: &[String]) -> Result<(), String> {
    let config = parse_config_from_args(args)?;

    run(config)
}

/// `--fence-db` 를 안 주면 매 프로세스마다 고유한 임시 SQLite 파일을
/// 만든다 — 기존(재시작 시나리오가 아닌) selftest 시나리오는 항상
/// 빈 watermark 에서 시작해야 하므로, PID 를 재사용해도 안전하도록
/// 발급 시각(나노초)도 섞는다.
fn default_fence_db_path() -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("gputeer-fence-{pid}-{nanos}.sqlite3"))
}

/// `--checkpoint-root`를 생략한 기존 호출도 안전하게 동작하도록
/// 프로세스별 임시 root를 만든다. selftest가 root를 검사해야 하는 경우에는
/// 명시적인 `--checkpoint-root`를 전달한다.
fn default_checkpoint_root() -> PathBuf {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("gputeer-checkpoints-{pid}-{nanos}"))
}

/// ★ 두 번째 칸은 **읽은 키**다. 설정을 다 만든 뒤 한 번도 안 읽힌 키는 이
///   명령이 모르는 이름이다 — Coordinator 쪽과 같은 이유(결함 ⑯ 확장,
///   2026-09-10). 전에는 오타가 조용히 사라졌다.
struct Flags(
    std::collections::HashMap<String, String>,
    std::cell::RefCell<std::collections::HashSet<String>>,
    // ★ 세 번째 칸 — `true`/`false` 가 아닌 불리언 값. `--x tru` 가 조용히
    //   false 가 되면 부정 테스트가 무력화된다(재검수 15, 결함 ㉑).
    std::cell::RefCell<Vec<String>>,
    // ★ 네 번째 칸 — 같은 키를 두 번 줬을 때 **덮어써진 앞 값**(결함 ㉒,
    //   재검수 18). 마지막 값을 쓰는 규칙은 그대로 둔다(selftest 34 가 쓴다).
    //   다만 `--x tru --x false` 의 `tru` 가 검사 전에 사라지면 안 된다.
    Vec<(String, String)>,
);

impl Flags {
    /// 값을 읽고 **읽었다고 적는다.**
    fn get(&self, key: &str) -> Option<&String> {
        self.1.borrow_mut().insert(key.to_string());
        self.0.get(key)
    }

    /// 한 번도 읽히지 않은 키 — 이름순.
    fn unread(&self) -> Vec<String> {
        let read = self.1.borrow();
        let mut keys: Vec<String> = self
            .0
            .keys()
            .filter(|key| !read.contains(*key))
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    fn require(&self, key: &str) -> Result<String, String> {
        self.get(key)
            .cloned()
            .ok_or_else(|| format!("필수 인자 누락: {key}"))
    }

    /// `crates/coordinator/src/lib.rs::Flags::bool_flag` 와 동일 — 값이
    /// 있는 boolean 플래그(`--flag true`). 안 주면 `false`.
    fn bool_flag(&self, key: &str) -> bool {
        for (_, earlier) in self.3.iter().filter(|(k, _)| k == key) {
            if earlier != "true" && earlier != "false" {
                self.2.borrow_mut().push(format!("{key}={earlier}"));
            }
        }
        match self.get(key).map(String::as_str) {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => {
                self.2.borrow_mut().push(format!("{key}={other}"));
                false
            }
        }
    }

    /// ★ 결함 ㉞ — 같은 키를 두 번 줬을 때 **덮어써진 앞 값**도 같은 타입으로 읽어 본다.
    ///   마지막 값을 쓰는 규칙은 그대로다. 앞 값이 잘못됐으면 거부한다 — 뒤의 중복에
    ///   가려 조용히 사라지면 오타를 모른다(㉒ 가 불리언에서 막은 것과 같은 모양).
    fn earlier_must_parse<T: std::str::FromStr>(&self, key: &str) -> Result<(), String>
    where
        T::Err: std::fmt::Display,
    {
        for (_, earlier) in self.3.iter().filter(|(k, _)| k == key) {
            earlier.parse::<T>().map_err(|e| {
                format!("{key} 앞 값 {earlier:?} 파싱 실패: {e} — 같은 키를 두 번 줬다. 앞 값도 검사한다")
            })?;
        }
        Ok(())
    }

    /// ★ 결함 ㊲ — 숫자 읽기 함수를 안 거치고 **직접** parse 하는 자리용. 앞 값을 먼저 본다.
    ///   (구현 검수 41 이 다섯 곳을 찾았다 — ㉞ 가 숫자 읽기 함수만 고쳤다)
    fn checked_get<T: std::str::FromStr>(&self, key: &str) -> Result<Option<&String>, String>
    where
        T::Err: std::fmt::Display,
    {
        self.earlier_must_parse::<T>(key)?;
        Ok(self.get(key))
    }

    /// `true`/`false` 가 아니었던 불리언 값들.
    fn bad_bools(&self) -> Vec<String> {
        self.2.borrow().clone()
    }

    /// 반복 Lease 갱신(2026-08-19) — 안 주면 `default`(왕복 횟수).
    /// 기본값 1은 기존 단일 왕복 시나리오와 동일하게 동작한다.
    fn u32_flag_with_default(&self, key: &str, default: u32) -> Result<u32, String> {
        self.earlier_must_parse::<u32>(key)?;
        match self.get(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<u32>()
                .map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }

    /// ★ 테스트 전용 — 갱신 요청 epoch 강제 주입(단계 5). 안 주면
    ///   `None`(보유 중인 Lease 의 실제 epoch 을 그대로 쓴다).
    fn u64_flag_with_default(&self, key: &str, default: u64) -> Result<u64, String> {
        self.earlier_must_parse::<u64>(key)?;
        match self.get(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<u64>()
                .map_err(|e| format!("{key} parse failed: {e}")),
        }
    }

    fn u64_opt_flag(&self, key: &str) -> Result<Option<u64>, String> {
        self.earlier_must_parse::<u64>(key)?;
        match self.get(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<u64>()
                .map(Some)
                .map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }

    fn u32_opt_flag(&self, key: &str) -> Result<Option<u32>, String> {
        self.earlier_must_parse::<u32>(key)?;
        match self.get(key) {
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
    let mut earlier: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let key = &args[i];
        if !key.starts_with("--") {
            return Err(format!("플래그가 아닌 인자: {key}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} 뒤에 값이 없다"))?;
        if let Some(previous) = map.insert(key.clone(), value.clone()) {
            earlier.push((key.clone(), previous));
        }
        i += 2;
    }
    Ok(Flags(map, Default::default(), Default::default(), earlier))
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
    // ★ 결함 ⑬(2026-09-10) — 바이트로 자르지 않는다. `gputeer_crypto::hex`
    //   가 한 바이트씩 읽으므로 문자 경계를 가를 수 없다.
    gputeer_crypto::hex::decode_even(hex).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Instant;

    #[test]
    fn nonce_attempt_counter_ignores_failed_connects() {
        let coordinator_seed = [0x31u8; 32];
        let agent_seed = [0x41u8; 32];
        let coordinator_key = SigningKey::from_bytes(&coordinator_seed);
        let agent_key = SigningKey::from_bytes(&agent_seed);
        let coordinator_device_id = "01JTESTCOORDINATOR00000001";
        let agent_device_id = "01JTESTAGENT00000000000001";
        let grant_id = "01JTESTGRANT00000000000001";
        let attempt_id = "01JTESTATTEMPT000000000001";
        let lease_id = "01JTESTLEASE00000000000001";
        let job_id = "01JTESTJOB000000000000001";

        // Close the ephemeral listener before starting Agent. This gives us a
        // known loopback address which initially refuses TCP connections.
        let reserved = TcpListener::bind("127.0.0.1:0").expect("reserve loopback port");
        let address = reserved.local_addr().expect("read reserved address");
        drop(reserved);
        let refused = TcpStream::connect_timeout(&address, Duration::from_millis(100));
        assert!(
            refused.is_err(),
            "test setup must provide a refused TCP port"
        );

        let tempdir = tempfile::tempdir().expect("create agent test directory");
        let fence_path = tempdir.path().join("fence.sqlite3");
        let agent_config = AgentConfig {
            coordinator_addr: address.to_string(),
            own_seed: agent_seed,
            coordinator_verifying_key: coordinator_key.verifying_key(),
            submitter_verifying_key: None,
            execute_workload: false,
            send_attempt_report: false,
            heartbeat_rounds: 0,
            heartbeat_interval_ms: 0,
            neighbor_report_rounds: 0,
            neighbor_report_interval_ms: 0,
            neighbor_report_target_node_id: None,
            corrupt_neighbor_report_coordinator: false,
            corrupt_heartbeat_fence: false,
            corrupt_heartbeat_coordinator: false,
            corrupt_heartbeat_device: false,
            corrupt_hello_mode: false,
            workload_cgroup_parent: None,
            multi_agent: false,
            owner_panel_state: owner_panel::OwnerPanelState::new(),
            owner_panel_port: None,
            workload_commit_limit_bytes: 256 * 1024 * 1024,
            coordinator_device_id: coordinator_device_id.into(),
            agent_device_id: agent_device_id.into(),
            corrupt_own_signature: false,
            expect_replay: false,
            do_renew: false,
            renew_request_epoch_override: None,
            corrupt_renew_request_signature: false,
            fence_db_path: fence_path.clone(),
            checkpoint_root: tempdir.path().join("checkpoints"),
            renew_rounds: 1,
            renew_delay_ms: 0,
            renew_during_execution_ms: 0,
            expect_revoke_after_round: None,
            revoke_signer_id_override: None,
            max_reconnect_attempts: 1_000,
            max_reconnect_duration_seconds: 30,
            retry_base_ms: 25,
            retry_cap_ms: 100,
            connection_attempt: 0,
            reconnect_enabled: true,
            recover_ambiguous_renew_from_durable_lease: false,
            drop_after_renew_request_once: false,
            reuse_renew_nonce_after_reconnect: false,
            resume_protocol: false,
            session_id: "test-session".into(),
            resume_lease_id: String::new(),
            resume_job_id: String::new(),
            resume_attempt_id: String::new(),
            resume_fence_epoch: 0,
        };

        let agent_thread = thread::spawn(move || run(agent_config));

        // Fence DB creation is the last production initialization step before
        // Agent enters its connect/retry loop. Wait for that observable event,
        // then leave the port refused long enough to force real failed
        // connect() calls before opening the successful retry listener.
        let setup_deadline = Instant::now() + Duration::from_secs(5);
        while !fence_path.exists() {
            assert!(
                Instant::now() < setup_deadline,
                "Agent did not initialize its fence DB"
            );
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_secs(5));
        let listener = TcpListener::bind(address).expect("bind successful retry listener");
        listener
            .set_nonblocking(true)
            .expect("make retry listener nonblocking");

        let accept_deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < accept_deadline,
                        "Agent never reached successful TCP connect"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept successful retry connection: {error}"),
            }
        };
        stream
            .set_nonblocking(false)
            .expect("make accepted test stream blocking");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set test stream read timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .expect("set test stream write timeout");

        let now = SystemClock.now_unix_ms();
        let mut lease = pb::Lease {
            schema_version: 1,
            lease_id: lease_id.into(),
            job_id: job_id.into(),
            attempt_id: attempt_id.into(),
            fence_epoch: 1,
            coordinator_term: 1,
            holder_node_id: agent_device_id.into(),
            member_node_ids: vec![agent_device_id.into()],
            issuing_coordinator_id: coordinator_device_id.into(),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + 60_000,
            renew_after_unix_ms: now + 30_000,
            max_total_duration_seconds: 86_400,
            ..Default::default()
        };
        lease.coordinator_signature = sign(&coordinator_key, &lease).to_vec();

        let mut grant = pb::ExecutionGrant {
            schema_version: 1,
            grant_id: grant_id.into(),
            attempt_id: attempt_id.into(),
            coordinator_device_id: coordinator_device_id.into(),
            issued_at_unix_ms: now,
            expires_at_unix_ms: now + 60_000,
            nonce: derive_nonce("grant", grant_id, 0),
            lease: Some(lease),
            ..Default::default()
        };
        grant.coordinator_signature = sign(&coordinator_key, &grant).to_vec();

        // D2 — 실제 Agent 는 Grant 를 받기 전에 Hello(FRESH) 를 보낸다.
        let mut hello_header = [0u8; 5];
        std::io::Read::read_exact(&mut stream, &mut hello_header).expect("Agent Hello header");
        assert_eq!(hello_header[0], FrameType::SessionHello as u8, "첫 프레임은 Hello 다(D2)");
        let hello_len = u32::from_be_bytes([hello_header[1], hello_header[2], hello_header[3], hello_header[4]]) as usize;
        let mut hello_body = vec![0u8; hello_len];
        std::io::Read::read_exact(&mut stream, &mut hello_body).expect("Agent Hello body");

        let frame =
            write_frame(FrameType::Grant, &grant.encode_to_vec()).expect("encode test Grant frame");
        stream.write_all(&frame).expect("send test Grant");
        stream.flush().expect("flush test Grant");
        assert_eq!(grant.nonce, derive_nonce("grant", grant_id, 0));

        let mut agent_keys = InMemoryKeyring::new();
        agent_keys.insert(agent_device_id, agent_key.verifying_key());
        let mut replay = InMemoryReplayGuard::new();
        let received_result = read_frame(
            &mut stream,
            1,
            KeyDirectorySource::Provided(&agent_keys),
            &mut replay,
            &SystemClock,
        );

        // Keep the accepted stream alive until Agent observes a live peer;
        // otherwise the test would turn a successful handshake into a
        // deliberate reconnect case.
        let agent_result = agent_thread.join().expect("Agent thread did not panic");
        let received = received_result.unwrap_or_else(|error| {
            panic!("receive ACK from production Agent run: {error}; Agent result: {agent_result:?}")
        });
        let ack = match received {
            IngressMessage::GrantAck(verified) => verified
                .require_replay_checked()
                .expect("ACK replay check")
                .clone(),
            other => panic!("expected GrantAck, got {other:?}; Agent result: {agent_result:?}"),
        };
        assert_eq!(ack.nonce, derive_nonce("grant-ack", grant_id, 0));
        assert!(
            agent_result.is_ok(),
            "production Agent run failed: {agent_result:?}"
        );
    }

    fn held_lease() -> pb::Lease {
        pb::Lease {
            lease_id: "lease-a".into(),
            fence_epoch: 7,
            expires_at_unix_ms: 10_000,
            ..Default::default()
        }
    }

    fn revoke(lease_id: &str, fence_epoch: u64) -> pb::RevokeLeaseNotice {
        pb::RevokeLeaseNotice {
            lease_id: lease_id.into(),
            fence_epoch,
            ..Default::default()
        }
    }

    #[test]
    fn revoke_notice_matching_lease_and_epoch_is_applicable() {
        assert!(validate_revoke_notice(&held_lease(), &revoke("lease-a", 7), 9_999).is_ok());
    }

    #[test]
    fn revoke_notice_with_wrong_lease_id_is_rejected() {
        let result = validate_revoke_notice(&held_lease(), &revoke("lease-b", 7), 9_999);
        assert!(result.unwrap_err().contains("lease_id 불일치"));
    }

    #[test]
    fn revoke_notice_with_wrong_fence_epoch_is_rejected() {
        let result = validate_revoke_notice(&held_lease(), &revoke("lease-a", 6), 9_999);
        assert!(result.unwrap_err().contains("fence_epoch 불일치"));
    }

    #[test]
    fn revoke_notice_for_expired_lease_is_rejected() {
        let result = validate_revoke_notice(&held_lease(), &revoke("lease-a", 7), 10_000);
        assert!(result.unwrap_err().contains("이미 만료됐다"));
    }

    /// 독립 검수가 든 반례들이 전부 체크포인트 루트 **밖**으로
    /// 떨어지는가.
    ///
    /// ★ 초안은 경로 문자열 끝에 접미사를 붙였고, 그러면 후행
    ///   구분자나 `.` 이 들어오는 순간 결과가 루트 **안**으로
    ///   떨어졌다 — 정확히 막으려던 것이 다시 생긴다.
    ///   반례를 테스트로 고정해 다시 돌아오지 못하게 한다.
    #[test]
    fn workload_run_root_is_never_inside_the_checkpoint_root() {
        for raw in [
            "C:/data/checkpoints",
            "C:/data/checkpoints/",
            ".",
            "./cp",
            "cp/",
        ] {
            let root = std::path::Path::new(raw);
            let run_root = workload_run_root(root)
                .unwrap_or_else(|error| panic!("{raw:?} 에서 작업 루트를 못 만들었다: {error}"));
            let absolute_root = std::path::absolute(root).expect("절대 경로");
            assert!(
                !run_root.starts_with(&absolute_root),
                "{raw:?} 의 작업 루트가 체크포인트 루트 안에 생겼다 —                  startup_gc 가 이걸 체크포인트로 오인한다: {run_root:?} ⊂ {absolute_root:?}"
            );
            assert_eq!(
                run_root.parent(),
                absolute_root.parent(),
                "{raw:?} 의 작업 루트가 형제가 아니다"
            );
        }
    }

    /// 파일시스템 루트는 거부한다.
    ///
    /// 형제 디렉터리를 만들 수 없는 경로에서 적당히 루트 안에 두면
    /// 위의 불변식이 조용히 깨진다.
    #[test]
    fn filesystem_root_as_checkpoint_root_is_refused() {
        let root = if cfg!(windows) { "C:/" } else { "/" };
        let error = workload_run_root(std::path::Path::new(root))
            .expect_err("파일시스템 루트가 받아들여졌다");
        assert!(
            error.contains("파일시스템 루트"),
            "거부 이유가 분명하지 않다: {error}"
        );
    }
}

/// 결함 ㊷ ㊸ ㊺ (구현 검수 49) 의 단위 테스트.
#[cfg(test)]
mod defect_19_tests {
    //! 결함 ⑲ — 산출물 확정 실패가 **어느 단계**인지 가른다. 단계가 틀리면 보고의 outcome(6 · FAILED)과 단계 값이 틀린다.
    use super::*;
    use std::collections::BTreeMap;

    fn spec() -> gputeer_protocol::execution_spec::ExecutionSpec {
        gputeer_protocol::execution_spec::ExecutionSpec {
            job_id: "job-19".into(),
            entrypoint: "prog".into(),
            args: Vec::new(),
            env_vars: BTreeMap::new(),
        }
    }

    fn outcome() -> exec::ExecutionOutcome {
        exec::ExecutionOutcome {
            exit: exec::ExitObserved::Code(0),
            commit_limit_bytes: 1,
            peak_commit_bytes: None,
            memory_observation_error: None,
        }
    }

    fn lease() -> pb::Lease {
        pb::Lease {
            job_id: "job-19".into(),
            fence_epoch: 3,
            ..Default::default()
        }
    }

    /// stdout 자리에 **디렉터리**가 있으면 읽기가 NotFound 가 아닌 오류로 실패한다 -> READ_OUTPUTS.
    #[test]
    fn an_unreadable_output_is_read_outputs() {
        let run = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(run.path().join(exec::STDOUT_FILENAME)).unwrap();
        let error = finalize_workload_outputs(run.path(), &spec(), &outcome(), true, root.path(), "ckpt-19", &lease(), "attempt-19")
            .expect_err("디렉터리를 파일로 읽을 수 없다");
        assert_eq!(error.0, pb::FinalizationFailureStage::ReadOutputs, "{}", error.1);
    }

    /// 체크포인트 디렉터리 자리에 **파일**이 있으면 확정이 실패한다 -> COMMIT_CHECKPOINT.
    #[test]
    fn a_blocked_checkpoint_is_commit_checkpoint() {
        let run = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("ckpt-19"), b"not a directory").unwrap();
        // 캡처한 출력은 있어야 한다 — 없으면 결함 84 규칙(READ_OUTPUTS)이 먼저 걸려 확정 단계까지 가지 않는다.
        std::fs::write(run.path().join(exec::STDOUT_FILENAME), b"hello").unwrap();
        std::fs::write(run.path().join(exec::STDERR_FILENAME), b"").unwrap();
        let error = finalize_workload_outputs(run.path(), &spec(), &outcome(), true, root.path(), "ckpt-19", &lease(), "attempt-19")
            .expect_err("파일 위에 체크포인트를 만들 수 없다");
        assert_eq!(error.0, pb::FinalizationFailureStage::CommitCheckpoint, "{}", error.1);
    }

    /// 대조 — 막힌 곳이 없으면 확정이 성공하고 파일 목록을 돌려준다(위 두 테스트가 무조건 실패하는 fixture 가 아님을 확인).
    #[test]
    fn an_unblocked_run_finalizes() {
        let run = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(run.path().join(exec::STDOUT_FILENAME), b"hello").unwrap();
        std::fs::write(run.path().join(exec::STDERR_FILENAME), b"").unwrap();
        let (files, publish_failure) = finalize_workload_outputs(run.path(), &spec(), &outcome(), true, root.path(), "ckpt-19", &lease(), "attempt-19")
            .expect("막힌 곳이 없으면 확정한다");
        assert!(files.iter().any(|(name, _)| name == exec::STDOUT_FILENAME));
        assert!(publish_failure.is_none());
    }

    /// 결함 84 — 캡처를 켰는데 출력 파일이 **없으면** 잃은 것이다 -> READ_OUTPUTS.
    #[test]
    fn a_lost_captured_output_is_read_outputs() {
        let run = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let error = finalize_workload_outputs(run.path(), &spec(), &outcome(), true, root.path(), "ckpt-19", &lease(), "attempt-19")
            .expect_err("캡처했는데 출력이 없으면 확정하지 않는다");
        assert_eq!(error.0, pb::FinalizationFailureStage::ReadOutputs, "{}", error.1);
    }

    /// 대조 — 캡처를 안 켰으면 출력 파일 부재는 실패가 아니다(관측하지 않았다).
    #[test]
    fn an_uncaptured_missing_output_is_not_a_failure() {
        let run = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        finalize_workload_outputs(run.path(), &spec(), &outcome(), false, root.path(), "ckpt-19", &lease(), "attempt-19")
            .expect("캡처하지 않은 출력은 요구하지 않는다");
    }

    /// 결함 83 — LATEST 자리를 막아 **해시 검증 뒤** 공개가 실패하면 확정 실패가 아니다(보고는 COMPLETED).
    #[test]
    fn a_publish_failure_after_hash_verification_is_not_a_finalization_failure() {
        let run = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(run.path().join(exec::STDOUT_FILENAME), b"hello").unwrap();
        std::fs::write(run.path().join(exec::STDERR_FILENAME), b"").unwrap();
        std::fs::create_dir(root.path().join(gputeer_checkpoint::writer::POINTER_FILENAME)).unwrap();
        let (_, publish_failure) = finalize_workload_outputs(run.path(), &spec(), &outcome(), true, root.path(), "ckpt-19", &lease(), "attempt-19")
            .expect("검증까지 끝난 산출물은 확정 실패가 아니다");
        assert!(publish_failure.is_some(), "공개 실패는 알려야 한다");
    }

    /// 결함 82 — 삭제가 실패해도 종료 관측 보고는 살아남고, 삭제 실패는 따로 돌아온다.
    #[test]
    fn a_cleanup_failure_keeps_the_observed_report() {
        let report = WorkloadReport {
            exit: exec::ExitObserved::Code(0),
            finalization_failure: None,
            file_count: 1,
            total_bytes: 5,
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
        };
        let (kept, cleanup) = merge_run_and_cleanup(Ok(Some(report)), Err("삭제 실패".to_string()))
            .expect("종료 관측 보고는 살아야 한다");
        assert!(kept.is_some());
        assert_eq!(cleanup.as_deref(), Some("삭제 실패"));
        assert!(merge_run_and_cleanup(Err("실행 실패".to_string()), Ok(())).is_err());
        assert!(merge_run_and_cleanup(Err("실행 실패".to_string()), Err("삭제 실패".to_string())).is_err());
    }

    /// ★ 결함 ⑲ 의 핵심 — **실제로 끝난 프로세스**의 산출물 확정이 실패해도 `run_and_capture_workload` 는 Agent 오류가
    ///   아니라 확정 실패 단계를 실은 보고를 돌려준다. 이 변경을 `?` 로 되돌리면 여기서 실패한다.
    ///   실행에 Job Object 가 필요해 Windows 에서만 돈다(리눅스는 cgroup 위임이 필요하다).
    #[cfg(windows)]
    #[test]
    fn a_real_exit_with_a_blocked_checkpoint_still_yields_a_report() {
        let run = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("ckpt-19"), b"not a directory").unwrap();
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let spec = gputeer_protocol::execution_spec::ExecutionSpec {
            job_id: "job-19".into(),
            entrypoint: format!("{system_root}\\System32\\cmd.exe"),
            args: vec!["/c".into(), "exit".into(), "0".into()],
            env_vars: BTreeMap::new(),
        };
        let policy = exec::ExecutionPolicy {
            opted_in: true,
            commit_limit_bytes: 256 * 1024 * 1024,
            gpu_requirements: None,
            capture_dir: Some(run.path().to_path_buf()),
            isolation: exec::IsolationIdentity {
                grant_id: "grant-19".into(),
                attempt_id: "attempt-19".into(),
            },
            cgroup_parent: None,
        };
        let report = run_and_capture_workload(
            &spec,
            policy,
            root.path(),
            "ckpt-19",
            &lease(),
            "attempt-19",
            run.path(),
            &owner_panel::OwnerPanelState::new(),
            "submitter-19",
            1,
            &SystemClock,
        )
        .expect("확정 실패는 Agent 오류가 아니다 — 종료를 관측했으면 보고한다")
        .expect("opt-in 했으니 실행됐다");
        assert_eq!(report.exit, exec::ExitObserved::Code(0));
        let (stage, _) = report
            .finalization_failure
            .expect("확정 실패가 보고에 실려야 한다");
        assert_eq!(stage, pb::FinalizationFailureStage::CommitCheckpoint);
    }
}

#[cfg(test)]
mod defect_42_45_tests {
    use super::*;

    /// ★ 결함 ㊸ — 정리가 실패해도 앞선 거부 사유가 남는다.
    #[cfg(windows)]
    #[test]
    fn a_cleanup_failure_keeps_the_original_refusal() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let run_dir = dir.path().join("run");
        std::fs::create_dir_all(&run_dir).expect("작업 디렉터리");
        // 공유 없이 열어 둔 파일이 있으면 Windows 는 그 디렉터리를 지우지 못한다 — 정리를 실패시킨다.
        let held = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .share_mode(0)
            .open(run_dir.join("held"))
            .expect("파일");
        let message = fail_after_cleanup("EXEC_REFUSED:LIMIT_NOT_APPLIED".to_string(), &run_dir);
        drop(held);
        assert!(message.contains("EXEC_REFUSED:LIMIT_NOT_APPLIED"), "{message}");
        assert!(message.contains("그리고"), "정리 실패가 보고되지 않았다: {message}");
    }

    #[test]
    fn a_successful_cleanup_returns_the_original_error_and_removes_the_directory() {
        let dir = tempfile::tempdir().expect("임시 디렉터리");
        let run_dir = dir.path().join("run");
        std::fs::create_dir_all(&run_dir).expect("작업 디렉터리");
        let message = fail_after_cleanup("ACK 전송 실패: x".to_string(), &run_dir);
        assert_eq!(message, "ACK 전송 실패: x");
        assert!(!run_dir.exists(), "작업 디렉터리가 남았다");
    }

    /// ★ 결함 ㊺ — 워크로드를 띄운 뒤의 오류는 재접속 · 재조회로 분류되지 않는다.
    #[test]
    fn errors_after_the_workload_ran_are_not_retried() {
        assert!(matches!(
            SessionError::from("RETRYABLE_CONNECTION: x".to_string()),
            SessionError::Retryable(_)
        ));
        assert!(matches!(
            SessionError::from("AMBIGUOUS_RENEW: z".to_string()),
            SessionError::AmbiguousRenew(_)
        ));
        for raw in ["RETRYABLE_CONNECTION: x", "RETRYABLE_RESUME: y", "AMBIGUOUS_RENEW: z"] {
            let mapped = not_retried_after_workload(raw);
            assert!(
                matches!(SessionError::from(mapped.clone()), SessionError::Fatal(_)),
                "{mapped}"
            );
            assert!(mapped.starts_with("WORKLOAD_EXECUTION_ATTEMPTED"), "{mapped}");
        }
    }
}
