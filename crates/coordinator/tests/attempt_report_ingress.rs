//! Agent 가 보낸 `AttemptReport` 가 **wire 를 건너 저장소에 도달하는가**,
//! 그리고 **도달하면 안 되는 것이 안 도달하는가**.
//!
//! ★ 이 테스트는 Coordinator 의 **production 진입점**(`run()`)을 그대로
//!   부른다. 세션 함수는 비공개이므로, 여기서 손으로 만든 세션을 흉내 내면
//!   실제로 도는 코드와 다른 것을 재게 된다.
//!
//! 테스트 쪽이 Agent 역할을 한다 — Grant 를 받고, `AgentGrantAck` 를 서명해
//! 보내고, 그다음 `AttemptReport` 프레임을 보낸다.
//!
//! # 여기서 재지 **못하는** 것 (정직하게 적는다)
//!
//! ```text
//! node_id 대조          이 lane 은 Agent 키를 **하나만** 등록한다.
//!                       `AttemptReport::signer_id()` 가 `node_id` 라
//!                       다른 이름을 실으면 서명 검증이 그 이름의 키를
//!                       못 찾아 먼저 막는다(UnknownSigner) — 대조에
//!                       닿지 않는다. heartbeat·이웃 신고 경로가 같은
//!                       이유로 같은 공백을 갖고 있다
//!
//! Lease 부재 분기        이 lane 의 Grant 는 항상 Lease 를 싣는다.
//!                       그 분기는 나중에 가정이 깨졌을 때를 위한
//!                       fail-closed 이지 지금 도달하는 경로가 아니다
//!
//! Agent 가 실제로 보내는가  Agent 의 `run()` 은 워크로드를 **실제로 실행**
//!                       해야 종료를 관측한다. 그 경로는 이 테스트가
//!                       띄우지 않는다 — 여기서 재는 것은 수신·검증·저장
//!                       쪽이다. 발신 쪽은 `crates/agent/src/report.rs`
//!                       의 단위 테스트가 잰다
//! ```

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gputeer_coordinator::attempt_report_store::CoordinatorAttemptReportStore;
use gputeer_coordinator::inventory_store::{
    AgentInventory, AgentRegistry, CoordinatorInventoryStore, GpuInventory,
};
use gputeer_coordinator::job_store::{AcceptedJobSubmission, CoordinatorJobStore};
use gputeer_coordinator::lease_store::CoordinatorLeaseStore;
use gputeer_coordinator::staging_store::{CoordinatorStagingStore, StageQueuedRequest};
use gputeer_crypto::{
    sign, Ed25519Verifier, FrameType, InMemoryKeyring, KeyProtection, PersistentKeyring,
    PlaintextPolicy, SigningKey,
};
use gputeer_protocol::canonical::blake3_256;
use gputeer_protocol::signing::{signing_input, verify, NoReplayCheck};
use gputeer_protocol::nonce::derive_replay_nonce;
use gputeer_protocol::pb;
use prost::Message;

const JOB_ID: &str = "job-1";
const ATTEMPT_ID: &str = "attempt-1";
const LEASE_ID: &str = "lease-1";
/// Agent 의 장치 ID 이자 예약된 노드 이름. `AttemptReport.node_id` 이며
/// 동시에 `signer_id` 다 — 셋이 같아야 저장소가 결합한다.
const NODE_ID: &str = "node-1";
const COORDINATOR_ID: &str = "coordinator-1";
const GRANT_ID: &str = "grant-1";

const COORDINATOR_SEED: [u8; 32] = [0x31; 32];
const AGENT_SEED: [u8; 32] = [0x41; 32];
const SUBMITTER_ID: &str = "submitter-1";
const SUBMITTER_SEED: [u8; 32] = [0x51; 32];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("시계가 UNIX epoch 뒤에 있다")
        .as_millis() as u64
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Attempt·Lease·노드 예약이 durable 하게 있는 control DB.
///
/// `tests/reservation_release.rs` 의 fixture 와 같은 모양이다 — 다만 Lease
/// 시각을 **지금 기준**으로 둔다. `signed_grant_from_stored()` 가 발급
/// 시각에 Lease 가 살아 있는지 보기 때문이다.
fn prepare_control_db(path: &Path) {
    let now = now_ms();

    let mut inventory = CoordinatorInventoryStore::open(path).expect("inventory store");
    inventory
        .register_agent(&AgentRegistry {
            node_id: NODE_ID.into(),
            device_id: NODE_ID.into(),
            owner_member_id: "owner-1".into(),
            verifying_key: SigningKey::from_bytes(&AGENT_SEED)
                .verifying_key()
                .to_bytes()
                .to_vec(),
            node_state: None,
            risk_state: None,
            security_tier: None,
            isolation_class: None,
            key_protection: None,
        })
        .expect("register agent");
    inventory
        .update_inventory(&AgentInventory {
            node_id: NODE_ID.into(),
            inventory_revision: 7,
            observed_at_unix_ms: now,
            gpus: Some(vec![GpuInventory {
                gpu_id: "gpu-1".into(),
                model: Some("model-1".into()),
                healthy: Some(true),
                available_vram_bytes: Some(16),
            }]),
            available_cpu_cores: Some(8),
            available_ram_bytes: Some(64),
            available_workspace_bytes: Some(64),
            allowed_workload_classes: None,
            third_party_workloads_opt_in: None,
        })
        .expect("update inventory");
    drop(inventory);

    // ★ 저장된 예약 lane 은 이제 제출자 서명 Manifest 를 **지금** 다시 검증해
    //   싣는다(§A1 1.5 선행). 그래서 hash 만 있는 Job 이 아니라 서명된 Manifest 를
    //   묶어 저장한다 — 없으면 `LegacyManifestMissing` 으로 발급이 거부된다.
    let submitter = SigningKey::from_bytes(&SUBMITTER_SEED);
    let mut manifest = pb::JobManifest {
        schema_version: 1,
        job_id: JOB_ID.into(),
        entrypoint: "train.py".into(),
        submitter_device_id: SUBMITTER_ID.into(),
        issued_at_unix_ms: now.saturating_sub(60_000),
        expires_at_unix_ms: now + 7 * 24 * 3_600_000,
        ..Default::default()
    };
    manifest.submitter_signature = sign(&submitter, &manifest).to_vec();
    let mut ring = InMemoryKeyring::new();
    ring.insert(SUBMITTER_ID, submitter.verifying_key());
    let verified = verify(&manifest, 1, &Ed25519Verifier::new(ring), now, &mut NoReplayCheck)
        .expect("fixture Manifest 서명이 검증된다");
    let mut jobs = CoordinatorJobStore::open(path).expect("job store");
    jobs.submit_verified_manifest(
        &AcceptedJobSubmission {
            idempotency_key: [1; 16],
            job_id: JOB_ID.into(),
            submitter_device_id: SUBMITTER_ID.into(),
            manifest_hash: blake3_256(&signing_input(&manifest)),
            deadline_unix_ms: Some(now + 3_600_000),
            max_queue_duration_ms: Some(3_600_000),
        },
        &verified,
        now,
    )
    .expect("submit");
    jobs.start_planning(JOB_ID, now).expect("planning");
    jobs.enqueue(JOB_ID, "plan-1", now).expect("enqueue");
    drop(jobs);

    CoordinatorStagingStore::open(path)
        .expect("staging store")
        .reserve_node_and_stage_queued_with_lease(
            &StageQueuedRequest {
                operation_key: [2; 16],
                job_id: JOB_ID.into(),
                attempt_id: ATTEMPT_ID.into(),
                lease_id: LEASE_ID.into(),
                node_id: NODE_ID.into(),
                selected_gpu_ids: vec!["gpu-1".into()],
                issuing_coordinator_id: COORDINATOR_ID.into(),
                coordinator_term: 1,
                issued_at_unix_ms: now,
                renew_after_unix_ms: now + 300_000,
                expires_at_unix_ms: now + 600_000,
                max_total_duration_seconds: 3_600,
            },
            7,
        )
        .expect("reserve + stage");
}

