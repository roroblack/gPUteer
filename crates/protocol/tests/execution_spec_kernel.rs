use std::collections::BTreeMap;

use gputeer_protocol::execution_spec::{derive_execution_spec, ExecutionSpecError, SpecField};
use gputeer_protocol::signing::{verify, NoReplayCheck, SignatureVerifier, VerifyOutcome};
use gputeer_protocol::{pb, Verified};

/// 테스트 전용 검증자.
///
/// 이 kernel 은 서명을 **검증하지 않는다** — `Verified<M>` 를 받는 것이
/// 계약이고, 검증은 그 전 단계의 일이다. 그래서 이 테스트도 실제 서명
/// 알고리즘을 재현하지 않고, `verify()` 경로를 실제로 통과시켜
/// `Verified` 를 얻는 데만 쓴다.
///
/// ★ **뒷문이 아니다.** `Verified` 를 만드는 유일한 길은 여전히
/// `verify()` 이며, raw `pb::JobManifest` 로 kernel 을 부르는 것은
/// `execution_spec.rs` 의 compile_fail doctest 가 막는다.
struct AcceptAll;

impl SignatureVerifier for AcceptAll {
    fn verify_signature(
        &self,
        _signer_id: &str,
        _message: &[u8],
        _signature: &[u8],
    ) -> Result<(), VerifyOutcome> {
        Ok(())
    }
}

/// `verify()` 를 실제로 통과시켜 `Verified` 를 얻는다.
fn verified_manifest(mut manifest: pb::JobManifest) -> Verified<pb::JobManifest> {
    manifest.schema_version = 1;
    // `JobManifest` 는 `Lifetime::LongLived` 라 만료 검사를 받는다.
    manifest.issued_at_unix_ms = 1_000;
    manifest.expires_at_unix_ms = 1_000_000;
    manifest.submitter_signature = vec![0u8; 64];
    verify(&manifest, 1, &AcceptAll, 2_000, &mut NoReplayCheck)
        .expect("테스트 fixture 는 verify() 를 통과해야 한다")
}

fn base_manifest() -> pb::JobManifest {
    pb::JobManifest {
        job_id: "01JJOBEXECSPECTEST0000001".to_owned(),
        entrypoint: "python".to_owned(),
        args: vec!["train.py".to_owned(), "--epochs".to_owned(), "3".to_owned()],
        ..Default::default()
    }
}

// ── 정상 경로 ──────────────────────────────────────────────────────

#[test]
fn derives_entrypoint_and_args_in_order() {
    let spec =
        derive_execution_spec(&verified_manifest(base_manifest())).expect("유효한 Manifest 다");
    assert_eq!(spec.entrypoint, "python");
    assert_eq!(spec.args, vec!["train.py", "--epochs", "3"]);
    assert_eq!(spec.job_id, "01JJOBEXECSPECTEST0000001");
    assert!(spec.env_vars.is_empty());
}

/// 빈 인자는 정당하다 — 규범이 금지한 적이 없으므로 발명해서 거부하지 않는다.
#[test]
fn empty_argument_is_allowed() {
    let mut manifest = base_manifest();
    manifest.args = vec!["".to_owned(), "x".to_owned()];
    let spec = derive_execution_spec(&verified_manifest(manifest)).expect("빈 인자는 정당하다");
    assert_eq!(spec.args, vec!["", "x"]);
}

/// `entrypoint` 자신은 `args` 에 들어가지 않는다 — 플랫폼별 명령줄 조립은
/// 각 runtime crate 의 몫이고, 여기서 미리 합치면 이중 삽입이 된다.
#[test]
fn entrypoint_is_not_duplicated_into_args() {
    let spec =
        derive_execution_spec(&verified_manifest(base_manifest())).expect("유효한 Manifest 다");
    assert!(
        !spec.args.contains(&"python".to_owned()),
        "entrypoint 가 args 에 중복으로 들어갔다: {:?}",
        spec.args
    );
}

// ── 결정성: env_vars 는 key 바이트 오름차순 ────────────────────────

