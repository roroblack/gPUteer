//! ★ **실제 동시 접근** — 순차 호출이 아니다.
//!
//! # 왜 이 파일이 따로 있나
//!
//! `durable_replay.rs` 의 `two_connections_share_state_sequentially` 는
//! 두 연결을 만들지만 **호출은 순차적**이다. 독립 검수(2026-08-17)가
//! "race 테스트가 아니다" 라고 지적했고 맞는 지적이었다.
//!
//! ```text
//! 순차 호출이 보는 것    두 연결이 같은 파일 상태를 공유하는가
//! 이 파일이 보는 것      실제로 동시에 두드릴 때도 그런가
//! ```
//!
//! `DoD-10` limitations 에 "다중 프로세스 동시 접근을 실측하지 않았다.
//! SQLite 가 직렬화한다는 것은 **문헌 지식이지 이 환경의 측정이 아니다**" 라고
//! 적었다. 이 파일이 그것을 측정한다.
//!
//! # 방법
//!
//! `DurableReplayGuard` 는 `rusqlite::Connection` 을 갖는데 그것은 `Send` 지만
//! `Sync` 가 아니다. 그래서 **스레드마다 자기 연결을 연다** —
//! 그것이 오히려 실제 다중 프로세스에 가깝다(각자 자기 핸들을 갖는다).
//!
//! # ★ 이 파일이 측정하지 **않는** 것
//!
//! - 진짜 별도 **프로세스** (스레드다. SQLite 락은 파일 단위이므로
//!   프로세스 경계를 넘어도 같은 메커니즘이지만, 그것을 측정한 것은 아니다)
//! - 전원 차단 중의 경합
//! - Linux (D-3)

use std::sync::{Arc, Barrier};
use std::thread;

use gputeer_crypto::DurableReplayGuard;
use gputeer_protocol::canonical::Domain;
use gputeer_protocol::signing::{ReplayDecision, ReplayGuard, ReplayStoreError};

const T: u64 = 1_755_200_000_000;
const RETAIN: u64 = T + 900_000;

fn nonce(a: u8, b: u8) -> Vec<u8> {
    let mut v = vec![0u8; 16];
    v[0] = a;
    v[1] = b;
    v
}

/// 결과를 세는 상자. 오류를 **버리지 않는다** (`CLAUDE.md` §3).
#[derive(Debug, Default)]
struct Tally {
    fresh: usize,
    duplicate: usize,
    lock_timeout: usize,
    other_err: Vec<String>,
}

impl Tally {
    fn add(&mut self, r: Result<ReplayDecision, ReplayStoreError>) {
        match r {
            Ok(ReplayDecision::Fresh) => self.fresh += 1,
            Ok(ReplayDecision::Duplicate) => self.duplicate += 1,
            Err(ReplayStoreError::LockTimeout) => self.lock_timeout += 1,
            Err(e) => self.other_err.push(format!("{e:?}")),
        }
    }

    fn merge(&mut self, o: Tally) {
        self.fresh += o.fresh;
        self.duplicate += o.duplicate;
        self.lock_timeout += o.lock_timeout;
        self.other_err.extend(o.other_err);
    }
}

// ══════════════════════════════════════════════════════════════════
// ★ 같은 nonce 를 여러 스레드가 동시에 두드린다
// ══════════════════════════════════════════════════════════════════

/// N 개 스레드가 **같은 순간에** 같은 nonce 를 기록하려 한다.
///
/// 정확히 하나만 `Fresh` 여야 한다. 둘 이상이면 **replay 방어가 없는 것**이다.
///
/// ★ `LockTimeout` 은 실패가 아니다 — 거부이므로 안전한 방향이다.
///   그러나 `Fresh` 가 2개 이상 나오면 그것은 결함이다.
#[test]
fn concurrent_same_nonce_yields_exactly_one_fresh() {
    const THREADS: usize = 8;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("replay.sqlite3");
    // 먼저 스키마를 만든다 — 생성 자체가 경합하면 무엇을 재는지 흐려진다.
    drop(DurableReplayGuard::open(&path).unwrap());

    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::new();

    for _ in 0..THREADS {
        let p = path.clone();
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            // ★ 스레드마다 **자기 연결**을 연다 — 다중 프로세스에 가깝다.
            let mut g = DurableReplayGuard::open(&p).expect("연결 실패");
            let mut t = Tally::default();
            b.wait(); // 같은 순간에 출발한다
            t.add(g.check_and_record("dev-a", Domain::Grant, &nonce(1, 0), RETAIN));
            t
        }));
    }

    let mut total = Tally::default();
    for h in handles {
        total.merge(h.join().expect("스레드가 패닉했다"));
    }

    assert!(
        total.other_err.is_empty(),
        "예상하지 못한 오류가 났다: {:?}",
        total.other_err
    );
    assert_eq!(
        total.fresh, 1,
        "★ 같은 nonce 에 Fresh 가 {}개 나왔다 — 동시 접근에서 replay 방어가 없다 \
         (duplicate={}, lock_timeout={})",
        total.fresh, total.duplicate, total.lock_timeout
    );
    assert_eq!(
        total.fresh + total.duplicate + total.lock_timeout,
        THREADS,
        "세지 않은 결과가 있다 — 분모가 줄면 성공률이 실제보다 좋아 보인다"
    );

    // 사후 상태도 하나여야 한다
    let g = DurableReplayGuard::open(&path).unwrap();
    assert_eq!(g.entry_count().unwrap(), 1, "항목이 중복 기록됐다");
}