fn staged_fence_epoch(path: &Path) -> u64 {
    CoordinatorStagingStore::open(path)
        .expect("staging store")
        .get_attempt(ATTEMPT_ID)
        .expect("attempt 조회")
        .expect("fixture 가 Attempt 를 만들었다")
        .fence_epoch
}

/// 커널이 고른 포트를 쓰되, Coordinator 가 그 주소로 bind 하도록
/// **먼저 잡았다가 놓는다.** `run()` 은 실제 주소를 stdout 에만 찍어서
/// 라이브러리 호출자가 읽을 방법이 없다.
fn reserve_loopback_port() -> SocketAddr {
    let reserved = TcpListener::bind("127.0.0.1:0").expect("포트 예약");
    let address = reserved.local_addr().expect("주소");
    drop(reserved);
    address
}

struct Fixture {
    _dir: tempfile::TempDir,
    control_db: PathBuf,
    keyring: PathBuf,
    address: SocketAddr,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("임시 디렉터리");
    let control_db = dir.path().join("control.sqlite3");
    prepare_control_db(&control_db);
    let keyring = dir.path().join("submitters.keyring");
    let mut ring = PersistentKeyring::new(
        &keyring,
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .expect("keyring 생성");
    ring.insert_public(SUBMITTER_ID, SigningKey::from_bytes(&SUBMITTER_SEED).verifying_key())
        .expect("공개키 등록");
    ring.save().expect("keyring 저장");
    Fixture {
        _dir: dir,
        control_db,
        keyring,
        address: reserve_loopback_port(),
    }
}

fn coordinator_args(fixture: &Fixture, expect_attempt_reports: u32) -> Vec<String> {
    let agent_pubkey = hex(
        SigningKey::from_bytes(&AGENT_SEED)
            .verifying_key()
            .as_bytes(),
    );
    [
        "--listen",
        &fixture.address.to_string(),
        "--own-seed",
        &hex(&COORDINATOR_SEED),
        "--peer-pubkey",
        &agent_pubkey,
        "--coordinator-device-id",
        COORDINATOR_ID,
        "--agent-device-id",
        NODE_ID,
        "--grant-id",
        GRANT_ID,
        "--attempt-id",
        ATTEMPT_ID,
        "--lease-id",
        LEASE_ID,
        "--job-id",
        JOB_ID,
        "--i-understand-legacy-mode-is-unsafe",
        "true",
        "--grant-from-control-db",
        fixture.control_db.to_str().expect("경로"),
        "--stored-grant-job-id",
        JOB_ID,
        "--stored-grant-attempt-id",
        ATTEMPT_ID,
        "--stored-grant-lease-id",
        LEASE_ID,
        "--submitter-keyring",
        fixture.keyring.to_str().expect("경로"),
        "--i-understand-plaintext-keyring-is-unsafe",
        "true",
        "--expect-attempt-reports",
        &expect_attempt_reports.to_string(),
        "--max-connections",
        "1",
        "--accept-timeout-ms",
        "15000",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn spawn_coordinator(
    fixture: &Fixture,
    expect_attempt_reports: u32,
) -> std::thread::JoinHandle<Result<(), String>> {
    let args = coordinator_args(fixture, expect_attempt_reports);
    std::thread::spawn(move || {
        let config = gputeer_coordinator::parse_config_from_args(&args).expect("설정 파싱");
        gputeer_coordinator::run(config)
    })
}

fn connect_when_ready(address: SocketAddr) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match TcpStream::connect_timeout(&address, Duration::from_millis(200)) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .expect("읽기 타임아웃");
                stream
                    .set_write_timeout(Some(Duration::from_secs(10)))
                    .expect("쓰기 타임아웃");
                return stream;
            }
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("Coordinator 가 {address} 에 뜨지 않았다: {error}"),
        }
    }
}

/// 프레임 하나를 읽는다. 검증은 하지 않는다 — 이 테스트는 Agent 역할이고
/// 재려는 것은 Coordinator 쪽 검증이다.
fn read_frame_body(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut header = [0u8; 5];
    stream.read_exact(&mut header).expect("프레임 헤더");
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).expect("프레임 본문");
    (header[0], body)
}

fn write_frame_body(stream: &mut TcpStream, frame_type: FrameType, body: &[u8]) {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(frame_type as u8);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    stream.write_all(&out).expect("프레임 전송");
    stream.flush().expect("flush");
}

/// Grant 를 받고 유효한 `AgentGrantAck` 로 답한다.
fn handshake(stream: &mut TcpStream) -> pb::ExecutionGrant {
    handshake_at(stream, 0)
}

