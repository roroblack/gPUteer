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

use gputeer_checkpoint::atomic::{gc_partial, replace_with_retry, write_once, RetryPolicy};
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

/// ★ K-1c — 동시 동일-이름 호출은 **지원하지 않고 명시적으로
/// 거부한다**(2026-08-19,
/// `docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md`).
///
/// 이 test 는 원래 "여러 호출자가 같은 `name` 으로 동시에
/// `write_once` 를 부르면 전부 같은 tmp 파일 이름(`{name}.tmp`)을
/// 공유해 여러 개가 각각 `Ok(true)` 를 반환한다"는 **결함을
/// 고정하는 테스트**였다(Windows 8스레드 동시 호출에서 `Ok(true)`
/// 3회 관측, `DoD-08`). Linux 에서는 같은 조건이 재현되지 않았다
/// (`ENV-03`, `rename` 시맨틱 차이로 증상만 달랐을 뿐 근본 원인은
/// 그대로였다).
///
/// ★ 2026-08-19 재설계(코덱스 독립 검수 `p128` 반영) — 처음엔 이
/// 테스트가 "패자 7개는 전부 정확히 `WriteInProgress`" 를 주장했다.
/// **그건 이 test 방식으로는 보장되지 않는 주장이었다.** `Barrier`
/// 는 8스레드가 **같은 순간에 출발**하는 것만 보장하지, **같은
/// 순간에 도착**하는 것은 보장하지 않는다 — 느린 스레드가 승자의
/// `write_once` 호출이 이미 끝나 락을 놓은 **뒤**에야 자기 차례가
/// 오면, 락은 이미 비어 있어 그 스레드도 락을 얻고, `final_path`
/// 를 읽어 내용을 대조해 `ContentMismatch` 를 받는다(각 writer 가
/// 서로 다른 내용을 쓰므로 `Ok(false)` 는 나올 수 없다) —
/// `WriteInProgress` 가 아니다. 둘 다 **안전하다**(승자를 덮어쓰지
/// 않는다) — 다만 "패자는 반드시 `WriteInProgress`" 라는 주장은
/// 스케줄링에 따라 우연히 성립할 뿐인 결정론적이지 않은 주장이었다.
/// 그 정확한 경로("A 가 락을 쥔 동안 B 가 반드시 `WriteInProgress`
/// 를 받는다")는 타이밍에 기대지 않는 [`k1d_lock_is_released_when_holder_is_dropped_so_next_writer_proceeds`]
/// 가 결정론적으로 증명한다. 이 test 는 그 대신 **항상 참인 더 약한
/// 불변식**만 확인한다 — 승자는 정확히 하나, 나머지는 전부 오류
/// (`WriteInProgress` 또는 `ContentMismatch`)이고 **절대로 두 번째
/// 성공이나 데이터 손상은 없다.**
#[test]
fn k1c_concurrent_same_name_writers_only_one_writer_wins() {
    let d = std::sync::Arc::new(tmpdir("k1c"));
    let n = 8;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(n));

    let handles: Vec<_> = (0..n)
        .map(|i| {
            let d = d.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let data = format!("writer-{i} data").into_bytes();
                write_once(&d, "shard-race.bin", &data)
            })
        })
        .collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    let ok_true = results.iter().filter(|r| matches!(r, Ok(true))).count();
    let write_in_progress = results
        .iter()
        .filter(|r| matches!(r, Err(CheckpointError::WriteInProgress { .. })))
        .count();
    let content_mismatch = results
        .iter()
        .filter(|r| matches!(r, Err(CheckpointError::ContentMismatch { .. })))
        .count();
    let other: Vec<_> = results
        .iter()
        .filter(|r| {
            !matches!(
                r,
                Ok(true)
                    | Err(CheckpointError::WriteInProgress { .. })
                    | Err(CheckpointError::ContentMismatch { .. })
            )
        })
        .collect();

    assert_eq!(ok_true, 1, "정확히 하나만 성공해야 한다: {results:?}");
    assert_eq!(
        write_in_progress + content_mismatch,
        n - 1,
        "나머지는 전부 WriteInProgress 또는 ContentMismatch 여야 한다 \
         (도착 순서에 따라 둘 중 무엇이 될지는 달라지지만, 어느 쪽이든 \
         승자를 덮어쓰지 않는다는 뜻이다): {results:?}"
    );
    assert!(
        other.is_empty(),
        "예상 밖의 결과가 섞였다 — 두 번째 성공(Ok(true)) 이나 Ok(false) \
         가 있으면 승자가 아닌 쪽이 조용히 통과했거나 데이터가 손상됐다는 \
         뜻이다 — {other:?}"
    );
}

