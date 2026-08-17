//! 체크포인트 실패 경로 테스트.
//!
//! # ★ 실패 주입 방법을 바꿨다 (2026-08-17)
//!
//! 초안은 **Windows 파일 공유 규칙**에 기댔다 —
//! "Rust 표준 파일 핸들은 FILE_SHARE_DELETE 없이 열리므로
//!  핸들이 살아 있는 동안 LATEST 를 대체하는 rename 이 실패한다."
//!
//! **틀렸다.** Rust 의 `File::open` 은 Windows 에서
//! `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` 로 연다.
//! 핸들을 잡아도 rename 이 그대로 성공했다.
//!
//! 초안이 스스로 넣어 둔 단언이 이것을 잡았다:
//! "포인터 대상 파일을 점유했는데 성공하면 실패 주입이 무효하다."
//! ★ **실패를 주입했다고 믿는 테스트가 아무것도 주입하지 않는 것**이
//!   가장 위험한 종류의 공허함이다.
//!
//! 지금은 `LATEST` 를 **디렉터리로 만든다.**
//! `fs::rename(tmp, LATEST)` 는 파일을 디렉터리 위로 옮길 수 없으므로
//! 반드시 실패한다 — 플랫폼에 의존하지 않는다.

use std::path::Path;
use std::sync::{Arc, Barrier};
use std::thread;

use gputeer_checkpoint::durability::{
    publication_failed_marker_path, state_recorded, DurabilityState,
    MANIFEST_FILENAME,
};
use gputeer_checkpoint::writer::{
    find_resume_point_for, manifest_for, startup_gc, write_checkpoint,
};
use gputeer_checkpoint::CheckpointManifest;

const JOB: &str = "job-write-failure";
const ATTEMPT: &str = "attempt-write-failure";

fn put(
    root: &Path,
    id: &str,
    step: u64,
    files: &[(String, Vec<u8>)],
) -> CheckpointManifest {
    let mut manifest = manifest_for(id, JOB, ATTEMPT, step, 1, files);
    manifest.created_at_unix_ms = step;

    write_checkpoint(root, &manifest, files, 0).expect("체크포인트 기록");
    manifest
}

#[test]
fn pointer_failure_leaves_residue_marked_and_unselectable() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    let old_files = vec![("model.bin".to_string(), b"old".to_vec())];
    let old = put(root, "ckpt-old", 10, &old_files);

    // ★ LATEST 를 **디렉터리로** 바꾼다.
    //   `fs::rename(LATEST.tmp, LATEST)` 는 파일을 디렉터리 위로 옮길 수 없다.
    //   플랫폼 동작에 기대지 않는 확실한 실패 주입이다.
    let pointer_path = root.join("LATEST");
    std::fs::remove_file(&pointer_path).unwrap();
    std::fs::create_dir(&pointer_path).unwrap();
    // 비어 있지 않게 해서 "빈 디렉터리 위로 rename" 같은 여지도 없앤다.
    std::fs::write(pointer_path.join("occupied"), b"x").unwrap();

    let new_files = vec![("model.bin".to_string(), b"new".to_vec())];
    let mut new = manifest_for("ckpt-new", JOB, ATTEMPT, 20, 1, &new_files);
    new.created_at_unix_ms = 20;

    let result = write_checkpoint(root, &new, &new_files, 0);

    assert!(
        result.is_err(),
        "포인터 대상 파일을 점유했는데 성공하면 실패 주입이 무효하다"
    );

    let new_dir = root.join("ckpt-new");

    assert!(
        new_dir.join(MANIFEST_FILENAME).is_file(),
        "포인터 실패 전까지 기록된 artifact는 진단을 위해 남아 있어야 한다"
    );

    assert!(
        publication_failed_marker_path(&new_dir).is_file(),
        "포인터 실패가 불변 마커로 기록되어야 한다"
    );

    assert!(
        state_recorded(&new_dir, DurabilityState::HashVerified).unwrap(),
        "포인터 실패 전 HASH_VERIFIED 상태가 기록되어야 한다"
    );

    assert!(
        !state_recorded(&new_dir, DurabilityState::Committed).unwrap(),
        "포인터 실패한 체크포인트는 COMMITTED로 기록되면 안 된다"
    );

    let resumed = find_resume_point_for(root, JOB, ATTEMPT)
        .unwrap()
        .expect("이전 체크포인트는 재개 후보로 남아야 한다");

    assert_eq!(
        resumed.checkpoint_id, old.checkpoint_id,
        "Err를 반환한 새 체크포인트가 재개 후보가 되면 W-4가 재발한다"
    );
}

#[test]
fn registered_tmp_suffix_is_preserved() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    let files = vec![("weights.tmp".to_string(), b"real-data".to_vec())];
    let manifest = put(root, "ckpt-tmp-name", 10, &files);

    let dir = root.join(&manifest.checkpoint_id);
    assert!(dir.join("weights.tmp").is_file());

    startup_gc(root).unwrap();

    assert!(
        dir.join("weights.tmp").is_file(),
        "매니페스트에 등록된 .tmp 데이터 파일을 GC가 삭제하면 안 된다"
    );
}

#[test]
fn unregistered_tmp_suffix_is_removed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    let files = vec![("model.bin".to_string(), b"real-data".to_vec())];
    let manifest = put(root, "ckpt-unregistered-tmp", 10, &files);

    let dir = root.join(&manifest.checkpoint_id);
    std::fs::write(dir.join("orphan.tmp"), b"partial").unwrap();

    startup_gc(root).unwrap();

    assert!(
        !dir.join("orphan.tmp").exists(),
        "매니페스트에 없는 .tmp는 GC되어야 한다"
    );
}

#[test]
fn concurrent_startup_gc_treats_not_found_as_normal_race() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().to_path_buf();

    for index in 0..64 {
        std::fs::create_dir_all(root.join(format!("orphan-{index}"))).unwrap();
    }

    let barrier = Arc::new(Barrier::new(2));

    let run = |root: std::path::PathBuf, barrier: Arc<Barrier>| {
        barrier.wait();
        startup_gc(&root)
    };

    let left_root = root.clone();
    let left_barrier = barrier.clone();
    let left = thread::spawn(move || run(left_root, left_barrier));

    let right = thread::spawn(move || run(root, barrier));

    let left_result = left.join().unwrap();
    let right_result = right.join().unwrap();

    assert!(
        left_result.is_ok(),
        "첫 번째 GC의 정상 경합이 오류가 되면 안 된다: {left_result:?}"
    );

    assert!(
        right_result.is_ok(),
        "두 번째 GC의 정상 경합이 오류가 되면 안 된다: {right_result:?}"
    );
}

#[test]
fn successful_write_records_file_operation_states() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();

    let files = vec![("model.bin".to_string(), b"weights".to_vec())];
    let manifest = put(root, "ckpt-states", 10, &files);
    let dir = root.join(&manifest.checkpoint_id);

    for state in [
        DurabilityState::Writing,
        DurabilityState::LocalWritten,
        DurabilityState::HashVerified,
        DurabilityState::Committed,
    ] {
        assert!(
            state_recorded(&dir, state).unwrap(),
            "{state:?} 상태가 파일로 기록되지 않았다"
        );
    }
}