/// `connection_attempt` 번째 연결의 handshake — Hello 와 ACK nonce 가 그 번호를 쓴다(Coordinator 가 대조한다).
fn handshake_at(stream: &mut TcpStream, connection_attempt: u32) -> pb::ExecutionGrant {
    // D2 — 모든 연결은 Agent 의 Hello(FRESH) 로 시작한다.
    let hello_key = SigningKey::from_bytes(&AGENT_SEED);
    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        mode: gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT,
        node_id: NODE_ID.into(),
        connection_attempt,
        issued_at_unix_ms: now_ms(),
        nonce: (100u8..116).map(|b| b.wrapping_add((connection_attempt as u8).wrapping_mul(16))).collect(),
        ..Default::default()
    };
    hello.node_signature = sign(&hello_key, &hello).to_vec();
    write_frame_body(stream, FrameType::SessionHello, &hello.encode_to_vec());

    let (frame_type, body) = read_frame_body(stream);
    assert_eq!(
        frame_type,
        FrameType::Grant as u8,
        "첫 프레임은 Grant 여야 한다"
    );
    let grant = pb::ExecutionGrant::decode(body.as_slice()).expect("Grant 디코드");

    let agent_key = SigningKey::from_bytes(&AGENT_SEED);
    let now = now_ms();
    let mut ack = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: grant.grant_id.clone(),
        attempt_id: grant.attempt_id.clone(),
        agent_device_id: NODE_ID.into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_replay_nonce("grant-ack", &grant.grant_id, connection_attempt),
        accepted: true,
        ..Default::default()
    };
    ack.agent_signature = sign(&agent_key, &ack).to_vec();
    write_frame_body(stream, FrameType::GrantAck, &ack.encode_to_vec());
    grant
}

/// 이 Attempt 에 대한 **정상** 종료 보고.
fn terminal_report(fence_epoch: u64) -> pb::AttemptReport {
    let started = now_ms();
    signed_report(pb::AttemptReport {
        schema_version: 1,
        job_id: JOB_ID.into(),
        attempt_id: ATTEMPT_ID.into(),
        node_id: NODE_ID.into(),
        fence_epoch,
        outcome: pb::AttemptOutcome::Completed as i32,
        started_at_unix_ms: started,
        finished_at_unix_ms: started + 5,
        issued_at_unix_ms: started + 6,
        ..Default::default()
    })
}

fn signed_report(mut report: pb::AttemptReport) -> pb::AttemptReport {
    let agent_key = SigningKey::from_bytes(&AGENT_SEED);
    report.node_signature = sign(&agent_key, &report).to_vec();
    report
}

fn stored_binding(path: &Path) -> Option<pb::AttemptReport> {
    CoordinatorAttemptReportStore::open(path)
        .expect("보고 저장소")
        .get_report_binding(ATTEMPT_ID, NODE_ID)
        .expect("보고 조회")
        .map(|binding| binding.report)
}

/// 한 번의 세션을 돌리고 Coordinator 의 판정을 돌려준다.
fn run_session_with(reports: &[pb::AttemptReport], expect: u32) -> (Fixture, Result<(), String>) {
    let fixture = fixture();
    let handle = spawn_coordinator(&fixture, expect);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    for report in reports {
        write_frame_body(
            &mut stream,
            FrameType::AttemptReport,
            &report.encode_to_vec(),
        );
    }
    let outcome = handle.join().expect("Coordinator 스레드가 panic 하지 않았다");
    drop(stream);
    (fixture, outcome)
}

// ── 정상 경로 ────────────────────────────────────────────────────────

/// 서명된 종료 보고가 wire 를 건너 **durable 하게 저장된다.**
#[test]
fn a_signed_terminal_report_crosses_the_wire_and_lands_in_the_store() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);

    let report = terminal_report(fence_epoch);
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &report.encode_to_vec(),
    );

    let outcome = handle.join().expect("Coordinator 스레드");
    assert!(outcome.is_ok(), "정상 보고가 거부됐다: {outcome:?}");

    let stored = stored_binding(&fixture.control_db).expect("증거가 저장돼 있어야 한다");
    assert_eq!(stored, report, "저장된 것이 보낸 것과 바이트 단위로 같아야 한다");
}

/// 같은 보고를 두 번 보내면 **행은 하나**이고 오류도 아니다.
///
/// `CLAUDE.md` §4 — "동일 요청 10회 -> side effect 1회" 는 말이 아니라
/// 테스트로 증명한다.
#[test]
fn the_same_report_sent_twice_is_idempotent() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let handle = spawn_coordinator(&fixture, 2);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);

    let report = terminal_report(fence_epoch);
    for _ in 0..2 {
        write_frame_body(
            &mut stream,
            FrameType::AttemptReport,
            &report.encode_to_vec(),
        );
    }

    let outcome = handle.join().expect("Coordinator 스레드");
    assert!(outcome.is_ok(), "같은 보고의 재전송이 거부됐다: {outcome:?}");
    assert_eq!(
        stored_binding(&fixture.control_db).expect("증거"),
        report,
        "재전송이 저장된 증거를 바꾸면 안 된다"
    );
}

// ── negative ─────────────────────────────────────────────────────────

/// 위조 서명은 **검증 단계에서** 막힌다 — 저장소에 닿지 않는다.
#[test]
fn a_forged_signature_is_refused_before_the_store() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let mut report = terminal_report(fence_epoch);
    *report
        .node_signature
        .last_mut()
        .expect("서명이 비어 있지 않다") ^= 0x01;

    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &report.encode_to_vec(),
    );

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("위조 서명은 거부돼야 한다");
    assert!(
        error.contains("AttemptReport 프레임 읽기/검증 실패"),
        "거부 사유가 서명 검증 실패라고 말해야 한다: {error}"
    );
    assert!(
        stored_binding(&fixture.control_db).is_none(),
        "검증에 실패한 보고가 저장됐다"
    );
}

/// 다른 Attempt 의 보고는 거부된다 — **사유가 attempt_id 라고 말해야 한다.**
#[test]
fn a_report_for_another_attempt_is_refused_by_attempt_id() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let mut report = terminal_report(fence_epoch);
    report.attempt_id = "attempt-somewhere-else".into();
    let report = signed_report(report);

    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &report.encode_to_vec(),
    );

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("다른 Attempt 의 보고는 거부돼야 한다");
    assert!(
        error.contains("ATTEMPT_REPORT_REJECTED: attempt_id 불일치"),
        "거부 사유가 attempt_id 라고 말해야 한다: {error}"
    );
    assert!(
        error.contains(ATTEMPT_ID) && error.contains("attempt-somewhere-else"),
        "기대값과 실제값이 둘 다 있어야 한다: {error}"
    );
    assert!(
        stored_binding(&fixture.control_db).is_none(),
        "다른 Attempt 의 보고가 저장됐다"
    );
}

/// 다른 Job 의 보고도 거부된다 — 사유는 job_id 다.
#[test]
fn a_report_for_another_job_is_refused_by_job_id() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let mut report = terminal_report(fence_epoch);
    report.job_id = "job-somewhere-else".into();
    let report = signed_report(report);

    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &report.encode_to_vec(),
    );

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("다른 Job 의 보고는 거부돼야 한다");
    assert!(
        error.contains("ATTEMPT_REPORT_REJECTED: job_id 불일치"),
        "거부 사유가 job_id 라고 말해야 한다: {error}"
    );
    assert!(
        error.contains(JOB_ID) && error.contains("job-somewhere-else"),
        "기대값과 실제값이 둘 다 있어야 한다: {error}"
    );
    assert!(stored_binding(&fixture.control_db).is_none());
}

