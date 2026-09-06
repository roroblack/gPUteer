//! 확정 절차(`commit.rs`) — 순서와 실패 경로.
//!
//! `RULE.md` §6 — 정상 경로 테스트만으로는 완료가 아니다.
//! `CLAUDE.md` §0.3 — 매니페스트는 **모든 데이터 파일이 확정된 뒤 마지막에**
//! 쓰고, 매니페스트 없는 데이터 파일은 PARTIAL 이며 부팅 시 GC 한다.
//!
//! ★ **이 파일의 단언은 전부 "왜 실패했는지"까지 본다.** `!ok` 만 보는
//!   테스트는 이 저장소에서 다섯 번 사고를 냈다.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use gputeer_checkpoint::commit::{
    logical_name_of, stored_name_for, CommitError, ManifestMeta, StagedCheckpoint,
};
use gputeer_checkpoint::durability::{state_recorded, DurabilityState, MANIFEST_FILENAME};
use gputeer_checkpoint::writer::{find_resume_point, find_resume_point_for, startup_gc};
use gputeer_checkpoint::CheckpointManifest;

const WRITING_MARKER: &str = ".durability.writing";

fn meta() -> ManifestMeta {
    ManifestMeta {
        job_id: "job-staged".to_string(),
        attempt_id: "att-1".to_string(),
        step: 100,
        fence_epoch: 42,
        producer_node_id: "node-staged".to_string(),
        created_at_unix_ms: 0,
    }
}

/// 디렉터리 안 모든 **파일**의 이름 -> 내용.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut map = BTreeMap::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            map.insert(
                entry.file_name().to_string_lossy().to_string(),
                fs::read(entry.path()).unwrap(),
            );
        }
    }
    map
}

fn writer_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test exe");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    let exe = if cfg!(windows) {
        "staged_ckpt_writer.exe"
    } else {
        "staged_ckpt_writer"
    };
    path.join(exe)
}

/// 자식 프로세스를 돌려 `(exit code, stdout, 확정된 저장 이름들)` 을 얻는다.
fn run_writer(
    root: &Path,
    checkpoint_id: &str,
    files: usize,
    bytes: usize,
    abort_after: i64,
) -> (Option<i32>, String, Vec<String>) {
    let output = Command::new(writer_bin())
        .arg(root)
        .arg(checkpoint_id)
        .arg(files.to_string())
        .arg(bytes.to_string())
        .arg(abort_after.to_string())
        .output()
        .expect("staged_ckpt_writer 실행 실패 — cargo build 를 먼저 해야 한다");

    let stdout = String::from_utf8(output.stdout).expect("stdout 이 UTF-8 이 아니다");
    let staged = stdout
        .lines()
        .filter_map(|line| line.strip_prefix("STAGED "))
        .filter_map(|rest| rest.split_whitespace().nth(1))
        .map(|name| name.to_string())
        .collect();

    (output.status.code(), stdout, staged)
}

// ═══════════════════════════════════════════════════════════════════
// 순서 — 매니페스트는 마지막이다
// ═══════════════════════════════════════════════════════════════════

