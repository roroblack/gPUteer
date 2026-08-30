//! cgroup v2 강제 실측 — Linux 에서만 돈다.
//!
//! # 왜 컴파일만으로는 안 되는가
//!
//! `cargo check --target x86_64-unknown-linux-gnu` 는 "컴파일된다" 만
//! 말한다. `CLAUDE.md` §4 는 한 플랫폼 통과를 다른 플랫폼 통과로 세지
//! 말라고 하고, §0.4 는 강제할 수 없는 것을 보장으로 선언하지 말라고
//! 한다 — 그래서 실제 Linux 에서 실제 프로세스를 상한 안에 넣어 본다.

#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

use gputeer_runtime_linux::{create_constrained_child, CgroupParent, SpawnSpec};

/// ★ 이 테스트 환경(WSL, /init.scope)은 위임된 subtree 가 없어서
///   루트를 써야 한다. 그 선택이 상위 제한을 우회한다는 것은
///   `CgroupParent` 문서에 적혀 있고, 이름이 그것을 계속 상기시킨다.
fn parent() -> CgroupParent {
    CgroupParent::RootBypassingAncestorLimits
}

const LIMIT: u64 = 256 * 1024 * 1024;

fn spec(program: &str, args: &[&str]) -> SpawnSpec {
    SpawnSpec {
        program: program.into(),
        args: args.iter().map(|a| (*a).into()).collect(),
        current_dir: None,
        stdout_path: None,
        stderr_path: None,
    }
}

/// `wait()` 을 시간 상한과 함께 부른다.
///
/// ★ 왜 필요한가 — 2026-08-30 뮤테이션에서 알아냈다. `pre_exec` 의
///   cgroup 투입을 없애면 자식이 cgroup **밖**에서 돌고, 그러면
///   `cgroup.kill` 이 아무도 안 죽여 `wait()` 이 영원히 안 끝난다.
///   그냥 `wait()` 을 부르면 테스트가 실패하는 대신 **매달린다** —
///   CI 에서 매달림은 실패보다 훨씬 나쁘다. 무엇이 틀렸는지 안 알려주고
///   시간만 태운다.
fn wait_within(mut child: gputeer_runtime_linux::ConstrainedChild, limit: Duration) -> i32 {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let code = child.wait();
        // child 를 여기서 drop 해야 cgroup 이 정리된다.
        let _ = tx.send(code);
    });
    match rx.recv_timeout(limit) {
        Ok(Ok(code)) => code,
        Ok(Err(error)) => panic!("wait 실패: {error}"),
        Err(_) => panic!(
            "{limit:?} 안에 자식이 끝나지 않았다 — 자식이 cgroup 밖에서 돌고 있어              정지 손잡이가 아무도 못 죽이는 것이다"
        ),
    }
}

/// 상한이 실제로 걸리고 자식이 그 cgroup 안에서 도는가.
#[test]
fn a_child_actually_runs_inside_the_cgroup() {
    let mut child = create_constrained_child(&spec("/bin/sleep", &["3"]), LIMIT, "runs-inside", &parent())
        .expect("자식 기동");

    // 자식이 실제로 그 cgroup 에 들어갔는지 본다. 안 들어갔다면
    // 상한은 아무것도 제한하지 않는다.
    let mut pids = Vec::new();
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(50));
        pids = child.process_ids();
        if !pids.is_empty() {
            break;
        }
    }
    assert!(!pids.is_empty(), "자식이 cgroup 안에 없다 — 상한이 무의미하다");

    assert_eq!(
        child.memory_limit_bytes(),
        Some(LIMIT),
        "memory.max 가 요청한 값과 다르다"
    );
    // ★ 스왑이 열려 있으면 memory.max 는 상한이 아니라 감속기다.
    //   실측으로 확인한 결함이므로 여기서 고정한다.
    assert_eq!(
        child.swap_limit_bytes(),
        Some(0),
        "memory.swap.max 가 0 이 아니다 — 상한을 넘겨도 스왑으로 살아남는다"
    );

    let code = child.wait().expect("wait");
    assert_eq!(code, 0, "sleep 이 정상 종료하지 않았다");
}

/// 상한을 넘는 할당이 실제로 막히는가.
///
/// ★ 이게 핵심이다. cgroup 을 만들고 숫자만 써 놓아도 위 테스트는
///   통과한다 — 실제로 **제한되는지**는 넘겨 봐야 안다.
#[test]
fn exceeding_the_limit_actually_kills_the_child() {
    // 32MiB 상한에 64MiB 를 잡으려 한다.
    let small = 32 * 1024 * 1024;
    let mut child = create_constrained_child(
        &spec("/bin/sh", &["-c", "A=$(head -c 67108864 /dev/urandom | base64); echo ${#A}"]),
        small,
        "exceeds",
        &parent(),
    )
    .expect("자식 기동");

    let code = child.wait().expect("wait");
    assert_ne!(
        code, 0,
        "상한을 넘겼는데 자식이 정상 종료했다 — 상한이 강제되지 않는다"
    );
}

