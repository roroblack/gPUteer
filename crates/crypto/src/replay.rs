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
use gputeer_protocol::signing::{ReplayDecision, ReplayGuard, ReplayStoreError, NONCE_LEN};

/// §10 — 기본 상한. 도달하면 **축출이 아니라 거부**한다.
///
/// v5 초안은 "오래된 것부터 제거" 였다.
/// **아직 유효한 nonce 가 밀려나면 replay 창이 열린다.**
pub const DEFAULT_CAPACITY: usize = 100_000;

/// ★ **한 서명자가 쓸 수 있는 몫** (독립 검수 2026-08-17).
///
/// 전역 상한만 있으면 한 device 가 nonce 를 상한만큼 만들어
/// **다른 모든 device 를 차단**할 수 있다.
/// 그것은 replay 방어가 아니라 DoS 통로다.
///
/// 기본값은 전역 상한의 1/10 이다. 근거는 측정이 아니라 정책이다 —
/// "한 device 가 전체의 10% 를 넘게 쓰면 비정상" 이라는 판단이다.
/// 팀 규모가 10명 미만이면 이 값이 너무 빡빡할 수 있다. 조정 가능하다.
pub const DEFAULT_PER_SIGNER_CAPACITY: usize = DEFAULT_CAPACITY / 10;

