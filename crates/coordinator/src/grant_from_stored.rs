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
//! # Manifest 를 싣는다 (2026-09-10 · §A1 1.5 선행 · 결함 ⑯)
//!
//! 저장된 binding 은 **raw** 다(`DoD-50`). 싣기 전에 호출부가 준 제출자 키
//! 디렉터리로 **발급 시각 기준** 다시 검증한다 — `plan-job`·`scheduler-tick`
//! 과 같은 방식이다. 저장소가 이미 보는 것(본문 디코딩·job_id·제출자·제출 당시
//! 서명자·hash 일치)은 여기서 다시 보지 않는다.
//!
//! ```text
//! binding 없음 · 옛 Job · 손상      GRANT_REFUSED
//! 지금 다시 검증 실패               GRANT_REFUSED (모르는 제출자 · 만료 포함)
//! Grant 만료 > Manifest 만료        GRANT_REFUSED — Agent 는 받는 시각으로 검증한다
//! ```
//!
//! ★ Coordinator 의 재검증 결과는 Grant 에 싣지 않는다. Agent 는 자기에게 설정된
//!   제출자 키로 **따로** 검증한다 — 두 관문은 서로 다르다(설계 논의 36).
//!   설계 `docs/plans/2026-09-10_1854_저장된_예약_Grant_에_Manifest_싣기.md` §7
//!
//! # 이 모듈이 하지 않는 것
//!
//! ```text
//! 안 한다   저장소 열기·영속성 판정   호출부가 연 저장소를 받는다
//! 안 한다   전송                      파일로 낼지 wire 로 보낼지는 호출부다
//! 안 한다   ResourceScope 채우기      GPU scope 는 authoritative provenance
//!                                     가 없어 막혀 있다(`DoD-55`)
//! ```

use gputeer_crypto::{sign, Ed25519Verifier, KeyDirectory, SigningKey};
use gputeer_protocol::pb;

use crate::job_store::{CoordinatorJobStore, JobState, JobStoreError};
use crate::lease_store::CoordinatorLeaseStore;
use crate::staging_store::CoordinatorStagingStore;
use crate::unsigned_lease_from_stored;

/// 호출부가 정하는 값. **이 모듈은 시계를 읽지 않는다.**
///
/// ★ 시각을 절대값으로 받는 이유 — 실시간 시각을 결과에 반영하면 같은 명시적
///   입력으로도 Grant 가 달라질 수 있다. 시각을 통제하고 결과를 재현하기 위해
///   절대값을 인자로 받는다(`stage-job` 에서 통합 테스트가 달라진 사례를 실제로
///   잡았다. 재검수 30·31 — 한때 "테스트로 고정할 수 없다" 고 적었는데, 시계 대역을
///   고정하면 테스트할 수 있다).
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

/// 저장된 예약 Grant 를 만들지 못한 이유 — 저장소 장애와 거부를 **타입으로** 가른다(결함 104, 재검수 62).
///
/// ★ 전에는 전부 문자열이었고 Coordinator 가 전부 Protocol 로 고정했다 — 저장소를 **읽다가** 난 장애(손상된 행 · I/O · 락 시한)도
///   Protocol 이 돼, 연결이 남으면 손상된 저장소를 두고 다음 연결을 받았다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoredGrantError {
    /// 저장소를 읽다가 난 장애 — 호출자는 fail-closed(Storage) 한다.
    Storage(String),
    /// 저장된 사실과 요청이 맞지 않아 만들지 않는다(GRANT_REFUSED · 시각 · Manifest 재검증 등) — 그 요청만의 거부다.
    Refused(String),
}

impl std::fmt::Display for StoredGrantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(message) | Self::Refused(message) => f.write_str(message),
        }
    }
}

// ★ 결함 112 (재검수 64) — `From<String> for StoredGrantError`(= Refused)를 두지 않는다. 있으면 저장소 호출에 `format!` 으로 감싼 `?` 를
//   더해도 컴파일되면서 거부로 분류된다. 지점마다 `Refused` · `Storage` 를 쓰게 해 분류 누락을 컴파일 오류로 만든다.

/// 파일로 내는 경로(`issue-grant`)는 문구만 쓴다.
impl From<StoredGrantError> for String {
    fn from(error: StoredGrantError) -> Self {
        error.to_string()
    }
}