#[test]
fn env_vars_are_sorted_by_key_bytes() {
    let mut manifest = base_manifest();
    manifest.env_vars = [("ZZZ", "3"), ("AAA", "1"), ("MMM", "2"), ("aaa", "4")]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

    let spec = derive_execution_spec(&verified_manifest(manifest)).expect("유효한 Manifest 다");
    let keys: Vec<&str> = spec.env_vars.keys().map(String::as_str).collect();
    // 대문자가 소문자보다 바이트가 작다 — 사전순이 아니라 바이트순이다.
    assert_eq!(keys, vec!["AAA", "MMM", "ZZZ", "aaa"]);
}

/// protobuf `map` 은 순서가 없다. 같은 내용이면 몇 번을 만들어도 같은
/// 지시가 나와야 한다.
#[test]
fn repeated_derivation_is_identical() {
    let mut manifest = base_manifest();
    manifest.env_vars = [("B", "2"), ("A", "1"), ("C", "3")]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();

    let first =
        derive_execution_spec(&verified_manifest(manifest.clone())).expect("유효한 Manifest 다");
    for _ in 0..8 {
        let again = derive_execution_spec(&verified_manifest(manifest.clone()))
            .expect("유효한 Manifest 다");
        assert_eq!(first, again, "같은 Manifest 가 다른 지시를 만들었다");
    }
}

// ── negative: 식별자 부재는 fail closed ────────────────────────────

#[test]
fn blank_job_id_is_rejected() {
    for blank in ["", "   ", "\t"] {
        let mut manifest = base_manifest();
        manifest.job_id = blank.to_owned();
        assert_eq!(
            derive_execution_spec(&verified_manifest(manifest)),
            Err(ExecutionSpecError::BlankJobId),
            "빈 job_id({blank:?})가 통과했다"
        );
    }
}

#[test]
fn blank_entrypoint_is_rejected() {
    for blank in ["", "   ", "\t\n"] {
        let mut manifest = base_manifest();
        manifest.entrypoint = blank.to_owned();
        assert_eq!(
            derive_execution_spec(&verified_manifest(manifest)),
            Err(ExecutionSpecError::BlankEntrypoint),
            "빈 entrypoint({blank:?})가 통과했다"
        );
    }
}

// ── negative: OS 가 표현할 수 없는 것 ──────────────────────────────

/// NUL 바이트는 Windows 도 POSIX 도 프로세스 인자로 전달할 수 없다.
/// 조용히 잘리면 **의도하지 않은 명령이 실행된다** — fail closed 한다.
#[test]
fn nul_byte_is_rejected_everywhere_it_can_appear() {
    let mut manifest = base_manifest();
    manifest.entrypoint = "py\0thon".to_owned();
    assert_eq!(
        derive_execution_spec(&verified_manifest(manifest)),
        Err(ExecutionSpecError::NulByte {
            field: SpecField::Entrypoint
        })
    );

    let mut manifest = base_manifest();
    manifest.args = vec!["ok".to_owned(), "bad\0arg".to_owned()];
    assert_eq!(
        derive_execution_spec(&verified_manifest(manifest)),
        Err(ExecutionSpecError::NulByte {
            field: SpecField::Arg { index: 1 }
        })
    );

    let mut manifest = base_manifest();
    manifest.job_id = "job\0id".to_owned();
    assert_eq!(
        derive_execution_spec(&verified_manifest(manifest)),
        Err(ExecutionSpecError::NulByte {
            field: SpecField::JobId
        })
    );

    let mut manifest = base_manifest();
    manifest.env_vars = [("KEY".to_owned(), "va\0lue".to_owned())]
        .into_iter()
        .collect();
    assert_eq!(
        derive_execution_spec(&verified_manifest(manifest)),
        Err(ExecutionSpecError::NulByte {
            field: SpecField::EnvVarValue {
                name: "KEY".to_owned()
            }
        })
    );
}

#[test]
fn blank_env_var_name_is_rejected() {
    let mut manifest = base_manifest();
    manifest.env_vars = [("".to_owned(), "v".to_owned())].into_iter().collect();
    assert_eq!(
        derive_execution_spec(&verified_manifest(manifest)),
        Err(ExecutionSpecError::BlankEnvVarName)
    );
}

