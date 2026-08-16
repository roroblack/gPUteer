//! P0-03 카오스 테스트 — 쓰기 도중 프로세스 강제 종료.
//!
//! `RULE.md` §6 — 정상 경로 테스트만으로는 완료가 아니다.
//!
//! 이 테스트가 검증하는 불변식 (기준선 §18.2)
//!   1. 매니페스트가 존재하면 그 파일들이 전부 온전하다 (해시 일치)
//!   2. 쓰기 중 kill 은 PARTIAL 만 남긴다. COMMITTED 로 승격되지 않는다
//!   3. 포인터는 항상 유효한 체크포인트를 가리킨다
//!   4. 재개는 마지막 유효 체크포인트에서 이뤄지고, 그 지점이 단조 증가한다
//!
//! **kill 시점을 난수가 아니라 여러 고정 지점으로 스윕한다.**
//! 재현 불가능한 테스트는 실패했을 때 디버깅할 수 없다.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use gputeer_checkpoint::writer::{
    find_resume_point, read_pointer, startup_gc, POINTER_FILENAME,
};
use gputeer_checkpoint::CheckpointManifest;
use gputeer_checkpoint::durability::MANIFEST_FILENAME;

fn writer_bin() -> PathBuf {
    // cargo 가 통합 테스트 실행 시 deps 옆에 바이너리를 둔다
    let mut p = std::env::current_exe().expect("test exe");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    let exe = if cfg!(windows) { "ckpt_writer.exe" } else { "ckpt_writer" };
    p.join(exe)
}

/// 체크포인트 트리 전체를 검사해 불변식 1 을 확인한다.
///
/// 반환값: (유효 체크포인트 수, 매니페스트는 있으나 파일이 깨진 수)
fn audit(root: &Path) -> (usize, usize, Vec<String>) {
    let mut valid = 0;
    let mut corrupt = 0;
    let mut broken = Vec::new();

    if !root.is_dir() {
        return (0, 0, broken);
    }
    for e in std::fs::read_dir(root).unwrap() {
        let dir = e.unwrap().path();
        if !dir.is_dir() {
            continue;
        }
        let mpath = dir.join(MANIFEST_FILENAME);
        if !mpath.exists() {
            continue; // PARTIAL
        }
        let data = std::fs::read(&mpath).unwrap();
        match CheckpointManifest::from_json(&data) {
            Ok(m) => {
                if m.verify_files(&dir).is_ok() {
                    valid += 1;
                } else {
                    corrupt += 1;
                    broken.push(dir.display().to_string());
                }
            }
            Err(_) => {
                corrupt += 1;
                broken.push(format!("{} (manifest parse)", dir.display()));
            }
        }
    }
    (valid, corrupt, broken)
}

fn run_and_kill(root: &Path, kill_after: Duration) -> (usize, String) {
    let mut child = Command::new(writer_bin())
        .arg(root)
        .arg("50") // 체크포인트 50개
        .arg("4") // 파일 4개
        .arg("65536") // 64KB
        .arg("3") // 파일당 3ms 지연
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("ckpt_writer 실행 실패 — cargo build 를 먼저 해야 한다");

    std::thread::sleep(kill_after);
    let _ = child.kill();
    let out = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let committed = stdout.lines().filter(|l| l.starts_with("COMMITTED")).count();
    (committed, stdout)
}

// ══════════════════════════════════════════════════════════════════
// 불변식 1·2 — kill 후에도 손상된 체크포인트가 없다
// ══════════════════════════════════════════════════════════════════

#[test]
fn kill_during_write_never_produces_corrupt_checkpoint() {
    // kill 시점을 고정 스윕한다 (난수 금지 — 재현 가능해야 한다)
    let kill_points_ms = [40u64, 90, 150, 230, 310, 420, 560, 700];

    for &ms in &kill_points_ms {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let (committed, _out) = run_and_kill(root, Duration::from_millis(ms));
        let (valid, corrupt, broken) = audit(root);

        assert_eq!(
            corrupt, 0,
            "kill@{ms}ms — 손상된 체크포인트 {corrupt}건: {broken:?}\n\
             매니페스트가 있는데 파일이 깨졌다면 §18.2 의 '매니페스트 마지막' 규칙이 깨진 것이다"
        );

        // 확정 보고된 개수와 실제 유효 개수가 일치해야 한다.
        // (마지막 하나는 kill 타이밍상 아직 stdout 에 안 나왔을 수 있으므로 valid >= committed)
        assert!(
            valid >= committed,
            "kill@{ms}ms — 확정 보고 {committed}건 > 실제 유효 {valid}건. \
             보고했는데 없는 체크포인트가 있다"
        );
    }
}

// ══════════════════════════════════════════════════════════════════
// 불변식 3 — 포인터는 항상 유효한 체크포인트를 가리킨다
// ══════════════════════════════════════════════════════════════════

