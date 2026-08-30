//! `ADR-033` §8 의 여섯 조건을 **타입으로 요구하는 관문**.
//!
//! `liveness.rs` 가 남긴 문장을 이행한다 —
//!
//! > 재배정을 만들 때는 `ADR-033` §8 의 여섯 조건을 타입으로 요구하는
//! > 별도 관문이 필요하다.
//!
//! # 무엇을 판정하는가
//!
//! Broker 에 연락이 닿지 않는 동안, 이미 배치된 작업을 **다른 기계로
//! 옮겨도 되는가**. `ADR-033` §1 은 배치 권한을 Broker 하나로 고정했고
//! §8 은 그 예외를 딱 하나 열었다 — 여섯 조건을 **전부** 충족할 때만.
//!
//! ```text
//! 1  Broker 무응답이 풀 정책이 정한 시간을 넘겼다
//! 2  원래 Lease 의 유효기간이 이미 지났다              <- 핵심 조건
//! 3  이웃 N 대가 "나도 그 노드에 연락하지 못한다" 고 서명했다 (§7)
//! 4  작업을 받을 기계가 승인했다
//! 5  쓰는 fence 번호가 Broker 에게 미리 받아 둔 예비 범위 안이다
//! 6  이 재할당이 감사 로그에 기록된다 (§4)
//! ```
//!
//! # ★ 오늘 **정직한 호출은** 아무것도 허용받지 못한다 — 그게 설계다
//!
//! (초안은 "이 커널은 아무것도 허용하지 못한다" 고 썼는데 사실이 아니다 —
//! 테스트는 오늘도 `Allowed` 를 받아낸다. 2026-08-30 독립 검수 2라운드가
//! 이 과장을 짚었다. 정확히는 **진술을 거짓으로 쓰지 않고는** 허용받을
//! 수 없다는 뜻이다.)
//!
//! `ADR-033` §8 이 조건 2 에 직접 달아 둔 단서다.
//!
//! > 이 규범을 실제로 강제하는 코드는 아직 없다. Job 실행 자체가
//! > 미착수여서 멈출 대상이 없다. 조건 2 는 지금 계약으로만 존재하고
//! > 강제는 없다 — **그 전까지 이 경로를 켜면 안 된다.**
//!
//! 그래서 조건 2 는 "만료 시각이 지났는가" 만 보지 않는다.
//! [`PartitionPauseEnforcement`] 를 **함께** 요구한다 — 만료된 노드가
//! 스스로 멈추는 것을 호출부가 실제로 강제하고 있음을 진술해야 한다.
//! 오늘 그런 호출부는 없으므로 정직한 호출은 전부
//! [`PartitionPauseEnforcement::NotEnforcedYet`] 를 넘기고, 이 커널은
//! 나머지 다섯 조건이 완벽해도 거부한다.
//!
//! # ★ 그러나 이것은 "강제" 가 아니라 "요구" 다
//!
//! 2026-08-30 독립 검수가 초안의 과장을 정확히 짚었다. 초안은 이
//! 관문이 금지를 "타입으로 **강제**한다" 고 썼는데, 틀렸다.
//!
//! ```text
//! 이 커널이 하는 것    호출부가 각 사실을 **값으로 진술**하게 만든다.
//!                      진술하지 않으면(기본값이 없다) 컴파일되지 않고,
//!                      거짓으로 진술하면 그 거짓말이 호출 지점에 남는다
//! 이 커널이 못 하는 것  그 진술이 참인지 확인. `EnforcedByCaller` 도
//!                      `BrokerPreSigned` 도 누구나 그냥 쓸 수 있는 값이다
//! ```
//!
//! 순수 커널은 서명을 검증할 수 없다 — 검증하려면 crypto 에 묶여야
//! 하고 그러면 순수하지 않다. 그래서 **확인하는 척하지 않는다**
//! (`CLAUDE.md` §0.4). 이 관문의 값어치는 "막는다" 가 아니라 **"거짓말을
//! 하지 않고는 통과할 수 없고, 그 거짓말이 코드 리뷰에 보인다"** 다.
//!
//! # 순수 커널이다 · production 에 연결되지 않았다
//!
//! 시계·I/O·DB·network·난수·crypto·전역 상태를 쓰지 않는다.
//! `now_unix_ms` 를 인자로 받고, 처리 전에 입력을 정렬해 **성공과 오류
//! 모두** 입력 순서와 무관하게 같은 답을 낸다 —
//! `evaluate_effective_replicas`·`gpu_scope_candidate`·
//! `classify_node_liveness` 와 같은 규칙이다.
//!
//! 호출하는 production 경로는 **없다**. 있으면 안 된다(위 §8 단서).
//!
//! # 이 커널이 하지 않는 것
//!
//! ```text
//! 서명 검증        이웃 신고의 서명·멤버십·유효기간은 호출부가 해소한다.
//!                  여기서 흉내 내면 강제하지 못하는 것을 강제한다고
//!                  주장하게 된다(CLAUDE.md §0.4)
//! 신고 메시지 정의  §7 의 이웃 신고 wire 메시지는 아직 없다. 이 커널은
//!                  그 메시지가 생겼을 때 채워질 자리를 잡아 둘 뿐이다
//! 감사 로그 쓰기    쓰지 않는다. 호출부가 **먼저 확정한** 기록의 id 를
//!                  받을 뿐이다(조건 6)
//! 상태 전이·예약    아무것도 쓰지 않는다. 판정만 돌려준다
//! 중복 실행 방지    막지 못한다. §8 이 명시했듯 원래 노드가 규범을
//!                  어기고 계속 돌면 이 판정으로 막을 수 없고, 외부 API
//!                  호출은 fence 로도 못 막는다(CLAUDE.md §0.4)
//! node·member ID 형식  `proto/common.proto` §5 는 "ID 는 별도 명시가
//!                  없으면 ULID 26자" 로 정했지만, node/member ID 에 대해
//!                  그걸 강제하는 계층은 아직 없다 — `filter.rs`·`model.rs`
//!                  를 비롯한 scheduler 전체가 임의 문자열을 쓴다. 여기서만
//!                  강제하면 다른 커널과 어긋나므로, 정규화 공격만 막는
//!                  최소 형태 검사에 그친다
//! ```
//!
//! ★ **job/attempt ID 는 다르다 — 여기서 26자를 실제로 강제한다.**
//!   2라운드에 "어느 계층도 ULID 를 강제하지 않는다" 고 썼는데 **틀렸다**
//!   (3라운드 지적). `crates/protocol/src/fenced_operation.rs` 가 정확히
//!   job/attempt ID 를 26자로 거부한다. 이 관문은 그것과 같은 fencing
//!   영역이므로 같은 규칙을 쓴다.