/// 상한 안에서 도는 작업은 방해받지 않는가.
///
/// ★ 위 테스트만 있으면 "전부 죽인다" 로도 통과한다. 정상 작업이
///   살아남는 것을 같이 봐야 검사가 공허하지 않다.
#[test]
fn a_workload_within_the_limit_is_untouched() {
    let mut child = create_constrained_child(
        &spec("/bin/sh", &["-c", "A=$(head -c 1048576 /dev/urandom | base64); echo ${#A}"]),
        LIMIT,
        "within",
        &parent(),
    )
    .expect("자식 기동");
    let code = child.wait().expect("wait");
    assert_eq!(code, 0, "상한 안의 작업이 죽었다 — 상한이 너무 세게 걸렸다");
}

/// 붙잡힌 wait 을 다른 스레드에서 풀 수 있는가.
#[test]
fn a_blocked_wait_can_be_released_from_another_thread() {
    let child =
        create_constrained_child(&spec("/bin/sleep", &["99999"]), LIMIT, "stoppable", &parent()).expect("기동");
    let stopper = child.stopper();

    let started = Instant::now();
    let killer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        stopper.stop().expect("stop");
    });

    let code = wait_within(child, Duration::from_secs(20));
    killer.join().expect("killer");

    assert!(
        started.elapsed() < Duration::from_secs(20),
        "정지시켰는데 {:?} 동안 안 풀렸다",
        started.elapsed()
    );
    // 신호로 죽었으므로 종료 코드가 0 이 아니어야 한다 — 0 이면
    // 자연 종료를 죽였다고 착각한 것이다.
    assert_ne!(code, 0, "강제 종료인데 정상 종료 코드가 나왔다");
}

/// 손자까지 죽는가.
///
/// cgroup.kill 은 트리 전체를 끝내야 한다 — 하나만 죽이면 손자가
/// GPU 를 쥔 채 남는다.
#[test]
fn killing_the_cgroup_kills_grandchildren_too() {
    let child = create_constrained_child(
        &spec("/bin/sh", &["-c", "sleep 99999 & sleep 99999"]),
        LIMIT, "grandchildren", &parent())
    .expect("기동");

    let mut before = Vec::new();
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(50));
        before = child.process_ids();
        if before.len() >= 2 {
            break;
        }
    }
    assert!(
        before.len() >= 2,
        "손자가 안 생겼다 — 이 테스트의 전제가 깨졌다(pids={before:?})"
    );

    child.stopper().stop().expect("stop");

    // ★ `wait()` 은 여기서 부르지 않는다. 자식이 cgroup 밖에 있으면
    //   영원히 안 끝나기 때문이다 — 대신 cgroup 이 실제로 비는지를
    //   시간 상한 안에서 본다. 그게 이 테스트가 보려는 것이기도 하다.
    let mut after = before.clone();
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        after = child.process_ids();
        if after.is_empty() {
            break;
        }
    }
    assert!(
        after.is_empty(),
        "cgroup 을 죽였는데 {after:?} 가 남았다 — 손자가 살아남으면 GPU 는 여전히 잡혀 있다"
    );
}

/// ★ 자식이 cgroup 밖으로 **실제로 나갈 수 있는가.**
///
/// # 왜 못 막는 것을 테스트하는가
///
/// 2026-08-30 독립 검수가 "이건 협조하는 작업에는 상한이지만 적대적인
/// 코드에는 아니다" 를 짚었다. 모듈 문서에 그렇게 적었는데, **적어 두는
/// 것만으로는 그게 사실인지 알 수 없다.**
///
/// `CLAUDE.md` §0.4 는 강제할 수 없는 것을 보장으로 선언하지 말라고
/// 한다. 그러려면 무엇을 강제 못 하는지 정확히 알아야 하고, 아는
/// 방법은 해 보는 것뿐이다.
///
/// 이 테스트는 **탈출이 성공하기를 기대한다.** 나중에 누가 cgroup
/// namespace 나 권한 강등으로 이 구멍을 닫으면 이 테스트가 실패하고,
/// 그때 모듈 문서의 "못 막는다" 를 같이 고치게 된다.
#[test]
fn a_determined_child_can_still_escape_the_cgroup() {
    // 자기 pid 를 루트 cgroup.procs 에 써서 나간 뒤, 상한을 훌쩍 넘는
    // 메모리를 잡는다. 상한이 강제됐다면 죽어야 한다.
    let escape = "echo $$ > /sys/fs/cgroup/cgroup.procs 2>/dev/null; \
                  A=$(head -c 67108864 /dev/urandom | base64); echo escaped=${#A}";
    let child = create_constrained_child(
        &spec("/bin/sh", &["-c", escape]),
        32 * 1024 * 1024,
        "escape",
        &parent(),
    )
    .expect("기동");

    let code = wait_within(child, Duration::from_secs(30));

    // ★ 이 assert 가 실패하면 좋은 소식이다 — 구멍이 닫혔다는 뜻이다.
    //   그때는 이 테스트를 지우는 게 아니라 뒤집고, 모듈 문서의
    //   "못 막는다" 도 같이 고쳐야 한다.
    assert_eq!(
        code, 0,
        "자식이 탈출하지 못했다 — 구멍이 닫혔다면 모듈 문서의 \
         '적대적인 코드는 못 막는다' 를 같이 고쳐야 한다"
    );
}
