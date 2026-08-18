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

use std::io::Write;
use std::net::TcpListener;
use std::time::Duration;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, InMemoryKeyring, InMemoryReplayGuard,
    IngressMessage, KeyDirectorySource, SigningKey, SystemClock, VerifyingKey,
};
use gputeer_protocol::pb;
use prost::Message;

const IO_TIMEOUT: Duration = Duration::from_secs(10);

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
}

/// 정상 handshake 한 번을 실행한다.
///
/// 리스닝을 시작하면 즉시 `stdout` 에 `READY <addr>` 한 줄을 찍는다 —
/// 호출자(주로 `coordinator-agent-selftest`)가 이 줄로 실제 바인딩된
/// 주소를 얻는다. `port 0` 요청은 커널이 포트를 고르므로 미리 알 수 없다.
///
/// 성공하면 `stdout` 에 `RESULT ok=true ...` 를 찍고 `Ok(())`,
/// 실패하면 그 이유를 담아 `Err` 를 반환한다(호출자가 exit code 로 매핑).
pub fn run(config: CoordinatorConfig) -> Result<(), String> {
    let listener = TcpListener::bind(&config.listen).map_err(|e| format!("bind 실패: {e}"))?;
    let address = listener.local_addr().map_err(|e| e.to_string())?;

    println!("READY {address}");
    std::io::stdout().flush().map_err(|e| e.to_string())?;

    let signing_key = SigningKey::from_bytes(&config.own_seed);
    let mut agent_keys = InMemoryKeyring::new();
    agent_keys.insert(config.agent_device_id.clone(), config.agent_verifying_key);

    let mut replay = InMemoryReplayGuard::new();
    let clock = SystemClock;

    let (mut stream, _) = listener.accept().map_err(|e| format!("accept 실패: {e}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;

    let now = clock.now_unix_ms();
    let mut grant = issue_grant(&config, &signing_key, now);

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
        &mut stream,
        1,
        KeyDirectorySource::Provided(&agent_keys),
        &mut replay,
        &clock,
    )
    .map_err(|e| format!("ACK 프레임 읽기/검증 실패: {e}"))?;

    // ★ `require_replay_checked()` 를 반드시 거친다 — replay 상태가
    //   `permits_side_effects()` 를 만족하지 못한 `Verified<M>` 로
    //   grant_id 대조 같은 부작용을 실행하지 않는다(§10).
    let ack = match &received {
        IngressMessage::GrantAck(verified) => verified
            .require_replay_checked()
            .map_err(|e| format!("ACK replay 검사 실패: {e:?}"))?,
        other => return Err(format!("예상하지 못한 응답 타입: {other:?}")),
    };

    if !ack.accepted {
        return Err("Agent 가 Grant 를 accepted=false 로 응답했다".into());
    }
    if ack.grant_id != grant.grant_id {
        return Err(format!(
            "grant_id 상관관계 불일치: 보낸 값 {} != ACK 값 {}",
            grant.grant_id, ack.grant_id
        ));
    }
    if ack.attempt_id != grant.attempt_id {
        return Err(format!(
            "attempt_id 상관관계 불일치: 보낸 값 {} != ACK 값 {}",
            grant.attempt_id, ack.attempt_id
        ));
    }
    if ack.agent_device_id != config.agent_device_id {
        return Err(format!(
            "agent_device_id 불일치: 기대값 {} != ACK 값 {}",
            config.agent_device_id, ack.agent_device_id
        ));
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
            &mut stream,
            1,
            KeyDirectorySource::Provided(&agent_keys),
            &mut replay,
            &clock,
        ) {
            Err(error) => Err(format!(
                "REPLAY_SCENARIO_NO_EXTRA_MESSAGE: 연결에 더 이상 아무것도 오지 않았다(기대한 결과) — {error}"
            )),
            Ok(message) => Err(format!(
                "REPLAY_SCENARIO_UNEXPECTED_EXTRA_MESSAGE: {message:?}"
            )),
        };
    }

    // ★ Lease 갱신 (2026-08-19) — 같은 연결에 이어서 Agent 가 보낸
    //   `RenewLeaseRequest` 를 받고 서명된 `RenewLeaseResult` 로
    //   응답한다. `do_renew == false` 면 건너뛴다(기존 핸드셰이크
    //   전용 시나리오와 완전히 같게 동작).
    if config.do_renew {
        let renew_msg = read_frame(
            &mut stream,
            1,
            KeyDirectorySource::Provided(&agent_keys),
            &mut replay,
            &clock,
        )
        .map_err(|e| format!("RenewLeaseRequest 프레임 읽기/검증 실패: {e}"))?;

        // ★ `require_replay_checked()` — ACK 와 같은 이유(§10).
        let renew_req = match &renew_msg {
            IngressMessage::LeaseRenew(verified) => verified
                .require_replay_checked()
                .map_err(|e| format!("RenewLeaseRequest replay 검사 실패: {e:?}"))?,
            other => return Err(format!("예상하지 못한 갱신 요청 타입: {other:?}")),
        };

        if renew_req.node_id != config.agent_device_id {
            return Err(format!(
                "RenewLeaseRequest.node_id 불일치: 기대값 {} != {}",
                config.agent_device_id, renew_req.node_id
            ));
        }
        if renew_req.lease_id != config.lease_id {
            return Err(format!(
                "RenewLeaseRequest.lease_id 불일치: 기대값 {} != {}",
                config.lease_id, renew_req.lease_id
            ));
        }
        // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — 이전에는
        //   `renew_req.fence_epoch` 를 아무것도와 대조하지 않았다.
        //   이 stub 은 별도 Lease 저장소가 없으므로, 처음 발급한
        //   `config.fence_epoch` 를 "Coordinator 가 기억하는 현재
        //   epoch" 로 삼는다 — 그 값과 다르면 요청 자체를 거부한다.
        if renew_req.fence_epoch != config.fence_epoch {
            return Err(format!(
                "RenewLeaseRequest.fence_epoch 불일치: 기대값 {} != {}",
                config.fence_epoch, renew_req.fence_epoch
            ));
        }

        let renew_now = clock.now_unix_ms();
        let result =
            build_renew_result(&config, &signing_key, renew_now, renew_req.nonce.clone());

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
    }

    println!(
        "RESULT ok=true grant_id={} attempt_id={} agent_device_id={}",
        grant.grant_id, grant.attempt_id, ack.agent_device_id
    );
    Ok(())
}