/// 다른 세대의 보고도 거부된다 — 사유는 fence_epoch 다.
///
/// ★ 세대를 안 보면 **폐기된 Lease 로 돌던 옛 실행**의 종료가 지금 세대의
///   증거로 저장된다.
#[test]
fn a_report_from_another_fence_epoch_is_refused_by_fence_epoch() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let mut report = terminal_report(fence_epoch);
    report.fence_epoch = fence_epoch.wrapping_add(1);
    let report = signed_report(report);

    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &report.encode_to_vec(),
    );

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("다른 세대의 보고는 거부돼야 한다");
    assert!(
        error.contains("ATTEMPT_REPORT_REJECTED: fence_epoch 불일치"),
        "거부 사유가 fence_epoch 라고 말해야 한다: {error}"
    );
    assert!(
        error.contains(&fence_epoch.to_string()),
        "발급 세대가 메시지에 있어야 한다: {error}"
    );
    assert!(stored_binding(&fixture.control_db).is_none());
}

/// terminal 이 아닌 outcome 은 종료 증거가 아니다.
///
/// ★ `ATTEMPT_OUTCOME_UNSPECIFIED = 0` 은 "안 정했다" 이지 "끝났다" 가
///   아니다. 이것을 받아들이면 아직 도는 Attempt 의 예약이 풀릴 근거가
///   생긴다.
#[test]
fn a_non_terminal_outcome_is_refused_with_the_outcome_value() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let mut report = terminal_report(fence_epoch);
    report.outcome = pb::AttemptOutcome::Unspecified as i32;
    let report = signed_report(report);

    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &report.encode_to_vec(),
    );

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("terminal 이 아닌 outcome 은 거부돼야 한다");
    assert!(
        error.contains("terminal 이 아니다"),
        "거부 사유가 terminal 판정이라고 말해야 한다: {error}"
    );
    assert!(
        error.contains("outcome 0"),
        "어떤 outcome 이 거부됐는지 말해야 한다: {error}"
    );
    assert!(stored_binding(&fixture.control_db).is_none());
}

/// B+E 조건 (a) — 이 수신 경로는 schema_version 2 보고를 읽는다. 지원 버전을 1 로 두면 SCHEMA_TOO_NEW 로 거부된다.
#[test]
fn a_v2_report_with_an_observed_zero_exit_crosses_the_wire_and_is_stored() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let mut report = terminal_report(fence_epoch);
    report.schema_version = 2;
    report.exit_observation = pb::ExitObservation::ObservedWithCode as i32;
    let report = signed_report(report);

    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &report.encode_to_vec(),
    );

    let outcome = handle.join().expect("Coordinator 스레드");
    assert!(outcome.is_ok(), "정상 v2 보고가 거부됐다: {outcome:?}");
    assert_eq!(stored_binding(&fixture.control_db), Some(report));
}

/// 필드 조합 규칙(§5.7 (4)) — 수신 단계가 저장소를 **건드리기 전에** 거부한다. 저장소도 같은 검사를 하므로,
/// 거부 문구에 저장소의 말("field combination rejected")이 없는 것까지 본다 — 수신 검사를 지우면 그 말로 바뀐다.
#[test]
fn a_v2_report_that_breaks_the_field_rules_is_refused_before_the_store() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let mut report = terminal_report(fence_epoch);
    report.schema_version = 2;
    // 종료를 관측하지 못했는데 COMPLETED — 허용 표 밖이다.
    report.exit_observation = pb::ExitObservation::NotObserved as i32;
    let report = signed_report(report);

    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &report.encode_to_vec(),
    );

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("허용 표 밖의 보고는 거부돼야 한다");
    assert!(
        error.contains("ATTEMPT_REPORT_REJECTED: ATTEMPT_REPORT_RULE: 허용하지 않는 조합"),
        "수신 단계의 규칙 거부여야 한다: {error}"
    );
    assert!(
        !error.contains("field combination rejected"),
        "저장소까지 가서 거부됐다 — 수신 검사가 빠졌다: {error}"
    );
    assert!(stored_binding(&fixture.control_db).is_none());
}

/// D2 — Hello 없이 다른 프레임부터 보내는 연결(D2 이전 Agent 의 모양)은 **명시적으로** 거부된다(HELLO_MISSING).
///   서로 상대가 먼저 말하기를 기다리다 원인 모를 시간 초과로 끝나지 않게 한다.
#[test]
fn a_first_frame_that_is_not_a_hello_is_refused_as_hello_missing() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    // Hello 대신 서명된 종료 보고부터 보낸다.
    write_frame_body(
        &mut stream,
        FrameType::AttemptReport,
        &terminal_report(fence_epoch).encode_to_vec(),
    );

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("Hello 로 시작하지 않는 연결은 거부돼야 한다");
    assert!(error.contains("HELLO_MISSING"), "거부 사유가 Hello 부재라고 말해야 한다: {error}");
    assert!(stored_binding(&fixture.control_db).is_none());
}

/// D2 — 다른 lane(Resume) 용으로 서명한 Hello 는 mode 대조로 거부된다(HELLO_REJECTED).
///   서명은 유효하다 — 서명 검증으로는 이 혼동을 못 잡는다.
#[test]
fn a_hello_for_another_mode_is_refused() {
    let fixture = fixture();
    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    let key = SigningKey::from_bytes(&AGENT_SEED);
    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        mode: gputeer_protocol::constants::MODE_RESUME,
        node_id: NODE_ID.into(),
        connection_attempt: 0,
        issued_at_unix_ms: now_ms(),
        nonce: (120u8..136).collect(),
        ..Default::default()
    };
    hello.node_signature = sign(&key, &hello).to_vec();
    write_frame_body(&mut stream, FrameType::SessionHello, &hello.encode_to_vec());

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("다른 mode 의 Hello 는 거부돼야 한다");
    assert!(error.contains("HELLO_REJECTED: mode 불일치"), "{error}");
    assert!(stored_binding(&fixture.control_db).is_none());
}

// ── B+E 구현 단계 5a — RENEW 세션(새 연결로 갱신만) ─────────────────────

