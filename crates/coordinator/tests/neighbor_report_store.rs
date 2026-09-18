//! `CoordinatorNeighborReportStore` 실측.
//!
//! `ADR-033` §7 의 "Broker 가 그 보고를 **모아**" 에서 모아 두는 자리가
//! 실제로 재시작을 넘고, 판정을 하지 않고, 남의 Coordinator 앞으로 온
//! 신고를 받지 않는지 본다.

use gputeer_coordinator::neighbor_report_store::{
    CoordinatorNeighborReportStore, NeighborReportCorruption, NeighborReportStoreError,
    RecordOutcome, StoredNeighborReport,
};
use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
use gputeer_protocol::{
    pb,
    signing::{verify, NoReplayCheck, Verified},
};

const OURS: &str = "01JCOORDNEIGHBOR00000000001";
const THEIRS: &str = "01JCOORDNEIGHBOR00000000002";
const TARGET: &str = "01JNODETARGET0000000000001";

fn seed(label: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(label.as_bytes());
    *hasher.finalize().as_bytes()
}

/// 실제로 서명해 `Verified` 를 만든다.
///
/// ★ 검증을 우회하는 지름길을 만들지 않는다 — 저장소가 `Verified` 만
///   받는다는 계약이 테스트에서 뚫리면 그 계약이 실제로 지켜지는지
///   아무도 모른다(`node_liveness_store.rs` 와 같은 원칙).
fn verified_report(
    reporter_node_id: &str,
    reporter_device_id: &str,
    unreachable_node_id: &str,
    coordinator_device_id: &str,
    observed_at_unix_ms: u64,
) -> Verified<pb::NeighborUnreachableReport> {
    let key = SigningKey::from_bytes(&seed(reporter_device_id));
    let mut report = pb::NeighborUnreachableReport {
        schema_version: 1,
        reporter_node_id: reporter_node_id.to_string(),
        reporter_device_id: reporter_device_id.to_string(),
        unreachable_node_id: unreachable_node_id.to_string(),
        coordinator_device_id: coordinator_device_id.to_string(),
        observed_at_unix_ms,
        request_nonce: vec![9u8; 16],
        ..Default::default()
    };
    report.reporter_signature = sign(&key, &report).to_vec();

    let mut keys = InMemoryKeyring::new();
    keys.insert(reporter_device_id, key.verifying_key());
    verify(
        &report,
        1,
        &Ed25519Verifier::new(keys),
        observed_at_unix_ms.max(1),
        &mut NoReplayCheck,
    )
    .expect("테스트 신고 서명은 검증돼야 한다")
}

fn reporter_id(neighbor: &str) -> String {
    format!("01JNODE{:0>19}", neighbor)
}

fn reporter_device(neighbor: &str) -> String {
    format!("01JDEV{:0>20}", neighbor)
}

/// 기본 신고 — 이 Coordinator 앞으로, 지정한 이웃이 `TARGET` 을 지목.
fn report_from(neighbor: &str, observed_at_unix_ms: u64) -> Verified<pb::NeighborUnreachableReport> {
    verified_report(
        &reporter_id(neighbor),
        &reporter_device(neighbor),
        TARGET,
        OURS,
        observed_at_unix_ms,
    )
}

/// 손상 오류를 **식별 필드까지** 대조한다.
///
/// ★ `matches!(..., Corrupt { kind, .. })` 는 잘못된 행 ID 를 담은 오류도
///   통과시킨다(독립 검수 6라운드 지적) — 어느 행이 손상됐는지 잘못 전하면
///   운영자가 엉뚱한 곳을 본다(`CLAUDE.md` §3).
fn assert_corrupt(
    error: NeighborReportStoreError,
    reporter_node_id: &str,
    unreachable_node_id: &str,
    kind: NeighborReportCorruption,
) {
    assert_eq!(
        error,
        NeighborReportStoreError::Corrupt {
            reporter_node_id: reporter_node_id.to_string(),
            unreachable_node_id: unreachable_node_id.to_string(),
            kind,
        }
    );
}

/// 상한을 **넘는** 행을 직접 심는다 — 이 API 로는 만들 수 없는 상태를
/// 재현하려면 필요하다. 몸통·해시·인덱스가 전부 맞는 **정상** 행이어야
/// 한다(손상으로 거부되면 복구 경로를 못 본다).
fn seed_rows_directly(path: &std::path::Path, machine: &str, device: &str, count: u64) {
    use prost::Message;

    // 스키마를 먼저 만든다.
    drop(CoordinatorNeighborReportStore::open(path, OURS).expect("열기"));
    let connection = rusqlite::Connection::open(path).expect("직접 열기");
    for i in 0..count {
        let target = format!("01JNODEOVER{:0>15}", i);
        let report = verified_report(machine, device, &target, OURS, 1_000 + i)
            .get()
            .clone();
        let body = report.encode_to_vec();
        let hash = blake3::hash(&body);
        connection
            .execute(
                "INSERT INTO coordinator_neighbor_reports(
                    reporter_node_id, unreachable_node_id, reporter_device_id,
                    observed_at_unix_ms, report_hash, report_body
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    machine,
                    target,
                    device,
                    (1_000u64 + i).to_be_bytes().to_vec(),
                    hash.as_bytes().as_slice(),
                    body,
                ],
            )
            .expect("심기");
    }
}

/// 상한 오류를 **전 필드** 대조한다.
///
/// ★ `..` 로 일부를 버리면 `reporter_device_id`·`stored_rows`·`limit` 가
///   틀리게 돌아와도 통과한다(독립 검수 7라운드 지적).
fn assert_quota_exhausted(
    error: NeighborReportStoreError,
    reporter_device_id: &str,
    stored_rows: i64,
    blocking_observed_at_unix_ms: u64,
) {
    assert_eq!(
        error,
        NeighborReportStoreError::ReporterDeviceQuotaExhausted {
            reporter_device_id: reporter_device_id.to_string(),
            stored_rows,
            limit: 64,
            blocking_observed_at_unix_ms,
        }
    );
}

/// 정상 행 **한 건**을 직접 심는다 — 이 API 로는 만들 수 없는 조합
/// (한 기계에 서로 다른 장치의 행이 섞여 있는 상태 등)을 재현할 때 쓴다.
fn seed_one_row(
    path: &std::path::Path,
    reporter_node_id: &str,
    reporter_device_id: &str,
    unreachable_node_id: &str,
    observed_at_unix_ms: u64,
) {
    use prost::Message;

    let connection = rusqlite::Connection::open(path).expect("직접 열기");
    let report = verified_report(
        reporter_node_id,
        reporter_device_id,
        unreachable_node_id,
        OURS,
        observed_at_unix_ms,
    )
    .get()
    .clone();
    let body = report.encode_to_vec();
    connection
        .execute(
            "INSERT INTO coordinator_neighbor_reports(
                reporter_node_id, unreachable_node_id, reporter_device_id,
                observed_at_unix_ms, report_hash, report_body
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                reporter_node_id,
                unreachable_node_id,
                reporter_device_id,
                observed_at_unix_ms.to_be_bytes().to_vec(),
                blake3::hash(&body).as_bytes().as_slice(),
                body,
            ],
        )
        .expect("심기");
}

fn store(dir: &tempfile::TempDir, name: &str) -> CoordinatorNeighborReportStore {
    CoordinatorNeighborReportStore::open(dir.path().join(name), OURS).expect("저장소 열기")
}

/// 저장소 **전체**를 **모든 컬럼**으로 찍는다.
///
/// ★ 거부 경로가 "오류를 내면서 다른 행을 지우거나 고치는" 것을 잡으려면
///   한두 행이 아니라 전체를 전후 대조해야 한다(독립 검수 3라운드 지적).
///
/// ★ 그리고 **일부 컬럼만 비교하면 안 된다** — 몸통·해시·장치를 바꿔치기해도
///   통과한다(4라운드 지적). 여섯 컬럼 전부를 싣는다.
type SnapshotRow = (String, String, String, Vec<u8>, Vec<u8>, Vec<u8>);

