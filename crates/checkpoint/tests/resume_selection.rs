//! `find_resume_point` 의 후보 선택 — 독립 검수(2026-08-16 3차) 지적.
//!
//! # 무엇이 문제였나
//!
//! `find_resume_point` 는 `root` 아래 **모든** 디렉터리를 후보로 삼고
//! 해시 검증만 통과하면 `step` 이 가장 큰 것을 고른다.
//!
//! ```text
//! W-1  job_id / attempt_id 를 거르지 않는다
//!      => **다른 job 의 체크포인트에서 재개한다**
//!
//! W-2  files 가 빈 매니페스트도 통과한다
//!      => 데이터가 하나도 없는 "체크포인트" 에서 재개한다
//!
//! W-3  checkpoint_id 와 디렉터리 이름이 같은지 안 본다
//!      => 반환된 id 로 파일을 찾으면 엉뚱한 곳을 본다
//! ```
//!
//! ★ 셋 다 **해시 검증을 통과한다.** "해시가 맞으면 안전하다" 가 아니다 —
//!   해시는 *그 파일이 그 매니페스트의 것*임을 보장할 뿐,
//!   *그 매니페스트가 내 것*임은 보장하지 않는다.

use std::path::{Path, PathBuf};

use gputeer_checkpoint::writer::{find_resume_point, manifest_for, write_checkpoint};
use gputeer_checkpoint::CheckpointManifest;

const JOB: &str = "01JBXR7Q0000000000000000AA";
const ATT: &str = "01JBXATT00000000000000001";

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gputeer-resume-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn put(root: &Path, id: &str, job: &str, att: &str, step: u64, files: &[(String, Vec<u8>)]) {
    let mut m = manifest_for(id, job, att, step, 1, files);
    m.created_at_unix_ms = 1_755_200_000_000 + step;
    write_checkpoint(root, &m, files, 0).expect("체크포인트 기록");
}

fn f(name: &str, data: &str) -> (String, Vec<u8>) {
    (name.to_string(), data.as_bytes().to_vec())
}

// ══════════════════════════════════════════════════════════════════
// W-1 ★ 다른 job 의 체크포인트를 고르는가
// ══════════════════════════════════════════════════════════════════

/// **가장 위험한 결함이다.**
///
/// 재개는 "이 job 의 이 attempt 를 이어간다" 는 동작인데,
/// 남의 job 체크포인트에서 이어가면 **완전히 다른 학습 상태를 로드한다.**
#[test]
fn w1_resume_must_not_pick_another_job() {
    let root = tmpdir("w1");

    // 내 job 은 step 10
    put(&root, "ckpt-mine", JOB, ATT, 10, &[f("a.bin", "mine")]);
    // 남의 job 이 step 100 — 같은 root 에 있다
    put(
        &root,
        "ckpt-other",
        "01JBXOTHER00000000000000B",
        ATT,
        100,
        &[f("a.bin", "other")],
    );

    let picked = find_resume_point_for(&root, JOB, ATT)
        .expect("검색 실패")
        .expect("후보가 있어야 한다");

    assert_eq!(
        picked.job_id, JOB,
        "★ 다른 job 의 체크포인트에서 재개하려 한다 — 완전히 다른 학습 상태를 로드한다"
    );
    assert_eq!(picked.step, 10);
}

/// attempt 도 걸러야 한다.
///
/// 같은 job 의 다른 attempt 는 **fencing 으로 무효화된 실행**일 수 있다.
#[test]
fn w1b_resume_must_not_pick_another_attempt() {
    let root = tmpdir("w1b");
    put(&root, "ckpt-mine", JOB, ATT, 10, &[f("a.bin", "mine")]);
    put(
        &root,
        "ckpt-stale",
        JOB,
        "01JBXATT00000000000000999",
        100,
        &[f("a.bin", "stale")],
    );

    let picked = find_resume_point_for(&root, JOB, ATT).unwrap().unwrap();
    assert_eq!(
        picked.attempt_id, ATT,
        "★ 다른 attempt 의 체크포인트를 골랐다 — fencing 으로 무효화된 실행일 수 있다"
    );
}

// ══════════════════════════════════════════════════════════════════
// W-2 ★ 빈 매니페스트
// ══════════════════════════════════════════════════════════════════