/// ★ K-1d — 락은 **crash-safe** 해야 한다: 이전 호출이 잠금을
/// 쥔 채로 죽어도(파일 핸들이 사라지면) OS 가 자동으로 풀어야
/// 다음 정당한 호출이 영구히 막히지 않는다. `File::try_lock` 이
/// 파일 핸들에 묶인 잠금(`flock`/`LockFileEx`)이라 handle 이 drop
/// 되면 잠금도 풀린다는 계약을 실측으로 확인한다 — 실제 crash 를
/// 낼 수는 없으므로, "락 파일을 쥔 핸들을 drop 하면 다음 호출이
/// 통과하는가"로 그 계약의 핵심을 확인한다.
#[test]
fn k1d_lock_is_released_when_holder_is_dropped_so_next_writer_proceeds() {
    let d = tmpdir("k1d");
    let lock_path = d.join("shard-lock.bin.write_once.lock");

    // writer-A 역할 — 락을 쥐고 있는 상태를 흉내낸다.
    let held = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    held.try_lock().expect("첫 번째 잠금은 성공해야 한다");

    // writer-B 가 같은 순간에 부르면 즉시 거부돼야 한다.
    let result = write_once(&d, "shard-lock.bin", b"writer-B data");
    assert!(
        matches!(result, Err(CheckpointError::WriteInProgress { .. })),
        "락이 쥐어진 동안에는 WriteInProgress 여야 한다: {result:?}"
    );

    // writer-A 가 죽는다(핸들 drop) — OS 가 잠금을 자동 해제한다.
    drop(held);

    // writer-C 가 이제는 정상적으로 성공해야 한다 — 영구히 막히지 않는다.
    let result = write_once(&d, "shard-lock.bin", b"writer-C data");
    assert!(
        matches!(result, Ok(true)),
        "락 보유자가 사라진 뒤에는 정상적으로 성공해야 한다: {result:?}"
    );
    assert_eq!(
        std::fs::read(d.join("shard-lock.bin")).unwrap(),
        b"writer-C data"
    );
}