/// ★ 저장소가 **무엇을 담고 있어야 하는가**를 테스트 입력만으로 만든다.
///
/// 이전에는 `get_report()` 로 읽은 값을 다시 `reports_about()` 의 기대값으로
/// 썼는데, 그러면 **처음부터 잘못 저장돼도 두 조회가 사이좋게 같은 값을
/// 내놓아 통과한다**(독립 검수 8라운드 지적). 기대값은 저장소를 거치지 않고
/// 나와야 한다.
fn expected_row(
    reporter_node_id: &str,
    reporter_device_id: &str,
    unreachable_node_id: &str,
    observed_at_unix_ms: u64,
) -> StoredNeighborReport {
    use prost::Message;

    let report = verified_report(
        reporter_node_id,
        reporter_device_id,
        unreachable_node_id,
        OURS,
        observed_at_unix_ms,
    )
    .get()
    .clone();
    let body = report.encode_to_vec();
    StoredNeighborReport {
        reporter_node_id: reporter_node_id.to_string(),
        reporter_device_id: reporter_device_id.to_string(),
        unreachable_node_id: unreachable_node_id.to_string(),
        observed_at_unix_ms,
        report_hash: *blake3::hash(&body).as_bytes(),
        report,
    }
}

/// 이웃 별칭(`"a"`)으로 기대 행을 만든다.
fn expected_from(neighbor: &str, observed_at_unix_ms: u64) -> StoredNeighborReport {
    expected_row(
        &reporter_id(neighbor),
        &reporter_device(neighbor),
        TARGET,
        observed_at_unix_ms,
    )
}

fn current_row(
    s: &CoordinatorNeighborReportStore,
    reporter: &str,
    target: &str,
) -> StoredNeighborReport {
    s.get_report(reporter, target).expect("읽기").expect("행이 있어야 한다")
}

fn snapshot(path: &std::path::Path) -> Vec<SnapshotRow> {
    let connection = rusqlite::Connection::open(path).expect("직접 열기");
    let mut statement = connection
        .prepare(
            "SELECT reporter_node_id, unreachable_node_id, reporter_device_id,
                    observed_at_unix_ms, report_hash, report_body
             FROM coordinator_neighbor_reports
             ORDER BY reporter_node_id, unreachable_node_id",
        )
        .expect("준비");
    let rows = statement
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .expect("조회");
    rows.map(|r| r.expect("행")).collect()
}

// ─────────────────────────────────────────────────────────────────────
// 이 조각의 존재 이유
// ─────────────────────────────────────────────────────────────────────

/// 신고가 실제로 남고 재시작 뒤에도 읽히는가.
///
/// ★ 메모리에만 있으면 Coordinator 가 재시작하는 순간 여러 이웃의
///   관측이 전부 사라진다 — §8 조건 3 은 그걸 모아야 성립한다.
#[test]
fn a_report_survives_a_restart() {
    let dir = tempfile::tempdir().expect("temp");
    {
        let mut s = store(&dir, "neighbors.db");
        let outcome = s.record_verified_report(&report_from("a", 5_000)).expect("기록");
        // ★ variant 만 보면 반환 payload 가 조작돼도 통과한다(6라운드 지적).
        match outcome {
            RecordOutcome::Recorded { stored, evicted } => {
                assert!(evicted.is_empty(), "빈 저장소에서 밀어낼 것이 있을 리 없다");
                let from_db = s
                    .get_report(&reporter_id("a"), TARGET)
                    .expect("읽기")
                    .expect("행이 있어야 한다");
                assert_eq!(stored, from_db, "반환값이 저장된 행과 다르다");
            }
            other => panic!("{other:?}"),
        }
    }
    // 완전히 새 연결 — 프로세스 재시작을 흉내낸다.
    let s = store(&dir, "neighbors.db");
    // ★ 재시작 뒤 읽은 행이 **입력으로 만든 기대 행과 전부** 같은가.
    assert_eq!(
        s.reports_about(TARGET).expect("읽기"),
        vec![expected_from("a", 5_000)]
    );
}

/// ★ **이 저장소가 실제로 닫는 구멍.**
///
/// `DoD-63` 이 "프레이밍 계층은 `coordinator_device_id` 를 자기 ID 와
/// 대조하지 않는다 — 소비자가 반드시 대조해야 한다" 고 열어 뒀다.
/// 이 저장소가 그 첫 소비자이고, 여기서 대조한다.
///
/// 대조하지 않으면 A 에게 보낸 신고를 B 가 자기 판정에 쓸 수 있다.
#[test]
fn a_report_addressed_to_another_coordinator_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let elsewhere = verified_report(
        "01JNODEELSEWHERE000000001",
        "01JDEVELSEWHERE0000000001",
        TARGET,
        THEIRS, // ★ 우리 앞으로 온 게 아니다
        5_000,
    );
    let error = s.record_verified_report(&elsewhere).expect_err("거부돼야 한다");
    assert_eq!(
        error,
        NeighborReportStoreError::AddressedToAnotherCoordinator {
            addressed: THEIRS.to_string(),
            ours: OURS.to_string(),
        }
    );
    // 행이 하나도 안 생겼는지 확인 — "거부했다" 는 반환값만으로는 부족하다.
    assert!(s.reports_about(TARGET).expect("읽기").is_empty());
}

// ─────────────────────────────────────────────────────────────────────
// 뒤로 가지 않는다
// ─────────────────────────────────────────────────────────────────────

#[test]
fn a_newer_observation_replaces_the_older_one() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    s.record_verified_report(&report_from("a", 1_000)).expect("첫 기록");
    let outcome = s.record_verified_report(&report_from("a", 2_000)).expect("갱신");
    match outcome {
        RecordOutcome::Recorded { stored, evicted } => {
            assert!(evicted.is_empty(), "갱신은 아무것도 밀어내지 않는다");
            // ★ 기대값을 **입력에서** 만든다 — DB 에서 읽은 값을 기대값으로
            //   쓰면 저장과 조회가 함께 틀렸을 때 통과한다(9라운드 지적).
            assert_eq!(stored, expected_from("a", 2_000));
        }
        other => panic!("갱신돼야 한다: {other:?}"),
    }
    assert_eq!(
        s.reports_about(TARGET).expect("읽기"),
        vec![expected_from("a", 2_000)]
    );
}

/// ★ 지연·재전송으로 옛 관측이 늦게 도착해도 최신 값을 밀어내지 않는다.
///
/// 밀어내면 이미 복구된 노드가 다시 연락 두절로 보인다.
#[test]
fn an_older_observation_never_overwrites_a_newer_one() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    s.record_verified_report(&report_from("a", 2_000)).expect("첫 기록");
    let outcome = s.record_verified_report(&report_from("a", 1_000)).expect("옛 관측");
    match outcome {
        RecordOutcome::NotNewer(stored) => {
            // ★ 기대값을 입력에서 만든다(9라운드 지적).
            assert_eq!(stored, expected_from("a", 2_000));
        }
        other => panic!("밀어내면 안 된다: {other:?}"),
    }
    // 저장된 값 자체를 다시 읽어 확인한다 — 반환값만 보면 DB 가 바뀌어도 모른다.
    assert_eq!(
        s.reports_about(TARGET).expect("읽기"),
        vec![expected_from("a", 2_000)]
    );
}

/// 같은 시각의 재전송도 덮어쓰지 않는다(`<=` 경계).
///
/// ★ 반환값만 보면 공허하다(독립 검수 1라운드 지적) — 구현이
///   `NotNewer` 를 돌려주면서 뒤로 행을 바꿔도 통과한다. 그래서 저장된
///   행 전체를 다시 읽어 대조한다.
#[test]
fn the_same_observation_time_is_not_a_new_observation() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let first = expected_from("a", 2_000);
    match s.record_verified_report(&report_from("a", 2_000)).expect("첫 기록") {
        RecordOutcome::Recorded { stored, evicted } => {
            assert!(evicted.is_empty());
            assert_eq!(stored, first);
        }
        other => panic!("{other:?}"),
    }

    let outcome = s.record_verified_report(&report_from("a", 2_000)).expect("재전송");
    match outcome {
        // ★ variant 만 보면 payload 가 조작돼도 통과한다(5라운드 지적).
        RecordOutcome::NotNewer(stored) => assert_eq!(stored, first, "반환값이 최초 기록과 달라졌다"),
        other => panic!("덮어쓰면 안 된다: {other:?}"),
    }

    // 저장된 행이 최초 기록 그대로인가 — 반환값이 아니라 DB 를 본다.
    assert_eq!(s.reports_about(TARGET).expect("읽기"), vec![first]);
}

// ─────────────────────────────────────────────────────────────────────
// 여러 이웃을 모은다 — 그러나 정족수를 세지는 않는다
// ─────────────────────────────────────────────────────────────────────

