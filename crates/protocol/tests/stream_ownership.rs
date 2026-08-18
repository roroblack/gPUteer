//! 스트림 소유권 경계 — `RULE.md` §4.1 · `docs/contracts/01_스트림_소유권.md` 의 **강제 장치**.
//!
//! # 왜 필요한가
//!
//! 소유권 규칙은 문서에만 있으면 지켜지지 않는다. 실제로 어겼다.
//!
//! ```text
//! 2026-08-16  crates/protocol/src/signing.rs 가 ed25519-dalek 을 직접 썼다.
//!             RULE.md §4.1 은 Ed25519 를 Crypto 스트림 소유로 정한다.
//!             테스트 100건이 전부 통과했고 아무도 알아채지 못했다.
//! ```
//!
//! `cargo add` 한 줄이면 경계가 무너지고, **테스트는 여전히 초록색**이다.
//! P0-08 의 `SCHEMA_FINGERPRINT` 와 같은 원리로 여기서도 경계를 코드로 고정한다.
//!
//! # 무엇을 검사하는가
//!
//! `Cargo.toml` 의 의존성 목록만 본다. 소스의 `use` 는 보지 않는다.
//! 크레이트가 의존하지 않는 라이브러리는 **쓸 수 없으므로** 의존성만 막으면 충분하다.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `Cargo.toml` 의 `[dependencies]` · `[build-dependencies]` · `[dev-dependencies]`
/// 에 나오는 크레이트 이름을 전부 뽑는다.
fn declared_deps(crate_dir: &str) -> BTreeSet<String> {
    let path = repo_root().join("crates").join(crate_dir).join("Cargo.toml");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 읽기 실패: {e}", path.display()));

    let mut out = BTreeSet::new();
    let mut in_deps = false;
    for raw in src.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            in_deps = matches!(
                line,
                "[dependencies]" | "[build-dependencies]" | "[dev-dependencies]"
            );
            continue;
        }
        if !in_deps || line.is_empty() {
            continue;
        }
        // "name = ..." / "name.workspace = true"
        let Some((lhs, _)) = line.split_once('=') else {
            continue;
        };
        let name = lhs.trim().split('.').next().unwrap_or("").trim();
        if !name.is_empty() {
            out.insert(name.to_string());
        }
    }
    out
}

/// 파서가 조용히 빈 결과를 내면 아래 검사가 전부 공허해진다.
#[test]
fn parser_is_not_vacuous() {
    let p = declared_deps("protocol");
    assert!(p.contains("prost"), "protocol 의 prost 의존성을 못 찾았다: {p:?}");
    assert!(p.contains("blake3"));
    assert!(p.contains("prost-build"), "build-dependencies 를 못 읽었다");

    let c = declared_deps("crypto");
    assert!(c.contains("ed25519-dalek"), "crypto 의 ed25519 의존성을 못 찾았다: {c:?}");
}

// ══════════════════════════════════════════════════════════════════
// 경계
// ══════════════════════════════════════════════════════════════════

/// `RULE.md` §4.1 — Ed25519 는 **Crypto 스트림** 소유다.
///
/// `crates/protocol` 은 **무엇을 서명하는가 · 어떤 순서로 검증하는가**만 정의하고,
/// 서명을 실제로 만들고 확인하는 일은 `crates/crypto` 가 한다.
///
/// 이 경계가 있어야 서명 알고리즘을 바꿔도 프로토콜 규범이 흔들리지 않는다.
#[test]
fn protocol_does_not_depend_on_crypto_libraries() {
    // BLAKE3 는 예외다 — canonical 인코딩(§6 Merkle · sig_input 다이제스트)의
    // 일부라서 프로토콜 정의 자체에 들어간다. 소유권 표의 "BLAKE3" 항목은
    // 키 유도·MAC 용도를 가리킨다.
    const ALLOWED: &[&str] = &["blake3"];

    let forbidden = [
        "ed25519-dalek",
        "ed25519",
        "signature",
        "rand",
        "rand_core",
        "sha2",
        "curve25519-dalek",
        "ring",
        "rustls",
        "aes-gcm",
        "chacha20poly1305",
    ];

    let deps = declared_deps("protocol");
    let violations: Vec<_> = forbidden
        .iter()
        .filter(|f| deps.contains(**f) && !ALLOWED.contains(*f))
        .collect();

    assert!(
        violations.is_empty(),
        "crates/protocol 이 암호 라이브러리에 의존한다: {violations:?}\n\
         RULE.md §4.1 — Ed25519 · 키 보관은 Crypto 스트림(crates/crypto) 소유다.\n\
         프로토콜에는 trait 만 두고 구현은 crates/crypto 에 둔다 (§4.3)."
    );
}