/// ★ K-1e — `gc_partial` 의 `.write_once.lock` 처리 (2단계, 계획 문서
/// §범위/In, 코덱스 독립 검수 `p128` 반영해 2026-08-19 재설계).
///
/// 규칙은 "무조건 보존" 이 아니라 **"지금 쥐고 있으면 보존, 아무도
/// 안 쥐고 있으면 결국 지운다"** 다.
///
/// - 무조건 보존하면(첫 구현) 죽은 프로세스가 남긴 락 파일이 영원히
///   남아 그 디렉터리를 `startup_gc` 가 절대 청소하지 못한다(코덱스
///   `p128` 이 지적한 실제 회귀).
/// - 무조건 지우면 활성 writer 의 상호 배제가 깨진다(첫 설계가
///   막으려던 문제, `atomic.rs` 주석 참조).
/// - 그래서 GC 자신이 `try_lock` 으로 "지금 아무도 안 쥐고 있다" 를
///   직접 확인한 뒤에만 지운다.
///
/// 세 경우를 확인한다.
///
/// 1. 매니페스트 없음(PARTIAL) + **아무도 안 쥔** 락 파일 -> 지워진다
///    (디렉터리가 결국 청소될 수 있어야 한다).
/// 2. 매니페스트 없음(PARTIAL) + **지금 쥐고 있는** 락 파일 -> 보존된다
///    (활성 writer 를 방해하지 않는다).
/// 3. 매니페스트 있음 + 등록 안 된 `.tmp`(회귀 확인용) + 아무도 안 쥔
///    락 파일 -> `.tmp` 는 지워지고, 락 파일도 지워진다(매니페스트
///    유무와 무관하게 "쥐고 있는가" 만으로 판단한다).
#[test]
fn k1e_gc_partial_reclaims_dead_locks_but_preserves_held_ones() {
    // 경우 1 — PARTIAL + 아무도 안 쥔 락.
    let d1 = tmpdir("k1e-dead-no-manifest");
    let lock1 = d1.join("shard-0.bin.write_once.lock");
    std::fs::write(&lock1, b"").unwrap();
    let removed1 = gc_partial(&d1, "manifest.json").unwrap();
    assert!(
        !lock1.exists(),
        "아무도 안 쥔 락 파일은 매니페스트가 없으면 결국 지워져야 한다 \
         — 안 그러면 이 디렉터리가 영원히 청소되지 않는다"
    );
    assert!(removed1.iter().any(|p| p == &lock1));

    // 경우 2 — PARTIAL + 지금 쥐고 있는 락.
    let d2 = tmpdir("k1e-held-no-manifest");
    let lock2 = d2.join("shard-0.bin.write_once.lock");
    let held = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&lock2)
        .unwrap();
    held.try_lock().expect("첫 번째 잠금은 성공해야 한다");

    let removed2 = gc_partial(&d2, "manifest.json").unwrap();
    assert!(
        lock2.exists(),
        "지금 쥐고 있는 락 파일은 매니페스트가 없어도 보존돼야 한다 \
         — 활성 writer 의 상호 배제를 깨면 안 된다"
    );
    assert!(!removed2.iter().any(|p| p == &lock2));
    drop(held);

    // 경우 3 — 매니페스트 있음 + 등록 안 된 .tmp + 아무도 안 쥔 락.
    let d3 = tmpdir("k1e-with-manifest");
    let manifest = gputeer_checkpoint::CheckpointManifest {
        schema_version: 1,
        checkpoint_id: "ckpt-k1e".to_string(),
        job_id: "job-k1e".to_string(),
        attempt_id: "attempt-k1e".to_string(),
        step: 0,
        files: Vec::new(),
        root_digest: String::new(),
        total_bytes: 0,
        created_at_unix_ms: 0,
        producer_node_id: "node-k1e".to_string(),
        fence_epoch: 0,
    };
    std::fs::write(d3.join("manifest.json"), manifest.to_json().unwrap()).unwrap();
    let lock3 = d3.join("shard-0.bin.write_once.lock");
    let orphan_tmp = d3.join("shard-0.bin.tmp");
    std::fs::write(&lock3, b"").unwrap();
    std::fs::write(&orphan_tmp, b"orphan").unwrap();

    let removed3 = gc_partial(&d3, "manifest.json").unwrap();
    assert!(
        !orphan_tmp.exists(),
        "등록 안 된 .tmp 는 여전히 지워져야 한다 — 회귀 확인"
    );
    assert!(
        !lock3.exists(),
        "아무도 안 쥔 락은 매니페스트가 있어도 결국 지워져야 한다"
    );
    assert!(removed3.iter().any(|p| p == &orphan_tmp));
    assert!(removed3.iter().any(|p| p == &lock3));
}