#[test]
fn reports_from_different_neighbors_about_one_node_are_all_kept() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    for (neighbor, at) in [("c", 3_000), ("a", 1_000), ("b", 2_000)] {
        s.record_verified_report(&report_from(neighbor, at)).expect("기록");
    }
    // ★ 개수와 정렬만 보면 대상·시각·해시·원본이 틀려도 통과한다
    //   (7라운드 지적) — 예상 집합 **전체**와 대조한다.
    let expected = vec![
        expected_from("a", 1_000),
        expected_from("b", 2_000),
        expected_from("c", 3_000),
    ];
    assert_eq!(s.reports_about(TARGET).expect("읽기"), expected);
}

/// 한 이웃이 서로 다른 노드를 지목하면 별개의 행이다.
#[test]
fn one_neighbor_can_report_about_several_nodes() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let other_target = "01JNODETARGET0000000000002";
    s.record_verified_report(&report_from("a", 1_000)).expect("기록");
    s.record_verified_report(&verified_report(
        "01JNODE0000000000000000a",
        "01JDEV00000000000000000a",
        other_target,
        OURS,
        1_000,
    ))
    .expect("기록");

    // 개수가 아니라 내용으로 대조한다(7라운드 지적).
    assert_eq!(
        s.reports_about(TARGET).expect("읽기"),
        vec![expected_from("a", 1_000)]
    );
    assert_eq!(
        s.reports_about(other_target).expect("읽기"),
        vec![expected_row(
            "01JNODE0000000000000000a",
            "01JDEV00000000000000000a",
            other_target,
            1_000
        )]
    );
}

/// ★ **닫지 못한 구멍을 통과하는 테스트로 고정한다.**
///
/// 서명은 **장치**를 인증할 뿐, 그 장치가 주장한 기계 ID 의 실제
/// 소유자인지는 인증하지 않는다. 그래서 장치 하나가 서로 다른 기계
/// ID 를 주장하면 행이 두 개 생긴다 — §8 조건 3 의 정족수가 기계
/// 수로 세어지므로 이건 실제 위험이다.
///
/// 이 저장소는 그것을 **막지 못한다.** 대신 각 행에
/// `reporter_device_id` 를 실어 보내 호출부가 볼 수 있게 한다.
///
/// ★ 이 테스트가 **통과하는 동안은 구멍이 열려 있다는 뜻**이고,
///   멤버십 해소가 생겨 닫히면 이 테스트가 실패하며 문서도 같이
///   고치라고 알린다(`runtime-linux` 의 "탈출이 성공하기를 기대하는
///   테스트" 와 같은 장치).
#[test]
fn one_device_can_still_claim_two_machines_and_the_store_makes_that_visible() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let one_device = "01JDEVONEKEYTWOMACHINES01";

    for machine in ["01JNODECLAIMED0000000001", "01JNODECLAIMED0000000002"] {
        s.record_verified_report(&verified_report(machine, one_device, TARGET, OURS, 1_000))
            .expect("같은 장치가 두 기계를 주장해도 지금은 통과한다");
    }

    // 행은 둘이다 — 기계 수로 세면 두 표로 보인다.
    let expected: Vec<_> = ["01JNODECLAIMED0000000001", "01JNODECLAIMED0000000002"]
        .iter()
        .map(|m| expected_row(m, one_device, TARGET, 1_000))
        .collect();
    // ★ 위 대조가 이미 각 행의 `reporter_device_id` 를 포함한다 — 방금 만든
    //   `expected` 를 다시 순회하는 단언은 공허하다(9라운드 지적).
    // ★ 이 대조가 각 행의 `reporter_device_id` 를 이미 포함한다 — 뒤에
    //   장치만 다시 훑는 단언은 논리적으로 중복이다(10라운드 지적).
    //   `expected` 를 `one_device` 로 만들었으므로, 이 대조가 통과한다는 것이
    //   곧 "두 기계 행이 같은 장치의 것" 이라는 뜻이다.
    assert_eq!(s.reports_about(TARGET).expect("읽기"), expected);
}

/// ★ **다른 대상으로도 남의 기계 ID 를 주장할 수 없다.**
///
/// 장치 결합 검사가 `(기계, 대상)` 쌍에만 걸리면, 같은 기계 ID 를 **아직
/// 신고되지 않은 다른 대상**으로 신고해서 두 장치가 그 기계를 동시에 주장할
/// 수 있다(독립 검수 9라운드가 잡은 결함). 정족수 단위가 기계이므로
/// 기계→장치 결합은 대상과 무관하게 하나여야 한다.
#[test]
fn another_device_cannot_claim_a_machine_through_a_different_target() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let machine = "01JNODESQUATTED00000001";
    let first_device = "01JDEVLEGIT000000000001";
    let other_device = "01JDEVSQUATTER0000000001";
    let first_target = "01JNODETARGETONE00000001";
    let other_target = "01JNODETARGETTWO00000001";

    s.record_verified_report(&verified_report(machine, first_device, first_target, OURS, 1_000))
        .expect("정당한 신고자의 첫 기록");

    // ★ 대상이 다르므로 쌍 단위 검사로는 걸리지 않는다.
    let error = s
        .record_verified_report(&verified_report(
            machine,
            other_device,
            other_target,
            OURS,
            2_000,
        ))
        .expect_err("다른 대상이어도 기계 결합은 지켜져야 한다");
    assert_eq!(
        error,
        NeighborReportStoreError::ConflictingReporterDevice {
            reporter_node_id: machine.to_string(),
            stored_device_id: first_device.to_string(),
            incoming_device_id: other_device.to_string(),
        }
    );
    assert!(s.get_report(machine, other_target).expect("읽기").is_none());
    // 원래 행은 그대로다.
    assert_eq!(
        current_row(&s, machine, first_target),
        expected_row(machine, first_device, first_target, 1_000)
    );
}

/// ★ **이미 섞여 있는 상태를 첫 행만 보고 통과시키지 않는다.**
///
/// 결합 조회가 `LIMIT 1` 이면, 한 기계에 장치 A·B 행이 이미 함께 있을 때
/// 정렬상 첫 행과 같은 장치의 신고가 **다른 충돌 행을 남긴 채** 통과한다
/// (독립 검수 10라운드가 잡은 결함). "어떤 행이든" 을 주장하려면 전부 봐야
/// 한다.
#[test]
fn a_machine_with_mixed_devices_is_refused_even_for_the_first_sorted_device() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let machine = "01JNODEMIXED00000000001";
    let device_a = "01JDEVMIXEDAAAAAAAAAAA01";
    let device_b = "01JDEVMIXEDBBBBBBBBBBB01";
    let target_a = "01JNODETARGETAAAAAAAAA01";
    let target_b = "01JNODETARGETBBBBBBBBB01";

    // 스키마를 만든 뒤, 한 기계에 두 장치의 행을 섞어 심는다.
    drop(CoordinatorNeighborReportStore::open(&path, OURS).expect("열기"));
    seed_one_row(&path, machine, device_a, target_a, 1_000);
    seed_one_row(&path, machine, device_b, target_b, 1_100);

    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let before = snapshot(&path);

    // ★ 정렬상 첫 행(`target_a`)의 장치로 새 신고를 넣는다 — `LIMIT 1` 이면
    //   통과하지만, 전수로 보면 `device_b` 행과 충돌한다.
    let error = s
        .record_verified_report(&verified_report(
            machine,
            device_a,
            "01JNODETARGETNEW00000001",
            OURS,
            2_000,
        ))
        .expect_err("섞여 있는 상태는 거부돼야 한다");
    assert_eq!(
        error,
        NeighborReportStoreError::ConflictingReporterDevice {
            reporter_node_id: machine.to_string(),
            stored_device_id: device_b.to_string(),
            incoming_device_id: device_a.to_string(),
        }
    );
    assert_eq!(snapshot(&path), before, "거부했는데 DB 가 바뀌었다");
}

