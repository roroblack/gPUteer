//! 저장된 예약에서 **서명된 `ExecutionGrant`** 를 만든다.
//!
//! # ★ 왜 CLI 가 아니라 여기 있는가
//!
//! 처음에는 `gputeer issue-grant` 안에만 있었다. 그런데 `coordinator-stub`
//! 이 같은 일을 해야 하는 순간(저장된 예약을 Agent 에게 실제로 보내기)
//! **두 번째 복사본**이 될 참이었다.
//!
//! 이 저장소는 그 함정을 이미 두 번 겪었다 — `DoD-67` 에서 영속성 판정을
//! CLI 와 저장소 두 곳에 두었다가 한쪽만 낡아 `--job-db ""` 가 조용히
//! 성공했고, `StoredLease → pb::Lease` 변환은 **세 벌**로 복사돼 있었다.
//!
//! # ★ 한 행만 믿지 않는다
//!
//! Attempt·Lease·예약은 서로 다른 행이다. 셋이 어긋나 있으면 그 자체가
//! 사실이고, 그때 한쪽만 읽으면 없는 일관성을 가정하게 된다.
//!
//! ```text
//! Job 상태            STAGING 이어야 한다 — 예약 없이 Grant 를 내면
//!                     Agent 는 자기가 그 노드를 쓸 권한이 있다고 믿는다
//! Attempt ↔ Lease     job_id · attempt_id · lease_id · fence epoch
//! 예약(node)          Lease 의 보유 노드에 이 Attempt 로 걸려 있는가
//! Lease 상태          폐기되지 않았고, 발급 시각에 아직 안 만료됐는가
//! Grant 수명          Lease 보다 오래 살지 않는가
//! ```
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 안 한다   저장소 열기·영속성 판정   호출부가 연 저장소를 받는다
//! 안 한다   전송                      파일로 낼지 wire 로 보낼지는 호출부다
//! 안 한다   Manifest 싣기             제출자 서명 원본이 필요하다 — 별도 조각
//! 안 한다   ResourceScope 채우기      GPU scope 는 authoritative provenance
//!                                     가 없어 막혀 있다(`DoD-55`)
//! ```

use gputeer_crypto::{sign, SigningKey};
use gputeer_protocol::pb;

use crate::job_store::{CoordinatorJobStore, JobState};
use crate::lease_store::CoordinatorLeaseStore;
use crate::staging_store::CoordinatorStagingStore;
use crate::unsigned_lease_from_stored;

/// 호출부가 정하는 값. **이 모듈은 시계를 읽지 않는다.**
///
/// ★ 시각을 절대값으로 받는 이유 — 시계를 읽으면 같은 입력으로 두 번
///   발급했을 때 결과가 달라지고, 그러면 "같은 입력이면 같은 Grant" 를
///   확인할 방법이 없어진다(`stage-job` 에서 통합 테스트가 그 문제를
///   실제로 잡았다).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGrantRequest {
    pub job_id: String,
    pub attempt_id: String,
    pub lease_id: String,
    pub grant_id: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    /// ★ **nonce 는 저장된 사실이 아니라 전송 계약이다.**
    ///
    ///   처음에는 이 모듈이 `(grant_id, attempt_id)` 로 직접 뽑았다.
    ///   그랬더니 wire 로 보낼 때 Agent 가 거부했다 —
    ///   `GRANT_REJECTED: nonce does not match connection attempt`.
    ///   Agent 는 **연결 시도 번호**에서 유도한 값을 기대한다.
    ///
    ///   두 소비자가 요구하는 것이 다르다:
    ///     · 파일로 낼 때(`issue-grant`) — 연결 개념이 없다.
    ///       같은 입력이면 같은 Grant 여야 재발급이 멱등하다.
    ///     · wire 로 보낼 때 — Agent 와 합의된 유도식을 써야 한다.
    ///
    ///   그래서 이 모듈이 정하지 않고 **호출부가 준다.** 여기서 하나를
    ///   고르면 다른 소비자가 조용히 깨진다.
    pub nonce: Vec<u8>,
}

