//! `CoordinatorNodeLivenessStore` 실측.
//!
//! `ADR-033` §7 이 "값을 채우는 관측자가 없다" 고 지목한 자리를 채운
//! 저장소가, 실제로 재시작을 넘어 최신 사실을 유지하고 손상을 통과시키지
//! 않는지 본다.

use gputeer_coordinator::node_liveness_store::{
    CoordinatorNodeLivenessStore, NodeLivenessStoreError,
};
use gputeer_crypto::{sign, Ed25519Verifier, InMemoryKeyring, SigningKey};
use gputeer_protocol::{
    pb,
    signing::{verify, NoReplayCheck, Verified},
};

const NODE: &str = "01JNODESELFTEST000000000001";
const DEVICE: &str = "01JNODESELFTEST000000000001";
const COORDINATOR: &str = "01JCOORDSELFTEST00000000001";

fn seed(label: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(label.as_bytes());
    *hasher.finalize().as_bytes()
}

/// 실제로 서명해 `Verified` 를 만든다.
///
/// ★ 검증을 우회하는 지름길을 만들지 않는다. 저장소가 `Verified` 만
///   받는다는 계약이 테스트에서 뚫리면, 그 계약이 실제로 지켜지는지
///   아무도 모른다.
fn verified_heartbeat(
    device_id: &str,
    issued_at_unix_ms: u64,
    fence_epoch: u64,
    running_attempts: u32,
) -> Verified<pb::NodeHeartbeat> {
    let key = SigningKey::from_bytes(&seed(device_id));
    let mut heartbeat = pb::NodeHeartbeat {
        schema_version: 1,
        node_id: NODE.to_string(),
        device_id: device_id.to_string(),
        coordinator_device_id: COORDINATOR.to_string(),
        issued_at_unix_ms,
        fence_epoch,
        running_attempts,
        request_nonce: vec![7u8; 16],
        ..Default::default()
    };
    heartbeat.node_signature = sign(&key, &heartbeat).to_vec();

    let mut keys = InMemoryKeyring::new();
    keys.insert(device_id, key.verifying_key());
    verify(
        &heartbeat,
        1,
        &Ed25519Verifier::new(keys),
        // ★ 만료 검사를 통과하도록 발행 시각 그대로 쓴다. 이 테스트가
        //   보려는 것은 저장소이지 서명 계층의 만료 규칙이 아니다.
        issued_at_unix_ms.max(1),
        &mut NoReplayCheck,
    )
    .expect("테스트 heartbeat 서명은 검증돼야 한다")
}

fn store(dir: &tempfile::TempDir, name: &str) -> CoordinatorNodeLivenessStore {
    CoordinatorNodeLivenessStore::open(dir.path().join(name)).expect("저장소 열기")
}

/// 관측이 실제로 남고 재시작 뒤에도 읽히는가.
///
/// ★ 이게 이 조각의 존재 이유다. 메모리에만 있으면 Coordinator 가
///   재시작하는 순간 모든 노드가 "한 번도 못 봤다" 가 된다.
#[test]
fn an_observation_survives_a_restart() {
    let dir = tempfile::tempdir().expect("temp");
    {
        let mut s = store(&dir, "liveness.db");
        let result = s.observe(&verified_heartbeat(DEVICE, 1_000, 7, 1)).expect("관측");
        assert!(result.advanced);
        assert_eq!(result.stored.last_heartbeat_unix_ms, 1_000);
    }
    // 완전히 새 연결 — 프로세스 재시작을 흉내낸다.
    let s = store(&dir, "liveness.db");
    let all = s.load_all().expect("읽기");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].node_id, NODE);
    assert_eq!(all[0].last_heartbeat_unix_ms, 1_000);
    assert_eq!(all[0].fence_epoch, 7);
    assert_eq!(all[0].running_attempts, 1);
}

/// ★ 늦게 도착한 옛 heartbeat 가 최신 값을 밀어내지 않는가.
///
/// 재전송·경로 지연으로 실제로 일어난다. 밀어내면 살아 있는 노드가
/// 갑자기 조용해 보이고, 그 위에서 재배정 판단이 내려진다.
#[test]
fn a_late_older_heartbeat_does_not_move_the_clock_backwards() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "liveness.db");

    s.observe(&verified_heartbeat(DEVICE, 5_000, 9, 2)).expect("최신");
    let late = s.observe(&verified_heartbeat(DEVICE, 1_000, 3, 0)).expect("지연 도착");

    assert!(!late.advanced, "옛 관측이 저장됐다");
    assert_eq!(
        late.stored.last_heartbeat_unix_ms, 5_000,
        "최신 시각이 밀렸다"
    );
    assert_eq!(late.stored.fence_epoch, 9, "옛 세대가 최신 세대를 덮었다");

    let all = s.load_all().expect("읽기");
    assert_eq!(all[0].last_heartbeat_unix_ms, 5_000);
    assert_eq!(all[0].fence_epoch, 9);
}

