//! 스키마 지문 — `signing.md` §7.3 "schema_version 증가 없는 필드 추가 금지" 의 **강제 장치**.
//!
//! # 왜 필요한가 (P0-08 결과)
//!
//! P0-08 이 실측으로 확인한 것:
//!
//! ```text
//! prost 는 미지 필드를 조용히 버린다        (118B -> 148B -> 118B)
//! 미지 필드는 canonical 에 흔적이 없다
//! -> 구버전 검증자는 메시지 본문만 보고는 새 필드의 존재를 알 수 없다
//! -> 유일한 신호는 schema_version 이다
//! ```
//!
//! 그래서 `signing.md` §7.3 은 "schema_version 증가 없는 필드 추가" 를 **금지**한다.
//! 그런데 **프로토콜은 이 금지를 강제하지 못한다.** 누군가 `.proto` 에 필드를
//! 하나 추가하고 `schema_version` 상수를 그대로 두면, 구버전은 새 보안 제약을
//! 무시한 채 서명 검증을 통과시킨다. 아무도 알아채지 못한다.
//!
//! **규범이 강제할 수 없는 규칙을 두면 그것은 규범이 아니라 희망이다**
//! (`CLAUDE.md` §0.4 와 같은 정신).
//!
//! # 이 테스트가 하는 일
//!
//! `.proto` 전체의 지문을 계산해 `proto/SCHEMA_FINGERPRINT.txt` 와 대조한다.
//! 스키마가 바뀌면 실패하며, 두 가지 중 하나를 하게 만든다.
//!
//! ```text
//! (a) schema_version 을 올린다        새 필드 추가 · 의미 변경
//! (b) 지문만 갱신한다                 주석 · 서식 · 신규 메시지 추가 등
//!                                     -> 왜 버전을 올리지 않아도 되는지 커밋에 적는다
//! ```
//!
//! 지문 갱신:
//!
//! ```text
//! UPDATE_SCHEMA_FINGERPRINT=1 cargo test -p gputeer-protocol --test schema_fingerprint
//! ```
//!
//! # 한계 — 실측으로 확인한 것
//!
//! ★ 처음에 "정규식 파서라 `oneof` 를 다루지 못한다" 고 적었는데 **틀렸다.**
//!   `control.proto` 에는 `oneof` 가 5개 있고, 파서는 그 안의 필드를
//!   **전부 잡는다** (`ControlAction` 의 21개 필드가 지문에 있다).
//!   뮤테이션으로 확인했다 — `oneof` 안에 `AddMember sneaky_action = 44;` 를
//!   넣으면 "추가된 줄: 44 AddMember sneaky_action" 을 내며 실패한다.
//!
//!   **틀린 한계 서술은 없는 것만큼 나쁘다.** 누군가 불필요하게 파서를 갈아엎거나,
//!   멀쩡한 가드를 믿지 못하게 된다.
//!
//! 실제로 남아 있는 한계:
//!
//! ```text
//! oneof 소속을 기록하지 않는다
//!     필드를 oneof 안팎으로 옮기면서 번호·타입·이름을 그대로 두면 탐지되지 않는다.
//!     의미(상호 배타성)가 바뀌는 변경이므로 §7.3 대상인데 지문은 통과한다.
//!
//! 중첩 message 선언을 처리하지 못한다
//!     `message Outer { message Inner { ... } }` 의 Inner 필드는 Outer 것으로
//!     기록되며, 번호가 겹치면 덮어쓴다.
//!     -> 합성 입력으로 이 동작을 확인했다. 현 스키마에 중첩 선언은 **0건**이며,
//!        도입되면 별도 지원 구문 검사가 명확히 실패한다.
//!
//! reserved 는 필드로 기록하지 않는다
//!     합성 입력으로 명시적인 건너뛰기 분기를 확인했다. 다만 reserved 자체는 지문에
//!     기록되지 않는다. 현 스키마에 reserved 는 **0건**이며, 도입되면 별도 지원 구문
//!     검사가 검토와 가드 확장을 요구하며 실패한다.
//! ```
//!
//! 지문은 **필드 집합의 변화**를 잡으며, 주석·공백·필드 선언 순서 변화는 무시한다.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gputeer_protocol::blake3_256;