/// 저장된 예약을 읽어 대조한 뒤 서명된 Grant 를 만든다.
///
/// nested Lease 를 **먼저** 완성해 서명한다 — 그래야 outer Grant 의 서명이
/// 최종 nested 바이트를 덮는다.
pub fn signed_grant_from_stored(
    jobs: &CoordinatorJobStore,
    staging: &CoordinatorStagingStore,
    leases: &CoordinatorLeaseStore,
    request: &StoredGrantRequest,
    key: &SigningKey,
) -> Result<pb::ExecutionGrant, String> {
    if request.issued_at_unix_ms >= request.expires_at_unix_ms {
        return Err(format!(
            "Grant 발급 시각({})이 만료({}) 보다 앞서지 않는다",
            request.issued_at_unix_ms, request.expires_at_unix_ms
        ));
    }

    let job = jobs
        .get(&request.job_id)
        .map_err(|e| format!("Job 조회 실패: {e}"))?
        .ok_or_else(|| format!("GRANT_REFUSED: {} 를 모른다", request.job_id))?;
    if job.state != JobState::Staging {
        return Err(format!(
            "GRANT_REFUSED: Job 이 STAGING 이 아니다(현재 {:?}) — 예약 없이 Grant 를 만들지 않는다",
            job.state
        ));
    }

    let attempt = staging
        .get_attempt(&request.attempt_id)
        .map_err(|e| format!("Attempt 조회 실패: {e}"))?
        .ok_or_else(|| {
            format!(
                "GRANT_REFUSED: Attempt {} 가 저장소에 없다",
                request.attempt_id
            )
        })?;
    let stored_lease = leases
        .get(&request.lease_id)
        .map_err(|e| format!("Lease 조회 실패: {e}"))?
        .ok_or_else(|| format!("GRANT_REFUSED: Lease {} 가 저장소에 없다", request.lease_id))?;

    // ── 한 행만 믿지 않는다 ─────────────────────────────────────────
    for (label, left, right) in [
        (
            "job_id",
            attempt.job_id.as_str(),
            stored_lease.job_id.as_str(),
        ),
        (
            "attempt_id",
            attempt.attempt_id.as_str(),
            stored_lease.attempt_id.as_str(),
        ),
        (
            "lease_id",
            attempt.lease_id.as_str(),
            stored_lease.lease_id.as_str(),
        ),
    ] {
        if left != right {
            return Err(format!(
                "GRANT_REFUSED: Attempt 와 Lease 의 {label} 가 다르다(Attempt {left:?}, Lease {right:?}) — 두 행이 같은 예약을 가리키지 않는다"
            ));
        }
    }
    if attempt.fence_epoch != stored_lease.fence_epoch {
        return Err(format!(
            "GRANT_REFUSED: Attempt 와 Lease 의 fence epoch 가 다르다(Attempt {}, Lease {}) — 오래된 한쪽으로 Grant 를 만들면 fencing 이 무의미해진다",
            attempt.fence_epoch, stored_lease.fence_epoch
        ));
    }
    if attempt.job_id != request.job_id {
        return Err(format!(
            "GRANT_REFUSED: Attempt 가 다른 Job 의 것이다({} != {})",
            attempt.job_id, request.job_id
        ));
    }

    let reservation = staging
        .get_node_reservation(&stored_lease.holder_node_id)
        .map_err(|e| format!("예약 조회 실패: {e}"))?
        .ok_or_else(|| {
            format!(
                "GRANT_REFUSED: {} 에 예약이 없다 — Lease 는 있는데 노드가 안 잡혀 있다",
                stored_lease.holder_node_id
            )
        })?;
    if reservation.job_id != request.job_id || reservation.attempt_id != request.attempt_id {
        return Err(format!(
            "GRANT_REFUSED: {} 의 예약은 다른 Attempt 의 것이다(job {}, attempt {})",
            stored_lease.holder_node_id, reservation.job_id, reservation.attempt_id
        ));
    }

    // ── 상태 관문 ───────────────────────────────────────────────────
    if let Some(revoked_at) = stored_lease.revoked_at_unix_ms {
        return Err(format!(
            "GRANT_REFUSED: Lease 가 {revoked_at} 에 폐기됐다 — 폐기된 Lease 로 Grant 를 만들지 않는다"
        ));
    }
    // ★ 경계 포함(`<=`) — `DoD-26`·`DoD-32`·`DoD-34` 가 정착시킨 규칙과
    //   같다. 정확히 만료 시각인 Lease 를 여기서만 유효로 보면, Agent 는
    //   같은 순간 그 Lease 를 거부한다.
    if stored_lease.expires_at_unix_ms <= request.issued_at_unix_ms {
        return Err(format!(
            "GRANT_REFUSED: 발급 시각({})에 Lease 가 이미 만료다(만료 {})",
            request.issued_at_unix_ms, stored_lease.expires_at_unix_ms
        ));
    }
    if request.expires_at_unix_ms > stored_lease.expires_at_unix_ms {
        return Err(format!(
            "GRANT_REFUSED: Grant 만료({})가 Lease 만료({}) 보다 늦다",
            request.expires_at_unix_ms, stored_lease.expires_at_unix_ms
        ));
    }

    // ── 서명 ────────────────────────────────────────────────────────
    let mut lease = unsigned_lease_from_stored(&stored_lease)?;
    lease.coordinator_signature = sign(key, &lease).to_vec();

    let mut grant = pb::ExecutionGrant {
        schema_version: 2,
        grant_id: request.grant_id.clone(),
        attempt_id: attempt.attempt_id.clone(),
        coordinator_device_id: stored_lease.issuing_coordinator_id.clone(),
        // ★ 리터럴 1 이 아니라 저장된 term 이다. `coordinator-stub` 의
        //   기존 `issue_grant()` 는 여기에 1 을 박아 넣는다.
        coordinator_term: stored_lease.coordinator_term,
        issued_at_unix_ms: request.issued_at_unix_ms,
        expires_at_unix_ms: request.expires_at_unix_ms,
        nonce: request.nonce.clone(),
        // 이 비트는 v2 Grant 서명 대상이다. 이 경로는 **저장소에서만**
        // Lease 를 읽으므로 정직하게 true 다.
        lease_from_durable_store: true,
        lease: Some(lease),
        ..Default::default()
    };
    grant.coordinator_signature = sign(key, &grant).to_vec();

    // ── 방금 만든 것을 바로 다시 검증한다 ───────────────────────────
    //
    // ★ `gputeer submit` 이 이미 하는 일이다("서명하자마자 깨진 파일을
    //   내보내는 것보다 여기서 실패하는 편이 훨씬 싸다"). 이 경로에는
    //   그게 **없었다** — 내 뮤테이션이 아니라 코드를 나란히 놓고 보다가
    //   찾았다.
    //
    //   `AlwaysValid` 를 쓰므로 이것이 **확인하는 것과 못 하는 것**을
    //   정확히 적는다:
    //
    //   확인한다   구조·`schema_version`·수명·canonical 인코딩이
    //              성립하는가. nested Lease 도 같이 본다.
    //   못 한다    **이 키가 정말 `issuing_coordinator_id` 의 것인가.**
    //              이 모듈에는 key directory 가 없다 — 호출부가 준
    //              키를 그대로 쓴다.
    //
    //   ★ 그래서 엉뚱한 키로 서명하면 **여기서는 통과하고**, 나중에
    //     Agent 가 거부한다(그 id 의 공개키로 서명이 안 맞는다). 위험
    //     하지는 않다 — 아무도 못 쓰는 Grant 다. 다만 **운영자는 유효한
    //     것을 만든 줄 안다**는 문제가 남는다. 그걸 닫으려면 Coordinator
    //     공개키 목록이 필요하고, 이 저장소에 아직 없다.
    verify_own_output(&grant, request.issued_at_unix_ms)?;
    Ok(grant)
}

