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
#[cfg(feature = "chaos-hooks")]
use gputeer_checkpoint::writer::find_resume_point_for;
use gputeer_checkpoint::CheckpointManifest;
use gputeer_checkpoint::durability::MANIFEST_FILENAME;
#[cfg(feature = "chaos-hooks")]
use gputeer_checkpoint::durability::{publication_failed, state_recorded, DurabilityState};

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

/// 쓰기 도중 kill 한다.
///
/// # ★ 고정 sleep 을 쓰지 않는다 (2026-08-17 정정)
///
/// 전에는 `sleep(kill_after)` 후 kill 했다. **부하에 취약했다** —
/// `cargo test --workspace` 를 연속으로 돌리면 500ms 안에 완료된
/// 체크포인트가 **0개**가 되어, 재개 지점을 요구하는 테스트가 무너졌다.
/// 3회 반복 중 1회 실패했다.
///
/// ★ 부하에 따라 초록/빨강이 바뀌는 테스트는 **아무것도 말하지 않는다.**
///
/// 지금은 자식의 stdout 을 읽어 **`COMMITTED` 를 `min_committed` 개 볼 때까지**
/// 기다린 뒤 kill 한다. "쓰기 도중에 죽인다" 라는 의도는 그대로다 —
/// 남은 46개는 아직 쓰는 중이다.
///
/// `hard_timeout` 은 자식이 영영 멈춰 있을 때를 위한 안전장치다.
fn run_and_kill_after(
    root: &Path,
    min_committed: usize,
    hard_timeout: Duration,
) -> (usize, String) {
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;

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

    let stdout = child.stdout.take().expect("stdout 파이프");
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        let mut collected = String::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            collected.push_str(&line);
            collected.push('\n');
            // 보낸 뒤 끊겨도 계속 모은다 — 세지 않고 버리지 않는다.
            let _ = tx.send(line);
        }
        collected
    });

    let deadline = std::time::Instant::now() + hard_timeout;
    let mut seen = 0usize;
    while seen < min_committed {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match rx.recv_timeout(left) {
            Ok(line) if line.starts_with("COMMITTED") => seen += 1,
            Ok(_) => {}
            Err(_) => break, // 자식이 끝났거나 시간이 다 됐다
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    let collected = reader.join().unwrap_or_default();
    let committed = collected
        .lines()
        .filter(|l| l.starts_with("COMMITTED"))
        .count();
    (committed, collected)
}

/// 옛 시그니처 — 시간만 주면 "그 시간 안에 최소 1개" 로 해석한다.
fn run_and_kill(root: &Path, kill_after: Duration) -> (usize, String) {
    // ★ 최소 1개는 있어야 재개·GC 검사가 의미를 갖는다.
    //   그 1개를 못 보면 hard timeout(원래 값의 20배)까지 기다린다.
    run_and_kill_after(root, 1, kill_after * 20)
}

/// ★ 시간이 아니라 **코드 순서**로 kill 지점을 겨냥한다 (2026-08-17,
/// 독립 검수의 P0-03 재검수가 지적한 공백을 메운다).
///
/// 위 `run_and_kill*` 는 전부 "밖에서 stdout 을 세다가 죽인다" — 그래서
/// `LATEST` 교체 직후 · `COMMITTED` 마커 기록 직전이라는 좁은 구간을
/// 실제로 맞혔는지 알 수 없다. 이 헬퍼는 `ckpt_writer` 프로세스
/// 자신이 `writer.rs::chaos_kill_after_latest` 에서 그 지점에 도달한
/// 순간 스스로 `abort()` 하게 만든다 — 밖에서 타이밍을 맞출 필요가
/// 없다.
///
/// `chaos-hooks` feature 로 빌드된 `ckpt_writer` 만 이 환경 변수에 반응한다.
#[cfg(feature = "chaos-hooks")]
fn run_and_kill_after_latest(root: &Path) -> String {
    let output = Command::new(writer_bin())
        .arg(root)
        .args(["1", "2", "4096", "0"])
        .env("GPUTEER_CHECKPOINT_CHAOS_KILL_AFTER_LATEST", "1")
        .output()
        .expect("ckpt_writer 실행 실패 — --features chaos-hooks 로 빌드했는지 확인하라");

    assert!(
        !output.status.success(),
        "self-kill 훅이 실행되지 않았거나 ckpt_writer가 정상 종료했다 \
         (chaos-hooks feature 없이 빌드됐을 가능성): {:?}",
        output.status
    );

    String::from_utf8(output.stdout).expect("ckpt_writer stdout가 UTF-8이 아니다")
}

// ══════════════════════════════════════════════════════════════════
// ★ HASH_VERIFIED ~ COMMITTED 구간 — 결정적 kill (2026-08-17)
//
// P0-03 evidence 의 독립 재검수가 지적했다: 아래 시간 기반 스윕은
// "LATEST 교체 직후 · COMMITTED 마커 기록 직전" 이라는 정확한 구간을
// 겨냥하지 않는다 — 우연히 맞혔을 수도, 아닐 수도 있다. 이 테스트는
// `cargo test --features chaos-hooks` 로만 켜지고, self-kill 훅이
// 코드 순서로 그 구간을 정확히 겨냥한다(`writer.rs::chaos_kill_after_latest`
// 참조). `--features chaos-hooks` 없이는 이 테스트 자체가 컴파일되지
// 않는다 — 기본 `cargo test --workspace` 에는 포함되지 않는다.
// ══════════════════════════════════════════════════════════════════

#[cfg(feature = "chaos-hooks")]
#[test]
fn kill_after_latest_before_committed_is_resume_candidate() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    let stdout = run_and_kill_after_latest(root);

    let checkpoint_id = "ckpt-00000100";
    let expected_signal = format!("CHAOS_AFTER_LATEST {checkpoint_id}");

    assert!(
        stdout.lines().any(|line| line == expected_signal),
        "LATEST 직후 훅 신호를 관측하지 못했다 — self-kill 이 의도한 지점에서 \
         일어나지 않았을 수 있다:\n{stdout}"
    );

    assert!(
        !stdout.lines().any(|line| line.starts_with("COMMITTED ")),
        "self-kill 전에 COMMITTED stdout 가 출력됐다 — 훅이 record_state_transition \
         이후에 불렸다는 뜻이다:\n{stdout}"
    );

    assert_eq!(
        read_pointer(root).as_deref(),
        Some(checkpoint_id),
        "LATEST 가 이 체크포인트를 가리켜야 한다 — chaos_kill_after_latest 는 \
         replace_with_retry 성공 '이후' 에만 호출된다"
    );

    let dir = root.join(checkpoint_id);
    assert!(
        dir.join(MANIFEST_FILENAME).is_file(),
        "LATEST 대상에 manifest.json 이 없다"
    );

    let manifest_data = std::fs::read(dir.join(MANIFEST_FILENAME)).unwrap();
    let manifest = CheckpointManifest::from_json(&manifest_data).unwrap();
    manifest
        .verify_files(&dir)
        .expect("LATEST 대상의 데이터 파일 또는 해시가 유효하지 않다");

    assert!(
        state_recorded(&dir, DurabilityState::HashVerified).unwrap(),
        "HashVerified 마커가 없다 — chaos_kill_after_latest 호출 위치가 잘못됐을 수 있다"
    );
    assert!(
        !state_recorded(&dir, DurabilityState::Committed).unwrap(),
        "★ 이 단언이 실패하면 가장 위험하다 — self-kill 전에 이미 Committed 가 \
         기록됐다는 뜻이고, 이 테스트가 겨냥하려던 구간을 놓쳤다는 뜻이다"
    );
    assert!(
        !publication_failed(&dir).unwrap(),
        "정상적인 LATEST 교체 뒤 publication-failed 가 기록됐다 — 있으면 안 된다"
    );

    // 핵심 주장 — P0-03 이 그동안 직접 관측한 적 없는 것: COMMITTED
    // 마커가 없어도 이 체크포인트가 실제로 재개된다.
    let resumed = find_resume_point_for(root, "job-chaos", "att-1")
        .unwrap()
        .expect("HashVerified 상태의 온전한 체크포인트가 재개 후보로 선택되지 않았다");
    assert_eq!(resumed.checkpoint_id, checkpoint_id);
    assert_eq!(resumed.step, 100);
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
