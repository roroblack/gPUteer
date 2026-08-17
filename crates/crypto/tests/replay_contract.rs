//! ★ `ReplayGuard` **계약 적합성** — 두 구현이 같은 답을 내는가.
//!
//! # 왜 이 파일이 있나 (독립 검수 2026-08-17)
//!
//! `InMemoryReplayGuard` 와 `DurableReplayGuard` 는 같은 trait 를 구현한다.
//! 그런데 검수자가 **같은 입력에 다른 답을 내는 경우 4가지**를 찾았다.
//!
//! ```text
//! 15바이트 nonce          영속: Io      메모리: Fresh
//! retain_until = u64::MAX 영속: Io      메모리: Fresh
//! gc(u64::MAX)            영속: Io      메모리: clamp
//! capacity = 0            영속: 생성 실패  메모리: 생성 성공
//! ```
//!
//! ★ **같은 입력에 다른 답을 내는 guard 는 계약이 아니다.**
//!   호출자가 어느 구현을 쓰는지에 따라 replay 방어가 달라진다는 뜻이고,
//!   그것은 "방어가 있다" 고 말할 수 없는 상태다.
//!
//! 각 구현을 따로 시험하면 이 불일치는 **영원히 안 보인다.**
//! 그래서 시나리오를 한 벌 만들고 **두 구현에 똑같이** 먹인다.
//!
//! # 이 파일이 검사하지 않는 것
//!
//! - 영속성 자체 (`durable_replay.rs` 의 재시작 테스트가 본다)
//! - 다중 프로세스 경쟁 (같은 파일 참조)
//! - 성능

use gputeer_crypto::{DurableReplayGuard, InMemoryReplayGuard};
use gputeer_protocol::canonical::Domain;
use gputeer_protocol::constants::{CLOCK_SKEW_TOLERANCE_MS, MAX_SHORTLIVED_TTL_MS};
use gputeer_protocol::signing::{ReplayDecision, ReplayGuard, ReplayStoreError};

const T: u64 = 1_755_200_000_000;

fn n(b: u8) -> Vec<u8> {
    let mut v = vec![0u8; 16];
    v[0] = b;
    v
}

/// 두 구현을 같은 상한으로 만든다.
fn both(
    capacity: usize,
    per_signer: usize,
) -> (InMemoryReplayGuard, DurableReplayGuard, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mem = InMemoryReplayGuard::with_capacities(capacity, per_signer);
    let dur = DurableReplayGuard::open_with_capacities(
        dir.path().join("replay.sqlite3"),
        capacity,
        per_signer,
    )
    .unwrap();
    (mem, dur, dir)
}

/// 오류를 **종류**로만 비교한다 — 메시지 문자열까지 같을 필요는 없다.
fn kind(r: &Result<ReplayDecision, ReplayStoreError>) -> String {
    match r {
        Ok(d) => format!("Ok({d:?})"),
        Err(ReplayStoreError::Io(_)) => "Err(Io)".into(),
        Err(ReplayStoreError::LockTimeout) => "Err(LockTimeout)".into(),
        Err(ReplayStoreError::CacheFull) => "Err(CacheFull)".into(),
        Err(ReplayStoreError::InvalidNonce { len }) => format!("Err(InvalidNonce {len})"),
        Err(ReplayStoreError::SignerQuotaExceeded { quota, .. }) => {
            format!("Err(SignerQuota {quota})")
        }
    }
}

/// 두 구현에 같은 호출을 하고 답이 같은지 본다.
macro_rules! agree {
    ($mem:expr, $dur:expr, $label:expr, |$g:ident| $body:expr) => {{
        let a = {
            let $g = &mut $mem;
            $body
        };
        let b = {
            let $g = &mut $dur;
            $body
        };
        assert_eq!(
            kind(&a),
            kind(&b),
            "★ 두 구현이 다른 답을 냈다 [{}] — 메모리={} 영속={}",
            $label,
            kind(&a),
            kind(&b)
        );
        a
    }};
}

// ══════════════════════════════════════════════════════════════════
// 정상 경로 — 두 구현이 같아야 한다
// ══════════════════════════════════════════════════════════════════

#[test]
fn fresh_then_duplicate_agrees() {
    let (mut m, mut d, _t) = both(10, 10);
    let r = agree!(m, d, "첫 사용", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
    let r = agree!(m, d, "재사용", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Duplicate)));
    // 비공허성 — 다른 nonce 는 통과한다 (무조건 거부가 아니다)
    let r = agree!(m, d, "다른 nonce", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(2),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
}

