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

/// cgroup 루트에 하위 cgroup 을 **실제로 만들 수 있는가**.
///
/// ★★ 2026-09-22 결함 206 — 위 `parent()` 의 주석("WSL 은 루트를 써야
///   한다")이 **전제로 굳어 있었다.** 루트에 쓰려면 대개 root 권한이
///   필요한데, 비-root 리눅스(GitHub Actions 러너)에서 이 시험 7건이
///   전부 `NotDelegated ... Permission denied` 로 실패했다.
///   제품은 옳게 거부한 것이고, 틀린 것은 시험의 전제다
///   (`CLAUDE.md` §4 — 한 플랫폼 통과를 다른 플랫폼 통과로 세지 않는다.
///   같은 OS 안의 **권한 차이**에도 적용된다).
///
///   그래서 **제품이 아니라 파일시스템에 직접** 물어본다. 제품 함수로
///   판정하면 제품이 망가져도 "환경 없음" 으로 조용히 건너뛰게 된다
///   — 그건 회귀를 숨긴다(결함 205 와 같은 이유).
fn cgroup_delegation_state() -> Result<(), String> {
    let probe = std::path::Path::new("/sys/fs/cgroup").join(format!(
        "gputeer-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    match std::fs::create_dir(&probe) {
        Ok(()) => {
            // 빈 cgroup 디렉터리는 rmdir 로만 지워진다(remove_dir 가 그것이다).
            let _ = std::fs::remove_dir(&probe);
            Ok(())
        }
        Err(e) => Err(format!("{}: {e}", probe.display())),
    }
}

/// 위임이 없으면 `ENVIRONMENT-BLOCKED` 를 적고, **제품이 조용히 통과하지
/// 않는지**만 확인한 뒤 `false` 를 돌려준다(호출한 시험은 거기서 끝낸다).
///
/// ★ 빈손으로 건너뛰지 않는다 — 상한을 못 거는데 자식이 그냥 돌아 버리면
///   "보호된다" 고 믿으면서 보호 없이 도는 것이다(`CLAUDE.md` §0.4).
fn require_cgroup_delegation(what: &str) -> bool {
    let why = match cgroup_delegation_state() {
        Ok(()) => return true,
        Err(why) => why,
    };
    eprintln!("ENVIRONMENT-BLOCKED: cgroup 하위 생성이 거부됐다(비-root 로 보인다) — {what} 은 측정하지 않았다: {why}");
    let refused =
        create_constrained_child(&spec("/bin/true", &[]), LIMIT, "blocked-probe", &parent());
    match refused {
        Ok(_) => panic!("cgroup 을 못 만드는 환경인데 자식이 기동됐다 — 상한 없이 실행한 것이다"),
        Err(error) => eprintln!("  (확인: 제품이 기동을 거부했다 — {error})"),
    }
    false
}

const LIMIT: u64 = 256 * 1024 * 1024;

fn spec(program: &str, args: &[&str]) -> SpawnSpec {
    SpawnSpec {
        environment: Vec::new(),
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
            "{limit:?} 안에 자식이 끝나지 않았다 — 자식이 cgroup 밖에서 돌고 있어 \
                정지 손잡이가 아무도 못 죽이는 것이다"
        ),
    }
}

