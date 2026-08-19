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
use std::path::PathBuf;
use std::time::Duration;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, InMemoryKeyring, InMemoryReplayGuard,
    IngressMessage, KeyDirectorySource, SigningKey, SystemClock, VerifyingKey,
};
use gputeer_protocol::pb;
use prost::Message;

pub mod lease_store;
use lease_store::{CoordinatorLeaseStore, LeaseStoreError, RenewDecision, StoredLease};

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
    /// ★ 테스트 전용 — Agent의 ACK를 검증한 직후 연결을 의도적으로
    ///   닫는다. 재접속 조각의 첫 번째 프로세스 쌍이 실제로 끊긴
    ///   뒤 종료되는지 확인하기 위한 주입이며, 운영 재시도는 만들지
    ///   않는다.
    pub disconnect_after_ack: bool,

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
    // ★ fail closed — lease store 를 **listener bind 보다 먼저** 연다.
    //   `--lease-db` 를 안 주면(기존 전부) `None` 이라 이 단계는
    //   아무것도 하지 않는다(`docs/plans/2026-08-19_2300_...v1.md`).
    let mut lease_store = match &config.lease_db_path {
        Some(path) => {
            let store = CoordinatorLeaseStore::open(path)
                .map_err(|e| format!("lease store 저장소 열기 실패: {e}"))?;
            if !store.is_durable() {
                return Err(format!(
                    "lease store 저장소가 영속이 아니다(lease_db_path={path:?}) — \
                     재시작을 넘는 Lease 복원이 조용히 무력화된다"
                ));
            }
            Some(store)
        }
        None => None,
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

    let (mut stream, _) = listener.accept().map_err(|e| format!("accept 실패: {e}"))?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .map_err(|e| e.to_string())?;

    let now = clock.now_unix_ms();
    let mut grant = issue_grant(&config, &mut lease_store, &signing_key, now)?;

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
        send_revoke_notice(&config, &grant, &mut lease_store, &signing_key, &mut stream)?;
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
        for _round in if config.do_renew { 0..config.renew_rounds } else { 0..0 } {
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
                    &config,
                    &signing_key,
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
            ));
        }

        let result = build_renew_result(
            &config,
            &mut lease_store,
            &signing_key,
            renew_now,
            &renew_req.lease_id,
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

        // Agent는 정상 정책 거부(outcome=2/3/6/8)를 받으면 즉시
        // 갱신 함수를 종료하므로, 다음 회차의 요청을 기다리지 않는다.
        // Coordinator도 같은 회차에서 갱신 루프를 끝내야 교착/EOF 오류를
        // 만들지 않는다.
        if matches!(result.outcome, 2 | 3 | 6 | 8) {
            break;
        }

            if config.revoke_after_round == Some(_round + 1) {
                send_revoke_notice(&config, &grant, &mut lease_store, &signing_key, &mut stream)?;
                break;
            }
        }
    }

    println!(
        "RESULT ok=true grant_id={} attempt_id={} agent_device_id={}",
        grant.grant_id, grant.attempt_id, ack.agent_device_id
    );
    Ok(())
}

/// 이미 발급한 Grant 안의 Lease를 대상으로 revoke 통지를 만들고
/// 서명해 같은 연결로 보낸다.
///
/// `RevokeLeaseNotice::signer_id()`는 현재 프로토콜 계약상 `lease_id`를
/// 반환한다(V-08). 따라서 테스트용 target override도 바뀐 payload에
/// 대해 정상 서명해, Agent의 identity 검증과 서명 검증을 분리한다.
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
                    .ok_or_else(|| format!("RenewLeaseRequest.lease_id({lease_id}) 가 lease store 에 없다"))?;
                if let Some(revoked_at_unix_ms) = stored.revoked_at_unix_ms {
                    revoked_result(request_nonce, revoked_at_unix_ms)
                } else if stored.is_max_duration_exceeded(now) {
                    max_duration_exceeded_result(request_nonce)
                } else {
                    policy_override(outcome, request_nonce)
                }
            }
            None => match store.renew_existing_within_duration(
                lease_id,
                now,
                now + 60_000,
                now + 30_000,
            ) {
                Err(LeaseStoreError::Revoked { revoked_at_unix_ms }) => {
                    revoked_result(request_nonce, revoked_at_unix_ms)
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
fn build_renewed_lease_result(
    config: &CoordinatorConfig,
    key: &SigningKey,
    now: u64,
    resolved: StoredLease,
    request_nonce: Vec<u8>,
) -> Result<pb::RenewLeaseResult, String> {
    let max_total_duration_seconds =
        u32_from_stored(resolved.max_total_duration_seconds, "max_total_duration_seconds")?;
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
) -> Result<pb::ExecutionGrant, String> {
    let lease = issue_lease(config, lease_store, key, now)?;

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
    Ok(grant)
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

    let max_total_duration_seconds =
        u32_from_stored(resolved.max_total_duration_seconds, "max_total_duration_seconds")?;
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
        disconnect_after_ack: flags.bool_flag("--disconnect-after-ack"),
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
        renew_rounds: flags.u32_flag_with_default("--renew-rounds", 1)?,
        lease_db_path: flags.0.get("--lease-db").map(PathBuf::from),
        max_total_duration_seconds: flags
            .u64_flag_with_default("--max-total-duration-seconds", 86_400)?,
        revoke_after_round: flags.u32_opt_flag("--revoke-after-round")?,
        revoke_before_renew: flags.bool_flag("--revoke-before-renew"),
        revoke_lease_id_override: flags.0.get("--revoke-lease-id").cloned(),
        revoke_fence_epoch_override: flags.u64_opt_flag("--revoke-fence-epoch")?,
        corrupt_revoke_signature: flags.bool_flag("--corrupt-revoke-signature"),
        lease_ttl_ms: flags.u64_flag_with_default("--lease-ttl-ms", 60_000)?,
        revoke_delay_ms: flags.u64_flag_with_default("--revoke-delay-ms", 0)?,
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
            Some(v) => v.parse::<u32>().map_err(|e| format!("{key} 파싱 실패: {e}")),
        }
    }

    /// `max_total_duration_seconds`(2026-08-19) — 안 주면 `default`
    /// (기존 하드코딩 값 86,400초 = 24시간과 동일, 회귀 없음).
    fn u64_flag_with_default(&self, key: &str, default: u64) -> Result<u64, String> {
        match self.0.get(key) {
            None => Ok(default),
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
        assert_eq!(u32_from_stored(86_400, "max_total_duration_seconds"), Ok(86_400));
    }

    #[test]
    fn u32_from_stored_accepts_u32_max() {
        assert_eq!(
            u32_from_stored(u64::from(u32::MAX), "x"),
            Ok(u32::MAX)
        );
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
