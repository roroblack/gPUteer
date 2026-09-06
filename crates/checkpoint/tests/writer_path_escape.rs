//! `write_checkpoint()` 가 매니페스트의 `checkpoint_id` 로 루트 밖에
//! 디렉터리를 만들지 못하게 한다.
//!
//! # 왜 이 파일이 생겼나 (2026-09-06)
//!
//! `writer.rs` 가 `root.join(&manifest.checkpoint_id)` 를 **검증 없이**
//! 하고 있었다. 매니페스트는 `CLAUDE.md` §0 이 말하는 **외부 입력**이다 —
//! 서명자가 악의적일 수 있다. `checkpoint_id` 가 `"../evil"` 이면
//! 체크포인트 루트 **밖**에 디렉터리와 데이터 파일이 생긴다.
//!
//! ★ 이 함수는 **테스트 전용이 아니다** — `crates/agent/src/lib.rs` 의
//!   실행 경로가 실제로 부른다.
//!
//! ★★ `atomic.rs` 의 `validate_relative_name()` 은 **파일 이름**에만
//!   적용되고 디렉터리 이름에는 안 걸렸다. 같은 크레이트의
//!   `commit.rs::validate_checkpoint_id()` 는 이미 막고 있었다 — 즉
//!   **막는 문 하나와 안 막는 문 하나**가 있었고, 그 상태가 가장 위험하다.
//!   막힌다고 믿으면서 안 막힌 쪽으로 들어간다.
//!
//! # 이 테스트가 공허하지 않게 하려고 한 것 셋
//!
//! 1. **오류 변형을 타입으로 단언한다.** 문자열을 뒤지지 않는다 — 문구가
//!    바뀌면 조용히 통과하는 테스트가 되고, 그게 이 저장소가 반복해 데인
//!    `!ok` 트랩이다.
//! 2. **루트 밖에 아무것도 안 생겼는지 파일시스템에서 직접 본다.**
//!    거부하면서 디렉터리는 만들어 버리는 구현이 있을 수 있다.
//! 3. **통과 대조를 같이 둔다.** 없으면 "전부 거부하는" 고장난 구현도
//!    위 전부를 통과한다.

use std::path::{Path, PathBuf};

use gputeer_checkpoint::writer::{manifest_for, write_checkpoint};
use gputeer_checkpoint::{CheckpointError, CheckpointManifest};

const JOB: &str = "job-path-escape";
const ATTEMPT: &str = "attempt-path-escape";

fn manifest_with_id(id: &str, files: &[(String, Vec<u8>)]) -> CheckpointManifest {
    let mut manifest = manifest_for(id, JOB, ATTEMPT, 1, 1, files);
    manifest.created_at_unix_ms = 1;
    manifest
}

fn payload() -> Vec<(String, Vec<u8>)> {
    vec![("shard-0.bin".to_string(), b"payload".to_vec())]
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gputeer-path-escape-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("임시 디렉터리");
    dir
}

/// `temp/ckpt-root` 와 그 **형제** `temp/sibling` 을 만든다.
///
/// 형제가 있어야 "밖으로 나갔다" 를 관측할 자리가 생긴다 — 루트만 있으면
/// 탈출한 파일이 어디로 갔는지 볼 곳이 없다.
fn fixture(temp: &Path) -> PathBuf {
    let root = temp.join("ckpt-root");
    std::fs::create_dir_all(&root).expect("루트 생성");
    std::fs::create_dir_all(temp.join("sibling")).expect("형제 생성");
    root
}

/// 루트 **아래** 항목 이름 목록. 거부됐다면 비어 있어야 한다.
fn entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("디렉터리 읽기")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

