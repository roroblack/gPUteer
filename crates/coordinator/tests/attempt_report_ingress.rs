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
    // D2 — 모든 연결은 Agent 의 Hello(FRESH) 로 시작한다.
    let hello_key = SigningKey::from_bytes(&AGENT_SEED);
    let mut hello = pb::AgentSessionHello {
        schema_version: 1,
        mode: gputeer_protocol::constants::MODE_MULTI_AGENT_GRANT,
        node_id: NODE_ID.into(),
        connection_attempt: 0,
        issued_at_unix_ms: now_ms(),
        nonce: (100u8..116).collect(),
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
        nonce: derive_replay_nonce("grant-ack", &grant.grant_id, 0),
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
