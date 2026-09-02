//! `gputeer import-manifest` — **거부 경로가 이 조각의 값어치다.**
//!
//! 정상 반입이 되는 것보다, 신뢰 목록에 없는 서명자를 거부하고 그때
//! **DB 가 안 바뀌는지**가 중요하다. "거부했다" 만 보면 거부하면서
//! 흔적을 남기는 구현도 통과한다.

use std::path::{Path, PathBuf};
use std::process::Command;

use gputeer_crypto::{KeyProtection, PersistentKeyring, PlaintextPolicy, SigningKey};

fn cli_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(if cfg!(windows) { "gputeer.exe" } else { "gputeer" })
}

const SUBMITTER: &str = "01JSUBMITTERIMPORT0000001";
const JOB_ID: &str = "01JJOBIMPORT000000000001";
const SEED: &str = "33333333333333333333333333333333333333333333333333333333333333cc";

fn seed_bytes() -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&SEED[i * 2..i * 2 + 2], 16).expect("seed hex");
    }
    out
}

/// 운영자가 provision 하는 신뢰 목록을 만든다.
///
/// ★ `signer` 를 바꿔 주면 **다른 사람의 키를 넣은 keyring** 이 된다 —
///   그게 "모르는 서명자" 반례다.
fn write_keyring(path: &Path, signer_id: &str, key: &SigningKey) {
    let mut keyring = PersistentKeyring::new(
        path,
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .expect("keyring 생성");
    keyring
        .insert_public(signer_id, key.verifying_key())
        .expect("공개키 등록");
    keyring.save().expect("keyring 저장");
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix epoch")
        .as_millis() as u64
}

/// 제출자가 서명한 Manifest 를 만든다. `expires_at` 을 과거로 주면
/// **만료 반례**가 된다.
fn submit_manifest_expiring_at(dir: &Path, name: &str, expires_at_unix_ms: u64) -> PathBuf {
    let out = dir.join(name);
    let issued = now_unix_ms().saturating_sub(60_000);
    let status = Command::new(cli_bin())
        .args([
            "submit",
            "--job-id",
            JOB_ID,
            "--entrypoint",
            "python",
            "--submitter-device-id",
            SUBMITTER,
            "--submitter-seed",
            SEED,
            "--issued-at-unix-ms",
            &issued.to_string(),
            "--expires-at-unix-ms",
            &expires_at_unix_ms.to_string(),
            "--out",
            out.to_str().expect("경로"),
        ])
        .output()
        .expect("submit 실행");
    assert!(
        status.status.success(),
        "submit 이 실패했다: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    out
}

fn submit_manifest(dir: &Path) -> PathBuf {
    // 넉넉히 유효한 것 — 반입 시점에 만료로 걸리면 재려는 것을 못 잰다.
    submit_manifest_expiring_at(dir, "manifest.pb", now_unix_ms() + 7 * 24 * 3_600_000)
}

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn import(manifest: &Path, keyring: &Path, db: &Path, extra: &[&str]) -> Run {
    let mut args: Vec<String> = vec![
        "import-manifest".into(),
        "--manifest".into(),
        manifest.to_str().expect("경로").into(),
        "--submitter-keyring".into(),
        keyring.to_str().expect("경로").into(),
        "--job-db".into(),
        db.to_str().expect("경로").into(),
        "--idempotency-key".into(),
        "0102030405060708090a0b0c0d0e0f10".into(),
    ];
    args.extend(extra.iter().map(|s| (*s).to_string()));
    let out = Command::new(cli_bin())
        .args(&args)
        .output()
        .expect("import-manifest 실행");
    Run {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// 이 Job 이 DB 에 들어 있는가.
fn stored_job(db: &Path) -> Option<gputeer_coordinator::job_store::StoredJob> {
    if !db.exists() {
        return None;
    }
    let store = gputeer_coordinator::job_store::CoordinatorJobStore::open(db)
        .expect("job store 열기");
    store.get(JOB_ID).expect("job 조회")
}

/// **DB 파일 전체**의 상태. 없으면 `None`.
///
/// ★ 독립 검수 1라운드 지적 — 전에는 거부 뒤 "고정된 `JOB_ID` 한 건이
///   없는지" 만 봤다. 그러면 거부 경로가 **스키마·idempotency 행·manifest
///   binding·다른 Job 행**을 남겨도 통과한다. 바이트 전체를 비교한다.
fn db_snapshot(db: &Path) -> Option<Vec<u8>> {
    if !db.exists() {
        return None;
    }
    Some(std::fs::read(db).expect("db 읽기"))
}

/// 거부가 **아무 흔적도 안 남겼는가**.
///
/// 거부 전에 DB 가 없었으면 여전히 없어야 하고, 있었으면 바이트가 같아야
/// 한다. 어느 쪽이든 "이 job_id 만 없다" 보다 훨씬 강하다.
fn assert_db_unchanged(db: &Path, before: &Option<Vec<u8>>, label: &str) {
    let after = db_snapshot(db);
    match (before, &after) {
        (None, None) => {}
        (None, Some(_)) => panic!("{label}: 거부했는데 DB 파일이 새로 생겼다"),
        (Some(_), None) => panic!("{label}: 거부했는데 DB 파일이 사라졌다"),
        (Some(b), Some(a)) => assert!(
            b == a,
            "{label}: 거부했는데 DB 내용이 바뀌었다({} -> {} 바이트)",
            b.len(),
            a.len()
        ),
    }
    assert!(
        stored_job(db).is_none(),
        "{label}: 거부했는데 Job 이 남았다"
    );
}

const ALLOW_PLAINTEXT: [&str; 2] = ["--i-understand-plaintext-keyring-is-unsafe", "true"];

#[test]
fn a_manifest_signed_by_a_trusted_submitter_is_imported() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_manifest(dir.path());
    let keyring = dir.path().join("submitters.keyring");
    write_keyring(&keyring, SUBMITTER, &SigningKey::from_bytes(&seed_bytes()));
    let db = dir.path().join("jobs.sqlite3");

    let run = import(&manifest, &keyring, &db, &ALLOW_PLAINTEXT);
    assert!(run.ok, "정상 반입이 실패했다: {}", run.stderr);
    assert!(
        run.stdout.contains("IMPORTED") && run.stdout.contains(JOB_ID),
        "출력이 무엇을 했는지 말하지 않는다: {}",
        run.stdout
    );
    let job = stored_job(&db).expect("Job 이 저장되지 않았다");
    assert_eq!(job.job_id, JOB_ID, "다른 Job 이 저장됐다");
    assert_eq!(
        job.submitter_device_id, SUBMITTER,
        "제출자가 잘못 기록됐다 — 검증한 서명자와 같아야 한다"
    );
}

/// ★ **이 조각의 핵심 거부**다. keyring 에 없는 서명자는 받지 않는다 —
///   그게 없으면 "아무 키나 붙이면 검증됨" 이 된다.
#[test]
fn a_manifest_from_an_unknown_signer_is_rejected_and_the_db_is_untouched() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_manifest(dir.path());
    let keyring = dir.path().join("submitters.keyring");
    // 신뢰 목록에 **다른 사람**을 넣는다. 서명자 ID 자체가 없다.
    write_keyring(
        &keyring,
        "01JSOMEONEELSE0000000001",
        &SigningKey::from_bytes(&[7u8; 32]),
    );
    let db = dir.path().join("jobs.sqlite3");
    let before = db_snapshot(&db);

    let run = import(&manifest, &keyring, &db, &ALLOW_PLAINTEXT);
    assert!(!run.ok, "모르는 서명자를 받아들였다: {}", run.stdout);
    // ★ 세 거부 사유를 **구분해서** 확인한다(독립 검수 1라운드 지적).
    //   공통 문자열만 보면 셋을 하나로 뭉갠 구현도 전부 통과한다.
    assert!(
        run.stderr.contains("신뢰 목록에서 쓸 수 있는 키를 찾지 못했다"),
        "서명자를 못 찾은 것을 그렇게 부르지 않는다: {}",
        run.stderr
    );
    assert_db_unchanged(&db, &before, "모르는 서명자");
}

/// 서명 바이트를 한 개 뒤집는다 — 서명자 ID 는 신뢰 목록에 있다.
#[test]
fn a_forged_signature_is_rejected_and_the_db_is_untouched() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_manifest(dir.path());
    let keyring = dir.path().join("submitters.keyring");
    write_keyring(&keyring, SUBMITTER, &SigningKey::from_bytes(&seed_bytes()));
    let db = dir.path().join("jobs.sqlite3");

    // 마지막 바이트를 뒤집는다 — 서명은 메시지 끝부분에 있다.
    let mut bytes = std::fs::read(&manifest).expect("manifest 읽기");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    let forged = dir.path().join("forged.pb");
    std::fs::write(&forged, &bytes).expect("forged 쓰기");

    let before = db_snapshot(&db);
    let run = import(&forged, &keyring, &db, &ALLOW_PLAINTEXT);
    assert!(!run.ok, "위조 서명을 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("서명이 맞지 않는다"),
        "위조를 서명 문제로 부르지 않는다: {}",
        run.stderr
    );
    assert_db_unchanged(&db, &before, "위조 서명");
}

/// ★ 평문 keyring 은 **기본으로 거부한다.** 이 파일은 "누구를 믿는가" 의
///   정본이므로, 보호되지 않은 목록을 쓰려면 명시적으로 진술해야 한다.
#[test]
fn a_plaintext_keyring_is_refused_without_the_explicit_opt_in() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_manifest(dir.path());
    let keyring = dir.path().join("submitters.keyring");
    write_keyring(&keyring, SUBMITTER, &SigningKey::from_bytes(&seed_bytes()));
    let db = dir.path().join("jobs.sqlite3");

    let before = db_snapshot(&db);
    let run = import(&manifest, &keyring, &db, &[]);
    assert!(!run.ok, "평문 keyring 을 조용히 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("keyring"),
        "거부 사유가 keyring 을 가리키지 않는다: {}",
        run.stderr
    );
    assert_db_unchanged(&db, &before, "평문 keyring");
}

