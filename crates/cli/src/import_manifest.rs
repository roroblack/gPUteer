//! `gputeer import-manifest` — 운영자가 서명된 Manifest 를 durable job
//! store 에 **반입**한다.
//!
//! # 왜 "제출 접수" 가 아니라 "반입" 인가
//!
//! `DoD-50` 이 `submit_verified_manifest()` 를 만들면서 "production wire
//! 연결은 범위 밖" 이라고 적어 뒀다 — 그 저장소는 **아무도 부르지 않는다.**
//! 이 명령이 첫 소비자다.
//!
//! ★ 그러나 이건 제출 **접수**가 아니다. 독립 설계 조사가 정확히 짚었다:
//!
//! > 임의 공개키까지 같은 명령에서 받아 "verified/accepted" 라고 부르는
//! > 것은 정직하지 않다.
//!
//! 명령줄로 공개키를 받으면 **신뢰 경계가 그 명령 한 줄**이 된다. 아무나
//! 자기 키를 붙여 "검증됨" 을 만들 수 있고, 그건 검증이 아니라 서명 확인일
//! 뿐이다. 그래서 이 명령은 **운영자가 미리 provision 한 keyring 파일**에
//! 있는 서명자만 받는다.
//!
//! ```text
//! 안 한다   네트워크로 오는 제출을 받아들이기
//! 안 한다   누가 제출할 자격이 있는지 판정하기(멤버십)
//! 안 한다   명령줄 공개키를 신뢰하기
//!
//! 한다      keyring 파일에 있는 서명자만 받기
//! 한다      검증한 **뒤에만** 필드를 읽고 durable 저장소에 넣기
//! 한다      모르는 서명자·위조 서명·만료를 각각 구분해 거부하기
//! ```
//!
//! 신뢰 경계가 **운영자가 소유하는 파일**이다. 거기 누구를 넣을지는
//! 운영자가 정하고 이 명령은 그 결정을 실행할 뿐이다 — `PersistentKeyring`
//! 이 `revoke()`·`quarantine()` 을 갖지만 그건 **로컬 운영자의 목록**이지
//! 팀 합의가 아니다. 멤버십 권위는 여전히 없다.

use std::collections::BTreeMap;

use gputeer_coordinator::job_store::{AcceptedJobSubmission, CoordinatorJobStore};
use gputeer_crypto::{Ed25519Verifier, PersistentKeyring, PlaintextPolicy};
use gputeer_protocol::{
    pb,
    signing::{verify, NoReplayCheck},
};
use prost::Message;

/// `gputeer import-manifest` 진입점.
pub fn run(args: &[String]) -> Result<String, String> {
    let flags = parse_flags(args)?;

    let manifest_path = require(&flags, "--manifest")?;
    let keyring_path = require(&flags, "--submitter-keyring")?;
    let job_db_path = require(&flags, "--job-db")?;
    let idempotency_key = parse_idempotency_key(require(&flags, "--idempotency-key")?)?;

    let bytes = std::fs::read(&manifest_path)
        .map_err(|e| format!("Manifest 파일을 읽지 못했다({manifest_path}): {e}"))?;
    let manifest = pb::JobManifest::decode(bytes.as_slice())
        .map_err(|e| format!("Manifest 가 protobuf 로 해석되지 않는다({manifest_path}): {e}"))?;

    // 운영자가 provision 한 신뢰 목록. 여기 없는 서명자는 거부된다.
    //
    // ★ 평문(K0) keyring 은 **기본으로 거부한다.** 이 파일은 "누구를
    //   믿는가" 의 정본이므로, 파일을 쓸 수 있는 사람이 자기 공개키를
    //   넣으면 아무 Manifest 나 "검증됨" 이 된다. 보호되지 않은 목록을
    //   쓰려면 그 위험을 **명시적으로 진술**해야 한다(`DoD-29` 의
    //   `--i-understand-legacy-mode-is-unsafe` 와 같은 모양).
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
    let keyring = PersistentKeyring::load(&keyring_path, policy)
        .map_err(|e| format!("제출자 keyring 을 열지 못했다({keyring_path}): {e:?}"))?;
    let verifier = Ed25519Verifier::new(&keyring);

    let now_unix_ms = now_unix_ms();

    // ★ **여기 위에서는 `manifest` 의 어떤 필드도 읽지 않았다.**
    //   `CLAUDE.md` §0.2 — 검증 전에 값을 로직에 쓰지 않는다. 로그에
    //   찍는 것만으로도 그 규율은 깨진다.
    //
    //   ★ replay 방어는 `NoReplayCheck` 다. `JobManifest` 는 장수명
    //     메시지라 nonce 가 없다 — 같은 Manifest 를 두 번 반입하는 것은
    //     재생 공격이 아니라 **멱등 재시도**이며, 그건 저장소의
    //     idempotency key 가 다룬다.
    let verified = verify(&manifest, 1, &verifier, now_unix_ms, &mut NoReplayCheck)
        .map_err(|e| format!("MANIFEST_REJECTED: {}: {e:?}", rejection_reason(&e)))?;

    let m = verified.get();
    let job_id = m.job_id.clone();
    let submitter_device_id = m.submitter_device_id.clone();
    let deadline_unix_ms = minutes_after(now_unix_ms, m.deadline_minutes);
    let max_queue_duration_ms = duration_ms(m.max_queue_minutes);

    // caller 가 hash 를 지어내지 않는다 — 저장소가 재계산해 대조한다.
    let manifest_hash = gputeer_protocol::canonical::blake3_256(
        &gputeer_protocol::signing::signing_input(&manifest),
    );

    let submission = AcceptedJobSubmission {
        idempotency_key,
        job_id: job_id.clone(),
        submitter_device_id: submitter_device_id.clone(),
        manifest_hash,
        deadline_unix_ms,
        max_queue_duration_ms,
    };

    let mut store = CoordinatorJobStore::open(&job_db_path)
        .map_err(|e| format!("job store 를 열지 못했다({job_db_path}): {e}"))?;

    // 영속성 판정을 여기서 다시 만들지 않는다 — 저장소가 이미 안다.
    //
    // 처음에는 ":memory:" 문자열만 손으로 비교했는데, 독립 검수가
    // **빈 경로**를 짚었다 — SQLite 는 그것도 임시 DB 로 열어 주므로
    // `--job-db ""` 가 성공으로 끝났다. 같은 판정을 두 곳에 두면 한쪽만
    // 낡는다.
    //
    // 넣은 것이 프로세스와 함께 사라지면 넣은 것이 아니다 — 그런데 로그
    // 에는 성공으로 찍힌다. 조용한 무시는 실패보다 나쁘다.
    if !store.is_durable() {
        return Err(format!(
            "--job-db 가 영속이 아니다({job_db_path:?}) — 반입한 Job 이 프로세스와 함께 사라지는데 로그에는 저장한 것처럼 찍힌다"
        ));
    }
    let result = store
        .submit_verified_manifest(&submission, &verified, now_unix_ms)
        .map_err(|e| format!("MANIFEST_REJECTED: 저장 실패: {e}"))?;

    Ok(format!(
        "IMPORTED job_id={job_id} submitter={submitter_device_id} state={:?} created={}",
        result.job.state, result.created
    ))
}