const PROTO_FILES: &[&str] = &[
    "common.proto",
    "job.proto",
    "lease.proto",
    "artifact.proto",
    "control.proto",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[derive(Debug)]
enum SyntaxToken {
    Ident(String, usize),
    OpenBrace,
    CloseBrace,
}

/// 지원 한계 탐지용 최소 lexer. 주석과 문자열 안의 키워드는 구문으로 세지 않는다.
fn proto_syntax_tokens(src: &str) -> Vec<SyntaxToken> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut line = 1usize;

    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                line += 1;
                index += 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    if bytes[index] == b'\n' {
                        line += 1;
                    }
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            quote @ (b'\'' | b'"') => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        break;
                    } else {
                        if bytes[index] == b'\n' {
                            line += 1;
                        }
                        index += 1;
                    }
                }
            }
            b'{' => {
                tokens.push(SyntaxToken::OpenBrace);
                index += 1;
            }
            b'}' => {
                tokens.push(SyntaxToken::CloseBrace);
                index += 1;
            }
            ch if ch.is_ascii_alphabetic() || ch == b'_' => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                tokens.push(SyntaxToken::Ident(src[start..index].to_string(), line));
            }
            _ => index += 1,
        }
    }

    tokens
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scope {
    Message,
    Other,
}

fn validate_guard_supported_syntax(file: &str, src: &str) -> Result<(), String> {
    let tokens = proto_syntax_tokens(src);
    let mut message_openings: BTreeMap<usize, (&str, usize)> = BTreeMap::new();

    for (index, window) in tokens.windows(3).enumerate() {
        if let [SyntaxToken::Ident(keyword, line), SyntaxToken::Ident(name, _), SyntaxToken::OpenBrace] =
            window
        {
            if keyword == "message" {
                message_openings.insert(index + 2, (name.as_str(), *line));
            }
        }
    }

    let mut scopes = Vec::new();
    let mut violations = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            SyntaxToken::Ident(keyword, line) if keyword == "reserved" => {
                violations.push(format!(
                    "{file}:{line}: `reserved` declaration is unsupported: the parser deliberately skips it, so the reservation itself is absent from the fingerprint. Extend the fingerprint representation and its regression tests, then update this support check before introducing `reserved`."
                ));
            }
            SyntaxToken::OpenBrace => {
                if let Some((name, line)) = message_openings.get(&index) {
                    if scopes.contains(&Scope::Message) {
                        violations.push(format!(
                            "{file}:{line}: nested message `{name}` is unsupported: its fields are recorded under the outer message, and a reused field number silently overwrites the outer entry. Teach the parser nested ownership and add regression tests, then update this support check before introducing nested messages."
                        ));
                    }
                    scopes.push(Scope::Message);
                } else {
                    scopes.push(Scope::Other);
                }
            }
            SyntaxToken::CloseBrace => {
                scopes.pop();
            }
            SyntaxToken::Ident(_, _) => {}
        }
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "schema fingerprint guard refuses syntax it cannot safely enforce:\n{}",
            violations.join("\n")
        ))
    }
}

fn read_proto(file: &str) -> String {
    std::fs::read_to_string(repo_root().join("proto").join(file))
        .unwrap_or_else(|e| panic!("{file} 읽기 실패: {e}"))
}

/// `.proto` 한 파일에서 `message <Name> { <num> <name> <type> }` 를 전부 뽑는다.
///
/// 반환: "File::Message" -> (field number -> "type name")
fn parse_proto(file: &str) -> BTreeMap<String, BTreeMap<u32, String>> {
    let src = read_proto(file);
    validate_guard_supported_syntax(file, &src).unwrap_or_else(|message| panic!("{message}"));
    parse_proto_source(file, &src)
}

fn parse_proto_source(file: &str, src: &str) -> BTreeMap<String, BTreeMap<u32, String>> {
    let mut out: BTreeMap<String, BTreeMap<u32, String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut depth = 0usize;

    for raw in src.lines() {
        let line = raw.split("//").next().unwrap_or("").trim().to_string();
        if line.is_empty() {
            continue;
        }

        if current.is_none() {
            if let Some(rest) = line.strip_prefix("message ") {
                if let Some(name) = rest.split_whitespace().next() {
                    current = Some(format!("{file}::{name}"));
                    depth = line.matches('{').count();
                    continue;
                }
            }
            continue;
        }

        depth += line.matches('{').count();
        depth -= line.matches('}').count().min(depth);
        if depth == 0 {
            current = None;
            continue;
        }

        if !line.ends_with(';') || line.starts_with("option") {
            continue;
        }
        let body = line.trim_end_matches(';');
        // reserved 는 필드가 아니다. 유효한 reserved 문법은 `=`가 없으므로 먼저 거른다.
        if body.split_whitespace().next() == Some("reserved") {
            continue;
        }
        let Some((lhs, rhs)) = body.rsplit_once('=') else {
            continue;
        };
        let Ok(num) = rhs.trim().parse::<u32>() else {
            continue;
        };
        // "repeated Digest input_artifacts" / "map<string, string> env_vars"
        let parts: Vec<&str> = lhs.split_whitespace().collect();
        let Some(name) = parts.last() else { continue };
        let ty = parts[..parts.len() - 1].join(" ");
        let key = current.clone().unwrap();
        out.entry(key)
            .or_default()
            .insert(num, format!("{ty} {name}"));
    }
    out
}

