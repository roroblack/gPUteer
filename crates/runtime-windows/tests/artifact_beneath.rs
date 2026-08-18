//! `open_beneath` 가 실제로 TOCTOU 를 막는지 실측한다.
//!
//! `crates/runtime-policy/src/artifact.rs::ArtifactPolicy::check()` 는
//! 경로 문자열만 본다 — 검사 통과 직후 그 경로(또는 경로 위의 중간
//! 디렉터리)를 symlink/junction 으로 바꿔치기하면 그 검사는 무력하다.
//! 이 테스트는 실제로 그 바꿔치기를 만들어 `open_beneath` 가 잡는지
//! 확인한다.
//!
//! ★ symlink 대신 **junction** 을 쓴다 — 이 개발 기계에서 실측한
//! 결과(2026-08-18), `New-Item -ItemType SymbolicLink` 는
//! "Administrator privilege required for this operation" 로 실패했지만
//! `New-Item -ItemType Junction` 은 승격 없이 성공했다. junction 도
//! symlink 와 마찬가지로 `FILE_ATTRIBUTE_REPARSE_POINT` 를 갖는
//! 디렉터리 reparse point이므로, `open_beneath` 의 방어 대상과
//! 정확히 일치한다 — 이 저장소가 실제로 CI 에서 승격 없이 돌릴 수
//! 있는 유일한 reparse-point 종류다.

use std::fs;
use std::path::Path;
use std::process::Command;

use gputeer_runtime_windows::open_beneath;

/// `mklink /J` 로 디렉터리 junction 을 만든다.
///
/// ★ 2026-08-18 정정(코덱스 독립 검수 · `p62` 프롬프트). 초안은 이
/// 실패하면 조용히 `return` 해서 테스트를 "통과"로 보고했다 —
/// junction 생성이 이 환경에서 막히면(그룹 정책 등) 이 테스트는
/// **아무것도 검증하지 않고도 초록불**을 켰다. 이 개발 기계에서는
/// junction 생성이 승격 없이 성공함을 이미 실측으로 확인했으므로
/// (모듈 문서 참조), 실패는 "정상적으로 건너뛸 상황"이 아니라
/// **panic 으로 크게 알려야 할 이례적인 상황**이다 — 조용한 거짓
/// 통과보다 시끄러운 실패가 낫다.
fn make_junction(link: &Path, target: &Path) {
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("mklink 프로세스 스폰 실패");
    assert!(
        status.success(),
        "mklink /J 실패({link:?} -> {target:?}) — 이 개발 기계에서는 \
         승격 없이 성공함을 실측으로 확인했다(모듈 문서 참조). 실패했다면 \
         환경이 바뀐 것이니 원인을 확인해야 한다 — 조용히 건너뛰지 않는다"
    );
}

/// ★ 핵심 실측 — allowed 디렉터리 안의 junction 이 허용 영역 밖을
/// 가리켜도, `ArtifactPolicy::check()` 는 문자열만 보므로 통과시킨다
/// (아래에서 비공허성으로 재확인). 그러나 `open_beneath` 는 junction
/// 자체를 발견해 거부해야 한다.
#[test]
fn junction_inside_allowed_dir_pointing_outside_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let allowed = root.join("allowed");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&allowed).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let marker = outside.join("marker.txt");
    fs::write(&marker, b"original").unwrap();

    let junction = allowed.join("escape");
    make_junction(&junction, &outside);

    // ★ 비공허성 — 문자열 검사(ArtifactPolicy)가 이 경로를 실제로
    //   "허용"으로 판정한다는 것을 먼저 확인한다. 그러지 않으면 아래
    //   open_beneath 거부가 "애초에 문자열 검사가 다 걸렀다"는 우연일
    //   수 있다 — 이 테스트 전체가 증명하려는 것은 "문자열 검사만으로는
    //   부족하다" 는 것이다.
    let policy = gputeer_runtime_policy::ArtifactPolicy::new(vec!["allowed".into()]);
    assert!(
        policy.check("allowed/escape/marker.txt").is_ok(),
        "문자열 검사가 이미 이 경로를 거부하면 이 테스트는 open_beneath 를 시험하지 못한다"
    );

    let result = open_beneath(&root, Path::new("allowed/escape/marker.txt"));
    assert!(
        result.is_err(),
        "junction 을 통한 허용 영역 밖 접근이 open_beneath 를 통과했다"
    );

    assert_eq!(
        fs::read(&marker).unwrap(),
        b"original",
        "junction 을 거부했어야 하는데 실제로 바깥 파일에 닿았다"
    );
}

/// junction 이 최종 파일 자체가 아니라 **중간 디렉터리**인 경우도
/// 잡아야 한다 — 위 테스트와 다른 코드 경로(루프 안의 중간 컴포넌트
/// 검사 vs. 마지막 컴포넌트 검사)의 회귀를 막는다.
#[test]
fn junction_as_intermediate_directory_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let allowed = root.join("allowed");
    let outside = temp.path().join("outside_dir");
    fs::create_dir_all(&allowed).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::create_dir_all(outside.join("sub")).unwrap();

    let junction = allowed.join("link_to_outside");
    make_junction(&junction, &outside);

    let result = open_beneath(&root, Path::new("allowed/link_to_outside/sub/file.txt"));
    assert!(result.is_err(), "중간 디렉터리 junction 을 거부하지 못했다");
}

/// 비공허성 — reparse point 가 전혀 없는 정상 경로는 실제로 열려야
/// 한다. 이게 없으면 위 거부 테스트들이 "그냥 모든 걸 거부하는 결함"
/// 때문에 우연히 통과하는 것과 구분이 안 된다.
#[test]
fn normal_path_without_reparse_point_opens_successfully() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let allowed = root.join("allowed");
    fs::create_dir_all(&allowed).unwrap();

    let result = open_beneath(&root, Path::new("allowed/output.txt"));
    assert!(result.is_ok(), "정상 경로가 거부됐다: {:?}", result.err());
}
