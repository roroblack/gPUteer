//! 체크포인트 durability 카오스·negative 테스트.
//!
//! `RULE.md` §6 — 정상 경로 테스트만으로는 완료가 아니다.
//! 이 파일은 ADR-026 이 실제로 Windows 문제를 해결하는지 검증한다.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use gputeer_checkpoint::durability::{record_initial_state, ReplicaSet, MANIFEST_FILENAME};
use gputeer_checkpoint::{
    gc_partial, replace_with_retry, startup_gc, sync_dir, write_once, CheckpointError,
    CheckpointFile, CheckpointManifest, Durability, DurabilityState, RetryPolicy,
};

// ═══════════════════════════════════════════════════════════════════
// ADR-026 핵심 검증
//
// P0-03a 에서 Windows 는 독자가 파일을 연 상태에서 rename 이 실패했다
// (313/3000). ADR-026 은 "데이터 파일을 고유 이름으로 write-once 하면
// rename 대상이 존재하지 않으므로 문제를 회피한다" 고 결정했다.
//
// 이 테스트가 그 결정을 실증한다.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn adr026_write_once_succeeds_while_readers_hold_files_open() {
    let dir = tempfile::tempdir().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let read_count = Arc::new(AtomicU64::new(0));

    // 독자 스레드: 이미 확정된 파일들을 계속 열어둔다.
    // P0-03a 의 최악 조건과 같다.
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let d = dir.path().to_path_buf();
            let stop = Arc::clone(&stop);
            let cnt = Arc::clone(&read_count);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(entries) = fs::read_dir(&d) {
                        for e in entries.flatten() {
                            if let Ok(mut f) = File::open(e.path()) {
                                let mut buf = Vec::new();
                                let _ = f.read_to_end(&mut buf);
                                cnt.fetch_add(1, Ordering::Relaxed);
                                // 핸들을 잠시 유지해 경합을 만든다
                                thread::sleep(Duration::from_micros(50));
                            }
                        }
                    }
                }
            })
        })
        .collect();

    // 고유 이름으로 write-once — ADR-026 의 데이터 파일 경로
    let total = 500;
    let mut written = 0;
    for i in 0..total {
        let name = format!("chunk-{i:05}.bin");
        let data = vec![(i % 251) as u8; 4096];
        let is_new = write_once(dir.path(), &name, &data)
            .unwrap_or_else(|e| panic!("write_once 실패 (i={i}): {e}"));
        if is_new {
            written += 1;
        }
    }

    stop.store(true, Ordering::Relaxed);
    for r in readers {
        r.join().unwrap();
    }

    assert_eq!(
        written, total,
        "독자가 파일을 열고 있어도 write-once 는 전부 성공해야 한다 (ADR-026)"
    );
    assert!(
        read_count.load(Ordering::Relaxed) > 0,
        "독자가 실제로 파일을 열었어야 유효한 테스트다"
    );
}

#[test]
fn adr026_write_once_is_idempotent_for_same_name() {
    let dir = tempfile::tempdir().unwrap();
    let data = b"content-addressed";

    assert!(
        write_once(dir.path(), "a.bin", data).unwrap(),
        "첫 쓰기는 새 파일"
    );
    assert!(
        !write_once(dir.path(), "a.bin", data).unwrap(),
        "이미 존재하면 rename 을 시도하지 않고 false 를 반환해야 한다"
    );

    // .tmp 가 남으면 안 된다
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), ".tmp 잔여물이 남았다: {leftovers:?}");
}

#[test]
fn pointer_replace_retries_and_eventually_reports_error() {
    let dir = tempfile::tempdir().unwrap();

    // 정상 경로: 포인터는 여러 번 갱신 가능해야 한다
    for i in 0..10u32 {
        replace_with_retry(
            dir.path(),
            "latest",
            format!("step={i}").as_bytes(),
            RetryPolicy::default(),
        )
        .unwrap_or_else(|e| panic!("포인터 갱신 실패 (i={i}): {e}"));
    }
    let content = fs::read_to_string(dir.path().join("latest")).unwrap();
    assert_eq!(content, "step=9");
}