/// 연결 둘(FRESH 한 번 · RENEW 한 번)을 받는 Coordinator. `with_lease_db` 면 같은 control DB 를 lease 저장소로 쓴다 —
/// staging 저장소가 Lease 를 `coordinator_leases` 에 넣으므로 저장된 예약의 Lease 를 RENEW 가 찾는다.
fn spawn_coordinator_for_renew(
    fixture: &Fixture,
    with_lease_db: bool,
) -> std::thread::JoinHandle<Result<(), String>> {
    let mut args = coordinator_args(fixture, 0);
    let at = args
        .iter()
        .position(|arg| arg == "--max-connections")
        .expect("--max-connections");
    args[at + 1] = "2".into();
    if with_lease_db {
        args.push("--lease-db".into());
        args.push(fixture.control_db.to_str().expect("경로").into());
    }
    std::thread::spawn(move || {
        let config = gputeer_coordinator::parse_config_from_args(&args).expect("설정 파싱");
        gputeer_coordinator::run(config)
    })
}

fn send_hello(stream: &mut TcpStream, mode: i32, connection_attempt: u32, nonce_start: u8) {
    let key = SigningKey::from_bytes(&AGENT_SEED);
    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        mode,
        node_id: NODE_ID.into(),
        connection_attempt,
        issued_at_unix_ms: now_ms(),
        nonce: (nonce_start..nonce_start + 16).collect(),
        ..Default::default()
    };
    hello.node_signature = sign(&key, &hello).to_vec();
    write_frame_body(stream, FrameType::SessionHello, &hello.encode_to_vec());
}

fn signed_renew_request(fence_epoch: u64, nonce_start: u8) -> pb::RenewLeaseRequest {
    let key = SigningKey::from_bytes(&AGENT_SEED);
    let mut request = pb::RenewLeaseRequest {
        schema_version: 1,
        lease_id: LEASE_ID.into(),
        fence_epoch,
        node_id: NODE_ID.into(),
        issued_at_unix_ms: now_ms(),
        nonce: (nonce_start..nonce_start + 16).collect(),
        ..Default::default()
    };
    request.node_signature = sign(&key, &request).to_vec();
    request
}

/// 연결 1 은 FRESH(Grant -> ACK), 연결 2 는 RENEW 로 갱신 요청 하나를 보낸다. 받은 응답을 돌려준다.
fn fresh_then_renew(fixture: &Fixture, fence_epoch: u64) -> (pb::RenewLeaseRequest, pb::RenewLeaseResult) {
    {
        let mut fresh = connect_when_ready(fixture.address);
        handshake(&mut fresh);
    }
    let mut renew = connect_when_ready(fixture.address);
    send_hello(&mut renew, gputeer_protocol::constants::MODE_RENEW, 1, 140);
    let request = signed_renew_request(fence_epoch, 160);
    write_frame_body(&mut renew, FrameType::LeaseRenew, &request.encode_to_vec());
    let (frame_type, body) = read_frame_body(&mut renew);
    assert_eq!(frame_type, FrameType::LeaseRenewResult as u8, "RENEW 세션의 응답은 갱신 결과다");
    (request, pb::RenewLeaseResult::decode(body.as_slice()).expect("갱신 결과 디코드"))
}

/// 단계 5a — FRESH 연결을 닫은 뒤 **새 연결**(RENEW)로 저장된 Lease 를 갱신한다. 결과의 바깥 서명 · 요청 nonce, 그리고 중첩 Lease 의
///   독립 서명과 Agent 가 대조하는 신원(lease · job · attempt · 발급자 · 보유자 · 세대)을 본다(결함 96). 워크로드가 도는 **동안**의
///   갱신은 cli 통합 테스트가 본다(단계 5b).
#[test]
fn a_renew_session_on_a_new_connection_renews_the_stored_lease() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let handle = spawn_coordinator_for_renew(&fixture, true);
    let (request, result) = fresh_then_renew(&fixture, fence_epoch);

    assert_eq!(result.outcome, 1, "RENEWED 여야 한다: {result:?}");
    assert_eq!(result.request_nonce, request.nonce, "요청 nonce 를 되돌려야 한다");
    let lease = result.lease.clone().expect("갱신된 Lease 가 실린다");
    assert!(lease.expires_at_unix_ms > now_ms(), "갱신된 만료가 지금보다 뒤다");
    let mut ring = InMemoryKeyring::new();
    ring.insert(COORDINATOR_ID, SigningKey::from_bytes(&COORDINATOR_SEED).verifying_key());
    let verifier = Ed25519Verifier::new(ring);
    verify(&result, 1, &verifier, now_ms(), &mut NoReplayCheck)
        .expect("Coordinator 가 서명한 갱신 결과다");
    assert_eq!(result.coordinator_id, COORDINATOR_ID, "결과의 coordinator_id");
    // ★ 결함 96 — 중첩 Lease 는 바깥 서명과 **따로** 검증한다(Agent 의 규칙 i). 바깥만 보면 중첩 서명이 깨져도 통과한다.
    verify(&lease, 1, &verifier, now_ms(), &mut NoReplayCheck)
        .expect("중첩 Lease 도 Coordinator 가 따로 서명했다");
    assert_eq!(
        (
            lease.lease_id.as_str(),
            lease.job_id.as_str(),
            lease.attempt_id.as_str(),
            lease.issuing_coordinator_id.as_str(),
            lease.holder_node_id.as_str(),
            lease.fence_epoch,
        ),
        (LEASE_ID, JOB_ID, ATTEMPT_ID, COORDINATOR_ID, NODE_ID, fence_epoch),
        "Agent 가 대조하는 신원이 그대로여야 한다"
    );
    let outcome = handle.join().expect("Coordinator 스레드");
    assert!(outcome.is_ok(), "{outcome:?}");
}

/// 단계 5a — 낮은 세대의 갱신은 연결을 끊지 않고 서명된 SUPERSEDED 로 답한다(FRESH 연결 안의 갱신과 같은 규칙).
#[test]
fn a_renew_session_with_a_lower_fence_epoch_is_superseded() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    assert!(fence_epoch > 0, "fixture 의 세대가 0 이면 더 낮은 세대를 만들 수 없다");
    let handle = spawn_coordinator_for_renew(&fixture, true);
    let (_, result) = fresh_then_renew(&fixture, fence_epoch - 1);
    assert_eq!(result.outcome, 2, "SUPERSEDED 여야 한다: {result:?}");
    assert!(result.lease.is_none(), "물러나라는 응답에 새 Lease 를 싣지 않는다");
    let outcome = handle.join().expect("Coordinator 스레드");
    assert!(outcome.is_ok(), "{outcome:?}");
}

/// 단계 5a — 영속 lease 저장소 없이 들어온 RENEW 는 거부한다. 연결 밖 갱신을 판정할 근거가 없다.
#[test]
fn a_renew_session_without_a_lease_db_is_refused() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let handle = spawn_coordinator_for_renew(&fixture, false);
    {
        let mut fresh = connect_when_ready(fixture.address);
        handshake(&mut fresh);
    }
    let mut renew = connect_when_ready(fixture.address);
    send_hello(&mut renew, gputeer_protocol::constants::MODE_RENEW, 1, 140);
    write_frame_body(
        &mut renew,
        FrameType::LeaseRenew,
        &signed_renew_request(fence_epoch, 160).encode_to_vec(),
    );
    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("저장소 없는 RENEW 는 거부돼야 한다");
    assert!(error.contains("RENEW_SESSION_REFUSED"), "{error}");
}

