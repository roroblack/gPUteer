//! Agent 실행 계층의 소유자 정지 — `CLAUDE.md` §0.1.
//!
//! `crates/runtime-windows/tests/owner_stop.rs` 는 Win32 원시 계층을
//! 실측했다. 여기서 확인하는 것은 그 위 계층이 그것을 **실제로 쓰는가**
//! 다 — 원시 계층이 아무리 정확해도 호출부가 안 쓰면 소유자는 여전히
//! 멈출 수 없다(`runtime-policy` 가 판정만 하고 아무도 안 쓰던 것과
//! 정확히 같은 함정).

#![cfg(windows)]

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gputeer_agent::exec::{
    execute_with_control, ExecutionError, ExecutionPolicy, EXIT_CODE_OWNER_STOPPED,
};
use gputeer_protocol::execution_spec::ExecutionSpec;

const COMMIT_LIMIT: u64 = 256 * 1024 * 1024;

fn cmd_exe() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    format!("{root}\\System32\\cmd.exe")
}

fn spec(args: &[&str]) -> ExecutionSpec {
    ExecutionSpec {
        job_id: "01JOWNERSTOPTEST000000001".to_string(),
        entrypoint: cmd_exe(),
        args: args.iter().map(|a| (*a).to_string()).collect(),
        env_vars: BTreeMap::new(),
    }
}

fn policy(opted_in: bool, limit: u64) -> ExecutionPolicy {
    ExecutionPolicy {
        opted_in,
        commit_limit_bytes: limit,
        capture_dir: None,
    }
}

/// 오래 도는 작업을 소유자가 실제로 멈출 수 있는가.
///
/// ★ 종료 코드가 `EXIT_CODE_OWNER_STOPPED` 여야 한다. 0 이면 자식이
///   스스로 끝난 것이고, 그러면 이 검사는 아무것도 증명하지 않는다.
#[test]
fn the_owner_can_stop_a_running_workload() {
    // `ping` 을 쓴다 — `timeout /t` 는 stdin 이 콘솔이 아니면 즉시
    // 실패해 자식이 스스로 끝나므로 검사가 공허해진다.
    let spec = spec(&["/c", "ping", "-n", "99999", "127.0.0.1"]);
    let (tx, rx) = mpsc::channel();

    let started = Instant::now();
    let worker = std::thread::spawn(move || {
        execute_with_control(&spec, policy(true, COMMIT_LIMIT), move |stopper| {
            tx.send(stopper).expect("손잡이 전달");
        })
    });

    // ★ 손잡이가 **자식이 끝나기 전에** 도착해야 한다. 여기서 시간
    //   초과가 나면 "wait 뒤에 넘긴다" 는 순서 결함이다.
    let stopper = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("자식이 도는 동안 정지 손잡이를 받지 못했다 — 넘기는 순서가 틀렸다");

    stopper.stop().expect("정지 실패");
    let outcome = worker.join().expect("worker thread").expect("실행 결과");

    assert!(
        started.elapsed() < Duration::from_secs(60),
        "정지를 요청했는데 {:?} 동안 안 끝났다",
        started.elapsed()
    );
    assert_eq!(
        outcome.exit_code, EXIT_CODE_OWNER_STOPPED,
        "소유자 정지 종료 코드가 아니다 — 자식이 스스로 끝났다면 이 검사는 공허하다"
    );
}

/// 정지를 두 번 요청해도 오류가 아니어야 한다.
///
/// 소유자가 버튼을 두 번 누르는 것은 정상적인 일이다. 여기서 오류가
/// 나면 화면에 빨간 글씨가 뜨고, 소유자는 안 멈춘 줄 알게 된다.
#[test]
fn stopping_twice_is_not_an_error() {
    let spec = spec(&["/c", "ping", "-n", "99999", "127.0.0.1"]);
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        execute_with_control(&spec, policy(true, COMMIT_LIMIT), move |stopper| {
            tx.send(stopper).expect("손잡이 전달");
        })
    });

    let stopper = rx.recv_timeout(Duration::from_secs(10)).expect("손잡이");
    stopper.stop().expect("첫 정지");
    worker.join().expect("worker thread").expect("실행 결과");
    stopper.stop().expect("이미 끝난 작업에 대한 두 번째 정지가 실패했다");
}

/// 실행하지 못한 경우에는 손잡이를 주지 않는다.
///
/// ★ 안 뜬 작업에 대해 "멈출 수 있다" 는 손잡이를 주면, 소유자 화면에
///   돌고 있지도 않은 작업이 보이게 된다. 그건 §0.1 이 요구하는
///   "누가·어느 Job 을·언제부터 돌리는지 항상 보인다" 를 거짓으로
///   만든다.
#[test]
fn refused_executions_hand_out_no_stopper() {
    for (name, policy, expected) in [
        (
            "opt-in 꺼짐",
            policy(false, COMMIT_LIMIT),
            "NOT_OPTED_IN",
        ),
        ("상한 0", policy(true, 0), "LIMIT_NOT_APPLIED"),
    ] {
        let spec = spec(&["/c", "exit", "0"]);
        let mut handed_out = false;
        let result = execute_with_control(&spec, policy, |_| handed_out = true);

        let error = match result {
            Err(error) => error,
            Ok(outcome) => panic!("{name}: 거부돼야 하는데 실행됐다(exit={})", outcome.exit_code),
        };
        assert!(
            error.to_string().contains(expected),
            "{name}: 기대한 거부 사유가 아니다: {error}"
        );
        assert!(
            !handed_out,
            "{name}: 실행하지 않았는데 정지 손잡이를 넘겼다 — 소유자 화면에 유령 작업이 뜬다"
        );
    }
}

/// 소유자 정지 종료 코드가 정상 완료와 구분되는가.
///
/// 0 이면 중단된 작업이 "성공" 으로 기록된다.
#[test]
fn owner_stop_exit_code_is_not_success() {
    assert_ne!(
        EXIT_CODE_OWNER_STOPPED, 0,
        "소유자 정지 종료 코드가 0 이면 정상 완료와 구분되지 않는다"
    );
    // 흔한 사용자 종료 코드와도 겹치지 않아야 구분이 쉽다.
    assert_ne!(EXIT_CODE_OWNER_STOPPED, 1);
}

/// `ExecutionError::StopFailed` 가 다른 실패와 구분되는 문자열을 갖는가.
///
/// 소유자가 "비워라" 라고 했는데 못 비운 것은 다른 오류들보다 심각하다.
/// 로그에서 한눈에 갈라져야 한다.
#[test]
fn stop_failure_is_distinguishable_in_logs() {
    let error = ExecutionError::StopFailed {
        detail: "테스트".into(),
    };
    assert!(
        error.to_string().starts_with("OWNER_STOP_FAILED"),
        "정지 실패가 다른 오류와 구분되지 않는다: {error}"
    );
}