/// 저장된 예약을 읽어 대조한 뒤 서명된 Grant 를 만든다.
///
/// nested Lease 를 **먼저** 완성해 서명한다 — 그래야 outer Grant 의 서명이
/// 최종 nested 바이트를 덮는다.
///
/// 제출자 서명 Manifest 는 `submitters` 로 **발급 시각 기준** 다시 검증한 뒤
/// 싣는다(모듈 문서 "Manifest 를 싣는다").
pub fn signed_grant_from_stored<K: KeyDirectory + ?Sized>(
    jobs: &CoordinatorJobStore,
    staging: &CoordinatorStagingStore,
    leases: &CoordinatorLeaseStore,
    request: &StoredGrantRequest,
    key: &SigningKey,
    submitters: &K,
) -> Result<pb::ExecutionGrant, StoredGrantError> {
    if request.issued_at_unix_ms >= request.expires_at_unix_ms {
        return Err(StoredGrantError::Refused(format!(
            "Grant 발급 시각({})이 만료({}) 보다 앞서지 않는다",
            request.issued_at_unix_ms, request.expires_at_unix_ms
        )));
    }

    let job = jobs
        .get(&request.job_id)
        .map_err(|e| StoredGrantError::Storage(format!("Job 조회 실패: {e}")))?
        .ok_or_else(|| {
            StoredGrantError::Refused(format!("GRANT_REFUSED: {} 를 모른다", request.job_id))
        })?;
    if job.state != JobState::Staging {
        return Err(StoredGrantError::Refused(format!(
            "GRANT_REFUSED: Job 이 STAGING 이 아니다(현재 {:?}) — 예약 없이 Grant 를 만들지 않는다",
            job.state
        )));
    }

    let attempt = staging
        .get_attempt(&request.attempt_id)
        .map_err(|e| StoredGrantError::Storage(format!("Attempt 조회 실패: {e}")))?
        .ok_or_else(|| {
            StoredGrantError::Refused(format!(
                "GRANT_REFUSED: Attempt {} 가 저장소에 없다",
                request.attempt_id
            ))
        })?;
    let stored_lease = leases
        .get(&request.lease_id)
        .map_err(|e| StoredGrantError::Storage(format!("Lease 조회 실패: {e}")))?
        .ok_or_else(|| {
            StoredGrantError::Refused(format!(
                "GRANT_REFUSED: Lease {} 가 저장소에 없다",
                request.lease_id
            ))
        })?;

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
            return Err(StoredGrantError::Refused(format!(
                "GRANT_REFUSED: Attempt 와 Lease 의 {label} 가 다르다(Attempt {left:?}, Lease {right:?}) — 두 행이 같은 예약을 가리키지 않는다"
            )));
        }
    }
    // ★★ **아래 두 대조는 테스트로 고정되지 않았다 — 왜인지 적는다.**
    //
    //   뮤테이션 G5(fence 대조 제거)·G6(예약 존재 확인 제거)를 걸어도
    //   `issue_grant` 10건이 전부 통과한다. 억지로 통과시킨 게 아니라
    //   **그 상태를 정상 경로로 만들 수 없어서**다 — Attempt·Lease·
    //   예약은 `staging_store.rs` 의 **한 `BEGIN IMMEDIATE` 안에서 함께**
    //   쓰인다(`:605`·`:915`·`:1985`). 셋이 어긋나려면 DB 를 직접
    //   조작해야 한다.
    //
    //   그래서 이건 **입력 검증이 아니라 손상 방어**다. 값어치가 없다는
    //   뜻은 아니지만, "테스트가 지키고 있다" 고 말하면 거짓이다.
    //   고정하려면 rusqlite 로 행을 직접 망가뜨리는 테스트가 필요하다.
    //   ★ 2026-09-17 정정 — 여기 "이 crate 의 통합 테스트에는 그 의존성이 없다" 고 적혀 있었다. 틀렸다 —
    //     `tests/attempt_report_ingress.rs` 가 rusqlite 로 행을 망가뜨린다(결함 104 · 111). 위 두 대조를 고정하는 테스트는 아직 없다.
    if attempt.fence_epoch != stored_lease.fence_epoch {
        return Err(StoredGrantError::Refused(format!(
            "GRANT_REFUSED: Attempt 와 Lease 의 fence epoch 가 다르다(Attempt {}, Lease {}) — 오래된 한쪽으로 Grant 를 만들면 fencing 이 무의미해진다",
            attempt.fence_epoch, stored_lease.fence_epoch
        )));
    }
    if attempt.job_id != request.job_id {
        return Err(StoredGrantError::Refused(format!(
            "GRANT_REFUSED: Attempt 가 다른 Job 의 것이다({} != {})",
            attempt.job_id, request.job_id
        )));
    }

    let reservation = staging
        .get_node_reservation(&stored_lease.holder_node_id)
        .map_err(|e| StoredGrantError::Storage(format!("예약 조회 실패: {e}")))?
        .ok_or_else(|| {
            StoredGrantError::Refused(format!(
                "GRANT_REFUSED: {} 에 예약이 없다 — Lease 는 있는데 노드가 안 잡혀 있다",
                stored_lease.holder_node_id
            ))
        })?;
    if reservation.job_id != request.job_id || reservation.attempt_id != request.attempt_id {
        return Err(StoredGrantError::Refused(format!(
            "GRANT_REFUSED: {} 의 예약은 다른 Attempt 의 것이다(job {}, attempt {})",
            stored_lease.holder_node_id, reservation.job_id, reservation.attempt_id
        )));
    }

    // ── 상태 관문 ───────────────────────────────────────────────────
    if let Some(revoked_at) = stored_lease.revoked_at_unix_ms {
        return Err(StoredGrantError::Refused(format!(
            "GRANT_REFUSED: Lease 가 {revoked_at} 에 폐기됐다 — 폐기된 Lease 로 Grant 를 만들지 않는다"
        )));
    }
    // ★ 경계 포함(`<=`) — `DoD-26`·`DoD-32`·`DoD-34` 가 정착시킨 규칙과
    //   같다. 정확히 만료 시각인 Lease 를 여기서만 유효로 보면, Agent 는
    //   같은 순간 그 Lease 를 거부한다.
    if stored_lease.expires_at_unix_ms <= request.issued_at_unix_ms {
        return Err(StoredGrantError::Refused(format!(
            "GRANT_REFUSED: 발급 시각({})에 Lease 가 이미 만료다(만료 {})",
            request.issued_at_unix_ms, stored_lease.expires_at_unix_ms
        )));
    }
    if request.expires_at_unix_ms > stored_lease.expires_at_unix_ms {
        return Err(StoredGrantError::Refused(format!(
            "GRANT_REFUSED: Grant 만료({})가 Lease 만료({}) 보다 늦다",
            request.expires_at_unix_ms, stored_lease.expires_at_unix_ms
        )));
    }

    // ── 제출자 서명 Manifest — 싣기 전에 **지금** 다시 검증한다 ──────
    //
    // 저장소가 보는 것(본문·job_id·제출자·서명자·hash 일치)은 다시 보지 않는다 —
    // 어긋나면 `get_manifest_binding()` 이 오류를 낸다.
    let binding = jobs
        .get_manifest_binding(&request.job_id)
        // 저장된 Manifest 를 읽지 못한 것은 저장소 장애다(결함 104) — 없는 것은 거부다.
        // ★ 결함 111 (재검수 64) — Job 은 있고 Manifest 행만 없는 옛 hash-only Job 은 `Ok(None)` 이 아니라 `LegacyManifestMissing` 이다.
        //   그것까지 Storage 로 묶어 옛 Job 하나가 리스너를 멈췄다. 본문 손상 · I/O · 락은 그대로 Storage 다.
        // ★ 결함 122 (재검수 64b) — 한계: **현대 Job 의 Manifest 행이 유실된** 손상도 이 분기로 와 거부(Refused)가 된다. 저장 형식에
        //   "Manifest 와 함께 제출됐다" 는 출처 표식이 없어 옛 Job 과 가를 수 없다. 가르려면 저장 형식을 바꿔야 한다 — 이번에 하지 않는다.
        .map_err(|e| match e {
            JobStoreError::LegacyManifestMissing { job_id } => StoredGrantError::Refused(format!(
                "GRANT_REFUSED: {job_id} 는 서명된 Manifest 없이 저장된 옛 Job 이다 — 실을 Manifest 가 없다"
            )),
            other => StoredGrantError::Storage(format!("저장된 Manifest 를 읽지 못했다: {other}")),
        })?
        .ok_or_else(|| {
            StoredGrantError::Refused(format!(
                "GRANT_REFUSED: {} 의 저장된 Manifest 가 없다",
                request.job_id
            ))
        })?;
    let verified = gputeer_protocol::verify(
        &binding.manifest,
        1,
        &Ed25519Verifier::new(submitters),
        request.issued_at_unix_ms,
        &mut gputeer_protocol::signing::NoReplayCheck,
    )
    .map_err(|e| {
        StoredGrantError::Refused(format!(
            "GRANT_REFUSED: 저장된 Manifest 를 지금 다시 검증하지 못했다: {e:?} — 저장될 때는 유효했더라도 그 사이 제출자가 신뢰 목록에서 빠졌거나 Manifest 가 만료됐을 수 있다"
        ))
    })?;
    // ★ Agent 는 **받는 시각**으로 Manifest 를 검증한다. Grant 가 Manifest 보다
    //   오래 살면 Grant 는 유효한데 Manifest 는 만료된 구간이 생긴다(설계 논의 36).
    //   Lease 경계와 같은 모양으로 막는다.
    let manifest_expires_at = verified.get().expires_at_unix_ms;
    if request.expires_at_unix_ms > manifest_expires_at {
        return Err(StoredGrantError::Refused(format!(
            "GRANT_REFUSED: Grant 만료({})가 Manifest 만료({manifest_expires_at}) 보다 늦다",
            request.expires_at_unix_ms
        )));
    }

    // ── 서명 ────────────────────────────────────────────────────────
    // 저장값을 wire 형으로 옮기지 못한 것(u64 -> u32 범위)은 저장소 손상이다(결함 104).
    let mut lease = unsigned_lease_from_stored(&stored_lease).map_err(StoredGrantError::Storage)?;
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
        // ★ outer 서명 **전에** 싣는다 — 그래야 Grant 서명이 최종 nested 바이트와
        //   hash 를 덮는다. hash 는 저장소가 재계산해 대조한 값을 그대로 쓴다.
        manifest_hash: Some(pb::Digest {
            algo: 1, // HASH_ALGORITHM_BLAKE3_256
            value: binding.manifest_hash.to_vec(),
        }),
        manifest: Some(binding.manifest),
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
    //   확인하는 것과 못 하는 것을 정확히 적는다:
    //
    //   확인한다   구조·`schema_version`·수명·canonical 인코딩,
    //              그리고 **서명이 실제로 검증되는가**(2026-09-07 추가).
    //              nested Lease 도 같이 본다.
    //   못 한다    **이 키가 정말 `issuing_coordinator_id` 의 것인가.**
    //              이 모듈에는 key directory 가 없다 — 호출부가 준
    //              키를 그대로 쓴다.
    //
    //   ★★ **"위험하지 않다" 고 적었던 것을 고친다** (2026-09-07 독립
    //     검수 지적). 엉뚱한 키로 서명하면 여기서는 통과하고 나중에
    //     Agent 가 거부한다. 인증 우회는 아니지만 **위험이 없는 것은
    //     아니다**:
    //
    //       · 운영자는 유효한 Grant 를 만든 줄 안다
    //       · 실패가 Agent 단계까지 밀려 원인 파악이 늦어진다
    //       · `--out` 에 기존 파일이 있었다면 못 쓰는 Grant 로 덮인다
    //
    //     닫으려면 Coordinator 공개키 목록이 필요하고 아직 없다.
    // 자기 검증 실패는 저장소 장애가 아니다 — 메모리 keyring 으로 보고, 입력 정책(수명 상한 등) 실패도 포함한다(재검수 64).
    verify_own_output(&grant, key, request.issued_at_unix_ms).map_err(StoredGrantError::Refused)?;
    Ok(grant)
}