/// 같은 시각의 재전송도 진행시키지 않는가.
///
/// `<=` 경계다 — 이 저장소가 다루는 다른 시각 비교(`DoD-26`·`DoD-32`)와
/// 같은 규칙이다.
#[test]
fn an_identical_timestamp_does_not_advance() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "liveness.db");
    s.observe(&verified_heartbeat(DEVICE, 5_000, 9, 2)).expect("첫 관측");
    let again = s.observe(&verified_heartbeat(DEVICE, 5_000, 9, 2)).expect("재전송");
    assert!(!again.advanced);
}

/// 더 최신이면 진행하는가.
///
/// ★ 위 두 테스트만 있으면 "아무것도 저장 안 함" 으로도 통과한다.
#[test]
fn a_newer_heartbeat_advances() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "liveness.db");
    s.observe(&verified_heartbeat(DEVICE, 1_000, 3, 0)).expect("첫 관측");
    let newer = s.observe(&verified_heartbeat(DEVICE, 2_000, 4, 1)).expect("최신");
    assert!(newer.advanced);
    assert_eq!(newer.stored.last_heartbeat_unix_ms, 2_000);
    assert_eq!(newer.stored.fence_epoch, 4);
    assert_eq!(newer.stored.running_attempts, 1);
}

/// 노드마다 한 행씩 유지되는가.
#[test]
fn each_node_keeps_exactly_one_row() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "liveness.db");
    for at in [1_000u64, 2_000, 3_000] {
        s.observe(&verified_heartbeat(DEVICE, at, 1, 0)).expect("관측");
    }
    let all = s.load_all().expect("읽기");
    assert_eq!(all.len(), 1, "노드 하나에 행이 여럿이다 — 저장소가 무한히 자란다");
    assert_eq!(all[0].last_heartbeat_unix_ms, 3_000);
}

/// 시각 없는 관측을 거부하는가.
///
/// `issued_at_unix_ms == 0` 이면 침묵 시간을 계산할 수 없다. 0 을
/// 그대로 저장하면 그 노드는 영원히 "아주 오래 전에 봤다" 가 된다.
#[test]
fn a_heartbeat_without_a_timestamp_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "liveness.db");
    let error = s
        .observe(&verified_heartbeat(DEVICE, 0, 1, 0))
        .expect_err("시각 0 이 통과했다");
    assert!(matches!(
        error,
        NodeLivenessStoreError::InvalidHeartbeat { .. }
    ));
    assert!(s.load_all().expect("읽기").is_empty(), "거부했는데 행이 남았다");
}

/// 손상된 행을 조용히 통과시키지 않는가.
///
/// ★ 손상된 사실 위에서 생존을 판정하면 그 판정이 틀렸다는 것을
///   아무도 모른다. `DoD-52`·`DoD-53` 과 같은 규칙이다.
#[test]
fn a_corrupted_row_fails_closed() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("liveness.db");
    {
        let mut s = CoordinatorNodeLivenessStore::open(&path).expect("열기");
        s.observe(&verified_heartbeat(DEVICE, 1_000, 7, 1)).expect("관측");
    }
    // 저장된 시각만 손으로 바꾼다 — body 는 그대로라 대조에서 걸려야 한다.
    {
        let connection = rusqlite::Connection::open(&path).expect("직접 열기");
        connection
            .execute(
                "UPDATE coordinator_node_liveness SET last_heartbeat_unix_ms = 999999",
                [],
            )
            .expect("손상 주입");
    }
    let s = CoordinatorNodeLivenessStore::open(&path).expect("다시 열기");
    let error = s.load_all().expect_err("손상된 행이 통과했다");
    assert!(matches!(error, NodeLivenessStoreError::Corrupt { .. }));
}

/// body 를 바꾸면 해시 대조가 잡는가.
#[test]
fn a_tampered_body_is_caught_by_the_hash() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("liveness.db");
    {
        let mut s = CoordinatorNodeLivenessStore::open(&path).expect("열기");
        s.observe(&verified_heartbeat(DEVICE, 1_000, 7, 1)).expect("관측");
    }
    {
        let connection = rusqlite::Connection::open(&path).expect("직접 열기");
        connection
            .execute(
                "UPDATE coordinator_node_liveness SET heartbeat_body = X'00'",
                [],
            )
            .expect("손상 주입");
    }
    let s = CoordinatorNodeLivenessStore::open(&path).expect("다시 열기");
    assert!(matches!(
        s.load_all(),
        Err(NodeLivenessStoreError::Corrupt { .. })
    ));
}

