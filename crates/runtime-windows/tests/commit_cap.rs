//! `create_constrained_child` 가 실제로 커밋 메모리를 제한하는지 실측한다.
//!
//! `crates/runtime-policy/src/vram.rs` 는 판정만 하고 실제 Win32 호출이
//! 없다는 사실을 스스로 기록해 뒀다 — 이 테스트가 그 "판정이 실제
//! 강제로 이어지는가" 를 증명한다. `RULE.md` §6 — 정상 경로만으로는
//! 부족하다: **negative control**(같은 fixture 를 Job Object 없이
//! 돌린 결과)이 없으면 "제약 때문에 일찍 멈췄다" 를 "원래 이 정도만
//! 할당됐다" 와 구분할 수 없다.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use gputeer_runtime_windows::{create_constrained_child, quote_command_line, CreateProcessSpec};

fn fixture_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("alloc_fixture.exe")
}

struct Result_ {
    allocated_bytes: u64,
    last_error: Option<u32>,
}

fn parse_result(path: &std::path::Path) -> Result_ {
    let text = fs::read_to_string(path).expect("결과 파일을 읽지 못했다");
    let mut allocated_bytes = None;
    let mut last_error = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("allocated_bytes=") {
            allocated_bytes = Some(v.parse::<u64>().expect("allocated_bytes 파싱 실패"));
        } else if let Some(v) = line.strip_prefix("last_error=") {
            last_error = if v == "none" {
                None
            } else {
                Some(v.parse::<u32>().expect("last_error 파싱 실패"))
            };
        }
    }
    Result_ {
        allocated_bytes: allocated_bytes.expect("allocated_bytes 줄이 없다"),
        last_error,
    }
}

const CHUNK_MIB: u64 = 4;
const COMMIT_LIMIT_MIB: u64 = 64;

/// ★ 핵심 실측 — Job Object 커밋 상한이 걸린 자식은 그 상한 근처에서
/// 할당이 실패해야 하고, `PeakJobMemoryUsed` 는 걸어 둔 상한을 넘지
/// 않아야 한다.
#[test]
fn constrained_child_is_capped_near_the_limit() {
    let temp = tempfile::tempdir().unwrap();
    let result_file = temp.path().join("result.txt");

    let exe = fixture_bin();
    let command_line = quote_command_line(
        exe.as_os_str(),
        &[
            std::ffi::OsStr::new(&CHUNK_MIB.to_string()),
            result_file.as_os_str(),
        ],
    );

    let spec = CreateProcessSpec {
        application_name: exe.clone().into_os_string(),
        command_line,
        current_dir: None,
            stdout_path: None,
            stderr_path: None,
    };

    let child = create_constrained_child(&spec, COMMIT_LIMIT_MIB * 1024 * 1024)
        .expect("create_constrained_child 실패 — alloc_fixture 를 먼저 빌드해야 한다");
    child.wait().expect("자식 대기 실패");

    let (peak, limit) = child
        .query_memory_limits()
        .expect("Job 메모리 정보 조회 실패");

    let result = parse_result(&result_file);

    assert!(
        result.last_error.is_some(),
        "제약된 자식이 할당 실패 없이 안전 상한(4096MiB)까지 다 채웠다 — \
         Job Object 상한이 전혀 걸리지 않았다는 뜻이다. allocated_bytes={}",
        result.allocated_bytes
    );

    // ★ 정확히 COMMIT_LIMIT_MIB 에서 실패하리라고 기대하지 않는다 —
    //   프로세스 시작 오버헤드(런타임 초기화·스택 등)가 이미 일부
    //   커밋을 차지하므로 유효 여유는 그보다 작다. "상한보다 훨씬
    //   많이 할당하지 못했다" 만 확인한다.
    assert!(
        result.allocated_bytes <= COMMIT_LIMIT_MIB * 1024 * 1024,
        "제약된 자식이 상한({} bytes)보다 많이({} bytes) 할당했다 — \
         Job Object 커밋 상한이 강제되지 않았다",
        COMMIT_LIMIT_MIB * 1024 * 1024,
        result.allocated_bytes
    );

    // ★ 실측(2026-08-18, 5회 연속) — `PeakJobMemoryUsed` 가
    //   `JobMemoryLimit` 을 **항상 약간 넘었다**(약 737KiB~836KiB, 이
    //   테스트의 chunk 크기 4MiB 보다 작다 — "한 청크가 더 통과했다"가
    //   아니라 그보다 작은 오버슈트다). `JOB_OBJECT_LIMIT_JOB_MEMORY`
    //   는 딱딱한 상한이 아니라 "이 근처에서 이후 커밋을 거부하기
    //   시작한다"는 **소프트** 제한이라는 뜻으로 읽는다 — 이것이 바로
    //   `crates/runtime-policy/src/vram.rs::guarantees_hard_limit()`
    //   가 `WindowsCommitCap` 도 `false` 라고 미리 못박아 둔 이유를
    //   실측으로 확인한 것이다. 여유(margin)는 관측값의 약 2.5배인
    //   2MiB 로 잡는다 — 느슨하게 잡되 "전혀 강제되지 않았다"(예:
    //   4096MiB 안전 상한까지 다 채웠다) 는 절대 통과시키지 않는다.
    const OBSERVED_OVERSHOOT_TOLERANCE_BYTES: usize = 2 * 1024 * 1024;
    assert!(
        peak <= limit + OBSERVED_OVERSHOOT_TOLERANCE_BYTES,
        "PeakJobMemoryUsed({peak})가 JobMemoryLimit({limit}) + 오버슈트 여유({OBSERVED_OVERSHOOT_TOLERANCE_BYTES})를 넘었다 — \
         실측(700~850KiB)보다 훨씬 큰 오버슈트는 Job Object 강제가 사실상 안 걸린 것으로 본다"
    );
    assert_eq!(
        limit,
        (COMMIT_LIMIT_MIB * 1024 * 1024) as usize,
        "Job 이 실제로 우리가 요청한 상한을 갖고 있는지 확인 — \
         SetInformationJobObject 호출 자체가 무시됐다면 이 값이 0 이거나 다를 것이다"
    );
}

/// ★ 비공허성(negative control) — 같은 fixture, 같은 chunk 크기를
/// **제약 없이** 돌리면 위 테스트의 상한(64MiB)보다 훨씬 많이 할당할
/// 수 있어야 한다. 이게 없으면 위 테스트의 "실패"가 Job Object 때문인지
/// fixture 자체의 한계 때문인지 구분할 수 없다.
#[test]
fn unconstrained_child_allocates_far_more_than_the_cap() {
    let temp = tempfile::tempdir().unwrap();
    let result_file = temp.path().join("result.txt");

    let exe = fixture_bin();
    let status = Command::new(&exe)
        .arg(CHUNK_MIB.to_string())
        .arg(&result_file)
        .status()
        .expect("alloc_fixture 스폰 실패 — 먼저 빌드해야 한다");
    assert!(
        status.success(),
        "제약 없는 fixture 실행 자체가 실패했다: {status:?}"
    );

    let result = parse_result(&result_file);

    assert!(
        result.allocated_bytes > COMMIT_LIMIT_MIB * 1024 * 1024 * 4,
        "제약 없는 자식이 상한의 4배도 할당 못 했다({} bytes) — \
         negative control 이 성립하지 않는다. 이 값이 작으면 위 테스트의 \
         '실패'가 Job Object 때문이라고 주장할 근거가 없다",
        result.allocated_bytes
    );
}
