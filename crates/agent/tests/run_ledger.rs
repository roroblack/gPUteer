//! 노드 실행 원장 — 파일 쪽 시험(계획 `docs/plans/2026-09-29_0212_노드_실행원장_기존노드_이관_구현계획.md` 의 R 목록 중
//! 원장 모듈만으로 확인할 수 있는 것). Agent 기동 흐름 · CLI 연결 시험은 그 연결과 함께 따로 둔다.

use gputeer_agent::run_ledger::{
    adopt_legacy_files, detect, open_for_agent, open_for_clear, scan_start_records, AdoptOutcome,
    AttemptRow, CloseReason, Executor, LedgerPaths, RowState, PAIR_NAME,
};
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    paths: LedgerPaths,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("checkpoints");
    fs::create_dir_all(&root).unwrap();
    let paths = LedgerPaths::for_root(&root).unwrap();
    Fixture {
        _tmp: tmp,
        root,
        paths,
    }
}

fn record_name(id: &str) -> String {
    blake3::hash(id.as_bytes()).to_hex().to_string()
}

/// 지금 Agent 가 쓰는 모양 그대로(lib.rs record_attempt_started_here).
fn write_record(dir: &Path, id: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(record_name(id)), format!("attempt_id={id}\n")).unwrap();
}

fn assert_err_contains<T: std::fmt::Debug>(result: Result<T, String>, needle: &str) {
    match result {
        Ok(v) => panic!("거부돼야 하는데 통과했다({v:?}) — 기대한 문구: {needle}"),
        Err(e) => assert!(e.contains(needle), "오류 문구에 {needle:?} 가 없다: {e}"),
    }
}

fn sql(paths: &LedgerPaths, statement: &str) {
    let conn = rusqlite::Connection::open(&paths.ledger).unwrap();
    conn.execute_batch(statement).unwrap();
}

// ---------- 열기 (R2 · R3 · R4 · R5 · R5b · R6 · R11e) ----------

#[test]
fn r2_a_new_node_creates_the_ledger_and_its_generation_pair() {
    let f = fixture();
    assert!(detect(&f.paths).unwrap().never_enabled());
    let ledger = open_for_agent(&f.paths).unwrap();
    let presence = detect(&f.paths).unwrap();
    assert!(presence.ledger && presence.pair && !presence.adopting);
    assert_eq!(
        fs::read_to_string(f.paths.pair()).unwrap(),
        ledger.generation()
    );
    assert!(ledger.rows().unwrap().is_empty());
    drop(ledger);
    // 다시 열어도 같은 세대다.
    let again = open_for_agent(&f.paths).unwrap();
    assert_eq!(
        fs::read_to_string(f.paths.pair()).unwrap(),
        again.generation()
    );
}

#[test]
fn r3_a_node_that_already_ran_must_adopt_first() {
    let f = fixture();
    write_record(&f.paths.started_dir, "attempt-a");
    assert_err_contains(open_for_agent(&f.paths), "RUN_LEDGER_ADOPT_FIRST");
    assert!(
        !detect(&f.paths).unwrap().ledger,
        "거부할 때 원장을 만들지 않는다"
    );
}

#[test]
fn r4_generation_pair_missing_or_different_is_refused() {
    let f = fixture();
    drop(open_for_agent(&f.paths).unwrap());
    fs::remove_file(f.paths.pair()).unwrap();
    assert_err_contains(open_for_agent(&f.paths), "RUN_LEDGER_PAIR_MISSING");

    let g = fixture();
    drop(open_for_agent(&g.paths).unwrap());
    fs::write(g.paths.pair(), "0123456789abcdef0123456789abcdef").unwrap();
    assert_err_contains(open_for_agent(&g.paths), "RUN_LEDGER_GENERATION_MISMATCH");
}

#[test]
fn r5_a_corrupt_file_or_unknown_format_version_is_refused() {
    let f = fixture();
    drop(open_for_agent(&f.paths).unwrap());
    fs::write(&f.paths.ledger, b"not a sqlite database at all, just bytes").unwrap();
    assert!(open_for_agent(&f.paths).is_err());

    let g = fixture();
    drop(open_for_agent(&g.paths).unwrap());
    sql(
        &g.paths,
        "UPDATE meta SET value = '2' WHERE key = 'format_version';",
    );
    assert_err_contains(open_for_agent(&g.paths), "형식 버전을 모른다");
}

