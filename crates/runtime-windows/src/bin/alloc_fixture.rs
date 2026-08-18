//! `runtime-windows` 커밋 상한 실측 테스트용 fixture.
//!
//! `crates/runtime-windows/tests/commit_cap.rs` 가 이 바이너리를
//! `create_constrained_child`(Job Object 커밋 상한 적용) 와 일반
//! `Command::spawn`(제약 없음, negative control) 양쪽으로 띄워
//! 같은 조건에서 실제로 다른 결과가 나오는지 비교한다.
//!
//! 인자: `alloc_fixture <chunk_mib> <result_file>`
//!
//! `VirtualAlloc(MEM_RESERVE | MEM_COMMIT)` 를 chunk 단위로 반복해
//! 실패할 때까지(또는 안전 상한 4096MiB 도달까지) 커밋하고, 결과를
//! `result_file` 에 `allocated_bytes=<n>\nlast_error=<code 또는 none>\n`
//! 형식으로 적는다. **stdout 이 아니라 파일에 적는다** — `CreateProcessW`
//! 를 직접 부르는 호출자는 표준 파이프 핸들 상속을 설정하지 않으므로
//! stdout 캡처를 신뢰할 수 없다. 파일은 어느 경로에서든 신뢰할 수 있다.

use std::env;
use std::fs;

#[cfg(windows)]
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (chunk_mib, result_file) = match args.as_slice() {
        [chunk, file] => (
            chunk.parse::<usize>().expect("chunk_mib 는 정수여야 한다"),
            file.clone(),
        ),
        _ => {
            eprintln!("usage: alloc_fixture <chunk_mib> <result_file>");
            std::process::exit(2);
        }
    };

    // ★ 안전 상한. 이 fixture 를 제약 없이(negative control) 돌릴 때
    //   호스트 메모리를 실제로 고갈시키지 않기 위해서다 — 4096MiB 는
    //   이 테스트가 쓰는 상한(수십~수백 MiB)보다 훨씬 크므로 "제약이
    //   있었다면 훨씬 일찍 실패했어야 한다" 를 비교하기에 충분하다.
    const SAFETY_CEILING_MIB: usize = 4096;

    const MEM_RESERVE: u32 = 0x2000;
    const MEM_COMMIT: u32 = 0x1000;
    const PAGE_READWRITE: u32 = 0x04;

    let chunk_bytes = chunk_mib * 1024 * 1024;
    let mut allocated_bytes: usize = 0;
    let mut last_error: Option<u32> = None;

    while allocated_bytes < SAFETY_CEILING_MIB * 1024 * 1024 {
        let ptr = unsafe {
            windows_sys::Win32::System::Memory::VirtualAlloc(
                std::ptr::null(),
                chunk_bytes,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        if ptr.is_null() {
            last_error = Some(unsafe { windows_sys::Win32::Foundation::GetLastError() });
            break;
        }
        // ★ 커밋만으로는 페이지가 실제로 물리적으로 붙는지(vs. 그냥
        //   장부상 커밋) 의심할 수 있어, 각 청크의 첫 바이트를 실제로
        //   건드려 페이지 폴트를 강제한다.
        unsafe {
            std::ptr::write_volatile(ptr as *mut u8, 0xAB);
        }
        allocated_bytes += chunk_bytes;
    }

    let body = format!(
        "allocated_bytes={allocated_bytes}\nlast_error={}\n",
        last_error.map(|e| e.to_string()).unwrap_or_else(|| "none".into())
    );
    fs::write(&result_file, body).expect("결과 파일 쓰기 실패");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("alloc_fixture 는 Windows 전용이다");
    std::process::exit(2);
}