use std::collections::BTreeSet;

/// 풀의 참여 모델.
///
/// `ADR-033` §8 이 이 축으로 기본값을 갈랐다 — 조건 3(이웃 신고)이
/// "다수가 정직하다" 를 전제하는데, 공개 풀은 그 전제를 깔 수 없다.
///
/// # `gputeer_protocol::ParticipationModel` 과의 관계
///
/// ★ 2026-08-30 독립 검수 2라운드가 "정본 타입이 이미 있는데 동명 enum 을
///   새로 만들었다" 고 지적했다. 사실이고, **의도한 중복**임을 여기 적어
///   둔다.
///
/// `crates/scheduler` 는 의존성이 **하나도 없다** — `SecurityTier`·
/// `IsolationClass`·`KeyProtection` 도 같은 이유로 여기 따로 있다. 순수
/// 커널이 protocol/prost 에 묶이면 그 순수성이 사라진다.
///
/// 대신 **1:1 대응을 깨지 않도록** 정본과 같은 변형·같은
/// [`Self::assumes_mutual_trust`] 를 둔다. 정본이 바뀌면 여기도 바꿔야
/// 하고, 배선 시점의 변환은 두 값만 다루므로 눈으로 확인된다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ParticipationModel {
    /// 사설 팀 — 상호 신뢰를 명시적으로 전제한다. 기본 허용.
    PrivateTeam,
    /// 공개 풀 — 참여자를 신뢰하지 않는다. 기본 금지.
    PublicPool,
}

impl ParticipationModel {
    /// 참여자가 서로 안다고 전제하는가.
    ///
    /// `ADR-033` §8 이 조건 3 의 근거로 직접 지목한 성질이다 — 이웃 신고는
    /// "다수가 정직하다" 를 전제하는데, 그 전제를 깔 수 있는 모델에서만
    /// 성립한다. 정본 `gputeer_protocol::ParticipationModel` 과 같은 답을
    /// 내야 한다.
    pub fn assumes_mutual_trust(self) -> bool {
        match self {
            Self::PrivateTeam => true,
            Self::PublicPool => false,
        }
    }
}

/// 조건 2 의 **강제 여부에 대한 호출부의 진술**.
///
/// `PARTITION_BEHAVIOR_PAUSE` 는 "lease 만료 시 checkpoint 후 PAUSE" 를
/// 규정한다. 만료된 노드가 규범대로 멈춰 있어야 재배정이 안전하다.
///
/// ★ 그 규범을 강제하는 코드가 이 저장소에 아직 없다. 그래서 "만료
///   시각이 지났다" 는 사실만으로 조건 2 를 충족시키지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PartitionPauseEnforcement {
    /// 호출부가 만료 시 PAUSE 를 **실제로 강제한다고 진술한다**.
    ///
    /// ★ 이름에 `ByCaller` 를 넣은 이유 — 이 커널은 그 진술을 확인하지
    ///   못한다(2026-08-30 독립 검수 지적). 오늘 이 값을 정직하게 넘길
    ///   수 있는 호출부는 없다.
    EnforcedByCaller,
    /// 강제 수단이 없다 — 오늘의 정직한 값이다.
    NotEnforcedYet,
}

/// 조건 3 — 이웃 신고에 대해 호출부가 무엇을 해소했는지.
///
/// `scope.rs` 의 `ProvenanceGate` 와 같은 형태다. 값을 반드시 쓰게 만들어,
/// 검증하지 않았으면서 검증했다고 적는 것이 호출 지점에 보이게 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NeighborReportResolution {
    /// 서명·멤버십·유효기간을 호출부가 확인했다고 진술한다.
    SignatureAndMembershipVerifiedByCaller,
    /// 확인하지 않았다 — 세지 않고 입력 오류로 거부한다.
    Unresolved,
}

/// 조건 5 — 예비 fence 범위의 출처에 대한 진술.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FenceRangeIssuance {
    /// Broker 가 최초 배치 때 이 범위를 서명해 발행했다고 진술한다.
    BrokerPreSignedVerifiedByCaller,
    /// 그 서명을 확인하지 못했다 — 입력 오류로 거부한다.
    ///
    /// 확인 없이 범위를 받으면 "한 풀에 발급자 하나" 금지가 무의미해진다
    /// (`ADR-033` (c)).
    Unproven,
}

/// 조건 4 — 작업을 받을 기계의 승인.
///
/// 남의 PC 에 작업을 억지로 밀어넣지 않는다(`CLAUDE.md` §0.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReceivingNodeConsent {
    Granted,
    Withheld,
}

/// 조건 6 — 감사 로그 **기록 결과**.
///
/// ★ 초안은 "기록할 수 있는가"(`Recordable`)만 물었다. 독립 검수가
///   짚었듯 `ADR-033` §8 조건 6 은 "이 재할당이 감사 로그에 **기록된다**"
///   이지 "기록할 수 있다" 가 아니다 — 미래의 가능성만 확인하고 허용하면
///   기록 없이 재배정이 일어난다.
///
///   그래서 이제 **이미 확정된 기록의 id** 를 요구한다. 순서도 이쪽이
///   맞다 — `send_revoke_notice()` 가 wire 전송보다 먼저 커밋을 확정한
///   것과 같은 이유다(`DoD-25`).
///   ★ 2라운드가 다시 짚었다 — id 만 받으면 **다른 재할당의 기록**을
///     가져다 붙일 수 있다. 그래서 기록이 **무엇을 기록했는지**(job·
///     attempt·fence 번호)도 함께 받아 요청과 대조한다.
///
///   ★ 3라운드가 그 표현도 과했다고 짚었다. 정확히는 **다른 재할당의 값이
///     적힌 기록을 붙이는 것**을 막을 뿐이다. 같은 값을 그냥 써 넣는 위조는
///     막지 못한다 — `record_id` 와 나머지 필드 사이에 검증된 결합이 없기
///     때문이고, 순수 커널은 그 결합을 확인할 수 없다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AuditRecord {
    /// 이 재할당의 감사 기록을 **이미 확정했다**.
    Committed {
        /// 그 기록의 식별자.
        record_id: String,
        /// 그 기록이 담고 있는 job. 요청과 달라야 할 이유가 없다.
        recorded_job_id: String,
        /// 그 기록이 담고 있는 attempt.
        recorded_attempt_id: String,
        /// 그 기록이 담고 있는 fence 번호.
        recorded_fence_epoch: u64,
    },
    /// 아직 기록되지 않았다.
    NotRecorded,
}