#[test]
fn pointer_replace_exhaustion_is_explicit_error_not_silent() {
    // RULE.md §3.2 — 재시도 소진은 조용히 넘어가지 않는다.
    // 대상 경로를 디렉터리로 만들어 rename 이 반드시 실패하게 한다.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("blocked")).unwrap();
    fs::write(dir.path().join("blocked").join("keep"), b"x").unwrap();

    let policy = RetryPolicy {
        max_attempts: 2,
        initial_backoff: Duration::from_millis(1),
        max_backoff: Duration::from_millis(2),
    };
    let err = replace_with_retry(dir.path(), "blocked", b"data", policy)
        .expect_err("비어있지 않은 디렉터리 위로는 replace 가 실패해야 한다");

    match err {
        CheckpointError::ReplaceExhausted { attempts, .. } => {
            assert_eq!(attempts, 2, "재시도 횟수가 보고되어야 한다");
        }
        other => panic!("ReplaceExhausted 를 기대했으나 {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════
// PARTIAL 판정과 GC — 기준선 §18.2 규칙 2·3
// ═══════════════════════════════════════════════════════════════════

#[test]
fn manifest_last_rule_identifies_partial_checkpoints() {
    let root = tempfile::tempdir().unwrap();

    // 정상: 데이터 확정 후 매니페스트
    let good = root.path().join("ckpt-good");
    fs::create_dir(&good).unwrap();
    write_once(&good, "model.bin", b"weights").unwrap();
    write_once(&good, MANIFEST_FILENAME, b"{}").unwrap();

    // 비정상: 매니페스트 없음 (쓰기 중 kill 시뮬레이션)
    let partial = root.path().join("ckpt-partial");
    fs::create_dir(&partial).unwrap();
    write_once(&partial, "model.bin", b"weights").unwrap();

    let removed_good = gc_partial(&good, MANIFEST_FILENAME).unwrap();
    assert!(
        removed_good.is_empty(),
        "완전한 체크포인트는 GC 되면 안 된다"
    );
    assert!(good.join("model.bin").exists());

    let removed_partial = gc_partial(&partial, MANIFEST_FILENAME).unwrap();
    assert_eq!(
        removed_partial.len(),
        1,
        "매니페스트 없는 데이터는 GC 대상이다"
    );
    assert!(!partial.join("model.bin").exists());
}

#[test]
fn startup_gc_removes_marker_only_checkpoint_but_preserves_manifest_checkpoint() {
    let root = tempfile::tempdir().unwrap();

    // Match Agent::record_start_checkpoint(): create the checkpoint directory,
    // then write only the initial durability marker.
    let marker_only = root.path().join("start-checkpoint");
    fs::create_dir(&marker_only).unwrap();
    record_initial_state(&marker_only).unwrap();
    assert!(marker_only.join(".durability.writing").is_file());
    assert!(!marker_only.join(MANIFEST_FILENAME).exists());

    // A complete checkpoint has a data file and a manifest in the same
    // checkpoint directory, which is the layout startup_gc expects.
    let complete = root.path().join("complete-checkpoint");
    fs::create_dir(&complete).unwrap();
    let manifest = manifest_for(&complete, &[("model.bin", b"weights")]);
    write_once(&complete, MANIFEST_FILENAME, &manifest.to_json().unwrap()).unwrap();

    let (dirs, removed) = startup_gc(root.path()).unwrap();

    assert_eq!(
        dirs, 2,
        "startup_gc must inspect both checkpoint directories"
    );
    assert_eq!(
        removed, 1,
        "the marker-only checkpoint contributes one removed marker"
    );
    assert!(
        !marker_only.exists(),
        "marker-only checkpoint directory must be removed"
    );
    assert!(
        complete.is_dir(),
        "manifest checkpoint directory must be preserved"
    );
    assert!(complete.join("model.bin").is_file());
    assert!(complete.join(MANIFEST_FILENAME).is_file());
}

#[test]
fn gc_removes_orphan_tmp_files() {
    let dir = tempfile::tempdir().unwrap();
    write_once(dir.path(), MANIFEST_FILENAME, b"{}").unwrap();
    // 중단된 쓰기가 남긴 .tmp
    fs::write(dir.path().join("model.bin.tmp"), b"partial").unwrap();

    let removed = gc_partial(dir.path(), MANIFEST_FILENAME).unwrap();
    assert_eq!(removed.len(), 1, ".tmp 는 매니페스트가 있어도 항상 GC 된다");
}

// ═══════════════════════════════════════════════════════════════════
// 해시 검증 — LocalWritten -> HashVerified 의 근거
// ═══════════════════════════════════════════════════════════════════

fn manifest_for(dir: &std::path::Path, files: &[(&str, &[u8])]) -> CheckpointManifest {
    let entries: Vec<CheckpointFile> = files
        .iter()
        .map(|(name, data)| CheckpointFile {
            path: (*name).to_string(),
            digest: blake3::hash(data).to_hex().to_string(),
            size_bytes: data.len() as u64,
        })
        .collect();
    let total: u64 = entries.iter().map(|f| f.size_bytes).sum();
    for (name, data) in files {
        write_once(dir, name, data).unwrap();
    }
    CheckpointManifest {
        schema_version: 1,
        checkpoint_id: "ckpt-1".into(),
        job_id: "job-1".into(),
        attempt_id: "att-1".into(),
        step: 7300,
        files: entries,
        root_digest: "unused-in-this-test".into(),
        total_bytes: total,
        created_at_unix_ms: 1_755_100_800_000,
        producer_node_id: "node-1".into(),
        fence_epoch: 42,
    }
}

#[test]
fn hash_verification_passes_for_intact_files() {
    let dir = tempfile::tempdir().unwrap();
    let m = manifest_for(
        dir.path(),
        &[("model.bin", b"weights"), ("optim.bin", b"adam")],
    );
    m.verify_files(dir.path())
        .expect("온전한 파일은 검증을 통과해야 한다");
}

#[test]
fn negative_hash_mismatch_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let m = manifest_for(dir.path(), &[("model.bin", b"weights")]);

    // 1바이트 변조
    fs::write(dir.path().join("model.bin"), b"weightz").unwrap();

    match m.verify_files(dir.path()) {
        Err(CheckpointError::HashMismatch { .. }) => {}
        other => panic!("HashMismatch 를 기대했으나 {other:?}"),
    }
}

#[test]
fn manifest_json_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let m = manifest_for(dir.path(), &[("a.bin", b"x")]);
    let json = m.to_json().unwrap();
    let back = CheckpointManifest::from_json(&json).unwrap();
    assert_eq!(back.step, m.step);
    assert_eq!(back.fence_epoch, m.fence_epoch);
    assert_eq!(back.files.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════
// 상태 전이 — state-machines.md §4. 표에 없는 전이는 거부
// ═══════════════════════════════════════════════════════════════════

#[test]
fn allowed_transitions_follow_normative_table() {
    use DurabilityState::*;
    let allowed = [
        (Writing, LocalWritten),
        (Writing, Partial),
        (LocalWritten, HashVerified),
        (LocalWritten, Partial),
        (HashVerified, Replicating),
        (HashVerified, Committed),
        (Replicating, Replicated),
        (Replicating, HashVerified),
        (Replicated, Committed),
        (Replicated, Replicating),
        (Committed, CommittedDegraded),
        (CommittedDegraded, Committed),
    ];
    for (from, to) in allowed {
        from.transition(to)
            .unwrap_or_else(|e| panic!("규범 표의 전이가 거부됐다 {from:?}->{to:?}: {e}"));
    }
}

#[test]
fn negative_undefined_transitions_are_rejected() {
    use DurabilityState::*;
    // 규범 표에 없는 전이들. 특히 PARTIAL -> COMMITTED 는
    // "미완성 데이터가 canonical 후보가 되는" 최악의 결함이다.
    let forbidden = [
        (Partial, Committed),
        (Partial, HashVerified),
        (Writing, Committed),
        (Writing, HashVerified),
        (LocalWritten, Committed),
        (Committed, Writing),
        (Committed, Partial),
    ];
    for (from, to) in forbidden {
        assert!(
            from.transition(to).is_err(),
            "표에 없는 전이가 허용됐다: {from:?} -> {to:?}"
        );
    }
}

#[test]
fn negative_partial_can_never_become_canonical_candidate() {
    // P0-03 DoD 의 핵심 항목
    assert!(!DurabilityState::Partial.is_canonical_candidate());
    assert!(!DurabilityState::Writing.is_canonical_candidate());
    assert!(!DurabilityState::LocalWritten.is_canonical_candidate());
    assert!(!DurabilityState::HashVerified.is_canonical_candidate());
    assert!(!DurabilityState::Replicating.is_canonical_candidate());
    assert!(!DurabilityState::Replicated.is_canonical_candidate());

    assert!(DurabilityState::Committed.is_canonical_candidate());
    // COMMITTED 이후 replica 유실은 자격을 박탈하지 않는다 (기준선 §18.2)
    assert!(DurabilityState::CommittedDegraded.is_canonical_candidate());
}

// ═══════════════════════════════════════════════════════════════════
// replica 계수 — 기준선 §18.2 규칙 1~4
// ═══════════════════════════════════════════════════════════════════

#[test]
fn replica_counting_applies_all_normative_rules() {
    let mut rs = ReplicaSet::new();

    // 규칙 1 — 서명 안 된 ACK 은 세지 않는다
    rs.add("dc-a", "dev-1", false, false);
    assert_eq!(
        rs.effective_count(),
        0,
        "서명되지 않은 ACK 은 replica 가 아니다"
    );

    rs.add("dc-a", "dev-1", false, true);
    assert_eq!(rs.effective_count(), 1);

    // 규칙 2 — 같은 failure domain 은 1개로 센다
    rs.add("dc-a", "dev-2", false, true);
    assert_eq!(
        rs.effective_count(),
        1,
        "같은 failure domain 은 중복 계상하지 않는다"
    );

    rs.add("dc-b", "dev-3", false, true);
    assert_eq!(rs.effective_count(), 2);

    // 규칙 3 — ephemeral 노드의 로컬 복사본은 세지 않는다
    rs.add("dc-c", "runpod-1", true, true);
    assert_eq!(
        rs.effective_count(),
        2,
        "ephemeral 노드는 durable replica 가 아니다"
    );
}

#[test]
fn durability_requirements_match_baseline() {
    assert_eq!(Durability::Local.required_replicas(), 0);
    assert_eq!(Durability::Mirrored.required_replicas(), 1);
    assert_eq!(Durability::Replicated.required_replicas(), 2);

    let mut rs = ReplicaSet::new();
    rs.add("dc-a", "dev-1", false, true);
    assert!(rs.satisfies(Durability::Local));
    assert!(rs.satisfies(Durability::Mirrored));
    assert!(
        !rs.satisfies(Durability::Replicated),
        "replica 1개로 REPLICATED 는 불가"
    );
}

// ═══════════════════════════════════════════════════════════════════
// sync_dir — P0-03a 발견 (Windows 는 쓰기 권한 필요)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn sync_dir_works_on_this_platform() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("f"), b"x").unwrap();
    sync_dir(dir.path()).unwrap_or_else(|e| {
        panic!("sync_dir 실패 — Windows 라면 쓰기 권한 누락일 수 있다 (P0-03a): {e}")
    });
}

#[test]
fn write_once_survives_readers_with_no_share_delete() {
    // Windows 에서 std::fs::File::open 이 FILE_SHARE_DELETE 를 포함하는지와 무관하게
    // write-once 는 성공해야 한다 (대상이 존재하지 않으므로).
    let dir = tempfile::tempdir().unwrap();
    write_once(dir.path(), "held.bin", b"data").unwrap();

    // 파일을 연 채로 유지
    let _held = OpenOptions::new()
        .read(true)
        .open(dir.path().join("held.bin"))
        .unwrap();

    // 다른 이름으로는 계속 쓸 수 있어야 한다
    for i in 0..50 {
        let name = format!("other-{i}.bin");
        assert!(write_once(dir.path(), &name, b"x").unwrap());
    }
}
