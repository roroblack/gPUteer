//! 워크로드를 띄우기 **전에** GPU 요구를 확인하는 관문.
//!
//! # 무엇을 고정하는가 (2026-09-07 신설)
//!
//! `crates/runtime-nvml` 이 판정 함수를 갖고 있었지만 **아무도 부르지
//! 않았다** — `crates/agent/Cargo.toml` 에 의존조차 없었다. 즉 확인은
//! 만들어져 있는데 실행 경로에서는 안 돌고 있었다.
//!
//! 이 파일이 고정하는 것 넷:
//!
//! ```text
//! 1  요구가 없으면 관문이 없다        GPU 안 쓰는 Job 을 막지 않는다
//! 2  요구가 있으면 띄우기 전에 막는다  프로세스가 뜨면 이미 늦다
//! 3  "모자라다" 와 "확인 못 했다" 를 가른다
//! 4  값싼 관문이 먼저다               상한을 못 걸면 NVML 을 안 부른다
//! ```
//!
//! ★★ **3번이 이 관문의 존재 이유다.** 둘을 합치면 NVML 이 잠깐 안 열린
//!   노드가 "GPU 요구를 못 맞추는 노드" 로 낙인찍혀 계속 배제된다.
//!   `runtime-nvml` 이 `is_unknown()` 을 따로 둔 이유가 그것이고, 이
//!   계층에서 뭉개면 그 설계가 무의미해진다.
//!
//! # 이 기계에는 NVIDIA GPU 가 없다 — 그게 오히려 낫다
//!
//! 개발 기계에는 NVIDIA 카드가 없어서 NVML 이 아예 안 열린다. 그래서
//! **"확인 못 했다" 경로를 실물로 잴 수 있다.** 2026-09-07 x600(4070
//! SUPER)·remote5090(5090)·이 기계 셋으로 실측했고, GPU 있는 쪽에서는
//! 없는 UUID 가 `SelectedGpuAbsent` 로 나온다
//! (`docs/evidence/_raw/NVML_preflight_실측.txt`).
//!
//! ★ 그래서 이 테스트는 **GPU 유무에 관계없이** 성립하도록 썼다 —
//!   둘 중 어느 쪽이 나오든 **"관문이 돌았다" 는 사실**을 단언한다.
//!   특정 결과를 기대하면 다른 기계에서 깨진다(`CLAUDE.md` §4).

use gputeer_agent::exec::{execute, ExecutionError, ExecutionPolicy, IsolationIdentity};
use gputeer_protocol::execution_spec::ExecutionSpec;
use gputeer_runtime_nvml::preflight::GpuRequirements;

fn spec() -> ExecutionSpec {
    // ★ 실행 자체는 이 테스트의 관심사가 아니다. 관문에서 막히면 이
    //   entrypoint 는 절대 안 뜬다 — 그게 요점이다.
    ExecutionSpec {
        job_id: "01JGPUGATETEST0000000001".to_string(),
        entrypoint: if cfg!(windows) {
            "cmd.exe".to_string()
        } else {
            "/bin/true".to_string()
        },
        args: if cfg!(windows) {
            vec!["/c".to_string(), "exit".to_string(), "0".to_string()]
        } else {
            Vec::new()
        },
        env_vars: std::collections::BTreeMap::new(),
    }
}

fn policy(gpu: Option<GpuRequirements>) -> ExecutionPolicy {
    ExecutionPolicy {
        workload_environment: Vec::new(),
        // ★ opt-in 은 켠다. 안 켜면 GPU 관문에 **도달하지 못하고**
        //   `NotOptedIn` 으로 먼저 끝난다 — 그러면 이 테스트가 관문을
        //   재는 것이 아니라 opt-in 을 재게 된다.
        opted_in: true,
        commit_limit_bytes: 256 * 1024 * 1024,
        gpu_requirements: gpu,
        capture_dir: None,
        isolation: IsolationIdentity {
            grant_id: "grant-gpu-gate".to_string(),
            attempt_id: "attempt-gpu-gate".to_string(),
        },
        cgroup_parent: None,
    }
}

/// 실재하지 않는 UUID 를 요구한다.
fn absent_gpu() -> GpuRequirements {
    GpuRequirements {
        required_gpu_count: 1,
        minimum_free_vram_bytes_per_gpu: 0,
        selected_gpu_uuids: vec!["GPU-00000000-0000-0000-0000-000000000000".to_string()],
    }
}

