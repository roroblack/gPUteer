//! `to_fields.rs` 의 field number 가 `.proto` 와 일치하는가.
//!
//! # 왜 이 테스트가 필요한가
//!
//! `to_fields.rs` 의 결함은 3종류인데, **2종류는 컴파일러가 잡고 1종류는 못 잡는다.**
//!
//! ```text
//! 1. 타입 불일치   put_bytes 에 String     -> 컴파일 오류. 잡힌다
//! 2. 없는 필드     self.nonexistent        -> 컴파일 오류. 잡힌다
//! 3. 잘못된 번호   put_str(&mut f, 61, &self.entrypoint)  -> ★ 안 잡힌다
//! ```
//!
//! 3번이 가장 위험하다. 코드는 돌아가고, 서명도 만들어지고, 자기 자신과는
//! 검증도 통과한다. **다른 구현체와 붙는 순간에만 깨진다** — 그리고 그때는
//! 이미 서명된 매니페스트가 돌아다니고 있다.
//!
//! 그래서 `to_fields.rs` 소스와 `.proto` 소스를 둘 다 파싱해 대조한다.
//!
//! # 한계
//!
//! 정규식 기반 파싱이다. `.proto` 문법을 완전히 이해하지 않는다.
//! - `oneof` · `reserved` · 중첩 message 선언은 다루지 않는다
//! - 주석 안의 `put_str(...)` 는 오탐이 될 수 있다 (아래에서 주석 줄을 걸러낸다)
//!
//! 이 한계가 문제가 되면 `prost-reflect` 로 교체한다. 지금 스키마 규모(17종)에서는
//! 정규식이 충분하고, 의존성이 적은 쪽이 낫다.

