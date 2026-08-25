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
//! 두 검증의 시점을 의도적으로 분리한다.
//! - worker-only 경합: 8개 프로세스가 모두 `ready` barrier 에 도달한
//!   뒤 한꺼번에 풀리고, 그 **첫 API 호출** 자체에서 정확히 하나만
//!   `Fresh` 인지 확인한다. holder 도 재시도도 없다.
//! - 강제 lock 경합: holder 가 모든 worker 의 첫 호출이 반환할 때까지
//!   write lock 을 유지하고, 그 첫 반환 전부가 실제 `LockTimeout` 인지
//!   확인한다. holder 해제 뒤 재시도 결과는 오직 멱등 수렴 검증이며
//!   동시 `Fresh` 불변식의 근거로 사용하지 않는다.

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

#[derive(Debug)]
struct Observation {
    initial_outcome: String,
    retry_outcome: String,
}

fn parse_worker_output(stdout: &[u8]) -> Observation {
    let text = String::from_utf8_lossy(stdout);
    let line = text
        .lines()
        .find(|line| line.starts_with("RESULT "))
        .unwrap_or_else(|| panic!("missing RESULT line:\n{text}"));

    let field = |key: &str| {
        line.split_whitespace()
            .find_map(|part| part.strip_prefix(key))
            .unwrap_or_else(|| panic!("missing {key} in {line:?}"))
            .to_string()
    };

    Observation {
        initial_outcome: field("initial_outcome="),
        retry_outcome: field("retry_outcome="),
    }
}

struct Scenario {
    observations: Vec<Observation>,
    entry_count: usize,
}

fn run_scenario(
    workers: usize,
    distinct_nonces: bool,
    seed_same_nonce: bool,
    mode: &str,
    with_holder: bool,
) -> Scenario {
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
                .check_and_record(
                    "process-race-signer",
                    Domain::Grant,
                    &nonce,
                    4_000_000_000_000
                )
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
            .arg(mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn replay worker fixture — cargo build -p gputeer-crypto 를 먼저 해야 한다");

        wait_for(&gate.join(format!("ready-{id}")));
        children.push(child);
    }

    let holder = with_holder.then(|| {
        let child = Command::new(fixture_bin())
            .arg("holder")
            .arg(&database)
            .arg(&gate)
            .arg(workers.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn SQLite lock-holder fixture");

        // BEGIN IMMEDIATE 가 실제로 락을 얻었다는 확인이다.
        wait_for(&gate.join("locked"));
        child
    });

    // 모든 worker process 가 guard 를 열고 ready marker 를 남긴 뒤에만
    // 공용 start barrier 를 푼다. worker-race 는 이 barrier 뒤 단 한
    // 번 호출하므로, worker 를 하나씩 실행·종료하는 순차 하네스로는
    // 이 지점에 도달할 수 없다.
    std::fs::write(gate.join("start"), b"START").unwrap();

    if let Some(holder) = holder {
        // holder 가 모든 worker 의 1차 호출 직전 marker 를 확인했다.
        // 실제 lock 대기는 아래 initial_outcome 으로 별도 검증한다.
        wait_for(&gate.join("contended"));

        let holder_output = holder.wait_with_output().expect("holder wait failed");

        assert!(
            holder_output.status.success(),
            "holder failed: stdout={:?}, stderr={:?}",
            String::from_utf8_lossy(&holder_output.stdout),
            String::from_utf8_lossy(&holder_output.stderr)
        );
    }

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
        entry_count: guard.entry_count().unwrap(),
    }
}

fn count_by(
    observations: &[Observation],
    select: impl Fn(&Observation) -> &str,
    expected: &str,
) -> usize {
    observations
        .iter()
        .filter(|observation| select(observation) == expected)
        .count()
}

fn count(observations: &[Observation], expected: &str) -> usize {
    count_by(
        observations,
        |observation| &observation.initial_outcome,
        expected,
    )
}