/// 검증 실패를 **운영자가 무엇을 고쳐야 하는지**로 나눈다.
///
/// 대응이 전혀 다르다:
///   · 쓸 수 있는 키 없음 -> **왜** 없는지부터 봐야 한다. 미등록이면
///     넣으면 되지만 **폐기·격리라면 넣는 것이 틀린 대응**이다 —
///     운영자가 그 서명자를 일부러 뺐다는 뜻이다.
///   · 위조 서명          -> 파일이 손상됐거나 누가 손댔다
///   · 만료               -> 제출자가 다시 서명해야 한다
///
/// ★ 처음에는 전부 "서명 검증 실패" 라고 적었다. **만료된 정상 서명**을
///   서명 실패라고 부르면 운영자가 엉뚱한 것을 고치러 간다
///   (`CLAUDE.md` §3 — 오류 메시지가 사실을 잘못 전하지 않게 한다).
///
/// ★ 그 다음에는 `UnknownSigner` 를 "목록에 없다" 로 단정했다(독립 검수
///   2라운드). 검증 계층은 미등록·폐기·회전 만료·격리를 **같은 결과로
///   합친다** — 이 명령은 그 넷을 나눌 수 없으므로 나눌 수 있는 척하지
///   않고 넷을 다 말한다.
fn rejection_reason(error: &gputeer_protocol::signing::VerifyError) -> &'static str {
    use gputeer_protocol::VerifyOutcome;
    match error.outcome() {
        // ★ `UnknownSigner` 는 **미등록만이 아니다**(독립 검수 2라운드
        //   지적) — 폐기(revoke)·회전 만료·격리(quarantine)된 서명자도
        //   같은 결과다. "목록에 없다" 고 단정하면 목록에 **있는데**
        //   폐기된 서명자를 운영자가 잘못 찾으러 간다.
        Some(VerifyOutcome::UnknownSigner) => {
            "신뢰 목록에서 쓸 수 있는 키를 찾지 못했다(미등록·폐기·회전 만료·격리)"
        }
        Some(VerifyOutcome::InvalidSignature) => "서명이 맞지 않는다",
        Some(VerifyOutcome::Expired) => "Manifest 가 만료됐다",
        Some(VerifyOutcome::SchemaTooNew) => "이 빌드가 모르는 schema_version",
        Some(_) => "프로토콜 검증 실패",
        // 서명 자체가 아니라 로컬 정책·저장소·파생 해시 문제다.
        None => "검증을 완료하지 못했다",
    }
}

/// `deadline_minutes == 0` 은 "마감 없음" 이다 — 0 을 "지금 마감" 으로
/// 바꾸면 모든 Job 이 즉시 만료된다.
fn minutes_after(now_unix_ms: u64, minutes: u32) -> Option<u64> {
    if minutes == 0 {
        return None;
    }
    Some(now_unix_ms.saturating_add(u64::from(minutes) * 60_000))
}

/// `max_queue_minutes == 0` 은 규범상 "독립적인 큐 타임아웃 없음" 이고
/// 마감이 지배한다(`job_store.rs` 의 `max_queue_duration_ms` 주석).
fn duration_ms(minutes: u32) -> Option<u64> {
    if minutes == 0 {
        return None;
    }
    Some(u64::from(minutes) * 60_000)
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as u64
}

fn parse_idempotency_key(hex: &str) -> Result<[u8; 16], String> {
    if hex.len() != 32 {
        return Err(format!(
            "--idempotency-key 는 32자리 hex(16바이트)여야 한다(길이 {})",
            hex.len()
        ));
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("--idempotency-key hex 파싱 실패: {e}"))?;
    }
    Ok(out)
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
