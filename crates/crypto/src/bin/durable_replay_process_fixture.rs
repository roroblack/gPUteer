//! `DurableReplayGuard` 별도 프로세스 경쟁 테스트용 fixture.
//!
//! `crates/crypto/tests/durable_replay_process.rs` 가 이 바이너리를
//! 여러 개 별도 OS 프로세스로 띄워 같은 SQLite replay DB 에 동시에
//! 접근시킨다. 스레드 안에서만 측정하던 기존
//! `crates/crypto/tests/durable_replay_race.rs` 와 달리, 이 fixture
//! 는 **진짜 프로세스 경계**를 넘는 SQLite 잠금 경합을 만든다.
//!
//! 두 모드:
//!   worker <db> <gate> <id> <nonce-byte>   DurableReplayGuard 로 nonce 기록 시도
//!   holder <db> <gate> <workers> <timeout-mode>
//!                                          별도 연결로 BEGIN IMMEDIATE 락을 쥐고 있는다
//!
//! `holder` 는 `DurableReplayGuard` 의 공개 API 를 우회하는 것이
//! 아니다 — "트랜잭션을 일정 시간 쥐고 있는" 테스트 전용 기능이
//! 그 API 에 없기 때문에, 별도 rusqlite 연결로 같은 파일에 락을
//! 걸어 경합을 강제한다.

use std::{
    env, fs,
    path::Path,
    process,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use gputeer_crypto::DurableReplayGuard;
use gputeer_protocol::{
    canonical::Domain,
    signing::{ReplayDecision, ReplayGuard, ReplayStoreError},
};
use rusqlite::Connection;

const RETAIN_UNTIL_MS: u64 = 4_000_000_000_000;

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos()
}

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

fn worker(database: &Path, gate: &Path, id: usize, nonce_second_byte: u8) -> Result<(), String> {
    let mut guard =
        DurableReplayGuard::open(database).map_err(|e| format!("worker {id} open failed: {e:?}"))?;

    fs::write(gate.join(format!("ready-{id}")), b"READY")
        .map_err(|e| format!("worker {id} ready marker failed: {e}"))?;

    wait_for(&gate.join("start"))?;

    let mut nonce = [0u8; 16];
    nonce[0] = 0xa5;
    nonce[1] = nonce_second_byte;

    let start_ns = now_ns();

    // holder 가 아직 SQLite write lock 을 쥐고 있는 동안(즉 이 call
    // marker 를 전부 확인하기 전까지는 commit 하지 않는다) 이 marker
    // 를 남긴다 — "모든 worker 가 실제로 lock 보유 구간에 도달했다"
    // 는 비공허성 증거다.
    fs::write(gate.join(format!("call-{id}")), b"CALL_BEGIN")
        .map_err(|e| format!("worker {id} call marker failed: {e}"))?;

    let result = guard.check_and_record("process-race-signer", Domain::Grant, &nonce, RETAIN_UNTIL_MS);

    let end_ns = now_ns();

    let outcome = match &result {
        Ok(ReplayDecision::Fresh) => "Fresh",
        Ok(ReplayDecision::Duplicate) => "Duplicate",
        Err(ReplayStoreError::LockTimeout) => "LockTimeout",
        Err(_) => "Other",
    };

    println!(
        "RESULT id={id} pid={} outcome={outcome} start_ns={start_ns} end_ns={end_ns} result={result:?}",
        process::id()
    );

    Ok(())
}

fn holder(database: &Path, gate: &Path, workers: usize, timeout_mode: bool) -> Result<(), String> {
    let connection = Connection::open(database).map_err(|e| format!("holder open failed: {e}"))?;

    connection
        .execute_batch("BEGIN IMMEDIATE;")
        .map_err(|e| format!("holder could not acquire BEGIN IMMEDIATE: {e}"))?;

    let locked_ns = now_ns();
    fs::write(gate.join("locked"), b"LOCKED").map_err(|e| format!("locked marker failed: {e}"))?;

    for id in 0..workers {
        wait_for(&gate.join(format!("call-{id}")))?;
    }

    let contended_ns = now_ns();
    fs::write(gate.join("contended"), b"CONTENDED")
        .map_err(|e| format!("contended marker failed: {e}"))?;

    if timeout_mode {
        // DurableReplayGuard 의 busy_timeout 은 1초다(durable_replay.rs
        // BUSY_TIMEOUT). 1300ms 동안 lock 을 쥐고 있으면 worker 는
        // 반드시 LockTimeout 을 받아야 한다.
        thread::sleep(Duration::from_millis(1_300));
    } else {
        wait_for(&gate.join("release"))?;
    }

    connection
        .execute_batch("COMMIT;")
        .map_err(|e| format!("holder commit failed: {e}"))?;

    let released_ns = now_ns();
    fs::write(gate.join("released"), b"RELEASED")
        .map_err(|e| format!("released marker failed: {e}"))?;

    println!("HOLDER locked_ns={locked_ns} contended_ns={contended_ns} released_ns={released_ns}");

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let result = match args.first().map(String::as_str) {
        Some("worker") if args.len() == 5 => worker(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[3].parse().expect("worker id"),
            args[4].parse().expect("nonce byte"),
        ),
        Some("holder") if args.len() == 5 => holder(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args[3].parse().expect("worker count"),
            args[4].parse::<u8>().expect("timeout mode") != 0,
        ),
        _ => Err(
            "usage: worker <db> <gate> <id> <nonce-byte> | holder <db> <gate> <workers> <timeout-mode>"
                .into(),
        ),
    };

    if let Err(error) = result {
        eprintln!("{error}");
        process::exit(2);
    }
}
