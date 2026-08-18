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

use std::io::Write;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, Ed25519Verifier, FrameType, InMemoryKeyring,
    InMemoryReplayGuard, IngressMessage, KeyDirectorySource, SigningKey, SystemClock,
    VerifyingKey,
};
use gputeer_protocol::{pb, verify};
use gputeer_runtime_policy::{DurableFenceError, DurableFenceWatermark};
use prost::Message;

const IO_TIMEOUT: Duration = Duration::from_secs(10);

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
}

/// 정상 handshake 한 번을 실행한다.
///
/// Coordinator 에 연결해 `ExecutionGrant` 를 받아 검증하고, 서명된
/// `AgentGrantAck` 를 돌려준다. 성공하면 `stdout` 에
/// `RESULT ok=true ...` 를 찍고 `Ok(())`, 실패하면 그 이유를 담아
/// `Err` 를 반환한다(호출자가 exit code 로 매핑).
pub fn run(config: AgentConfig) -> Result<(), String> {
    let signing_key = SigningKey::from_bytes(&config.own_seed);
    let mut coordinator_keys = InMemoryKeyring::new();
    coordinator_keys.insert(
        config.coordinator_device_id.clone(),
        config.coordinator_verifying_key,
    );

    let mut replay = InMemoryReplayGuard::new();
    let clock = SystemClock;

    // ★ fail closed — fence watermark 저장소를 **네트워크 연결보다
    //   먼저** 연다. 열 수 없는 저장소로 epoch 를 검증하는 척하지
    //   않는다(`docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md`).
    let mut fence_watermark = DurableFenceWatermark::open(&config.fence_db_path)
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

    let mut stream = TcpStream::connect(&config.coordinator_addr)
        .map_err(|e| format!("Coordinator 연결 실패: {e}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;

    let received = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&coordinator_keys),
        &mut replay,
        &clock,
    )
    .map_err(|e| format!("Grant 프레임 읽기/검증 실패: {e}"))?;

    // ★ Grant 를 replay 검사까지 통과한 뒤에만 그 내용으로 ACK 를
    //   만든다 — 부작용(ACK 발급)이 검증되지 않은 값에서 나오지 않는다.
    let grant: pb::ExecutionGrant = match &received {
        IngressMessage::Grant(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("Grant replay 검사 실패: {e:?}"))?
            .clone(),
        other => return Err(format!("예상하지 못한 요청 타입: {other:?}")),
    };

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

    let now = clock.now_unix_ms();
    let mut ack = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: grant.grant_id.clone(),
        attempt_id: grant.attempt_id.clone(),
        agent_device_id: config.agent_device_id.clone(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_nonce("grant-ack", &grant.grant_id),
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
            1,
            KeyDirectorySource::Provided(&coordinator_keys),
            &mut replay,
            &clock,
        ) {
            Err(error) => Err(format!("REPLAY_REJECTED: {error}")),
            Ok(message) => Err(format!(
                "REPLAY_NOT_REJECTED: 두 번째 Grant 가 거부되지 않고 {message:?} 로 검증됐다"
            )),
        };
    }

    // ★ Lease 갱신 (2026-08-19) — ACK 를 보낸 뒤 같은 연결에 이어서
    //   `RenewLeaseRequest` 를 보내고 서명된 `RenewLeaseResult` 를
    //   기다린다. `do_renew == false` 면 건너뛴다.
    if config.do_renew {
        let renew_now = clock.now_unix_ms();
        let mut renew_req = pb::RenewLeaseRequest {
            schema_version: 1,
            lease_id: held_lease.lease_id.clone(),
            fence_epoch: config
                .renew_request_epoch_override
                .unwrap_or(held_lease.fence_epoch),
            node_id: config.agent_device_id.clone(),
            issued_at_unix_ms: renew_now,
            nonce: derive_nonce("lease-renew", &held_lease.lease_id),
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
            .map_err(|e| format!("RenewLeaseRequest 전송 실패: {e}"))?;
        stream.flush().map_err(|e| e.to_string())?;

        let result_msg = read_frame(
            &mut stream,
            1,
            KeyDirectorySource::Provided(&coordinator_keys),
            &mut replay,
            &clock,
        )
        .map_err(|e| format!("RenewLeaseResult 프레임 읽기/검증 실패: {e}"))?;

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
            return Err(
                "RENEW_REJECTED: request_nonce 가 우리가 보낸 요청과 다르다".into(),
            );
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
                let new_lease = result
                    .lease
                    .clone()
                    .ok_or_else(|| "RENEW_REJECTED: outcome=RENEWED 인데 Lease 가 없다".to_string())?;

                let verifier = Ed25519Verifier::new(&coordinator_keys);
                let verified_lease = verify(&new_lease, 1, &verifier, clock.now_unix_ms(), &mut replay)
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
                        "RENEW_REJECTED: 갱신된 Lease.issuing_coordinator_id 가 기대값과 다르다".into(),
                    );
                }
                if new_lease.holder_node_id != config.agent_device_id {
                    return Err("RENEW_REJECTED: 갱신된 Lease.holder_node_id 가 이 Agent 가 아니다".into());
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
            other => return Err(format!("RENEW_REJECTED: 알 수 없는 outcome {other}")),
        }
    }

    println!(
        "RESULT ok=true grant_id={} attempt_id={} agent_device_id={}",
        grant.grant_id, grant.attempt_id, ack.agent_device_id
    );
    Ok(())
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

fn derive_nonce(tag: &str, id: &str) -> Vec<u8> {
    let mut input = Vec::with_capacity(tag.len() + 1 + id.len());
    input.extend_from_slice(tag.as_bytes());
    input.push(0);
    input.extend_from_slice(id.as_bytes());
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

    /// ★ 테스트 전용 — 갱신 요청 epoch 강제 주입(단계 5). 안 주면
    ///   `None`(보유 중인 Lease 의 실제 epoch 을 그대로 쓴다).
    fn u64_opt_flag(&self, key: &str) -> Result<Option<u64>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<u64>()
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
