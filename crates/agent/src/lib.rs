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

/// 정상 handshake 한 번을 실행한다.
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
pub fn run(config: AgentConfig) -> Result<(), String> {
    let policy = RetryPolicy {
        max_attempts: config.max_reconnect_attempts,
        max_duration: Duration::from_secs(config.max_reconnect_duration_seconds),
        base_delay: Duration::from_millis(config.retry_base_ms),
        cap_delay: Duration::from_millis(config.retry_cap_ms),
        connect_timeout: Duration::from_secs(3),
        safety_margin_ms: 1_000,
    };
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
        mode: 2,
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

fn run_one_connection(
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
    ack.agent_signature = sign(&signing_key, &ack).to_vec();

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

    if config.reconnect_enabled
        && !config.do_renew
        && !config.expect_replay
        && config.expect_revoke_after_round.is_none()
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

        // ★ 서명은 이미 검증됐다 — 그 뒤에 상관관계를 확인한다.
        //   request_nonce 가 우리가 보낸 요청과 다르면, 이 결과가 다른
        //   갱신 요청에 대한 응답이 재사용되고 있다는 뜻이다.
        if result.request_nonce != renew_req.nonce {
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
                // RENEW_OUTCOME_RENEWED — nested Lease 는 outer 결과
                // 서명과 **무관하게** 독립적으로 검증한다(규칙 i).
                let new_lease = result.lease.clone().ok_or_else(|| {
                    "RENEW_REJECTED: outcome=RENEWED 인데 Lease 가 없다".to_string()
                })?;

                let verifier = Ed25519Verifier::new(&*coordinator_keys);
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
                    return Err(
                        "RENEW_REJECTED: 갱신된 Lease.issuing_coordinator_id 가 기대값과 다르다"
                            .into(),
                    );
                }
                if new_lease.holder_node_id != config.agent_device_id {
                    return Err(
                        "RENEW_REJECTED: 갱신된 Lease.holder_node_id 가 이 Agent 가 아니다".into(),
                    );
                }

                // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — epoch **상승**은
                //   이 조각의 범위 밖이라 정책상 거부해야 한다(계획서 §범위
                //   "이 조각이 결정하지 않는 것" — "높으면 이 조각에서는
                //   정책상 거부"). `FenceWatermark.check_and_advance()` 는
                //   `<` 만 거부하고 `>` 는 **통과시키므로**(그것이 정상적인
                //   epoch 전진의 정의다), 그것만으로는 이 계약을 강제하지
                //   못한다 — 여기서 명시적으로 막는다.
                if new_lease.fence_epoch > held_lease.fence_epoch {
                    return Err(format!(
                        "RENEW_REJECTED: 갱신된 Lease.fence_epoch({}) 이 기존({}) 보다 높다 \
                         — epoch 상승은 이 조각의 범위 밖이라 정책상 거부한다",
                        new_lease.fence_epoch, held_lease.fence_epoch
                    ));
                }

                // ★ **같은 job_id 를 resource key 로 재사용한다** —
                //   `lease_id` 를 새 키로 쓰면 기존 watermark 와 분리되어
                //   강등 방어가 깨진다(계획서 "FenceWatermark 재사용" 절).
                fence_watermark
                    .check_and_advance(&new_lease.job_id, new_lease.fence_epoch)
                    .map_err(|e| fence_error_message("RENEW_REJECTED", e))?;

                held_lease = new_lease.clone();
                println!(
                    "RENEW_RESULT ok=true outcome=RENEWED lease_id={} fence_epoch={}",
                    held_lease.lease_id, held_lease.fence_epoch
                );
            }
            2 => return Err("RENEW_REFUSED:SUPERSEDED".into()),
            3 => return Err("RENEW_REFUSED:QUARANTINED".into()),
            // ★ max_total_duration_seconds 갱신 차단(2026-08-19,
            //   docs/plans/2026-08-19_2350_...) — 이 lease_id 로 누적
            //   가능한 최대 시간을 넘었다. 새 lease_id 재발급은 이
            //   조각의 범위 밖이다 — 여기서는 명시적으로 거부만 한다.
            6 => return Err("RENEW_REFUSED:MAX_DURATION_EXCEEDED".into()),
            8 => return Err("RENEW_REFUSED:REVOKED".into()),
            other => return Err(format!("RENEW_REJECTED: 알 수 없는 outcome {other}")),
        }

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
fn derive_nonce(tag: &str, id: &str, connection_attempt: u32) -> Vec<u8> {
    let mut input = Vec::with_capacity(tag.len() + 1 + id.len() + 4);
    input.extend_from_slice(tag.as_bytes());
    input.push(0);
    input.extend_from_slice(id.as_bytes());
    if connection_attempt != 0 {
        input.extend_from_slice(&connection_attempt.to_be_bytes());
    }
    gputeer_protocol::canonical::blake3_256(&input)[..16].to_vec()
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

/// `--flag value` 쌍으로 이루어진 CLI 인자를 [`AgentConfig`] 로
/// 파싱해 [`run`] 을 부른다. `crates/coordinator/src/lib.rs::run_from_args`
/// 와 같은 이유로 `crates/cli` 대신 여기(Agent 스트림)가 인자 의미를
/// 정의한다.
pub fn run_from_args(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args)?;

    let config = AgentConfig {
        coordinator_addr: flags.require("--connect")?,
        own_seed: hex_to_seed(&flags.require("--own-seed")?)?,
        coordinator_verifying_key: hex_to_verifying_key(&flags.require("--peer-pubkey")?)?,
        coordinator_device_id: flags.require("--coordinator-device-id")?,
        agent_device_id: flags.require("--agent-device-id")?,
        corrupt_own_signature: flags.bool_flag("--corrupt-own-signature"),
        expect_replay: flags.bool_flag("--expect-replay"),
        do_renew: flags.bool_flag("--do-renew"),
        renew_request_epoch_override: flags.u64_opt_flag("--renew-request-epoch-override")?,
        corrupt_renew_request_signature: flags.bool_flag("--corrupt-renew-request-signature"),
        fence_db_path: match flags.0.get("--fence-db") {
            Some(v) => PathBuf::from(v),
            None => default_fence_db_path(),
        },
        checkpoint_root: match flags.0.get("--checkpoint-root") {
            Some(v) => PathBuf::from(v),
            None => default_checkpoint_root(),
        },
        renew_rounds: flags.u32_flag_with_default("--renew-rounds", 1)?,
        renew_delay_ms: flags.u64_flag_with_default("--renew-delay-ms", 0)?,
        expect_revoke_after_round: flags.u32_opt_flag("--expect-revoke-after-round")?,
        revoke_signer_id_override: flags.0.get("--revoke-signer-id").cloned(),
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
        session_id: flags
            .0
            .get("--session-id")
            .cloned()
            .unwrap_or_else(|| "resume-session".into()),
        resume_lease_id: flags
            .0
            .get("--resume-lease-id")
            .cloned()
            .unwrap_or_default(),
        resume_job_id: flags.0.get("--resume-job-id").cloned().unwrap_or_default(),
        resume_attempt_id: flags
            .0
            .get("--resume-attempt-id")
            .cloned()
            .unwrap_or_default(),
        resume_fence_epoch: flags.u64_flag_with_default("--resume-fence-epoch", 0)?,
    };

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

struct Flags(std::collections::HashMap<String, String>);

impl Flags {
    fn require(&self, key: &str) -> Result<String, String> {
        self.0
            .get(key)
            .cloned()
            .ok_or_else(|| format!("필수 인자 누락: {key}"))
    }

    /// `crates/coordinator/src/lib.rs::Flags::bool_flag` 와 동일 — 값이
    /// 있는 boolean 플래그(`--flag true`). 안 주면 `false`.
    fn bool_flag(&self, key: &str) -> bool {
        self.0.get(key).map(|v| v == "true").unwrap_or(false)
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

    /// ★ 테스트 전용 — 갱신 요청 epoch 강제 주입(단계 5). 안 주면
    ///   `None`(보유 중인 Lease 의 실제 epoch 을 그대로 쓴다).
    fn u64_flag_with_default(&self, key: &str, default: u64) -> Result<u64, String> {
        match self.0.get(key) {
            None => Ok(default),
            Some(v) => v
                .parse::<u64>()
                .map_err(|e| format!("{key} parse failed: {e}")),
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
}
