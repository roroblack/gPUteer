//! 결함 77 (재검수 57) — **실제 자식**의 종료 코드가 `ExecutionOutcome` 까지 값 그대로 오는가.
//!
//! 보고 생성 테스트는 이미 만든 `Some(code)` 를 넣으므로 플랫폼 어댑터에서 코드를 잃어도 못 잡는다. 여기서는 실제
//! `cmd.exe` 를 띄워 0 · 7 · 259(STILL_ACTIVE 와 같은 값) · u32::MAX(`exit -1`) 가 코드로 남는지 본다 —
//! 259 는 고치기 전 "코드 없음" 으로 보고되던 값이다.
//!
//! 실행에 Job Object 가 필요해 Windows 에서만 돈다. 리눅스의 신호 종료 매핑은 여기서 재지 않는다.

#![cfg(windows)]

use std::collections::BTreeMap;

use gputeer_agent::exec::{execute, ExecutionPolicy, ExitObserved, IsolationIdentity};
use gputeer_protocol::execution_spec::ExecutionSpec;

fn run(code_arg: &str) -> ExitObserved {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let spec = ExecutionSpec {
        job_id: "01JEXITCODE00000000000001".to_string(),
        entrypoint: format!("{root}\\System32\\cmd.exe"),
        args: vec!["/d".into(), "/c".into(), "exit".into(), code_arg.into()],
        env_vars: BTreeMap::new(),
    };
    let policy = ExecutionPolicy {
        workload_environment: Vec::new(),
        opted_in: true,
        commit_limit_bytes: 256 * 1024 * 1024,
        // GPU 관문을 켜면 이 기계의 GPU 유무가 먼저 판정을 가른다 — 여기서 재려는 것은 종료 코드다.
        gpu_requirements: None,
        capture_dir: None,
        isolation: IsolationIdentity {
            grant_id: format!("exit-code-{code_arg}"),
            attempt_id: "attempt".to_string(),
        },
        cgroup_parent: None,
        container: gputeer_agent::container::ContainerDecision::Host,
    };
    execute(&spec, policy)
        .unwrap_or_else(|e| panic!("exit {code_arg} 를 실행하지 못했다: {e}"))
        .exit
}

#[test]
fn an_exit_code_of_259_is_kept_as_a_code() {
    assert_eq!(
        run("259"),
        ExitObserved::Code(259),
        "259 로 끝난 작업을 코드 없음으로 보고하면 실제 종료 코드를 버린다(결함 77)"
    );
}

#[test]
fn an_exit_code_of_u32_max_is_kept_as_a_code() {
    assert_eq!(run("-1"), ExitObserved::Code(u32::MAX));
}

#[test]
fn ordinary_exit_codes_are_kept() {
    assert_eq!(run("0"), ExitObserved::Code(0));
    assert_eq!(run("7"), ExitObserved::Code(7));
}