fn ledger_with_one_container_row(f: &Fixture) {
    let mut ledger = open_for_agent(&f.paths).unwrap();
    ledger
        .insert_active(&AttemptRow::new_active(
            "attempt-c",
            "job-1",
            "node-1",
            3,
            Executor::Container,
        ))
        .unwrap();
    write_record(&f.paths.started_dir, "attempt-c");
}

#[test]
fn r5b_a_row_with_an_unknown_value_is_refused_on_open() {
    for (column, value, needle) in [
        ("row_format_version", "2", "형식 버전을 모른다"),
        ("origin", "'somebody'", "출처를 모른다"),
        ("executor", "'vm'", "실행기를 모른다"),
        ("state", "'OPEN'", "상태를 모른다"),
        ("stopped", "7", "stopped 값이"),
    ] {
        let f = fixture();
        ledger_with_one_container_row(&f);
        sql(
            &f.paths,
            &format!("UPDATE attempts SET {column} = {value};"),
        );
        assert_err_contains(open_for_agent(&f.paths), needle);
    }
}

#[test]
fn r11e_container_removed_must_match_the_executor() {
    for (value, needle) in [
        ("2", "container_removed 값이"),
        ("NULL", "실행기와 맞지 않는다"),
    ] {
        let f = fixture();
        ledger_with_one_container_row(&f);
        sql(
            &f.paths,
            &format!("UPDATE attempts SET container_removed = {value};"),
        );
        assert_err_contains(open_for_agent(&f.paths), needle);
    }
}

#[test]
fn r6_a_start_record_without_a_row_is_refused() {
    let f = fixture();
    drop(open_for_agent(&f.paths).unwrap());
    write_record(&f.paths.started_dir, "attempt-orphan");
    assert_err_contains(open_for_agent(&f.paths), "RUN_LEDGER_ROW_MISSING");
}

// ---------- 시작 기록 식별 · 부산물 (R29 · R33) ----------

#[test]
fn r29_the_pair_is_not_a_record_and_malformed_records_are_refused() {
    let f = fixture();
    let dir = &f.paths.started_dir;
    write_record(dir, "attempt-a");
    fs::write(dir.join(PAIR_NAME), "0123456789abcdef0123456789abcdef").unwrap();
    let ids = scan_start_records(dir, false).unwrap();
    assert_eq!(
        ids.into_iter().collect::<Vec<_>>(),
        vec!["attempt-a".to_string()]
    );

    // 깨진 내용
    let g = fixture();
    fs::create_dir_all(&g.paths.started_dir).unwrap();
    fs::write(g.paths.started_dir.join(record_name("x")), "attempt_id=x").unwrap(); // 줄바꿈 없음
    assert_err_contains(
        scan_start_records(&g.paths.started_dir, false),
        "내용이 깨졌다",
    );

    // 이름과 해시가 다름
    let h = fixture();
    fs::create_dir_all(&h.paths.started_dir).unwrap();
    fs::write(h.paths.started_dir.join(record_name("x")), "attempt_id=y\n").unwrap();
    assert_err_contains(
        scan_start_records(&h.paths.started_dir, false),
        "해시가 다르다",
    );

    // 모르는 파일
    let i = fixture();
    write_record(&i.paths.started_dir, "a");
    fs::write(i.paths.started_dir.join("notes.txt"), "hello").unwrap();
    assert_err_contains(
        scan_start_records(&i.paths.started_dir, false),
        "모르는 파일",
    );

    // 하위 폴더
    let j = fixture();
    fs::create_dir_all(j.paths.started_dir.join("sub")).unwrap();
    assert_err_contains(
        scan_start_records(&j.paths.started_dir, false),
        "일반 파일이 아닌",
    );
}