/// Crypto 는 Protocol 의 trait 를 **구현**한다. 반대 방향 의존은 순환이다.
#[test]
fn crypto_depends_on_protocol_not_the_reverse() {
    assert!(
        declared_deps("crypto").contains("gputeer-protocol"),
        "crypto 가 protocol 의 trait 를 구현하려면 의존해야 한다"
    );
    assert!(
        !declared_deps("protocol").contains("gputeer-crypto"),
        "순환 의존 — protocol 이 crypto 를 의존하면 계약이 구현을 따라간다 (RULE.md §3.5 위반)"
    );
}

/// `crates/protocol` 은 **아무 크레이트도 의존하지 않는다** (§4.3 — 공용 trait 는
/// protocol 에서 먼저 확정하고, 구현 크레이트가 그것을 구현한다).
#[test]
fn protocol_is_a_leaf_crate() {
    let internal: Vec<_> = declared_deps("protocol")
        .into_iter()
        .filter(|d| d.starts_with("gputeer-"))
        .collect();
    assert!(
        internal.is_empty(),
        "protocol 이 다른 gputeer 크레이트를 의존한다: {internal:?}\n\
         계약 크레이트는 잎이어야 모든 스트림이 같은 계약을 본다."
    );
}

/// 새 크레이트가 생기면 이 목록에 등록하게 만든다.
///
/// 등록되지 않은 크레이트는 소유권 검사를 받지 않는다 —
/// `field_number_audit` 의 `every_impl_is_audited` 와 같은 이유다.
#[test]
fn every_crate_is_covered_by_ownership_rules() {
    // ★ 2026-08-17 `cli` 추가. 소유권 표(`docs/contracts/01_스트림_소유권.md`)에
    //   이미 `CLI | crates/cli/ | gputeer 명령` 으로 선언되어 있다.
    //   이 가드가 새 크레이트를 실제로 잡았다 — 등록 없이 통과하지 않았다.
    //
    // ★ 2026-08-18 `coordinator`·`agent` 추가
    //   (`docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`).
    //   소유권 표에도 이미 `Coordinator | crates/coordinator/`,
    //   `Agent | crates/agent/` 로 선언되어 있었다 — 아직 크레이트가
    //   없던 시점부터 자리를 예약해 둔 것이다(같은 문서의
    //   "★ 5.2 4종은 proto 메시지가 없다"와 같은 성격).
    //   이 가드가 이번에도 실제로 잡았다.
    //
    // ★ 2026-08-18 `runtime-windows` 추가 — VRAM 판정
    //   (`crates/runtime-policy/src/vram.rs`)을 실제 Windows Job Object
    //   커밋 상한 호출로 연결한다. 소유권 표(`RULE.md:159`)에도 이미
    //   `Runtime | crates/runtime-windows/` 로 예약되어 있었다.
    const KNOWN: &[&str] = &[
        "protocol",
        "crypto",
        "checkpoint",
        "cli",
        "runtime-policy",
        "runtime-windows",
        "coordinator",
        "agent",
    ];

    let dir = repo_root().join("crates");
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .expect("crates/ 읽기")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("Cargo.toml").exists())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    found.sort();

    let unknown: Vec<_> = found
        .iter()
        .filter(|c| !KNOWN.contains(&c.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "소유권 검사에 등록되지 않은 크레이트: {unknown:?}\n\
         RULE.md §4.1 표에 스트림을 정하고 이 테스트의 KNOWN 에 추가하라."
    );
    assert_eq!(found.len(), KNOWN.len(), "KNOWN 에 없어진 크레이트가 남아 있다");
}
