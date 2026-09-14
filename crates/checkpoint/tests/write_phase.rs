//! 결함 83 (재검수 58) — `write_checkpoint` 실패가 해시 검증 **전**(저장 · 검증)인지 **뒤**(공개)인지 가른다.
//!
//! 뒤의 실패를 앞과 같은 칸에 넣으면, 검증까지 끝난 산출물을 Agent 가 "확정 실패(outcome 6)" 로 보고한다.

use gputeer_checkpoint::writer::{manifest_for, write_checkpoint_phased, WritePhase, POINTER_FILENAME};

fn files() -> Vec<(String, Vec<u8>)> {
    vec![("a.bin".to_string(), b"hello".to_vec())]
}

/// LATEST 자리를 **디렉터리**로 막으면 포인터 교체가 실패한다 — 산출물은 이미 HASH_VERIFIED 다 -> Publish.
#[test]
fn a_blocked_pointer_fails_in_the_publish_phase() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(POINTER_FILENAME)).unwrap();
    let files = files();
    let manifest = manifest_for("ckpt-83", "job-83", "attempt-83", 0, 1, &files);
    let (phase, error) = write_checkpoint_phased(root.path(), &manifest, &files, 0)
        .expect_err("LATEST 가 디렉터리면 교체가 실패한다");
    assert_eq!(phase, WritePhase::Publish, "{error}");
}

/// 체크포인트 디렉터리 자리에 **파일**이 있으면 저장부터 실패한다 -> StoreAndVerify.
#[test]
fn a_blocked_checkpoint_dir_fails_in_the_store_phase() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("ckpt-83"), b"not a directory").unwrap();
    let files = files();
    let manifest = manifest_for("ckpt-83", "job-83", "attempt-83", 0, 1, &files);
    let (phase, error) = write_checkpoint_phased(root.path(), &manifest, &files, 0)
        .expect_err("파일 위에 체크포인트 디렉터리를 만들 수 없다");
    assert_eq!(phase, WritePhase::StoreAndVerify, "{error}");
}

/// 대조 — 막힌 곳이 없으면 성공한다(위 둘이 무조건 실패하는 fixture 가 아님을 확인).
#[test]
fn an_unblocked_write_succeeds() {
    let root = tempfile::tempdir().unwrap();
    let files = files();
    let manifest = manifest_for("ckpt-83", "job-83", "attempt-83", 0, 1, &files);
    write_checkpoint_phased(root.path(), &manifest, &files, 0).expect("막힌 곳이 없으면 확정한다");
}

/// 결함 90 (재검수 59) — 파일 검증은 성공하고 **검증 상태 마커** 기록만 실패하면 Publish 다(산출물은 검증됐다).
#[test]
fn a_blocked_hash_verified_marker_fails_after_verification() {
    use gputeer_checkpoint::durability::{state_marker_name, DurabilityState};
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("ckpt-83");
    std::fs::create_dir_all(dir.join(state_marker_name(DurabilityState::HashVerified))).unwrap();
    let files = files();
    let manifest = manifest_for("ckpt-83", "job-83", "attempt-83", 0, 1, &files);
    let (phase, error) = write_checkpoint_phased(root.path(), &manifest, &files, 0)
        .expect_err("검증 상태 마커 자리가 디렉터리면 마커 기록이 실패한다");
    assert_eq!(phase, WritePhase::Publish, "{error}");
}