/// 상한이 실제로 걸리고 자식이 그 cgroup 안에서 도는가.
#[test]
fn a_child_actually_runs_inside_the_cgroup() {
    if !require_cgroup_delegation("a_child_actually_runs_inside_the_cgroup") {
        return;
    }
    let mut child =
        create_constrained_child(&spec("/bin/sleep", &["3"]), LIMIT, "runs-inside", &parent())
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
    assert!(
        !pids.is_empty(),
        "자식이 cgroup 안에 없다 — 상한이 무의미하다"
    );

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
    if !require_cgroup_delegation("exceeding_the_limit_actually_kills_the_child") {
        return;
    }
    // 32MiB 상한에 64MiB 를 잡으려 한다.
    let small = 32 * 1024 * 1024;
    let mut child = create_constrained_child(
        &spec(
            "/bin/sh",
            &[
                "-c",
                "A=$(head -c 67108864 /dev/urandom | base64); echo ${#A}",
            ],
        ),
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
    if !require_cgroup_delegation("a_workload_within_the_limit_is_untouched") {
        return;
    }
    let mut child = create_constrained_child(
        &spec(
            "/bin/sh",
            &[
                "-c",
                "A=$(head -c 1048576 /dev/urandom | base64); echo ${#A}",
            ],
        ),
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
    if !require_cgroup_delegation("a_blocked_wait_can_be_released_from_another_thread") {
        return;
    }
    let child = create_constrained_child(
        &spec("/bin/sleep", &["99999"]),
        LIMIT,
        "stoppable",
        &parent(),
    )
    .expect("기동");
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
    if !require_cgroup_delegation("killing_the_cgroup_kills_grandchildren_too") {
        return;
    }
    let child = create_constrained_child(
        &spec("/bin/sh", &["-c", "sleep 99999 & sleep 99999"]),
        LIMIT,
        "grandchildren",
        &parent(),
    )
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
/// # 왜 exit code 만 보면 안 되는가
///
/// ★ 두 번째 검수 지적. 초안은 마지막 exit code 가 0 인지만 봤는데,
///   그건 다음 경우에도 통과한다.
///
/// ```text
/// echo $$ 가 실패했는데 상한 자체가 안 걸려서 할당이 성공
/// head/base64 가 실패해 할당 없이 마지막 echo 만 0 으로 끝남
/// 할당 크기가 0 이어도 exit 0
/// ```
///
/// 그래서 실제 증거를 남기게 한다 — 탈출 **전후의 cgroup 경로**와
/// **실제 할당 크기**를 파일에 쓰고, 셋을 전부 확인한다.
///
/// 이 테스트는 **탈출이 성공하기를 기대한다.** 나중에 누가 cgroup
/// namespace 나 권한 강등으로 이 구멍을 닫으면 이 테스트가 실패하고,
/// 그때 모듈 문서의 "못 막는다" 를 같이 고치게 된다.
#[test]
fn a_determined_child_can_still_escape_the_cgroup() {
    if !require_cgroup_delegation("a_determined_child_can_still_escape_the_cgroup") {
        return;
    }
    let dir = std::env::temp_dir().join("gputeer-escape-probe");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("증거 디렉터리");
    let before = dir.join("before");
    let after = dir.join("after");
    let size = dir.join("size");

    // 탈출 전 cgroup 을 기록 -> 루트로 이동 -> 탈출 후 cgroup 기록 ->
    // 상한을 훌쩍 넘는 메모리를 잡고 실제 크기를 기록.
    // ★ `set -eu` — 중간 명령이 실패하면 **거기서 끝난다**
    //   (2026-08-30 독립 검수 2라운드 지적). 초안은 `;` 로만 이어서,
    //   탈출 후 `cat` 이 실패해도 스크립트가 계속됐고 빈 `after` 파일이
    //   `!contains("gputeer-escape")` 를 만족해 통과했다 — 탈출이 아니라
    //   **관측 실패**로 통과할 수 있었다.
    let script = format!(
        "set -eu;          cat /proc/self/cgroup > {before};          echo $$ > /sys/fs/cgroup/cgroup.procs;          cat /proc/self/cgroup > {after};          A=$(head -c 67108864 /dev/urandom | base64);          printf '%s' ${{#A}} > {size}",
        before = before.display(),
        after = after.display(),
        size = size.display()
    );
    let child = create_constrained_child(
        &spec("/bin/sh", &["-c", &script]),
        32 * 1024 * 1024,
        "escape",
        &parent(),
    )
    .expect("기동");

    let code = wait_within(child, Duration::from_secs(60));
    assert_eq!(code, 0, "탈출 스크립트가 끝까지 못 갔다");

    // ★ 읽기 실패를 빈 문자열로 접지 않는다 — 그게 통과 사유가 됐었다.
    let read = |p: &std::path::Path| {
        std::fs::read_to_string(p)
            .unwrap_or_else(|error| panic!("증거 파일을 못 읽었다({p:?}): {error}"))
    };
    let (before_txt, after_txt, size_txt) = (read(&before), read(&after), read(&size));

    // 1) 처음에는 우리가 만든 cgroup 안에 있었는가.
    assert!(
        before_txt.contains("gputeer-escape"),
        "자식이 애초에 우리 cgroup 안에 없었다 — 이 테스트의 전제가 깨졌다: {before_txt:?}"
    );
    // 2) 실제로 루트로 나갔는가.
    //
    //    ★ 부분 문자열이 아니라 **정확히 `0::/`** 인지 본다. 부분
    //      문자열 검사는 관측이 비었을 때도 만족한다.
    assert_eq!(
        after_txt.trim(),
        "0::/",
        "탈출 후 cgroup 이 루트가 아니다 — 구멍이 닫혔다면 모듈 문서의          '적대적인 코드는 못 막는다' 를 같이 고쳐야 한다"
    );
    // 3) 상한(32MiB)을 실제로 넘겨 살아남았는가.
    let allocated: u64 = size_txt.trim().parse().unwrap_or(0);
    assert!(
        allocated > 64 * 1024 * 1024,
        "상한을 넘는 할당이 실제로 일어나지 않았다({allocated}바이트) —          exit 0 만으로는 탈출을 증명하지 못한다"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// ★ Agent 가 실제로 쓰는 경로(`Current`·`Explicit`)도 확인한다.
///
/// 2026-08-30 독립 검수 지적 — 기존 테스트는 전부
/// `RootBypassingAncestorLimits` 만 썼다. Agent 는 그것을 절대 안 쓰므로,
/// **Agent 의 실제 경로는 한 번도 실행된 적이 없었다.**
#[test]
fn the_parent_the_agent_actually_uses_is_exercised() {
    if !require_cgroup_delegation("the_parent_the_agent_actually_uses_is_exercised") {
        return;
    }
    // 1) 위임받은 subtree 를 실제로 만든다 — 운영자가 Agent 몫으로
    //    준비해 주는 것과 같은 모양이다.
    let delegated = std::path::Path::new("/sys/fs/cgroup/gputeer-delegated-probe");
    let _ = std::fs::remove_dir(delegated);
    std::fs::create_dir(delegated).expect("위임 subtree 생성");
    std::fs::write(delegated.join("cgroup.subtree_control"), "+memory")
        .expect("위임 subtree 에 memory 켜기");

    let child = create_constrained_child(
        &spec("/bin/sleep", &["1"]),
        LIMIT,
        "explicit-parent",
        &CgroupParent::Explicit(delegated.to_path_buf()),
    )
    .expect("Explicit 부모로 기동");
    assert_eq!(
        child.memory_limit_bytes(),
        Some(LIMIT),
        "Explicit 부모에서 상한이 안 걸렸다"
    );
    let code = wait_within(child, Duration::from_secs(30));
    assert_eq!(code, 0);
    let _ = std::fs::remove_dir(delegated);

    // 2) `Current` 는 이 환경(/init.scope)에서 **거부돼야 한다.**
    //    거부가 곧 fail-closed 다 — 상한 없이 띄우지 않는다.
    let refused = create_constrained_child(
        &spec("/bin/true", &[]),
        LIMIT,
        "current-parent",
        &CgroupParent::Current,
    );
    match refused {
        Err(error) => assert!(
            error.to_string().contains("CGROUP_NO_MEMORY_CONTROLLER"),
            "거부는 됐는데 사유가 위임 부재로 식별되지 않는다: {error}"
        ),
        Ok(_) => {
            // 이 환경의 현재 cgroup 이 위임을 받은 경우다 — 그러면
            // 성공이 정상이다. 어느 쪽이든 "조용히 상한 없이 실행" 은
            // 아니라는 것이 이 테스트의 주장이다.
        }
    }
}