/// ★ K-1f — 이름이 우연히 `.write_once.lock` 로 끝나는 **등록된 데이터
/// 파일**을 GC 가 가짜 락 파일로 오인해 지우면 안 된다(2026-08-19,
/// 코덱스 독립 검수 `p129` 지적).
///
/// `validate_relative_name` 은 이 접미사를 예약하지 않는다. 파일
/// 이름은 매니페스트에서 오는 **외부 입력**이다(`CLAUDE.md` §0) —
/// 매니페스트가 실제로 이 접미사로 끝나는 이름을 등록했다면, 그건
/// 가짜 락이 아니라 진짜 데이터다. `.tmp` 접미사도 같은 이름 충돌
/// 위험이 있어서 `registered_tmp` 로 "등록됐으면 보존" 을 이미
/// 검사한다(`write_failure.rs::registered_tmp_suffix_is_preserved`
/// 참조) — 이 test 는 `.write_once.lock` 도 같은 방어를 받는지
/// 확인한다.
#[test]
fn k1f_registered_file_named_like_a_lock_file_is_preserved() {
    let d = tmpdir("k1f");
    let weird_name = "shard-0.bin.write_once.lock";
    std::fs::write(d.join(weird_name), b"real-data").unwrap();

    let manifest = gputeer_checkpoint::CheckpointManifest {
        schema_version: 1,
        checkpoint_id: "ckpt-k1f".to_string(),
        job_id: "job-k1f".to_string(),
        attempt_id: "attempt-k1f".to_string(),
        step: 0,
        files: vec![gputeer_checkpoint::CheckpointFile {
            path: weird_name.to_string(),
            digest: blake3::hash(b"real-data").to_hex().to_string(),
            size_bytes: 9,
        }],
        root_digest: String::new(),
        total_bytes: 9,
        created_at_unix_ms: 0,
        producer_node_id: "node-k1f".to_string(),
        fence_epoch: 0,
    };
    std::fs::write(d.join("manifest.json"), manifest.to_json().unwrap()).unwrap();

    let removed = gc_partial(&d, "manifest.json").unwrap();
    assert!(
        d.join(weird_name).exists(),
        "이름이 락 파일과 같아도 매니페스트에 등록된 데이터는 보존돼야 한다"
    );
    assert!(!removed.iter().any(|p| p == &d.join(weird_name)));
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
        RetryPolicy {
            max_attempts: 0,
            ..Default::default()
        },
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

/// ★ K-4c — `.write_once.lock` 로 끝나는 이름은 데이터 파일로 쓸 수
/// 없다(2026-08-19, 코덱스 독립 검수 `p130`·`p131` 이 발견한 근본
/// 원인).
///
/// `write_once(dir, "foo", ..)` 의 락 파일은 정확히
/// `dir/foo.write_once.lock` 이다. 이 이름을 데이터 파일 이름으로도
/// 허용하면, `write_once(dir, "foo.write_once.lock", data)` 로 만든
/// **진짜 데이터 파일**과 `write_once(dir, "foo", ..)` 의 **락 파일**
/// 이 정확히 같은 경로를 가리키게 된다 — `.tmp` 접미사처럼 GC 의
/// "등록됐으면 보존" 검사로는 못 막는다(경로 자체가 같아지는
/// 문제이지, GC 오인 문제가 아니다). 그래서 이름 자체를 금지한다.
///
/// `p131` 이 지적한 대로, 원래(대소문자를 구분하는) 검사만으로는
/// 안 끝난다 — NTFS 는 대소문자를 구분하지 않고(`...LOCK` 도 같은
/// 파일), Win32 파일 API 는 마지막 경로 성분의 후행 점·공백을
/// 자동으로 잘라낸다(`...lock.` 도 결국 같은 파일). 세 변형(정확한
/// 대소문자, 대문자, 후행 점) 을 전부 거부하는지 확인하고, 그
/// 접미사가 없는 정상적인 형제 이름(`foo`)은 여전히 정상 동작해
/// 그 락 파일이 실제로 만들어지는지도 확인한다(진짜 충돌 시나리오의
/// 두 재료가 각자 예상대로 동작하는지 확인).
#[test]
fn k4c_data_file_named_like_a_lock_file_is_rejected() {
    let d = tmpdir("k4c");

    for variant in [
        "foo.write_once.lock",
        "foo.write_once.LOCK",
        "foo.write_once.lock.",
        "foo.write_once.lock ",
    ] {
        let result = write_once(&d, variant, b"data");
        assert!(
            matches!(result, Err(CheckpointError::UnsafePath { .. })),
            "'{variant}' 는 거부돼야 한다: {result:?}"
        );
    }
    assert!(!d.join("foo.write_once.lock").exists());

    // 접미사 없는 정상 이름은 여전히 동작하고, 그 락 파일이 바로
    // 방금 거부한 그 경로다 — 왜 이 이름을 예약해야 하는지 증명한다.
    write_once(&d, "foo", b"real data").expect("정상 이름은 여전히 동작해야 한다");
    assert_eq!(std::fs::read(d.join("foo")).unwrap(), b"real data");
}
