//! proto/*.proto -> Rust 코드 생성.
//!
//! ★ 생성된 코드는 직접 수정하지 않는다 (RULE.md §4.3).
//! ★ 서명에는 prost 의 기본 인코더를 쓰지 않는다. canonical.rs 를 쓴다 (signing.md §13.1).

use std::io::Result;

fn main() -> Result<()> {
    // protoc 를 vendored 바이너리로 고정한다. 개발자 기계에 protoc 설치를 요구하지 않는다.
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    std::env::set_var("PROTOC", protoc);

    let protos = [
        "../../proto/common.proto",
        "../../proto/job.proto",
        "../../proto/lease.proto",
        "../../proto/artifact.proto",
        "../../proto/control.proto",
    ];
    for p in &protos {
        println!("cargo:rerun-if-changed={p}");
    }

    let mut cfg = prost_build::Config::new();
    cfg.compile_protos(&protos, &["../../proto"])?;
    Ok(())
}