/// 연결 `max_connections` 개 · 추가 인자를 받는 Coordinator.
fn spawn_coordinator_with(
    fixture: &Fixture,
    max_connections: u32,
    extra: &[&str],
) -> std::thread::JoinHandle<Result<(), String>> {
    let mut args = coordinator_args(fixture, 0);
    let at = args
        .iter()
        .position(|arg| arg == "--max-connections")
        .expect("--max-connections");
    args[at + 1] = max_connections.to_string();
    args.extend(extra.iter().map(|arg| arg.to_string()));
    std::thread::spawn(move || {
        let config = gputeer_coordinator::parse_config_from_args(&args).expect("설정 파싱");
        gputeer_coordinator::run(config)
    })
}

/// 결함 104 (재검수 62) — 저장된 예약 lane 에서 저장소를 **읽다가** 난 장애(손상된 Lease 행)는 Storage(fail-closed)다 — 받을 연결이
///   남아 있어도 리스너를 멈춘다. 전에는 모든 오류를 Protocol 로 고정해 다음 연결을 받았다.
#[test]
fn a_corrupt_stored_lease_row_on_the_stored_lane_stops_the_listener() {
    let fixture = fixture();
    {
        let db = rusqlite::Connection::open(&fixture.control_db).expect("control DB");
        let changed = db
            .execute(
                "UPDATE coordinator_leases SET fence_epoch = X'00' WHERE lease_id = ?1",
                [LEASE_ID],
            )
            .expect("Lease 행 손상");
        assert_eq!(changed, 1, "fixture 의 Lease 행이 하나 있어야 한다");
    }
    let handle = spawn_coordinator_with(&fixture, 2, &[]);
    let mut stream = connect_when_ready(fixture.address);
    send_hello(&mut stream, gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT, 0, 100);
    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("손상된 저장소는 리스너를 멈춰야 한다");
    assert!(error.contains("Lease 조회 실패"), "{error}");
}

/// 결함 111 (재검수 64) — Manifest 행이 없는 옛 hash-only Job 은 저장소 장애가 아니라 **거부**다. 첫 연결이 거부돼도 리스너는 두 번째
///   연결을 받는다. 104 는 이것까지 Storage 로 묶어 옛 Job 하나로 리스너를 멈췄다.
#[test]
fn a_legacy_job_without_a_manifest_is_refused_and_the_listener_keeps_accepting() {
    let fixture = fixture();
    {
        let db = rusqlite::Connection::open(&fixture.control_db).expect("control DB");
        let changed = db
            .execute("DELETE FROM coordinator_job_manifests WHERE job_id = ?1", [JOB_ID])
            .expect("Manifest 행 삭제");
        assert_eq!(changed, 1, "fixture 의 Manifest 행이 하나 있어야 한다");
    }
    let handle = spawn_coordinator_with(&fixture, 2, &[]);
    let mut first = connect_when_ready(fixture.address);
    send_hello(&mut first, gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT, 0, 100);
    // Coordinator 가 거부하고 닫을 때까지 기다린다. 닫힘은 EOF 로도 연결 재설정 오류로도 온다 — 어느 쪽이든 닫힌 것이다.
    let mut rest = Vec::new();
    let closed = first.read_to_end(&mut rest);
    assert!(rest.is_empty(), "거부된 연결에 프레임이 왔다({} 바이트, {closed:?})", rest.len());
    // Storage 로 분류했다면 리스너가 멈춰 이 연결이 거부되거나, 아래 join 이 저장소 문구로 끝난다.
    let mut second = TcpStream::connect_timeout(&fixture.address, Duration::from_secs(2))
        .expect("첫 거부 뒤에도 리스너가 두 번째 연결을 받아야 한다");
    second.set_write_timeout(Some(Duration::from_secs(10))).expect("쓰기 타임아웃");
    send_hello(&mut second, gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT, 1, 120);
    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("마지막 연결의 거부는 오류로 끝난다");
    assert!(
        error.contains("GRANT_REFUSED") && error.contains("옛 Job") && !error.contains("저장된 Manifest 를 읽지 못했다"),
        "{error}"
    );
}

/// 결함 105 (재검수 62) — `--revoke-before-renew` 의 revoke 저장이 락 시한으로 실패하면 Storage(fail-closed)다(결함 99 의 음성 테스트).
///   순서로 보장한다 — Grant 를 받은 뒤 ACK 를 보류하고, 다른 연결로 BEGIN IMMEDIATE 를 잡은 다음 ACK 를 보낸다(lease 저장소 busy
///   timeout 1초). 리스너가 멈추고 revoke 는 기록되지 않는다.
#[test]
fn a_revoke_store_lock_timeout_before_renew_stops_the_listener() {
    let fixture = fixture();
    let control_db = fixture.control_db.to_str().expect("경로").to_string();
    let handle = spawn_coordinator_with(
        &fixture,
        2,
        &["--lease-db", control_db.as_str(), "--revoke-before-renew", "true"],
    );
    let mut stream = connect_when_ready(fixture.address);
    send_hello(&mut stream, gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT, 0, 100);
    let (frame_type, body) = read_frame_body(&mut stream);
    assert_eq!(frame_type, FrameType::Grant as u8, "첫 프레임은 Grant 여야 한다");
    let grant = pb::ExecutionGrant::decode(body.as_slice()).expect("Grant 디코드");

    let lock = rusqlite::Connection::open(&fixture.control_db).expect("control DB");
    lock.execute_batch("BEGIN IMMEDIATE").expect("쓰기 락");
    let key = SigningKey::from_bytes(&AGENT_SEED);
    let now = now_ms();
    let mut ack = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: grant.grant_id.clone(),
        attempt_id: grant.attempt_id.clone(),
        agent_device_id: NODE_ID.into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_replay_nonce("grant-ack", &grant.grant_id, 0),
        accepted: true,
        ..Default::default()
    };
    ack.agent_signature = sign(&key, &ack).to_vec();
    write_frame_body(&mut stream, FrameType::GrantAck, &ack.encode_to_vec());
    let outcome = handle.join().expect("Coordinator 스레드");
    lock.execute_batch("ROLLBACK").expect("락 해제");

    let error = outcome.expect_err("revoke 저장 실패는 리스너를 멈춰야 한다");
    assert!(error.contains("갱신 전 revoke 저장 실패"), "{error}");
    let stored = CoordinatorLeaseStore::open(&fixture.control_db)
        .expect("lease store")
        .get(LEASE_ID)
        .expect("Lease 조회")
        .expect("Lease");
    assert!(stored.revoked_at_unix_ms.is_none(), "실패했는데 revoke 가 기록됐다");
}