/// ★ **순서를 직접 잰다.** 데이터 파일이 하나씩 확정되는 동안
///   `manifest.json` 은 단 한 순간도 존재하면 안 된다.
///
///   이 단언이 없으면 "매니페스트를 stage() 마다 갱신" 같은 구현도
///   통과한다 — 그건 부분 상태를 완결로 보이게 만드는 정확히 그 결함이다.
#[test]
fn manifest_appears_only_after_every_data_file_is_final() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-order").unwrap();
    let dir = session.dir().to_path_buf();

    let payloads: [(&str, &[u8]); 3] = [
        ("model.bin", b"weights"),
        ("optim.bin", b"adam-state"),
        ("rng.bin", b"seed"),
    ];

    for (index, (logical, data)) in payloads.iter().enumerate() {
        let stored = session.stage(logical, data).unwrap().stored_name.clone();

        assert_eq!(
            fs::read(dir.join(&stored)).unwrap(),
            *data,
            "{logical} 은 stage() 반환 시점에 이미 확정돼 있어야 한다"
        );
        assert!(
            !dir.join(MANIFEST_FILENAME).exists(),
            "데이터 파일 {}/{} 를 쓴 시점에 매니페스트가 이미 있다 — \
             매니페스트가 마지막이 아니다",
            index + 1,
            payloads.len()
        );
        assert!(
            find_resume_point(root.path()).unwrap().is_none(),
            "확정 전인 체크포인트가 재개 후보로 보인다"
        );
    }

    let committed = session.commit(&meta()).unwrap();

    assert!(committed.manifest_newly_written);
    assert_eq!(committed.manifest.files.len(), 3);
    assert!(dir.join(MANIFEST_FILENAME).is_file());
    committed
        .manifest
        .verify_files(&dir)
        .expect("완결 직후 매니페스트의 해시가 맞아야 한다");

    // 매니페스트에 적힌 순서는 stage 순서 그대로다(정렬하지 않는다 —
    // proto/artifact.proto 의 files 주석이 그렇게 정한다).
    let recorded: Vec<&str> = committed
        .manifest
        .files
        .iter()
        .map(|file| logical_name_of(&file.path).expect("저장 이름이 content-addressed 가 아니다"))
        .collect();
    assert_eq!(recorded, vec!["model.bin", "optim.bin", "rng.bin"]);

    assert!(state_recorded(&dir, DurabilityState::HashVerified).unwrap());

    let resumed = find_resume_point_for(root.path(), "job-staged", "att-1")
        .unwrap()
        .expect("완결된 체크포인트는 재개 후보여야 한다");
    assert_eq!(resumed.checkpoint_id, "ckpt-order");
    assert_eq!(resumed.step, 100);
}

#[test]
fn staged_names_are_content_addressed_and_reversible() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-names").unwrap();

    let a = session.stage("model.bin", b"same").unwrap().clone();
    let b = session.stage("optim.bin", b"same").unwrap().clone();

    assert_eq!(
        a.digest, b.digest,
        "같은 내용이면 digest 는 같아야 한다 (테스트 전제)"
    );
    assert_ne!(
        a.stored_name, b.stored_name,
        "논리 이름이 다르면 저장 이름도 달라야 한다 — 겹치면 서로를 덮어쓴다"
    );
    assert_eq!(
        a.stored_name,
        stored_name_for("model.bin", b"same").unwrap(),
        "저장 이름은 (논리 이름, 내용)만으로 예측 가능해야 한다"
    );
    assert_eq!(logical_name_of(&a.stored_name), Some("model.bin"));

    // ADR-026 의 전제: 같은 이름 = 같은 내용. 다른 내용은 다른 이름이 된다.
    assert_ne!(
        a.stored_name,
        stored_name_for("model.bin", b"different").unwrap(),
        "내용이 다르면 이름도 달라야 rename-over-existing 이 필요 없어진다"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 완결된 것은 GC 가 절대 안 건드린다
// ═══════════════════════════════════════════════════════════════════

#[test]
fn committed_checkpoint_is_a_resume_candidate_and_gc_never_touches_it() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-keep").unwrap();
    session.stage("model.bin", b"weights").unwrap();
    session.stage("optim.bin", b"adam").unwrap();
    let committed = session.commit(&meta()).unwrap();

    let before = snapshot(&committed.dir);
    assert!(before.contains_key(MANIFEST_FILENAME));

    let (dirs, removed) = startup_gc(root.path()).unwrap();

    assert_eq!(dirs, 1);
    assert_eq!(
        removed,
        0,
        "완결된 체크포인트에서 GC 가 무언가를 지웠다 — 지운 것: {:?}",
        before
            .keys()
            .filter(|name| !committed.dir.join(name).exists())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        snapshot(&committed.dir),
        before,
        "GC 전후로 파일 목록·내용이 바이트 단위로 같아야 한다"
    );
    assert!(find_resume_point(root.path()).unwrap().is_some());
}

// ═══════════════════════════════════════════════════════════════════
// negative 1 — 데이터 파일을 쓰다 죽으면 매니페스트가 없고 GC 가 지운다
// ═══════════════════════════════════════════════════════════════════