/// 모든 worker 의 같은 API 호출이 실제 SQLite lock 대기에서
/// LockTimeout 으로 반환했는지 확인한다. holder 는 모든 호출의 반환
/// marker 전에는 COMMIT 하지 않으므로 스케줄링이나 시각 비교에
/// 기대지 않는 경합 증거다.
fn assert_all_initial_attempts_contended(scenario: &Scenario) {
    assert_eq!(
        count_by(
            &scenario.observations,
            |observation| &observation.initial_outcome,
            "LockTimeout",
        ),
        scenario.observations.len(),
        "not every worker demonstrably waited on the holder lock: {:?}",
        scenario.observations
    );
}

/// 핵심 불변식 — 별도 프로세스 8개가 **같은 nonce** 로 동시에
/// 경합해도 정확히 하나만 Fresh 를 받는다. 이중 승인은 replay
/// 방어의 존재 이유 자체를 무너뜨린다.
#[test]
fn separate_processes_same_nonce_have_exactly_one_fresh() {
    let scenario = run_scenario(WORKERS, false, false, "worker-race", false);

    assert_eq!(count(&scenario.observations, "Fresh"), 1);
    assert_eq!(count(&scenario.observations, "Duplicate"), WORKERS - 1);
    assert_eq!(count(&scenario.observations, "LockTimeout"), 0);
    assert_eq!(count(&scenario.observations, "Other"), 0);
    assert_eq!(
        count_by(
            &scenario.observations,
            |observation| &observation.retry_outcome,
            "NotRun"
        ),
        WORKERS
    );
    assert_eq!(scenario.entry_count, 1);
}

/// 비공허성 — 항상 거부하는 버그가 아님을 확인한다. 서로 다른
/// nonce 를 쓰는 프로세스는 전부 통과해야 한다.
#[test]
fn separate_processes_distinct_nonces_are_all_fresh() {
    let scenario = run_scenario(WORKERS, true, false, "worker-race", false);

    assert_eq!(count(&scenario.observations, "Fresh"), WORKERS);
    assert_eq!(count(&scenario.observations, "Duplicate"), 0);
    assert_eq!(count(&scenario.observations, "LockTimeout"), 0);
    assert_eq!(count(&scenario.observations, "Other"), 0);
    assert_eq!(scenario.entry_count, WORKERS);
}

/// 외부 holder 가 모든 첫 호출의 반환까지 write lock 을 유지하므로,
/// 8개 initial_outcome 전부가 LockTimeout 이어야 한다. released marker
/// 뒤의 retry_outcome 은 경합 중 판정이 아니라 해제 후 멱등성 확인이다.
#[test]
fn separate_processes_all_initial_attempts_lock_timeout_then_retry_idempotently() {
    let scenario = run_scenario(WORKERS, false, false, "held-retry", true);

    assert_all_initial_attempts_contended(&scenario);
    assert_eq!(
        count_by(
            &scenario.observations,
            |observation| &observation.retry_outcome,
            "Fresh"
        ),
        1
    );
    assert_eq!(
        count_by(
            &scenario.observations,
            |observation| &observation.retry_outcome,
            "Duplicate"
        ),
        WORKERS - 1
    );
    assert_eq!(scenario.entry_count, 1);
}

/// `LockTimeout` 이 `Duplicate` 로 위장되지 않는다는 계약이
/// 프로세스 경계에서도 지켜지는지 확인한다. 이미 기록된 nonce 를
/// 다시 검사하는 동안 holder 가 worker 의 호출이 반환할 때까지 락을
/// 쥐고 있으면, 그 결과는 "이미 봤다"(Duplicate)가 아니라
/// "저장소를 확정하지 못했다"(LockTimeout)여야 한다 — 둘을 섞으면
/// "확인했다" 와 "확인 못 했다" 가 같은 값으로 보인다.
#[test]
fn separate_process_lock_timeout_is_not_duplicate() {
    let scenario = run_scenario(1, false, true, "held", true);

    let observation = &scenario.observations[0];

    assert_eq!(observation.initial_outcome, "LockTimeout");
    assert_eq!(observation.retry_outcome, "NotRun");
    assert_ne!(observation.initial_outcome, "Duplicate");
    assert_ne!(observation.initial_outcome, "Fresh");

    // 사전 기록된 nonce 1개만 남아 있어야 한다 — LockTimeout 이
    // 조용히 새 항목을 만들지 않았는지 확인한다.
    assert_eq!(scenario.entry_count, 1);
}
