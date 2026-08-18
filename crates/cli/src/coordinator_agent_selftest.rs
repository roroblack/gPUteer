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

/// 4가지 시나리오(정상 1 + 거부 경로 3)를 차례로 돌린다. 하나라도
/// 기대와 다르면 그 자리에서 이유를 담아 반환한다.
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
    if restart_second.agent_success
        || !restart_second
            .agent_stderr
            .contains("LEASE_REJECTED: fence_epoch 검사 실패")
    {
        return Err(format!(
            "durable watermark 가 재시작(별도 프로세스)을 넘어 낮은 epoch 의 최초 Grant 를 \
             거부하지 못했다.\n\
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

    Ok(report)
}

fn seed_from_label(label: &str) -> [u8; 32] {
    gputeer_protocol::canonical::blake3_256(label.as_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