/// 저장된 사실이 생존 판정 커널에 그대로 들어가는가.
///
/// ★ 저장소와 커널이 따로 놀면 둘 다 맞아도 시스템은 틀린다. 실제로
///   이어 본다.
#[test]
fn stored_facts_feed_the_liveness_kernel() {
    use gputeer_scheduler::{
        classify_node_liveness, HeartbeatObservation, LivenessPolicy, NodeLiveness,
    };

    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "liveness.db");
    s.observe(&verified_heartbeat(DEVICE, 1_000_000, 7, 1)).expect("관측");

    let observations: Vec<HeartbeatObservation> = s
        .load_all()
        .expect("읽기")
        .into_iter()
        .map(|row| HeartbeatObservation {
            node_id: row.node_id,
            device_id: row.device_id,
            issued_at_unix_ms: row.last_heartbeat_unix_ms,
            fence_epoch: row.fence_epoch,
            running_attempts: row.running_attempts,
        })
        .collect();

    let policy = LivenessPolicy {
        live_within_ms: 30_000,
        silent_after_ms: 90_000,
    };
    let live = classify_node_liveness(&observations, &[], policy, 1_010_000).expect("판정");
    assert_eq!(live[0].liveness, NodeLiveness::Live);

    // 시간이 충분히 흐르면 같은 사실이 Silent 로 읽힌다 — 저장소는
    // 안 바뀌고 판정만 바뀐다는 것을 확인한다.
    let silent = classify_node_liveness(&observations, &[], policy, 1_200_000).expect("판정");
    assert_eq!(silent[0].liveness, NodeLiveness::Silent);
}

/// ★ 같은 밀리초의 두 관측을 **커널과 같은 규칙**으로 가르는가.
///
/// 2026-08-30 독립 검수 2라운드 지적. 초안은 무조건 먼저 온 것이 이겨서,
/// 저장소와 `classify_node_liveness` 가 서로 다른 답을 냈다 — 잠금 획득
/// 순서가 결과를 바꿨다. 두 계층이 다른 규칙을 쓰면 시스템은 틀린다.
#[test]
fn a_same_millisecond_tie_uses_the_same_rule_as_the_kernel() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "liveness.db");

    // 낮은 세대를 먼저 넣고 같은 시각의 높은 세대를 넣는다.
    s.observe(&verified_heartbeat(DEVICE, 5_000, 3, 0)).expect("첫 관측");
    let later = s.observe(&verified_heartbeat(DEVICE, 5_000, 9, 1)).expect("동점");
    assert!(later.advanced, "같은 시각의 나중 세대가 반영되지 않았다");
    assert_eq!(later.stored.fence_epoch, 9);

    // 반대 방향은 진행하지 않아야 한다.
    let back = s.observe(&verified_heartbeat(DEVICE, 5_000, 3, 0)).expect("역방향");
    assert!(!back.advanced, "같은 시각의 옛 세대가 최신을 밀어냈다");
    assert_eq!(back.stored.fence_epoch, 9);
}

/// ★ 같은 노드를 다른 장치가 보고하면 덮지 않고 멈추는가.
///
/// 덮으면 커널의 `ConflictingDevice` 방어에 도달하기 전에 충돌 증거가
/// 사라진다 — 등록된 B 가 A 의 노드를 조용히 인수할 수 있다.
#[test]
fn a_different_device_claiming_the_same_node_is_refused() {
    let dir = tempfile::tempdir().expect("temp");
    let mut s = store(&dir, "liveness.db");
    s.observe(&verified_heartbeat(DEVICE, 1_000, 1, 0)).expect("첫 관측");

    // 더 최신 시각인데도 거부돼야 한다 — 시각이 문제가 아니라 신원이다.
    let error = s
        .observe(&verified_heartbeat("01JOTHERDEVICE0000000000001", 9_000, 1, 0))
        .expect_err("다른 장치가 같은 노드를 인수했다");
    assert!(matches!(
        error,
        NodeLivenessStoreError::DeviceChanged { .. }
    ));

    // 기존 사실이 그대로인가.
    let all = s.load_all().expect("읽기");
    assert_eq!(all[0].device_id, DEVICE);
    assert_eq!(all[0].last_heartbeat_unix_ms, 1_000);
}
