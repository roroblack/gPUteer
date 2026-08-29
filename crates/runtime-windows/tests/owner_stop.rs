//! 소유자 강제 종료 실측 — `CLAUDE.md` §0.1.
//!
//! # 무엇을 증명해야 하는가
//!
//! §0.1 은 "노드 소유자는 **언제든** 자기 GPU 를 즉시 비울 수 있어야
//! 한다" 고 정한다. 그 말이 참이려면 두 가지가 실제로 성립해야 한다.
//!
//! ```text
//! 1. 붙잡힌 스레드 밖에서 멈출 수 있다   wait() 이 블로킹 중이어도
//! 2. 손자까지 죽는다                     자식이 만든 프로세스가 남으면
//!                                        GPU 는 여전히 잡혀 있다
//! ```
//!
//! ★ **2번이 핵심이다.** `TerminateProcess` 만 쓰면 1번은 만족하지만
//!   2번은 아니다 — 그러면 "비웠다" 고 보고하면서 실제로는 안 비운
//!   것이 되고, 그건 `CLAUDE.md` §0.4 가 금지하는 "강제할 수 없는
//!   것을 보장으로 선언" 이다.

#![cfg(windows)]

use std::ffi::{OsStr, OsString};
use std::time::{Duration, Instant};

use gputeer_runtime_windows::{create_constrained_child, quote_command_line, CreateProcessSpec};

/// 256MiB. 이 테스트의 관심사는 상한 값이 아니라 종료다 —
/// `cmd.exe` 가 여유 있게 도는 값이면 된다.
const COMMIT_LIMIT: u64 = 256 * 1024 * 1024;

fn cmd_exe() -> OsString {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    OsString::from(format!("{root}\\System32\\cmd.exe"))
}

fn spawn(args: &[&str]) -> gputeer_runtime_windows::ConstrainedChild {
    let exe = cmd_exe();
    let owned: Vec<OsString> = args.iter().map(OsString::from).collect();
    let refs: Vec<&OsStr> = owned.iter().map(OsString::as_os_str).collect();
    let spec = CreateProcessSpec {
        application_name: exe.clone(),
        command_line: quote_command_line(&exe, &refs),
        current_dir: None,
        stdout_path: None,
        stderr_path: None,
    };
    create_constrained_child(&spec, COMMIT_LIMIT).expect("자식 기동")
}

/// `wait()` 이 블로킹 중인 동안 다른 스레드에서 멈출 수 있는가.
///
/// 오래 도는 자식을 띄우고, 다른 스레드에서 `stopper().terminate()` 를
/// 부른 뒤 `wait()` 이 실제로 풀리는지 본다.
#[test]
fn a_blocked_wait_can_be_released_from_another_thread() {
    // ★ `timeout /t` 를 쓰면 안 된다. 그 명령은 stdin 이 콘솔이
    //   아니면 "Input redirection is not supported" 로 **즉시 실패**해
    //   자식이 스스로 끝난다 — 그러면 "죽였다" 가 아니라 "저절로
    //   끝난 걸 죽였다고 착각한" 공허한 검사가 된다. 실제로 처음
    //   이 테스트를 그렇게 썼고, 아래 종료 코드 단정이 그걸 잡았다.
    //   `ping` 은 stdin 을 안 쓰고 오래 돌아 안전하다.
    let child = spawn(&["/c", "ping", "-n", "99999", "127.0.0.1"]);
    let stopper = child.stopper().expect("stopper 생성");

    let started = Instant::now();
    let killer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        stopper.terminate(1).expect("terminate");
    });

    child.wait().expect("wait");
    let elapsed = started.elapsed();
    killer.join().expect("killer thread");

    // ★ 상한을 넉넉히 잡되 무한대는 아니다. 여기서 걸리면 "멈출 수
    //   있다" 는 주장 자체가 거짓이다.
    assert!(
        elapsed < Duration::from_secs(30),
        "다른 스레드에서 종료시켰는데 wait 이 {elapsed:?} 동안 안 풀렸다"
    );
    // 실제로 강제 종료된 코드가 보여야 한다 — 자연 종료(0)면
    // 테스트가 우연히 통과한 것이다.
    let code = child.exit_code().expect("exit code");
    assert_eq!(
        code, 1,
        "강제 종료 코드가 아니다 — 자식이 스스로 끝났다면 이 검사는 공허하다"
    );
}

