//! ★ 2026-09-30 (소유자 "일시정지") — `CgroupStopper::freeze` · `thaw` 를 실제 리눅스 · 실제 프로세스로 잰다. 리눅스에서만 돈다.
//!
//! ★ 제품 경로는 cgroup 루트 바로 아래에 하위 cgroup 을 만든다 — 대개 root 가 필요하다. 비-root(CI · 권한 없는 서버)에서는
//!   `ENVIRONMENT-BLOCKED` 를 적고 **제품이 조용히 통과하지 않는지**만 본다(`cgroup_enforcement.rs` 와 같은 규칙 · 결함 206).
//!   제품이 기대는 커널 동작 자체(얼리기 확인 · 얼린 채 cgroup.kill)는 비-root 위임 cgroup 에서 따로 쟀다 —
//!   `docs/evidence/_raw/cgroup_freezer_비root_리눅스_실측_2026-09-30.txt`.

#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use gputeer_runtime_linux::{create_constrained_child, CgroupParent, SpawnSpec};

const LIMIT: u64 = 256 * 1024 * 1024;

fn parent() -> CgroupParent {
    CgroupParent::RootBypassingAncestorLimits
}

/// 제품이 아니라 파일시스템에 직접 묻는다 — 제품으로 판정하면 제품이 망가져도 "환경 없음" 으로 건너뛴다(결함 205 · 206).
fn require_cgroup_delegation(what: &str) -> bool {
    let probe = std::path::Path::new("/sys/fs/cgroup").join(format!(
        "gputeer-freeze-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let why = match std::fs::create_dir(&probe) {
        Ok(()) => {
            if let Err(error) = std::fs::remove_dir(&probe) {
                eprintln!("  (시험용 빈 cgroup {probe:?} 를 지우지 못했다: {error})");
            }
            return true;
        }
        Err(error) => format!("{}: {error}", probe.display()),
    };
    eprintln!("ENVIRONMENT-BLOCKED: cgroup 하위 생성이 거부됐다(비-root 로 보인다) — {what} 은 측정하지 않았다: {why}");
    match create_constrained_child(&spec(&["-c", "true"]), LIMIT, "freeze-blocked", &parent()) {
        Ok(_) => panic!(
            "cgroup 을 못 만드는 환경인데 자식이 기동됐다 — 얼릴 수도 멈출 수도 없는 작업이다"
        ),
        Err(error) => eprintln!("  (확인: 제품이 기동을 거부했다 — {error})"),
    }
    false
}

fn spec(args: &[&str]) -> SpawnSpec {
    SpawnSpec {
        environment: Vec::new(),
        program: "/bin/sh".into(),
        args: args.iter().map(|a| (*a).into()).collect(),
        current_dir: None,
        stdout_path: None,
        stderr_path: None,
    }
}

fn counter(path: &std::path::Path) -> u64 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// 얼리면 트리 전체가 멈추고(카운터가 1초 동안 그대로), 풀면 다시 돈다. 얼린 채 `stop()` 해도 끝난다.
#[test]
fn freezing_stops_the_tree_thawing_resumes_it_and_a_frozen_tree_can_be_stopped() {
    if !require_cgroup_delegation(
        "freezing_stops_the_tree_thawing_resumes_it_and_a_frozen_tree_can_be_stopped",
    ) {
        return;
    }
    let file = std::env::temp_dir().join(format!("gputeer-freeze-counter-{}", std::process::id()));
    // 손자(서브셸 루프)가 센다 — 자식 하나가 아니라 트리 전체가 얼어야 한다.
    let script = format!(
        "( i=0; while :; do i=$((i+1)); echo $i > {}; sleep 0.02; done ) & wait",
        file.display()
    );
    let mut child = create_constrained_child(&spec(&["-c", &script]), LIMIT, "freeze", &parent())
        .expect("기동");
    let stopper = child.stopper();
    let deadline = Instant::now() + Duration::from_secs(10);
    while counter(&file) < 5 {
        assert!(
            Instant::now() < deadline,
            "카운터가 돌지 않는다 — 전제가 깨졌다"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    stopper.freeze().expect("얼리기(확인까지)");
    let before = counter(&file);
    std::thread::sleep(Duration::from_secs(1));
    let after = counter(&file);
    assert_eq!(before, after, "얼렸는데 1초 동안 카운터가 움직였다");

    stopper.thaw().expect("풀기(확인까지)");
    std::thread::sleep(Duration::from_millis(500));
    assert!(counter(&file) > after, "풀었는데 다시 돌지 않는다");

    stopper.freeze().expect("다시 얼리기");
    stopper.stop().expect("얼린 채 끝내기");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let status = child.wait_status();
        tx.send(status.map(|s| format!("{s:?}"))).expect("전달");
    });
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(status)) => eprintln!("얼린 채 끝낸 자식의 종료: {status}"),
        Ok(Err(error)) => panic!("종료를 관측하지 못했다: {error}"),
        Err(_) => panic!(
            "얼린 채 끝냈는데 20초 안에 끝나지 않았다 — 얼린 cgroup 에 kill 이 닿지 않은 것이다"
        ),
    }
    if let Err(error) = std::fs::remove_file(&file) {
        eprintln!("  (카운터 파일 {file:?} 를 지우지 못했다: {error})");
    }
}
