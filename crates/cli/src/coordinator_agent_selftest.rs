//! `gputeer coordinator-agent-selftest` — coordinator/agent 프로세스 경계 실측.
//!
//! `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md` 단계 4·5.
//!
//! `gputeer selftest` §5(2026-08-17)는 127.0.0.1 실제 TCP 소켓을 왕복하지만
//! **같은 프로세스 안 스레드 하나**가 서버를 연다(`crates/cli/src/selftest.rs:543`).
//! 이 서브커맨드는 그 다음 한 걸음이다 — `coordinator-stub`/`agent-stub`
//! 을 `Command::current_exe()` 로 **실제 별도 OS 프로세스**로 띄우고,
//! 서명된 `ExecutionGrant`/`AgentGrantAck` 가 그 프로세스 경계를 실제로
//! 넘어 검증되는지, 그리고 위조·replay 가 실제로 거부되는지 확인한다.
//!
//! # 위조를 어떻게 만드는가 (2026-08-18, 코덱스 설계 — `p56` 프롬프트)
//!
//! 정직하게 동작하는 coordinator/agent 프로세스는 자기 서명을 절대
//! 위조하지 않는다. 그래서 각 stub 에 테스트 전용 플래그
//! (`--corrupt-own-signature`, `--send-grant-twice`, `--expect-replay`)
//! 를 두어 "이 프로세스가 스스로 타락한 메시지를 보낸다" 를 흉내낸다
//! — `crates/coordinator/src/lib.rs`·`crates/agent/src/lib.rs` 참조.
//! TCP proxy 로 진짜 중간자 변조를 재현하는 대안도 검토했지만, 이
//! 계획의 DoD 는 TLS/MITM 방어가 아니라 서명 검증·replay guard 검증
//! 이므로 범위를 넘는다고 판단했다.

use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::thread;

const RESULT_OK_MARKER: &str = "RESULT ok=true";

struct Fixture {
    exe: std::path::PathBuf,
    coordinator_seed: [u8; 32],
    agent_seed: [u8; 32],
    coordinator_pub_hex: String,
    agent_pub_hex: String,
    coordinator_device_id: &'static str,
    agent_device_id: &'static str,
    grant_id: &'static str,
    attempt_id: &'static str,
    lease_id: &'static str,
    job_id: &'static str,
}

impl Fixture {
    fn new() -> Result<Self, String> {
        let exe = std::env::current_exe()
            .map_err(|e| format!("현재 실행 파일 경로를 못 얻었다: {e}"))?;

        // ★ 실제 CSPRNG 대신 라벨을 해시해 시드를 결정적으로 만든다 —
        //   이 selftest 는 매번 같은 조건으로 재현 가능해야 하고, 키
        //   프로비저닝 자체를 증명하는 것이 이 단계의 목적이 아니다
        //   (계획서 "Out" 절 — 운영용 key protection 은 범위 밖).
        let coordinator_seed = seed_from_label("coordinator-agent-selftest/coordinator");
        let agent_seed = seed_from_label("coordinator-agent-selftest/agent");

        let coordinator_pub =
            gputeer_crypto::SigningKey::from_bytes(&coordinator_seed).verifying_key();
        let agent_pub = gputeer_crypto::SigningKey::from_bytes(&agent_seed).verifying_key();

        Ok(Self {
            exe,
            coordinator_seed,
            agent_seed,
            coordinator_pub_hex: to_hex(coordinator_pub.as_bytes()),
            agent_pub_hex: to_hex(agent_pub.as_bytes()),
            coordinator_device_id: "01JCOORDSELFTEST0000000001",
            agent_device_id: "01JAGENTSELFTEST00000000001",
            grant_id: "01JGRANTSELFTEST000000001",
            attempt_id: "01JATTEMPTSELFTEST00000001",
            lease_id: "01JLEASESELFTEST000000001",
            job_id: "01JJOBSELFTEST00000000001",
        })
    }

    fn expected_result_line(&self) -> String {
        format!(
            "{RESULT_OK_MARKER} grant_id={} attempt_id={} agent_device_id={}",
            self.grant_id, self.attempt_id, self.agent_device_id
        )
    }
}

/// 한 handshake 시도(정상 경로든 거부 경로 시나리오든)의 전체 결과.
/// 시나리오마다 "무엇이 성공/실패여야 하는가" 판정 기준이 다르므로,
/// 이 구조체는 판정하지 않고 관측한 사실만 담는다.
struct HandshakeOutcome {
    self_pid: u32,
    coordinator_pid: u32,
    agent_pid: u32,
    coordinator_success: bool,
    coordinator_stdout: String,
    coordinator_stderr: String,
    agent_success: bool,
    agent_stdout: String,
    agent_stderr: String,
}

/// coordinator-stub/agent-stub 을 별도 프로세스로 한 번 띄워 끝까지
/// 실행하고 결과를 모은다. `extra_coordinator_args`/`extra_agent_args`
/// 로 단계 5 의 거부 경로 플래그를 얹는다(정상 경로는 빈 슬라이스).
fn run_handshake(
    fixture: &Fixture,
    extra_coordinator_args: &[&str],
    extra_agent_args: &[&str],
) -> Result<HandshakeOutcome, String> {
    let mut coordinator_args: Vec<&str> = vec![
        "coordinator-stub",
        "--listen",
        "127.0.0.1:0",
        "--own-seed",
        // to_hex 결과를 빌려 쓰기 위해 아래에서 String 을 만들어 둔다.
    ];
    let coordinator_own_seed_hex = to_hex(&fixture.coordinator_seed);
    coordinator_args.push(&coordinator_own_seed_hex);
    coordinator_args.push("--peer-pubkey");
    coordinator_args.push(&fixture.agent_pub_hex);
    coordinator_args.push("--coordinator-device-id");
    coordinator_args.push(fixture.coordinator_device_id);
    coordinator_args.push("--agent-device-id");
    coordinator_args.push(fixture.agent_device_id);
    coordinator_args.push("--grant-id");
    coordinator_args.push(fixture.grant_id);
    coordinator_args.push("--attempt-id");
    coordinator_args.push(fixture.attempt_id);
    coordinator_args.push("--lease-id");
    coordinator_args.push(fixture.lease_id);
    coordinator_args.push("--job-id");
    coordinator_args.push(fixture.job_id);
    coordinator_args.extend_from_slice(extra_coordinator_args);

    let mut coordinator = Command::new(&fixture.exe)
        .args(&coordinator_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("coordinator-stub 스폰 실패: {e}"))?;

    // ★ stdout 을 여기서 take() 하면 이후 `wait_with_output()` 은 이
    //   출력을 더 이상 얻지 못한다(핸들이 이미 소비됐다) — 그래서
    //   coordinator 는 `wait()` 만 쓰고, stdout·stderr 는 끝까지 직접
    //   읽는다. 처음엔 이걸 놓쳐서 coordinator 의 RESULT 줄이 조용히
    //   사라지는 결함이 있었다 — 실제로 이 서브커맨드를 실행해서 잡았다.
    let mut coordinator_stdout_reader =
        BufReader::new(coordinator.stdout.take().expect("piped stdout"));
    let coordinator_stderr_reader = coordinator.stderr.take().expect("piped stderr");

    // ★ 코덱스 독립 검수(2026-08-18)가 지적한 결함 — stderr 를 메인
    //   흐름과 동시에 비우지 않으면, coordinator 가 OS 파이프 버퍼를
    //   채울 만큼 stderr 에 쓰다가 블로킹되고, agent 는 coordinator 의
    //   TCP 응답을 기다리느라 블로킹되어 이 selftest 전체가 교착할 수
    //   있다. stdout 은 READY/RESULT 줄을 순서대로 읽어야 하므로 메인
    //   스레드에 남기고, stderr 만 별도 스레드에서 끝까지 비운다.
    let stderr_drain = thread::spawn(move || -> Result<String, std::io::Error> {
        let mut buf = String::new();
        let mut reader = coordinator_stderr_reader;
        reader.read_to_string(&mut buf)?;
        Ok(buf)
    });

    let mut ready_line = String::new();
    coordinator_stdout_reader
        .read_line(&mut ready_line)
        .map_err(|e| format!("coordinator READY 줄 읽기 실패: {e}"))?;
    let address = ready_line
        .trim()
        .strip_prefix("READY ")
        .ok_or_else(|| format!("coordinator 가 READY 대신 이걸 찍었다: {ready_line:?}"))?
        .to_string();

    let mut agent_args: Vec<&str> = vec!["agent-stub", "--connect", &address, "--own-seed"];
    let agent_own_seed_hex = to_hex(&fixture.agent_seed);
    agent_args.push(&agent_own_seed_hex);
    agent_args.push("--peer-pubkey");
    agent_args.push(&fixture.coordinator_pub_hex);
    agent_args.push("--coordinator-device-id");
    agent_args.push(fixture.coordinator_device_id);
    agent_args.push("--agent-device-id");
    agent_args.push(fixture.agent_device_id);
    agent_args.extend_from_slice(extra_agent_args);

    let agent: Child = Command::new(&fixture.exe)
        .args(&agent_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("agent-stub 스폰 실패: {e}"))?;

    // ★ 비공허성의 핵심 — 이 값이 서로 다르지 않으면 "별도 프로세스가
    //   핸드셰이크했다" 는 이 서브커맨드 전체의 전제가 무너진다.
    let self_pid = std::process::id();
    let coordinator_pid = coordinator.id();
    let agent_pid = agent.id();
    if coordinator_pid == agent_pid || coordinator_pid == self_pid || agent_pid == self_pid {
        return Err(format!(
            "PID 가 구분되지 않는다: self={self_pid} coordinator={coordinator_pid} agent={agent_pid}"
        ));
    }

    let agent_output = agent
        .wait_with_output()
        .map_err(|e| format!("agent-stub 대기 실패: {e}"))?;

    let mut coordinator_stdout_rest = String::new();
    coordinator_stdout_reader
        .read_to_string(&mut coordinator_stdout_rest)
        .map_err(|e| format!("coordinator 나머지 stdout 읽기 실패: {e}"))?;
    let coordinator_stdout = format!("{ready_line}{coordinator_stdout_rest}");

    let coordinator_stderr = stderr_drain
        .join()
        .map_err(|_| "coordinator stderr 배수 스레드가 패닉했다".to_string())?
        .map_err(|e| format!("coordinator stderr 읽기 실패: {e}"))?;

    let coordinator_status = coordinator
        .wait()
        .map_err(|e| format!("coordinator-stub 대기 실패: {e}"))?;

    Ok(HandshakeOutcome {
        self_pid,
        coordinator_pid,
        agent_pid,
        coordinator_success: coordinator_status.success(),
        coordinator_stdout,
        coordinator_stderr,
        agent_success: agent_output.status.success(),
        agent_stdout: String::from_utf8_lossy(&agent_output.stdout).into_owned(),
        agent_stderr: String::from_utf8_lossy(&agent_output.stderr).into_owned(),
    })
}