// ══════════════════════════════════════════════════════════════════
// 비공허성 — 서로 다른 nonce 는 전부 통과해야 한다
// ══════════════════════════════════════════════════════════════════

/// 위 테스트가 "동시에 부르면 무조건 하나만 통과" 로 통과하는 것이
/// 아님을 보인다. 서로 다른 nonce 면 전부 `Fresh` 여야 한다.
#[test]
fn concurrent_distinct_nonces_all_succeed() {
    const THREADS: usize = 8;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("replay.sqlite3");
    drop(DurableReplayGuard::open(&path).unwrap());

    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::new();

    for i in 0..THREADS {
        let p = path.clone();
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let mut g = DurableReplayGuard::open(&p).expect("연결 실패");
            let mut t = Tally::default();
            b.wait();
            t.add(g.check_and_record("dev-a", Domain::Grant, &nonce(2, i as u8), RETAIN));
            t
        }));
    }

    let mut total = Tally::default();
    for h in handles {
        total.merge(h.join().expect("스레드가 패닉했다"));
    }

    assert!(total.other_err.is_empty(), "오류: {:?}", total.other_err);
    assert_eq!(
        total.duplicate, 0,
        "서로 다른 nonce 인데 Duplicate 이 나왔다"
    );
    assert_eq!(
        total.fresh + total.lock_timeout,
        THREADS,
        "세지 않은 결과가 있다"
    );

    // ★ LockTimeout 은 **안전한 실패**지만 공짜가 아니다.
    //   전부 timeout 이면 이 저장소는 동시 부하에서 쓸 수 없다는 뜻이다.
    //   그 사실이 숫자로 보이게 남긴다.
    assert!(
        total.fresh > 0,
        "★ 동시 요청 {THREADS}건이 전부 LockTimeout 이다 — \
         busy_timeout(1초)이 이 환경에 너무 짧다"
    );

    let g = DurableReplayGuard::open(&path).unwrap();
    assert_eq!(
        g.entry_count().unwrap(),
        total.fresh,
        "Fresh 로 답한 수와 실제 저장된 수가 다르다 — commit 뒤에만 Fresh 라는 계약이 깨졌다"
    );
}

// ══════════════════════════════════════════════════════════════════
// GC 와 기록이 동시에
// ══════════════════════════════════════════════════════════════════

/// 한 스레드가 GC 를 도는 동안 다른 스레드가 기록한다.
///
/// ★ **미만료 항목이 사라지면 안 된다** (§10 MUST NOT).
///   GC 트랜잭션과 기록 트랜잭션이 겹칠 때 그 불변식이 깨지는지 본다.
#[test]
fn concurrent_gc_never_drops_unexpired_entries() {
    const WRITERS: usize = 4;
    const ROUNDS: usize = 25;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("replay.sqlite3");
    drop(DurableReplayGuard::open(&path).unwrap());

    let barrier = Arc::new(Barrier::new(WRITERS + 1));
    let mut handles = Vec::new();

    for w in 0..WRITERS {
        let p = path.clone();
        let b = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let mut g = DurableReplayGuard::open(&p).expect("연결 실패");
            let mut t = Tally::default();
            b.wait();
            for r in 0..ROUNDS {
                t.add(g.check_and_record(
                    "dev-a",
                    Domain::Grant,
                    &nonce(3 + w as u8, r as u8),
                    RETAIN, // 전부 미만료
                ));
            }
            t
        }));
    }

    // GC 스레드 — 만료된 것이 하나도 없는 시각으로 계속 돈다
    let p = path.clone();
    let b = Arc::clone(&barrier);
    let gc = thread::spawn(move || {
        let mut g = DurableReplayGuard::open(&p).expect("연결 실패");
        let mut removed_total = 0usize;
        let mut lock_timeouts = 0usize;
        b.wait();
        for _ in 0..ROUNDS {
            match g.gc(T) {
                Ok(n) => removed_total += n,
                Err(ReplayStoreError::LockTimeout) => lock_timeouts += 1,
                Err(e) => panic!("GC 가 예상치 못한 오류를 냈다: {e:?}"),
            }
        }
        (removed_total, lock_timeouts)
    });

    let mut total = Tally::default();
    for h in handles {
        total.merge(h.join().expect("writer 스레드가 패닉했다"));
    }
    let (gc_removed, gc_timeouts) = gc.join().expect("GC 스레드가 패닉했다");

    assert!(total.other_err.is_empty(), "오류: {:?}", total.other_err);
    assert_eq!(
        gc_removed, 0,
        "★ GC 가 미만료 항목을 {gc_removed}개 지웠다 — §10 MUST NOT 위반이다 \
         (replay 창이 열린다)"
    );

    // Fresh 로 답한 것은 전부 남아 있어야 한다
    let g = DurableReplayGuard::open(&path).unwrap();
    assert_eq!(
        g.entry_count().unwrap(),
        total.fresh,
        "★ Fresh 로 답한 {}건 중 {}건만 남았다 — 동시 GC 가 유효 nonce 를 지웠다 \
         (gc_lock_timeout={gc_timeouts})",
        total.fresh,
        g.entry_count().unwrap()
    );
    assert!(
        total.fresh > 0,
        "기록이 하나도 성공하지 않았다 — 이 테스트는 아무것도 검사하지 못했다"
    );
}