#[test]
fn crash_between_data_files_leaves_no_manifest_and_gc_removes_everything() {
    let root = tempfile::tempdir().unwrap();
    let (code, stdout, staged) = run_writer(root.path(), "ckpt-crash", 4, 4096, 2);

    assert_eq!(
        code,
        Some(70),
        "자식이 의도한 지점에서 죽지 않았다:\n{stdout}"
    );
    assert_eq!(staged.len(), 2, "STAGED 2건을 기대했다:\n{stdout}");
    assert!(
        stdout.lines().any(|line| line == "DIED_AFTER 2"),
        "{stdout}"
    );
    assert!(
        !stdout.lines().any(|line| line.starts_with("COMMITTED")),
        "죽었는데 확정을 보고했다:\n{stdout}"
    );

    let dir = root.path().join("ckpt-crash");

    // 확정된 2개는 온전하다 — Windows 의 문제는 "찢어진 데이터"가 아니라
    // "확정이 거부됨"이라는 ADR-026 §근거의 관측과 일치해야 한다.
    for stored in &staged {
        assert!(dir.join(stored).is_file(), "{stored} 이 사라졌다");
        assert!(
            logical_name_of(stored).is_some(),
            "{stored} 이 content-addressed 이름이 아니다"
        );
    }

    // ★ GC 대상인 **이유**를 직접 단언한다: 매니페스트가 없다.
    assert!(
        !dir.join(MANIFEST_FILENAME).exists(),
        "데이터 파일을 쓰다 죽었는데 매니페스트가 있다 — 순서가 뒤집혔다"
    );
    assert!(
        find_resume_point(root.path()).unwrap().is_none(),
        "매니페스트 없는 체크포인트가 재개 후보로 선택됐다"
    );

    let before = snapshot(&dir);
    assert_eq!(
        before.len(),
        staged.len() + 1,
        "기대한 잔여물은 데이터 {}개 + {WRITING_MARKER} 뿐이다: {:?}",
        staged.len(),
        before.keys().collect::<Vec<_>>()
    );

    let (dirs, removed) = startup_gc(root.path()).unwrap();
    assert_eq!(dirs, 1);
    assert_eq!(
        removed,
        before.len(),
        "PARTIAL 디렉터리의 파일이 전부 지워지지 않았다 — 남은 것: {:?}",
        snapshot(&dir).keys().collect::<Vec<_>>()
    );
    assert!(!dir.exists(), "빈 PARTIAL 디렉터리가 남았다");
}

#[test]
fn crash_after_all_data_files_but_before_manifest_is_still_partial() {
    let root = tempfile::tempdir().unwrap();
    // abort_after == n_files -> 데이터는 전부 확정, 매니페스트 직전에 죽는다.
    let (code, stdout, staged) = run_writer(root.path(), "ckpt-eve", 3, 2048, 3);

    assert_eq!(code, Some(70), "{stdout}");
    assert_eq!(staged.len(), 3, "{stdout}");

    let dir = root.path().join("ckpt-eve");
    assert!(
        !dir.join(MANIFEST_FILENAME).exists(),
        "★ 이 단언이 가장 중요하다 — 데이터가 전부 확정됐어도 매니페스트가 \
         없으면 완결이 아니다"
    );
    assert!(
        find_resume_point(root.path()).unwrap().is_none(),
        "데이터만 전부 있는 상태가 재개 후보가 됐다"
    );

    let before = snapshot(&dir);
    let (_dirs, removed) = startup_gc(root.path()).unwrap();
    assert_eq!(
        removed,
        before.len(),
        "GC 가 PARTIAL 을 전부 지우지 않았다 — 남은 것: {:?}",
        snapshot(&dir).keys().collect::<Vec<_>>()
    );
    assert!(!dir.exists());
}

// ═══════════════════════════════════════════════════════════════════
// negative 2 — 매니페스트를 쓰다 죽으면 완결이 아니고 GC 가 지운다
// ═══════════════════════════════════════════════════════════════════