/// ★ **손상이 충돌로 가려지지 않는다.**
///
/// 결합 검사를 손상 검사보다 먼저 하면, 장치 컬럼과 몸통이 어긋난 행에서
/// 정당한 장치가 재보고할 때 그 손상이 `ConflictingReporterDevice` 로
/// 보고된다(독립 검수 10라운드 지적). 원인이 다르면 대응도 다르다.
#[test]
fn a_corrupted_row_is_reported_as_corruption_not_as_a_device_conflict() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let machine = "01JNODEMASKED0000000001";
    let device = "01JDEVMASKED00000000001";
    let target = "01JNODETARGETMASK0000001";

    drop(CoordinatorNeighborReportStore::open(&path, OURS).expect("열기"));
    seed_one_row(&path, machine, device, target, 1_000);
    // 장치 **컬럼만** 바꾼다 — 몸통은 원래 장치 그대로다.
    tamper(
        &path,
        "UPDATE coordinator_neighbor_reports SET reporter_device_id = '01JDEVOTHER00000000000001'",
    );

    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let error = s
        .record_verified_report(&verified_report(
            machine,
            device,
            "01JNODETARGETNEW00000001",
            OURS,
            2_000,
        ))
        .expect_err("손상이므로 거부돼야 한다");
    // ★ 충돌이 아니라 **손상**으로 보고돼야 한다.
    assert_corrupt(
        error,
        machine,
        target,
        NeighborReportCorruption::ReporterDeviceMismatch,
    );
}

/// 같은 기계 ID 에 대해 **다른 장치**가 덮어쓰려 하면 거부한다.
///
/// 어느 쪽이 그 기계의 진짜 장치인지 저장소는 모른다 — 추측하면
/// 조작된 신고가 정당한 신고를 밀어낼 수 있다.
#[test]
fn a_different_device_cannot_take_over_an_existing_reporter_machine() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let machine = "01JNODECONTESTED00000001";
    s.record_verified_report(&verified_report(
        machine,
        "01JDEVFIRST000000000001",
        TARGET,
        OURS,
        1_000,
    ))
    .expect("첫 기록");

    let error = s
        .record_verified_report(&verified_report(
            machine,
            "01JDEVSECOND00000000001",
            TARGET,
            OURS,
            9_000, // 더 새로워도 소용없다
        ))
        .expect_err("거부돼야 한다");
    assert_eq!(
        error,
        NeighborReportStoreError::ConflictingReporterDevice {
            reporter_node_id: machine.to_string(),
            stored_device_id: "01JDEVFIRST000000000001".to_string(),
            incoming_device_id: "01JDEVSECOND00000000001".to_string(),
        }
    );
    // 기존 행이 **전부** 그대로인지 다시 읽어 확인한다.
    assert_eq!(
        s.reports_about(TARGET).expect("읽기"),
        vec![expected_row(machine, "01JDEVFIRST000000000001", TARGET, 1_000)]
    );
}

// ─────────────────────────────────────────────────────────────────────
// 입력 거부
// ─────────────────────────────────────────────────────────────────────

/// 자기 자신을 연락 두절이라고 신고하는 것은 관측이 아니다 —
/// 신고를 보냈다는 사실과 모순된다.
#[test]
fn a_node_cannot_report_itself_as_unreachable() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let same = "01JNODESELFREPORT0000001";
    let error = s
        .record_verified_report(&verified_report(
            same,
            "01JDEVSELFREPORT00000001",
            same,
            OURS,
            1_000,
        ))
        .expect_err("거부돼야 한다");
    assert_eq!(
        error,
        NeighborReportStoreError::InvalidInput("reporter_node_id == unreachable_node_id")
    );
    // ★ 오류를 돌려주면서 행은 쓰는 구현을 잡으려면 DB 를 봐야 한다.
    assert!(s.reports_about(same).expect("읽기").is_empty());
}

#[test]
fn each_empty_identity_field_is_refused_on_its_own() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");

    let cases: [(&str, pb::NeighborUnreachableReport); 4] = [
        ("reporter_node_id", {
            let mut r = report_from("a", 1_000).get().clone();
            r.reporter_node_id = "   ".to_string();
            r
        }),
        ("reporter_device_id", {
            let mut r = report_from("a", 1_000).get().clone();
            r.reporter_device_id = String::new();
            r
        }),
        ("unreachable_node_id", {
            let mut r = report_from("a", 1_000).get().clone();
            r.unreachable_node_id = String::new();
            r
        }),
        ("coordinator_device_id", {
            let mut r = report_from("a", 1_000).get().clone();
            r.coordinator_device_id = String::new();
            r
        }),
    ];

    for (field, mut tampered) in cases {
        // 바뀐 몸통으로 다시 서명해야 `Verified` 를 만들 수 있다 —
        // 서명 계층을 우회하지 않는다.
        let key = SigningKey::from_bytes(&seed(&tampered.reporter_device_id));
        tampered.reporter_signature = sign(&key, &tampered).to_vec();
        let mut keys = InMemoryKeyring::new();
        keys.insert(&tampered.reporter_device_id, key.verifying_key());
        let verified = verify(
            &tampered,
            1,
            &Ed25519Verifier::new(keys),
            1_000,
            &mut NoReplayCheck,
        )
        .expect("서명 자체는 유효하다");

        let error = s.record_verified_report(&verified).expect_err("거부돼야 한다");
        assert_eq!(
            error,
            NeighborReportStoreError::InvalidInput(match field {
                "reporter_node_id" => "reporter_node_id",
                "reporter_device_id" => "reporter_device_id",
                "unreachable_node_id" => "unreachable_node_id",
                _ => "coordinator_device_id",
            }),
            "{field} 는 그 필드 이름으로 거부돼야 한다"
        );
    }

    // ★ 네 번 다 거부했는데 행이 하나라도 생겼는가 — DB 를 직접 센다.
    //   오류를 돌려주면서 쓰기도 하는 구현은 반환값만으로 안 잡힌다.
    let connection = rusqlite::Connection::open(dir.path().join("neighbors.db"))
        .expect("직접 열기");
    let rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM coordinator_neighbor_reports", [], |r| r.get(0))
        .expect("세기");
    assert_eq!(rows, 0, "거부된 신고가 행을 남겼다");
}

#[test]
fn opening_without_a_coordinator_identity_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let error = CoordinatorNeighborReportStore::open(dir.path().join("x.db"), "  ")
        .err()
        .expect("거부돼야 한다");
    assert_eq!(
        error,
        NeighborReportStoreError::InvalidInput("coordinator_device_id")
    );
}

/// `:memory:` 는 재시작을 넘지 못하므로 durable 이 아니다.
#[test]
fn an_in_memory_store_is_not_durable() {
    let memory = CoordinatorNeighborReportStore::open(":memory:", OURS).expect("열기");
    assert!(!memory.is_durable());

    let dir = tempfile::tempdir().expect("temp");
    assert!(store(&dir, "neighbors.db").is_durable());
}

// ─────────────────────────────────────────────────────────────────────
// 손상은 조용히 통과하지 않는다
// ─────────────────────────────────────────────────────────────────────

fn tamper(path: &std::path::Path, sql: &str) {
    let connection = rusqlite::Connection::open(path).expect("직접 열기");
    connection.execute(sql, []).expect("변조");
}

#[test]
fn a_body_that_disagrees_with_its_index_columns_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    {
        let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
        s.record_verified_report(&report_from("a", 1_000)).expect("기록");
    }
    // 몸통은 그대로 두고 인덱스 컬럼만 바꾼다 — 어느 쪽이 진실인지
    // 모른 채 판정에 들어가면 안 된다.
    tamper(
        &path,
        "UPDATE coordinator_neighbor_reports SET unreachable_node_id = 'somewhere-else'",
    );
    let s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let error = s.reports_about("somewhere-else").expect_err("거부돼야 한다");
    assert_corrupt(
        error,
        &reporter_id("a"),
        "somewhere-else",
        NeighborReportCorruption::UnreachableNodeMismatch,
    );
}

#[test]
fn a_body_whose_hash_no_longer_matches_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    {
        let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
        s.record_verified_report(&report_from("a", 1_000)).expect("기록");
    }
    tamper(
        &path,
        "UPDATE coordinator_neighbor_reports SET report_body = X'00'",
    );
    let s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let error = s.reports_about(TARGET).expect_err("거부돼야 한다");
    assert_corrupt(error, &reporter_id("a"), TARGET, NeighborReportCorruption::HashMismatch);
}

#[test]
fn a_stored_observation_time_that_disagrees_with_the_body_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    {
        let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
        s.record_verified_report(&report_from("a", 1_000)).expect("기록");
    }
    // 8바이트 형식은 유지한 채 값만 바꾼다 — 인코딩 오류가 아니라
    // 몸통과의 불일치로 잡혀야 한다.
    tamper(
        &path,
        "UPDATE coordinator_neighbor_reports SET observed_at_unix_ms = X'0000000000000063'",
    );
    let s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let error = s.reports_about(TARGET).expect_err("거부돼야 한다");
    assert_corrupt(
        error,
        &reporter_id("a"),
        TARGET,
        NeighborReportCorruption::ObservedAtEncoding,
    );
}