/// 손자 프로세스까지 죽는가.
///
/// `cmd /c start /b cmd /c timeout ...` 로 손자를 만든다. 부모
/// `cmd` 는 곧 끝나고 손자만 남으므로, Job 종료가 트리 전체에
/// 닿지 않으면 손자가 살아남는다.
///
/// ★ 손자가 **실제로 생겼는지** 먼저 확인한다. 안 생겼는데 "죽었다"
///   고 보면 이 테스트는 아무것도 증명하지 않는다.
#[test]
fn terminating_the_job_kills_grandchildren_too() {
    // ★ `start` 에 제목을 주지 않는다. 처음엔 마커 문자열을 제목으로
    //   넘겼는데, `start` 는 따옴표 없는 첫 토큰을 **제목이 아니라
    //   실행 파일**로 해석해서 "gputeer_stop_probe_NNN 을(를) 찾을 수
    //   없습니다" 로 실패했다 — 손자가 아예 안 떴다.
    //   `/b` 를 첫 인자로 두면 그 다음 토큰이 실행 파일이 된다.
    //
    //   부모 `cmd` 는 `start /b` 직후 끝나므로, 남는 것은 손자 `ping`
    //   하나다. 그게 이 테스트가 보려는 대상이다.
    let child = spawn(&["/c", "start", "/b", "ping", "-n", "99999", "127.0.0.1"]);
    let stopper = child.stopper().expect("stopper 생성");

    // 손자가 뜰 시간을 준다. 뜬 것을 확인하지 못하면 전제가 깨진 것이다.
    let mut alive_before = 0;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        alive_before = count_ping_processes();
        if alive_before >= 1 {
            break;
        }
    }
    assert!(
        alive_before >= 1,
        "손자(ping)가 뜨지 않았다 — 이 테스트의 전제가 깨졌다"
    );

    // ★ 부모 `cmd` 는 `start /b` 직후 끝난다. 즉 지금 살아 있는 건
    //   손자뿐이다 — 그것을 죽이는지가 이 테스트의 전부다. 종료
    //   직전에 한 번 더 확인해 "이미 죽은 걸 죽였다" 를 막는다.
    assert!(
        count_ping_processes() >= 1,
        "종료 직전에 손자가 이미 사라졌다 — 검사가 공허해진다"
    );

    stopper.terminate(1).expect("terminate");

    // 종료가 반영될 시간을 준다.
    let mut after = alive_before;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        after = count_ping_processes();
        if after == 0 {
            break;
        }
    }
    assert_eq!(
        after, 0,
        "Job 을 종료했는데 손자 ping 이 {after}개 살아있다 — \
         손자가 살아남으면 GPU 는 여전히 잡혀 있다"
    );
}

/// 이미 끝난 Job 에 다시 불러도 실패하지 않는가(멱등).
///
/// 소유자가 정지 버튼을 두 번 누르는 것은 정상적인 일이다.
#[test]
fn terminating_twice_is_not_an_error() {
    let child = spawn(&["/c", "exit", "0"]);
    let stopper = child.stopper().expect("stopper 생성");
    child.wait().expect("wait");

    stopper
        .terminate(1)
        .expect("이미 끝난 Job 에 대한 첫 종료가 실패했다");
    stopper
        .terminate(1)
        .expect("두 번째 종료가 실패했다 — 소유자가 버튼을 두 번 누르면 오류가 난다");
}

/// `ConstrainedChild` 가 먼저 사라져도 손잡이가 살아 있는가.
///
/// ★ 핸들을 복제하지 않고 그대로 넘겼다면 여기서 이미 닫힌 핸들을
///   쓰게 된다. 그 값은 나중에 **다른 객체에 재사용될 수 있으므로**,
///   운 나쁘면 남의 Job 을 죽인다.
#[test]
fn stopper_outlives_the_child_handle() {
    let stopper = {
        let child = spawn(&["/c", "exit", "0"]);
        let stopper = child.stopper().expect("stopper 생성");
        child.wait().expect("wait");
        stopper
        // 여기서 child 가 drop 되며 job 핸들을 닫는다
    };

    stopper
        .terminate(1)
        .expect("child drop 뒤 손잡이가 죽었다 — 핸들 수명이 분리되지 않았다");
}

/// 이 테스트가 띄운 `PING.EXE` 프로세스 수.
///
/// ★ `cmd.exe` 를 세지 않는다. 이 기계에서 다른 이유로 `cmd` 가
///   떠 있으면 절대 수가 흔들려 판정이 흐려진다. `ping -n 99999` 는
///   평범한 상황에 떠 있지 않으므로 신호가 훨씬 깨끗하다.
///
///   완벽히 격리되지는 않는다 — 누군가 동시에 `ping` 을 돌리면
///   0 이 안 될 수 있다. 그 경우 이 테스트는 **실패**하지 통과하지
///   않으므로, 거짓 통과가 아니라 거짓 실패 쪽으로 기운다.
fn count_ping_processes() -> usize {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq PING.EXE", "/NH"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.to_uppercase().contains("PING.EXE"))
        .count()
}