#[test]
fn namespacing_agrees() {
    let (mut m, mut d, _t) = both(10, 10);
    let r = agree!(m, d, "a/Grant", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
    // 다른 device — 통과해야 한다
    let r = agree!(m, d, "b/Grant 같은 nonce", |g| g.check_and_record(
        "dev-b",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
    // 다른 domain — 통과해야 한다
    let r = agree!(m, d, "a/LeaseRenew 같은 nonce", |g| g.check_and_record(
        "dev-a",
        Domain::LeaseRenew,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
}

// ══════════════════════════════════════════════════════════════════
// ★ 검수자가 찾은 불일치 4건 — 이제 같아야 한다
// ══════════════════════════════════════════════════════════════════

/// 불일치 1 — 15바이트 nonce.
///
/// 메모리 구현에 길이 검사가 **없었다.** 영속 구현만 거부했다.
/// 메모리 guard 를 쓰는 배포에서는 §10 의 nonce 규정이 강제되지 않았다는 뜻이다.
#[test]
fn short_nonce_is_rejected_by_both() {
    let (mut m, mut d, _t) = both(10, 10);
    let r = agree!(m, d, "15바이트 nonce", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &[7u8; 15],
        T + 120_000
    ));
    assert!(
        matches!(r, Err(ReplayStoreError::InvalidNonce { len: 15 })),
        "길이 위반이 {r:?} 로 보고됐다 — 입력 위반과 저장소 장애를 섞으면 안 된다"
    );

    // 잘못된 nonce 가 **자리를 차지하지 않아야** 한다.
    // 차지하면 공격자가 malformed 요청만으로 캐시를 채울 수 있다.
    let r = agree!(m, d, "정상 nonce 는 여전히 통과", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
}

/// 불일치 2 — `retain_until = u64::MAX`.
///
/// 영속 구현이 `i64` 변환 실패로 오류를 냈다. 메모리는 받아들였다.
///
/// ★ `verify()` 는 `MAX_SHORTLIVED_TTL_MS` 로 상한을 걸므로 실전에서는
///   이 값이 오지 않는다. 그러나 guard 는 공개 API 다 —
///   "실전에서는 안 온다" 는 계약이 아니다.
#[test]
fn extreme_retain_until_agrees() {
    let (mut m, mut d, _t) = both(10, 10);
    let r = agree!(m, d, "retain_until = u64::MAX", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        u64::MAX
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
    // 그리고 그 nonce 는 재사용될 수 없어야 한다
    let r = agree!(m, d, "u64::MAX 항목 재사용", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        u64::MAX
    ));
    assert!(matches!(r, Ok(ReplayDecision::Duplicate)));
}

/// 불일치 3 — `gc(u64::MAX)`.
///
/// 영속 구현이 clamp **전에** `i64` 로 바꿔서 오류를 냈다.
/// 메모리 구현은 clock jump 로 보고 잘라냈다.
#[test]
fn extreme_gc_time_agrees() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = InMemoryReplayGuard::with_capacities(10, 10);
    let mut d =
        DurableReplayGuard::open_with_capacities(dir.path().join("r.sqlite3"), 10, 10).unwrap();

    // ★ 보존 시한은 **clamp 상한보다 길어야** 이 시나리오가 성립한다.
    //   처음에 2분(T + 120_000)으로 잡았다가 계약 테스트가 잡아냈다 —
    //   5분 clamp 안에서 2분짜리 항목이 만료되는 것은 **정상**이다.
    //   실제 최대 보존 시한(TTL 상한 15분 + skew 1분)을 쓴다.
    let retain = T + MAX_SHORTLIVED_TTL_MS + CLOCK_SKEW_TOLERANCE_MS;
    for g in [&mut m as &mut dyn ReplayGuard, &mut d as &mut dyn ReplayGuard] {
        g.check_and_record("dev-a", Domain::Grant, &n(1), retain)
            .unwrap();
    }

    // 기준선
    assert_eq!(m.gc(T), 0);
    assert_eq!(d.gc(T).expect("영속 구현이 정상 시각 GC 에서 오류를 냈다"), 0);

    // ★ 극단값 — 둘 다 clamp 해야 하고, 아무것도 지우면 안 된다
    let mem_removed = m.gc(u64::MAX);
    let dur_removed = d
        .gc(u64::MAX)
        .expect("★ 영속 구현이 gc(u64::MAX) 를 오류로 처리했다 — 메모리 구현은 clamp 한다");
    assert_eq!(
        (mem_removed, dur_removed),
        (0, 0),
        "★ 극단적 미래 시각 한 번으로 캐시가 비었다"
    );

    // 그 nonce 는 여전히 막혀야 한다 — replay 창이 열리지 않았다
    for (label, r) in [
        ("메모리", m.check_and_record("dev-a", Domain::Grant, &n(1), retain)),
        ("영속", d.check_and_record("dev-a", Domain::Grant, &n(1), retain)),
    ] {
        assert!(
            matches!(r, Ok(ReplayDecision::Duplicate)),
            "★ {label} 구현에서 replay 창이 열렸다: {r:?}"
        );
    }
}

/// 불일치 4 — `capacity = 0`.
///
/// 영속 구현은 생성 자체를 거부했고 메모리 구현은 성공했다.
#[test]
fn zero_capacity_is_rejected_by_both() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        DurableReplayGuard::open_with_capacities(dir.path().join("r.sqlite3"), 0, 10).is_err(),
        "영속 구현이 상한 0을 받아들였다"
    );
    assert!(
        InMemoryReplayGuard::try_with_capacities(0, 10).is_err(),
        "★ 메모리 구현이 상한 0을 받아들였다 — 영속 구현은 거부한다"
    );

    // 비공허성 — 정상 상한은 둘 다 만들어진다
    assert!(InMemoryReplayGuard::try_with_capacities(10, 10).is_ok());
    assert!(
        DurableReplayGuard::open_with_capacities(dir.path().join("ok.sqlite3"), 10, 10).is_ok()
    );
}

// ══════════════════════════════════════════════════════════════════
// 상한 동작 — 두 구현이 같아야 한다
// ══════════════════════════════════════════════════════════════════

#[test]
fn signer_quota_agrees() {
    let (mut m, mut d, _t) = both(10, 2);
    let r = agree!(m, d, "quota 1/2", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
    let r = agree!(m, d, "quota 2/2", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(2),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
    let r = agree!(m, d, "quota 초과", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(3),
        T + 120_000
    ));
    assert!(matches!(r, Err(ReplayStoreError::SignerQuotaExceeded { .. })));

    // 다른 서명자는 영향받지 않는다
    let r = agree!(m, d, "다른 서명자", |g| g.check_and_record(
        "dev-b",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
}

#[test]
fn cache_full_agrees_and_never_evicts() {
    let (mut m, mut d, _t) = both(2, 10);
    let r = agree!(m, d, "1/2", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
    let r = agree!(m, d, "2/2", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(2),
        T + 120_000
    ));
    assert!(matches!(r, Ok(ReplayDecision::Fresh)));
    let r = agree!(m, d, "상한 초과", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(3),
        T + 120_000
    ));
    assert!(matches!(r, Err(ReplayStoreError::CacheFull)));

    // ★ §10 — 거부하면서 기존 미만료 항목을 축출하지 않았는가
    let r = agree!(m, d, "기존 항목 생존", |g| g.check_and_record(
        "dev-a",
        Domain::Grant,
        &n(1),
        T + 120_000
    ));
    assert!(
        matches!(r, Ok(ReplayDecision::Duplicate)),
        "★ 상한 도달 시 기존 유효 nonce 가 축출됐다 — replay 창이 열렸다"
    );
}

// ══════════════════════════════════════════════════════════════════
// GC — 두 구현이 같아야 한다
// ══════════════════════════════════════════════════════════════════

#[test]
fn gc_expiry_and_rollback_agree() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = InMemoryReplayGuard::with_capacities(10, 10);
    let mut d =
        DurableReplayGuard::open_with_capacities(dir.path().join("r.sqlite3"), 10, 10).unwrap();

    for g in [&mut m as &mut dyn ReplayGuard, &mut d as &mut dyn ReplayGuard] {
        g.check_and_record("dev-a", Domain::Grant, &n(1), T + 1_000)
            .unwrap();
        g.check_and_record("dev-a", Domain::Grant, &n(2), T + 100_000)
            .unwrap();
    }

    assert_eq!((m.gc(T), d.gc(T).unwrap()), (0, 0), "만료 전인데 지웠다");
    assert_eq!(
        (m.gc(T + 2_000), d.gc(T + 2_000).unwrap()),
        (1, 1),
        "만료된 항목 하나만 지워야 한다"
    );

    // 되감김 — 둘 다 아무것도 지우지 않는다
    assert_eq!(
        (m.gc(T + 1_000), d.gc(T + 1_000).unwrap()),
        (0, 0),
        "★ 시계 되감김에서 항목을 지웠다 — replay 창이 열린다"
    );
}