#[test]
fn a_stored_device_that_disagrees_with_the_body_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    {
        let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
        s.record_verified_report(&report_from("a", 1_000)).expect("기록");
    }
    tamper(
        &path,
        "UPDATE coordinator_neighbor_reports SET reporter_device_id = 'someone-else'",
    );
    let s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let error = s.reports_about(TARGET).expect_err("거부돼야 한다");
    assert_corrupt(
        error,
        &reporter_id("a"),
        TARGET,
        NeighborReportCorruption::ReporterDeviceMismatch,
    );
}

/// 손상된 행은 **한 건 조회에서도** 잡힌다 — 목록 경로만 검사하면
/// `get_report()` 로 우회할 수 있다.
#[test]
fn corruption_is_caught_on_the_single_row_path_too() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let reporter = {
        let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
        match s.record_verified_report(&report_from("a", 1_000)).expect("기록") {
            RecordOutcome::Recorded { stored, evicted } => {
                assert!(evicted.is_empty(), "빈 저장소에서 밀어낼 것이 없다");
                assert_eq!(stored, expected_from("a", 1_000));
                stored.reporter_node_id
            }
            other => panic!("{other:?}"),
        }
    };
    tamper(
        &path,
        "UPDATE coordinator_neighbor_reports SET report_body = X'00'",
    );
    let s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let error = s.get_report(&reporter, TARGET).expect_err("거부돼야 한다");
    assert_corrupt(error, &reporter, TARGET, NeighborReportCorruption::HashMismatch);
}

/// 없는 것을 물으면 오류가 아니라 빈 결과다.
#[test]
fn asking_about_a_node_nobody_reported_is_not_an_error() {
    let dir = tempfile::tempdir().expect("temp");
    let s = store(&dir, "neighbors.db");
    assert!(s.reports_about(TARGET).expect("읽기").is_empty());
    assert!(s.get_report("01JNODENOBODY00000000001", TARGET).expect("읽기").is_none());
}

// ─────────────────────────────────────────────────────────────────────
// 자체 재검토가 찾은 결함 두 건 — 고정한다
// ─────────────────────────────────────────────────────────────────────

/// ★ **쓰기 경로의 수신자 대조만으로는 부족하다.**
///
/// 이미 행이 들어 있는 파일을 다른 Coordinator 가 열면(파일 복사·이관·
/// 설정 실수) 남에게 보낸 신고를 자기 것으로 읽게 된다. 검사 지점이
/// 하나뿐이면 그 지점을 지나지 않는 경로가 생기는 순간 방어가 사라진다.
#[test]
fn rows_written_for_another_coordinator_are_refused_on_read_too() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let reporter = {
        // OURS 앞으로 정상적으로 기록한다.
        let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
        match s.record_verified_report(&report_from("a", 1_000)).expect("기록") {
            RecordOutcome::Recorded { stored, evicted } => {
                assert!(evicted.is_empty(), "빈 저장소에서 밀어낼 것이 없다");
                assert_eq!(stored, expected_from("a", 1_000));
                stored.reporter_node_id
            }
            other => panic!("{other:?}"),
        }
    };

    // 같은 파일을 **다른** Coordinator 가 연다.
    let theirs = CoordinatorNeighborReportStore::open(&path, THEIRS).expect("열기");

    let error = theirs.reports_about(TARGET).expect_err("목록에서 거부돼야 한다");
    assert_eq!(
        error,
        NeighborReportStoreError::AddressedToAnotherCoordinator {
            addressed: OURS.to_string(),
            ours: THEIRS.to_string(),
        }
    );

    // 한 건 조회로도 우회할 수 없다.
    let error = theirs
        .get_report(&reporter, TARGET)
        .expect_err("단건 조회에서도 거부돼야 한다");
    // ★ `..` 로 두 필드를 버리면 addressed/ours 가 뒤바뀌어도 통과한다
    //   (5라운드 지적) — 목록 경로와 같은 수준으로 대조한다.
    assert_eq!(
        error,
        NeighborReportStoreError::AddressedToAnotherCoordinator {
            addressed: OURS.to_string(),
            ours: THEIRS.to_string(),
        }
    );

    // 원래 주인은 그대로 읽는다 — 방어가 과하지 않은지 대조한다.
    let ours = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    assert_eq!(
        ours.reports_about(TARGET).expect("읽기"),
        vec![expected_from("a", 1_000)]
    );
}

/// ★ **장치 하나가 저장소를 무한히 키우지 못한다.**
///
/// 이 저장소는 신고자가 진짜 이웃인지, 지목된 노드가 실재하는지 **둘 다
/// 확인하지 못한다.** 그래서 장치 하나가 대상 ID 를 지어내며 행을 계속
/// 만들 수 있다 — 그건 남의 PC 를 채우는 일이다(§0.5).
///
/// ★ 이 테스트가 재는 것은 **오류가 나는가** 가 아니라 **행 수가 묶이는가**
///   다(독립 검수 1라운드 지적) — 상한에 걸리면 거부가 아니라 그 장치의
///   가장 오래된 관측을 밀어내므로, 오류를 기대하면 정책이 바뀌는 순간
///   테스트가 엉뚱한 것을 재게 된다.
///
/// ★ 그리고 이건 **저장소 전체 상한이 아니다** — 다른 장치 ID 를 계속
///   만들면 각각 다시 64행을 쓸 수 있다. 진짜 경계는 멤버십에 있다.
#[test]
fn one_device_cannot_grow_the_store_without_bound() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let attacker_machine = "01JNODEATTACKER000000001";
    let attacker_device = "01JDEVATTACKER0000000001";

    fn count(path: &std::path::Path) -> i64 {
        rusqlite::Connection::open(path)
            .expect("직접 열기")
            .query_row("SELECT COUNT(*) FROM coordinator_neighbor_reports", [], |r| r.get(0))
            .expect("세기")
    }

    // 지금 저장돼 있는 행을 오래된 순서로 들고 다닌다 — 다음 희생자가
    // 누구인지 예상하고 실제 축출과 대조하기 위해서다.
    let mut expected: Vec<gputeer_coordinator::neighbor_report_store::StoredNeighborReport> =
        Vec::new();
    let mut evictions = 0usize;
    for i in 0..200u64 {
        let invented_target = format!("01JNODEINVENTED{:0>11}", i);
        // ★ 기대 행을 **입력에서** 만든다 — DB 에서 읽어 기대값으로 쓰면
        //   저장과 조회가 함께 틀렸을 때 통과한다(독립 검수 9라운드 지적).
        let this_row = expected_row(
            attacker_machine,
            attacker_device,
            &invented_target,
            1_000 + i,
        );
        let before = count(&path);
        // 관측 시각을 계속 올린다 — 정상적인 신고자가 하는 그대로다.
        let outcome = s
            .record_verified_report(&verified_report(
                attacker_machine,
                attacker_device,
                &invented_target,
                OURS,
                1_000 + i,
            ))
            .expect("상한에 걸려도 거부가 아니라 축출이다");
        let after = count(&path);

        match outcome {
            RecordOutcome::Recorded { stored, evicted } if !evicted.is_empty() => {
                evictions += 1;
                // ★ 정상 경로에서 축출은 **정확히 한 행**이다 — 한 행 지우고
                //   한 행 넣었으니 총량은 그대로여야 한다. `<= 64` 만 보면
                //   과도한 삭제를 놓친다(독립 검수 2라운드 지적).
                assert_eq!(evicted.len(), 1, "정상 경로에서 두 행이 밀려났다");
                assert_eq!(after, before, "축출이 한 행보다 많이 지웠다");
                // ★ **어느 행이** 밀려났는지까지 본다(7라운드 지적) — 시각이
                //   1_000 부터 하나씩 오르므로 i 회차의 희생자는 (i-64) 번이다.
                let expected_victim = expected.remove(0);
                assert_eq!(evicted[0], expected_victim, "엉뚱한 행이 밀려났다");
                assert!(
                    s.get_report(attacker_machine, &expected_victim.unreachable_node_id)
                        .expect("읽기")
                        .is_none(),
                    "축출 보고와 실제 삭제가 다르다"
                );
                assert_eq!(stored, this_row);
            }
            RecordOutcome::Recorded { stored, evicted } => {
                assert!(evicted.is_empty());
                assert_eq!(after, before + 1, "축출 없이 한 행이 늘어야 한다");
                assert_eq!(stored, this_row);
            }
            other => panic!("{other:?}"),
        }
        expected.push(this_row);
    }

    // ★ 200 번 넣었는데 행 수가 정확히 상한인가 — DB 를 직접 센다.
    assert_eq!(count(&path), 64, "상한에 정확히 묶여야 한다");
    assert!(evictions > 0, "축출이 한 번도 보고되지 않았다 — 조용히 버렸을 수 있다");

    // ★ 그리고 **남아 있는 64행이 예상과 정확히 같은가**(8라운드 지적) —
    //   개수만 맞추면 예상 밖의 생존자를 지우고 다른 행으로 채워도 통과한다.
    assert_eq!(expected.len(), 64);
    for row in &expected {
        assert_eq!(
            current_row(&s, attacker_machine, &row.unreachable_node_id),
            *row,
            "남아 있어야 할 행이 바뀌거나 사라졌다"
        );
    }
}