/// `:memory:` 는 넣은 것이 프로세스와 함께 사라지는데 로그에는 성공으로
/// 찍힌다 — 조용한 무시는 실패보다 나쁘다.
#[test]
fn a_non_durable_job_db_is_refused() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_manifest(dir.path());
    let keyring = dir.path().join("submitters.keyring");
    write_keyring(&keyring, SUBMITTER, &SigningKey::from_bytes(&seed_bytes()));

    // ★ 빈 경로도 비영속이다(독립 검수 1라운드 지적) — SQLite 는 그것도
    //   임시 DB 로 열어 준다. 전에는 `:memory:` 문자열만 봤기 때문에
    //   `--job-db ""` 가 성공으로 끝났다.
    for label in [":memory:", ""] {
        let run = import(&manifest, &keyring, Path::new(label), &ALLOW_PLAINTEXT);
        assert!(!run.ok, "{label:?} 를 받아들였다: {}", run.stdout);
        assert!(
            run.stderr.contains("영속이 아니다"),
            "{label:?}: 거부 사유가 무엇인지 말하지 않는다: {}",
            run.stderr
        );
    }
}

/// 같은 입력을 두 번 반입해도 행은 하나다 — 저장소의 idempotency key 가
/// 다룬다. 재시도는 정상적인 운영이다.
#[test]
fn importing_the_same_manifest_twice_keeps_one_row() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_manifest(dir.path());
    let keyring = dir.path().join("submitters.keyring");
    write_keyring(&keyring, SUBMITTER, &SigningKey::from_bytes(&seed_bytes()));
    let db = dir.path().join("jobs.sqlite3");

    let first = import(&manifest, &keyring, &db, &ALLOW_PLAINTEXT);
    assert!(first.ok, "첫 반입 실패: {}", first.stderr);
    let after_first = stored_job(&db).expect("첫 반입이 저장되지 않았다");

    let second = import(&manifest, &keyring, &db, &ALLOW_PLAINTEXT);
    assert!(second.ok, "재반입이 실패했다 — 재시도는 정상이다: {}", second.stderr);
    let after_second = stored_job(&db).expect("재반입 뒤 Job 이 사라졌다");

    // ★ 개수가 아니라 **행 전체**를 비교한다 — 두 번째가 덮어써서 값이
    //   바뀌면 개수는 그대로여도 멱등이 아니다.
    assert_eq!(after_first, after_second, "재반입이 기존 행을 바꿨다");
    // 두 번째 실행이 "새로 만들었다" 고 말하면 안 된다.
    assert!(
        second.stdout.contains("created=false"),
        "재반입이 새로 만들었다고 보고했다: {}",
        second.stdout
    );
}

