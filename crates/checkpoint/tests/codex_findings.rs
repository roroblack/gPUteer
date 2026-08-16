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
use gputeer_checkpoint::CheckpointError;

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
    // ★ 오류 종류까지 확인한다 — is_err() 만 보면 I/O 오류도 통과한다
    assert!(
        matches!(r, Err(CheckpointError::InvalidRetryPolicy)),
        "★ 코덱스 지적 K-2 — max_attempts=0 이 InvalidRetryPolicy 가 아니다: {r:?}"
    );

    // ★ 존재하지 않는 디렉터리에서도 **정책 오류**가 먼저 나와야 한다.
    //   tmp 생성 뒤에 검사하면 I/O 오류가 나와 진단이 흐려진다 (2차 지적).
    let missing = d.join("does-not-exist");
    let r2 = replace_with_retry(
        &missing,
        "LATEST",
        b"x",
        RetryPolicy { max_attempts: 0, ..Default::default() },
    );
    assert!(
        matches!(r2, Err(CheckpointError::InvalidRetryPolicy)),
        "정책 검증이 tmp 생성보다 늦다 — I/O 오류가 먼저 나왔다: {r2:?}"
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

    // ★★ 아래는 **소스 텍스트 검사**이며 동작을 증명하지 않는다.
    //   독립 검수 2차가 정확히 지적했다 — "구현 동작을 증명하지 않는 소스 텍스트 테스트".
    //   그럼에도 남겨 두는 이유는 `?` 가 `let _ =` 로 바뀌는 회귀를 잡기 때문이다.
    //   **실제 실패 주입은 하지 못했다**(권한 조작 필요) — 그 사실을 여기 적어 둔다.
    //   ENVIRONMENT-BLOCKED 로 남기고 별도 스파이크가 필요하다.
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
    assert!(write_once(&d, "a.bin", b"x").unwrap(), "첫 쓰기는 true");
    // ★ 결과를 무시하지 않는다 (2차 지적) — 같은 내용이므로 Ok(false) 여야 한다
    assert!(
        !write_once(&d, "a.bin", b"x").unwrap(),
        "같은 내용 재시도가 Ok(false) 가 아니다"
    );
    // 내용이 다르면 오류이고, 그때도 tmp 가 남으면 안 된다
    assert!(matches!(
        write_once(&d, "a.bin", b"different"),
        Err(CheckpointError::ContentMismatch { .. })
    ));

    let leftovers: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), ".tmp 가 남았다: {leftovers:?}");
}

fn _unused(_p: &Path) {}

// ══════════════════════════════════════════════════════════════════
// K-4 ★★ 경로 탈출 — 체크포인트 디렉터리 밖에 파일을 만들 수 있는가
// ══════════════════════════════════════════════════════════════════

/// 코덱스 2차 검토가 찾았다.
///
/// `write_once(dir, name, data)` 는 `dir.join(name)` 을 그대로 한다.
/// `name` 은 `write_checkpoint` 의 `files: &[(String, Vec<u8>)]` 에서 오고,
/// 그것은 **외부 입력**이다 (매니페스트의 파일 목록).
///
/// ```text
/// files = [("../../outside.bin", attacker_data)]
/// => 체크포인트 디렉터리 **밖**에 파일이 생긴다
/// ```
///
/// `proto/common.proto` 의 `CheckpointFile.path` 주석은
/// "`..`, 절대경로, 심볼릭 링크 금지" 라고 적혀 있다.
/// **그런데 아무도 검사하지 않았다.**
#[test]
fn k4_write_once_must_reject_path_traversal() {
    let root = tmpdir("k4");
    let inside = root.join("ckpt-1");
    std::fs::create_dir_all(&inside).unwrap();

    let attacks = [
        "../escaped.bin",
        "../../escaped.bin",
        "sub/../../escaped.bin",
        "a/../../escaped.bin",
    ];

    for name in attacks {
        let r = write_once(&inside, name, b"attacker");
        assert!(
            matches!(r, Err(CheckpointError::UnsafePath { .. })),
            "★ 코덱스 지적 K-4 — 경로 탈출이 UnsafePath 로 거부되지 않았다: {name:?} -> {r:?}"
        );
    }

    // 절대 경로도 막아야 한다
    #[cfg(windows)]
    let abs = "C:/Windows/Temp/gputeer-escape.bin";
    #[cfg(not(windows))]
    let abs = "/tmp/gputeer-escape.bin";
    assert!(
        matches!(
            write_once(&inside, abs, b"attacker"),
            Err(CheckpointError::UnsafePath { .. })
        ),
        "절대 경로가 UnsafePath 로 거부되지 않았다"
    );

    // ★ 실제로 밖에 파일이 안 생겼는지 확인 — 오류를 냈어도 이미 썼을 수 있다
    assert!(
        !root.join("escaped.bin").exists(),
        "오류를 반환했지만 파일은 이미 만들어졌다"
    );
}

/// 정상 이름은 여전히 통과해야 한다 (비공허성).
///
/// **하위 디렉터리는 허용한다** — `model/weights.safetensors` 같은 경로가
/// 실제 벡터에 있다(`v21_checkpoint_manifest`).
#[test]
fn k4b_normal_names_including_subdirs_still_work() {
    let d = tmpdir("k4b");
    write_once(&d, "shard-0.bin", b"x").expect("평범한 이름");

    std::fs::create_dir_all(d.join("model")).unwrap();
    write_once(&d, "model/weights.safetensors", b"y").expect("하위 디렉터리 경로");
    assert!(d.join("model/weights.safetensors").exists());
}