/// ★ **정상 상한 상태에서 한 번의 삽입이 정확히 한 행만 지운다** — 독립
///   검수 2라운드가 실제로 잡은 결함의 회귀 테스트다.
///
/// ★ "축출이 절대 여러 행을 지우지 않는다" 가 아니다(11라운드 정정) —
///   상한을 이미 넘은 DB 를 복구할 때는 의도적으로 여러 행이 나간다
///   (`a_store_already_over_the_bound_is_brought_back_under_it` 참조).
///
/// 행의 정본 키는 `(reporter_node_id, unreachable_node_id)` 인데 축출
/// 삭제가 `(reporter_device_id, unreachable_node_id)` 로 지우고 있었다.
/// 한 장치가 여러 기계 ID 를 주장하면(이 저장소는 그걸 막지 못한다 —
/// `one_device_can_still_claim_two_machines_and_the_store_makes_that_visible`
/// 참조) 같은 대상에 대해 신고자가 다른 행이 함께 존재하므로, 그 키로
/// 지우면 **둘 다 사라진다.**
#[test]
fn a_single_insert_at_the_bound_evicts_exactly_one_row() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let device = "01JDEVTWOMACHINES0000001";
    let machine_a = "01JNODEMACHINEA000000001";
    let machine_b = "01JNODEMACHINEB000000001";

    // 같은 장치가 두 기계 ID 로 **같은 대상**을 신고한다 — 가장 오래된 쌍.
    let shared_target = "01JNODESHAREDTARGET00001";
    s.record_verified_report(&verified_report(machine_a, device, shared_target, OURS, 1_000))
        .expect("A 신고");
    s.record_verified_report(&verified_report(machine_b, device, shared_target, OURS, 1_000))
        .expect("B 신고");

    // 나머지를 상한까지 채운다(둘은 이미 넣었으므로 62개).
    for i in 0..62u64 {
        let target = format!("01JNODEFILL{:0>15}", i);
        s.record_verified_report(&verified_report(machine_a, device, &target, OURS, 2_000 + i))
            .expect("채우기");
    }

    let connection = rusqlite::Connection::open(&path).expect("직접 열기");
    let before: i64 = connection
        .query_row("SELECT COUNT(*) FROM coordinator_neighbor_reports", [], |r| r.get(0))
        .expect("세기");
    assert_eq!(before, 64);

    // 한 개 더 — 가장 오래된 쌍 중 **하나만** 밀려나야 한다.
    // 밀려날 것으로 예상되는 행을 미리 읽어 둔다.
    let expected_victim = expected_row(machine_a, device, shared_target, 1_000);
    assert_eq!(current_row(&s, machine_a, shared_target), expected_victim);

    let outcome = s
        .record_verified_report(&verified_report(
            machine_a,
            device,
            "01JNODEONEMORE000000001",
            OURS,
            9_999,
        ))
        .expect("축출하고 받는다");

    let evicted = match outcome {
        RecordOutcome::Recorded { stored, evicted } if evicted.len() == 1 => {
            assert_eq!(
                stored,
                expected_row(machine_a, device, "01JNODEONEMORE000000001", 9_999)
            );
            evicted.into_iter().next().expect("한 건")
        }
        other => panic!("정확히 한 건이 축출 보고돼야 한다: {other:?}"),
    };

    // ★ **어느 행이 밀려나는지가 정해져 있어야 한다.** 두 행의 관측 시각이
    //   같으므로 `ORDER BY observed_at, reporter_node_id, ...` 의 둘째 성분이
    //   승부를 낸다 — `machine_a < machine_b` 이므로 A 다. "둘 중 아무거나"
    //   를 받아 주면 정렬이 비결정적으로 바뀌어도 통과한다(3라운드 지적).
    assert_eq!(evicted, expected_victim, "동점은 신고자 ID 오름차순으로 갈린다");

    let after: i64 = connection
        .query_row("SELECT COUNT(*) FROM coordinator_neighbor_reports", [], |r| r.get(0))
        .expect("세기");
    assert_eq!(after, 64, "한 행 지우고 한 행 넣었으니 총량은 그대로다");

    // ★ 밀려난 행은 실제로 사라졌고, 새 행은 들어왔고, 나머지는 전부 살아
    //   있는가 — 개수만 맞으면 다른 행이 대신 지워져도 통과한다(7라운드 지적).
    assert!(s.get_report(machine_a, shared_target).expect("읽기").is_none());
    assert_eq!(
        current_row(&s, machine_a, "01JNODEONEMORE000000001"),
        expected_row(machine_a, device, "01JNODEONEMORE000000001", 9_999)
    );
    // ★ 생존자는 존재 여부가 아니라 **내용**까지 그대로여야 한다(8라운드 지적)
    //   — 유효하게 변조된 생존 행은 `is_some()` 으로는 안 잡힌다.
    assert_eq!(
        current_row(&s, machine_b, shared_target),
        expected_row(machine_b, device, shared_target, 1_000),
        "같은 대상의 다른 신고자 행이 바뀌었다"
    );
    for i in 0..62u64 {
        let target = format!("01JNODEFILL{:0>15}", i);
        assert_eq!(
            current_row(&s, machine_a, &target),
            expected_row(machine_a, device, &target, 2_000 + i),
            "{target} 이 바뀌거나 지워졌다"
        );
    }
}

/// ★ **시각 컬럼이 손상된 행이 후보에서 숨지 못한다.**
///
/// 축출 후보를 SQL 정렬로 고르면, 실제로 가장 오래된 행의 시각 컬럼을 큰
/// 값으로 손상시켰을 때 그 행이 정렬 뒤로 숨는다 — 그러면 **멀쩡한 행이
/// 대신 밀려나고 손상된 행은 남는다**(독립 검수 4라운드가 잡은 결함).
///
/// 그래서 그 장치의 행을 전부 검증해 읽은 뒤 최솟값을 고른다. 어느 한
/// 행이라도 손상됐으면 축출하지 않고 거부한다.
#[test]
fn a_corrupted_time_column_cannot_hide_a_row_from_eviction() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let machine = "01JNODEHIDETIME000000001";
    let device = "01JDEVHIDETIME0000000001";

    let truly_oldest = format!("01JNODEFILL{:0>15}", 0);
    for i in 0..64u64 {
        let target = format!("01JNODEFILL{:0>15}", i);
        s.record_verified_report(&verified_report(machine, device, &target, OURS, 1_000 + i))
            .expect("채우기");
    }

    // 진짜 가장 오래된 행의 **인덱스 시각만** 아주 큰 값으로 바꾼다.
    // 몸통은 그대로이므로 8바이트 형식은 유지되고, SQL 정렬에서는 맨 뒤로
    // 밀려 후보에서 사라진다.
    tamper(
        &path,
        &format!(
            "UPDATE coordinator_neighbor_reports \
                SET observed_at_unix_ms = X'FFFFFFFFFFFFFFFF' \
                WHERE unreachable_node_id = '{truly_oldest}'"
        ),
    );

    let before = snapshot(&path);
    let error = s
        .record_verified_report(&verified_report(
            machine,
            device,
            "01JNODEONEMORE000000001",
            OURS,
            9_999,
        ))
        .expect_err("손상된 행이 있으면 축출하지 않고 거부해야 한다");
    assert_corrupt(
        error,
        machine,
        &truly_oldest,
        NeighborReportCorruption::ObservedAtEncoding,
    );
    // ★ 멀쩡한 행이 대신 밀려나지 않았는가 — 저장소 전체가 그대로여야 한다.
    assert_eq!(snapshot(&path), before, "손상된 행을 숨긴 채 다른 행을 지웠다");
}