/// ★ GC 가 한 번에 앞으로 갈 수 있는 최대 시간 (독립 검수 2026-08-17).
///
/// # 왜 필요한가
///
/// 되감김만 막는 것으로는 부족하다. **앞으로 튀는** 시각도 위험하다.
///
/// ```text
/// 1. (A, Grant, n1) 을 retain_until = 20_000 으로 기록
/// 2. GC 가 잘못된 미래 시각 1_000_000 으로 호출된다
/// 3. retain_until <= now 이므로 n1 이 지워진다
/// 4. 시각이 10_000 으로 돌아오면 같은 메시지가 Fresh 가 된다
/// ```
///
/// # 값의 근거 — **최대 보존 시한보다 짧아야 한다**
///
/// ★ 처음에 1시간으로 잡았다가 **테스트가 잡아냈다.**
///
/// 단수명 메시지의 보존 시한은 최대
/// `MAX_SHORTLIVED_TTL_MS`(15분) + `CLOCK_SKEW_TOLERANCE_MS`(1분) = 16분이다.
/// 상한이 1시간이면 한 번의 잘못된 GC 가 **여전히 캐시 전체를 지운다.**
/// 자르는 시늉만 하고 아무것도 막지 못한 것이다.
///
/// 5분으로 잡으면 잘못된 GC 한 번이 지울 수 있는 것은
/// "5분 안에 어차피 만료될 항목" 뿐이다.
///
/// # ★ 이것이 막지 못하는 것
///
/// **지속적으로 틀린 시계**는 막지 못한다.
/// 잘못된 GC 를 4번 연달아 부르면 `last_seen_ms` 가 5분씩 전진해
/// 결국 캐시가 빈다. 이 상한은 **한 번의 잘못된 읽기**를 묶을 뿐이다.
///
/// 제대로 막으려면 단조 시계(monotonic clock)를 함께 써야 한다.
/// 그것은 영속 저장소와 함께 할 일이다 (§10 3단계, 미구현).
///
/// # 정상 동작에 미치는 영향
///
/// 오래 쉰 프로세스는 GC 한 번에 5분씩만 전진한다.
/// §10 이 정한 1분 주기 GC 라면 1시간 공백도 12분 안에 따라잡는다.
pub const MAX_GC_ADVANCE_MS: u64 = 5 * 60 * 1_000;

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
    /// ★ 서명자별 사용량. 한 device 가 전체를 삼키지 못하게 한다.
    per_signer: HashMap<String, usize>,
    per_signer_capacity: usize,
    /// ★ 마지막으로 본 시각. **시계 되감김 감지용.**
    ///
    /// 시계가 앞으로 튄 뒤 GC 로 항목을 지우고 다시 뒤로 돌아오면
    /// **지워진 nonce 를 재사용할 수 있다.** 되감김을 보면 GC 를 멈춘다.
    last_seen_ms: u64,
    /// 되감김을 감지한 횟수. 운영 신호로 쓴다.
    clock_rollbacks: u64,
    /// ★ 앞으로 과도하게 튄 것을 잘라낸 횟수. 운영 신호로 쓴다.
    clock_jumps: u64,
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
        // 서명자 몫도 전역 상한에 비례해 줄인다.
        // 그러지 않으면 `with_capacity(3)` 같은 테스트에서
        // 서명자 몫(10_000)이 전역 상한보다 커져 아무 의미가 없어진다.
        let per = (capacity / 10).max(1);
        Self::with_capacities(capacity, per)
    }

    /// 전역 상한과 서명자별 상한을 따로 정한다.
    /// ★ 상한이 0이면 **거부한다** (독립 검수 2026-08-17).
    ///
    /// 영속 구현은 `open_with_capacities` 에서 0을 오류로 거부한다.
    /// 메모리 구현만 조용히 받아들이면 **두 구현이 다른 계약**을 갖는다.
    /// 같은 입력에 다른 답을 내는 guard 는 계약이 아니다.
    pub fn try_with_capacities(
        capacity: usize,
        per_signer_capacity: usize,
    ) -> Result<Self, ReplayStoreError> {
        if capacity == 0 || per_signer_capacity == 0 {
            return Err(ReplayStoreError::Io(
                "replay 캐시 상한은 1 이상이어야 한다".into(),
            ));
        }
        Ok(Self::with_capacities(capacity, per_signer_capacity))
    }

    /// # Panics
    ///
    /// 상한이 0이면 패닉한다. 프로그래밍 오류이므로 조용히 넘기지 않는다.
    /// 오류로 다루려면 [`Self::try_with_capacities`] 를 쓴다.
    pub fn with_capacities(capacity: usize, per_signer_capacity: usize) -> Self {
        assert!(
            capacity > 0 && per_signer_capacity > 0,
            "replay 캐시 상한은 1 이상이어야 한다 (capacity={capacity}, per_signer={per_signer_capacity})"
        );
        Self {
            seen: HashMap::new(),
            capacity,
            per_signer: HashMap::new(),
            per_signer_capacity,
            last_seen_ms: 0,
            clock_rollbacks: 0,
            clock_jumps: 0,
        }
    }

    /// 서명자 한 명의 몫.
    pub fn per_signer_capacity(&self) -> usize {
        self.per_signer_capacity
    }

    /// 이 서명자가 지금 몇 개를 쓰고 있는가.
    pub fn signer_usage(&self, signer_id: &str) -> usize {
        self.per_signer.get(signer_id).copied().unwrap_or(0)
    }

    /// 시각이 앞으로 과도하게 튀어 잘라낸 횟수. 0이 아니면 운영 신호다.
    pub fn clock_jumps(&self) -> u64 {
        self.clock_jumps
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

        // ★ 앞으로 튀는 것도 막는다 (독립 검수 2026-08-17).
        //   되감김만 막으면, 한 번의 엉뚱한 미래 시각으로 캐시를 통째로
        //   비운 뒤 시각이 돌아왔을 때 replay 창이 열린다.
        //
        //   첫 호출(last_seen_ms == 0)은 기준선이 없으므로 자르지 않는다.
        //   자르면 실제 unix 시각(약 1.7e12)이 항상 상한을 넘어 GC 가 영영 안 돈다.
        let effective = if self.last_seen_ms == 0 {
            now_unix_ms
        } else {
            let bound = self.last_seen_ms.saturating_add(MAX_GC_ADVANCE_MS);
            if now_unix_ms > bound {
                self.clock_jumps += 1;
                bound
            } else {
                now_unix_ms
            }
        };

        self.last_seen_ms = effective;
        let before = self.seen.len();
        let mut removed: Vec<String> = Vec::new();
        self.seen.retain(|k, retain_until| {
            let keep = *retain_until > effective;
            if !keep {
                removed.push(k.0.clone());
            }
            keep
        });
        // 서명자별 사용량도 함께 줄인다.
        // 이것을 빠뜨리면 만료 뒤에도 그 서명자가 영영 막힌다.
        for signer in removed {
            if let Some(n) = self.per_signer.get_mut(&signer) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    self.per_signer.remove(&signer);
                }
            }
        }
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
        // ★ §10 — nonce 는 CSPRNG 16바이트여야 한다 (독립 검수 2026-08-17).
        //   전에는 메모리 구현만 이 검사가 **없었다.**
        //   15바이트 nonce 를 영속 구현은 거부하고 메모리 구현은 Fresh 로 받았다.
        //   같은 입력에 다른 답을 내면 그것은 계약이 아니다.
        if nonce.len() != NONCE_LEN {
            return Err(ReplayStoreError::InvalidNonce { len: nonce.len() });
        }

        // §10 — 키는 (sender_device_id, domain_tag, nonce).
        // device 별 namespace 가 없으면 한 device 가 다른 device 의
        // nonce 공간을 소진시킬 수 있다.
        let key: Key = (signer_id.to_string(), domain as u32, nonce.to_vec());

        if self.seen.contains_key(&key) {
            return Ok(ReplayDecision::Duplicate);
        }

        // ★ 서명자 몫을 **전역 상한보다 먼저** 본다 (독립 검수 2026-08-17).
        //   순서가 반대면, 한 서명자가 캐시를 채운 뒤에는 모든 요청이
        //   CacheFull 로 보고되어 **누가 원인인지 알 수 없다.**
        let used = self.per_signer.get(signer_id).copied().unwrap_or(0);
        if used >= self.per_signer_capacity {
            return Err(ReplayStoreError::SignerQuotaExceeded {
                signer_id: signer_id.to_string(),
                quota: self.per_signer_capacity,
            });
        }

        // ★ 상한 도달 시 **축출하지 않는다.** 거부가 안전한 실패 방향이다(§10).
        //   미만료 nonce 를 밀어내면 replay 창이 열린다.
        if self.seen.len() >= self.capacity {
            return Err(ReplayStoreError::CacheFull);
        }

        self.seen.insert(key, retain_until_ms);
        *self.per_signer.entry(signer_id.to_string()).or_insert(0) += 1;
        Ok(ReplayDecision::Fresh)
    }

    fn is_effective(&self) -> bool {
        true
    }
}