/// 이웃 하나가 낸 "나도 그 노드에 연락하지 못한다" 신고.
///
/// **권한이 아니라 입력이다**(`ADR-033` §7). 몇 건이 모여도 그 자체로는
/// 어떤 예약도 무효화하지 않는다.
/// ★ 정족수는 **기계 수**로 센다.
///
/// `ADR-033` §8 조건 3 은 "이웃 N **대**" 다. 초안은 `reporter_member_id`
/// 개수로 셌는데, 그러면 한 멤버가 여러 기계를 갖고 있어도 한 표이고
/// 한 기계에 멤버 ID 가 여럿이면 여러 표가 된다 — 둘 다 ADR 이 정한
/// 것과 다르다(2026-08-30 독립 검수 5라운드 지적).
///
/// ★ 한 멤버가 N 대를 소유해 혼자 정족수를 채우는 것은 이 커널이 막지
///   못한다. 이건 `ADR-033` §8 의 "담합한 다수" 와 **같은 것이 아니다** —
///   저건 서로 다른 주체 여럿이 짜는 경우이고 이건 한 주체가 기계를
///   여럿 가진 경우다(2026-08-30 독립 검수 6라운드가 이 비유의 부정확함을
///   짚었다). 둘 다 이 커널 밖이지만 막는 방법이 다르다 — 후자는
///   멤버십/failure-domain 해석이 신고자를 주체별로 묶어야 막힌다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NeighborUnreachableReport {
    /// 신고한 **기계**. 정족수는 이 값의 서로 다른 개수로 센다.
    pub reporter_node_id: String,
    /// 그 기계를 소유한 멤버. 자기 신고를 거르는 데 쓴다.
    pub reporter_member_id: String,
    /// 연락이 안 된다고 신고된 노드.
    pub unreachable_node_id: String,
    /// 신고자가 관측한 시각.
    pub observed_at_unix_ms: u64,
    /// 호출부가 이 신고에 대해 무엇을 해소했는지.
    pub resolution: NeighborReportResolution,
}

/// Broker 가 최초 배치 때 **미리 서명해 발행한** 예비 fence 번호 범위.
///
/// `ADR-033` §8 조건 5. 장애 중에 번호를 새로 만들면 "한 풀에 발급자
/// 하나" 금지를 어긴다 — 그래서 미리 받아 둔 범위 안에서만 쓴다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedFenceRange {
    /// 범위의 시작(포함).
    pub first: u64,
    /// 범위의 끝(포함).
    pub last: u64,
    /// 이미 써 버린 번호들. 재사용하면 fencing 이 무의미해진다.
    pub already_used: BTreeSet<u64>,
    /// 이 범위가 Broker 서명에서 왔는지에 대한 진술.
    pub issuance: FenceRangeIssuance,
    /// Broker 가 **어느 Job 의** failover 용으로 발행했는지.
    ///
    /// ★ `ADR-033` §8 조건 5 는 "**이 Job 의** failover 에는 101~110 을
    ///   써도 된다" 다. 초안은 범위 숫자만 받아서, 다른 Job 용으로 받은
    ///   범위를 이 Job 에 가져다 쓸 수 있었다(2026-08-30 독립 검수 2라운드
    ///   지적).
    pub issued_for_job_id: String,
    /// 같은 이유로 attempt 도 함께 묶는다.
    pub issued_for_attempt_id: String,
}

/// 풀 정책이 정한 값들.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReassignmentPolicy {
    /// 조건 1 — 이 시간을 **넘겨야** 이 경로가 열린다.
    ///
    /// 0 은 허용하지 않는다 — 0 이면 조건 1 이 사실상 없는 것이 된다.
    pub broker_silence_threshold_ms: u64,
    /// 조건 3 — 사설 팀에서 요구하는 서로 다른 이웃 신고 수. 1 이상.
    pub private_team_required_reports: u32,
    /// 조건 3 — 공개 풀에서 요구하는 수.
    ///
    /// `ADR-033` §8 은 공개 풀에서 "이웃 정족수 N 을 더 높게 잡아야
    /// 한다" 고만 정하고 값은 안 정했다. 그래서 **값을 지어내지 않고**
    /// 정책이 선언하게 하되, 사설 팀보다 크지 않으면 정책 오류로
    /// 거부한다.
    pub public_pool_required_reports: u32,
    /// 공개 풀에서 이 경로를 아예 여는가. 기본은 닫힘이어야 한다.
    pub public_pool_enabled: bool,
}

/// 재배정 요청 — 여섯 조건의 재료가 전부 여기 들어온다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReassignmentRequest {
    pub job_id: String,
    pub attempt_id: String,
    /// 지금 작업이 묶여 있는(=연락이 안 되는) 노드.
    pub stranded_node_id: String,
    /// 그 노드를 소유한 멤버. 자기 신고를 걸러내는 데 쓴다.
    pub stranded_member_id: String,
    /// 작업을 받겠다는 노드.
    pub receiving_node_id: String,
    /// 받는 노드를 소유한 멤버. 자기편향 재배정을 걸러내는 데 쓴다.
    pub receiving_member_id: String,

    /// 조건 1.
    pub broker_silent_for_ms: u64,
    /// 조건 2 — 원래 Lease 의 만료 시각.
    pub original_lease_expires_at_unix_ms: u64,
    /// 조건 2 — 그 만료가 실제로 강제되는가(호출부 진술).
    pub partition_pause: PartitionPauseEnforcement,
    /// 조건 3 — 호출부가 서명·멤버십을 해소한 이웃 신고들.
    pub neighbor_reports: Vec<NeighborUnreachableReport>,
    /// 조건 4.
    pub receiving_consent: ReceivingNodeConsent,
    /// 원래 배치가 쓰던 fence 번호.
    ///
    /// 재배정 번호는 이보다 **반드시 커야** 한다 — fence 번호가 안 올라가면
    /// 옛 노드의 쓰기를 밀어낼 수 없어 fencing 자체가 무의미하다
    /// (`DurableFenceWatermark` 가 같은 규칙을 쓴다).
    pub original_fence_epoch: u64,
    /// 조건 5 — 이번에 쓰려는 번호.
    pub proposed_fence_epoch: u64,
    /// 조건 5 — 미리 받아 둔 범위.
    pub reserved_fence_range: ReservedFenceRange,
    /// 조건 6 — 이미 확정된 감사 기록.
    pub audit_record: AuditRecord,

    pub model: ParticipationModel,
    pub now_unix_ms: u64,
}