#[test]
fn r33_write_once_residue_is_cleaned_only_for_known_targets() {
    let f = fixture();
    let dir = &f.paths.started_dir;
    write_record(dir, "attempt-a");
    let lock = dir.join(format!("{}.write_once.lock", record_name("attempt-a")));
    let tmp_done = dir.join(format!("{}.tmp", record_name("attempt-a")));
    let tmp_orphan = dir.join(format!("{}.tmp", record_name("attempt-b")));
    let pair_lock = dir.join(format!("{PAIR_NAME}.write_once.lock"));
    for p in [&lock, &tmp_done, &tmp_orphan, &pair_lock] {
        fs::write(p, b"").unwrap();
    }
    let ids = scan_start_records(dir, true).unwrap();
    assert_eq!(ids.len(), 1);
    for p in [&lock, &tmp_done, &tmp_orphan, &pair_lock] {
        assert!(!p.exists(), "부산물이 남았다: {p:?}");
    }
    assert!(
        dir.join(record_name("attempt-a")).exists(),
        "기록은 그대로다"
    );

    let g = fixture();
    fs::create_dir_all(&g.paths.started_dir).unwrap();
    fs::write(g.paths.started_dir.join("random.write_once.lock"), b"").unwrap();
    assert_err_contains(
        scan_start_records(&g.paths.started_dir, true),
        "규칙에 맞지 않는 부산물",
    );
}

// ---------- 전이 (R26 · CLOSED 경계) ----------

#[test]
fn r26_a_container_row_does_not_close_without_proof_of_removal() {
    let f = fixture();
    let mut ledger = open_for_agent(&f.paths).unwrap();
    ledger
        .insert_active(&AttemptRow::new_active(
            "c1",
            "job",
            "node",
            1,
            Executor::Container,
        ))
        .unwrap();
    assert_err_contains(
        ledger.close_active("c1", CloseReason::ReportPersisted),
        "부재 확인 없이",
    );
    assert_err_contains(
        ledger.close_active("c1", CloseReason::HostRestart),
        "호스트 규칙으로",
    );
    ledger.mark_container_removed("c1", Some("abc123")).unwrap();
    ledger
        .close_active("c1", CloseReason::ReportPersisted)
        .unwrap();
    let row = ledger.row("c1").unwrap().unwrap();
    assert_eq!(row.state, RowState::Closed);
    assert_eq!(row.container_removed, Some(true));
    assert_eq!(row.container_id.as_deref(), Some("abc123"));
    // 표에 없는 전이
    assert_err_contains(
        ledger.close_active("c1", CloseReason::ReportPersisted),
        "ACTIVE 에서만",
    );
    assert_err_contains(ledger.mark_local_blocked("c1", "x"), "ACTIVE 에서만");
    assert_err_contains(ledger.clear_local_blocked("c1"), "LOCAL_BLOCKED 에서만");
}

#[test]
fn r26_local_blocked_is_released_only_by_the_clear_path_and_blocks_new_work() {
    let f = fixture();
    let mut ledger = open_for_agent(&f.paths).unwrap();
    assert!(!ledger.blocks_new_work().unwrap());
    ledger
        .insert_active(&AttemptRow::new_active(
            "c2",
            "job",
            "node",
            1,
            Executor::Container,
        ))
        .unwrap();
    assert!(
        ledger.blocks_new_work().unwrap(),
        "풀지 않은 ACTIVE 도 새 작업을 막는다"
    );
    ledger.mark_local_blocked("c2", "container_left").unwrap();
    assert!(ledger.blocks_new_work().unwrap());
    assert_err_contains(
        ledger.close_active("c2", CloseReason::NoSentinel),
        "ACTIVE 에서만",
    );
    ledger.clear_local_blocked("c2").unwrap();
    assert!(!ledger.blocks_new_work().unwrap());
}

#[test]
fn host_rows_close_after_the_report_without_a_removal_flag() {
    let f = fixture();
    let mut ledger = open_for_agent(&f.paths).unwrap();
    ledger
        .insert_active(&AttemptRow::new_active(
            "h1",
            "job",
            "node",
            1,
            Executor::Host,
        ))
        .unwrap();
    assert_err_contains(ledger.mark_container_removed("h1", None), "컨테이너 행에만");
    ledger
        .close_active("h1", CloseReason::ReportPersisted)
        .unwrap();
    assert_eq!(ledger.row("h1").unwrap().unwrap().container_removed, None);
}

// ---------- 이관 (R20 · R24 · R25 · R25c · R25d · R25e · R25f) ----------

