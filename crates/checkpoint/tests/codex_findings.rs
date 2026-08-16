//! 코덱스 독립 검토(2026-08-16)가 `crates/checkpoint` 에서 지적한 결함의 실측 확인.
//!
//! `CLAUDE.md` §4 — "중요한 판단은 독립 검수자와 교차검증한다."
//!
//! ★ 지적을 액면 그대로 받지 않는다. 각각이 실제로 성립하는지 확인하고
//!   성립하는 것만 고친다.
//!
//! ```text
//! K-1  write_once 가 "content-addressed 이름" 을 **전제**하는데
//!      실제 이름은 shard-N.bin 이다. 전제가 지켜지지 않는다.
//!      => 이름이 같고 내용이 다른 파일을 조용히 받아들인다
//!
//! K-2  RetryPolicy { max_attempts: 0 } 이면 panic 한다.
//!      ADR-026 의 "최종 실패는 명시적 오류" 계약 위반
//!
//! K-3  rename 성공 후 sync_dir 실패 시 API 는 실패를 반환하는데
//!      **파일 상태는 이미 바뀌어 있다**
//! ```

use std::path::Path;

use gputeer_checkpoint::atomic::{replace_with_retry, write_once, RetryPolicy};

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("gputeer-codex-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ══════════════════════════════════════════════════════════════════
// K-1 ★ write_once 의 전제가 지켜지지 않는다
// ══════════════════════════════════════════════════════════════════

/// `write_once` 의 주석은 이렇게 적혀 있(었)다.
///
/// > **대상이 이미 존재하면 rename 을 시도하지 않는다.**
/// > content-addressed 이름이므로 존재한다는 것은 내용이 같다는 뜻이다.
///
/// 그런데 `writer.rs` 가 넘기는 이름은 `shard-0.bin` 같은 **위치 기반 이름**이다.
/// content-addressed 가 아니다. **전제가 성립하지 않는다.**
///
/// # 시나리오
///
/// ```text
/// 1. writer-A 가 ckpt-100/shard-0.bin 을 쓴다
/// 2. 매니페스트 쓰기 전에 프로세스가 죽는다
/// 3. writer-B 가 같은 checkpoint_id 로 **다른 내용**을 쓴다
/// 4. write_once 는 내용 비교 없이 Ok(false) 를 반환한다
/// 5. writer-B 의 매니페스트에는 **B 의 해시**가 기록된다
/// 6. 디스크에는 **A 의 데이터**가 있다
/// 7. write_checkpoint 는 성공을 반환한다
/// ```
///
/// `find_resume_point` 가 나중에 해시 불일치로 제외하므로 **데이터 손상은 아니다.**
/// 그러나 **writer 가 "확정했다" 고 거짓 보고한다** — 그것이 문제다.
#[test]
fn k1_write_once_must_detect_content_mismatch() {
    let d = tmpdir("k1");

    assert!(write_once(&d, "shard-0.bin", b"writer-A data").unwrap());

    // 같은 이름, 다른 내용
    let r = write_once(&d, "shard-0.bin", b"writer-B data");

    assert!(
        r.is_err(),
        "★ 코덱스 지적 K-1 — 이름이 같고 내용이 다른데 조용히 받아들였다: {r:?}\n\
         writer 가 '확정했다' 고 거짓 보고하게 된다."
    );

    // 디스크 내용은 그대로여야 한다 — 덮어쓰지 않는다
    assert_eq!(
        std::fs::read(d.join("shard-0.bin")).unwrap(),
        b"writer-A data",
        "기존 파일을 덮어썼다 — write-once 가 아니다"
    );
}

/// 같은 이름 · **같은 내용**은 정상적으로 멱등하다.
///
/// 위 테스트만 있으면 "무조건 거부" 하는 구현이 통과한다.
#[test]
fn k1b_write_once_is_idempotent_for_identical_content() {
    let d = tmpdir("k1b");
    assert!(write_once(&d, "shard-0.bin", b"same").unwrap(), "첫 쓰기");
    assert!(
        !write_once(&d, "shard-0.bin", b"same").unwrap(),
        "같은 내용 재시도는 Ok(false) 여야 한다 — 재시작 후 이어쓰기가 막힌다"
    );
}

// ══════════════════════════════════════════════════════════════════
// K-2 ★ max_attempts == 0 이면 panic 한다
// ══════════════════════════════════════════════════════════════════

/// ADR-026 은 "최종 실패는 **명시적 오류**" 를 계약으로 정한다.
/// 그런데 `RetryPolicy` 가 공개 구조체라 `max_attempts: 0` 을 만들 수 있고,
/// 그러면 루프가 한 번도 돌지 않아 `last_err.expect(..)` 에서 **panic** 한다.
///
/// panic 은 오류가 아니다 — 호출자가 처리할 수 없고, 데이터 경로에서
/// 프로세스를 죽인다.
#[test]
fn k2_zero_attempts_returns_error_not_panic() {
    let d = tmpdir("k2");
    let policy = RetryPolicy {
        max_attempts: 0,
        ..Default::default()
    };
    let r = replace_with_retry(&d, "LATEST", b"ckpt-1", policy);
    assert!(
        r.is_err(),
        "★ 코덱스 지적 K-2 — max_attempts=0 이 오류가 아니라 성공/panic 이 됐다: {r:?}"
    );
}

/// 정상 정책에서는 여전히 동작해야 한다 (비공허성).
#[test]
fn k2b_normal_policy_still_works() {
    let d = tmpdir("k2b");
    replace_with_retry(&d, "LATEST", b"ckpt-1", RetryPolicy::default()).expect("첫 쓰기");
    assert_eq!(std::fs::read(d.join("LATEST")).unwrap(), b"ckpt-1");
    replace_with_retry(&d, "LATEST", b"ckpt-2", RetryPolicy::default()).expect("덮어쓰기");
    assert_eq!(std::fs::read(d.join("LATEST")).unwrap(), b"ckpt-2");
}

// ══════════════════════════════════════════════════════════════════
// K-3  rename 성공 후 sync_dir 실패
// ══════════════════════════════════════════════════════════════════

/// 코덱스 지적 — `rename` 이 성공한 뒤 `sync_dir` 이 실패하면
/// API 는 오류를 반환하는데 **파일 상태는 이미 바뀌어 있다.**
///
/// # 이것을 어떻게 다뤄야 하는가
///
/// `sync_dir` 실패를 **성공으로 바꾸면 안 된다** — 디렉터리 엔트리가
/// 디스크에 확정되지 않았을 수 있고, 그러면 전원 차단 시 포인터가 사라진다.
///
/// **오류를 반환하는 것이 옳다.** 다만 호출자가 "파일은 이미 바뀌었을 수 있다" 는
/// 것을 알아야 하므로, 오류 타입이 그 사실을 전해야 한다.
///
/// ★ `sync_dir` 실패를 인위적으로 일으키기 어려워(권한 조작 필요)
///   **이 테스트는 현재 동작을 고정하는 데 그친다.**
///   실제 실패 주입은 `ENVIRONMENT-BLOCKED` 로 남긴다.
#[test]
fn k3_documents_rename_then_syncdir_failure_semantics() {
    let d = tmpdir("k3");
    replace_with_retry(&d, "LATEST", b"v1", RetryPolicy::default()).unwrap();

    // 정상 경로에서는 파일이 바뀌고 Ok 가 온다
    assert_eq!(std::fs::read(d.join("LATEST")).unwrap(), b"v1");

    // ★ sync_dir 실패 주입은 하지 못했다. 이 테스트는 다음 사실만 고정한다:
    //   replace_with_retry 는 rename 성공 후 sync_dir 을 호출하며,
    //   그 실패를 삼키지 않는다(오류로 전파). 코드 검토로 확인.
    //   실패 주입 테스트는 별도 스파이크가 필요하다.
    let src = include_str!("../src/atomic.rs");
    assert!(
        src.contains("sync_dir(dir)?"),
        "sync_dir 실패가 삼켜지고 있다 — 전원 차단 시 포인터가 사라질 수 있다"
    );
}

// ══════════════════════════════════════════════════════════════════
// 부수 확인 — write_once 의 tmp 정리
// ══════════════════════════════════════════════════════════════════

/// 존재하는 파일을 만나 `Ok(false)` 로 돌아갈 때 `.tmp` 가 남지 않는가.
#[test]
fn write_once_leaves_no_tmp_behind() {
    let d = tmpdir("tmpclean");
    write_once(&d, "a.bin", b"x").unwrap();
    let _ = write_once(&d, "a.bin", b"x");

    let leftovers: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), ".tmp 가 남았다: {leftovers:?}");
}

fn _unused(_p: &Path) {}