/// 충족되지 않은 조건.
///
/// **하나만 돌려주지 않고 전부 돌려준다.** 하나씩 고치게 하면 운영자가
/// 남은 것을 모른 채 다음 시도를 한다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnmetCondition {
    /// 조건 1 — 침묵이 임계값을 **넘지** 못했다.
    ///
    /// `ADR-033` §8 조건 1 은 "정한 시간을 **넘겼다**" 이므로 정확히 같은
    /// 값은 충족이 아니다(2026-08-30 독립 검수 지적으로 정정).
    BrokerSilenceTooShort { observed_ms: u64, required_ms: u64 },
    /// 조건 2 — 만료 시각이 아직 안 지났다.
    ///
    /// `now == expires_at` 은 **만료된 것으로 본다**(`<=` 경계) —
    /// 이 저장소의 다른 만료 판정과 같은 규칙이다.
    OriginalLeaseStillValid {
        expires_at_unix_ms: u64,
        now_unix_ms: u64,
    },
    /// 조건 2 — 만료는 지났지만 그 만료를 강제하는 코드가 없다.
    ///
    /// ★ 오늘 모든 정직한 호출이 여기서 막힌다.
    PartitionPauseNotEnforced,
    /// 조건 3 — 서로 다른 **기계** 수가 모자란다.
    NotEnoughNeighborReports { distinct: u32, required: u32 },
    /// 조건 4.
    ReceivingNodeDidNotConsent,
    /// 조건 5 — 번호가 예비 범위 밖이다.
    FenceEpochOutsideReservedRange {
        proposed: u64,
        first: u64,
        last: u64,
    },
    /// 조건 5 — 범위 안이지만 이미 쓴 번호다.
    FenceEpochAlreadyUsed { proposed: u64 },
    /// 조건 5 — 이미 관측된 번호보다 높지 않다.
    ///
    /// ★ 초안은 **예비 범위의 시작**이 원래 번호보다 크기만 하면 통과시켰다.
    ///   그래서 `original=100`, 범위 `101..=110`, 이미 쓴 `{105}` 상태에서
    ///   `104` 가 허용됐다 — 105 가 이미 돌았는데 104 로 되돌아가는 것이고,
    ///   그건 `crates/protocol/src/fenced_operation.rs` 가 `StaleFence` 로
    ///   거부하는 바로 그 상황이다(2026-08-30 독립 검수 3라운드 지적).
    ///
    ///   이제 제안 번호가 **원래 번호와 이미 쓴 번호 전부보다** 커야 한다.
    FenceEpochNotAboveHighestSeen { proposed: u64, must_exceed: u64 },
    /// 조건 6.
    AuditRecordNotCommitted,
    /// 모델 관문 — 공개 풀에서 이 경로가 꺼져 있다.
    PublicPoolReassignmentDisabled,
}

/// 입력 자체가 판정할 수 없는 상태 — **fail closed**.
///
/// 조용히 무시하지 않는다. 무시하면 분모가 줄어 조건이 실제보다 쉽게
/// 충족된다(`CLAUDE.md` §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReassignmentInputError {
    /// 식별자가 규범 형태가 아니다.
    ///
    /// 공백을 포함하거나(앞뒤 포함), 비어 있거나, ASCII 출력 문자가 아닌
    /// 것이 섞여 있다. **다듬어서 받아들이지 않고 거부한다** — 다듬으면
    /// `"member-a"` 와 `" member-a"` 가 조용히 같아지고, 그 조용함이
    /// 자기편향 검사를 우회하는 통로가 된다(2026-08-30 독립 검수 지적).
    NonCanonicalIdentifier {
        field: &'static str,
        value: String,
    },
    /// 작업을 옮길 곳이 원래 있던 곳과 같다.
    ReceivingNodeIsStrandedNode { node_id: String },
    /// 서명·멤버십이 해소되지 않은 신고.
    UnresolvedNeighborReport { reporter_member_id: String },
    /// 같은 기계가 서로 다른 멤버 소유로 신고됐다.
    ///
    /// 어느 쪽이 맞는지 이 커널은 모른다 — 추측하면 자기 신고 걸러내기가
    /// 뚫린다. fail closed.
    ConflictingReporterOwnership {
        reporter_node_id: String,
        first_member_id: String,
        second_member_id: String,
    },
    /// 신고가 다른 노드를 가리킨다. 세면 안 되는 표다.
    ReportTargetsAnotherNode {
        reporter_member_id: String,
        reported_node_id: String,
        expected_node_id: String,
    },
    /// 미래에 관측했다는 신고.
    ReportObservedInFuture {
        reporter_member_id: String,
        observed_at_unix_ms: u64,
        now_unix_ms: u64,
    },
    /// 이득 보는 쪽이 스스로 신고했다.
    ///
    /// `ADR-033` §8 이 조건 4 만으로 부족하다고 한 이유가 이것이다 —
    /// 감염된 리더가 자기 기계를 대상으로 삼으면 자기가 자기를
    /// 승인한다. 승인하는 쪽과 이득 보는 쪽을 분리한다.
    ///
    ReportFromBeneficiary { member_id: String },
    /// 연락이 안 된다고 지목된 노드의 소유자가 스스로 신고했다.
    ReportFromStrandedMember { member_id: String },
    /// **지목당한 기계 자신**이 신고했다.
    ///
    /// ★ 초안은 이 경우도 `ReportFromBeneficiary` 로 보고했는데, 지목당한
    ///   기계는 이득을 보는 쪽이 아니다 — 오류가 사실을 잘못 전한 것이다
    ///   (`CLAUDE.md` §3, 2026-08-30 독립 검수 6라운드 지적).
    ReportFromStrandedNode { reporter_node_id: String },
    /// **작업을 받을 기계**가 신고했다 — 이쪽이 이득을 보는 쪽이다.
    ReportFromReceivingNode { reporter_node_id: String },
    /// 두 식별자가 **같은지 다른지 이 커널이 정할 수 없다.**
    ///
    /// 대소문자만 다른 두 값이 그렇다. 같다고 보면 정당한 표를 잃고,
    /// 다르다고 보면 한 명이 정족수를 혼자 채운다.
    ///
    /// ★ 초안은 대소문자 무시 비교를 **발명**했다 — 독립 검수 2라운드가
    ///   짚었듯 이 저장소의 정본 멤버십 판정
    ///   (`crates/protocol/src/membership.rs`)은 **정확한 문자열 비교**를
    ///   쓰고, 판단할 수 없을 때 `Ambiguous` 로 fail closed 한다. 그래서
    ///   여기도 같은 규칙을 쓴다 — 비교는 정확히 하고, 애매하면 **추측하지
    ///   않고 거부한다.**
    AmbiguousIdentifier {
        field: &'static str,
        first: String,
        second: String,
    },
    /// 감사 기록에 적힌 job·attempt·fence 가 이 요청의 값과 다르다.
    ///
    /// ★ "다른 재할당을 기록했다" 고 단정하지 않는다 — 이 커널이 아는 것은
    ///   **적힌 값이 다르다** 는 사실뿐이고, `record_id` 와 그 값들 사이의
    ///   결합은 확인하지 못한다(2026-08-30 독립 검수 9라운드 지적).
    AuditRecordDoesNotMatchRequest {
        record_id: String,
        recorded_job_id: String,
        recorded_attempt_id: String,
        recorded_fence_epoch: u64,
    },
    /// job/attempt ID 가 ULID 26자가 아니다.
    ///
    /// `crates/protocol/src/fenced_operation.rs` 와 같은 규칙이다.
    IdentifierIsNotUlidLength {
        field: &'static str,
        actual: usize,
    },
    /// 예비 fence 범위가 다른 Job/attempt 용으로 발행됐다.
    FenceRangeIssuedForAnotherAttempt {
        issued_for_job_id: String,
        issued_for_attempt_id: String,
    },
    /// 예비 범위가 뒤집혔다.
    ReservedFenceRangeInverted { first: u64, last: u64 },
    /// 예비 범위 **전체**가 원래 fence 번호 이하다.
    ///
    /// 재배정 번호가 안 올라가면 옛 노드의 쓰기를 밀어낼 수 없다.
    ///
    /// ★ 초안은 `last` 로 걸러 놓고 `first` 를 보고했다 — 오류가 사실을
    ///   잘못 전한 것이다(`CLAUDE.md` §3, 2026-08-30 독립 검수 4라운드 지적).
    ReservedFenceRangeNotAboveOriginal { last: u64, original: u64 },
    /// Broker 서명이 확인되지 않은 예비 범위.
    UnprovenFenceRange,
    /// 사용 이력에 이 범위 밖 번호가 들어 있다.
    ///
    /// 그 집합이 이 범위의 이력이 아니라는 뜻이다 — fail closed.
    AlreadyUsedEpochOutsideRange { epoch: u64, first: u64, last: u64 },
    /// 정책의 침묵 임계값이 0 이다 — 조건 1 이 사실상 사라진다.
    BrokerSilenceThresholdIsZero,
    /// 정책이 요구하는 이웃 신고 수가 0 이다 — 조건 3 이 사실상 사라진다.
    RequiredNeighborReportsIsZero,
    /// 공개 풀 정족수가 사설 팀보다 크지 않다.
    ///
    /// `ADR-033` §8 은 "더 높게 잡아야 한다" 고 정했다. 같거나 낮으면
    /// 정책이 그 규범을 어긴 것이므로 판정하지 않는다.
    PublicPoolQuorumNotHigher { public_pool: u32, private_team: u32 },
}