#[test]
fn r24_adoption_writes_one_closed_legacy_row_per_record_and_the_agent_can_open_it() {
    let f = fixture();
    write_record(&f.paths.started_dir, "old-1");
    write_record(&f.paths.started_dir, "old-2");
    assert_eq!(
        adopt_legacy_files(&f.paths, false).unwrap(),
        AdoptOutcome::Adopted { rows: 2 }
    );
    let presence = detect(&f.paths).unwrap();
    assert!(presence.ledger && presence.pair && !presence.adopting);
    let ledger = open_for_agent(&f.paths).unwrap();
    let rows = ledger.rows().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|r| r.state == RowState::Closed && r.executor == Executor::Legacy));
    assert!(!ledger.blocks_new_work().unwrap());
    drop(ledger);
    assert_err_contains(adopt_legacy_files(&f.paths, false), "RUN_LEDGER_EXISTS");
}

#[test]
fn attested_adoption_is_recorded_in_the_row_origin() {
    let f = fixture();
    write_record(&f.paths.started_dir, "old-1");
    adopt_legacy_files(&f.paths, true).unwrap();
    let ledger = open_for_agent(&f.paths).unwrap();
    let row = ledger.row("old-1").unwrap().unwrap();
    assert_eq!(format!("{:?}", row.origin), "LegacyAdoptAttested");
}

#[test]
fn r25e_only_the_adopting_marker_survived_and_its_generation_is_reused() {
    let f = fixture();
    write_record(&f.paths.started_dir, "old-1");
    let g = "abcdefabcdefabcdefabcdefabcdef01";
    fs::write(&f.paths.adopting, g).unwrap();
    // Agent 는 이관이 끝나지 않았으면 거부한다(스위치와 무관 — 부르는 쪽은 detect 로도 본다).
    assert_err_contains(open_for_agent(&f.paths), "RUN_LEDGER_ADOPT_UNFINISHED");
    assert!(detect(&f.paths).unwrap().adopting);
    assert_eq!(
        adopt_legacy_files(&f.paths, false).unwrap(),
        AdoptOutcome::Adopted { rows: 1 }
    );
    assert_eq!(
        fs::read_to_string(f.paths.pair()).unwrap(),
        g,
        "표식의 G 를 그대로 썼다"
    );
    assert_eq!(open_for_agent(&f.paths).unwrap().generation(), g);

    let h = fixture();
    write_record(&h.paths.started_dir, "old-1");
    fs::write(&h.paths.adopting, "not-hex").unwrap();
    assert_err_contains(adopt_legacy_files(&h.paths, false), "형식이 틀렸다");

    let i = fixture();
    write_record(&i.paths.started_dir, "old-1");
    fs::write(&i.paths.adopting, "abcdefabcdefabcdefabcdefabcdef01").unwrap();
    fs::write(i.paths.pair(), "11111111111111111111111111111111").unwrap();
    assert_err_contains(
        adopt_legacy_files(&i.paths, false),
        "RUN_LEDGER_GENERATION_MISMATCH",
    );
}

#[test]
fn r25f_interrupted_after_rename_before_marker_removal_is_finished_not_refused() {
    let f = fixture();
    write_record(&f.paths.started_dir, "old-1");
    adopt_legacy_files(&f.paths, false).unwrap();
    // 이름 바꾸기 뒤 · 표식 삭제 전에 끊긴 모양을 만든다.
    let g = fs::read_to_string(f.paths.pair()).unwrap();
    fs::write(&f.paths.adopting, &g).unwrap();
    assert_err_contains(open_for_agent(&f.paths), "RUN_LEDGER_ADOPT_UNFINISHED");
    assert_eq!(
        adopt_legacy_files(&f.paths, false).unwrap(),
        AdoptOutcome::Resumed
    );
    assert!(!detect(&f.paths).unwrap().adopting);
    open_for_agent(&f.paths).unwrap();

    // 표식의 G 가 다르면 거부.
    let h = fixture();
    write_record(&h.paths.started_dir, "old-1");
    adopt_legacy_files(&h.paths, false).unwrap();
    fs::write(&h.paths.adopting, "22222222222222222222222222222222").unwrap();
    assert_err_contains(
        adopt_legacy_files(&h.paths, false),
        "RUN_LEDGER_GENERATION_MISMATCH",
    );
}

