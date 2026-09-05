//! `P0-02` 실측 프로브 — **AppContainer 안에서 CUDA 가 되는가.**
//!
//! 기준선 §32 가 요구하는 일곱 항목 중 이 프로브가 재는 것:
//!
//! ```text
//! [x] AppContainer 프로파일 생성
//! [x] AppContainer 에서 Python 실행
//! [x] torch.cuda.is_available()
//! [x] 작은 CUDA tensor 연산
//! [ ] filesystem allowlist / outbound 제한   <- 별도
//! [~] GPU 드라이버 접근 capability 식별      <- **빈 목록으로 띄워 무엇이
//!                                               실패하는지 본다**
//! [ ] Create Process in Sandbox API 상태     <- 별도
//! ```
//!
//! # 이 프로브가 지어내지 않는 것
//!
//! ★ **capability 를 추측해 넣지 않는다.** GPU 드라이버가 무엇을
//!   요구하는지 아직 아무도 모른다 — `P0-02` 가 그걸 **식별하라고**
//!   요구한다. 빈 목록으로 띄워 **무엇이 어떻게 실패하는지**가 곧 답이다.
//!
//! ★ **바깥 기준선을 먼저 잰다.** 컨테이너 안에서 실패했을 때 그것이
//!   "AppContainer 때문" 인지 "이 기계에서 원래 안 되는 것" 인지 구분해야
//!   한다. 대조 없이 "AppContainer 가 CUDA 를 막는다" 고 쓰면 거짓일 수
//!   있다.
//!
//! # 왜 결과를 파일로 받는가
//!
//! `CreateProcessW` 를 직접 부르면 표준 파이프 상속을 따로 설정해야 하고,
//! 안 한 채 stdout 을 신뢰하면 **조용히 빈 결과**를 얻는다
//! (`alloc_fixture` 가 같은 이유로 파일을 쓴다).
//!
//! 게다가 AppContainer 는 아무 데나 못 쓴다 — 그래서
//! `%LOCALAPPDATA%\Packages\<프로파일>\` 를 쓴다. Windows 가 프로파일을
//! 만들 때 **그 컨테이너에 권한을 주고 만드는 폴더**라 ACL 을 직접 줄
//! 필요가 없다(`the_profile_creates_a_folder_the_container_can_use` 가
//! 그 사실을 고정한다).
//!
//! # 쓰는 법
//!
//! ```text
//! p0_02_probe <python 실행 파일 경로>
//! ```
//!
//! GPU 가 있는 기계에서 돌려야 뜻이 있다.

/// 자주 나오는 NTSTATUS 를 사람이 읽을 수 있게 바꾼다.
///
/// ★ 숫자만 보고 "실패했다" 로 넘기면 **어디서** 막혔는지 잃는다.
///   `0xC0000135` 는 CUDA 와 아무 상관이 없고 **DLL 을 못 읽은 것**이다 —
///   그 둘을 구분 못 하면 "AppContainer 에서 CUDA 가 안 된다" 는 **틀린
///   결론**을 쓰게 된다.
#[cfg(windows)]
fn explain_exit(code: u32) -> &'static str {
    match code {
        0 => "정상",
        0xC000_0135 => "STATUS_DLL_NOT_FOUND — 필요한 DLL 을 못 읽었다(경로 ACL)",
        0xC000_0022 => "STATUS_ACCESS_DENIED — 접근이 거부됐다",
        0xC000_0142 => "STATUS_DLL_INIT_FAILED — DLL 초기화 실패",
        0xC000_00BB => "STATUS_NOT_SUPPORTED",
        _ => "(해설 없음)",
    }
}

#[cfg(windows)]
fn main() {
    use gputeer_runtime_windows::appcontainer::{run_in_container, AppContainerProfile};
    use std::path::PathBuf;

    let python = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("사용법: p0_02_probe <python 경로>");
            std::process::exit(2);
        }
    };

    const NAME: &str = "gputeer-p0-02-probe";

    let profile = match AppContainerProfile::create(
        NAME,
        "gPUteer P0-02 probe",
        "AppContainer + CUDA 실측. 끝나면 지워진다.",
    ) {
        Ok(p) => p,
        Err(e) => {
            println!("P0_02_RESULT stage=profile ok=false detail={e}");
            std::process::exit(1);
        }
    };
    println!(
        "P0_02 profile_sid={}",
        profile.sid_string().unwrap_or_else(|| "(못 읽음)".into())
    );

    let work: PathBuf = PathBuf::from(std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA"))
        .join("Packages")
        .join(NAME);
    let script = work.join("probe.py");
    let result = work.join("result.txt");
    let _ = std::fs::remove_file(&result);

    // ★ 스크립트는 **결과를 파일에 적는다.** 예외도 적는다 — 실패의 모양이
    //   곧 `P0-02` 의 답이라 삼키면 안 된다.
    let body = r#"import sys, traceback
out = []
try:
    import torch
    out.append("torch=" + torch.__version__)
    avail = torch.cuda.is_available()
    out.append("cuda_available=" + str(avail))
    if avail:
        out.append("device_count=" + str(torch.cuda.device_count()))
        out.append("device_name=" + torch.cuda.get_device_name(0))
        # 작은 CUDA tensor 연산 — "보인다" 와 "계산된다" 는 다른 사실이다.
        a = torch.ones(64, 64, device="cuda")
        b = (a @ a).sum().item()
        out.append("matmul_sum=" + str(b))
        out.append("compute=ok")
    else:
        out.append("compute=skipped")
except Exception:
    out.append("exception=" + traceback.format_exc().replace("\n", " | "))
open(sys.argv[1], "w", encoding="utf-8").write("\n".join(out))
"#;
    if let Err(e) = std::fs::write(&script, body) {
        println!("P0_02_RESULT stage=script ok=false detail={e}");
        std::process::exit(1);
    }

    // ── 1) 바깥 기준선 ────────────────────────────────────────────────
    //
    // ★ 이게 없으면 컨테이너 안의 실패를 해석할 수 없다.
    let outside_file = work.join("result_outside.txt");
    let _ = std::fs::remove_file(&outside_file);
    let outside = std::process::Command::new(&python)
        .arg(&script)
        .arg(&outside_file)
        .status();
    let outside_text = std::fs::read_to_string(&outside_file).unwrap_or_default();
    println!(
        "P0_02 outside exit={:?} result={}",
        outside.map(|s| s.code()).unwrap_or(None),
        outside_text.replace('\n', " ; ")
    );

    // ── 2) 컨테이너 안 ────────────────────────────────────────────────
    let command = format!("\"{}\" \"{}\" \"{}\"", python, script.display(), result.display());
    match run_in_container(&profile, &command, work.to_str()) {
        Ok(code) => {
            let text = std::fs::read_to_string(&result).unwrap_or_default();
            if text.is_empty() {
                // ★ 종료 코드만 보고 "됐다" 고 하지 않는다 — 파일이 비었으면
                //   스크립트가 시작조차 못 했을 수 있다.
                println!(
                    "P0_02_RESULT stage=inside ok=false exit={code} exit_hex=0x{code:08X} meaning={} detail=결과 파일이 비었다 — 스크립트가 시작하지 못했다",
                    explain_exit(code)
                );
            } else {
                println!(
                    "P0_02_RESULT stage=inside ok=true exit={code} result={}",
                    text.replace('\n', " ; ")
                );
            }
        }
        Err(e) => println!("P0_02_RESULT stage=inside ok=false detail={e}"),
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("이 프로브는 Windows 전용이다");
    std::process::exit(2);
}