/// POSIX `environ` 은 `NAME=VALUE` 한 문자열이다. 이름에 `=` 가 있으면
/// 경계가 무너져 다른 변수로 읽힌다.
#[test]
fn env_var_name_with_equals_is_rejected() {
    let mut manifest = base_manifest();
    manifest.env_vars = [("A=B".to_owned(), "v".to_owned())].into_iter().collect();
    assert_eq!(
        derive_execution_spec(&verified_manifest(manifest)),
        Err(ExecutionSpecError::EnvVarNameContainsEquals {
            name: "A=B".to_owned()
        })
    );
}

/// 값에 `=` 가 있는 것은 정당하다 — 첫 `=` 만 경계이므로 값은 자유롭다.
#[test]
fn equals_in_env_var_value_is_allowed() {
    let mut manifest = base_manifest();
    manifest.env_vars = [("FLAGS".to_owned(), "--a=1 --b=2".to_owned())]
        .into_iter()
        .collect();
    let spec = derive_execution_spec(&verified_manifest(manifest)).expect("값의 = 는 정당하다");
    assert_eq!(
        spec.env_vars.get("FLAGS").map(String::as_str),
        Some("--a=1 --b=2")
    );
}

// ── 발명하지 않는다 ────────────────────────────────────────────────

/// 규범은 `entrypoint` 의 경로 형태를 제한하지 않는다. 상대경로·절대경로·
/// 상위 참조를 이 kernel 이 임의로 거부하면 규범에 없는 정책을
/// 발명하는 것이다(`CLAUDE.md` §0.4). 실제 경로 안전은 실행 계층의
/// `artifact_scope`·`open_beneath` 가 맡는다.
#[test]
fn path_shape_is_not_policed_here() {
    for entrypoint in [
        "./run.sh",
        "/usr/bin/python",
        "C:\\Python\\python.exe",
        "../outside",
        "bin/../bin/tool",
    ] {
        let mut manifest = base_manifest();
        manifest.entrypoint = entrypoint.to_owned();
        let spec = derive_execution_spec(&verified_manifest(manifest))
            .unwrap_or_else(|e| panic!("{entrypoint:?} 가 거부됐다: {e}"));
        assert_eq!(spec.entrypoint, entrypoint);
    }
}

/// 인자 개수·길이에도 규범 제한이 없다.
#[test]
fn large_argument_list_is_not_policed_here() {
    let mut manifest = base_manifest();
    manifest.args = (0..500).map(|i| format!("--flag-{i}")).collect();
    let spec = derive_execution_spec(&verified_manifest(manifest)).expect("제한이 없다");
    assert_eq!(spec.args.len(), 500);
}

/// 이 kernel 은 실행 파일이 실제로 있는지 확인하지 않는다 — I/O 는
/// 범위 밖이다. 존재하지 않는 경로도 지시로는 정상 생성된다.
#[test]
fn missing_executable_is_not_detected_here() {
    let mut manifest = base_manifest();
    manifest.entrypoint = "/definitely/not/here/xyzzy".to_owned();
    let spec = derive_execution_spec(&verified_manifest(manifest))
        .expect("존재 확인은 이 kernel 의 일이 아니다");
    assert_eq!(spec.entrypoint, "/definitely/not/here/xyzzy");
}

/// 여러 환경변수가 있어도 원본 map 의 삽입 순서에 의존하지 않는다.
#[test]
fn env_var_insertion_order_does_not_matter() {
    let pairs = [("A", "1"), ("B", "2"), ("C", "3"), ("D", "4")];
    let mut expected: Option<BTreeMap<String, String>> = None;

    for rotation in 0..pairs.len() {
        let mut rotated: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        rotated.rotate_left(rotation);

        let mut manifest = base_manifest();
        manifest.env_vars = rotated.into_iter().collect();
        let spec = derive_execution_spec(&verified_manifest(manifest)).expect("유효하다");

        match &expected {
            None => expected = Some(spec.env_vars),
            Some(first) => assert_eq!(
                first, &spec.env_vars,
                "삽입 순서가 결과를 바꿨다 (rotation={rotation})"
            ),
        }
    }
}