#[test]
fn r25c_a_lost_ledger_is_never_recreated_by_adoption_or_by_the_agent() {
    let f = fixture();
    write_record(&f.paths.started_dir, "old-1");
    adopt_legacy_files(&f.paths, false).unwrap();
    let mut ledger = open_for_agent(&f.paths).unwrap();
    ledger
        .insert_active(&AttemptRow::new_active(
            "running",
            "job",
            "node",
            1,
            Executor::Container,
        ))
        .unwrap();
    write_record(&f.paths.started_dir, "running");
    drop(ledger);
    fs::remove_file(&f.paths.ledger).unwrap(); // 쓰던 원장을 잃었다
    assert_err_contains(open_for_agent(&f.paths), "RUN_LEDGER_LOST");
    assert_err_contains(adopt_legacy_files(&f.paths, false), "RUN_LEDGER_LOST");
    assert!(
        !detect(&f.paths).unwrap().ledger,
        "legacy 행으로 다시 만들지 않았다"
    );
}

#[test]
fn r25d_a_new_node_interrupted_after_the_pair_continues_only_with_the_creating_marker() {
    // "만드는 중" 표식이 있으면 같은 G 로 이어 만들고 표식을 지운다.
    let f = fixture();
    fs::create_dir_all(&f.paths.started_dir).unwrap();
    let g = "33333333333333333333333333333333";
    fs::write(&f.paths.creating, g).unwrap();
    fs::write(f.paths.pair(), g).unwrap();
    assert_eq!(open_for_agent(&f.paths).unwrap().generation(), g);
    assert!(!detect(&f.paths).unwrap().creating, "마친 뒤 표식을 지운다");
    // 표식만 쓰고 끊긴 경우도 같은 G 로 이어 간다.
    let h = fixture();
    let g2 = "66666666666666666666666666666666";
    fs::write(&h.paths.creating, g2).unwrap();
    assert_eq!(open_for_agent(&h.paths).unwrap().generation(), g2);
    // 표식과 짝의 값이 다르면 거부한다(코덱스 r1p) — 어느 쪽 G 가 맞는지 모른다. 원장도 만들지 않는다.
    let m = fixture();
    fs::create_dir_all(&m.paths.started_dir).unwrap();
    fs::write(&m.paths.creating, g).unwrap();
    fs::write(m.paths.pair(), g2).unwrap();
    assert_err_contains(open_for_agent(&m.paths), "RUN_LEDGER_GENERATION_MISMATCH");
    assert!(
        !detect(&m.paths).unwrap().ledger,
        "값이 다르면 원장을 만들지 않는다"
    );
    // 코덱스 r1l ① — 표식 없이 짝만 있으면(시작 기록이 없어도) 쓰던 원장을 잃은 것이다.
    let i = fixture();
    fs::create_dir_all(&i.paths.started_dir).unwrap();
    fs::write(i.paths.pair(), g).unwrap();
    assert_err_contains(open_for_agent(&i.paths), "RUN_LEDGER_LOST");
    assert_err_contains(adopt_legacy_files(&i.paths, false), "RUN_LEDGER_LOST");
}

/// 코덱스 r1l ① — ACTIVE 행을 적은 뒤 시작 기록 전에 죽고 원장까지 잃으면 시작 기록은 비어 있다. 그래도 빈 원장으로 다시 만들지 않는다.
#[test]
fn a_lost_ledger_with_an_empty_start_journal_is_not_recreated() {
    let f = fixture();
    let mut ledger = open_for_agent(&f.paths).unwrap();
    ledger
        .insert_active(&AttemptRow::new_active(
            "maybe-running",
            "job",
            "node",
            1,
            Executor::Container,
        ))
        .unwrap();
    drop(ledger);
    fs::remove_file(&f.paths.ledger).unwrap();
    assert_err_contains(open_for_agent(&f.paths), "RUN_LEDGER_LOST");
    assert!(!detect(&f.paths).unwrap().ledger);
}

