//! `DurableReplayGuard` 별도 프로세스 경쟁 테스트용 fixture.
//!
//! `crates/crypto/tests/durable_replay_process.rs` 가 이 바이너리를
//! 여러 개 별도 OS 프로세스로 띄워 같은 SQLite replay DB 에 동시에
//! 접근시킨다. 스레드 안에서만 측정하던 기존
//! `crates/crypto/tests/durable_replay_race.rs` 와 달리, 이 fixture
//! 는 **진짜 프로세스 경계**를 넘는 SQLite 잠금 경합을 만든다.
//!
//! 두 모드:
//!   worker <db> <gate> <id> <nonce-byte> <mode>
//!                                          DurableReplayGuard 로 nonce 기록 시도
//!   holder <db> <gate> <workers>           별도 연결로 BEGIN IMMEDIATE 락을 쥐고 있는다
//!
//! `holder` 는 `DurableReplayGuard` 의 공개 API 를 우회하는 것이
//! 아니다 — "트랜잭션을 일정 시간 쥐고 있는" 테스트 전용 기능이
//! 그 API 에 없기 때문에, 별도 rusqlite 연결로 같은 파일에 락을
//! 걸어 경합을 강제한다.

use std::{
    env, fs,
    path::Path,
    process, thread,
    time::{Duration, Instant},
};

use gputeer_crypto::DurableReplayGuard;
use gputeer_protocol::{
    canonical::Domain,
    signing::{ReplayDecision, ReplayGuard, ReplayStoreError},
};
use rusqlite::Connection;

const RETAIN_UNTIL_MS: u64 = 4_000_000_000_000;

/// `gate` 디렉터리에 `path` 파일이 생길 때까지 폴링한다.
/// named event 대신 파일 존재를 쓰는 이유: 플랫폼 독립적이고,
/// `kill_chaos.rs` 가 이미 같은 방식(stdout 라인 감시)을 쓴다.
fn wait_for(path: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);

    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {}", path.display()));
        }
        thread::sleep(Duration::from_millis(1));
    }

    Ok(())
}

fn outcome(result: &Result<ReplayDecision, ReplayStoreError>) -> &'static str {
    match result {
        Ok(ReplayDecision::Fresh) => "Fresh",
        Ok(ReplayDecision::Duplicate) => "Duplicate",
        Err(ReplayStoreError::LockTimeout) => "LockTimeout",
        Err(_) => "Other",
    }
}

fn worker(
    database: &Path,
    gate: &Path,
    id: usize,
    nonce_second_byte: u8,
    mode: &str,
) -> Result<(), String> {
    let mut guard = DurableReplayGuard::open(database)
        .map_err(|e| format!("worker {id} open failed: {e:?}"))?;

    fs::write(gate.join(format!("ready-{id}")), b"READY")
        .map_err(|e| format!("worker {id} ready marker failed: {e}"))?;

    wait_for(&gate.join("start"))?;

    let mut nonce = [0u8; 16];
    nonce[0] = 0xa5;
    nonce[1] = nonce_second_byte;

    // worker-race 에서는 모든 worker 가 이 지점 바로 앞의 start
    // barrier 에 함께 대기했다가, 이 단 한 번의 호출 결과로 동시성
    // 불변식을 판정한다. held-* 에서는 holder 가 모든 completed
    // marker 를 확인할 때까지 write lock 을 유지한다.
    fs::write(gate.join(format!("call-{id}")), b"CALL_BEGIN")
        .map_err(|e| format!("worker {id} call marker failed: {e}"))?;

    let initial_result = guard.check_and_record(
        "process-race-signer",
        Domain::Grant,
        &nonce,
        RETAIN_UNTIL_MS,
    );
    let initial_outcome = outcome(&initial_result);

    if mode == "worker-race" {
        println!(
            "RESULT id={id} pid={} initial_outcome={initial_outcome} retry_outcome=NotRun initial_result={initial_result:?}",
            process::id()
        );
        return Ok(());
    }

    if !matches!(mode, "held" | "held-retry") {
        return Err(format!("worker {id} has unknown mode {mode:?}"));
    }

    fs::write(
        gate.join(format!("completed-{id}")),
        initial_outcome.as_bytes(),
    )
    .map_err(|e| format!("worker {id} completed marker failed: {e}"))?;

    if mode == "held" {
        println!(
            "RESULT id={id} pid={} initial_outcome={initial_outcome} retry_outcome=NotRun initial_result={initial_result:?}",
            process::id()
        );
        return Ok(());
    }

    // 이 재시도는 holder 의 COMMIT 뒤에만 시작한다. 따라서 그 결과는
    // 동시 경합 불변식의 증거가 아니라, LockTimeout 뒤 재시도가 같은
    // nonce 에 대해 멱등적으로 수렴하는지만 확인한다.
    wait_for(&gate.join("released"))?;

    let retry_result = guard.check_and_record(
        "process-race-signer",
        Domain::Grant,
        &nonce,
        RETAIN_UNTIL_MS,
    );
    let retry_outcome = outcome(&retry_result);

    println!(
        "RESULT id={id} pid={} initial_outcome={initial_outcome} retry_outcome={retry_outcome} initial_result={initial_result:?} retry_result={retry_result:?}",
        process::id()
    );

    Ok(())
}

fn holder(database: &Path, gate: &Path, workers: usize) -> Result<(), String> {
    let connection = Connection::open(database).map_err(|e| format!("holder open failed: {e}"))?;

    connection
        .execute_batch("BEGIN IMMEDIATE;")
        .map_err(|e| format!("holder could not acquire BEGIN IMMEDIATE: {e}"))?;

    fs::write(gate.join("locked"), b"LOCKED").map_err(|e| format!("locked marker failed: {e}"))?;

    for id in 0..workers {
        wait_for(&gate.join(format!("call-{id}")))?;
    }

    fs::write(gate.join("contended"), b"CONTENDED")
        .map_err(|e| format!("contended marker failed: {e}"))?;

    // 모든 1차 호출이 반환하기 전에는 lock 을 해제하지 않는다. 테스트는
    // 각 반환값이 LockTimeout 인지 검증하므로 call marker 직후 선점된
    // worker 를 실제 경합으로 잘못 세는 빈틈이 없다.
    for id in 0..workers {
        wait_for(&gate.join(format!("completed-{id}")))?;
    }

    connection
        .execute_batch("COMMIT;")
        .map_err(|e| format!("holder commit failed: {e}"))?;

    fs::write(gate.join("released"), b"RELEASED")
        .map_err(|e| format!("released marker failed: {e}"))?;

    println!("HOLDER released_after_completed={workers}");

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let result = match args.first().map(String::as_str) {
        Some("worker") if args.len() == 6 => worker(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[3].parse().expect("worker id"),
            args[4].parse().expect("nonce byte"),
            &args[5],
        ),
        Some("holder") if args.len() == 4 => holder(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[3].parse().expect("worker count"),
        ),
        _ => Err(
            "usage: worker <db> <gate> <id> <nonce-byte> <worker-race|held|held-retry> | holder <db> <gate> <workers>"
                .into(),
        ),
    };

    if let Err(error) = result {
        eprintln!("{error}");
        process::exit(2);
    }
}