/// 결함 100 (재검수 61) — 발급한 Lease 를 **다른** lease 저장소에서 찾지 못하는 구성(--grant-from-control-db A · --lease-db B)은
///   구성 오류라 fail-closed(Storage)다 — 받을 연결이 남아 있어도 리스너를 멈춘다. 상대가 고른 모르는 ID(Protocol)와 다르다.
#[test]
fn a_fresh_renew_whose_lease_lives_in_another_store_stops_the_listener() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let other_db = fixture.control_db.with_file_name("other-lease.sqlite3");
    let mut args = coordinator_args(&fixture, 0);
    let at = args
        .iter()
        .position(|arg| arg == "--max-connections")
        .expect("--max-connections");
    args[at + 1] = "2".into();
    for extra in ["--lease-db", other_db.to_str().expect("경로"), "--do-renew", "true"] {
        args.push(extra.into());
    }
    let handle = std::thread::spawn(move || {
        let config = gputeer_coordinator::parse_config_from_args(&args).expect("설정 파싱");
        gputeer_coordinator::run(config)
    });
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    write_frame_body(
        &mut stream,
        FrameType::LeaseRenew,
        &signed_renew_request(fence_epoch, 160).encode_to_vec(),
    );
    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("구성 오류는 리스너를 멈춰야 한다");
    assert!(error.contains("다른 저장소에서 찾는 구성 문제"), "{error}");
}

/// 결함 87 — 첫 프레임의 **내용**이 오류 분류를 바꾸지 못한다. 등록된 Agent 가 job_id 에 "lease store" 를 넣은 서명된 보고를
///   Hello 대신 보내도, 리스너 전체를 멈추는 Storage 가 아니라 그 연결만의 Protocol 오류다 — 다음 정상 연결을 받는다.
#[test]
fn a_first_frame_whose_content_mentions_the_lease_store_does_not_stop_the_listener() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    // 연결 둘 · 보고 기대 0 · lease 저장소 없음(분류기가 "lease store" 로 Storage 를 고르던 경로 그대로).
    let handle = spawn_coordinator_for_renew(&fixture, false);
    {
        let mut first = connect_when_ready(fixture.address);
        let mut report = terminal_report(fence_epoch);
        report.job_id = "lease store".into();
        let report = signed_report(report);
        write_frame_body(&mut first, FrameType::AttemptReport, &report.encode_to_vec());
        // Coordinator 가 이 연결을 닫을 때까지 기다린다 — 먼저 끊으면 다른 원인(끊김)이 섞인다.
        let mut probe = [0u8; 1];
        let _closed_by_coordinator = first.read(&mut probe);
    }
    let mut second = connect_when_ready(fixture.address);
    handshake_at(&mut second, 1);
    let outcome = handle.join().expect("Coordinator 스레드");
    assert!(outcome.is_ok(), "받은 메시지의 내용 때문에 리스너가 멈췄다: {outcome:?}");
}

/// 결함 89 — 도착했지만 검증에 실패한 Hello(서명 변조)는 HELLO_MISSING 이 아니라 HELLO_REJECTED 다.
#[test]
fn a_hello_with_a_broken_signature_is_rejected_not_missing() {
    let fixture = fixture();
    let handle = spawn_coordinator(&fixture, 1);
    let mut stream = connect_when_ready(fixture.address);
    let key = SigningKey::from_bytes(&AGENT_SEED);
    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        mode: gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT,
        node_id: NODE_ID.into(),
        connection_attempt: 0,
        issued_at_unix_ms: now_ms(),
        nonce: (180u8..196).collect(),
        ..Default::default()
    };
    hello.node_signature = sign(&key, &hello).to_vec();
    hello.node_signature[0] ^= 0x01;
    write_frame_body(&mut stream, FrameType::SessionHello, &hello.encode_to_vec());

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("서명이 틀린 Hello 는 거부돼야 한다");
    assert!(error.contains("HELLO_REJECTED"), "{error}");
    assert!(!error.contains("HELLO_MISSING"), "검증 실패를 Hello 부재로 적었다: {error}");
}

/// FRESH 한 판을 ACK 까지 가되 ACK 의 grant_id 를 `grant_id` 로 바꿔 보낸다(서명은 정상). Coordinator 는 상관관계 불일치로 이
/// 연결을 끝낸다 — 그 오류 문구에 상대가 보낸 grant_id 가 그대로 들어간다.
fn handshake_with_ack_grant_id(stream: &mut TcpStream, grant_id: &str) {
    send_hello(stream, gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT, 0, 100);
    let (frame_type, body) = read_frame_body(stream);
    assert_eq!(frame_type, FrameType::Grant as u8, "첫 프레임은 Grant 여야 한다");
    let grant = pb::ExecutionGrant::decode(body.as_slice()).expect("Grant 디코드");
    let key = SigningKey::from_bytes(&AGENT_SEED);
    let now = now_ms();
    let mut ack = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: grant_id.into(),
        attempt_id: grant.attempt_id.clone(),
        agent_device_id: NODE_ID.into(),
        issued_at_unix_ms: now,
        expires_at_unix_ms: now + 60_000,
        nonce: derive_replay_nonce("grant-ack", &grant.grant_id, 0),
        accepted: true,
        ..Default::default()
    };
    ack.agent_signature = sign(&key, &ack).to_vec();
    write_frame_body(stream, FrameType::GrantAck, &ack.encode_to_vec());
    // Coordinator 가 이 연결을 닫을 때까지 기다린다 — 먼저 끊으면 다른 원인(끊김)이 섞인다.
    let mut probe = [0u8; 1];
    let _closed_by_coordinator = stream.read(&mut probe);
}

/// 결함 92 — FRESH 연결 안의 Legacy 경로도 상대 내용으로 Storage 를 고르지 못한다. 서명된 ACK 의 grant_id 에 "lease store" 를
///   넣으면 상관관계 불일치 문구에 그 값이 들어간다 — 그래도 그 연결만의 Protocol 오류이고 다음 정상 연결을 받는다.
#[test]
fn an_ack_whose_grant_id_mentions_the_lease_store_does_not_stop_the_listener() {
    let fixture = fixture();
    let handle = spawn_coordinator_for_renew(&fixture, false);
    {
        let mut first = connect_when_ready(fixture.address);
        handshake_with_ack_grant_id(&mut first, "lease store");
    }
    let mut second = connect_when_ready(fixture.address);
    handshake_at(&mut second, 1);
    let outcome = handle.join().expect("Coordinator 스레드");
    assert!(outcome.is_ok(), "받은 ACK 의 내용 때문에 리스너가 멈췄다: {outcome:?}");
}