#[test]
fn a_workload_that_asks_for_a_gpu_that_is_not_here_does_not_start() {
    let error =
        execute(&spec(), policy(Some(absent_gpu()))).expect_err("없는 GPU 를 요구했는데 실행됐다");

    // ★ 기계에 따라 둘 중 하나다. **둘 다 관문이 돈 증거**다.
    //     GPU 있는 기계   -> GpuRequirementUnmet   (없는 UUID 라 모자람)
    //     GPU 없는 기계   -> GpuUnverifiable       (NVML 을 못 엶)
    match &error {
        ExecutionError::GpuRequirementUnmet { detail } => {
            assert!(
                detail.contains("SelectedGpuAbsent") || detail.contains("Insufficient"),
                "GPU 관문이 아닌 다른 이유로 막혔다: {detail}"
            );
        }
        ExecutionError::GpuUnverifiable { detail } => {
            assert!(
                !detail.is_empty(),
                "확인 실패인데 이유가 비었다 — 운영자가 무엇을 고칠지 모른다"
            );
        }
        other => panic!(
            "GPU 관문이 돌지 않았다. 다른 이유로 막혔다: {other}\n\
             (관문이 아예 없으면 이 자리에서 SpawnFailed 나 Ok 가 나온다)"
        ),
    }
}

#[test]
fn the_two_refusals_are_not_interchangeable() {
    // ★★ **이 저장소가 이 구분에 값을 두는 이유를 문자열로도 고정한다.**
    //   접두사가 같아지면 로그만 보는 운영자가 둘을 구분할 수 없고,
    //   그러면 NVML 이 잠깐 안 열린 노드를 "GPU 없는 노드" 로 잘못 고친다.
    let unmet = ExecutionError::GpuRequirementUnmet { detail: "x".into() }.to_string();
    let unknown = ExecutionError::GpuUnverifiable { detail: "x".into() }.to_string();

    assert!(unmet.contains("GPU_REQUIREMENT_UNMET"), "{unmet}");
    assert!(unknown.contains("GPU_UNVERIFIABLE"), "{unknown}");
    assert_ne!(unmet, unknown, "두 거부가 같은 문장을 낸다");
    assert!(
        unknown.contains("모자란 것이 아니다"),
        "확인 실패 메시지가 '모자람' 과의 차이를 말하지 않는다: {unknown}"
    );
}

#[test]
fn no_gpu_requirement_means_no_gpu_gate() {
    // ★★ **대조군.** 이게 없으면 "항상 거부하는" 관문도 위 테스트를
    //   통과한다. GPU 를 안 쓰는 Job 은 NVML 근처에도 안 가야 한다.
    //
    //   실제 실행 결과는 플랫폼에 따라 다르므로 단언하지 않는다.
    //   **GPU 관련 오류가 안 나오는 것**만 본다.
    match execute(&spec(), policy(None)) {
        Err(ExecutionError::GpuRequirementUnmet { detail })
        | Err(ExecutionError::GpuUnverifiable { detail }) => {
            panic!("GPU 요구가 없는데 GPU 관문이 돌았다: {detail}");
        }
        _ => {}
    }
}

#[test]
fn the_cheap_gate_runs_first() {
    // ★ 상한을 못 걸면 어차피 안 띄운다. 그 경우 NVML 을 부를 이유가
    //   없다 — 없는 GPU 를 요구해도 **상한 오류**가 나와야 한다.
    //
    //   순서가 뒤집히면 상한이 0 인 요청마다 NVML 을 여는 비용이 든다.
    let mut p = policy(Some(absent_gpu()));
    p.commit_limit_bytes = 0;

    match execute(&spec(), p).expect_err("상한 0 인데 실행됐다") {
        ExecutionError::LimitNotApplied { .. } => {}
        other => panic!("상한 검사보다 GPU 검사가 먼저 돌았다: {other}"),
    }
}

#[test]
fn opt_in_is_still_checked_before_everything() {
    // ★ GPU 관문을 넣으면서 opt-in 이 뒤로 밀리지 않았는지 본다.
    //   운영자가 켜지 않았는데 NVML 을 여는 것은 그 자체로 잘못이다.
    let mut p = policy(Some(absent_gpu()));
    p.opted_in = false;

    match execute(&spec(), p).expect_err("opt-in 안 했는데 실행됐다") {
        ExecutionError::NotOptedIn => {}
        other => panic!("opt-in 검사가 더 이상 첫 관문이 아니다: {other}"),
    }
}
