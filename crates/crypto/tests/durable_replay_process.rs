//! `DurableReplayGuard` — **별도 프로세스** 경쟁 실측.
//!
//! `RULE.md` §6 — 정상 경로만으로는 부족하다. `crates/crypto/tests/durable_replay_race.rs`
//! 는 같은 프로세스 안 여러 **스레드**로만 경쟁을 만들었다. 이 파일은
//! 진짜 다른 OS 프로세스가 같은 SQLite 파일을 두고 실제로 경합할
//! 때의 동작을 확인한다 — Rust 스레드 락과 SQLite 파일 락은 서로
//! 다른 메커니즘이고, Windows 파일 잠금 관련 가정이 이 저장소에서
//! 두 번(체크포인트 GC 경합, `write_failure.rs` 의 FILE_SHARE_DELETE)
//! 이미 틀린 전례가 있다.
//!
//! ★ 단순히 여러 자식 프로세스를 거의 동시에 띄우는 것만으로는
//! 진짜 경합을 보장하지 않는다 — 먼저 뜬 프로세스가 끝나 버릴 수
//! 있다. 그래서 `holder` fixture 가 SQLite write lock 을 실제로
//! 쥔 채, **모든 worker 가 그 락이 걸린 동안 `check_and_record`
//! 호출 직전 지점에 도달했다는 것을 파일 마커로 확인한 뒤에만**
//! 락을 놓는다 — "우연히 겹쳤을 수도 있다" 가 아니라 "반드시
//! 겹쳤다" 를 만든다.

use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use gputeer_crypto::DurableReplayGuard;
use gputeer_protocol::{
    canonical::Domain,
    signing::{ReplayDecision, ReplayGuard},
};

const WORKERS: usize = 8;

fn fixture_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("test executable");
    path.pop();

    if path.ends_with("deps") {
        path.pop();
    }

    let name = if cfg!(windows) {
        "durable_replay_process_fixture.exe"
    } else {
        "durable_replay_process_fixture"
    };

    path.join(name)
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);

    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn kv_u128(line: &str, key: &str) -> u128 {
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(key).and_then(|value| value.parse::<u128>().ok()))
        .unwrap_or_else(|| panic!("missing {key} in {line:?}"))
}

#[derive(Debug)]
struct Observation {
    outcome: String,
    start_ns: u128,
    end_ns: u128,
}

fn parse_worker_output(stdout: &[u8]) -> Observation {
    let text = String::from_utf8_lossy(stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("RESULT "))
        .unwrap_or_else(|| panic!("missing RESULT line:\n{text}"));

    let outcome = line
        .split_whitespace()
        .find_map(|part| part.strip_prefix("outcome="))
        .expect("missing outcome")
        .to_string();

    Observation {
        outcome,
        start_ns: kv_u128(line, "start_ns="),
        end_ns: kv_u128(line, "end_ns="),
    }
}

fn holder_times(stdout: &[u8]) -> (u128, u128) {
    let text = String::from_utf8_lossy(stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("HOLDER "))
        .unwrap_or_else(|| panic!("missing HOLDER line:\n{text}"));

    (kv_u128(line, "locked_ns="), kv_u128(line, "released_ns="))
}

struct Scenario {
    observations: Vec<Observation>,
    holder_stdout: Vec<u8>,
    entry_count: usize,
}