/// 방금 서명한 Grant 를 **서명까지** 자기 검증한다.
///
/// ★★ **2026-09-07 독립 검수가 이 함수를 반려했다.** 전에는
///   `AlwaysValid`(서명을 읽지도 않는 검증자)를 썼다. 검수가 반례를
///   보였다 —
///
///   > `sign()` 이 버그 때문에 잘못된 preimage 를 서명하거나 64바이트
///   > 쓰레기를 반환해도, 검증기가 서명을 **읽지 않으므로** 통과한다.
///
///   즉 그 검사는 "서명 생성과 검증 양쪽에 같은 버그가 있으면 못 잡는다"
///   보다도 약했다. **생성기의 단독 버그조차 못 잡았다.**
///
///   지금은 서명에 쓴 키에서 공개키를 도출해 **실제로 검증한다.**
///   그러면 서명 생성과 canonical preimage 의 불일치를 잡는다.
///
/// ★ 여전히 못 하는 것은 그대로다 — **이 공개키가 정말
///   `issuing_coordinator_id` 의 신뢰된 키인가.** 이 모듈에는 key
///   directory 가 없고 호출부가 준 키를 그대로 쓴다. 그건 별도 검사가
///   필요하고 이 저장소에 아직 없다.
fn verify_own_output(
    grant: &pb::ExecutionGrant,
    key: &gputeer_crypto::SigningKey,
    at_unix_ms: u64,
) -> Result<(), String> {
    // 서명에 쓴 키의 공개키를 두 신원 모두에 등록한다 — Grant 와 nested
    // Lease 는 서명자 이름이 다를 수 있는데, 여기서 확인하려는 것은
    // "이 키로 서명한 것이 그대로 검증되는가" 뿐이다.
    let mut ring = gputeer_crypto::InMemoryKeyring::new();
    let public = key.verifying_key();
    ring.insert(grant.coordinator_device_id.clone(), public);
    if let Some(lease) = grant.lease.as_ref() {
        ring.insert(lease.issuing_coordinator_id.clone(), public);
    }
    let verifier = gputeer_crypto::Ed25519Verifier::new(&ring);

    gputeer_protocol::verify(
        grant,
        2,
        &verifier,
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
        &verifier,
        at_unix_ms,
        &mut gputeer_protocol::signing::NoReplayCheck,
    )
    .map_err(|e| format!("방금 만든 nested Lease 가 자기 검증을 통과하지 못했다: {e:?}"))?;
    Ok(())
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
