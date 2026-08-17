//! replay 캐시 — `signing.md` §10.
//!
//! # 왜 여기(Crypto 스트림)에 있는가
//!
//! `RULE.md` §4.1 — nonce 캐시는 Crypto 스트림 소유다.
//! `crates/protocol` 은 [`ReplayGuard`] **trait 만** 정의한다(§4.3).
//!
//! # 구현 순서 (독립 검수 설계 검토 2026-08-16)
//!
//! ```text
//! 1. 계약 · API 수정      ✅ nonce 결속 · Result API · retain_until
//! 2. 메모리 참조 구현     ← 이 파일
//! 3. 영속 저장소          ⬜ 미구현
//! 4. GC · 시각 정책       🟡 부분 (아래 clock rollback 참조)
//! 5. 부작용 경로 연결     ⬜ 소비 측이 없다
//! ```
//!
//! ★ **[`InMemoryReplayGuard`] 는 영속되지 않는다.**
//! 프로세스가 재시작하면 캐시가 비고, **재시작 직후 replay 창이 열린다.**
//! 그 사실을 이름이 아니라 [`InMemoryReplayGuard::is_durable`] 로도 드러낸다.

use std::collections::HashMap;

use gputeer_protocol::canonical::Domain;
use gputeer_protocol::signing::{ReplayDecision, ReplayGuard, ReplayStoreError};

/// §10 — 기본 상한. 도달하면 **축출이 아니라 거부**한다.
///
/// v5 초안은 "오래된 것부터 제거" 였다.
/// **아직 유효한 nonce 가 밀려나면 replay 창이 열린다.**
pub const DEFAULT_CAPACITY: usize = 100_000;

type Key = (String, u32, Vec<u8>);

/// 메모리 replay 캐시 — **참조 구현이다. 영속되지 않는다.**
///
/// # 무엇을 보장하는가
///
/// ```text
/// 같은 (device, domain, nonce) 재사용    거부한다
/// 미만료 항목 축출                        하지 않는다 (§10 MUST NOT)
/// 상한 도달                               거부한다 (축출하지 않는다)
/// 만료 항목 GC                            retain_until 기준으로만
/// ```
///
/// # 무엇을 보장하지 **않는가**
///
/// ```text
/// 프로세스 재시작        ★ 캐시가 비어 replay 창이 열린다
/// 다중 프로세스          공유되지 않는다
/// 전원 차단              동상
/// ```
///
/// 영속 저장소가 생기기 전까지 **이것으로 운영하면 안 된다.**
/// [`Self::is_durable`] 이 `false` 를 반환하는 것이 그 신호다.
#[derive(Debug)]
pub struct InMemoryReplayGuard {
    /// key -> retain_until_ms
    seen: HashMap<Key, u64>,
    capacity: usize,
    /// ★ 마지막으로 본 시각. **시계 되감김 감지용.**
    ///
    /// 시계가 앞으로 튄 뒤 GC 로 항목을 지우고 다시 뒤로 돌아오면
    /// **지워진 nonce 를 재사용할 수 있다.** 되감김을 보면 GC 를 멈춘다.
    last_seen_ms: u64,
    /// 되감김을 감지한 횟수. 운영 신호로 쓴다.
    clock_rollbacks: u64,
}

impl Default for InMemoryReplayGuard {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl InMemoryReplayGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            seen: HashMap::new(),
            capacity,
            last_seen_ms: 0,
            clock_rollbacks: 0,
        }
    }

    /// ★ 이 guard 가 **재시작을 견디는가.**
    ///
    /// 메모리 구현은 `false` 다. 영속 저장소가 생기면 `true` 를 반환하는
    /// 구현으로 교체한다. 호출자가 "재시작 후 replay 창" 을 알 수 있어야 한다.
    pub fn is_durable(&self) -> bool {
        false
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// 시계 되감김을 몇 번 봤는가. 0이 아니면 운영 신호다.
    pub fn clock_rollbacks(&self) -> u64 {
        self.clock_rollbacks
    }

    /// 만료 항목을 지운다 (§10 — 1분 주기 GC).
    ///
    /// ★ **`now` 가 뒤로 갔으면 아무것도 지우지 않는다.**
    ///
    /// 시계가 앞으로 튄 뒤 GC 로 항목을 지우고 다시 뒤로 돌아오면
    /// 지워진 nonce 가 "처음 보는 것" 이 되어 **replay 창이 열린다.**
    /// 되감김 중에는 지우지 않는 것이 안전한 방향이다 —
    /// 캐시가 커지는 것은 거부(`CacheFull`)로 드러나지만,
    /// 지워진 nonce 는 **조용히** 통과한다.
    ///
    /// 반환값: 지운 개수.
    pub fn gc(&mut self, now_unix_ms: u64) -> usize {
        if now_unix_ms < self.last_seen_ms {
            self.clock_rollbacks += 1;
            return 0;
        }
        self.last_seen_ms = now_unix_ms;
        let before = self.seen.len();
        self.seen.retain(|_, retain_until| *retain_until > now_unix_ms);
        before - self.seen.len()
    }
}

impl ReplayGuard for InMemoryReplayGuard {
    fn check_and_record(
        &mut self,
        signer_id: &str,
        domain: Domain,
        nonce: &[u8],
        retain_until_ms: u64,
    ) -> Result<ReplayDecision, ReplayStoreError> {
        // §10 — 키는 (sender_device_id, domain_tag, nonce).
        // device 별 namespace 가 없으면 한 device 가 다른 device 의
        // nonce 공간을 소진시킬 수 있다.
        let key: Key = (signer_id.to_string(), domain as u32, nonce.to_vec());

        if self.seen.contains_key(&key) {
            return Ok(ReplayDecision::Duplicate);
        }

        // ★ 상한 도달 시 **축출하지 않는다.** 거부가 안전한 실패 방향이다(§10).
        //   미만료 nonce 를 밀어내면 replay 창이 열린다.
        if self.seen.len() >= self.capacity {
            return Err(ReplayStoreError::CacheFull);
        }

        self.seen.insert(key, retain_until_ms);
        Ok(ReplayDecision::Fresh)
    }

    fn is_effective(&self) -> bool {
        true
    }
}