/// ★ **손상된 행은 조용히 축출되지 않는다.**
///
/// 축출 대상을 ID 만 읽고 지우면, 해시·몸통이 깨진 행이 `Corrupt` 없이
/// 사라진다 — 손상을 통과시키지 않는다는 이 모듈의 계약이 축출 경로에서만
/// 깨진다(독립 검수 3라운드 지적). 게다가 지워지고 나면 다시 볼 수도 없다.
#[test]
fn a_corrupted_eviction_candidate_is_refused_not_silently_dropped() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let machine = "01JNODECORRUPTEVICT00001";
    let device = "01JDEVCORRUPTEVICT000001";

    let oldest_target = format!("01JNODEFILL{:0>15}", 0);
    for i in 0..64u64 {
        let target = format!("01JNODEFILL{:0>15}", i);
        s.record_verified_report(&verified_report(machine, device, &target, OURS, 1_000 + i))
            .expect("채우기");
    }

    // 가장 오래된 행의 몸통만 깨뜨린다.
    tamper(
        &path,
        &format!(
            "UPDATE coordinator_neighbor_reports SET report_body = X'00' \
                WHERE unreachable_node_id = '{oldest_target}'"
        ),
    );

    let before = snapshot(&path);
    let error = s
        .record_verified_report(&verified_report(
            machine,
            device,
            "01JNODEONEMORE000000001",
            OURS,
            9_999,
        ))
        .expect_err("손상된 축출 후보는 거부돼야 한다");
    assert_corrupt(error, machine, &oldest_target, NeighborReportCorruption::HashMismatch);
    // 손상된 행도, 다른 어떤 행도 사라지지 않았다.
    assert_eq!(snapshot(&path), before, "거부했는데 DB 가 바뀌었다");
}

/// 축출은 **가장 오래된 관측**을 고른다 — 아무거나 지우면 최신 관측이
/// 사라져 판정 재료가 나빠진다.
#[test]
fn eviction_drops_the_oldest_observation_of_that_device() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let machine = "01JNODEEVICT000000000001";
    let device = "01JDEVEVICT0000000000001";

    // 가장 오래된 것을 특정할 수 있게 시각을 하나씩 올려 가며 채운다.
    let oldest_target = format!("01JNODEFILL{:0>15}", 0);
    for i in 0..64u64 {
        let target = format!("01JNODEFILL{:0>15}", i);
        s.record_verified_report(&verified_report(machine, device, &target, OURS, 1_000 + i))
            .expect("채우기");
    }

    // ★ 밀려나기 **직전에** 그 행을 읽어 둔다 — 축출 보고가 이것과
    //   **완전히** 같아야 한다. 필드를 몇 개만 확인하면 장치 ID·해시·원본
    //   메시지가 틀리게 돌아와도 통과한다(독립 검수 4라운드 지적).
    let expected = expected_row(machine, device, &oldest_target, 1_000);
    assert_eq!(current_row(&s, machine, &oldest_target), expected);

    // 한 개 더 — 가장 오래된 것이 밀려나야 한다.
    let outcome = s
        .record_verified_report(&verified_report(
            machine,
            device,
            "01JNODEONEMORE000000001",
            OURS,
            9_999,
        ))
        .expect("축출하고 받는다");
    match outcome {
        RecordOutcome::Recorded { stored, evicted } => {
            assert_eq!(evicted, vec![expected], "축출 보고가 사라진 행 전체와 같아야 한다");
            // ★ 다른 경로에는 적용한 "반환된 저장 행 전체를 DB 와 대조" 기준이
            //   이 경로에만 빠져 있었다(7라운드 지적).
            assert_eq!(
                stored,
                expected_row(machine, device, "01JNODEONEMORE000000001", 9_999)
            );
        }
        other => panic!("축출이 보고돼야 한다: {other:?}"),
    }

    // 밀려난 것은 실제로 사라졌고 새 것은 들어왔는가 — DB 를 다시 읽는다.
    assert!(s.get_report(machine, &oldest_target).expect("읽기").is_none());
    // ★ 새 행도 **내용**까지 확인한다 — 반환값 대조가 DB 대조를 대신하지
    //   않는다(독립 검수 10라운드 지적).
    assert_eq!(
        current_row(&s, machine, "01JNODEONEMORE000000001"),
        expected_row(machine, device, "01JNODEONEMORE000000001", 9_999)
    );

    // ★ 나머지 63 행이 **전부** 살아 있는가 — 하나만 확인하면 과도한
    //   삭제를 놓친다(독립 검수 2라운드 지적).
    // ★ 생존자는 존재 여부가 아니라 **내용**까지 그대로여야 한다.
    for i in 1..64u64 {
        let target = format!("01JNODEFILL{:0>15}", i);
        assert_eq!(
            current_row(&s, machine, &target),
            expected_row(machine, device, &target, 1_000 + i),
            "{target} 이 바뀌거나 지워졌다"
        );
    }
}

/// ★ **더 오래된 관측으로 더 새로운 관측을 밀어내지는 않는다.**
///
/// 축출이 무조건이면, 늦게 도착한 옛 신고가 최신 관측을 지워 버린다 —
/// 이 모듈의 "뒤로 가지 않는다" 규칙과 정면으로 어긋난다.
#[test]
fn an_observation_older_than_everything_stored_does_not_evict() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let machine = "01JNODEOLD0000000000001";
    let device = "01JDEVOLD00000000000001";

    for i in 0..64u64 {
        let target = format!("01JNODEFILL{:0>15}", i);
        s.record_verified_report(&verified_report(machine, device, &target, OURS, 5_000 + i))
            .expect("채우기");
    }
    let before = snapshot(&path);

    // 저장된 무엇보다도 오래된 관측.
    let error = s
        .record_verified_report(&verified_report(
            machine,
            device,
            "01JNODETOOOLD0000000001",
            OURS,
            10,
        ))
        .expect_err("거부돼야 한다");
    assert_quota_exhausted(error, device, 64, 5_000);

    // ★ 한 행이 아니라 **저장소 전체**가 그대로인가 — 63행을 지우고 오류를
    //   돌려주는 구현도 한 행만 보면 통과한다(3라운드 지적).
    assert_eq!(snapshot(&path), before, "거부했는데 DB 가 바뀌었다");

    // ★ 경계: **같은 시각**도 밀어내지 못한다(`<=`). 이 모듈의 다른
    //   신선도 비교와 같은 규칙이라 한 곳만 어긋나면 계층 간 모순이 된다.
    let error = s
        .record_verified_report(&verified_report(
            machine,
            device,
            "01JNODESAMETIME00000001",
            OURS,
            5_000,
        ))
        .expect_err("같은 시각도 거부돼야 한다");
    assert_quota_exhausted(error, device, 64, 5_000);
    // 같은 시각 거부 뒤에도 저장소 전체가 그대로여야 한다.
    assert_eq!(snapshot(&path), before, "같은 시각 거부가 DB 를 바꿨다");
}

/// 상한에 걸린 뒤에도 **기존 신고의 갱신**은 통과한다.
///
/// ★ 구분 안 하면, 상한을 채운 정당한 이웃의 신고가 영원히 낡은 채로
///   남는다.
///
/// ★ 반환값만 확인하면 공허하다(독립 검수 1라운드 지적) — 구현이
///   `Recorded` 를 돌려주면서 행을 실제로 안 바꿔도 통과한다. DB 를 다시
///   읽어 확인한다.
#[test]
fn reaching_the_quota_does_not_block_updating_an_existing_report() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "neighbors.db");
    let machine = "01JNODEBUSY000000000001";
    let device = "01JDEVBUSY0000000000001";

    let first_target = format!("01JNODEMANY{:0>15}", 0);
    for i in 0..64u64 {
        let target = format!("01JNODEMANY{:0>15}", i);
        s.record_verified_report(&verified_report(machine, device, &target, OURS, 1_000 + i))
            .expect("채우기");
    }

    // ★ 이미 있는 신고의 갱신은 상한과 무관하고, 아무것도 밀어내지 않는다.
    let outcome = s
        .record_verified_report(&verified_report(machine, device, &first_target, OURS, 8_000))
        .expect("기존 행 갱신은 상한과 무관하다");
    let returned = match outcome {
        RecordOutcome::Recorded { stored, evicted } => {
            assert!(
                evicted.is_empty(),
                "갱신은 저장소를 키우지 않으므로 축출할 이유가 없다"
            );
            stored
        }
        other => panic!("갱신돼야 한다: {other:?}"),
    };

    // ★ 저장된 행과 반환값을 **입력으로 만든 기대 행**과 대조한다.
    let expected = expected_row(machine, device, &first_target, 8_000);
    assert_eq!(current_row(&s, machine, &first_target), expected);
    assert_eq!(returned, expected, "반환값이 기대 행과 달라졌다");

    // 나머지 63행은 손대지 않았는가 — 내용까지 본다.
    for i in 1..64u64 {
        let target = format!("01JNODEMANY{:0>15}", i);
        assert_eq!(
            current_row(&s, machine, &target),
            expected_row(machine, device, &target, 1_000 + i),
            "{target} 이 바뀌었다"
        );
    }
}