/// 사람이 읽을 수 있는 지문 본문. 이것 자체를 파일로 저장한다.
///
/// 해시만 저장하면 "무엇이 바뀌었는지" 를 알 수 없어 리뷰가 불가능하다.
fn fingerprint_body() -> String {
    let mut all: BTreeMap<String, BTreeMap<u32, String>> = BTreeMap::new();
    for f in PROTO_FILES {
        all.extend(parse_proto(f));
    }
    assert!(
        all.len() >= 20,
        "메시지를 {}개만 파싱했다 — 파서 결함 (파일 5개에 20개 이상 있어야 한다)",
        all.len()
    );

    let mut s = String::new();
    for (msg, fields) in &all {
        s.push_str(msg);
        s.push('\n');
        for (num, decl) in fields {
            s.push_str(&format!("  {num} {decl}\n"));
        }
    }
    s
}

fn fingerprint_path() -> PathBuf {
    repo_root().join("proto/SCHEMA_FINGERPRINT.txt")
}

const HEADER: &str = "\
# proto 스키마 지문 — signing.md §7.3 강제 장치
#
# ★ 손으로 편집하지 않는다.
#   갱신: UPDATE_SCHEMA_FINGERPRINT=1 cargo test -p gputeer-protocol --test schema_fingerprint
#
# 이 파일이 바뀌었다면 둘 중 하나를 해야 한다.
#   (a) 필드 추가/의미 변경  -> schema_version 을 올린다 (signing.md §7.3)
#   (b) 그 외               -> 왜 버전을 올리지 않아도 되는지 커밋 메시지에 적는다
#
# P0-08 실측: prost 는 미지 필드를 조용히 버리며 canonical 에 흔적이 없다.
# 구버전 검증자가 새 필드의 존재를 알 수 있는 유일한 신호는 schema_version 이다.
";

#[test]
fn proto_schema_uses_only_fingerprint_guard_supported_syntax() {
    let mut violations = Vec::new();
    for file in PROTO_FILES {
        let src = read_proto(file);
        if let Err(message) = validate_guard_supported_syntax(file, &src) {
            violations.push(message);
        }
    }

    assert!(violations.is_empty(), "{}", violations.join("\n\n"));
}

#[test]
fn guard_rejects_nested_message_mutation_before_silent_overwrite() {
    let src = "message Outer {\n\
               string outer = 1;\n\
               message Inner {\n\
               string inner = 1;\n\
               }\n\
               }\n";

    let error = validate_guard_supported_syntax("synthetic.proto", src).unwrap_err();
    assert!(error.contains("nested message `Inner` is unsupported"));
    assert!(error.contains("silently overwrites the outer entry"));
    assert!(error.contains("Teach the parser nested ownership"));
}

#[test]
fn guard_rejects_reserved_mutation_before_it_can_be_invisible() {
    let src = "message Synthetic {\n\
               reserved 2, 4 to 6;\n\
               string kept = 1;\n\
               }\n";

    let error = validate_guard_supported_syntax("synthetic.proto", src).unwrap_err();
    assert!(error.contains("`reserved` declaration is unsupported"));
    assert!(error.contains("reservation itself is absent from the fingerprint"));
    assert!(error.contains("Extend the fingerprint representation"));
}

#[test]
fn support_check_ignores_keywords_in_comments_and_strings() {
    let src = "message Synthetic {\n\
               // message CommentedOut { reserved 1; }\n\
               /* reserved 2; message AlsoCommentedOut { } */\n\
               string text = 1 [default = \"reserved message NotADeclaration {\"];\n\
               }\n";

    assert_eq!(
        validate_guard_supported_syntax("synthetic.proto", src),
        Ok(())
    );
}