fn run_scenario(workers: usize, distinct_nonces: bool, timeout_mode: bool, seed_same_nonce: bool) -> Scenario {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().join("replay.sqlite3");
    let gate = temp.path().join("gate");
    std::fs::create_dir(&gate).unwrap();

    // SQLite 스키마를 worker 경쟁 전에 확정해 둔다 — 스키마 생성
    // 자체가 별도 락을 만드는 것을 경쟁 측정에서 배제한다.
    drop(DurableReplayGuard::open(&database).unwrap());

    if seed_same_nonce {
        let mut guard = DurableReplayGuard::open(&database).unwrap();
        let mut nonce = [0u8; 16];
        nonce[0] = 0xa5;

        assert_eq!(
            guard
                .check_and_record("process-race-signer", Domain::Grant, &nonce, 4_000_000_000_000)
                .unwrap(),
            ReplayDecision::Fresh
        );
    }

    let mut children: Vec<Child> = Vec::with_capacity(workers);

    for id in 0..workers {
        let nonce_second_byte = if distinct_nonces { id as u8 } else { 0 };

        let child = Command::new(fixture_bin())
            .arg("worker")
            .arg(&database)
            .arg(&gate)
            .arg(id.to_string())
            .arg(nonce_second_byte.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn replay worker fixture — cargo build -p gputeer-crypto 를 먼저 해야 한다");

        wait_for(&gate.join(format!("ready-{id}")));
        children.push(child);
    }

    let holder = Command::new(fixture_bin())
        .arg("holder")
        .arg(&database)
        .arg(&gate)
        .arg(workers.to_string())
        .arg(if timeout_mode { "1" } else { "0" })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn SQLite lock-holder fixture");

    // BEGIN IMMEDIATE 가 실제로 락을 얻었다는 확인이다.
    wait_for(&gate.join("locked"));

    // 모든 worker 를 한 번에 깨운다.
    std::fs::write(gate.join("start"), b"START").unwrap();

    // holder 는 이 marker 가 생기기 전까지 commit 하지 않는다 —
    // 그래서 이 대기가 끝났다는 것 자체가 "모든 worker 가 락 보유
    // 구간에 실제로 도달했다" 는 비공허성 증거다.
    wait_for(&gate.join("contended"));

    if !timeout_mode {
        std::fs::write(gate.join("release"), b"RELEASE").unwrap();
    }

    let holder_output = holder.wait_with_output().expect("holder wait failed");

    assert!(
        holder_output.status.success(),
        "holder failed: stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&holder_output.stdout),
        String::from_utf8_lossy(&holder_output.stderr)
    );

    let mut observations = Vec::with_capacity(workers);

    for child in children {
        let output = child.wait_with_output().expect("worker wait failed");

        assert!(
            output.status.success(),
            "worker failed: stdout={:?}, stderr={:?}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        observations.push(parse_worker_output(&output.stdout));
    }

    let guard = DurableReplayGuard::open(&database).unwrap();

    Scenario {
        observations,
        holder_stdout: holder_output.stdout,
        entry_count: guard.entry_count().unwrap(),
    }
}

fn count(observations: &[Observation], expected: &str) -> usize {
    observations.iter().filter(|observation| observation.outcome == expected).count()
}

/// 비공허성의 핵심 — 모든 worker 의 호출이 실제로 holder 의 락
/// 보유 구간 안에서 시작됐는지 타임스탬프로 재확인한다. 이게
/// 실패하면 "경합을 만들었다"는 이 테스트 전체의 전제가 무너진다.
fn assert_calls_started_while_lock_was_held(scenario: &Scenario) {
    let (locked_ns, released_ns) = holder_times(&scenario.holder_stdout);

    for observation in &scenario.observations {
        assert!(
            observation.start_ns >= locked_ns,
            "worker started before holder lock: {observation:?}"
        );
        assert!(
            observation.start_ns <= released_ns,
            "worker started after holder release: {observation:?}"
        );
    }
}

/// 핵심 불변식 — 별도 프로세스 8개가 **같은 nonce** 로 동시에
/// 경합해도 정확히 하나만 Fresh 를 받는다. 이중 승인은 replay
/// 방어의 존재 이유 자체를 무너뜨린다.
#[test]
fn separate_processes_same_nonce_have_exactly_one_fresh() {
    let scenario = run_scenario(WORKERS, false, false, false);

    assert_calls_started_while_lock_was_held(&scenario);

    assert_eq!(count(&scenario.observations, "Fresh"), 1);
    assert_eq!(count(&scenario.observations, "Duplicate"), WORKERS - 1);
    assert_eq!(count(&scenario.observations, "LockTimeout"), 0);
    assert_eq!(count(&scenario.observations, "Other"), 0);
    assert_eq!(scenario.entry_count, 1);
}

/// 비공허성 — 항상 거부하는 버그가 아님을 확인한다. 서로 다른
/// nonce 를 쓰는 프로세스는 전부 통과해야 한다.
#[test]
fn separate_processes_distinct_nonces_are_all_fresh() {
    let scenario = run_scenario(WORKERS, true, false, false);

    assert_calls_started_while_lock_was_held(&scenario);

    assert_eq!(count(&scenario.observations, "Fresh"), WORKERS);
    assert_eq!(count(&scenario.observations, "Duplicate"), 0);
    assert_eq!(count(&scenario.observations, "LockTimeout"), 0);
    assert_eq!(count(&scenario.observations, "Other"), 0);
    assert_eq!(scenario.entry_count, WORKERS);
}

/// `LockTimeout` 이 `Duplicate` 로 위장되지 않는다는 계약이
/// 프로세스 경계에서도 지켜지는지 확인한다. 이미 기록된 nonce 를
/// 다시 검사하는 동안 holder 가 worker 의 호출이 반환할 때까지 락을
/// 쥐고 있으면, 그 결과는 "이미 봤다"(Duplicate)가 아니라
/// "저장소를 확정하지 못했다"(LockTimeout)여야 한다 — 둘을 섞으면
/// "확인했다" 와 "확인 못 했다" 가 같은 값으로 보인다.
#[test]
fn separate_process_lock_timeout_is_not_duplicate() {
    let scenario = run_scenario(1, false, true, true);

    let (locked_ns, released_ns) = holder_times(&scenario.holder_stdout);
    let observation = &scenario.observations[0];

    assert_eq!(observation.outcome, "LockTimeout");
    assert_ne!(observation.outcome, "Duplicate");
    assert_ne!(observation.outcome, "Fresh");

    assert!(observation.start_ns >= locked_ns);
    assert!(observation.end_ns <= released_ns);

    // 사전 기록된 nonce 1개만 남아 있어야 한다 — LockTimeout 이
    // 조용히 새 항목을 만들지 않았는지 확인한다.
    assert_eq!(scenario.entry_count, 1);
}
