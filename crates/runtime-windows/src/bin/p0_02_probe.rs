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
    use gputeer_runtime_windows::appcontainer::{run_in_container_capture, AppContainerProfile};

    let python = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("사용법: p0_02_probe <python 경로>");
            std::process::exit(2);
        }
    };

    const NAME: &str = "gputeer-p0-02-probe";

    // ★★ **파일을 하나도 안 쓴다.** 코드는 `-c` 로 넘기고 결과는 stdout
    //   으로 받는다 — 그래서 컨테이너에 작업 폴더 권한을 줄 필요가 없다.
    //
    //   1차 실측(2026-09-05)이 정확히 거기서 막혔다: Python 은 떴는데
    //   `[Errno 13] Permission denied` 로 스크립트 파일을 못 읽었다.
    //   운영자가 손으로 권한 주는 일을 하나라도 줄인다.
    //
    // ★ 작은따옴표만 쓴다 — `CreateProcessW` 인용 규칙이 얽히면 거기부터
    //   디버깅하게 된다. 예외는 stderr 로 나가고 그것도 같이 캡처한다.
    const CODE: &str = "import torch;a=torch.cuda.is_available();print('torch='+torch.__version__+' ; cuda_available='+str(a));t=(torch.ones(64,64,device='cuda') if a else None);print('device_name='+torch.cuda.get_device_name(0)+' ; matmul_sum='+str((t@t).sum().item())+' ; compute=ok') if a else print('compute=skipped')";

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

    // ── 1) 바깥 기준선 ────────────────────────────────────────────────
    //
    // ★ 이게 없으면 안쪽 실패가 "AppContainer 때문" 인지 "이 기계가 원래
    //   안 되는 것" 인지 구분할 수 없다.
    match std::process::Command::new(&python).arg("-c").arg(CODE).output() {
        Ok(o) => println!(
            "P0_02 outside exit={:?} result={}",
            o.status.code(),
            String::from_utf8_lossy(&o.stdout).replace('\r', "").trim()
        ),
        Err(e) => println!("P0_02 outside 실행 실패: {e}"),
    }

    // ── 2) 컨테이너 안 ────────────────────────────────────────────────
    let command = format!("\"{python}\" -c \"{CODE}\"");
    match run_in_container_capture(&profile, &command, None) {
        Ok((code, text)) => {
            let clean = text.replace('\r', "").replace('\n', " ; ");
            let clean = clean.trim();
            if clean.is_empty() {
                // ★ 종료 코드만 보고 "됐다" 고 하지 않는다.
                println!(
                    "P0_02_RESULT stage=inside ok=false exit={code} exit_hex=0x{code:08X} meaning={} detail=출력이 비었다",
                    explain_exit(code)
                );
            } else {
                println!(
                    "P0_02_RESULT stage=inside ok={} exit={code} exit_hex=0x{code:08X} meaning={} output={clean}",
                    code == 0,
                    explain_exit(code)
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