#[test]
fn parser_explicitly_skips_valid_reserved_declarations() {
    let src = "message Synthetic {\n\
               reserved 2, 4 to 6;\n\
               reserved \"old_name\", \"older_name\";\n\
               string kept = 1;\n\
               }\n";

    let parsed = parse_proto_source("synthetic.proto", src);
    let fields = parsed.get("synthetic.proto::Synthetic").unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields.get(&1).map(String::as_str), Some("string kept"));
}

#[test]
fn parser_nested_message_failure_mode_is_recorded() {
    let src = "message Outer {\n\
               string outer = 1;\n\
               message Inner {\n\
               string inner_reuses_one = 1;\n\
               bytes inner_only = 2;\n\
               }\n\
               }\n";

    let parsed = parse_proto_source("synthetic.proto", src);
    assert!(!parsed.contains_key("synthetic.proto::Inner"));
    let outer = parsed.get("synthetic.proto::Outer").unwrap();
    assert_eq!(
        outer.get(&1).map(String::as_str),
        Some("string inner_reuses_one")
    );
    assert_eq!(outer.get(&2).map(String::as_str), Some("bytes inner_only"));
}

/// 파서가 조용히 빈 결과를 내면 지문 검사 전체가 공허해진다.
#[test]
fn parser_is_not_vacuous() {
    let job = parse_proto("job.proto");
    let key = "job.proto::JobManifest".to_string();
    let m = job
        .get(&key)
        .unwrap_or_else(|| panic!("JobManifest 를 못 찾았다"));
    assert!(m.len() >= 25, "JobManifest 필드를 {}개만 뽑았다", m.len());
    assert_eq!(m.get(&13).map(String::as_str), Some("string entrypoint"));
    assert_eq!(
        m.get(&11).map(String::as_str),
        Some("repeated Digest input_artifacts")
    );
    assert_eq!(
        m.get(&15).map(String::as_str),
        Some("map<string, string> env_vars")
    );
    assert_eq!(
        m.get(&90).map(String::as_str),
        Some("bytes submitter_signature")
    );
}

#[test]
fn proto_schema_matches_recorded_fingerprint() {
    let body = fingerprint_body();
    let digest = blake3_256(body.as_bytes());
    let digest_hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let content = format!("{HEADER}#\n# blake3-256: {digest_hex}\n\n{body}");

    let path = fingerprint_path();

    if std::env::var("UPDATE_SCHEMA_FINGERPRINT").is_ok() {
        std::fs::write(&path, &content).expect("지문 파일 쓰기");
        println!("지문 갱신: {}\nblake3-256: {digest_hex}", path.display());
        return;
    }

    let recorded = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "지문 파일이 없다: {}\n\
             UPDATE_SCHEMA_FINGERPRINT=1 로 생성하라",
            path.display()
        )
    });
    let recorded = recorded.replace("\r\n", "\n");

    if recorded != content {
        // 무엇이 달라졌는지 보여준다. "다르다" 만으로는 고칠 수 없다.
        let old: Vec<&str> = recorded.lines().filter(|l| !l.starts_with('#')).collect();
        let new: Vec<&str> = content.lines().filter(|l| !l.starts_with('#')).collect();
        let added: Vec<_> = new.iter().filter(|l| !old.contains(l)).collect();
        let removed: Vec<_> = old.iter().filter(|l| !new.contains(l)).collect();

        panic!(
            "★ proto 스키마가 바뀌었다 (signing.md §7.3).\n\n\
             추가된 줄:\n  {}\n\n\
             사라진 줄:\n  {}\n\n\
             필드 추가 · 타입 변경 · 의미 변경이라면 **schema_version 을 올려야 한다.**\n\
             P0-08 실측대로, 구버전 검증자가 새 필드의 존재를 알 수 있는 유일한 신호가\n\
             schema_version 이기 때문이다. 올리지 않으면 구버전이 새 보안 제약을\n\
             무시한 채 서명 검증을 통과시킨다.\n\n\
             그 외(주석·서식)라면 지문만 갱신하고 사유를 커밋에 적는다:\n  \
             UPDATE_SCHEMA_FINGERPRINT=1 cargo test -p gputeer-protocol --test schema_fingerprint",
            if added.is_empty() {
                "(없음)".to_string()
            } else {
                added
                    .iter()
                    .map(|s| s.trim())
                    .collect::<Vec<_>>()
                    .join("\n  ")
            },
            if removed.is_empty() {
                "(없음)".to_string()
            } else {
                removed
                    .iter()
                    .map(|s| s.trim())
                    .collect::<Vec<_>>()
                    .join("\n  ")
            },
        );
    }
}