/// `write_once(manifest)` 한복판에서 죽으면 디스크에 남는 것은
/// `manifest.json.tmp`(잘린 내용) + 자기 락 파일이고 `manifest.json` 은
/// 없다 — `atomic.rs::write_once` 의 tmp -> fsync -> rename 순서가 그렇게
/// 정한다. 그 잔여물을 그대로 재현해 GC 판정을 확인한다.
///
/// ★ 정직하게 적는다: 이 테스트는 `write_once` **안**에서 프로세스를
///   죽이지 않는다. 그 지점에 kill 훅을 넣으려면 프로덕션 공용 코드인
///   `write_once` 를 고쳐야 해서 이 조각의 범위를 넘는다. 대신 그 크래시가
///   남기는 **정확한 잔여물**을 만들고, 그 잔여물이 완결로 오인되지 않음을
///   단언한다.
#[test]
fn crash_while_writing_manifest_leaves_tmp_residue_and_gc_removes_everything() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-manifest-crash").unwrap();
    session.stage("model.bin", b"weights").unwrap();
    session.stage("optim.bin", b"adam").unwrap();
    let dir = session.dir().to_path_buf();
    drop(session); // 프로세스가 죽었다 — commit() 은 불리지 않았다

    // write_once 가 매니페스트를 쓰다 죽은 순간의 잔여물
    let truncated = br#"{"schema_version":1,"checkpoint_id":"ckpt-mani"#;
    fs::write(dir.join(format!("{MANIFEST_FILENAME}.tmp")), truncated).unwrap();
    fs::write(
        dir.join(format!("{MANIFEST_FILENAME}.write_once.lock")),
        b"",
    )
    .unwrap();

    // 실패 이유 단언 — tmp 는 매니페스트가 아니다. 두 근거를 모두 본다.
    assert!(
        !dir.join(MANIFEST_FILENAME).exists(),
        "rename 전에 죽었으므로 manifest.json 은 존재하지 않아야 한다"
    );
    let parse_error = CheckpointManifest::from_json(truncated)
        .expect_err("잘린 tmp 가 유효한 매니페스트로 파싱됐다");
    assert!(
        matches!(
            parse_error,
            gputeer_checkpoint::CheckpointError::Manifest(_)
        ),
        "기대: Manifest 파싱 오류, 실제: {parse_error:?}"
    );
    assert!(
        find_resume_point(root.path()).unwrap().is_none(),
        "manifest.json.tmp 만 있는 체크포인트가 재개 후보가 됐다"
    );

    let before = snapshot(&dir);
    assert_eq!(
        before.len(),
        5,
        "기대한 잔여물: 데이터 2 + writing 마커 + manifest tmp + 락. 실제: {:?}",
        before.keys().collect::<Vec<_>>()
    );

    let (_dirs, removed) = startup_gc(root.path()).unwrap();
    assert_eq!(
        removed,
        before.len(),
        "남은 것: {:?}",
        snapshot(&dir).keys().collect::<Vec<_>>()
    );
    assert!(!dir.exists(), "빈 PARTIAL 디렉터리가 남았다");
}

// ═══════════════════════════════════════════════════════════════════
// negative 3 — 매니페스트 직전 재검증. 순서를 뒤집으면 여기서 깨진다
// ═══════════════════════════════════════════════════════════════════

#[test]
fn commit_refuses_when_a_finalized_data_file_changed_before_the_manifest() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-tamper").unwrap();
    session.stage("model.bin", b"weights").unwrap();
    let victim = session.stage("optim.bin", b"adam").unwrap().clone();

    // 확정된 뒤에 밖에서 변조한다 (write_once 는 자기가 쓴 뒤의 변조를 모른다)
    fs::write(session.dir().join(&victim.stored_name), b"tampered").unwrap();

    let error = session
        .commit(&meta())
        .expect_err("변조된 데이터 파일 위에 매니페스트를 쓰면 안 된다");

    match error {
        CommitError::StagedFileChanged {
            stored,
            expected,
            actual,
        } => {
            assert_eq!(stored, victim.stored_name);
            assert_eq!(expected, victim.digest, "기대 digest 는 stage 시점 값이다");
            assert_eq!(
                actual,
                blake3::hash(b"tampered").to_hex().to_string(),
                "실제 digest 는 지금 디스크에 있는 바이트의 것이어야 한다"
            );
        }
        other => panic!("StagedFileChanged 를 기대했으나 {other:?}"),
    }

    let dir = root.path().join("ckpt-tamper");
    assert!(
        !dir.join(MANIFEST_FILENAME).exists(),
        "★ 재검증 실패인데 매니페스트가 쓰였다 — 재검증이 매니페스트보다 \
         뒤에 있다는 뜻이고, 그러면 부분 상태가 완결로 보인다"
    );
    assert!(find_resume_point(root.path()).unwrap().is_none());

    let (_dirs, removed) = startup_gc(root.path()).unwrap();
    assert!(removed > 0);
    assert!(!dir.exists(), "완결되지 못한 체크포인트가 남았다");
}

