//! `write_checkpoint()` 가 매니페스트의 `checkpoint_id` 로 루트 밖에
//! 디렉터리를 만들지 못하게 한다.
//!
//! # 왜 이 파일이 생겼나 (2026-09-06)
//!
//! `writer.rs` 가 `root.join(&manifest.checkpoint_id)` 를 **검증 없이**
//! 하고 있었다. 매니페스트는 바깥에서 온 값이므로 `checkpoint_id` 가
//! `"../evil"` 이면 체크포인트 루트 밖에 디렉터리가 생긴다.
//!
//! ★ 이 함수는 **테스트 전용이 아니다** — `crates/agent/src/lib.rs` 의
//!   실행 경로가 실제로 부른다.
//!
//! ★★ 같은 날 신설한 `StagedCheckpoint` 는 같은 검사를 이미 하고 있었다.
//!   그래서 저장소에 **막는 문 하나와 안 막는 문 하나**가 생겼다. 그
//!   상태가 가장 위험하다 — 막힌다고 믿으면서 안 막힌 쪽으로 들어간다.
//!
//! # 이 테스트가 공허하지 않게 하려고 한 것
//!
//! 실패했다는 것만 보지 않는다. **루트 밖에 아무것도 안 생겼는지**를
//! 파일시스템에서 직접 확인한다. 거부 이유만 보고 넘어가면, 거부하면서도
//! 디렉터리는 만들어 버리는 구현이 통과한다.

use std::path::Path;

use gputeer_checkpoint::writer::{manifest_for, write_checkpoint};
use gputeer_checkpoint::CheckpointManifest;

const JOB: &str = "job-path-escape";
const ATTEMPT: &str = "attempt-path-escape";

fn manifest_with_id(id: &str, files: &[(String, Vec<u8>)]) -> CheckpointManifest {
    let mut manifest = manifest_for(id, JOB, ATTEMPT, 1, 1, files);
    manifest.created_at_unix_ms = 1;
    manifest
}

/// 루트 아래에 `root/` 와 형제 `sibling/` 을 만들고 `root` 를 돌려준다.
///
/// 형제 디렉터리가 있어야 "밖으로 나갔다" 를 관측할 수 있다 — 루트만
/// 있으면 탈출한 파일이 어디로 갔는지 볼 자리가 없다.
fn fixture(temp: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = temp.join("ckpt-root");
    let sibling = temp.join("sibling");
    std::fs::create_dir_all(&root).expect("루트 생성");
    std::fs::create_dir_all(&sibling).expect("형제 생성");
    (root, sibling)
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
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

#[test]
fn a_parent_traversal_in_checkpoint_id_is_refused_and_nothing_is_created_outside_the_root() {
    let temp = temp_dir("parent");
    let (root, _sibling) = fixture(&temp);
    let files = vec![("shard-0.bin".to_string(), b"payload".to_vec())];

    // `..` 로 루트 밖 형제 디렉터리를 노린다.
    let manifest = manifest_with_id("../sibling/stolen", &files);
    let result = write_checkpoint(&root, &manifest, &files, 0);

    // 1) 거부해야 한다. **그리고 이유가 checkpoint_id 여야 한다** —
    //    엉뚱한 관문(예: 파일 쓰기 실패)에 걸려도 통과하면 이 테스트는
    //    아무것도 재지 않는다.
    let error = result.expect_err("루트 밖 경로인데 받아들였다");
    let text = error.to_string();
    assert!(
        text.contains("checkpoint_id"),
        "다른 이유로 실패했다 — 경로 검사에 걸린 것이 아니다: {text}"
    );

    // 2) ★ 이게 핵심이다. 거부만 하고 디렉터리는 만들어 버리는 구현이
    //    있을 수 있으므로 파일시스템을 직접 본다.
    let escaped = temp.join("sibling").join("stolen");
    assert!(
        !escaped.exists(),
        "거부는 했는데 루트 밖에 디렉터리가 생겼다: {escaped:?}"
    );

    // 3) 루트 안에도 아무것도 안 생겨야 한다.
    let inside: Vec<_> = std::fs::read_dir(&root)
        .expect("루트 읽기")
        .filter_map(Result::ok)
        .map(|e| e.file_name())
        .collect();
    assert!(
        inside.is_empty(),
        "거부했는데 루트 안에 무언가 생겼다: {inside:?}"
    );

    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn a_path_separator_in_checkpoint_id_is_refused() {
    let temp = temp_dir("sep");
    let (root, _sibling) = fixture(&temp);
    let files = vec![("shard-0.bin".to_string(), b"payload".to_vec())];

    // 구분자만으로도 하위 디렉터리를 만들어 이름공간을 어지럽힌다.
    let manifest = manifest_with_id("nested/inner", &files);
    let error = write_checkpoint(&root, &manifest, &files, 0)
        .expect_err("경로 구분자가 든 id 인데 받아들였다");
    assert!(
        error.to_string().contains("checkpoint_id"),
        "다른 이유로 실패했다: {error}"
    );
    assert!(
        !root.join("nested").exists(),
        "거부했는데 중간 디렉터리가 생겼다"
    );

    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn an_absolute_looking_checkpoint_id_is_refused() {
    let temp = temp_dir("abs");
    let (root, _sibling) = fixture(&temp);
    let files = vec![("shard-0.bin".to_string(), b"payload".to_vec())];

    // ★ Windows 에서 `root.join("C:\\evil")` 은 루트를 **통째로 버리고**
    //   절대 경로가 된다. `..` 보다 조용하고 더 위험하다.
    let manifest = manifest_with_id("C:\\evil", &files);
    let error =
        write_checkpoint(&root, &manifest, &files, 0).expect_err("절대 경로 같은 id 를 받아들였다");
    assert!(
        error.to_string().contains("checkpoint_id"),
        "다른 이유로 실패했다: {error}"
    );

    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn an_ordinary_checkpoint_id_still_works() {
    // ★ 대조. 이게 없으면 "전부 거부하는" 구현도 위 셋을 통과한다 —
    //   그러면 검사가 아니라 고장이다.
    let temp = temp_dir("ok");
    let (root, _sibling) = fixture(&temp);
    let files = vec![("shard-0.bin".to_string(), b"payload".to_vec())];

    let manifest = manifest_with_id("ckpt-normal-0001", &files);
    let dir = write_checkpoint(&root, &manifest, &files, 0).expect("평범한 id 를 거부했다");

    assert!(dir.starts_with(&root), "루트 안에 안 생겼다: {dir:?}");
    assert!(dir.join("shard-0.bin").exists(), "데이터 파일이 없다");

    let _ = std::fs::remove_dir_all(&temp);
}
