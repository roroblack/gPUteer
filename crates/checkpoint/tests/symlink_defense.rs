#![cfg(windows)]

use std::fs;
use std::path::Path;
use std::process::Command;

use gputeer_checkpoint::durability::{CheckpointFile, CheckpointManifest, MANIFEST_FILENAME};
use gputeer_checkpoint::writer::find_resume_point;

fn make_junction(link: &Path, target: &Path) {
    let status = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("mklink 프로세스 실행 실패");
    assert!(
        status.success(),
        "mklink /J 실패({link:?} -> {target:?}); 이 테스트는 조용히 건너뛰지 않는다"
    );
}

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

#[test]
fn checkpoint_data_read_through_junction_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let checkpoint = temp.path().join("checkpoint");
    let outside = temp.path().join("outside-data");
    fs::create_dir_all(&checkpoint).unwrap();
    fs::create_dir_all(&outside).unwrap();

    let data = b"outside checkpoint data";
    fs::write(outside.join("shard.bin"), data).unwrap();
    make_junction(&checkpoint.join("payload"), &outside);

    assert_eq!(
        fs::read(checkpoint.join("payload").join("shard.bin")).unwrap(),
        data,
        "junction이 실제로 바깥 파일을 가리켜야 테스트가 유효하다"
    );

    let result = manifest("checkpoint", "payload/shard.bin", data).verify_files(&checkpoint);
    assert!(
        result.is_err(),
        "체크포인트 데이터 읽기가 junction을 따라 바깥 파일을 읽었다"
    );
}

#[test]
fn checkpoint_manifest_read_from_junction_directory_is_rejected() {
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
    make_junction(&linked_checkpoint, &outside);
    assert!(
        fs::read(linked_checkpoint.join(MANIFEST_FILENAME)).is_ok(),
        "평범한 읽기가 junction 매니페스트를 따라가야 테스트가 유효하다"
    );

    assert!(
        find_resume_point(&root).unwrap().is_none(),
        "junction 디렉터리의 매니페스트가 재개 후보로 채택됐다"
    );
}

/// 읽기가 없는 파일을 만들면 안 된다.
///
/// `runtime-windows` 의 `open_beneath` 는 최종 대상에 `OPEN_ALWAYS` 를 써서
/// 없는 파일을 만든다(쓰기 경로용 계약이다). 링크 방어를 붙이면서 그것을
/// 그대로 읽기에 쓰면, "없는 체크포인트를 읽어본다" 는 정상 동작이 빈
/// 파일을 만들어 버린다. 존재 검사를 먼저 하는 우회는 그 자체가 TOCTOU 라
/// 이 방어의 목적과 어긋나므로, 커널이 `OPEN_EXISTING` 으로 판정하게 했다.
#[test]
fn reading_a_missing_file_must_not_create_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let missing = root.join("absent.bin");

    let err =
        gputeer_runtime_windows::open_beneath_read_only(root, std::path::Path::new("absent.bin"))
            .expect_err("없는 파일은 열리면 안 된다");
    assert_eq!(
        err.kind(),
        std::io::ErrorKind::NotFound,
        "없는 파일은 NotFound 여야 한다"
    );

    assert!(
        !missing.exists(),
        "읽기 시도가 파일을 만들었다 — OPEN_ALWAYS 가 읽기 경로에 새어 들어왔다"
    );
}

/// 대조군 — 쓰기용 `open_beneath` 는 여전히 만든다(기존 계약 유지 확인).
#[test]
fn the_write_variant_still_creates_by_contract() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let _f = gputeer_runtime_windows::open_beneath(root, std::path::Path::new("made.bin"))
        .expect("쓰기 변형은 만들어야 한다");
    assert!(
        root.join("made.bin").exists(),
        "쓰기 변형의 기존 계약이 깨졌다"
    );
}