#[test]
fn commit_refuses_when_a_finalized_data_file_disappeared_before_the_manifest() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-missing").unwrap();
    let victim = session.stage("model.bin", b"weights").unwrap().clone();
    fs::remove_file(session.dir().join(&victim.stored_name)).unwrap();

    let error = session
        .commit(&meta())
        .expect_err("없는 파일을 확정하면 안 된다");

    match error {
        CommitError::StagedFileUnreadable { stored, kind, .. } => {
            assert_eq!(stored, victim.stored_name);
            assert_eq!(
                kind,
                std::io::ErrorKind::NotFound,
                "사라진 파일은 NotFound 여야 한다 — 다른 이유면 진단이 틀린다"
            );
        }
        other => panic!("StagedFileUnreadable 을 기대했으나 {other:?}"),
    }

    assert!(!root
        .path()
        .join("ckpt-missing")
        .join(MANIFEST_FILENAME)
        .exists());
}

// ═══════════════════════════════════════════════════════════════════
// negative 4 — 같은 이름 재사용 거부
// ═══════════════════════════════════════════════════════════════════

#[test]
fn duplicate_logical_name_is_rejected_even_for_identical_content() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-dup").unwrap();
    let first = session.stage("model.bin", b"weights").unwrap().clone();

    for (label, data) in [("같은 내용", &b"weights"[..]), ("다른 내용", &b"other"[..])] {
        let error = match session.stage("model.bin", data) {
            Ok(file) => panic!("{label}: 같은 논리 이름 재사용이 허용됐다 -> {file:?}"),
            Err(error) => error,
        };

        match error {
            CommitError::DuplicateLogicalName { name, stored } => {
                assert_eq!(name, "model.bin", "{label}");
                assert_eq!(
                    stored, first.stored_name,
                    "{label} — 오류가 먼저 확정된 저장 이름을 알려줘야 한다"
                );
            }
            other => panic!("{label}: DuplicateLogicalName 을 기대했으나 {other:?}"),
        }
    }

    assert_eq!(session.staged().len(), 1, "거부된 호출이 목록에 들어갔다");
    assert_eq!(
        fs::read(session.dir().join(&first.stored_name)).unwrap(),
        b"weights",
        "먼저 확정된 파일이 변조됐다"
    );
    // 거부는 파일도 만들지 않는다 (writing 마커 + 데이터 1개뿐)
    assert_eq!(snapshot(session.dir()).len(), 2);
}

#[test]
fn reusing_a_committed_checkpoint_id_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-reuse").unwrap();
    session.stage("model.bin", b"weights").unwrap();
    let committed = session.commit(&meta()).unwrap();
    let before = snapshot(&committed.dir);

    let error = StagedCheckpoint::begin(root.path(), "ckpt-reuse")
        .expect_err("완결된 체크포인트 id 재사용은 거부해야 한다");

    match error {
        CommitError::AlreadyCommitted { checkpoint_id } => {
            assert_eq!(checkpoint_id, "ckpt-reuse");
        }
        other => panic!("AlreadyCommitted 를 기대했으나 {other:?}"),
    }

    assert_eq!(
        snapshot(&committed.dir),
        before,
        "거부된 begin() 이 완결된 디렉터리를 건드렸다"
    );
}

/// 크래시 뒤 같은 이름·같은 내용으로 다시 확정하는 것은 **재사용이 아니라
/// 재시도**다 — write-once 의 멱등 경로를 그대로 탄다.
#[test]
fn restaging_the_same_content_after_a_crash_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let (code, _stdout, staged) = run_writer(root.path(), "ckpt-retry", 2, 1024, 1);
    assert_eq!(code, Some(70));
    assert_eq!(staged.len(), 1);

    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-retry").unwrap();
    let again = session
        .stage("shard-0.bin", &vec![1u8; 1024])
        .unwrap()
        .clone();
    assert_eq!(again.stored_name, staged[0]);
    assert!(
        !again.newly_written,
        "같은 내용이 이미 확정돼 있으면 다시 쓰지 않는다 (ADR-026 §수정안 ★)"
    );

    session.stage("shard-1.bin", &vec![2u8; 1024]).unwrap();
    let committed = session.commit(&meta()).unwrap();
    committed.manifest.verify_files(&committed.dir).unwrap();
    assert!(find_resume_point(root.path()).unwrap().is_some());
}