/// ★ 독립 검수 2라운드가 짚은 **안 돌고 있던 분기** — 지금까지 거부
///   테스트는 전부 "DB 가 애초에 없음" 상태라, `db_snapshot()` 의 바이트
///   비교가 사실상 `None == None` 만 확인했다. **이미 Job 이 들어 있는
///   DB** 에 나쁜 Manifest 를 들이밀어야 그 비교가 실제로 무언가를 잰다.
///
/// 이게 현실의 모양이기도 하다 — 운영 중인 job store 는 비어 있지 않다.
/// 거부가 기존 행을 건드리면 그건 거부가 아니라 손상이다.
#[test]
fn a_rejection_leaves_an_already_populated_db_byte_for_byte_identical() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let manifest = submit_manifest(dir.path());
    let keyring = dir.path().join("submitters.keyring");
    write_keyring(&keyring, SUBMITTER, &SigningKey::from_bytes(&seed_bytes()));
    let db = dir.path().join("jobs.sqlite3");

    // 먼저 정상 반입해 DB 를 **채운다**.
    let first = import(&manifest, &keyring, &db, &ALLOW_PLAINTEXT);
    assert!(first.ok, "정상 반입이 실패했다: {}", first.stderr);
    let seeded = stored_job(&db).expect("반입한 Job 이 있어야 한다");
    let before = db_snapshot(&db);
    assert!(
        before.as_ref().is_some_and(|b| !b.is_empty()),
        "채운 DB 가 비어 있다 — 이 테스트는 채워진 DB 를 재야 한다"
    );

    // 이제 신뢰 목록에서 이 서명자를 **빼** 같은 Manifest 를 거부시킨다.
    write_keyring(
        &keyring,
        "01JSOMEONEELSE0000000001",
        &SigningKey::from_bytes(&[7u8; 32]),
    );
    let run = import(&manifest, &keyring, &db, &ALLOW_PLAINTEXT);
    assert!(!run.ok, "모르는 서명자를 받아들였다: {}", run.stdout);

    // 파일 전체가 바이트 단위로 같아야 한다.
    let after = db_snapshot(&db);
    assert!(
        before == after,
        "거부가 채워진 DB 를 바꿨다({:?} -> {:?} 바이트)",
        before.as_ref().map(Vec::len),
        after.as_ref().map(Vec::len)
    );
    // 그리고 원래 Job 은 그대로 살아 있어야 한다 — 파일 크기만 같고
    // 내용이 바뀌는 경우를 배제한다.
    let survivor = stored_job(&db).expect("거부가 기존 Job 을 지웠다");
    assert_eq!(
        (survivor.job_id.as_str(), survivor.state),
        (seeded.job_id.as_str(), seeded.state),
        "거부가 기존 Job 을 바꿨다"
    );
}

/// ★ 만료된 Manifest 는 거부한다 — 서명이 맞아도 수명은 별개다.
///
/// 이 대조가 없으면 "서명만 맞으면 언제든 반입" 이 된다.
#[test]
fn an_expired_manifest_is_rejected_and_the_db_is_untouched() {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    // 30초 전에 이미 만료됐다.
    let manifest =
        submit_manifest_expiring_at(dir.path(), "expired.pb", now_unix_ms() - 30_000);
    let keyring = dir.path().join("submitters.keyring");
    write_keyring(&keyring, SUBMITTER, &SigningKey::from_bytes(&seed_bytes()));
    let db = dir.path().join("jobs.sqlite3");

    let before = db_snapshot(&db);
    let run = import(&manifest, &keyring, &db, &ALLOW_PLAINTEXT);
    assert!(!run.ok, "만료된 Manifest 를 받아들였다: {}", run.stdout);
    assert!(
        run.stderr.contains("만료"),
        "만료를 만료라고 부르지 않는다 — 서명 실패로 뭉개면 운영자가          엉뚱한 것을 고치러 간다: {}",
        run.stderr
    );
    assert_db_unchanged(&db, &before, "만료");
}