/// 거부 한 건을 끝까지 확인한다.
///
/// 오류 **변형**과 이유를 보고, 그다음 파일시스템을 본다. 둘 다 봐야
/// "거부는 했는데 흔적은 남기는" 구현을 거를 수 있다.
fn assert_refused(tag: &str, bad_id: &str, expect_reason_contains: &str) {
    let temp = temp_dir(tag);
    let root = fixture(&temp);
    let files = payload();

    let error = write_checkpoint(&root, &manifest_with_id(bad_id, &files), &files, 0)
        .expect_err("루트 밖으로 나가는 checkpoint_id 인데 받아들였다");

    // (1) 정확한 변형과 이유. 문자열 뒤지기가 아니라 타입으로 가른다.
    match &error {
        CheckpointError::UnsafePath { name, reason } => {
            assert_eq!(name, bad_id, "거부는 했는데 다른 값을 신고했다");
            assert!(
                reason.contains(expect_reason_contains),
                "다른 규칙에 걸렸다 — 의도한 관문이 아니다: {reason:?}"
            );
        }
        other => panic!("UnsafePath 가 아닌 다른 이유로 실패했다: {other}"),
    }

    // (2) 루트 밖. 이게 진짜 확인해야 할 것이다.
    let escaped = temp.join("sibling");
    assert_eq!(
        entries(&escaped),
        Vec::<String>::new(),
        "거부했는데 루트 밖 형제 디렉터리에 무언가 생겼다"
    );
    // 상위 임시 폴더에도 루트·형제 말고는 아무것도 없어야 한다.
    let mut top = entries(&temp);
    top.sort();
    assert_eq!(
        top,
        vec!["ckpt-root".to_string(), "sibling".to_string()],
        "거부했는데 루트의 형제 자리에 무언가 생겼다"
    );

    // (3) 루트 **안**에도 흔적이 없어야 한다.
    assert_eq!(
        entries(&root),
        Vec::<String>::new(),
        "거부했는데 루트 안에 디렉터리가 생겼다 — 검사가 create_dir_all 뒤에 있다"
    );

    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn a_parent_traversal_is_refused() {
    // 루트 밖 형제를 정면으로 노린다.
    assert_refused("traversal", "../escape", "경로 구분자");
}

#[test]
fn a_bare_parent_component_is_refused() {
    // ★ 구분자가 없어서 위 검사에 안 걸린다. 그런데 `root.join("..")` 은
    //   루트의 **부모**가 되므로 그 자체로 탈출이다.
    assert_refused("dotdot", "..", "상위/현재 디렉터리 성분");
}

#[test]
fn a_path_separator_is_refused() {
    // 밖으로 나가지 않아도 하위 디렉터리를 만들어 이름공간을 어지럽힌다.
    assert_refused("separator", "a/b", "경로 구분자");
}

#[test]
fn an_empty_checkpoint_id_is_refused() {
    // ★ `root.join("")` 은 **루트 자신**이다. 그러면 데이터 파일이 루트에
    //   직접 쏟아지고, GC 가 보는 "체크포인트 하나 = 디렉터리 하나" 전제가
    //   무너진다.
    assert_refused("empty", "", "빈 이름");
}

#[test]
fn a_backslash_separator_is_refused_too() {
    // Windows 구분자도 같이 막는다 — 한쪽만 막으면 플랫폼에 따라 뚫린다.
    assert_refused("backslash", "a\\b", "경로 구분자");
}

#[test]
fn the_ordinary_checkpoint_id_shape_still_works() {
    // ★★ **대조군.** 이게 없으면 "전부 거부하는" 구현도 위 다섯을 통과한다.
    //   실제 형식(`ckpt-00000100`)을 그대로 쓴다.
    let temp = temp_dir("ok");
    let root = fixture(&temp);
    let files = payload();

    let dir = write_checkpoint(&root, &manifest_with_id("ckpt-00000100", &files), &files, 0)
        .expect("평범한 checkpoint_id 를 거부했다");

    assert!(dir.starts_with(&root), "루트 안에 안 생겼다: {dir:?}");
    assert!(dir.join("shard-0.bin").exists(), "데이터 파일이 없다");

    let _ = std::fs::remove_dir_all(&temp);
}
