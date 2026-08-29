//! Linux `openat2` 링크 방어 실측 — `symlink_defense.rs`(Windows junction)의
//! 대응판이다.
//!
//! Windows 는 junction 으로, Linux 는 symlink 로 같은 공격을 만든다.
//! **두 플랫폼의 통과를 서로 세지 않는다**(`CLAUDE.md` §4) — 그래서 파일을
//! 나누고 각자 실제로 그 OS 에서 돌게 한다.

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use gputeer_checkpoint::durability::{CheckpointFile, CheckpointManifest, MANIFEST_FILENAME};
use gputeer_checkpoint::platform::CHECKPOINT_READ_LINK_DEFENSE_ACTIVE;
use gputeer_checkpoint::writer::find_resume_point;

fn manifest(checkpoint_id: &str, path: &str, data: &[u8]) -> CheckpointManifest {
    CheckpointManifest {
        schema_version: 1,
        checkpoint_id: checkpoint_id.to_string(),
        job_id: "job-link-defense".to_string(),
        attempt_id: "attempt-1".to_string(),
        step: 1,
        files: vec![CheckpointFile {
            path: path.to_string(),
            digest: blake3::hash(data).to_hex().to_string(),
            size_bytes: data.len() as u64,
        }],
        root_digest: "root-digest-not-used-by-this-test".to_string(),
        total_bytes: data.len() as u64,
        created_at_unix_ms: 0,
        producer_node_id: "node-1".to_string(),
        fence_epoch: 1,
    }
}

/// symlink 가 실제로 바깥을 가리키는지 먼저 확인한다.
///
/// 이 확인이 없으면 "방어가 막았다" 와 "애초에 symlink 가 안 만들어졌다"
/// 를 구분할 수 없어 테스트가 공허해진다.
fn assert_link_actually_escapes(link: &Path, expected: &[u8]) {
    let read = fs::read(link).expect("symlink 가 바깥 파일을 가리켜야 테스트가 유효하다");
    assert_eq!(
        read, expected,
        "symlink 가 기대한 바깥 파일을 가리키지 않는다 — 테스트 전제가 깨졌다"
    );
}

#[test]
fn defense_constant_is_active_on_linux() {
    assert!(
        CHECKPOINT_READ_LINK_DEFENSE_ACTIVE,
        "Linux 에서 링크 방어가 꺼져 있다고 보고된다 — 배선과 상수가 어긋났다"
    );
}

/// 데이터 파일 경로에 낀 symlink 디렉터리를 통과해 바깥을 읽으면 안 된다.
#[test]
fn checkpoint_data_read_through_symlink_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let checkpoint = temp.path().join("checkpoint");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&checkpoint).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let data = b"outside payload";
    fs::write(outside.join("shard.bin"), data).unwrap();
    symlink(&outside, checkpoint.join("payload")).unwrap();

    assert_link_actually_escapes(&checkpoint.join("payload").join("shard.bin"), data);

    let result = manifest("checkpoint", "payload/shard.bin", data).verify_files(&checkpoint);
    assert!(
        result.is_err(),
        "체크포인트 데이터 읽기가 symlink 를 따라 바깥 파일을 읽었다"
    );
}

/// 매니페스트 자체가 symlink 디렉터리 안에 있으면 재개 후보가 되면 안 된다.
#[test]
fn checkpoint_manifest_read_from_symlink_directory_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let outside = temp.path().join("outside-checkpoint");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let data = b"outside checkpoint data";
    fs::write(outside.join("shard.bin"), data).unwrap();
    let manifest = manifest("linked-checkpoint", "shard.bin", data);
    fs::write(outside.join(MANIFEST_FILENAME), manifest.to_json().unwrap()).unwrap();

    let linked_checkpoint = root.join("linked-checkpoint");
    symlink(&outside, &linked_checkpoint).unwrap();
    assert!(
        fs::read(linked_checkpoint.join(MANIFEST_FILENAME)).is_ok(),
        "평범한 읽기가 symlink 매니페스트를 따라가야 테스트가 유효하다"
    );

    assert!(
        find_resume_point(&root).unwrap().is_none(),
        "symlink 디렉터리의 매니페스트가 재개 후보로 채택됐다"
    );
}

/// 마지막 구성요소가 symlink 인 경우도 막아야 한다.
///
/// 디렉터리 symlink 만 막고 파일 symlink 를 통과시키면 방어가 절반이다.
#[test]
fn final_component_symlink_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let checkpoint = temp.path().join("checkpoint");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&checkpoint).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let data = b"outside file via file symlink";
    let target = outside.join("real.bin");
    fs::write(&target, data).unwrap();
    symlink(&target, checkpoint.join("shard.bin")).unwrap();

    assert_link_actually_escapes(&checkpoint.join("shard.bin"), data);

    let result = manifest("checkpoint", "shard.bin", data).verify_files(&checkpoint);
    assert!(
        result.is_err(),
        "마지막 구성요소가 파일 symlink 인데 읽혔다"
    );
}

/// symlink 가 아니라 정상 파일이면 당연히 읽혀야 한다.
///
/// 이 테스트가 없으면 "전부 거부" 로도 위 테스트들이 통과해 방어가
/// 공허해진다.
#[test]
fn ordinary_file_beneath_root_is_still_readable() {
    let temp = tempfile::tempdir().unwrap();
    let checkpoint = temp.path().join("checkpoint");
    fs::create_dir_all(checkpoint.join("payload")).unwrap();

    let data = b"ordinary payload";
    fs::write(checkpoint.join("payload").join("shard.bin"), data).unwrap();

    manifest("checkpoint", "payload/shard.bin", data)
        .verify_files(&checkpoint)
        .expect("symlink 가 아닌 정상 파일은 읽혀야 한다");
}

/// 읽기가 없는 파일을 만들면 안 된다.
///
/// Windows 쪽은 `OPEN_ALWAYS` 때문에 이 위험이 실재해서 별도 방어가
/// 필요했다(`symlink_defense.rs` 참조). Linux `openat2` 는 `O_CREAT` 를
/// 넘기지 않으므로 구조적으로 안전하지만, **구조가 안전하다는 주장도
/// 테스트로 고정한다.**
#[test]
fn reading_a_missing_file_must_not_create_it() {
    let temp = tempfile::tempdir().unwrap();
    let checkpoint = temp.path().join("checkpoint");
    fs::create_dir_all(&checkpoint).unwrap();

    let missing = checkpoint.join("absent.bin");
    let data = b"never written";
    let result = manifest("checkpoint", "absent.bin", data).verify_files(&checkpoint);
    assert!(result.is_err(), "없는 파일이 검증을 통과했다");

    assert!(
        !missing.exists(),
        "읽기 시도가 파일을 만들었다 — O_CREAT 가 읽기 경로에 새어 들어왔다"
    );
}