/// 판정 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReassignmentDecision {
    /// 여섯 조건이 전부 충족됐다.
    ///
    /// ★ 이 값은 "해도 된다" 이지 "해라" 가 아니다. 실제 재배정은
    ///   여전히 별도 코드가 하고, 그 코드는 오늘 없다.
    Allowed { fence_epoch: u64 },
    /// 하나라도 충족되지 않았다. 충족 안 된 것을 **전부** 담는다.
    Refused { unmet: Vec<UnmetCondition> },
}

/// 식별자가 규범 형태인지 본다 — **다듬지 않고 거부한다.**
///
/// 규범 형태: 비어 있지 않고, 공백 문자가 하나도 없고, 전부 ASCII
/// 출력 문자다.
///
/// ★ 왜 이렇게 좁은가 — 넓게 받으면 `"member-a"` 와 `" member-a"`,
///   그리고 유니코드 정규화가 다른 두 표현이 **다른 멤버로** 세어진다.
///   그러면 한 사람이 정족수를 혼자 채울 수 있다(2026-08-30 독립 검수
///   지적). 정규화 규칙을 여기서 발명하는 대신, 정규화가 문제되지 않는
///   범위만 받는다.
fn require_canonical_identifier(
    value: &str,
    field: &'static str,
) -> Result<(), ReassignmentInputError> {
    let malformed = value.is_empty()
        || value.chars().any(|c| c.is_whitespace())
        || !value.chars().all(|c| c.is_ascii_graphic());
    if malformed {
        return Err(ReassignmentInputError::NonCanonicalIdentifier {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

/// `proto/common.proto` §5 — "ID 는 별도 명시가 없으면 ULID 26자 문자열".
///
/// `crates/protocol/src/fenced_operation.rs` 가 job/attempt ID 에 대해 쓰는
/// 것과 같은 상수다. 알파벳까지 보지는 않는다 — 그쪽도 길이만 본다.
const ULID_LEN: usize = 26;

/// job/attempt ID 는 규범 형태 + 26자여야 한다.
fn require_ulid_identifier(
    value: &str,
    field: &'static str,
) -> Result<(), ReassignmentInputError> {
    require_canonical_identifier(value, field)?;
    if value.chars().count() != ULID_LEN {
        return Err(ReassignmentInputError::IdentifierIsNotUlidLength {
            field,
            actual: value.chars().count(),
        });
    }
    Ok(())
}

/// 두 식별자의 관계.
///
/// ★ 2라운드 대응에서 "정본 `membership.rs` 와 **같은 규칙**" 이라고 썼는데
///   정확하지 않다(3라운드 지적). `membership.rs` 의 `Ambiguous` 는 "권위
///   있는 사실이 여러 개라 고를 수 없다" 이고, 여기 `Ambiguous` 는 "대소문자만
///   다른 두 문자열이라 같은 것인지 모른다" 다 — **다른 상황**이다.
///
///   공통점은 태도뿐이다 — **모르면 추측하지 않고 거부한다.** 이 저장소에
///   식별자 정규화 계약이 없으므로 대소문자 차이를 같다고도 다르다고도
///   단정할 수 없고, 그래서 이 값이 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityMatch {
    /// 바이트 단위로 같다.
    Same,
    /// 대소문자만 다르다 — 같은 사람인지 이 커널은 모른다.
    Ambiguous,
    /// 확실히 다르다.
    Different,
}

/// 식별자를 **정확히** 비교하되, 대소문자만 다른 경우를 따로 표시한다.
///
/// 식별자가 ASCII 출력 문자임은 [`require_canonical_identifier`] 가 이미
/// 보장했으므로 `eq_ignore_ascii_case` 로 충분하다.
fn compare_identity(left: &str, right: &str) -> IdentityMatch {
    if left == right {
        IdentityMatch::Same
    } else if left.eq_ignore_ascii_case(right) {
        IdentityMatch::Ambiguous
    } else {
        IdentityMatch::Different
    }
}

/// `ADR-033` §8 의 여섯 조건을 판정한다.
///
/// 하나라도 안 맞으면 [`ReassignmentDecision::Refused`] 이고, 안 맞은
/// 것을 전부 담는다. 입력이 애초에 판정 불가면 `Err` 다.
///
/// # 순수하다
///
/// 시계를 읽지 않는다(`now_unix_ms` 를 받는다). 입력을 정렬해 처리하므로
/// 신고 순서를 바꿔도 성공/실패와 그 내용이 모두 같다.
pub fn evaluate_reassignment(
    request: &ReassignmentRequest,
    policy: &ReassignmentPolicy,
) -> Result<ReassignmentDecision, ReassignmentInputError> {
    // --- 정책 검증. 정책으로 조건을 지워 버릴 수 없게 한다. ---
    //
    // ★ 초안은 임계값 0·정족수 0 을 그대로 받았다 — 그러면 조건 1 과 3 이
    //   있으나 마나다(2026-08-30 독립 검수 지적).
    if policy.broker_silence_threshold_ms == 0 {
        return Err(ReassignmentInputError::BrokerSilenceThresholdIsZero);
    }
    if policy.private_team_required_reports == 0 || policy.public_pool_required_reports == 0 {
        return Err(ReassignmentInputError::RequiredNeighborReportsIsZero);
    }
    if policy.public_pool_required_reports <= policy.private_team_required_reports {
        return Err(ReassignmentInputError::PublicPoolQuorumNotHigher {
            public_pool: policy.public_pool_required_reports,
            private_team: policy.private_team_required_reports,
        });
    }

    // --- 입력 검증. 판정보다 먼저, 그리고 조용히 넘기지 않는다. ---
    require_ulid_identifier(&request.job_id, "job_id")?;
    require_ulid_identifier(&request.attempt_id, "attempt_id")?;
    require_canonical_identifier(&request.stranded_node_id, "stranded_node_id")?;
    require_canonical_identifier(&request.stranded_member_id, "stranded_member_id")?;
    require_canonical_identifier(&request.receiving_node_id, "receiving_node_id")?;
    require_canonical_identifier(&request.receiving_member_id, "receiving_member_id")?;

    match compare_identity(&request.receiving_node_id, &request.stranded_node_id) {
        IdentityMatch::Same => {
            return Err(ReassignmentInputError::ReceivingNodeIsStrandedNode {
                node_id: request.stranded_node_id.clone(),
            })
        }
        IdentityMatch::Ambiguous => {
            return Err(ReassignmentInputError::AmbiguousIdentifier {
                field: "receiving_node_id",
                first: request.stranded_node_id.clone(),
                second: request.receiving_node_id.clone(),
            })
        }
        IdentityMatch::Different => {}
    }

    let range = &request.reserved_fence_range;
    require_ulid_identifier(&range.issued_for_job_id, "issued_for_job_id")?;
    require_ulid_identifier(&range.issued_for_attempt_id, "issued_for_attempt_id")?;
    // 이 범위가 **이 Job 의** failover 용인지 본다(`ADR-033` §8 조건 5).
    //
    // ★ 대소문자만 다르면 "다른 Job" 이라고 단정하지 않는다 — 그건 사실을
    //   잘못 전하는 오류다(`CLAUDE.md` §3, 독립 검수 3라운드 지적).
    for (mine, theirs, field) in [
        (
            &request.job_id,
            &range.issued_for_job_id,
            "issued_for_job_id",
        ),
        (
            &request.attempt_id,
            &range.issued_for_attempt_id,
            "issued_for_attempt_id",
        ),
    ] {
        if compare_identity(mine, theirs) == IdentityMatch::Ambiguous {
            return Err(ReassignmentInputError::AmbiguousIdentifier {
                field,
                first: mine.clone(),
                second: theirs.clone(),
            });
        }
    }
    if range.issued_for_job_id != request.job_id
        || range.issued_for_attempt_id != request.attempt_id
    {
        return Err(ReassignmentInputError::FenceRangeIssuedForAnotherAttempt {
            issued_for_job_id: range.issued_for_job_id.clone(),
            issued_for_attempt_id: range.issued_for_attempt_id.clone(),
        });
    }
    if range.first > range.last {
        return Err(ReassignmentInputError::ReservedFenceRangeInverted {
            first: range.first,
            last: range.last,
        });
    }
    // 범위 **전체**가 원래 번호 이하면 이 Job 의 failover 범위일 수 없다.
    //
    // ★ 초안은 `range.first` 를 봤는데, 그러면 `ADR-033` §8 의 "범위를 다
    //   쓸 때까지 재할당" 과 어긋난다 — 정직하게 현재 번호를 넣으면
    //   범위 시작이 그보다 낮아 입력 오류가 났다(독립 검수 3라운드 지적).
    //   범위가 쓸모 있는지는 `last` 로 보고, 개별 번호의 단조성은 아래
    //   조건 5 에서 본다.
    if range.last <= request.original_fence_epoch {
        return Err(ReassignmentInputError::ReservedFenceRangeNotAboveOriginal {
            last: range.last,
            original: request.original_fence_epoch,
        });
    }
    if range.issuance == FenceRangeIssuance::Unproven {
        return Err(ReassignmentInputError::UnprovenFenceRange);
    }
    // ★ `already_used` 는 **이 범위의 사용 이력**이다. 범위 밖 번호가 들어
    //   있으면 그 집합이 이 범위의 것이 아니라는 뜻이므로 판정하지 않는다
    //   (2026-08-30 독립 검수 5라운드 지적). 조용히 받아들이면 범위 밖
    //   값이 `highest_seen` 을 끌어올려 엉뚱한 이유로 거부된다.
    if let Some(outside) = range
        .already_used
        .iter()
        .find(|used| **used < range.first || **used > range.last)
    {
        return Err(ReassignmentInputError::AlreadyUsedEpochOutsideRange {
            epoch: *outside,
            first: range.first,
            last: range.last,
        });
    }

    if let AuditRecord::Committed {
        record_id,
        recorded_job_id,
        recorded_attempt_id,
        recorded_fence_epoch,
    } = &request.audit_record
    {
        // ★ `record_id` 도 다른 식별자와 **같은 규칙**으로 본다. 초안은
        //   여기만 `trim().is_empty()` 였다 — 규칙이 다를 이유가 없는데
        //   달랐고, 독립 검수 2라운드가 누락으로 짚었다.
        require_canonical_identifier(record_id, "audit_record_id")?;
        require_ulid_identifier(recorded_job_id, "recorded_job_id")?;
        require_ulid_identifier(recorded_attempt_id, "recorded_attempt_id")?;

        // 여기도 대소문자만 다르면 "다른 재할당" 이라고 단정하지 않는다.
        for (mine, theirs, field) in [
            (&request.job_id, recorded_job_id, "recorded_job_id"),
            (
                &request.attempt_id,
                recorded_attempt_id,
                "recorded_attempt_id",
            ),
        ] {
            if compare_identity(mine, theirs) == IdentityMatch::Ambiguous {
                return Err(ReassignmentInputError::AmbiguousIdentifier {
                    field,
                    first: mine.clone(),
                    second: theirs.clone(),
                });
            }
        }

        // 이 기록이 **이 재할당**을 기록한 것인지 본다.
        if recorded_job_id != &request.job_id
            || recorded_attempt_id != &request.attempt_id
            || *recorded_fence_epoch != request.proposed_fence_epoch
        {
            return Err(ReassignmentInputError::AuditRecordDoesNotMatchRequest {
                record_id: record_id.clone(),
                recorded_job_id: recorded_job_id.clone(),
                recorded_attempt_id: recorded_attempt_id.clone(),
                recorded_fence_epoch: *recorded_fence_epoch,
            });
        }
    }

    // ★ 정렬한 뒤에 본다. 그래야 **오류도** 입력 순서와 무관하게 같다 —
    //   `classify_node_liveness` 가 같은 이유로 같은 일을 한다.
    let mut reports = request.neighbor_reports.clone();
    reports.sort();

    // 정족수는 **기계**로 센다(`ADR-033` §8 조건 3 의 "이웃 N 대").
    // 멤버는 자기 신고를 거르는 데만 쓴다.
    let mut distinct_reporter_nodes: BTreeSet<&str> = BTreeSet::new();
    let mut node_owner: Vec<(&str, &str)> = Vec::new();
    for report in &reports {
        require_canonical_identifier(&report.reporter_node_id, "reporter_node_id")?;
        require_canonical_identifier(&report.reporter_member_id, "reporter_member_id")?;
        require_canonical_identifier(&report.unreachable_node_id, "unreachable_node_id")?;

        if report.resolution == NeighborReportResolution::Unresolved {
            return Err(ReassignmentInputError::UnresolvedNeighborReport {
                reporter_member_id: report.reporter_member_id.clone(),
            });
        }
        match compare_identity(&report.unreachable_node_id, &request.stranded_node_id) {
            IdentityMatch::Same => {}
            IdentityMatch::Ambiguous => {
                return Err(ReassignmentInputError::AmbiguousIdentifier {
                    field: "unreachable_node_id",
                    first: request.stranded_node_id.clone(),
                    second: report.unreachable_node_id.clone(),
                })
            }
            IdentityMatch::Different => {
                return Err(ReassignmentInputError::ReportTargetsAnotherNode {
                    reporter_member_id: report.reporter_member_id.clone(),
                    reported_node_id: report.unreachable_node_id.clone(),
                    expected_node_id: request.stranded_node_id.clone(),
                })
            }
        }
        if report.observed_at_unix_ms > request.now_unix_ms {
            return Err(ReassignmentInputError::ReportObservedInFuture {
                reporter_member_id: report.reporter_member_id.clone(),
                observed_at_unix_ms: report.observed_at_unix_ms,
                now_unix_ms: request.now_unix_ms,
            });
        }
        // 이득 보는 쪽·지목당한 쪽의 자기 신고를 거른다. 대소문자만 다르면
        // 같은 사람인지 알 수 없으므로 **추측하지 않고 거부한다.**
        for (other, field, ambiguous_first) in [
            (
                &request.receiving_member_id,
                "reporter_member_id/receiving_member_id",
                &request.receiving_member_id,
            ),
            (
                &request.stranded_member_id,
                "reporter_member_id/stranded_member_id",
                &request.stranded_member_id,
            ),
        ] {
            match compare_identity(&report.reporter_member_id, other) {
                IdentityMatch::Same if other == &request.receiving_member_id => {
                    return Err(ReassignmentInputError::ReportFromBeneficiary {
                        member_id: report.reporter_member_id.clone(),
                    })
                }
                IdentityMatch::Same => {
                    return Err(ReassignmentInputError::ReportFromStrandedMember {
                        member_id: report.reporter_member_id.clone(),
                    })
                }
                IdentityMatch::Ambiguous => {
                    return Err(ReassignmentInputError::AmbiguousIdentifier {
                        field,
                        first: ambiguous_first.clone(),
                        second: report.reporter_member_id.clone(),
                    })
                }
                IdentityMatch::Different => {}
            }
        }

        // 신고한 기계가 지목당한 기계이거나 받을 기계면 안 된다.
        //
        // ★ 두 경우를 **다른 오류로** 보고한다. 지목당한 기계가 스스로
        //   신고하는 것은 "이득 보는 쪽의 자기 신고" 가 아니다 — 같은
        //   이름으로 보고하면 오류가 사실을 잘못 전한다(독립 검수 6라운드).
        for (other, field, is_receiving) in [
            (
                &request.stranded_node_id,
                "reporter_node_id/stranded_node_id",
                false,
            ),
            (
                &request.receiving_node_id,
                "reporter_node_id/receiving_node_id",
                true,
            ),
        ] {
            match compare_identity(&report.reporter_node_id, other) {
                IdentityMatch::Same if is_receiving => {
                    return Err(ReassignmentInputError::ReportFromReceivingNode {
                        reporter_node_id: report.reporter_node_id.clone(),
                    })
                }
                IdentityMatch::Same => {
                    return Err(ReassignmentInputError::ReportFromStrandedNode {
                        reporter_node_id: report.reporter_node_id.clone(),
                    })
                }
                IdentityMatch::Ambiguous => {
                    return Err(ReassignmentInputError::AmbiguousIdentifier {
                        field,
                        first: other.clone(),
                        second: report.reporter_node_id.clone(),
                    })
                }
                IdentityMatch::Different => {}
            }
        }

        // 대소문자만 다른 두 신고 기계는 **추측하지 않고 거부한다.**
        //
        // 정렬된 목록의 **모든 이전 원소**와 비교한다 — 인접만 보면
        // `a` / `B` / `A` 처럼 사이에 낀 값이 있을 때 놓친다.
        if let Some(previous) = distinct_reporter_nodes
            .iter()
            .find(|seen| compare_identity(seen, &report.reporter_node_id) == IdentityMatch::Ambiguous)
        {
            return Err(ReassignmentInputError::AmbiguousIdentifier {
                field: "reporter_node_id",
                first: (*previous).to_string(),
                second: report.reporter_node_id.clone(),
            });
        }

        // 같은 기계가 서로 다른 멤버 소유로 신고되면 판정하지 않는다.
        //
        // ★ 대소문자만 다른 소유자 ID 를 "서로 다른 멤버" 라고 단정하지
        //   않는다 — 이 파일의 다른 식별자 비교와 같은 규칙이어야 한다
        //   (2026-08-30 독립 검수 8라운드 지적). 같은 사람인지 모르는
        //   것이지 다른 사람인 것이 아니다.
        for (node, member) in &node_owner {
            if *node != report.reporter_node_id {
                continue;
            }
            match compare_identity(member, &report.reporter_member_id) {
                IdentityMatch::Same => {}
                IdentityMatch::Ambiguous => {
                    return Err(ReassignmentInputError::AmbiguousIdentifier {
                        field: "reporter_member_id",
                        first: (*member).to_string(),
                        second: report.reporter_member_id.clone(),
                    })
                }
                IdentityMatch::Different => {
                    return Err(ReassignmentInputError::ConflictingReporterOwnership {
                        reporter_node_id: report.reporter_node_id.clone(),
                        first_member_id: (*member).to_string(),
                        second_member_id: report.reporter_member_id.clone(),
                    })
                }
            }
        }
        node_owner.push((
            report.reporter_node_id.as_str(),
            report.reporter_member_id.as_str(),
        ));

        // 같은 기계가 여러 번 신고해도 한 표다.
        distinct_reporter_nodes.insert(report.reporter_node_id.as_str());
    }

    // --- 여섯 조건. 하나라도 안 맞으면 담고 계속 본다. ---
    let mut unmet: Vec<UnmetCondition> = Vec::new();

    // 조건 1 — "정한 시간을 **넘겼다**". 같은 값은 충족이 아니다.
    if request.broker_silent_for_ms <= policy.broker_silence_threshold_ms {
        unmet.push(UnmetCondition::BrokerSilenceTooShort {
            observed_ms: request.broker_silent_for_ms,
            required_ms: policy.broker_silence_threshold_ms,
        });
    }

    // 조건 2 — 두 겹이다. 시각이 지났는가, 그리고 그 만료가 강제되는가.
    if request.original_lease_expires_at_unix_ms > request.now_unix_ms {
        unmet.push(UnmetCondition::OriginalLeaseStillValid {
            expires_at_unix_ms: request.original_lease_expires_at_unix_ms,
            now_unix_ms: request.now_unix_ms,
        });
    }
    if request.partition_pause == PartitionPauseEnforcement::NotEnforcedYet {
        unmet.push(UnmetCondition::PartitionPauseNotEnforced);
    }

    // 조건 3 — 모델에 따라 요구치가 다르다.
    let required_reports = match request.model {
        ParticipationModel::PrivateTeam => policy.private_team_required_reports,
        ParticipationModel::PublicPool => policy.public_pool_required_reports,
    };
    // ★ `as u32` 로 줄이지 않는다 — 신고가 `u32::MAX` 를 넘으면 조용히
    //   잘려 엉뚱한 답이 나온다(2026-08-30 독립 검수 2라운드 지적).
    //   `usize` 로 비교하고, 보고용 숫자만 상한에서 멈춘다.
    let distinct_count = distinct_reporter_nodes.len();
    if distinct_count < required_reports as usize {
        unmet.push(UnmetCondition::NotEnoughNeighborReports {
            distinct: u32::try_from(distinct_count).unwrap_or(u32::MAX),
            required: required_reports,
        });
    }

    // 조건 4.
    if request.receiving_consent == ReceivingNodeConsent::Withheld {
        unmet.push(UnmetCondition::ReceivingNodeDidNotConsent);
    }

    // 조건 5.
    // ★ 세 검사는 **서로 독립**이다. 초안은 `else if` 로 엮여 있어,
    //   예컨대 `already_used={104,105}` 에 `104` 를 제안하면 "이미 썼다" 만
    //   나오고 "105 보다 낮다" 는 가려졌다 — "안 맞은 것을 전부 돌려준다"
    //   는 이 함수의 계약과 어긋난다(2026-08-30 독립 검수 4라운드 지적).
    if request.proposed_fence_epoch < range.first || request.proposed_fence_epoch > range.last {
        unmet.push(UnmetCondition::FenceEpochOutsideReservedRange {
            proposed: request.proposed_fence_epoch,
            first: range.first,
            last: range.last,
        });
    }
    if range.already_used.contains(&request.proposed_fence_epoch) {
        unmet.push(UnmetCondition::FenceEpochAlreadyUsed {
            proposed: request.proposed_fence_epoch,
        });
    }
    // 이미 관측된 어떤 번호보다도 높아야 한다 — 낮은 번호로 되돌아가면
    // 옛 노드의 쓰기를 밀어내지 못한다(`StaleFence` 와 같은 규칙).
    let highest_seen = range
        .already_used
        .iter()
        .copied()
        .chain(std::iter::once(request.original_fence_epoch))
        .max()
        .expect("chain 에 최소 한 개가 있다");
    if request.proposed_fence_epoch <= highest_seen {
        unmet.push(UnmetCondition::FenceEpochNotAboveHighestSeen {
            proposed: request.proposed_fence_epoch,
            must_exceed: highest_seen,
        });
    }

    // 조건 6 — 기록 **가능성**이 아니라 기록 **완료**를 요구한다.
    if request.audit_record == AuditRecord::NotRecorded {
        unmet.push(UnmetCondition::AuditRecordNotCommitted);
    }

    // 모델 관문 — 상호 신뢰를 전제할 수 없는 모델은 기본 금지다.
    //
    // `ADR-033` §8 이 조건 3 의 근거로 지목한 성질을 그대로 쓴다 —
    // 변형 이름이 아니라 `assumes_mutual_trust()` 로 물어야 정본
    // `gputeer_protocol::ParticipationModel` 과 어긋나지 않는다.
    if !request.model.assumes_mutual_trust() && !policy.public_pool_enabled {
        unmet.push(UnmetCondition::PublicPoolReassignmentDisabled);
    }

    if unmet.is_empty() {
        return Ok(ReassignmentDecision::Allowed {
            fence_epoch: request.proposed_fence_epoch,
        });
    }

    // 담은 순서가 아니라 값 순서로 돌려준다 — 입력 순서와 무관하게 같다.
    unmet.sort();
    unmet.dedup();
    Ok(ReassignmentDecision::Refused { unmet })
}