/// `files: []` 인 매니페스트는 `verify_files` 의 반복문을 **0회 돌고 통과**한다.
///
/// 데이터가 하나도 없는데 "가장 최신 유효 체크포인트" 가 된다.
#[test]
fn w2_empty_manifest_must_not_be_a_resume_candidate() {
    let root = tmpdir("w2");
    put(&root, "ckpt-real", JOB, ATT, 10, &[f("a.bin", "real")]);
    put(&root, "ckpt-empty", JOB, ATT, 100, &[]); // 파일 0개

    let picked = find_resume_point_for(&root, JOB, ATT).unwrap().unwrap();
    assert_eq!(
        picked.checkpoint_id, "ckpt-real",
        "★ 파일이 하나도 없는 매니페스트를 재개 후보로 골랐다 (id={})",
        picked.checkpoint_id
    );
}

// ══════════════════════════════════════════════════════════════════
// W-3 ★ checkpoint_id 와 디렉터리 이름 불일치
// ══════════════════════════════════════════════════════════════════

/// 매니페스트의 `checkpoint_id` 가 디렉터리 이름과 다르면,
/// 반환된 id 로 파일을 찾는 호출자는 **엉뚱한 곳을 본다.**
#[test]
fn w3_manifest_id_must_match_directory_name() {
    let root = tmpdir("w3");
    put(&root, "ckpt-good", JOB, ATT, 10, &[f("a.bin", "good")]);

    // 디렉터리 이름과 다른 id 를 가진 매니페스트를 심는다
    let bogus_dir = root.join("dir-A");
    std::fs::create_dir_all(&bogus_dir).unwrap();
    let files = [f("a.bin", "x")];
    let mut m = manifest_for("dir-B", JOB, ATT, 100, 1, &files);
    m.created_at_unix_ms = 1;
    std::fs::write(bogus_dir.join("a.bin"), b"x").unwrap();
    std::fs::write(bogus_dir.join("manifest.json"), m.to_json().unwrap()).unwrap();

    let picked = find_resume_point_for(&root, JOB, ATT).unwrap().unwrap();
    assert_eq!(
        picked.checkpoint_id, "ckpt-good",
        "★ checkpoint_id({}) 와 디렉터리 이름이 다른 매니페스트를 골랐다 — \
         반환된 id 로 파일을 찾으면 엉뚱한 곳을 본다",
        picked.checkpoint_id
    );
}

// ══════════════════════════════════════════════════════════════════
// 비공허성 — 정상 경로는 여전히 동작하는가
// ══════════════════════════════════════════════════════════════════

#[test]
fn normal_resume_still_picks_highest_step() {
    let root = tmpdir("normal");
    for step in [10u64, 30, 20] {
        put(
            &root,
            &format!("ckpt-{step:08}"),
            JOB,
            ATT,
            step,
            &[f("a.bin", &format!("s{step}"))],
        );
    }
    let picked = find_resume_point_for(&root, JOB, ATT).unwrap().unwrap();
    assert_eq!(picked.step, 30, "가장 높은 step 을 고르지 않았다");
}

#[test]
fn no_candidates_returns_none() {
    let root = tmpdir("none");
    assert!(find_resume_point_for(&root, JOB, ATT).unwrap().is_none());
}

/// 필터를 걸지 않는 옛 API 는 여전히 존재하는가 — 있으면 **오용 위험**이다.
///
/// 이 테스트는 옛 API 가 어떻게 동작하는지 고정한다.
#[test]
fn unfiltered_api_documents_its_danger() {
    let root = tmpdir("unfiltered");
    put(&root, "ckpt-mine", JOB, ATT, 10, &[f("a.bin", "mine")]);
    put(
        &root,
        "ckpt-other",
        "OTHER",
        "OTHER",
        100,
        &[f("a.bin", "other")],
    );

    // 필터 없는 버전은 남의 것도 고른다 — 그것이 이 API 의 의미다
    let any = find_resume_point(&root).unwrap().unwrap();
    assert_eq!(
        any.step, 100,
        "필터 없는 find_resume_point 의 동작이 바뀌었다"
    );
    assert_ne!(
        any.job_id, JOB,
        "★ 이 API 는 job 을 거르지 않는다 — 호출자가 반드시 걸러야 한다"
    );
}

// ── 필터 있는 API (아래 구현이 없으면 컴파일 실패한다) ──────────────

fn find_resume_point_for(
    root: &Path,
    job_id: &str,
    attempt_id: &str,
) -> Result<Option<CheckpointManifest>, gputeer_checkpoint::CheckpointError> {
    gputeer_checkpoint::writer::find_resume_point_for(root, job_id, attempt_id)
}