// ═══════════════════════════════════════════════════════════════════
// negative 5 — 경로 탈출·예약 이름
// ═══════════════════════════════════════════════════════════════════

#[test]
fn unsafe_logical_names_are_rejected_without_touching_the_directory() {
    let root = tempfile::tempdir().unwrap();
    let mut session = StagedCheckpoint::begin(root.path(), "ckpt-unsafe").unwrap();
    let before = snapshot(session.dir());

    let cases: [(&str, &str); 8] = [
        ("", "빈 이름"),
        ("..", "상위/현재"),
        ("../escape.bin", "경로 구분자"),
        ("sub/model.bin", "경로 구분자"),
        ("sub\\model.bin", "경로 구분자"),
        (".durability.writing", "'.' 로 시작"),
        ("model.bin.", "후행 점/공백"),
        ("model.b3-x", "'.b3-'"),
    ];

    for (name, hint) in cases {
        let error = session
            .stage(name, b"payload")
            .expect_err("안전하지 않은 이름을 받아들였다");

        match error {
            CommitError::UnsafeLogicalName {
                name: reported,
                reason,
            } => {
                assert_eq!(reported, name);
                assert!(
                    reason.contains(hint),
                    "{name:?} 의 거부 이유가 예상과 다르다 — 기대 힌트 {hint:?}, 실제 {reason:?}"
                );
            }
            other => panic!("{name:?}: UnsafeLogicalName 을 기대했으나 {other:?}"),
        }
    }

    assert_eq!(
        snapshot(session.dir()),
        before,
        "거부된 이름이 파일을 만들었다"
    );
    assert!(
        !root.path().join("escape.bin").exists(),
        "체크포인트 디렉터리 밖에 파일이 생겼다"
    );
    assert_eq!(session.staged().len(), 0);
}

#[test]
fn unsafe_checkpoint_ids_are_rejected_without_creating_anything() {
    let root = tempfile::tempdir().unwrap();

    for (id, hint) in [
        ("", "빈 이름"),
        ("..", "상위/현재"),
        ("../escape", "경로 구분자"),
        ("a/b", "경로 구분자"),
        ("a\\b", "경로 구분자"),
        (".hidden", "'.' 로 시작"),
    ] {
        let error = StagedCheckpoint::begin(root.path(), id)
            .expect_err("안전하지 않은 체크포인트 id 를 받아들였다");

        match error {
            CommitError::UnsafeCheckpointId {
                id: reported,
                reason,
            } => {
                assert_eq!(reported, id);
                assert!(
                    reason.contains(hint),
                    "{id:?} 거부 이유 불일치 — 기대 힌트 {hint:?}, 실제 {reason:?}"
                );
            }
            other => panic!("{id:?}: UnsafeCheckpointId 를 기대했으나 {other:?}"),
        }
    }

    assert_eq!(
        fs::read_dir(root.path()).unwrap().count(),
        0,
        "거부된 id 가 디렉터리를 만들었다"
    );
}

#[test]
fn commit_with_nothing_staged_is_rejected_and_gc_removes_the_directory() {
    let root = tempfile::tempdir().unwrap();
    let session = StagedCheckpoint::begin(root.path(), "ckpt-empty").unwrap();
    let dir = session.dir().to_path_buf();

    let error = session
        .commit(&meta())
        .expect_err("빈 매니페스트를 쓰면 안 된다");

    match error {
        CommitError::NothingStaged { checkpoint_id } => {
            assert_eq!(checkpoint_id, "ckpt-empty");
        }
        other => panic!("NothingStaged 를 기대했으나 {other:?}"),
    }

    assert!(!dir.join(MANIFEST_FILENAME).exists());
    assert_eq!(
        snapshot(&dir).keys().collect::<Vec<_>>(),
        vec![WRITING_MARKER],
        "마커만 남아 있어야 한다"
    );

    let (_dirs, removed) = startup_gc(root.path()).unwrap();
    assert_eq!(removed, 1);
    assert!(!dir.exists());
}
