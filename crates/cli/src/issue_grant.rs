//! `gputeer issue-grant` — 예약된 Job 의 **저장된 Lease** 로 서명된
//! `ExecutionGrant` 를 만든다.
//!
//! ```text
//! stage-job    CAS 예약 + Attempt/Lease/fence epoch          (STAGING)
//! issue-grant  저장된 것을 읽어 Grant 를 서명한다             <- 여기
//! ```
//!
//! # ★ 지금까지 Grant 는 명령줄 값으로 만들어졌다
//!
//! `coordinator-stub` 의 `issue_grant()` 는 `config.grant_id`·
//! `config.attempt_id` 를 그대로 쓰고 **`coordinator_term` 을 리터럴 1
//! 로 박아 넣는다.** selftest 를 재현 가능하게 만드는 데는 충분하지만,
//! 그 Grant 는 저장소가 아는 사실과 아무 관계가 없다.
//!
//! 이 명령은 반대다 — **저장된 것만 쓴다.** Attempt·Lease·fence epoch·
//! coordinator term 이 전부 `stage-job` 이 커밋한 행에서 나온다.
//!
//! # ★ 한 행만 믿지 않는다
//!
//! Attempt 와 Lease 는 다른 행이다. 둘이 어긋나 있으면 그 자체가 사실
//! 이고, 그때 한쪽만 읽으면 없는 일관성을 가정하게 된다. 그래서 발급
//! 전에 대조한다:
//!
//! ```text
//! Attempt.job_id      == Lease.job_id
//! Attempt.attempt_id  == Lease.attempt_id
//! Attempt.fence_epoch == Lease.fence_epoch
//! Attempt.lease_id    == Lease.lease_id
//! 예약(node)          == Lease.holder_node_id 와 같은 노드
//! ```
//!
//! # 이 명령이 하지 않는 것
//!
//! ```text
//! 안 한다   Agent 에게 보내기       wire dispatch 는 ④ 다. 파일로 낸다
//! 안 한다   Manifest 를 싣기        Grant 에 Manifest 를 실으려면
//!                                   제출자 서명 원본이 필요하고, 그건
//!                                   별도 조각이다
//! 안 한다   ResourceScope 채우기    GPU scope 는 authoritative provenance
//!                                   가 없어 막혀 있다(`DoD-55`)
//! ```
//!
//! # ★ 개인키를 명령줄에 두지 않는다
//!
//! 이 저장소의 기존 관례(`--own-seed <hex32>`)는 **개인키가 프로세스
//! 목록에 뜬다.** 같은 기계의 다른 사용자가 `ps`/작업 관리자로 본다.
//! 그래서 이 명령은 **파일 경로**를 받는다 — 파일 권한이 경계가 된다.
//!
//! ★ 그렇다고 안전한 것은 아니다. 평문 seed 파일은 `KeyProtection::K0`
//!   이고, 이 저장소가 K1(DPAPI·systemd-creds)을 가졌지만 그 저장소에서
//!   **개인키를 꺼내 서명하는 공개 경로가 아직 없다.** 명령줄보다 낫다는
//!   것이지 보호된다는 뜻이 아니다.

use std::collections::BTreeMap;

use gputeer_coordinator::job_store::{CoordinatorJobStore, JobState};
use gputeer_coordinator::lease_store::CoordinatorLeaseStore;
use gputeer_coordinator::staging_store::CoordinatorStagingStore;
use gputeer_coordinator::unsigned_lease_from_stored;
use gputeer_crypto::{sign, SigningKey};
use gputeer_protocol::pb;
use prost::Message;