/// 결함 92 — durable 저장소일 때 "Lease" 와 "실패" 가 함께 든 문구를 Storage 로 고르던 규칙도 없앴다. 같은 모양의 ACK 에
///   grant_id "Lease 실패" 를 넣어 lease 저장소를 켠 Coordinator 로 보낸다.
#[test]
fn with_a_durable_lease_store_an_ack_mentioning_lease_failure_does_not_stop_the_listener() {
    let fixture = fixture();
    let handle = spawn_coordinator_for_renew(&fixture, true);
    {
        let mut first = connect_when_ready(fixture.address);
        handshake_with_ack_grant_id(&mut first, "Lease 실패");
    }
    let mut second = connect_when_ready(fixture.address);
    handshake_at(&mut second, 1);
    let outcome = handle.join().expect("Coordinator 스레드");
    assert!(outcome.is_ok(), "받은 ACK 의 내용 때문에 리스너가 멈췄다: {outcome:?}");
}

/// 결함 92 — 예상 밖 프레임은 **종류 이름**만 오류에 남긴다. ACK 자리에 서명된 보고를 보내고, 그 job_id 에 넣은 표지가 Coordinator
///   의 오류 문자열에 나오지 않는지 본다(전에는 `{other:?}` 가 `Verified` 안의 메시지를 통째로 찍었다).
#[test]
fn an_unexpected_frame_in_place_of_the_ack_is_named_by_kind_without_its_content() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let handle = spawn_coordinator(&fixture, 0);
    let mut stream = connect_when_ready(fixture.address);
    send_hello(&mut stream, gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT, 0, 100);
    let (frame_type, _) = read_frame_body(&mut stream);
    assert_eq!(frame_type, FrameType::Grant as u8, "첫 프레임은 Grant 여야 한다");
    let mut report = terminal_report(fence_epoch);
    report.job_id = "PEER-CONTENT-MARKER".into();
    write_frame_body(&mut stream, FrameType::AttemptReport, &signed_report(report).encode_to_vec());
    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("ACK 자리의 보고는 거부돼야 한다");
    assert!(error.contains("AttemptReport"), "무슨 종류가 왔는지는 말해야 한다: {error}");
    assert!(!error.contains("PEER-CONTENT-MARKER"), "상대가 보낸 내용이 오류 문자열에 들어갔다: {error}");
}

/// 결함 94 — 다른 Coordinator 가 발급한 Lease 는 RENEW 로 갱신하지 않는다 — **저장소를 바꾸기 전에** 거부한다.
///   같은 control DB 를 다른 신원(coordinator-elsewhere)의 Coordinator 가 열고, 보유 Agent 가 RENEW 를 첫 연결로 보낸다.
#[test]
fn a_renew_session_for_a_lease_issued_by_another_coordinator_is_refused_before_the_store_changes() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let before = CoordinatorLeaseStore::open(&fixture.control_db)
        .expect("lease store")
        .get(LEASE_ID)
        .expect("Lease 조회")
        .expect("fixture 가 Lease 를 만들었다");
    assert_eq!(before.issuing_coordinator_id, COORDINATOR_ID, "fixture 전제");
    let mut args = coordinator_args(&fixture, 0);
    let at = args
        .iter()
        .position(|arg| arg == "--coordinator-device-id")
        .expect("--coordinator-device-id");
    args[at + 1] = "coordinator-elsewhere".into();
    args.push("--lease-db".into());
    args.push(fixture.control_db.to_str().expect("경로").into());
    let handle = std::thread::spawn(move || {
        let config = gputeer_coordinator::parse_config_from_args(&args).expect("설정 파싱");
        gputeer_coordinator::run(config)
    });
    let mut renew = connect_when_ready(fixture.address);
    send_hello(&mut renew, gputeer_protocol::constants::MODE_RENEW, 0, 140);
    write_frame_body(
        &mut renew,
        FrameType::LeaseRenew,
        &signed_renew_request(fence_epoch, 160).encode_to_vec(),
    );
    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("다른 Coordinator 의 Lease 는 갱신하지 않는다");
    assert!(error.contains("가 발급한 Lease 가 아니다"), "{error}");
    let after = CoordinatorLeaseStore::open(&fixture.control_db)
        .expect("lease store")
        .get(LEASE_ID)
        .expect("Lease 조회")
        .expect("Lease");
    assert_eq!(after.expires_at_unix_ms, before.expires_at_unix_ms, "거부했는데 저장소의 만료가 바뀌었다");
}

/// 첫 증거와 **내용이 다른** 두 번째 보고는 거부된다.
///
/// ★ 이것이 없으면 노드가 같은 Attempt 에 대해 "성공" 을 보내고 나중에
///   "실패" 로 덮어쓸 수 있다 — 증거가 증거가 아니게 된다.
#[test]
fn a_second_report_that_contradicts_the_first_is_refused() {
    let fixture = fixture();
    let fence_epoch = staged_fence_epoch(&fixture.control_db);
    let first = terminal_report(fence_epoch);
    let mut second = first.clone();
    second.outcome = pb::AttemptOutcome::Failed as i32;
    let second = signed_report(second);

    let handle = spawn_coordinator(&fixture, 2);
    let mut stream = connect_when_ready(fixture.address);
    handshake(&mut stream);
    for report in [&first, &second] {
        write_frame_body(
            &mut stream,
            FrameType::AttemptReport,
            &report.encode_to_vec(),
        );
    }

    let error = handle
        .join()
        .expect("Coordinator 스레드")
        .expect_err("모순되는 두 번째 보고는 거부돼야 한다");
    assert!(
        error.contains("ATTEMPT_REPORT_REJECTED"),
        "이 계층의 거부라고 말해야 한다: {error}"
    );
    assert!(
        error.contains("conflicts with first durable evidence"),
        "첫 증거와 충돌한다고 말해야 한다: {error}"
    );
    assert_eq!(
        stored_binding(&fixture.control_db).expect("첫 증거"),
        first,
        "충돌하는 보고가 첫 증거를 덮어썼다"
    );
}

/// 보고를 기대하지 않으면 이 구간은 **통째로 없다** — 회귀 없음.
#[test]
fn a_session_that_expects_no_reports_behaves_exactly_as_before() {
    let (fixture, outcome) = run_session_with(&[], 0);
    assert!(outcome.is_ok(), "기존 handshake 가 깨졌다: {outcome:?}");
    assert!(
        stored_binding(&fixture.control_db).is_none(),
        "기대하지 않았는데 무언가 저장됐다"
    );
}