/// 34가지 시나리오를 차례로 돌린다. 하나라도 기대와 다르면 그 자리에서
/// 이유를 담아 반환한다.
pub fn run() -> Result<String, String> {
    let fixture = Fixture::new()?;
    let mut report = String::new();

    // ── 1. 정상 경로 ──────────────────────────────────────────────
    let normal = run_handshake(&fixture, &[], &[])?;
    if !normal.coordinator_success || !normal.agent_success {
        return Err(format!(
            "정상 handshake 가 실패했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            normal.coordinator_success,
            normal.coordinator_stdout,
            normal.coordinator_stderr,
            normal.agent_success,
            normal.agent_stdout,
            normal.agent_stderr
        ));
    }
    let expected = fixture.expected_result_line();
    if !normal.coordinator_stdout.contains(&expected) || !normal.agent_stdout.contains(&expected) {
        return Err(format!(
            "RESULT 줄이 기대한 상관관계와 다르다.\n기대: {expected}\n\
             coordinator stdout: {}\nagent stdout: {}",
            normal.coordinator_stdout, normal.agent_stdout
        ));
    }
    report.push_str(&format!(
        "1) 정상 handshake 성공 (self={} coordinator={} agent={}, grant_id={})\n",
        normal.self_pid, normal.coordinator_pid, normal.agent_pid, fixture.grant_id
    ));

    // ── 2. 위조 coordinator_signature — Agent 가 ACK 를 발급하지 않는다 ──
    let forged_grant = run_handshake(&fixture, &["--corrupt-own-signature", "true"], &[])?;
    if forged_grant.agent_success || forged_grant.agent_stdout.contains(RESULT_OK_MARKER) {
        return Err(format!(
            "위조된 coordinator_signature 가 거부되지 않았다 — Agent 가 ACK 를 발급했다.\n\
             agent exit={} stdout={} stderr={}",
            forged_grant.agent_success, forged_grant.agent_stdout, forged_grant.agent_stderr
        ));
    }
    // ★ Coordinator 도 ACK 를 못 받으므로 성공으로 끝나면 안 된다 —
    //   그러지 않으면 "위조를 보냈는데 스스로는 성공을 주장하는" 앞뒤가
    //   안 맞는 상태가 된다.
    if forged_grant.coordinator_success || forged_grant.coordinator_stdout.contains(RESULT_OK_MARKER) {
        return Err(format!(
            "위조된 Grant 를 보냈는데 coordinator 가 스스로 성공을 주장했다.\n\
             coordinator exit={} stdout={} stderr={}",
            forged_grant.coordinator_success,
            forged_grant.coordinator_stdout,
            forged_grant.coordinator_stderr
        ));
    }
    report.push_str("2) 위조 coordinator_signature 거부 확인 (Agent 가 ACK 미발급)\n");

    // ── 3. 위조 agent_signature — Coordinator 가 성공 처리하지 않는다 ──
    // ★ Agent 는 자기가 만든 서명이 위조됐는지 스스로 검증하지 않는다
    //   (검증은 언제나 수신자 책임이다) — 그래서 Agent 자신은 정상
    //   종료할 수 있다. 판정은 Coordinator 쪽 실패 여부로만 한다.
    let forged_ack = run_handshake(&fixture, &[], &["--corrupt-own-signature", "true"])?;
    if forged_ack.coordinator_success || forged_ack.coordinator_stdout.contains(RESULT_OK_MARKER) {
        return Err(format!(
            "위조된 agent_signature 가 거부되지 않았다 — Coordinator 가 성공을 주장했다.\n\
             coordinator exit={} stdout={} stderr={}",
            forged_ack.coordinator_success, forged_ack.coordinator_stdout, forged_ack.coordinator_stderr
        ));
    }
    report.push_str("3) 위조 agent_signature 거부 확인 (Coordinator 가 성공 처리 안 함)\n");

    // ── 4. 동일 Grant wire bytes replay ──────────────────────────
    let replay = run_handshake(
        &fixture,
        &["--send-grant-twice", "true"],
        &["--expect-replay", "true"],
    )?;
    if replay.agent_success || !replay.agent_stderr.contains("REPLAY_REJECTED:") {
        return Err(format!(
            "동일 Grant 를 두 번 보냈는데 두 번째가 거부됐다는 증거(REPLAY_REJECTED:)가 없다.\n\
             agent exit={} stdout={} stderr={}",
            replay.agent_success, replay.agent_stdout, replay.agent_stderr
        ));
    }
    if replay.coordinator_success || replay.coordinator_stdout.contains(RESULT_OK_MARKER) {
        return Err(format!(
            "replay 시나리오인데 coordinator 가 성공을 주장했다.\n\
             coordinator exit={} stdout={} stderr={}",
            replay.coordinator_success, replay.coordinator_stdout, replay.coordinator_stderr
        ));
    }
    report.push_str("4) 동일 Grant wire bytes replay 거부 확인 (DurableReplayGuard 계약과 동일하게 InMemoryReplayGuard 도 Duplicate 를 거부)\n");

    // ── 5. 위조 nested Lease 서명 — outer Grant 는 정상인데 Agent 가
    //    Lease 를 독립적으로 검증해야만 잡힌다 ──────────────────────
    // ★ Coordinator 는 이 시나리오에서 outer Grant 를 정상 서명하므로
    //   자기 자신은 "성공"을 주장할 수 있다(outer 검증만 보면 다
    //   맞다) — 판정은 Agent 가 LEASE_REJECTED 로 거부했는지로만 한다.
    let forged_lease = run_handshake(&fixture, &["--corrupt-lease-signature", "true"], &[])?;
    if forged_lease.agent_success || !forged_lease.agent_stderr.contains("LEASE_REJECTED:") {
        return Err(format!(
            "위조된 nested Lease 서명이 거부되지 않았다 — outer Grant 검증만으로는 \
             이 결함을 잡지 못한다는 뜻이다.\n\
             agent exit={} stdout={} stderr={}",
            forged_lease.agent_success, forged_lease.agent_stdout, forged_lease.agent_stderr
        ));
    }
    report.push_str(
        "5) 위조 nested Lease 서명 거부 확인 (Agent 가 Lease 를 outer Grant 와 \
         독립적으로 검증한다)\n",
    );

    // ── 6. 만료된 Lease ────────────────────────────────────────────
    let expired_lease = run_handshake(&fixture, &["--expire-lease", "true"], &[])?;
    if expired_lease.agent_success || !expired_lease.agent_stderr.contains("LEASE_REJECTED:") {
        return Err(format!(
            "만료된 Lease 가 거부되지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            expired_lease.agent_success, expired_lease.agent_stdout, expired_lease.agent_stderr
        ));
    }
    report.push_str("6) 만료된 Lease 거부 확인 (Lease::LIFETIME == LongLived 의 만료 검사)\n");

    // ══════════════════════════════════════════════════════════════
    // Lease 갱신 (2026-08-19, `docs/plans/2026-08-19_0500_...`)
    // ══════════════════════════════════════════════════════════════

    // ── 7. 정상 Lease 갱신 (같은 epoch 유지) ─────────────────────────
    let renew_ok = run_handshake(
        &fixture,
        &["--do-renew", "true", "--renewed-fence-epoch", "0"],
        &["--do-renew", "true"],
    )?;
    if !renew_ok.coordinator_success || !renew_ok.agent_success {
        return Err(format!(
            "정상 Lease 갱신이 실패했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            renew_ok.coordinator_success,
            renew_ok.coordinator_stdout,
            renew_ok.coordinator_stderr,
            renew_ok.agent_success,
            renew_ok.agent_stdout,
            renew_ok.agent_stderr
        ));
    }
    if !renew_ok.agent_stdout.contains("RENEW_RESULT ok=true outcome=RENEWED") {
        return Err(format!(
            "정상 Lease 갱신인데 Agent 가 RENEW_RESULT 를 찍지 않았다.\nagent stdout: {}",
            renew_ok.agent_stdout
        ));
    }
    report.push_str(
        "7) 정상 Lease 갱신 성공 (같은 epoch 유지, Agent 가 새 Lease 를 독립 검증)\n",
    );

    // ── 8. 위조 RenewLeaseRequest.node_signature — Coordinator ingress 검증 실패 ──
    let forged_renew_req = run_handshake(
        &fixture,
        &["--do-renew", "true", "--renewed-fence-epoch", "0"],
        &[
            "--do-renew",
            "true",
            "--corrupt-renew-request-signature",
            "true",
        ],
    )?;
    if forged_renew_req.coordinator_success
        || forged_renew_req.coordinator_stdout.contains(RESULT_OK_MARKER)
        // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — 실패 여부만 보지
        //   말고 **왜** 실패했는지 특정한다. 그러지 않으면 이 시나리오가
        //   무관한 이유(예: 다른 버그로 인한 크래시)로도 우연히 통과할
        //   수 있다.
        || !forged_renew_req
            .coordinator_stderr
            .contains("RenewLeaseRequest 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "위조된 RenewLeaseRequest.node_signature 가 거부되지 않았다 — coordinator 가 성공을 주장했거나 \
             기대한 이유(RenewLeaseRequest 프레임 읽기/검증 실패)로 거부하지 않았다.\n\
             coordinator exit={} stdout={} stderr={}",
            forged_renew_req.coordinator_success,
            forged_renew_req.coordinator_stdout,
            forged_renew_req.coordinator_stderr
        ));
    }
    report.push_str(
        "8) 위조 RenewLeaseRequest.node_signature 거부 확인 (Coordinator ingress 검증 실패)\n",
    );

    // ── 9. 위조 RenewLeaseResult.coordinator_signature — Agent 결과 서명 검증 실패 ──
    let forged_renew_result = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--corrupt-renew-result-signature",
            "true",
        ],
        &["--do-renew", "true"],
    )?;
    if forged_renew_result.agent_success
        || !forged_renew_result
            .agent_stderr
            .contains("RenewLeaseResult 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "위조된 RenewLeaseResult.coordinator_signature 가 거부되지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            forged_renew_result.agent_success,
            forged_renew_result.agent_stdout,
            forged_renew_result.agent_stderr
        ));
    }
    report.push_str(
        "9) 위조 RenewLeaseResult.coordinator_signature 거부 확인 (Agent 결과 서명 검증 실패)\n",
    );

    // ── 10. 위조 nested 새 Lease 서명 — outer 결과 서명은 정상, Agent 독립 검증으로 거부 ──
    let forged_renewed_lease = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--corrupt-renewed-lease-signature",
            "true",
        ],
        &["--do-renew", "true"],
    )?;
    if forged_renewed_lease.agent_success
        || !forged_renewed_lease
            .agent_stderr
            .contains("RENEW_REJECTED: 갱신된 Lease 서명 검증 실패")
    {
        return Err(format!(
            "위조된 nested Lease 서명(갱신)이 거부되지 않았다 — outer 결과 검증만으로는 \
             이 결함을 잡지 못한다는 뜻이다.\n\
             agent exit={} stdout={} stderr={}",
            forged_renewed_lease.agent_success,
            forged_renewed_lease.agent_stdout,
            forged_renewed_lease.agent_stderr
        ));
    }
    report.push_str(
        "10) 위조 nested 새 Lease 서명 거부 확인 (Agent 가 갱신된 Lease 를 outer 결과와 \
         독립적으로 검증한다)\n",
    );

    // ── 11. epoch 강등 — Agent 의 FenceWatermark 가 거부 ────────────
    let downgrade = run_handshake(
        &fixture,
        &[
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "3",
        ],
        &["--do-renew", "true"],
    )?;
    if downgrade.agent_success
        || !downgrade
            .agent_stderr
            .contains("RENEW_REJECTED: fence_epoch 검사 실패")
    {
        return Err(format!(
            "낮은 fence_epoch 로 갱신했는데 Agent watermark 가 거부하지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            downgrade.agent_success, downgrade.agent_stdout, downgrade.agent_stderr
        ));
    }
    report.push_str(
        "11) epoch 강등 거부 확인 (FenceWatermark.check_and_advance 가 낮은 epoch 를 거부)\n",
    );

    // ── 12. RENEW_OUTCOME_SUPERSEDED — 서명된 정상 정책 거부로 분류 ──
    let superseded = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--renew-outcome-override",
            "2",
        ],
        &["--do-renew", "true"],
    )?;
    if superseded.agent_success || !superseded.agent_stderr.contains("RENEW_REFUSED:SUPERSEDED") {
        return Err(format!(
            "RENEW_OUTCOME_SUPERSEDED 가 서명된 정상 정책 거부로 분류되지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            superseded.agent_success, superseded.agent_stdout, superseded.agent_stderr
        ));
    }
    report.push_str(
        "12) RENEW_OUTCOME_SUPERSEDED 서명된 정상 정책 거부로 분류 확인 (Lease·watermark 불변)\n",
    );

    // ── 13. RENEW_OUTCOME_QUARANTINED — 서명된 정상 정책 거부로 분류 ──
    let quarantined = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--renew-outcome-override",
            "3",
        ],
        &["--do-renew", "true"],
    )?;
    if quarantined.agent_success
        || !quarantined.agent_stderr.contains("RENEW_REFUSED:QUARANTINED")
    {
        return Err(format!(
            "RENEW_OUTCOME_QUARANTINED 가 서명된 정상 정책 거부로 분류되지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            quarantined.agent_success, quarantined.agent_stdout, quarantined.agent_stderr
        ));
    }
    report.push_str(
        "13) RENEW_OUTCOME_QUARANTINED 서명된 정상 정책 거부로 분류 확인 (Lease·watermark 불변)\n",
    );

    // ── 14. RenewLeaseResult.request_nonce 를 echo 하지 않음 — Agent 거부 ──
    //
    // ★ 계획서 "6종" 표에는 없지만, "In" 절이 request_nonce 의 목적으로
    //   든 것과 정확히 같은 공격이다 — 서명은 정상인 결과를 **다른**
    //   갱신 요청에 재사용하는 시나리오. 구현 중 발견한 별도 게이트라
    //   여기 추가한다.
    let wrong_nonce = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--corrupt-renew-result-nonce",
            "true",
        ],
        &["--do-renew", "true"],
    )?;
    if wrong_nonce.agent_success
        || !wrong_nonce
            .agent_stderr
            .contains("RENEW_REJECTED: request_nonce 가 우리가 보낸 요청과 다르다")
    {
        return Err(format!(
            "RenewLeaseResult.request_nonce 가 echo 되지 않았는데 Agent 가 거부하지 않았다.\n\
             agent exit={} stdout={} stderr={}",
            wrong_nonce.agent_success, wrong_nonce.agent_stdout, wrong_nonce.agent_stderr
        ));
    }
    report.push_str(
        "14) RenewLeaseResult.request_nonce 불일치 거부 확인 (응답이 다른 요청에 재사용되는 것을 방지)\n",
    );

    // ── 15. epoch 상승 — 계획서가 명시적으로 범위 밖(정책상 거부)이라고 정한 경우 ──
    //
    // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — `FenceWatermark` 는
    //   `<` 만 거부하고 `>` 는 정상적인 전진으로 통과시킨다. 계획서는
    //   "높으면 이 조각에서는 정책상 거부"라고 명시했으므로, Agent 가
    //   watermark 와 별개로 명시적으로 막아야 한다.
    let upgrade = run_handshake(
        &fixture,
        &["--do-renew", "true", "--renewed-fence-epoch", "5"],
        &["--do-renew", "true"],
    )?;
    if upgrade.agent_success
        || !upgrade
            .agent_stderr
            .contains("epoch 상승은 이 조각의 범위 밖이라 정책상 거부한다")
    {
        return Err(format!(
            "epoch 상승(0 -> 5) 갱신을 Agent 가 거부하지 않았다 — 계획서는 이를 범위 밖 정책 거부로 \
             명시했다.\n\
             agent exit={} stdout={} stderr={}",
            upgrade.agent_success, upgrade.agent_stdout, upgrade.agent_stderr
        ));
    }
    report.push_str(
        "15) epoch 상승 거부 확인 (계획서 범위 — FenceWatermark 만으로는 안 잡히고 명시적 검사가 필요하다)\n",
    );

    // ── 16. Coordinator 가 요청의 fence_epoch 을 자신이 기억하는 값과 대조 ──
    //
    // ★ 코덱스 독립 검수(2026-08-19, p99) 지적 — 이전에는
    //   `renew_req.fence_epoch` 를 아무것도와 비교하지 않았다.
    let epoch_mismatch = run_handshake(
        &fixture,
        &["--do-renew", "true", "--renewed-fence-epoch", "0"],
        &[
            "--do-renew",
            "true",
            "--renew-request-epoch-override",
            "99",
        ],
    )?;
    if epoch_mismatch.coordinator_success
        || epoch_mismatch.coordinator_stdout.contains(RESULT_OK_MARKER)
        || !epoch_mismatch
            .coordinator_stderr
            .contains("RenewLeaseRequest.fence_epoch 불일치")
    {
        return Err(format!(
            "요청의 fence_epoch(99) 이 Coordinator 가 기억하는 값(0)과 다른데 거부되지 않았다.\n\
             coordinator exit={} stdout={} stderr={}",
            epoch_mismatch.coordinator_success,
            epoch_mismatch.coordinator_stdout,
            epoch_mismatch.coordinator_stderr
        ));
    }
    report.push_str(
        "16) RenewLeaseRequest.fence_epoch 불일치 거부 확인 (Coordinator 가 요청 epoch 을 자신이 \
         기억하는 값과 대조한다)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // durable FenceWatermark (2026-08-19, `docs/plans/2026-08-19_2200_...`)
    //
    // 지금까지의 시나리오는 매번 새 `--fence-db` 임시 파일(기본값)을
    // 쓰므로 매 프로세스가 빈 watermark 에서 시작한다 — "재시작을
    // 넘는 방어"를 증명하지 않는다. 아래는 **같은 SQLite 파일**을
    // 서로 다른 `agent-stub` **프로세스**(별도 PID)가 순서대로 열어,
    // 앞선 프로세스가 기록한 watermark 가 실제로 남아 있는지 확인한다.
    //
    // ★ 설계 당시엔 "최초 Grant 경로"·"갱신 경로" 두 개를 따로
    //   시험하려 했다(`docs/plans/2026-08-19_2200_...v1.md` §재시작
    //   selftest 설계). **뮤테이션 테스트로 실제로 만들어보니 그 둘은
    //   분리되지 않는다** — Agent 의 제어 흐름상 갱신은 항상 같은
    //   프로세스의 최초 Grant 검증 **뒤에** 오고, 최초 Grant 검증
    //   자체가 이미 그 프로세스의 watermark 로컬 뷰를 durable 값으로
    //   채운다. 그래서 "최초 Grant 는 낮은 epoch 로 통과시키고 그
    //   프로세스의 갱신만 거부하는" 조합을 만들면, 그 거부는 진짜
    //   프로세스 경계를 넘는 영속성이 아니라 **그 프로세스 자신의
    //   최초 Grant 호출이 방금 쓴 값**만으로도 똑같이 재현된다 —
    //   `DurableFenceWatermark::open()` 이 주어진 경로를 무시하고 매번
    //   새 파일을 열도록 무력화해봤더니, "최초 Grant 경로" 거부는
    //   실패했지만 "갱신 경로" 거부는 **속아서 계속 통과했다**(그
    //   프로세스 자신의 최초 Grant 가 이미 watermark=5 를 로컬에
    //   써 놓았기 때문). 즉 진짜 재시작 방어의 유일한 관측 지점은
    //   **최초 Grant 검증 호출부 하나뿐**이다 — 그것이 durable 하면
    //   그 위에 올라타는 갱신도 자동으로 안전하고, 그것이 durable 하지
    //   않으면 갱신 검증이 아무리 정확해도 프로세스 경계를 넘는 방어는
    //   전혀 없다. 그래서 시나리오를 하나로 정리했다 — 존재하지 않는
    //   구분을 존재하는 것처럼 보고하지 않는다.

    let fence_dir = tempfile::tempdir()
        .map_err(|e| format!("fence watermark 임시 디렉터리 생성 실패: {e}"))?;
    let fence_db_path = fence_dir.path().join("fence.sqlite3");
    let fence_db = fence_db_path
        .to_str()
        .ok_or_else(|| "fence watermark 경로가 UTF-8 이 아니다".to_string())?;

    // ── 17. durable FenceWatermark 재시작 방어 ───────────────────────
    //
    // 1차(별도 프로세스): epoch 5 로 정상 발급 — durable watermark 가
    //   5 로 기록된 채 프로세스가 종료된다.
    // 2차(★ 별도 프로세스, 같은 DB): 최초 Grant 자체가 epoch=3(durable
    //   watermark 보다 낮다)을 담아 발급된다 — `verify_and_record_lease()`
    //   의 최초 검증 호출부가 거부해야 한다. **1차 프로세스의 메모리가
    //   2차 프로세스에 전달되지 않았다는 것**은 PID 가 다르고 각 실행이
    //   독립적으로 종료된다는 사실이 보장한다 — DB 파일만 상태를
    //   전달한다.
    let restart_first = run_handshake(
        &fixture,
        &["--fence-epoch", "5", "--do-renew", "false"],
        &["--do-renew", "false", "--fence-db", fence_db],
    )?;
    if !restart_first.coordinator_success || !restart_first.agent_success {
        return Err(format!(
            "재시작 방어 시나리오 1차 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            restart_first.coordinator_success,
            restart_first.coordinator_stdout,
            restart_first.coordinator_stderr,
            restart_first.agent_success,
            restart_first.agent_stdout,
            restart_first.agent_stderr
        ));
    }

    let restart_second = run_handshake(
        &fixture,
        &["--fence-epoch", "3", "--do-renew", "false"],
        &["--do-renew", "false", "--fence-db", fence_db],
    )?;
    if restart_second.agent_pid == restart_first.agent_pid {
        return Err(format!(
            "재시작 방어 시나리오의 2차 agent PID 가 1차와 같다 — 별도 프로세스가 \
             아니다.\n1차 agent_pid={} 2차 agent_pid={}",
            restart_first.agent_pid, restart_second.agent_pid
        ));
    }
    // ★ 코덱스 독립 검수(2026-08-19, p102) 지적 — 접두사
    //   "LEASE_REJECTED: fence_epoch 검사 실패" 만으로는 진짜 정책
    //   거부(`Stale`)와 저장소 장애(`Io`/`LockTimeout`)를 구분하지
    //   못한다(둘 다 그 접두사를 낼 수 있었다 — 지금은 갈라졌지만,
    //   회귀를 대비해 여기서 구체적인 값까지 확인한다). "(정책 거부)"
    //   표시와 `StaleEpoch` 의 실제 수치(incoming=3, watermark=5)까지
    //   본다 — 저장소 장애였다면 이 문구가 나올 수 없다.
    if restart_second.agent_success
        || !restart_second
            .agent_stderr
            .contains("LEASE_REJECTED: fence_epoch 검사 실패(정책 거부)")
        || !restart_second
            .agent_stderr
            .contains("fence_epoch 3 은 기록된 watermark 5 보다 낮다")
    {
        return Err(format!(
            "durable watermark 가 재시작(별도 프로세스)을 넘어 낮은 epoch 의 최초 Grant 를 \
             거부하지 못했다(또는 거부 이유가 정책 거부가 아니다 — 저장소 장애와 혼동됐을 \
             수 있다).\n\
             1차 agent_pid={} 2차 agent_pid={}\n\
             2차 agent exit={} stdout={} stderr={}",
            restart_first.agent_pid,
            restart_second.agent_pid,
            restart_second.agent_success,
            restart_second.agent_stdout,
            restart_second.agent_stderr
        ));
    }
    if restart_second.coordinator_success
        || restart_second.coordinator_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "Agent 가 최초 Grant 를 거부해 ACK 를 보내지 않았는데 coordinator 가 성공을 \
             주장했다.\n\
             coordinator exit={} stdout={} stderr={}",
            restart_second.coordinator_success,
            restart_second.coordinator_stdout,
            restart_second.coordinator_stderr
        ));
    }
    report.push_str(
        "17) durable FenceWatermark 재시작 방어 확인 — 별도 프로세스가 SQLite 파일로 이전 \
         watermark 를 물려받아 낮은 epoch 의 최초 Grant 를 거부한다(뮤테이션 테스트로 확인한 \
         대로, 이 하나의 관측 지점이 그 위에 올라타는 갱신 경로까지 보호한다)\n",
    );

    // ── 18. `--fence-db :memory:` 는 fail closed — 코덱스 독립 검수(2026-08-19, p102) 지적 ──
    //
    // ★ 전에는 `is_durable() == false` 인 채로도 계속 진행했다 — 이
    //   조각 전체의 목적(재시작을 넘는 방어)이 겉으로는 durable
    //   타입을 쓰는 것처럼 보이면서 조용히 거짓이 될 수 있었다.
    //   Coordinator 조차 필요 없다 — Agent 의 fail-closed 검사가
    //   `TcpStream::connect()` 보다 먼저 실행되므로, 연결 대상 주소가
    //   실재하지 않아도 절대 도달하지 않는다.
    let (memory_ok, _memory_stdout, memory_stderr) = run_agent_alone(&fixture, &["--fence-db", ":memory:"])?;
    if memory_ok || !memory_stderr.contains("fence watermark 저장소가 영속이 아니다") {
        return Err(format!(
            "--fence-db :memory: 가 fail closed 되지 않았다.\nexit ok={memory_ok} stderr={memory_stderr}"
        ));
    }
    report.push_str(
        "18) --fence-db :memory: fail closed 확인 (비영속 경로로 재시작 방어를 흉내내지 못한다)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // 반복 Lease 갱신 (2026-08-19, `docs/plans/2026-08-19_2330_...`)
    //
    // 설계(`p105`)가 코드 경로로 확정한 핵심 위험 — 갱신 요청의
    // nonce 가 lease_id 에서만 결정적으로 유도되면, 회차마다 같은
    // nonce 가 나와 두 번째 요청부터 replay guard 가 Duplicate 로
    // 거부한다. round 를 nonce 입력에 섞지 않았다면 이 시나리오는
    // round 1(0-based)에서 즉시 실패한다 — **성공 자체가 그 결함이
    // 고쳐졌다는 증거**다.
    // ══════════════════════════════════════════════════════════════

    // ── 19. 같은 연결에서 3회 정상 갱신 ──────────────────────────────
    let repeated = run_handshake(
        &fixture,
        &[
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
            "--renew-rounds",
            "3",
        ],
        &["--do-renew", "true", "--renew-rounds", "3"],
    )?;
    if !repeated.coordinator_success || !repeated.agent_success {
        return Err(format!(
            "같은 연결에서 3회 반복 갱신이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            repeated.coordinator_success,
            repeated.coordinator_stdout,
            repeated.coordinator_stderr,
            repeated.agent_success,
            repeated.agent_stdout,
            repeated.agent_stderr
        ));
    }
    let agent_renewed_count = repeated
        .agent_stdout
        .matches("RENEW_RESULT ok=true outcome=RENEWED")
        .count();
    let coordinator_renewed_count = repeated
        .coordinator_stdout
        .matches("RENEW_RESULT ok=true outcome=")
        .count();
    // ★ "RESULT ok=true" 는 "RENEW_RESULT ok=true" 의 부분 문자열이다
    //   — 줄 단위로 정확히 그 접두사로 시작하는 줄만 센다.
    let agent_result_count = repeated
        .agent_stdout
        .lines()
        .filter(|line| line.starts_with(RESULT_OK_MARKER))
        .count();
    if agent_renewed_count != 3 || coordinator_renewed_count != 3 || agent_result_count != 1 {
        return Err(format!(
            "3회 반복 갱신의 출력 횟수가 기대와 다르다 — agent RENEWED={agent_renewed_count}(기대 3), \
             coordinator RENEW_RESULT={coordinator_renewed_count}(기대 3), agent 최종 RESULT={agent_result_count}(기대 1).\n\
             agent stdout={}\ncoordinator stdout={}",
            repeated.agent_stdout, repeated.coordinator_stdout
        ));
    }
    report.push_str(
        "19) 같은 연결에서 3회 정상 갱신 확인 (회차별 nonce 분리로 replay guard 의 \
         Duplicate 거부를 피한다 — 성공 자체가 증거다)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // Coordinator 영속 Lease 저장소 (2026-08-19, `docs/plans/2026-08-19_2300_...`)
    //
    // 지금까지의 시나리오는 `--lease-db` 를 안 주므로 Coordinator 가
    // 매번 CLI 인자만으로 상태를 구성하는 레거시 경로를 쓴다. 아래는
    // **같은 SQLite 파일**을 서로 다른 `coordinator-stub` 프로세스가
    // 순서대로 열어, 저장된 Lease 신원·epoch 가 CLI 인자보다
    // 우선하는지 확인한다. Agent 쪽 durable FenceWatermark 설계 때
    // 겪은 함정(갱신 전용 시나리오가 실제로는 최초 검증에 의해 이미
    // 결정돼 판별력이 없었던 문제)을 참고해, 두 시나리오 모두
    // **Coordinator 가 스토어를 무시했다면 관측 가능한 방식으로
    // 실패해야 한다**는 조건을 명시적으로 설계했다.
    // ══════════════════════════════════════════════════════════════

    // ── 20. 발급 상태 복원 ────────────────────────────────────────────
    //
    // 1차(별도 프로세스): epoch 5 로 정상 발급 + 같은 epoch 갱신 —
    //   lease store 에 epoch=5 가 저장되고, Agent 의 durable
    //   watermark 에도 5 가 남는다(같은 --fence-db 재사용).
    // 2차(★ 별도 프로세스, 같은 두 DB): Coordinator 를 **의도적으로
    //   틀린 epoch=3** 으로 실행한다. Coordinator 가 스토어를 쓰면
    //   `get_or_issue()` 가 저장된 값(5)을 그대로 반환해 Grant 도
    //   epoch=5 로 나간다 — Agent 의 watermark(5)와 일치해 성공한다.
    //   반대로 스토어를 무시하고 CLI 값(3)을 그대로 썼다면 Agent 의
    //   watermark(5)가 3 을 거부해 **관측 가능하게** 실패한다.
    let lease_dir_20 = tempfile::tempdir()
        .map_err(|e| format!("lease store 임시 디렉터리 생성 실패(20): {e}"))?;
    let lease_db_path_20 = lease_dir_20.path().join("leases.sqlite3");
    let lease_db_20 = lease_db_path_20
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;
    let fence_dir_20 = tempfile::tempdir()
        .map_err(|e| format!("fence watermark 임시 디렉터리 생성 실패(20): {e}"))?;
    let fence_db_path_20 = fence_dir_20.path().join("fence.sqlite3");
    let fence_db_20 = fence_db_path_20
        .to_str()
        .ok_or_else(|| "fence watermark 경로가 UTF-8 이 아니다".to_string())?;

    let issue_first = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_20,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
        ],
        &["--fence-db", fence_db_20, "--do-renew", "true"],
    )?;
    if !issue_first.coordinator_success || !issue_first.agent_success {
        return Err(format!(
            "발급 상태 복원 시나리오 1차 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_first.coordinator_success,
            issue_first.coordinator_stdout,
            issue_first.coordinator_stderr,
            issue_first.agent_success,
            issue_first.agent_stdout,
            issue_first.agent_stderr
        ));
    }

    let issue_second = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_20,
            "--fence-epoch",
            "3",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_20, "--do-renew", "false"],
    )?;
    if issue_second.coordinator_pid == issue_first.coordinator_pid {
        return Err(format!(
            "발급 상태 복원 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 프로세스가 \
             아니다.\n1차 coordinator_pid={} 2차 coordinator_pid={}",
            issue_first.coordinator_pid, issue_second.coordinator_pid
        ));
    }
    if !issue_second.coordinator_success || !issue_second.agent_success {
        return Err(format!(
            "lease store 가 재시작(별도 프로세스)을 넘어 저장된 epoch(5)를 CLI 의 틀린 \
             값(3)보다 우선시키지 못했다 — Coordinator 가 스토어를 무시하고 CLI 값을 그대로 \
             썼다면 Agent 의 durable watermark(5) 가 epoch=3 을 거부해 이렇게 실패한다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_first.coordinator_pid,
            issue_second.coordinator_pid,
            issue_second.coordinator_success,
            issue_second.coordinator_stdout,
            issue_second.coordinator_stderr,
            issue_second.agent_success,
            issue_second.agent_stdout,
            issue_second.agent_stderr
        ));
    }
    report.push_str(
        "20) Coordinator 영속 Lease 저장소 — 발급 상태 복원 확인 (별도 프로세스가 SQLite \
         파일로 저장된 epoch 를 CLI 의 틀린 값보다 우선시킨다)\n",
    );

    // ── 21. 재시작 후 갱신 대조 ────────────────────────────────────────
    //
    // 1차(별도 프로세스): epoch 5 로 발급만 한다(갱신 없음) — lease
    //   store 에 epoch=5, Agent watermark 에도 5 가 남는다.
    // 2차(★ 별도 프로세스, 같은 두 DB): Coordinator 를 **의도적으로
    //   틀린 epoch=6** 으로 실행하고 갱신까지 수행한다.
    //   `get_or_issue()` 가 최초 Grant 를 저장된 epoch=5 로 자동
    //   교정하므로, Agent 는 held_lease.fence_epoch=5 로 갱신 요청을
    //   만든다. 이 요청을 Coordinator 가 **저장소의 epoch(5)와 대조**
    //   하면 일치해 갱신이 성공한다 — 만약 여전히 `config.fence_epoch`
    //   (틀린 값 6)과 비교했다면 5 != 6 으로 **정당한 갱신을 잘못
    //   거부**했을 것이다. 이 시나리오는 그 회귀를 정확히 잡는다.
    let lease_dir_21 = tempfile::tempdir()
        .map_err(|e| format!("lease store 임시 디렉터리 생성 실패(21): {e}"))?;
    let lease_db_path_21 = lease_dir_21.path().join("leases.sqlite3");
    let lease_db_21 = lease_db_path_21
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;
    let fence_dir_21 = tempfile::tempdir()
        .map_err(|e| format!("fence watermark 임시 디렉터리 생성 실패(21): {e}"))?;
    let fence_db_path_21 = fence_dir_21.path().join("fence.sqlite3");
    let fence_db_21 = fence_db_path_21
        .to_str()
        .ok_or_else(|| "fence watermark 경로가 UTF-8 이 아니다".to_string())?;

    let renew_first = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_21,
            "--fence-epoch",
            "5",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_21, "--do-renew", "false"],
    )?;
    if !renew_first.coordinator_success || !renew_first.agent_success {
        return Err(format!(
            "재시작 후 갱신 대조 시나리오 1차 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            renew_first.coordinator_success,
            renew_first.coordinator_stdout,
            renew_first.coordinator_stderr,
            renew_first.agent_success,
            renew_first.agent_stdout,
            renew_first.agent_stderr
        ));
    }

    let renew_second = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_21,
            "--fence-epoch",
            "6",
            "--do-renew",
            "true",
        ],
        &["--fence-db", fence_db_21, "--do-renew", "true"],
    )?;
    if renew_second.coordinator_pid == renew_first.coordinator_pid {
        return Err(format!(
            "재시작 후 갱신 대조 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 \
             프로세스가 아니다.\n1차 coordinator_pid={} 2차 coordinator_pid={}",
            renew_first.coordinator_pid, renew_second.coordinator_pid
        ));
    }
    if !renew_second.coordinator_success || !renew_second.agent_success {
        return Err(format!(
            "lease store 가 재시작을 넘어 갱신 요청의 fence_epoch 대조에 저장된 값을 \
             쓰지 못했다 — 여전히 그 실행의 CLI 값(6)과 비교했다면 저장된 값(5)과 달라 \
             정당한 갱신을 잘못 거부했을 것이다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            renew_first.coordinator_pid,
            renew_second.coordinator_pid,
            renew_second.coordinator_success,
            renew_second.coordinator_stdout,
            renew_second.coordinator_stderr,
            renew_second.agent_success,
            renew_second.agent_stdout,
            renew_second.agent_stderr
        ));
    }
    report.push_str(
        "21) Coordinator 영속 Lease 저장소 — 재시작 후 갱신 대조 확인 (별도 프로세스가 \
         갱신 요청의 fence_epoch 을 저장된 값과 대조한다, 그 실행의 CLI 값이 아니라)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // max_total_duration_seconds 갱신 차단 (2026-08-19, `docs/plans/2026-08-19_2350_...`)
    //
    // 실제로 시간을 흘려보내야 판별력이 생긴다 — 짧은 한도
    // (`--max-total-duration-seconds 2`)를 CLI 로 주고, 이 selftest
    // 프로세스가 실제로 2.2초 넘게 sleep 한 뒤 **별도** Coordinator/
    // Agent 프로세스로 갱신을 시도한다. `--renew-rounds` 만 늘리는
    // 것으로는 판별할 수 없다 — 현재 반복 요청 사이에 sleep 이 없어
    // 모든 라운드가 한도 안에서 끝난다(설계 p111).
    // ══════════════════════════════════════════════════════════════

    // ── 22. 누적 시간 초과 — 서명된 MAX_DURATION_EXCEEDED 로 갱신 거부 ──
    let lease_dir_22 = tempfile::tempdir()
        .map_err(|e| format!("lease store 임시 디렉터리 생성 실패(22): {e}"))?;
    let lease_db_path_22 = lease_dir_22.path().join("leases.sqlite3");
    let lease_db_22 = lease_db_path_22
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;

    let issue_22 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_22,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "2",
            "--do-renew",
            "false",
        ],
        &["--do-renew", "false"],
    )?;
    if !issue_22.coordinator_success || !issue_22.agent_success {
        return Err(format!(
            "누적 시간 초과 시나리오 1차(최초 발급) 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_22.coordinator_success,
            issue_22.coordinator_stdout,
            issue_22.coordinator_stderr,
            issue_22.agent_success,
            issue_22.agent_stdout,
            issue_22.agent_stderr
        ));
    }

    // 저장소에 실제로 기록된 만료시각을 미리 읽어둔다 — 초과 판정
    // 뒤에도 이 값이 그대로인지 나중에 직접 대조한다.
    let expires_before_22 = {
        let store = gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_22)
            .map_err(|e| format!("lease store 재조회 열기 실패(22, 사전): {e}"))?;
        store
            .get(fixture.lease_id)
            .map_err(|e| format!("lease store 재조회 실패(22, 사전): {e}"))?
            .ok_or_else(|| "lease store 에 22번 시나리오 lease 가 없다(사전)".to_string())?
            .expires_at_unix_ms
    };

    // 실제로 한도(2초)를 넘긴다 — 이 sleep 이 짧은 한도를 CLI 로 준
    // 것을 실측 가능하게 만든다(설계 p111 "두 프로세스 + sleep" 권장안).
    std::thread::sleep(std::time::Duration::from_millis(2_200));

    let renew_22 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_22,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "2",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
        ],
        &["--do-renew", "true"],
    )?;
    if renew_22.coordinator_pid == issue_22.coordinator_pid {
        return Err(format!(
            "누적 시간 초과 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 프로세스가 \
             아니다.\n1차 coordinator_pid={} 2차 coordinator_pid={}",
            issue_22.coordinator_pid, renew_22.coordinator_pid
        ));
    }
    if !renew_22.coordinator_success
        || renew_22.agent_success
        || !renew_22
            .agent_stderr
            .contains("RENEW_REFUSED:MAX_DURATION_EXCEEDED")
    {
        return Err(format!(
            "누적 시간(2초)을 넘겼는데 서명된 MAX_DURATION_EXCEEDED 로 거부되지 않았다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_22.coordinator_pid,
            renew_22.coordinator_pid,
            renew_22.coordinator_success,
            renew_22.coordinator_stdout,
            renew_22.coordinator_stderr,
            renew_22.agent_success,
            renew_22.agent_stdout,
            renew_22.agent_stderr
        ));
    }

    // 저장소의 만료시각이 연장되지 않았는지 직접 재조회로 확인한다 —
    // 초과 판정에서 UPDATE 를 아예 실행하지 않아야 다음 판정 시각도
    // 밀리지 않는다(연장해버리면 정책이 스스로 무력화된다).
    let expires_after_22 = {
        let store = gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_22)
            .map_err(|e| format!("lease store 재조회 열기 실패(22, 사후): {e}"))?;
        store
            .get(fixture.lease_id)
            .map_err(|e| format!("lease store 재조회 실패(22, 사후): {e}"))?
            .ok_or_else(|| "lease store 에 22번 시나리오 lease 가 없다(사후)".to_string())?
            .expires_at_unix_ms
    };
    if expires_after_22 != expires_before_22 {
        return Err(format!(
            "MAX_DURATION_EXCEEDED 로 거부했는데도 저장소의 expires_at_unix_ms 가 연장됐다 \
             — 초과 시 UPDATE 를 실행하지 않아야 한다.\n이전={expires_before_22} 이후={expires_after_22}"
        ));
    }
    report.push_str(
        "22) max_total_duration_seconds 초과 — 서명된 MAX_DURATION_EXCEEDED 로 갱신 거부 확인 \
         (저장소 만료시각 불변)\n",
    );

    // ── 23. 대조군 — 한도 안에서는 여전히 RENEWED (오탐 없음) ──────────
    let lease_dir_23 = tempfile::tempdir()
        .map_err(|e| format!("lease store 임시 디렉터리 생성 실패(23): {e}"))?;
    let lease_db_path_23 = lease_dir_23.path().join("leases.sqlite3");
    let lease_db_23 = lease_db_path_23
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;

    let issue_23 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_23,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "3600",
            "--do-renew",
            "false",
        ],
        &["--do-renew", "false"],
    )?;
    if !issue_23.coordinator_success || !issue_23.agent_success {
        return Err(format!(
            "대조군 시나리오 1차(최초 발급) 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_23.coordinator_success,
            issue_23.coordinator_stdout,
            issue_23.coordinator_stderr,
            issue_23.agent_success,
            issue_23.agent_stdout,
            issue_23.agent_stderr
        ));
    }

    let renew_23 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_23,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "3600",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
        ],
        &["--do-renew", "true"],
    )?;
    if renew_23.coordinator_pid == issue_23.coordinator_pid {
        return Err(format!(
            "대조군 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 프로세스가 아니다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}",
            issue_23.coordinator_pid, renew_23.coordinator_pid
        ));
    }
    if !renew_23.coordinator_success || !renew_23.agent_success {
        return Err(format!(
            "한도(3600초) 안인데 갱신이 실패했다 — 오탐(false positive)이다.\n\
             1차 coordinator_pid={} 2차 coordinator_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_23.coordinator_pid,
            renew_23.coordinator_pid,
            renew_23.coordinator_success,
            renew_23.coordinator_stdout,
            renew_23.coordinator_stderr,
            renew_23.agent_success,
            renew_23.agent_stdout,
            renew_23.agent_stderr
        ));
    }
    report.push_str(
        "23) max_total_duration_seconds 대조군 — 한도 안에서는 여전히 RENEWED 확인 (오탐 없음)\n",
    );

    // ── 24. lease_store=Some 에서 override — 저장소 불변 확인 ──────────
    //
    // 코덱스 독립 검수(2026-08-19, p114)가 찾은 결함의 회귀 방지 —
    // 처음 구현은 `lease_store=Some` 이고 `--renew-outcome-override`
    // 도 있을 때, 초과 여부와 무관하게 먼저 저장소를 갱신(만료시각
    // 연장)한 **뒤에** override 를 적용했다 — "거부 응답인데 저장소는
    // 갱신됨" 이라는 상태 불일치였다. override 는 저장소를 **전혀**
    // 건드리지 않아야 한다(레거시 `None` 경로와 같은 계약).
    let lease_dir_24 = tempfile::tempdir()
        .map_err(|e| format!("lease store 임시 디렉터리 생성 실패(24): {e}"))?;
    let lease_db_path_24 = lease_dir_24.path().join("leases.sqlite3");
    let lease_db_24 = lease_db_path_24
        .to_str()
        .ok_or_else(|| "lease store 경로가 UTF-8 이 아니다".to_string())?;

    let issue_24 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_24,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "3600",
            "--do-renew",
            "false",
        ],
        &["--do-renew", "false"],
    )?;
    if !issue_24.coordinator_success || !issue_24.agent_success {
        return Err(format!(
            "override·저장소 불변 시나리오 1차(최초 발급) 실행이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_24.coordinator_success,
            issue_24.coordinator_stdout,
            issue_24.coordinator_stderr,
            issue_24.agent_success,
            issue_24.agent_stdout,
            issue_24.agent_stderr
        ));
    }

    let expires_before_24 = {
        let store = gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_24)
            .map_err(|e| format!("lease store 재조회 열기 실패(24, 사전): {e}"))?;
        store
            .get(fixture.lease_id)
            .map_err(|e| format!("lease store 재조회 실패(24, 사전): {e}"))?
            .ok_or_else(|| "lease store 에 24번 시나리오 lease 가 없다(사전)".to_string())?
            .expires_at_unix_ms
    };

    // 2차(별도 프로세스): 한도(3600초) 안인데 override=SUPERSEDED(2) 를
    // 강제 주입한다. 초과가 아니므로 override 값 그대로 나가야 하고,
    // 저장소는 전혀 갱신되지 않아야 한다.
    let override_24 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_24,
            "--fence-epoch",
            "5",
            "--max-total-duration-seconds",
            "3600",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--renew-outcome-override",
            "2",
        ],
        &["--do-renew", "true"],
    )?;
    if override_24.coordinator_pid == issue_24.coordinator_pid {
        return Err(format!(
            "override·저장소 불변 시나리오의 2차 coordinator PID 가 1차와 같다 — 별도 \
             프로세스가 아니다.\n1차 coordinator_pid={} 2차 coordinator_pid={}",
            issue_24.coordinator_pid, override_24.coordinator_pid
        ));
    }
    if override_24.agent_success
        || !override_24.agent_stderr.contains("RENEW_REFUSED:SUPERSEDED")
    {
        return Err(format!(
            "lease_store=Some 에서 override(SUPERSEDED) 가 서명된 정상 정책 거부로 \
             분류되지 않았다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            override_24.coordinator_success,
            override_24.coordinator_stdout,
            override_24.coordinator_stderr,
            override_24.agent_success,
            override_24.agent_stdout,
            override_24.agent_stderr
        ));
    }

    let expires_after_24 = {
        let store = gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(&lease_db_path_24)
            .map_err(|e| format!("lease store 재조회 열기 실패(24, 사후): {e}"))?;
        store
            .get(fixture.lease_id)
            .map_err(|e| format!("lease store 재조회 실패(24, 사후): {e}"))?
            .ok_or_else(|| "lease store 에 24번 시나리오 lease 가 없다(사후)".to_string())?
            .expires_at_unix_ms
    };
    if expires_after_24 != expires_before_24 {
        return Err(format!(
            "override(SUPERSEDED) 로 거부했는데도 저장소의 expires_at_unix_ms 가 갱신됐다 \
             — override 는 저장소를 전혀 건드리지 않아야 한다.\n\
             이전={expires_before_24} 이후={expires_after_24}"
        ));
    }
    report.push_str(
        "24) lease_store=Some + override — 서명된 정책 거부는 그대로이고 저장소는 \
         전혀 갱신되지 않음을 확인 (코덱스 p114 지적의 회귀 방지)\n",
    );

    // ══════════════════════════════════════════════════════════════
    // Coordinator의 실제 SUPERSEDED 정책 (2026-08-19)
    // ══════════════════════════════════════════════════════════════

    // ── 25. 저장된 epoch보다 낮은 요청 — signed SUPERSEDED, 연결 유지 ──
    // 최초 Grant는 저장된 epoch=5로 기록하고, 별도 Coordinator 프로세스에서
    // 같은 DB를 다시 연다. Agent가 Grant를 먼저 같은 durable watermark에
    // 기록한 뒤 요청 epoch=4를 보내므로, Coordinator가 raw error로 연결을
    // 끊었다면 coordinator 성공/RENEW_RESULT가 남을 수 없다.
    let superseded_dir_25 = tempfile::tempdir()
        .map_err(|e| format!("SUPERSEDED 시나리오 임시 디렉터리 생성 실패(25): {e}"))?;
    let lease_db_25 = superseded_dir_25.path().join("lease.sqlite3");
    let fence_db_25 = superseded_dir_25.path().join("fence.sqlite3");
    let lease_db_25 = lease_db_25
        .to_str()
        .ok_or_else(|| "SUPERSEDED lease store 경로가 UTF-8이 아니다(25)".to_string())?;
    let fence_db_25 = fence_db_25
        .to_str()
        .ok_or_else(|| "SUPERSEDED fence watermark 경로가 UTF-8이 아니다(25)".to_string())?;

    let issue_25 = run_handshake(
        &fixture,
        &["--lease-db", lease_db_25, "--fence-epoch", "5", "--do-renew", "false"],
        &["--fence-db", fence_db_25, "--do-renew", "false"],
    )?;
    if !issue_25.coordinator_success || !issue_25.agent_success {
        return Err(format!(
            "SUPERSEDED 시나리오 1차 발급이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_25.coordinator_success,
            issue_25.coordinator_stdout,
            issue_25.coordinator_stderr,
            issue_25.agent_success,
            issue_25.agent_stdout,
            issue_25.agent_stderr
        ));
    }

    let superseded_25 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_25,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
        ],
        &[
            "--fence-db",
            fence_db_25,
            "--do-renew",
            "true",
            "--renew-request-epoch-override",
            "4",
        ],
    )?;
    if !superseded_25.coordinator_success
        || !superseded_25
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=2")
        || !superseded_25.coordinator_stdout.contains(RESULT_OK_MARKER)
        || superseded_25.agent_success
        || !superseded_25
            .agent_stderr
            .contains("RENEW_REFUSED:SUPERSEDED")
        || superseded_25
            .agent_stderr
            .contains("RenewLeaseResult 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "영속 저장소의 낮은 epoch 요청이 signed SUPERSEDED로 정상 응답되지 않았다 — raw 연결 종료가 아니어야 한다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            superseded_25.coordinator_success,
            superseded_25.coordinator_stdout,
            superseded_25.coordinator_stderr,
            superseded_25.agent_success,
            superseded_25.agent_stdout,
            superseded_25.agent_stderr
        ));
    }
    report.push_str(
        "25) 영속 stored epoch(5)보다 낮은 renew 요청(4)을 연결 종료 없이 signed SUPERSEDED로 응답하고 Agent가 정상 정책 거부로 분류함\n",
    );

    // ── 26. 저장된 epoch와 같은 요청 — 기존 signed RENEWED 대조군 ──
    let same_epoch_dir_26 = tempfile::tempdir()
        .map_err(|e| format!("same epoch 시나리오 임시 디렉터리 생성 실패(26): {e}"))?;
    let lease_db_26 = same_epoch_dir_26.path().join("lease.sqlite3");
    let fence_db_26 = same_epoch_dir_26.path().join("fence.sqlite3");
    let lease_db_26 = lease_db_26
        .to_str()
        .ok_or_else(|| "same epoch lease store 경로가 UTF-8이 아니다(26)".to_string())?;
    let fence_db_26 = fence_db_26
        .to_str()
        .ok_or_else(|| "same epoch fence watermark 경로가 UTF-8이 아니다(26)".to_string())?;

    let issue_26 = run_handshake(
        &fixture,
        &["--lease-db", lease_db_26, "--fence-epoch", "7", "--do-renew", "false"],
        &["--fence-db", fence_db_26, "--do-renew", "false"],
    )?;
    if !issue_26.coordinator_success || !issue_26.agent_success {
        return Err(format!(
            "same epoch 시나리오 1차 발급이 실패했다(정상이어야 한다).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            issue_26.coordinator_success,
            issue_26.coordinator_stdout,
            issue_26.coordinator_stderr,
            issue_26.agent_success,
            issue_26.agent_stdout,
            issue_26.agent_stderr
        ));
    }
    let same_epoch_26 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_26,
            "--fence-epoch",
            "0",
            "--do-renew",
            "true",
        ],
        &["--fence-db", fence_db_26, "--do-renew", "true"],
    )?;
    if !same_epoch_26.coordinator_success
        || !same_epoch_26
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=1")
        || !same_epoch_26.agent_success
        || !same_epoch_26
            .agent_stdout
            .contains("RENEW_RESULT ok=true outcome=RENEWED")
    {
        return Err(format!(
            "저장된 epoch와 같은 epoch의 기본 renew가 RENEWED가 아니었다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            same_epoch_26.coordinator_success,
            same_epoch_26.coordinator_stdout,
            same_epoch_26.coordinator_stderr,
            same_epoch_26.agent_success,
            same_epoch_26.agent_stdout,
            same_epoch_26.agent_stderr
        ));
    }
    report.push_str("26) 저장된 epoch와 같은 epoch의 기본 renew가 signed RENEWED임을 확인\n");

    // ══════════════════════════════════════════════════════════════
    // Lease revoke 최소 조각 (2026-08-19)
    // ══════════════════════════════════════════════════════════════

    // ── 27. ACK 직후 revoke + 양쪽 renew 활성화의 교착 방지 ─────────
    let revoke_ok = run_handshake(
        &fixture,
        &[
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "0",
            "--revoke-after-round",
            "0",
            "--renew-rounds",
            "2",
        ],
        &[
            "--do-renew",
            "true",
            "--renew-rounds",
            "2",
            "--expect-revoke-after-round",
            "0",
        ],
    )?;
    if !revoke_ok.coordinator_success
        || !revoke_ok.agent_success
        || !revoke_ok.agent_stdout.contains("REVOKE_RESULT ok=true")
        || !revoke_ok.agent_stdout.contains("RENEW_BLOCKED: lease revoked")
        || revoke_ok.agent_stdout.matches("RENEW_RESULT ok=true").count() != 0
        || revoke_ok.coordinator_stdout.matches("RENEW_RESULT ok=true").count() != 0
    {
        return Err(format!(
            "ACK 직후 revoke 교착 방지 또는 revoke 뒤 renew 차단이 실패했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            revoke_ok.coordinator_success,
            revoke_ok.coordinator_stdout,
            revoke_ok.coordinator_stderr,
            revoke_ok.agent_success,
            revoke_ok.agent_stdout,
            revoke_ok.agent_stderr
        ));
    }
    report.push_str(
        "27) ACK 직후 revoke + 양쪽 renew=true 정상 종료 및 Agent의 revoke 후 renew 미생성 경로 확인 \
         (Coordinator/Agent stub이 즉시 종료하므로 wire상 추가 프레임 부재 자체는 이 selftest가 직접 증명하지 않음)\n",
    );

    // 공통으로 ACK 직후 revoke를 받는 단일 왕복 negative 시나리오다.
    // V-08 때문에 잘못된 target id도 selftest 전용 키 alias를 등록해
    // 서명 검증을 통과시킨 뒤 Agent identity 게이트를 실제로 밟게 한다.
    let forged_revoke = run_handshake(
        &fixture,
        &["--revoke-after-round", "0", "--corrupt-revoke-signature", "true"],
        &["--do-renew", "false", "--expect-revoke-after-round", "0"],
    )?;
    if forged_revoke.agent_success
        || !forged_revoke
            .agent_stderr
            .contains("RevokeLeaseNotice 프레임 읽기/검증 실패")
    {
        return Err(format!(
            "위조된 revoke 서명이 거부되지 않았다.\nagent exit={} stdout={} stderr={}",
            forged_revoke.agent_success, forged_revoke.agent_stdout, forged_revoke.agent_stderr
        ));
    }
    report.push_str("28) 위조 RevokeLeaseNotice 서명 거부 확인\n");

    let wrong_revoke_lease_id = "01JWRONGREVOKELEASE00000001";
    let wrong_id_revoke = run_handshake(
        &fixture,
        &[
            "--revoke-after-round",
            "0",
            "--revoke-lease-id",
            wrong_revoke_lease_id,
        ],
        &[
            "--do-renew",
            "false",
            "--expect-revoke-after-round",
            "0",
            "--revoke-signer-id",
            wrong_revoke_lease_id,
        ],
    )?;
    if wrong_id_revoke.agent_success
        || !wrong_id_revoke.agent_stderr.contains("REVOKE_REJECTED: lease_id 불일치")
    {
        return Err(format!(
            "잘못된 revoke lease_id가 거부되지 않았다.\nagent exit={} stdout={} stderr={}",
            wrong_id_revoke.agent_success, wrong_id_revoke.agent_stdout, wrong_id_revoke.agent_stderr
        ));
    }
    report.push_str("29) 잘못된 revoke lease_id 거부 확인\n");

    let wrong_epoch_revoke = run_handshake(
        &fixture,
        &["--revoke-after-round", "0", "--revoke-fence-epoch", "6"],
        &["--do-renew", "false", "--expect-revoke-after-round", "0"],
    )?;
    if wrong_epoch_revoke.agent_success
        || !wrong_epoch_revoke.agent_stderr.contains("REVOKE_REJECTED: fence_epoch 불일치")
    {
        return Err(format!(
            "잘못된 revoke fence_epoch이 거부되지 않았다.\nagent exit={} stdout={} stderr={}",
            wrong_epoch_revoke.agent_success,
            wrong_epoch_revoke.agent_stdout,
            wrong_epoch_revoke.agent_stderr
        ));
    }
    report.push_str("30) 잘못된 revoke fence_epoch 거부 확인\n");

    // Grant 시점에는 아직 유효하지만, Coordinator가 실제로 2.2초
    // 기다린 뒤 revoke를 보내므로 Agent의 보유 Lease는 이미 만료된다.
    let expired_revoke = run_handshake(
        &fixture,
        &[
            "--revoke-after-round",
            "0",
            "--lease-ttl-ms",
            "2000",
            "--revoke-delay-ms",
            "2200",
        ],
        &["--do-renew", "false", "--expect-revoke-after-round", "0"],
    )?;
    if expired_revoke.agent_success
        || !expired_revoke.agent_stderr.contains("REVOKE_REJECTED: held Lease가 이미 만료됐다")
    {
        return Err(format!(
            "이미 만료된 Lease에 대한 revoke가 거부되지 않았다.\nagent exit={} stdout={} stderr={}",
            expired_revoke.agent_success,
            expired_revoke.agent_stdout,
            expired_revoke.agent_stderr
        ));
    }
    report.push_str("31) 이미 만료된 Lease에 대한 revoke 거부 확인\n");

    // ── 32. SUPERSEDED가 다회차 갱신의 첫 회차에 발생하면 즉시 종료 ──
    // Agent의 정책 거부는 함수 자체를 끝내므로 Coordinator도 다음
    // RenewLeaseRequest를 기다리지 않고 같은 회차에서 루프를 끝내야 한다.
    // 이 시나리오는 renew_rounds=2로 첫 회차에서만 SUPERSEDED를 유도하고,
    // 두 번째 회차의 RENEW_RESULT가 없으며 양쪽 프로세스가 정상적으로
    // 수렴하는지를 확인한다.
    let superseded_multi_round = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_25,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--renewed-fence-epoch",
            "5",
            "--renew-rounds",
            "2",
        ],
        &[
            "--fence-db",
            fence_db_25,
            "--do-renew",
            "true",
            "--renew-request-epoch-override",
            "4",
            "--renew-rounds",
            "2",
        ],
    )?;
    if !superseded_multi_round.coordinator_success
        || !superseded_multi_round
            .coordinator_stdout
            .contains("RENEW_RESULT ok=true outcome=2")
        || superseded_multi_round
            .coordinator_stdout
            .matches("RENEW_RESULT ok=true")
            .count()
            != 1
        || !superseded_multi_round.coordinator_stdout.contains(RESULT_OK_MARKER)
        || superseded_multi_round.agent_success
        || !superseded_multi_round
            .agent_stderr
            .contains("RENEW_REFUSED:SUPERSEDED")
        || superseded_multi_round.agent_stdout.contains("RENEW_RESULT ok=true")
    {
        return Err(format!(
            "다회차 SUPERSEDED가 첫 회차에서 정상 종료되지 않았거나 이후 RENEW_RESULT가 발생했다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            superseded_multi_round.coordinator_success,
            superseded_multi_round.coordinator_stdout,
            superseded_multi_round.coordinator_stderr,
            superseded_multi_round.agent_success,
            superseded_multi_round.agent_stdout,
            superseded_multi_round.agent_stderr
        ));
    }
    report.push_str(
        "32) renew_rounds=2의 첫 회차 SUPERSEDED 후 양쪽이 교착 없이 종료되고 이후 RENEW_RESULT가 없음을 확인\n",
    );

    // ══════════════════════════════════════════════════════════════
    // Active Lease process-restart rehydration (2026-08-19)
    //
    // 기존 Grant/ACK handshake를 그대로 재사용한다. 첫 번째 프로세스
    // 쌍은 ACK를 실제로 받은 뒤 Coordinator가 의도적으로 연결을 닫고,
    // 두 번째 새 프로세스 쌍은 같은 lease/fence DB를 열어 저장된 Lease를
    // 복원한다. 새 proto 메시지나 자동 재접속은 이 조각에 없다.
    // ══════════════════════════════════════════════════════════════

    // ── 33. ACK 뒤 단절 후 새 프로세스 쌍의 active Lease 복원 ───────
    let reconnect_dir_33 = tempfile::tempdir()
        .map_err(|e| format!("재접속 시나리오 임시 디렉터리 생성 실패(33): {e}"))?;
    let lease_db_path_33 = reconnect_dir_33.path().join("lease.sqlite3");
    let fence_db_path_33 = reconnect_dir_33.path().join("fence.sqlite3");
    let lease_db_33 = lease_db_path_33
        .to_str()
        .ok_or_else(|| "재접속 lease store 경로가 UTF-8이 아니다(33)".to_string())?;
    let fence_db_33 = fence_db_path_33
        .to_str()
        .ok_or_else(|| "재접속 fence watermark 경로가 UTF-8이 아니다(33)".to_string())?;

    let disconnected_33 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_33,
            "--fence-epoch",
            "5",
            "--do-renew",
            "true",
            "--disconnect-after-ack",
            "true",
        ],
        &["--fence-db", fence_db_33, "--do-renew", "true"],
    )?;
    if !disconnected_33.coordinator_success
        || !disconnected_33
            .coordinator_stdout
            .contains("DISCONNECT_AFTER_ACK coordinator_acknowledged=true")
        || disconnected_33.agent_success
        || disconnected_33.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "ACK 뒤 의도적 연결 단절이 정상적으로 관측되지 않았다(33) — Coordinator는 ACK 후 종료하고 Agent는 기존 오류 전파로 종료해야 한다.\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            disconnected_33.coordinator_success,
            disconnected_33.coordinator_stdout,
            disconnected_33.coordinator_stderr,
            disconnected_33.agent_success,
            disconnected_33.agent_stdout,
            disconnected_33.agent_stderr
        ));
    }

    let rehydrated_33 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_33,
            "--fence-epoch",
            "3",
            "--do-renew",
            "false",
        ],
        &["--fence-db", fence_db_33, "--do-renew", "false"],
    )?;
    if rehydrated_33.coordinator_pid == disconnected_33.coordinator_pid
        || rehydrated_33.agent_pid == disconnected_33.agent_pid
        || !rehydrated_33.coordinator_success
        || !rehydrated_33.agent_success
        || !rehydrated_33.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "연결 단절 뒤 새 Coordinator/Agent 프로세스 쌍이 저장된 Lease를 복원하지 못했다(33) — \
             새 PID, 같은 DB, CLI fence_epoch=3이어도 저장된 epoch=5 Grant/ACK가 성공해야 한다.\n\
             1차 coordinator_pid={} agent_pid={}\n\
             2차 coordinator_pid={} agent_pid={}\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            disconnected_33.coordinator_pid,
            disconnected_33.agent_pid,
            rehydrated_33.coordinator_pid,
            rehydrated_33.agent_pid,
            rehydrated_33.coordinator_success,
            rehydrated_33.coordinator_stdout,
            rehydrated_33.coordinator_stderr,
            rehydrated_33.agent_success,
            rehydrated_33.agent_stdout,
            rehydrated_33.agent_stderr
        ));
    }
    report.push_str(
        "33) ACK 직후 의도적 연결 단절 후 새 프로세스 쌍이 같은 lease/fence DB에서 active Lease를 복원하고, 잘못된 CLI fence_epoch=3 대신 저장된 epoch=5로 Grant/ACK 성공\n",
    );

    // ── 34. 같은 lease_id를 다른 holder identity로 주장 ─────────────
    // `get_or_issue()`의 기존 holder_node_id IdentityConflict가 실제
    // Coordinator 발급 호출부에서 동작하는지 확인한다. 새 Agent도 같은
    // 잘못된 identity를 사용해 Grant의 holder 검증에서 먼저 가려지지
    // 않도록 한다 — 거부 주체가 Coordinator임을 확인하는 시나리오다.
    let wrong_holder_id = "01JOTHERAGENTSELFTEST00000001";
    let wrong_holder_34 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_33,
            "--fence-epoch",
            "5",
            "--agent-device-id",
            wrong_holder_id,
            "--do-renew",
            "false",
        ],
        &[
            "--fence-db",
            fence_db_33,
            "--agent-device-id",
            wrong_holder_id,
            "--do-renew",
            "false",
        ],
    )?;
    if wrong_holder_34.coordinator_success
        || !wrong_holder_34
            .coordinator_stderr
            .contains("holder_node_id")
        || !wrong_holder_34
            .coordinator_stderr
            .contains("lease store 최초 발급 실패")
        || wrong_holder_34.agent_success
    {
        return Err(format!(
            "같은 lease_id의 다른 holder_node_id가 Coordinator에서 거부되지 않았다(34).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            wrong_holder_34.coordinator_success,
            wrong_holder_34.coordinator_stdout,
            wrong_holder_34.coordinator_stderr,
            wrong_holder_34.agent_success,
            wrong_holder_34.agent_stdout,
            wrong_holder_34.agent_stderr
        ));
    }
    report.push_str(
        "34) 같은 lease_id를 다른 holder_node_id로 재접속 주장 시 Coordinator의 기존 IdentityConflict 거부 확인\n",
    );

    // ── 35. revoke 상태의 Coordinator 영속화와 재시작 거부 ─────────────
    let revoke_persistence_dir = tempfile::tempdir()
        .map_err(|e| format!("revoke 영속화 시나리오 임시 디렉터리 생성 실패(35): {e}"))?;
    let lease_db_35 = revoke_persistence_dir.path().join("lease.sqlite3");
    let fence_db_35 = revoke_persistence_dir.path().join("fence.sqlite3");
    let lease_db_35 = lease_db_35
        .to_str()
        .ok_or_else(|| "revoke 영속화 lease store 경로가 UTF-8이 아니다(35)".to_string())?;
    let fence_db_35 = fence_db_35
        .to_str()
        .ok_or_else(|| "revoke 영속화 fence watermark 경로가 UTF-8이 아니다(35)".to_string())?;

    let revoked_first_35 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_35,
            "--fence-epoch",
            "5",
            "--revoke-after-round",
            "0",
            "--do-renew",
            "false",
        ],
        &[
            "--fence-db",
            fence_db_35,
            "--do-renew",
            "false",
            "--expect-revoke-after-round",
            "0",
        ],
    )?;
    if !revoked_first_35.coordinator_success
        || !revoked_first_35.agent_success
        || !revoked_first_35.agent_stdout.contains("REVOKE_RESULT ok=true")
    {
        return Err(format!(
            "revoke 영속화 시나리오 1차 프로세스 쌍이 정상 종료하지 않았다(35).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            revoked_first_35.coordinator_success,
            revoked_first_35.coordinator_stdout,
            revoked_first_35.coordinator_stderr,
            revoked_first_35.agent_success,
            revoked_first_35.agent_stdout,
            revoked_first_35.agent_stderr
        ));
    }

    let stored_revoked_35 = gputeer_coordinator::lease_store::CoordinatorLeaseStore::open(
        lease_db_35,
    )
    .map_err(|e| format!("revoke 영속화 lease store 재조회 열기 실패(35): {e}"))?
    .get(fixture.lease_id)
    .map_err(|e| format!("revoke 영속화 lease store 재조회 실패(35): {e}"))?
    .ok_or_else(|| "revoke 영속화 lease store에 Lease가 없다(35)".to_string())?;
    if stored_revoked_35.revoked_at_unix_ms.is_none() {
        return Err(
            "revoke 통지 성공 뒤 lease store의 revoked_at_unix_ms가 NULL이다(35)".to_string(),
        );
    }

    let revoked_second_35 = run_handshake(
        &fixture,
        &[
            "--lease-db",
            lease_db_35,
            "--fence-epoch",
            "5",
            "--do-renew",
            "false",
        ],
        &[
            "--fence-db",
            fence_db_35,
            "--do-renew",
            "false",
        ],
    )?;
    if revoked_second_35.coordinator_success
        || !revoked_second_35
            .coordinator_stderr
            .contains("lease store 최초 발급 실패")
        || !revoked_second_35.coordinator_stderr.contains("revoked")
        || revoked_second_35.agent_success
        || !revoked_second_35
            .agent_stderr
            .contains("Grant 프레임 읽기/검증 실패")
        || revoked_second_35.coordinator_stdout.contains(RESULT_OK_MARKER)
        || revoked_second_35.agent_stdout.contains(RESULT_OK_MARKER)
    {
        return Err(format!(
            "revoke 상태가 새 프로세스 쌍에서 거부되지 않았다(35).\n\
             coordinator exit={} stdout={} stderr={}\n\
             agent exit={} stdout={} stderr={}",
            revoked_second_35.coordinator_success,
            revoked_second_35.coordinator_stdout,
            revoked_second_35.coordinator_stderr,
            revoked_second_35.agent_success,
            revoked_second_35.agent_stdout,
            revoked_second_35.agent_stderr
        ));
    }
    report.push_str(
        "35) revoke 통지 후 revoked_at_unix_ms 영속화 및 새 Coordinator의 revoked Lease 최초 발급 거부 확인\n",
    );

    Ok(report)
}