/// 방금 서명한 Grant 를 **구조·수명 수준에서** 자기 검증한다.
///
/// 서명 자체를 다시 도는 것이 목적이 아니다(방금 우리가 만들었다).
/// 확인하려는 것은 canonical 인코딩과 수명이 성립하는가다.
fn verify_own_output(grant: &pb::ExecutionGrant, at_unix_ms: u64) -> Result<(), String> {
    gputeer_protocol::verify(
        grant,
        2,
        &AlwaysValid,
        at_unix_ms,
        &mut gputeer_protocol::signing::NoReplayCheck,
    )
    .map_err(|e| format!("방금 만든 Grant 가 자기 검증을 통과하지 못했다: {e:?}"))?;

    let lease = grant
        .lease
        .as_ref()
        .ok_or_else(|| "방금 만든 Grant 에 Lease 가 없다".to_string())?;
    gputeer_protocol::verify(
        lease,
        1,
        &AlwaysValid,
        at_unix_ms,
        &mut gputeer_protocol::signing::NoReplayCheck,
    )
    .map_err(|e| format!("방금 만든 nested Lease 가 자기 검증을 통과하지 못했다: {e:?}"))?;
    Ok(())
}

/// 서명 **검사를 건너뛰는** 검증자 — "내가 방금 만든 것" 에만 쓴다.
///
/// ★ 수신 측에서 이것을 쓰면 안 된다. `submit.rs` 에 같은 것이 있고
///   같은 이유로 거기서만 쓰인다.
struct AlwaysValid;

impl gputeer_protocol::signing::SignatureVerifier for AlwaysValid {
    fn verify_signature(
        &self,
        _signer_id: &str,
        _message: &[u8],
        _signature: &[u8],
    ) -> Result<(), gputeer_protocol::VerifyOutcome> {
        Ok(())
    }
}

/// `(grant_id, attempt_id)` 에서 16바이트 nonce 를 결정적으로 뽑는다.
///
/// ★ **무작위가 아니다.** 같은 입력이면 같은 Grant 를 내야 재발급이
///   멱등해지고 그것을 테스트로 확인할 수 있다. 재생 방어는 nonce 의
///   예측 불가능성이 아니라 **수신 측의 replay 저장소**가 맡는다
///   (`crates/crypto` 의 `DurableReplayGuard`).
///
/// 길이 프리픽스를 붙여 인접 값이 서로 스며들지 않게 한다.
pub fn derive_stored_grant_nonce(grant_id: &str, attempt_id: &str) -> Vec<u8> {
    let mut input = Vec::new();
    for part in [
        b"gputeer/v1/grant-nonce".as_slice(),
        grant_id.as_bytes(),
        attempt_id.as_bytes(),
    ] {
        input.extend_from_slice(&(part.len() as u64).to_le_bytes());
        input.extend_from_slice(part);
    }
    gputeer_protocol::canonical::blake3_256(&input)[..16].to_vec()
}