#[test]
fn pointer_always_references_a_valid_checkpoint() {
    for &ms in &[60u64, 140, 260, 380, 520] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        run_and_kill(root, Duration::from_millis(ms));

        if let Some(id) = read_pointer(root) {
            let dir = root.join(&id);
            assert!(
                dir.join(MANIFEST_FILENAME).exists(),
                "kill@{ms}ms — 포인터가 {id} 를 가리키는데 매니페스트가 없다"
            );
            let data = std::fs::read(dir.join(MANIFEST_FILENAME)).unwrap();
            let m = CheckpointManifest::from_json(&data)
                .unwrap_or_else(|e| panic!("kill@{ms}ms — 포인터 대상 매니페스트 파싱 실패: {e}"));
            m.verify_files(&dir).unwrap_or_else(|e| {
                panic!("kill@{ms}ms — 포인터가 손상된 체크포인트를 가리킨다: {e}")
            });
        }
        // 포인터가 없는 것은 정상이다 (첫 확정 전에 죽은 경우)
    }
}

// ══════════════════════════════════════════════════════════════════
// 불변식 4 — 재개 지점이 존재하고 단조 증가한다
// ══════════════════════════════════════════════════════════════════

#[test]
fn resume_point_is_valid_and_monotonic_across_restarts() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let mut last_step = 0u64;
    // 같은 디렉터리에서 kill -> 재시작을 반복한다
    for (i, &ms) in [80u64, 160, 240, 320, 400].iter().enumerate() {
        run_and_kill(root, Duration::from_millis(ms));

        let (_dirs, _removed) = startup_gc(root).unwrap();
        let (_valid, corrupt, broken) = audit(root);
        assert_eq!(corrupt, 0, "라운드{i} — GC 후에도 손상 {broken:?}");

        if let Some(m) = find_resume_point(root).unwrap() {
            assert!(
                m.step >= last_step,
                "라운드{i} — 재개 지점이 뒤로 갔다: {} -> {}",
                last_step,
                m.step
            );
            last_step = m.step;
        }
    }
    assert!(last_step > 0, "5회 반복했는데 유효한 재개 지점이 하나도 없다");
}

// ══════════════════════════════════════════════════════════════════
// GC — PARTIAL 은 지우고 COMMITTED 는 남긴다
// ══════════════════════════════════════════════════════════════════

#[test]
fn startup_gc_removes_partial_but_keeps_committed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    run_and_kill(root, Duration::from_millis(300));

    let (valid_before, corrupt_before, _) = audit(root);
    assert_eq!(corrupt_before, 0);

    let (_dirs, removed) = startup_gc(root).unwrap();
    let (valid_after, corrupt_after, _) = audit(root);

    assert_eq!(
        valid_before, valid_after,
        "GC 가 유효한 체크포인트를 지웠다 ({valid_before} -> {valid_after})"
    );
    assert_eq!(corrupt_after, 0);
    // removed 는 0 일 수도 있다 (마침 파일 경계에서 죽으면 tmp 가 안 남는다)
    println!("GC removed {removed} partial files, kept {valid_after} valid checkpoints");
}

// ══════════════════════════════════════════════════════════════════
// negative — 확정된 체크포인트를 변조하면 탐지된다
// ══════════════════════════════════════════════════════════════════

#[test]
fn negative_tampered_committed_checkpoint_is_rejected_from_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    run_and_kill(root, Duration::from_millis(500));

    let before = find_resume_point(root).unwrap().expect("재개 지점이 있어야 한다");
    let dir = root.join(&before.checkpoint_id);

    // 최신 체크포인트의 파일 1바이트를 변조
    let victim = dir.join(&before.files[0].path);
    let mut data = std::fs::read(&victim).unwrap();
    data[0] ^= 0xFF;
    std::fs::write(&victim, &data).unwrap();

    let after = find_resume_point(root).unwrap();
    match after {
        None => {} // 유일한 체크포인트였다면 재개 불가가 맞다
        Some(m) => assert!(
            m.step < before.step,
            "변조된 체크포인트({})가 여전히 재개 지점으로 선택됐다",
            before.checkpoint_id
        ),
    }
}

#[test]
fn negative_missing_data_file_is_rejected_from_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    run_and_kill(root, Duration::from_millis(500));

    let before = find_resume_point(root).unwrap().expect("재개 지점이 있어야 한다");
    let dir = root.join(&before.checkpoint_id);
    std::fs::remove_file(dir.join(&before.files[0].path)).unwrap();

    let after = find_resume_point(root).unwrap();
    if let Some(m) = after {
        assert!(
            m.step < before.step,
            "데이터 파일이 사라진 체크포인트가 여전히 선택됐다"
        );
    }
}

#[test]
fn negative_pointer_to_nonexistent_checkpoint_does_not_break_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    run_and_kill(root, Duration::from_millis(500));

    // 포인터를 존재하지 않는 id 로 덮어쓴다
    std::fs::write(root.join(POINTER_FILENAME), b"ckpt-99999999").unwrap();

    // find_resume_point 는 포인터를 신뢰하지 않고 디렉터리를 검사하므로
    // 여전히 유효한 지점을 찾아야 한다
    let m = find_resume_point(root).unwrap();
    assert!(
        m.is_some(),
        "포인터가 깨졌다고 재개가 불가능해지면 안 된다 — 포인터는 힌트일 뿐이다"
    );
}