/// 시나리오 18 전용 — Agent 하나만 단독으로 띄워 `--fence-db :memory:`
/// 가 네트워크 연결조차 시도하기 전에 fail closed 하는지 확인한다.
/// Coordinator 는 필요 없다 — `DurableFenceWatermark::is_durable()`
/// 검사가 `TcpStream::connect()` 보다 먼저 실행되므로(`crates/agent/src/lib.rs`),
/// `--connect` 주소가 실제로 열려 있지 않아도 상관없다.
fn run_agent_alone(
    fixture: &Fixture,
    extra_agent_args: &[&str],
) -> Result<(bool, String, String), String> {
    let mut agent_args: Vec<&str> = vec!["agent-stub", "--connect", "127.0.0.1:1", "--own-seed"];
    let agent_own_seed_hex = to_hex(&fixture.agent_seed);
    agent_args.push(&agent_own_seed_hex);
    agent_args.push("--peer-pubkey");
    agent_args.push(&fixture.coordinator_pub_hex);
    agent_args.push("--coordinator-device-id");
    agent_args.push(fixture.coordinator_device_id);
    agent_args.push("--agent-device-id");
    agent_args.push(fixture.agent_device_id);
    agent_args.extend_from_slice(extra_agent_args);

    let output = Command::new(&fixture.exe)
        .args(&agent_args)
        .output()
        .map_err(|e| format!("agent-stub 단독 실행 실패: {e}"))?;

    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn seed_from_label(label: &str) -> [u8; 32] {
    gputeer_protocol::canonical::blake3_256(label.as_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