/// ★ **이 API 로는 만들 수 없는 상태를 복구한다.**
///
/// 외부 쓰기나 구버전 DB 로 한 장치의 행이 이미 상한을 넘어 있으면, 한 건만
/// 밀어내는 구현은 그 DB 를 영영 상한 밖에 둔다(독립 검수 5라운드 지적).
/// 상한 **안쪽**이 될 만큼 밀어내야 한다.
#[test]
fn a_store_already_over_the_bound_is_brought_back_under_it() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let machine = "01JNODEOVERBOUND00000001";
    let device = "01JDEVOVERBOUND000000001";

    seed_rows_directly(&path, machine, device, 70);

    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");

    // ★ 밀려날 7건을 **미리 읽어 둔다** — 축출 보고가 이것과 전부 같아야 한다.
    let all_targets: Vec<String> =
        (0..70u64).map(|i| format!("01JNODEOVER{:0>15}", i)).collect();
    let expected_doomed: Vec<_> = all_targets[..7]
        .iter()
        .enumerate()
        .map(|(i, target)| expected_row(machine, device, target, 1_000 + i as u64))
        .collect();
    let outcome = s
        .record_verified_report(&verified_report(
            machine,
            device,
            "01JNODEONEMORE000000001",
            OURS,
            9_999,
        ))
        .expect("복구하며 받는다");

    match outcome {
        RecordOutcome::Recorded { stored, evicted } => {
            // 70 개였고 하나를 더 넣으니, 64 가 되려면 7 개가 나가야 한다.
            // ★ 개수와 양 끝만 보면 가운데 5건이 뒤바뀌거나 엉뚱한 행이어도
            //   통과한다(독립 검수 6라운드 지적) — 7건 전부를 대조한다.
            assert_eq!(evicted, expected_doomed, "밀려난 7건이 예상과 다르다");
            assert_eq!(
                stored,
                expected_row(machine, device, "01JNODEONEMORE000000001", 9_999),
                "반환값이 기대 행과 다르다"
            );
        }
        other => panic!("{other:?}"),
    }

    // ★ 남아야 할 63건이 **전부** 남았고, 밀려난 7건은 **전부** 사라졌는가.
    for (i, target) in all_targets.iter().enumerate() {
        match s.get_report(machine, target).expect("읽기") {
            None => assert!(i < 7, "{target} 이 지워지면 안 되는데 지워졌다"),
            // ★ 생존자는 **내용**까지 그대로여야 한다(9라운드 지적).
            Some(row) => {
                assert!(i >= 7, "{target} 이 지워졌어야 하는데 남았다");
                assert_eq!(row, expected_row(machine, device, target, 1_000 + i as u64));
            }
        }
    }

    let connection = rusqlite::Connection::open(&path).expect("직접 열기");
    let rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM coordinator_neighbor_reports", [], |r| r.get(0))
        .expect("세기");
    assert_eq!(rows, 64, "상한 안쪽으로 돌아와야 한다");
}

/// ★ **상한 초과 복구에서도 "뒤로 가지 않는다".**
///
/// 밀려날 것이 여러 건이면 비교 기준은 그중 **가장 새로운 것**이다. 가장
/// 오래된 것과 비교하면, 밀려날 집합 안의 더 새로운 관측을 더 오래된
/// 관측으로 덮어쓰게 된다(독립 검수 6라운드 지적 — 이 수정의 핵심을 고정할
/// 회귀 테스트가 없었다).
#[test]
fn over_the_bound_an_observation_not_newer_than_the_doomed_set_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let machine = "01JNODEOVERBND2000000001";
    let device = "01JDEVOVERBND20000000001";
    seed_rows_directly(&path, machine, device, 70);

    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    let before = snapshot(&path);

    // 시각 1_006 = 밀려날 7건(1_000~1_006) 중 **가장 새로운 것**과 같다.
    // 가장 오래된 것(1_000)보다는 새롭지만 받아들이면 안 된다.
    let error = s
        .record_verified_report(&verified_report(
            machine,
            device,
            "01JNODEONEMORE000000001",
            OURS,
            1_006,
        ))
        .expect_err("밀려날 집합보다 새롭지 않으면 거부해야 한다");
    // ★ 거부 이유로 보고한 값이 **실제 비교에 쓴 값**인가 — 전 필드 대조.
    assert_quota_exhausted(error, device, 70, 1_006);
    assert_eq!(snapshot(&path), before, "거부했는데 DB 가 바뀌었다");
}

/// ★ **골든 벡터 — 테스트와 프로덕션이 같이 틀리는 것을 막는다.**
///
/// `expected_row()` 는 저장소를 거치지 않지만, 프로덕션과 **같은**
/// `prost::Message::encode_to_vec()` 와 같은 서명 fixture 를 쓴다. 그 공통
/// 의존성이 함께 바뀌면 둘이 나란히 틀린 채 통과한다(독립 검수 9라운드 지적).
///
/// 그래서 한 건만 **고정된 바이트**로 못박는다. 인코딩·서명·해시 중 무엇이
/// 바뀌어도 이 테스트가 먼저 실패한다.
///
/// ★ 이 값이 바뀌면 그건 **wire 형식이 바뀌었다**는 뜻이다 — 고쳐서 통과
///   시키기 전에 왜 바뀌었는지부터 확인해야 한다.
#[test]
fn the_stored_body_and_hash_are_pinned_to_fixed_bytes() {
    let row = expected_row(
        "01JNODEGOLDEN00000000001",
        "01JDEVGOLDEN000000000001",
        "01JNODEGOLDENTARGET00001",
        1_700_000_000_000,
    );
    use prost::Message;
    const GOLDEN_BODY: &str = "0801121830314a4e4f4445474f4c44454e30303030303030303030311a1830314a444556474f4c44454e303030303030303030303031221830314a4e4f4445474f4c44454e54415247455430303030312a1b30314a434f4f52444e45494748424f5230303030303030303030313080d095ffbc313a1009090909090909090909090909090909d205409b1b66858f2fabf09c93bcc5e65aa8644d6bb2ad42998d6fc20e46a96cab35a87b98aaf99098d307c640f7f62fb75e82ced902af5e0f8101099573c4b3b9ec06";
    const GOLDEN_HASH: &str = "79eb518a369fbc18a1303ec52b4f30d66739d7846a693e1c312dc1975e5f3a21";

    assert_eq!(hex(&row.report.encode_to_vec()), GOLDEN_BODY);
    assert_eq!(hex(&row.report_hash), GOLDEN_HASH);

    // ★ 이름이 `stored` 이므로 **실제로 저장된 바이트**까지 고정한다
    //   (독립 검수 10라운드 지적) — 위 두 줄은 테스트 쪽 계산만 고정한다.
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("neighbors.db");
    let mut s = CoordinatorNeighborReportStore::open(&path, OURS).expect("열기");
    s.record_verified_report(&verified_report(
        "01JNODEGOLDEN00000000001",
        "01JDEVGOLDEN000000000001",
        "01JNODEGOLDENTARGET00001",
        OURS,
        1_700_000_000_000,
    ))
    .expect("기록");

    let connection = rusqlite::Connection::open(&path).expect("직접 열기");
    let (stored_body, stored_hash): (Vec<u8>, Vec<u8>) = connection
        .query_row(
            "SELECT report_body, report_hash FROM coordinator_neighbor_reports",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("읽기");
    assert_eq!(hex(&stored_body), GOLDEN_BODY, "저장된 몸통 바이트가 바뀌었다");
    assert_eq!(hex(&stored_hash), GOLDEN_HASH, "저장된 다이제스트가 바뀌었다");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