/// `RenewLeaseRequest` 에 대한 응답을 만들어 서명한다.
///
/// `renew_outcome_override` 가 있으면 그 값을 그대로 쓰고 새 Lease 를
/// 담지 않는다(서명된 정책 거부 시나리오). 없으면 `RENEW_OUTCOME_RENEWED`
/// 와 함께 `renewed_fence_epoch` 를 가진 새 Lease 를 독립적으로 서명해
/// 담는다(규칙 i — nested 서명은 outer 서명과 별개다).
fn build_renew_result(
    config: &CoordinatorConfig,
    key: &SigningKey,
    now: u64,
    request_nonce: Vec<u8>,
) -> pb::RenewLeaseResult {
    let request_nonce = if config.corrupt_renew_result_nonce {
        request_nonce.iter().map(|b| b ^ 0xFF).collect()
    } else {
        request_nonce
    };

    let mut result = match config.renew_outcome_override {
        Some(outcome) => pb::RenewLeaseResult {
            outcome,
            detail: "policy override".into(),
            schema_version: 1,
            coordinator_id: config.coordinator_device_id.clone(),
            issued_at_unix_ms: now,
            request_nonce,
            ..Default::default()
        },
        None => {
            let mut lease = pb::Lease {
                schema_version: 1,
                lease_id: config.lease_id.clone(),
                job_id: config.job_id.clone(),
                attempt_id: config.attempt_id.clone(),
                fence_epoch: config.renewed_fence_epoch,
                coordinator_term: 1,
                holder_node_id: config.agent_device_id.clone(),
                member_node_ids: vec![config.agent_device_id.clone()],
                issuing_coordinator_id: config.coordinator_device_id.clone(),
                issued_at_unix_ms: now,
                expires_at_unix_ms: now + 60_000,
                renew_after_unix_ms: now + 30_000,
                max_total_duration_seconds: 86_400,
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

            pb::RenewLeaseResult {
                outcome: 1, // RENEW_OUTCOME_RENEWED
                lease: Some(lease),
                detail: "ok".into(),
                schema_version: 1,
                coordinator_id: config.coordinator_device_id.clone(),
                issued_at_unix_ms: now,
                request_nonce,
                ..Default::default()
            }
        }
    };

    result.coordinator_signature = sign(key, &result).to_vec();

    if config.corrupt_renew_result_signature {
        let last = result
            .coordinator_signature
            .last_mut()
            .expect("RenewLeaseResult coordinator_signature 는 비어 있지 않다");
        *last ^= 0x01;
    }

    result
}

fn issue_grant(config: &CoordinatorConfig, key: &SigningKey, now: u64) -> pb::ExecutionGrant {
    let lease = issue_lease(config, key, now);

    let mut grant = pb::ExecutionGrant {
        schema_version: 1,
        grant_id: config.grant_id.clone(),
        attempt_id: config.attempt_id.clone(),
        coordinator_device_id: config.coordinator_device_id.clone(),
        coordinator_term: 1,
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_nonce("grant", &config.grant_id),
        lease: Some(lease),
        ..Default::default()
    };
    grant.coordinator_signature = sign(key, &grant).to_vec();
    grant
}

/// `ExecutionGrant.lease` 에 실어 보낼 `Lease` 를 만들어 서명한다.
///
/// ★ **독립적으로 서명한다** — `Lease::coordinator_signature` 는
///   outer `Grant::coordinator_signature` 의 계산에 들어가지 않는
///   서명 대상 필드(§6 규칙 i, 중첩 메시지는 각자 서명된다)이므로,
///   여기서 위조하면 outer Grant 서명은 여전히 유효한 채로 남는다 —
///   `corrupt_lease_signature` 시나리오가 정확히 이 성질을 시험한다.
fn issue_lease(config: &CoordinatorConfig, key: &SigningKey, now: u64) -> pb::Lease {
    let expires_at = if config.expire_lease {
        now.saturating_sub(1)
    } else {
        now + 60_000
    };

    let mut lease = pb::Lease {
        schema_version: 1,
        lease_id: config.lease_id.clone(),
        job_id: config.job_id.clone(),
        attempt_id: config.attempt_id.clone(),
        fence_epoch: config.fence_epoch,
        coordinator_term: 1,
        holder_node_id: config.agent_device_id.clone(),
        member_node_ids: vec![config.agent_device_id.clone()],
        issuing_coordinator_id: config.coordinator_device_id.clone(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: expires_at,
        renew_after_unix_ms: now + 30_000,
        max_total_duration_seconds: 86_400,
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

    lease
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
fn derive_nonce(tag: &str, id: &str) -> Vec<u8> {
    let mut input = Vec::with_capacity(tag.len() + 1 + id.len());
    input.extend_from_slice(tag.as_bytes());
    input.push(0);
    input.extend_from_slice(id.as_bytes());
    gputeer_protocol::canonical::blake3_256(&input)[..16].to_vec()
}

/// `--flag value` 쌍으로 이루어진 CLI 인자를 [`CoordinatorConfig`] 로
/// 파싱해 [`run`] 을 부른다. `crates/cli` 는 이 함수를 호출하기만 하고
/// 인자 의미는 여기(Coordinator 스트림)가 정의한다
/// (`docs/contracts/01_스트림_소유권.md`).
pub fn run_from_args(args: &[String]) -> Result<(), String> {
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
        lease_id: flags.require("--lease-id")?,
        job_id: flags.require("--job-id")?,
        fence_epoch: flags.u64_flag("--fence-epoch")?,
        corrupt_lease_signature: flags.bool_flag("--corrupt-lease-signature"),
        expire_lease: flags.bool_flag("--expire-lease"),
        do_renew: flags.bool_flag("--do-renew"),
        renewed_fence_epoch: flags.u64_flag("--renewed-fence-epoch")?,
        renew_outcome_override: flags.i32_opt_flag("--renew-outcome-override")?,
        corrupt_renew_result_signature: flags.bool_flag("--corrupt-renew-result-signature"),
        corrupt_renewed_lease_signature: flags.bool_flag("--corrupt-renewed-lease-signature"),
        corrupt_renew_result_nonce: flags.bool_flag("--corrupt-renew-result-nonce"),
    };

    run(config)
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
            Some(v) => v.parse::<u64>().map_err(|e| format!("{key} 파싱 실패: {e}")),
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