use std::collections::BTreeMap;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `.proto` 에서 `message <Name> { ... }` 의 필드를 뽑는다.
/// 반환: field number -> snake_case 필드명
fn proto_fields(proto_file: &str, message: &str) -> BTreeMap<u32, String> {
    let src = std::fs::read_to_string(repo_root().join("proto").join(proto_file))
        .unwrap_or_else(|e| panic!("{proto_file} 읽기 실패: {e}"));

    let head = format!("message {message} {{");
    let start = src
        .find(&head)
        .unwrap_or_else(|| panic!("{proto_file} 에 `{head}` 없음"))
        + head.len();

    // 중괄호 깊이로 message 본문 끝을 찾는다 (중첩 message 대응)
    let mut depth = 1usize;
    let mut end = start;
    for (i, ch) in src[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &src[start..end];

    let mut out = BTreeMap::new();
    for raw in body.lines() {
        let line = raw.split("//").next().unwrap_or("").trim();
        if line.is_empty() || !line.ends_with(';') {
            continue;
        }
        if line.starts_with("reserved") || line.starts_with("option") {
            continue;
        }
        // "<type...> <name> = <num>;"  — type 에 공백이 있을 수 있다 (map<a, b>, repeated X)
        let Some((lhs, rhs)) = line.trim_end_matches(';').rsplit_once('=') else {
            continue;
        };
        let Ok(num) = rhs.trim().parse::<u32>() else {
            continue;
        };
        let Some(name) = lhs.split_whitespace().last() else {
            continue;
        };
        out.insert(num, name.to_string());
    }
    assert!(!out.is_empty(), "{proto_file}::{message} 에서 필드를 못 뽑았다");
    out
}

/// `to_fields.rs` 의 `impl ToCanonicalFields for pb::<Name>` 블록에서
/// `put_*(&mut f, <num>, ... self.<field> ...)` 호출을 뽑는다.
/// 반환: field number -> self.<field> 이름
fn impl_fields(message: &str) -> BTreeMap<u32, String> {
    let src = include_str!("../src/to_fields.rs");
    let head = format!("impl ToCanonicalFields for pb::{message} {{");
    let start = src
        .find(&head)
        .unwrap_or_else(|| panic!("to_fields.rs 에 `{head}` 없음"))
        + head.len();
    // 다음 `impl ToCanonicalFields` 또는 파일 끝까지
    let end = src[start..]
        .find("\nimpl ToCanonicalFields")
        .map(|i| start + i)
        .unwrap_or(src.len());
    let body = &src[start..end];

    let mut out = BTreeMap::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.starts_with("//") {
            continue; // 주석 안의 예시를 오탐하지 않는다
        }
        let Some(open) = line.find("(&mut f, ") else {
            continue;
        };
        let rest = &line[open + "(&mut f, ".len()..];
        let Some((num_s, tail)) = rest.split_once(',') else {
            continue;
        };
        let Ok(num) = num_s.trim().parse::<u32>() else {
            continue;
        };
        // tail 어딘가에 self.<name> 이 있다
        let Some(sp) = tail.find("self.") else {
            continue;
        };
        let name: String = tail[sp + "self.".len()..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        assert!(
            out.insert(num, name.clone()).is_none(),
            "{message}: field number {num} 이 두 번 쓰였다"
        );
    }
    assert!(!out.is_empty(), "to_fields.rs::{message} 에서 호출을 못 뽑았다");
    out
}

/// 파싱기 자체가 동작하는지 먼저 확인한다.
/// 파싱기가 조용히 빈 결과를 내면 아래 대조 테스트가 전부 공허해진다.
#[test]
fn parsers_are_not_vacuous() {
    let p = proto_fields("job.proto", "JobManifest");
    assert!(p.len() >= 20, "JobManifest 필드를 {}개만 뽑았다", p.len());
    assert_eq!(p.get(&13).map(String::as_str), Some("entrypoint"));
    assert_eq!(p.get(&90).map(String::as_str), Some("submitter_signature"));

    let i = impl_fields("JobManifest");
    assert!(i.len() >= 15, "impl 호출을 {}개만 뽑았다", i.len());
    assert_eq!(i.get(&13).map(String::as_str), Some("entrypoint"));
}

// ══════════════════════════════════════════════════════════════════
// 본 검사
// ══════════════════════════════════════════════════════════════════

fn audit(proto_file: &str, message: &str, signature_field: u32) {
    let proto = proto_fields(proto_file, message);
    let imp = impl_fields(message);

    // 1. 쓰인 모든 번호가 proto 에 있고, 이름이 일치하는가
    for (num, impl_name) in &imp {
        match proto.get(num) {
            None => panic!(
                "{message}: to_fields 가 field {num} 을 쓰는데 {proto_file} 에 그 번호가 없다"
            ),
            Some(proto_name) => assert_eq!(
                proto_name, impl_name,
                "{message}: field {num} 이 proto 에서는 `{proto_name}` 인데 \
                 to_fields 는 `self.{impl_name}` 을 넣었다 — 번호 오타다"
            ),
        }
    }

    // 2. 서명 필드는 절대 들어가면 안 된다 (signing.md 규칙 i)
    assert!(
        !imp.contains_key(&signature_field),
        "{message}: 서명 필드 {signature_field} 이 canonical 에 들어갔다 — 자기참조 순환"
    );

    println!("{message}: proto {}개 필드 중 {}개 서명 대상", proto.len(), imp.len());
}

#[test]
fn job_manifest_field_numbers_match_proto() {
    audit("job.proto", "JobManifest", 90);
}

#[test]
fn lease_field_numbers_match_proto() {
    audit("lease.proto", "Lease", 90);
}

/// 감사 대상 — `ToCanonicalFields` 를 구현한 모든 메시지.
///
/// **새 `impl` 을 추가하면 여기에도 반드시 넣는다.**
/// 빠뜨리면 그 메시지의 field number 는 아무도 대조하지 않는다.
const AUDITED: &[(&str, &str, u32)] = &[
    ("job.proto", "JobManifest", 90),
    ("lease.proto", "Lease", 90),
    ("common.proto", "Digest", 90),
    ("common.proto", "CudaRequirement", 90),
    ("common.proto", "GpuRequest", 90),
    ("common.proto", "ResourceRequest", 90),
    ("common.proto", "WorkloadHint", 90),
    ("common.proto", "TarballPolicy", 90),
    ("common.proto", "ExecutionEnvironment", 90),
    ("common.proto", "DatasetRef", 90),
    ("common.proto", "NetworkPolicy", 90),
    ("common.proto", "ArtifactScope", 90),
    ("common.proto", "ResourceScope", 90),
    // T1 (2026-08-16)
    ("artifact.proto", "ReportedMetric", 90),
    ("artifact.proto", "CheckpointFile", 90),
    ("artifact.proto", "ResumeCompleteness", 90),
    ("artifact.proto", "CheckpointManifest", 90),
    ("artifact.proto", "ReplicaAck", 90),
    ("artifact.proto", "ArtifactRef", 90),
    ("artifact.proto", "AttemptReport", 90),
    ("artifact.proto", "CanonicalDecision", 90),
    ("lease.proto", "ProgressReport", 90),
    ("lease.proto", "RenewLeaseRequest", 90),
    ("lease.proto", "RevokeLeaseNotice", 90),
];

#[test]
fn common_message_field_numbers_match_proto() {
    for (file, msg, sig) in AUDITED {
        if *file == "common.proto" {
            audit(file, msg, *sig);
        }
    }
}

/// T1 — artifact.proto / lease.proto 의 서명 대상.
#[test]
fn artifact_and_lease_field_numbers_match_proto() {
    for (file, msg, sig) in AUDITED {
        if *file == "artifact.proto" || *file == "lease.proto" {
            audit(file, msg, *sig);
        }
    }
}

/// `to_fields.rs` 의 모든 `impl` 이 감사 대상에 등록되어 있는가.
///
/// ★ 등록되지 않은 `impl` 은 field number 대조를 받지 않는다.
///   이 테스트가 없으면 감사망에 조용히 구멍이 생긴다.
#[test]
fn every_impl_is_audited() {
    let src = include_str!("../src/to_fields.rs");
    let mut impls = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("impl ToCanonicalFields for pb::") {
            impls.push(rest.trim_end_matches(" {").to_string());
        }
    }
    assert!(impls.len() >= 20, "impl 을 {}개만 찾았다 — 파서 결함", impls.len());

    let missing: Vec<_> = impls
        .iter()
        .filter(|m| !AUDITED.iter().any(|(_, a, _)| *a == m.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "ToCanonicalFields 를 구현했는데 AUDITED 에 없는 메시지가 있다.\n\
         field number 대조를 받지 않는다:\n  {missing:?}"
    );
}

// ══════════════════════════════════════════════════════════════════
// ★ 누락 감사 — 서명에서 빠진 필드가 전부 선언되어 있는가
//
// 이것이 이 파일에서 가장 중요한 테스트다.
// 서명 대상에서 조용히 빠진 필드는 **위조 가능한 필드**다.
// ══════════════════════════════════════════════════════════════════

fn missing_fields(proto_file: &str, message: &str, signature_field: u32) -> Vec<(u32, String)> {
    let proto = proto_fields(proto_file, message);
    let imp = impl_fields(message);
    proto
        .into_iter()
        .filter(|(n, _)| *n != signature_field && !imp.contains_key(n))
        .collect()
}

#[test]
fn every_unsigned_field_is_declared_in_unimplemented_list() {
    use gputeer_protocol::UNIMPLEMENTED_FIELDS;

    let mut undeclared = Vec::new();
    for (file, msg, sig) in AUDITED {
        for (num, name) in missing_fields(file, msg, *sig) {
            let declared = UNIMPLEMENTED_FIELDS
                .iter()
                .any(|(m, n, _)| m == msg && *n == num);
            if !declared {
                undeclared.push(format!("{msg} field {num} ({name})"));
            }
        }
    }

    assert!(
        undeclared.is_empty(),
        "서명 대상에서 빠졌는데 UNIMPLEMENTED_FIELDS 에도 없는 필드가 있다.\n\
         이 필드들은 **위조 가능**하다. to_fields.rs 에 넣거나 목록에 선언하라:\n  {}",
        undeclared.join("\n  ")
    );
}

/// 반대 방향 — 목록에 있는데 실은 구현된 필드가 있으면 목록이 낡은 것이다.
#[test]
fn unimplemented_list_has_no_stale_entries() {
    use gputeer_protocol::UNIMPLEMENTED_FIELDS;

    let mut stale = Vec::new();
    for (msg, num, desc) in UNIMPLEMENTED_FIELDS {
        let imp = impl_fields(*msg);
        if imp.contains_key(num) {
            stale.push(format!("{msg} field {num} ({desc})"));
        }
    }
    assert!(
        stale.is_empty(),
        "UNIMPLEMENTED_FIELDS 에 있는데 이미 구현된 항목이 있다 — 목록에서 지워라:\n  {}",
        stale.join("\n  ")
    );
}
