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
//! # Manifest 를 싣는다 (2026-09-10)
//!
//! 저장된 제출자 서명 Manifest 를 `--submitter-keyring` 으로 **발급 시각 기준**
//! 다시 검증한 뒤 싣는다(`grant_from_stored` 모듈 문서). 평문 keyring 은
//! `--i-understand-plaintext-keyring-is-unsafe true` 로만 받는다.
//!
//! # 이 명령이 하지 않는 것
//!
//! ```text
//! 안 한다   Agent 에게 보내기       wire dispatch 는 ④ 다. 파일로 낸다
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

use gputeer_coordinator::grant_from_stored::{
    derive_stored_grant_nonce, signed_grant_from_stored, StoredGrantRequest,
};
use gputeer_coordinator::job_store::CoordinatorJobStore;
use gputeer_coordinator::lease_store::CoordinatorLeaseStore;
use gputeer_coordinator::staging_store::CoordinatorStagingStore;
use gputeer_crypto::{PersistentKeyring, PlaintextPolicy, SigningKey};
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
    // ★ 기본값은 **덮어쓰지 않는다.** 위 `write_grant_file` 주석 참조.
    let overwrite = matches!(
        flags.get("--overwrite-existing-grant").map(String::as_str),
        Some("true")
    );

    // ★ Grant 시각도 절대값이다. `stage-job` 에서 배운 것과 같은 이유 —
    //   실시간 시각을 결과에 반영하면 같은 명시적 입력으로도 Grant 가 달라질 수
    //   있다. 시각을 통제하고 결과를 재현하기 위해 절대값을 인자로 받는다
    //   (재검수 30·31 — 한때 "테스트로 고정할 수 없다" 고 적었는데, 시계 대역을
    //   고정하면 테스트할 수 있다. 설계 이유를 다른 방법의 불가능으로 넓힌 것이었다).
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
    drop(jobs);
    let jobs = CoordinatorJobStore::open(control_db)
        .map_err(|e| format!("job store 를 다시 열지 못했다({control_db}): {e}"))?;
    let staging = CoordinatorStagingStore::open(control_db)
        .map_err(|e| format!("staging store 를 열지 못했다({control_db}): {e}"))?;
    let leases = CoordinatorLeaseStore::open(control_db)
        .map_err(|e| format!("lease store 를 열지 못했다({control_db}): {e}"))?;

    // ── 제출자 keyring — 저장된 Manifest 를 **지금** 다시 검증한다 ────
    //
    // ★ 키·저장소 검사 **뒤에** 연다. 앞의 거부(키 hex · 비영속 DB)가 먼저 나와야
    //   그 테스트들이 자기 관문을 잰다.
    let submitters = load_submitter_keyring(&flags)?;

    // ★ 대조와 조립은 **coordinator 가 한다.** `coordinator-stub` 도 같은
    //   함수를 부르므로 두 벌이 생기지 않는다(모듈 문서 참조).
    let grant = signed_grant_from_stored(
        &jobs,
        &staging,
        &leases,
        &StoredGrantRequest {
            job_id: job_id.to_string(),
            attempt_id: attempt_id.to_string(),
            lease_id: lease_id.to_string(),
            grant_id: grant_id.to_string(),
            reissue_unacknowledged_start: false,
            issued_at_unix_ms,
            expires_at_unix_ms,
            // 파일로 내는 경로에는 연결 개념이 없다 — 같은 입력이면
            // 같은 Grant 가 나오도록 결정적으로 유도한다.
            nonce: derive_stored_grant_nonce(grant_id, attempt_id),
        },
        &key,
        &submitters,
    )?;
    let lease = grant.lease.as_ref().expect("서명 경로가 Lease 를 넣는다");
    let summary = format!(
        "GRANTED job_id={job_id} grant_id={grant_id} attempt={} lease={} node={} fence={} term={} out={out_path}",
        grant.attempt_id,
        lease.lease_id,
        lease.holder_node_id,
        lease.fence_epoch,
        grant.coordinator_term
    );

    // ★ 공유 도우미를 쓴다 — `crate::out_file` 주석에 왜인지 적어 뒀다.
    let warning = crate::out_file::write_new(
        out_path,
        &grant.encode_to_vec(),
        overwrite,
        "GRANT_REFUSED",
        "Grant",
    )?;

    // ★ 결함 ⑰ — 경고는 도우미가 출력하지 않는다. 요약에 싣는다.
    Ok(crate::out_file::with_warning(summary, warning))
}

/// 서명키를 **파일에서만** 읽는다. 명령줄에 남기지 않는 것이 목적이다.
fn load_signing_key(path: &str) -> Result<SigningKey, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("서명키 파일을 열지 못했다({path}): {e}"))?;
    let hex = raw.trim();
    if hex.len() != 64 {
        return Err(format!(
            "서명키 파일은 64자리 hex(32바이트) 하나여야 한다({path}, 길이 {})",
            hex.len()
        ));
    }
    // ★ 결함 ⑬(2026-09-10) — 바이트로 자르지 않는다. `gputeer_crypto::hex`
    //   가 한 바이트씩 읽으므로 문자 경계를 가를 수 없다.
    let seed = gputeer_crypto::hex::decode_fixed::<32>(hex)
        .map_err(|e| format!("GRANT_REFUSED: KEY_NOT_HEX — 서명키 hex 파싱 실패({path}): {e}"))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn u64_flag(flags: &BTreeMap<String, String>, key: &str) -> Result<u64, String> {
    require(flags, key)?
        .parse::<u64>()
        .map_err(|e| format!("{key} 파싱 실패: {e}"))
}

/// 저장된 Manifest 를 다시 검증할 제출자 keyring 을 연다.
///
/// `plan-job`·`scheduler-tick` 과 같은 규칙 — 평문(K0)은 명시적으로 허용했을 때만.
fn load_submitter_keyring(flags: &BTreeMap<String, String>) -> Result<PersistentKeyring, String> {
    let path = require(flags, "--submitter-keyring")?;
    let policy = if flags
        .get("--i-understand-plaintext-keyring-is-unsafe")
        .map(String::as_str)
        == Some("true")
    {
        eprintln!(
            "경고: 평문(K0) 제출자 keyring 을 허용했다 — 이 파일을 쓸 수 있는 사람은 임의의 공개키를 신뢰 목록에 넣을 수 있다"
        );
        PlaintextPolicy::Allow
    } else {
        PlaintextPolicy::Reject
    };
    PersistentKeyring::load(path, policy)
        .map_err(|e| format!("제출자 keyring 을 열지 못했다({path}): {e:?}"))
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