#[test]
fn a_creating_marker_left_after_the_ledger_was_published_is_removed_on_open() {
    let f = fixture();
    let g = open_for_agent(&f.paths).unwrap().generation().to_string();
    fs::write(&f.paths.creating, &g).unwrap();
    open_for_agent(&f.paths).unwrap();
    assert!(!detect(&f.paths).unwrap().creating);
    fs::write(&f.paths.creating, "77777777777777777777777777777777").unwrap();
    assert_err_contains(open_for_agent(&f.paths), "RUN_LEDGER_GENERATION_MISMATCH");
    assert_err_contains(adopt_legacy_files(&f.paths, false), "RUN_LEDGER_EXISTS");
}

#[test]
fn a_stale_temporary_ledger_is_removed_and_creation_continues() {
    let f = fixture();
    let parent = f.paths.ledger.parent().unwrap();
    let stale = parent.join(format!(
        "{}.tmp-deadbeefdeadbeefdeadbeefdeadbeef",
        f.paths.ledger.file_name().unwrap().to_str().unwrap()
    ));
    fs::write(&stale, b"half written").unwrap();
    open_for_agent(&f.paths).unwrap();
    assert!(!stale.exists());
}

// ---------- 해제 (R30b) ----------

#[test]
fn r30b_clear_refuses_a_root_that_enabled_the_ledger_but_lost_it() {
    let f = fixture();
    assert!(
        open_for_clear(&f.paths).unwrap().is_none(),
        "한 번도 켜지 않은 루트는 옛 동작"
    );

    let g = fixture();
    drop(open_for_agent(&g.paths).unwrap());
    assert!(open_for_clear(&g.paths).unwrap().is_some());
    fs::remove_file(&g.paths.ledger).unwrap();
    assert_err_contains(open_for_clear(&g.paths), "RUN_LEDGER_LOST");

    let h = fixture();
    fs::write(&h.paths.adopting, "44444444444444444444444444444444").unwrap();
    assert_err_contains(open_for_clear(&h.paths), "RUN_LEDGER_ADOPT_UNFINISHED");
    let _ = &h.root;
}

#[test]
fn r30c_clear_refuses_a_ledger_that_lost_a_row() {
    let f = fixture();
    drop(open_for_agent(&f.paths).unwrap());
    write_record(&f.paths.started_dir, "orphan");
    assert_err_contains(open_for_clear(&f.paths), "RUN_LEDGER_ROW_MISSING");
}

#[test]
fn a_fence_epoch_beyond_what_the_ledger_can_hold_is_refused_not_rewritten() {
    let f = fixture();
    let mut ledger = open_for_agent(&f.paths).unwrap();
    let too_big = u64::try_from(i64::MAX).unwrap() + 1;
    assert_err_contains(
        ledger.insert_active(&AttemptRow::new_active(
            "big",
            "job",
            "node",
            too_big,
            Executor::Host,
        )),
        "범위(i64)를 넘는다",
    );
    assert!(ledger.row("big").unwrap().is_none());
    let max = u64::try_from(i64::MAX).unwrap();
    ledger
        .insert_active(&AttemptRow::new_active(
            "edge",
            "job",
            "node",
            max,
            Executor::Host,
        ))
        .unwrap();
    assert_eq!(ledger.row("edge").unwrap().unwrap().fence_epoch, Some(max));
}

/// 코덱스 r1k ② — 종료 뒤 판정 사실(멈춤 · 로그 · 컨테이너 남김)이 원장 행에 남는다. ACTIVE 가 아니면 적지 않는다.
#[test]
fn judgement_facts_are_recorded_on_active_rows_only() {
    let f = fixture();
    let mut ledger = open_for_agent(&f.paths).unwrap();
    ledger
        .insert_active(&AttemptRow::new_active(
            "c",
            "job",
            "node",
            1,
            Executor::Container,
        ))
        .unwrap();
    ledger
        .record_facts("c", Some(true), Some(false), Some(true))
        .unwrap();
    let row = ledger.row("c").unwrap().unwrap();
    assert_eq!(
        (row.stopped, row.logs_complete, row.container_left),
        (Some(true), Some(false), Some(true))
    );
    ledger.mark_local_blocked("c", "container_left").unwrap();
    assert_err_contains(ledger.record_facts("c", None, None, None), "ACTIVE 행에만");
}