/// `gputeer issue-grant` 진입점.
pub fn run(args: &[String]) -> Result<String, String> {
    let flags = parse_flags(args)?;

    let job_id = require(&flags, "--job-id")?;
    let control_db = require(&flags, "--control-db")?;
    let attempt_id = require(&flags, "--attempt-id")?;
    let lease_id = require(&flags, "--lease-id")?;
    let grant_id = require(&flags, "--grant-id")?;
    let out_path = require(&flags, "--out")?;

    // ★ Grant 시각도 절대값이다. `stage-job` 에서 배운 것과 같은 이유 —
    //   시계를 읽으면 같은 입력으로 두 번 발급했을 때 결과가 달라지고,
    //   그러면 "같은 입력이면 같은 Grant" 를 확인할 방법이 없어진다.
    let issued_at_unix_ms = u64_flag(&flags, "--grant-issued-at-unix-ms")?;
    let expires_at_unix_ms = u64_flag(&flags, "--grant-expires-at-unix-ms")?;
    if issued_at_unix_ms >= expires_at_unix_ms {
        return Err(format!(
            "Grant 발급 시각({issued_at_unix_ms})이 만료({expires_at_unix_ms}) 보다 앞서지 않는다"
        ));
    }

    let key = load_signing_key(require(&flags, "--coordinator-key-file")?)?;

    // ── 저장된 사실을 읽는다 ────────────────────────────────────────
    let jobs = CoordinatorJobStore::open(control_db)
        .map_err(|e| format!("job store 를 열지 못했다({control_db}): {e}"))?;
    if !jobs.is_durable() {
        return Err(format!(
            "--control-db 가 영속이 아니다({control_db:?}) — 없는 예약으로 Grant 를 만들게 된다"
        ));
    }
    let job = jobs
        .get(job_id)
        .map_err(|e| format!("Job 조회 실패: {e}"))?
        .ok_or_else(|| format!("GRANT_REFUSED: {job_id} 를 모른다"))?;
    // ★ `STAGING` 이 아니면 발급하지 않는다. 예약이 없는데 Grant 를
    //   내면 Agent 는 자기가 그 노드를 쓸 권한이 있다고 믿는다.
    if job.state != JobState::Staging {
        return Err(format!(
            "GRANT_REFUSED: Job 이 STAGING 이 아니다(현재 {:?}) — 예약 없이 Grant 를 만들지 않는다",
            job.state
        ));
    }
    drop(jobs);

    let staging = CoordinatorStagingStore::open(control_db)
        .map_err(|e| format!("staging store 를 열지 못했다({control_db}): {e}"))?;
    let attempt = staging
        .get_attempt(attempt_id)
        .map_err(|e| format!("Attempt 조회 실패: {e}"))?
        .ok_or_else(|| format!("GRANT_REFUSED: Attempt {attempt_id} 가 저장소에 없다"))?;

    let leases = CoordinatorLeaseStore::open(control_db)
        .map_err(|e| format!("lease store 를 열지 못했다({control_db}): {e}"))?;
    let stored_lease = leases
        .get(lease_id)
        .map_err(|e| format!("Lease 조회 실패: {e}"))?
        .ok_or_else(|| format!("GRANT_REFUSED: Lease {lease_id} 가 저장소에 없다"))?;

    // ── 한 행만 믿지 않는다 ─────────────────────────────────────────
    for (label, left, right) in [
        ("job_id", attempt.job_id.as_str(), stored_lease.job_id.as_str()),
        (
            "attempt_id",
            attempt.attempt_id.as_str(),
            stored_lease.attempt_id.as_str(),
        ),
        ("lease_id", attempt.lease_id.as_str(), stored_lease.lease_id.as_str()),
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
    if attempt.job_id != job_id {
        return Err(format!(
            "GRANT_REFUSED: Attempt 가 다른 Job 의 것이다({} != {job_id})",
            attempt.job_id
        ));
    }

    // 예약이 실제로 이 Lease 의 보유 노드에 걸려 있는가.
    let reservation = staging
        .get_node_reservation(&stored_lease.holder_node_id)
        .map_err(|e| format!("예약 조회 실패: {e}"))?
        .ok_or_else(|| {
            format!(
                "GRANT_REFUSED: {} 에 예약이 없다 — Lease 는 있는데 노드가 안 잡혀 있다",
                stored_lease.holder_node_id
            )
        })?;
    if reservation.job_id != job_id || reservation.attempt_id != attempt_id {
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
    if stored_lease.expires_at_unix_ms <= issued_at_unix_ms {
        return Err(format!(
            "GRANT_REFUSED: 발급 시각({issued_at_unix_ms})에 Lease 가 이미 만료다(만료 {})",
            stored_lease.expires_at_unix_ms
        ));
    }
    // Grant 가 Lease 보다 오래 살면 Agent 는 Lease 없이 계속 돌아도
    // 된다고 읽을 수 있다.
    if expires_at_unix_ms > stored_lease.expires_at_unix_ms {
        return Err(format!(
            "GRANT_REFUSED: Grant 만료({expires_at_unix_ms})가 Lease 만료({}) 보다 늦다",
            stored_lease.expires_at_unix_ms
        ));
    }

    // ── 서명 ────────────────────────────────────────────────────────
    //
    // ★ nested Lease 를 **먼저** 완성해 서명한다. 그래야 outer Grant 의
    //   서명이 최종 nested 바이트를 덮는다(`issue_grant()` 의 주석과
    //   같은 이유).
    let mut lease = unsigned_lease_from_stored(&stored_lease)?;
    lease.coordinator_signature = sign(&key, &lease).to_vec();

    let mut grant = pb::ExecutionGrant {
        schema_version: 2,
        grant_id: grant_id.to_string(),
        // ★ **이 값은 명령줄과 같다** — `get_attempt()` 를 그 값으로
        //   조회했으므로 다를 수가 없다. 처음에는 "저장된 Attempt 에서
        //   온다 — 명령줄이 아니다" 라고 썼는데 **과장이었다**(내
        //   뮤테이션 G2 가 잡았다: 명령줄 값으로 바꿔도 아무 테스트도
        //   실패하지 않는다).
        //
        //   저장소에서만 오는 것은 아래 셋이다 — `coordinator_term`·
        //   nested Lease 의 `fence_epoch`·`holder_node_id`. 그것들이
        //   이 조각의 값어치다.
        //
        //   그래도 저장된 행을 쓰는 이유는, 조회가 성공했다는 사실이
        //   **그 Attempt 가 실재한다**는 뜻이기 때문이다.
        attempt_id: attempt.attempt_id.clone(),
        coordinator_device_id: stored_lease.issuing_coordinator_id.clone(),
        // ★ 리터럴 1 이 아니라 저장된 term 이다.
        coordinator_term: stored_lease.coordinator_term,
        issued_at_unix_ms,
        expires_at_unix_ms,
        nonce: derive_nonce(grant_id, &attempt.attempt_id),
        // 이 비트는 v2 Grant 서명 대상이다. 이 명령은 **저장소에서만**
        // Lease 를 읽으므로 정직하게 true 다.
        lease_from_durable_store: true,
        lease: Some(lease),
        ..Default::default()
    };
    grant.coordinator_signature = sign(&key, &grant).to_vec();

    std::fs::write(out_path, grant.encode_to_vec())
        .map_err(|e| format!("Grant 파일 쓰기 실패({out_path}): {e}"))?;

    Ok(format!(
        "GRANTED job_id={job_id} grant_id={grant_id} attempt={} lease={} node={} fence={} term={} out={out_path}",
        attempt.attempt_id,
        stored_lease.lease_id,
        stored_lease.holder_node_id,
        stored_lease.fence_epoch,
        stored_lease.coordinator_term
    ))
}

/// `(grant_id, attempt_id)` 에서 16바이트 nonce 를 결정적으로 뽑는다.
///
/// ★ **이것은 무작위가 아니다.** 같은 입력이면 같은 Grant 를 내야
///   재발급이 멱등해지고, 그것을 테스트로 확인할 수 있다. 대신 재생
///   방어는 nonce 의 예측 불가능성이 아니라 **수신 측의 replay 저장소**
///   가 맡는다(`crates/crypto` 의 `DurableReplayGuard`).
///
/// 길이 프리픽스를 붙여 인접 값이 서로 스며들지 않게 한다.
fn derive_nonce(grant_id: &str, attempt_id: &str) -> Vec<u8> {
    let mut input = Vec::new();
    for part in [b"gputeer/v1/grant-nonce".as_slice(), grant_id.as_bytes(), attempt_id.as_bytes()] {
        input.extend_from_slice(&(part.len() as u64).to_le_bytes());
        input.extend_from_slice(part);
    }
    gputeer_protocol::canonical::blake3_256(&input)[..16].to_vec()
}

/// 서명키를 **파일에서** 읽는다. 명령줄에 두지 않는 이유는 모듈 문서 참조.
fn load_signing_key(path: &str) -> Result<SigningKey, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("서명키 파일을 읽지 못했다({path}): {e}"))?;
    let hex = raw.trim();
    if hex.len() != 64 {
        return Err(format!(
            "서명키 파일은 64자리 hex(32바이트) 하나여야 한다({path}, 길이 {})",
            hex.len()
        ));
    }
    let mut seed = [0u8; 32];
    for (i, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("서명키 hex 파싱 실패({path}): {e}"))?;
    }
    Ok(SigningKey::from_bytes(&seed))
}

fn u64_flag(flags: &BTreeMap<String, String>, key: &str) -> Result<u64, String> {
    require(flags, key)?
        .parse::<u64>()
        .map_err(|e| format!("{key} 파싱 실패: {e}"))
}

fn parse_flags(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let key = &args[i];
        if !key.starts_with("--") {
            return Err(format!("알 수 없는 인자: {key}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("{key} 에 값이 없다"))?;
        out.insert(key.clone(), value.clone());
        i += 2;
    }
    Ok(out)
}

fn require<'a>(flags: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    flags
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("{key} 가 필요하다"))
}
