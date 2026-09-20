//! `ADR-033` §8 재배정 관문 테스트.
//!
//! ★ 이 파일에서 가장 중요한 테스트는 **오늘의 정직한 호출이 반드시
//!   거부됨을 고정하는 것**이다(`todays_honest_caller_is_always_refused`).
//!   나머지가 다 채워져도 조건 2 의 강제 수단이 없으므로 거부돼야 한다.
//!
//!   "아무것도 허용되지 않는다" 가 아니다 — 바로 아래 대조 테스트가
//!   `EnforcedByCaller` 를 넣어 `Allowed` 를 받아낸다. 이 커널이 막는 것은
//!   **거짓 진술 없는 통과**이지 통과 그 자체가 아니다(2026-08-30 독립 검수
//!   3라운드가 이 문서의 과장을 짚었다).

use std::collections::BTreeSet;

use gputeer_scheduler::{
    evaluate_reassignment, AuditRecord, FenceRangeIssuance, NeighborReportResolution,
    NeighborUnreachableReport, ParticipationModel, PartitionPauseEnforcement, ReassignmentDecision,
    ReassignmentInputError, ReassignmentPolicy, ReassignmentRequest, ReceivingNodeConsent,
    ReservedFenceRange, UnmetCondition,
};

const NOW: u64 = 1_700_000_000_000;

/// `proto/common.proto` §5 가 정한 26자 ULID.
///
/// ★ `crates/protocol/src/fenced_operation.rs` 가 job/attempt ID 에 이
///   길이를 실제로 강제한다 — 이 관문도 같은 규칙을 쓰므로 fixture 도
///   규범을 지켜야 한다(2026-08-30 독립 검수 3라운드 지적).
const JOB_ID: &str = "01JQBM4Z9K7X3N2P5R8T6V0W1Y";
const ATTEMPT_ID: &str = "01JQBM4Z9K7X3N2P5R8T6V0W2Z";
const OTHER_JOB_ID: &str = "01JQBM4Z9K7X3N2P5R8T6V0W3A";
const OTHER_ATTEMPT_ID: &str = "01JQBM4Z9K7X3N2P5R8T6V0W4B";

fn policy() -> ReassignmentPolicy {
    ReassignmentPolicy {
        broker_silence_threshold_ms: 60_000,
        private_team_required_reports: 2,
        public_pool_required_reports: 4,
        public_pool_enabled: false,
    }
}

/// 멤버 하나가 기계 하나를 가진 신고.
///
/// 정족수는 **기계** 수로 세므로(`ADR-033` §8 조건 3 의 "이웃 N 대"),
/// 기계 ID 는 멤버 ID 에서 파생시킨다.
fn report(member: &str, at: u64) -> NeighborUnreachableReport {
    report_from(&format!("node-of-{member}"), member, at)
}

fn report_from(node: &str, member: &str, at: u64) -> NeighborUnreachableReport {
    NeighborUnreachableReport {
        reporter_node_id: node.to_string(),
        reporter_member_id: member.to_string(),
        unreachable_node_id: "node-stranded".to_string(),
        observed_at_unix_ms: at,
        resolution: NeighborReportResolution::SignatureAndMembershipVerifiedByCaller,
    }
}

/// 제안 fence 번호에 맞는 감사 기록.
///
/// ★ 감사 기록은 **어느 재할당을 기록했는지**까지 들고 온다 — 그래야 값이
///   어긋난 기록을 붙이는 것을 거를 수 있다. `record_id` 와 그 값들 사이의
///   결합은 이 커널이 확인하지 못하므로, 같은 값을 적어 넣은 위조는 여전히
///   통과한다(독립 검수 2·9라운드 지적).
fn audit_for(fence_epoch: u64) -> AuditRecord {
    AuditRecord::Committed {
        record_id: "audit-1".to_string(),
        recorded_job_id: JOB_ID.to_string(),
        recorded_attempt_id: ATTEMPT_ID.to_string(),
        recorded_fence_epoch: fence_epoch,
    }
}

/// 조건 2 의 강제 수단을 제외한 나머지가 전부 채워진 요청.
///
/// ★ "다섯 조건이 충족됐다" 고 부르지 않는다 — 독립 검수가 짚었듯 이
///   fixture 의 이웃 신고는 실제 서명이 아니라 **호출부의 진술**이고,
///   감사 기록도 이 커널이 확인할 수 없는 id 다. `ADR-033` §8 수준에서
///   "충족" 이라고 말하려면 그 진술들이 참이어야 한다.
fn everything_but_the_enforcement_gate() -> ReassignmentRequest {
    ReassignmentRequest {
        job_id: JOB_ID.to_string(),
        attempt_id: ATTEMPT_ID.to_string(),
        stranded_node_id: "node-stranded".to_string(),
        stranded_member_id: "member-stranded".to_string(),
        receiving_node_id: "node-receiver".to_string(),
        receiving_member_id: "member-receiver".to_string(),

        broker_silent_for_ms: 120_000,
        original_lease_expires_at_unix_ms: NOW - 1,
        partition_pause: PartitionPauseEnforcement::NotEnforcedYet,
        neighbor_reports: vec![report("member-a", NOW - 500), report("member-b", NOW - 400)],
        receiving_consent: ReceivingNodeConsent::Granted,
        original_fence_epoch: 100,
        proposed_fence_epoch: 105,
        reserved_fence_range: ReservedFenceRange {
            first: 101,
            last: 110,
            already_used: BTreeSet::new(),
            issuance: FenceRangeIssuance::BrokerPreSignedVerifiedByCaller,
            issued_for_job_id: JOB_ID.to_string(),
            issued_for_attempt_id: ATTEMPT_ID.to_string(),
        },
        audit_record: audit_for(105),

        model: ParticipationModel::PrivateTeam,
        now_unix_ms: NOW,
    }
}

/// 조건 2 의 관문만 열어 둔 요청 — 다른 조건을 하나씩 무너뜨리는 기준선.
fn gate_open() -> ReassignmentRequest {
    let mut request = everything_but_the_enforcement_gate();
    request.partition_pause = PartitionPauseEnforcement::EnforcedByCaller;
    request
}

fn refusals(request: &ReassignmentRequest) -> Vec<UnmetCondition> {
    match evaluate_reassignment(request, &policy()).expect("입력은 유효하다") {
        ReassignmentDecision::Refused { unmet } => unmet,
        ReassignmentDecision::Allowed { fence_epoch } => {
            panic!("허용되면 안 되는 요청이 허용됐다 (fence={fence_epoch})")
        }
    }
}

fn allowed(request: &ReassignmentRequest, epoch: u64) {
    assert_eq!(
        evaluate_reassignment(request, &policy()).expect("입력은 유효하다"),
        ReassignmentDecision::Allowed { fence_epoch: epoch },
    );
}

// ---------------------------------------------------------------------------
// ★ 가장 중요한 것 — 오늘은 아무것도 허용되지 않는다
// ---------------------------------------------------------------------------

/// **나머지가 전부 채워져도 거부된다.**
///
/// `ADR-033` §8 이 조건 2 에 달아 둔 단서를 값으로 옮긴 결과다 —
/// 만료된 노드가 스스로 멈추는 것을 강제하는 코드가 없으므로, 오늘의
/// 정직한 호출은 전부 여기서 막힌다.
///
/// ★ **이 테스트가 증명하지 않는 것** — "오늘 아무도 재배정하지 못한다"
///   는 이 테스트가 아니라 **production 호출부가 하나도 없다는 사실**이
///   보장한다(2026-08-30 독립 검수 5라운드 지적). 이 테스트는 fixture 에
///   `NotEnforcedYet` 를 고정하므로, 내일 진짜 강제 수단이 생겨도 계속
///   통과한다. 여기서 고정하는 것은 **그 값을 넘기면 반드시 거부된다** 는
///   커널의 동작 하나뿐이다.
#[test]
fn todays_honest_caller_is_always_refused() {
    let unmet = refusals(&everything_but_the_enforcement_gate());

    assert_eq!(
        unmet,
        vec![UnmetCondition::PartitionPauseNotEnforced],
        "막는 이유가 정확히 조건 2 의 강제 부재 하나여야 한다"
    );
}

/// 강제 수단이 생겼다고 **진술하면** 허용된다.
///
/// 위 테스트가 "다른 조건을 못 채워서" 거부하는 게 아님을 증명하는 대조
/// 케이스다. 이게 없으면 위 테스트는 공허하다 — 아무 이유로나 거부해도
/// 통과한다.
#[test]
fn with_the_missing_enforcement_asserted_the_rest_does_pass() {
    allowed(&gate_open(), 105);
}

// ---------------------------------------------------------------------------
// 여섯 조건 각각이 실제로 판정에 쓰이는가
// ---------------------------------------------------------------------------

#[test]
fn condition_1_broker_silence_below_the_threshold_refuses() {
    let mut request = gate_open();
    request.broker_silent_for_ms = 59_999;

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::BrokerSilenceTooShort {
            observed_ms: 59_999,
            required_ms: 60_000,
        }],
    );
}

/// 경계 — `ADR-033` §8 조건 1 은 "정한 시간을 **넘겼다**" 이므로 정확히
/// 같은 값은 **충족이 아니다**.
///
/// ★ 초안은 `>=` 로 같은 값을 통과시켰고 독립 검수가 ADR 원문과 어긋난다고
///   짚었다. 만료 시각 판정의 `<=` 규칙과 헷갈리기 쉬운데, 저건 "시각이
///   지났는가" 이고 이건 "기간을 넘겼는가" 다 — 다른 물음이다.
#[test]
fn condition_1_is_not_met_exactly_at_the_threshold() {
    let mut request = gate_open();
    request.broker_silent_for_ms = 60_000;

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::BrokerSilenceTooShort {
            observed_ms: 60_000,
            required_ms: 60_000,
        }],
        "'넘겼다' 이므로 같은 값은 충족이 아니다"
    );
}

#[test]
fn condition_1_is_met_one_millisecond_past_the_threshold() {
    let mut request = gate_open();
    request.broker_silent_for_ms = 60_001;

    allowed(&request, 105);
}

#[test]
fn condition_2_lease_not_yet_expired_refuses() {
    let mut request = gate_open();
    request.original_lease_expires_at_unix_ms = NOW + 1;

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::OriginalLeaseStillValid {
            expires_at_unix_ms: NOW + 1,
            now_unix_ms: NOW,
        }],
    );
}

/// 경계 — `now == expires_at` 은 **만료된 것으로 본다**.
///
/// `DoD-26`·`DoD-32`·`DoD-34` 가 정착시킨 `<=` 규칙과 같다. 여기서만
/// 엄격한 `<` 를 쓰면 정확히 그 순간 계층 간 모순이 생긴다.
#[test]
fn condition_2_treats_the_exact_expiry_instant_as_expired() {
    let mut request = gate_open();
    request.original_lease_expires_at_unix_ms = NOW;

    allowed(&request, 105);
}

#[test]
fn condition_3_not_enough_distinct_reporters_refuses() {
    let mut request = gate_open();
    request.neighbor_reports = vec![report("member-a", NOW - 500)];

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::NotEnoughNeighborReports {
            distinct: 1,
            required: 2,
        }],
    );
}

/// 같은 **기계**가 여러 번 신고해도 한 표다.
///
/// ★ 처음엔 "같은 멤버가 여러 번 신고해도" 라고 썼는데 사실과 다르다 —
///   정족수는 기계 수이므로 같은 멤버가 **다른 기계 두 대**로 신고하면
///   두 표다(`the_quorum_counts_machines_not_members` 가 그걸 확인한다).
///   2026-08-30 독립 검수 7라운드가 이 설명의 오도를 짚었다.
#[test]
fn condition_3_counts_a_repeat_machine_once() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report("member-a", NOW - 400),
        report("member-a", NOW - 300),
    ];

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::NotEnoughNeighborReports {
            distinct: 1,
            required: 2,
        }],
        "같은 기계가 세 번 신고해도 서로 다른 기계는 하나다"
    );
}

#[test]
fn condition_4_withheld_consent_refuses() {
    let mut request = gate_open();
    request.receiving_consent = ReceivingNodeConsent::Withheld;

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::ReceivingNodeDidNotConsent],
    );
}

#[test]
fn condition_5_fence_epoch_outside_the_reserved_range_refuses() {
    let mut request = gate_open();
    request.proposed_fence_epoch = 111;
    request.audit_record = audit_for(111);

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::FenceEpochOutsideReservedRange {
            proposed: 111,
            first: 101,
            last: 110,
        }],
    );
}

/// 범위의 양 끝은 **포함**이다.
#[test]
fn condition_5_accepts_both_ends_of_the_reserved_range() {
    for epoch in [101_u64, 110] {
        let mut request = gate_open();
        request.proposed_fence_epoch = epoch;
        request.audit_record = audit_for(epoch);
        allowed(&request, epoch);
    }
}

/// 이미 쓴 번호는 재사용하지 않는다 — 재사용하면 fencing 이 무의미해진다.
#[test]
fn condition_5_refuses_an_already_used_epoch_from_the_range() {
    let mut request = gate_open();
    request.reserved_fence_range.already_used = BTreeSet::from([105]);

    // 이미 쓴 번호는 곧 "가장 높이 관측된 번호보다 높지 않다" 이기도 하다.
    // 두 사실이 다 참이므로 둘 다 나온다 — 하나만 나오면 그게 오히려 결함이다.
    assert_eq!(
        refusals(&request),
        vec![
            UnmetCondition::FenceEpochAlreadyUsed { proposed: 105 },
            UnmetCondition::FenceEpochNotAboveHighestSeen {
                proposed: 105,
                must_exceed: 105,
            },
        ],
    );
}

/// 조건 6 은 "기록할 수 있다" 가 아니라 **"기록했다"** 다.
///
/// ★ 초안은 `Recordable`(미래의 가능성)만 보고 허용했다 — 독립 검수가
///   `ADR-033` §8 조건 6 원문("이 재할당이 감사 로그에 기록된다")과
///   어긋난다고 짚었다. 이제 이미 확정된 기록의 id 를 요구한다.
#[test]
fn condition_6_an_unwritten_audit_record_refuses() {
    let mut request = gate_open();
    request.audit_record = AuditRecord::NotRecorded;

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::AuditRecordNotCommitted],
    );
}

/// 감사 기록 id 도 **다른 식별자와 같은 규칙**으로 본다.
///
/// * 초안은 여기만 `trim().is_empty()` 였다 - 규칙이 다를 이유가 없는데
///   달랐고 독립 검수 2라운드가 누락으로 짚었다. `" audit 1"` 같은 값이
///   통과했다.
#[test]
fn condition_6_a_non_canonical_audit_record_id_is_an_input_error() {
    for bad in ["   ", "audit 1", "audit-\u{00e9}"] {
        let mut request = gate_open();
        request.audit_record = AuditRecord::Committed {
            record_id: bad.to_string(),
            recorded_job_id: JOB_ID.to_string(),
            recorded_attempt_id: ATTEMPT_ID.to_string(),
            recorded_fence_epoch: 105,
        };

        assert_eq!(
            evaluate_reassignment(&request, &policy()),
            Err(ReassignmentInputError::NonCanonicalIdentifier {
                field: "audit_record_id",
                value: bad.to_string(),
            }),
            "{bad:?} 는 거부해야 한다"
        );
    }
}

/// 요청과 **다른 값이 적힌** 감사 기록은 붙일 수 없다.
///
/// ★ "다른 재할당의 기록을 가져다 붙일 수 없다" 는 과장이다(2026-08-30
///   독립 검수 8라운드 지적). `record_id` 와 나머지 필드 사이에 검증된
///   결합이 없으므로, 다른 기록의 id 에 이 요청의 값을 적어 넣으면
///   통과한다. 여기서 고정하는 것은 **값이 어긋난 기록의 거부** 하나다.
#[test]
fn an_audit_record_whose_fields_do_not_match_the_request_is_an_input_error() {
    let cases = [
        (OTHER_JOB_ID, ATTEMPT_ID, 105_u64),
        (JOB_ID, OTHER_ATTEMPT_ID, 105),
        (JOB_ID, ATTEMPT_ID, 106),
    ];
    for (job, attempt, fence) in cases {
        let mut request = gate_open();
        request.audit_record = AuditRecord::Committed {
            record_id: "audit-1".to_string(),
            recorded_job_id: job.to_string(),
            recorded_attempt_id: attempt.to_string(),
            recorded_fence_epoch: fence,
        };

        assert_eq!(
            evaluate_reassignment(&request, &policy()),
            Err(ReassignmentInputError::AuditRecordDoesNotMatchRequest {
                record_id: "audit-1".to_string(),
                recorded_job_id: job.to_string(),
                recorded_attempt_id: attempt.to_string(),
                recorded_fence_epoch: fence,
            }),
            "({job}, {attempt}, {fence}) 는 이 요청의 기록이 아니다"
        );
    }
}

/// **다른 Job 용으로 발행된 예비 범위**를 가져다 쓸 수 없다.
///
/// `ADR-033` 8 조건 5 는 "**이 Job 의** failover 범위" 다.
#[test]
fn a_fence_range_issued_for_another_attempt_is_an_input_error() {
    for (job, attempt) in [(OTHER_JOB_ID, ATTEMPT_ID), (JOB_ID, OTHER_ATTEMPT_ID)] {
        let mut request = gate_open();
        request.reserved_fence_range.issued_for_job_id = job.to_string();
        request.reserved_fence_range.issued_for_attempt_id = attempt.to_string();

        assert_eq!(
            evaluate_reassignment(&request, &policy()),
            Err(ReassignmentInputError::FenceRangeIssuedForAnotherAttempt {
                issued_for_job_id: job.to_string(),
                issued_for_attempt_id: attempt.to_string(),
            }),
        );
    }
}

// ---------------------------------------------------------------------------
// 정책으로 조건을 지워 버릴 수 없다
// ---------------------------------------------------------------------------

/// 침묵 임계값 0 은 조건 1 을 사실상 없앤다 — 판정하지 않는다.
#[test]
fn a_zero_silence_threshold_is_a_policy_error() {
    let mut bad = policy();
    bad.broker_silence_threshold_ms = 0;

    assert_eq!(
        evaluate_reassignment(&gate_open(), &bad),
        Err(ReassignmentInputError::BrokerSilenceThresholdIsZero),
    );
}

/// 정족수 0 은 조건 3 을 사실상 없앤다 — 신고가 하나도 없어도 통과하게 된다.
#[test]
fn a_zero_neighbor_quorum_is_a_policy_error() {
    for (private_team, public_pool) in [(0_u32, 4_u32), (2, 0)] {
        let mut bad = policy();
        bad.private_team_required_reports = private_team;
        bad.public_pool_required_reports = public_pool;

        assert_eq!(
            evaluate_reassignment(&gate_open(), &bad),
            Err(ReassignmentInputError::RequiredNeighborReportsIsZero),
            "정족수 ({private_team}, {public_pool}) 은 거부돼야 한다"
        );
    }
}

/// 공개 풀 정족수가 사설 팀보다 높지 않으면 **판정 자체를 거부한다.**
#[test]
fn a_public_pool_quorum_that_is_not_higher_is_a_policy_error() {
    let mut bad = policy();
    bad.public_pool_required_reports = 2;

    assert_eq!(
        evaluate_reassignment(&gate_open(), &bad),
        Err(ReassignmentInputError::PublicPoolQuorumNotHigher {
            public_pool: 2,
            private_team: 2,
        }),
    );
}

// ---------------------------------------------------------------------------
// 진술 관문 — 확인 안 한 것을 확인했다고 적지 않으면 통과 못 한다
// ---------------------------------------------------------------------------

/// 서명·멤버십이 해소되지 않은 신고는 **세지 않고 거부한다.**
#[test]
fn an_unresolved_neighbor_report_is_an_input_error() {
    let mut request = gate_open();
    request.neighbor_reports[1].resolution = NeighborReportResolution::Unresolved;

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::UnresolvedNeighborReport {
            reporter_member_id: "member-b".to_string(),
        }),
    );
}

/// Broker 서명이 확인되지 않은 예비 범위로는 판정하지 않는다.
///
/// ★ 확인 안 한 범위를 받으면 호출자가 `first=0, last=u64::MAX` 를 지어내
///   어떤 번호든 통과시킬 수 있다(독립 검수 지적). 이 관문은 그걸 막지는
///   못하지만, **거짓 진술 없이는 통과할 수 없게** 만든다.
#[test]
fn an_unproven_fence_range_is_an_input_error() {
    let mut request = gate_open();
    request.reserved_fence_range.issuance = FenceRangeIssuance::Unproven;

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::UnprovenFenceRange),
    );
}

/// 예비 범위가 원래 fence 번호보다 위가 아니면 fencing 이 무의미하다.
/// 범위 **전체**가 원래 번호 이하면 이 Job 의 failover 범위일 수 없다.
///
/// ★ 초안은 `first` 를 봤는데, 그러면 `ADR-033` §8 의 "범위를 다 쓸 때까지
///   재할당" 과 어긋난다 — 정직하게 현재 번호를 넣으면 범위 시작이 그보다
///   낮아 입력 오류가 났다(독립 검수 3라운드 지적).
#[test]
fn a_reserved_range_entirely_at_or_below_the_original_epoch_is_an_input_error() {
    for last in [99_u64, 100] {
        let mut request = gate_open();
        request.reserved_fence_range.first = 90;
        request.reserved_fence_range.last = last;

        assert_eq!(
            evaluate_reassignment(&request, &policy()),
            Err(ReassignmentInputError::ReservedFenceRangeNotAboveOriginal {
                last,
                original: 100,
            }),
            "범위 끝 {last} 은 원래 epoch 100 보다 커야 한다"
        );
    }
}

/// 조건 5 의 세 검사는 **서로 독립**이다 — 겹치면 겹친 대로 전부 나온다.
///
/// ★ 초안은 `else if` 로 엮여 있어 "이미 썼다" 가 "더 높아야 한다" 를
///   가렸다(2026-08-30 독립 검수 4라운드 지적). 하나만 보고하면 운영자가
///   그것만 고치고 다시 막힌다.
#[test]
fn overlapping_fence_problems_are_all_reported() {
    let mut request = gate_open();
    request.reserved_fence_range.already_used = BTreeSet::from([104, 105]);
    request.proposed_fence_epoch = 104;
    request.audit_record = audit_for(104);

    // ★ `contains` 가 아니라 **전체를 정확히** 비교한다 — `contains` 만
    //   보면 사실과 다른 진단이 하나 더 끼어도 통과한다(독립 검수 5라운드
    //   지적).
    assert_eq!(
        refusals(&request),
        vec![
            UnmetCondition::FenceEpochAlreadyUsed { proposed: 104 },
            UnmetCondition::FenceEpochNotAboveHighestSeen {
                proposed: 104,
                must_exceed: 105,
            },
        ],
    );

    // 범위 **밖**이면서 낮기도 한 경우도 둘 다, 그리고 그 둘만 나온다.
    let mut both = gate_open();
    both.reserved_fence_range.already_used = BTreeSet::from([108]);
    both.proposed_fence_epoch = 100;
    both.audit_record = audit_for(100);

    assert_eq!(
        refusals(&both),
        vec![
            UnmetCondition::FenceEpochOutsideReservedRange {
                proposed: 100,
                first: 101,
                last: 110,
            },
            UnmetCondition::FenceEpochNotAboveHighestSeen {
                proposed: 100,
                must_exceed: 108,
            },
        ],
    );
}

/// **범위를 이어서 쓸 수 있다** — 원래 번호가 범위 안으로 올라와도 막히지 않는다.
///
/// `ADR-033` §8 의 "범위를 다 쓰면 더 이상 재할당할 수 없다" 는 곧 **다 쓰기
/// 전까지는 쓸 수 있다** 는 뜻이다. `first` 기준으로 막으면 두 번째 재할당이
/// 아예 불가능했다.
#[test]
fn a_second_failover_within_the_same_range_is_still_possible() {
    let mut request = gate_open();
    request.original_fence_epoch = 105;
    request.reserved_fence_range.already_used = BTreeSet::from([105]);
    request.proposed_fence_epoch = 106;
    request.audit_record = audit_for(106);

    allowed(&request, 106);
}

/// ★ **이미 관측된 번호보다 낮은 번호는 거부한다.**
///
/// 초안은 `original=100`, 범위 `101..=110`, 이미 쓴 `{105}` 에서 `104` 를
/// 허용했다 — 105 가 이미 돌았는데 104 로 되돌아가는 것이고, 그건
/// `crates/protocol/src/fenced_operation.rs` 가 `StaleFence` 로 거부하는
/// 상황이다(2026-08-30 독립 검수 3라운드가 찾은 진짜 결함).
#[test]
fn a_fence_epoch_below_an_already_used_one_is_refused() {
    let mut request = gate_open();
    request.reserved_fence_range.already_used = BTreeSet::from([105]);
    request.proposed_fence_epoch = 104;
    request.audit_record = audit_for(104);

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::FenceEpochNotAboveHighestSeen {
            proposed: 104,
            must_exceed: 105,
        }],
        "이미 105 가 돌았으면 104 로 되돌아갈 수 없다"
    );
}

/// job/attempt ID 는 26자 ULID 여야 한다 — `fenced_operation.rs` 와 같은 규칙.
#[test]
fn job_and_attempt_ids_must_be_ulid_length() {
    let cases: Vec<(&'static str, Box<dyn Fn(&mut ReassignmentRequest)>)> = vec![
        (
            "job_id",
            Box::new(|r: &mut ReassignmentRequest| r.job_id = "job-1".to_string()),
        ),
        (
            "attempt_id",
            Box::new(|r: &mut ReassignmentRequest| r.attempt_id = "attempt-1".to_string()),
        ),
    ];

    for (field, mutate) in cases {
        let mut request = gate_open();
        mutate(&mut request);

        assert_eq!(
            evaluate_reassignment(&request, &policy()),
            Err(ReassignmentInputError::IdentifierIsNotUlidLength {
                field,
                actual: if field == "job_id" { 5 } else { 9 },
            }),
            "{field} 는 26자여야 한다"
        );
    }
}

/// 대소문자만 다른 job ID 는 "다른 Job" 이라고 **단정하지 않는다.**
#[test]
fn a_case_only_difference_in_the_range_job_id_is_ambiguous_not_another_job() {
    let mut request = gate_open();
    request.reserved_fence_range.issued_for_job_id = JOB_ID.to_ascii_lowercase();

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::AmbiguousIdentifier {
            field: "issued_for_job_id",
            first: JOB_ID.to_string(),
            second: JOB_ID.to_ascii_lowercase(),
        }),
    );
}

// ---------------------------------------------------------------------------
// 모델 관문
// ---------------------------------------------------------------------------

/// 공개 풀은 기본 금지다 — 조건을 다 채워도 정책이 안 열려 있으면 거부.
#[test]
fn a_public_pool_is_refused_by_default_even_with_every_condition_met() {
    let mut request = gate_open();
    request.model = ParticipationModel::PublicPool;
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report("member-b", NOW - 400),
        report("member-c", NOW - 300),
        report("member-d", NOW - 200),
    ];

    assert_eq!(
        refusals(&request),
        vec![UnmetCondition::PublicPoolReassignmentDisabled],
    );
}

/// 공개 풀은 **더 높은 정족수**를 요구한다.
#[test]
fn a_public_pool_demands_the_higher_quorum() {
    let mut request = gate_open();
    request.model = ParticipationModel::PublicPool;

    let mut open_policy = policy();
    open_policy.public_pool_enabled = true;

    let unmet = match evaluate_reassignment(&request, &open_policy).expect("입력은 유효하다")
    {
        ReassignmentDecision::Refused { unmet } => unmet,
        other => panic!("거부돼야 한다: {other:?}"),
    };
    assert_eq!(
        unmet,
        vec![UnmetCondition::NotEnoughNeighborReports {
            distinct: 2,
            required: 4,
        }],
        "사설 팀 정족수 2 로는 공개 풀을 통과하지 못한다"
    );
}

// ---------------------------------------------------------------------------
// 자기편향 분리 — `ADR-033` §8 이 조건 4 만으로 부족하다고 한 이유
// ---------------------------------------------------------------------------

/// **이득 보는 쪽이 스스로 신고하면 판정하지 않는다.**
#[test]
fn a_report_from_the_beneficiary_is_rejected_outright() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report("member-receiver", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ReportFromBeneficiary {
            member_id: "member-receiver".to_string(),
        }),
    );
}

/// **대소문자를 바꿔도 못 빠져나간다 - 다만 "같다" 고 단정하지도 않는다.**
///
/// * 초안은 바이트 비교만 해서 `Member-Receiver` 가 통과했다. 1차 수정은
///   대소문자 무시 비교로 바꿨는데, 그건 이 저장소에 없는 규칙을 발명한
///   것이었다 - 정본 멤버십 판정(`crates/protocol/src/membership.rs`)은
///   정확한 비교를 쓰고 판단 불가 시 `Ambiguous` 로 fail closed 한다
///   (독립 검수 2라운드 지적). 그래서 여기도 **추측하지 않고 거부한다.**
#[test]
fn the_beneficiary_cannot_evade_the_check_by_changing_case() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report("Member-Receiver", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::AmbiguousIdentifier {
            field: "reporter_member_id/receiving_member_id",
            first: "member-receiver".to_string(),
            second: "Member-Receiver".to_string(),
        }),
    );
}

/// **앞뒤 공백으로도 못 빠져나간다** — 다듬지 않고 거부한다.
///
/// 다듬어서 받아 주면 `" member-receiver"` 가 조용히 같아지는데, 그
/// 조용함이 바로 우회 통로다. 여기서는 아예 규범 형태가 아니라고 거부한다.
#[test]
fn the_beneficiary_cannot_evade_the_check_by_padding_whitespace() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report_from("node-b", " member-receiver", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::NonCanonicalIdentifier {
            field: "reporter_member_id",
            value: " member-receiver".to_string(),
        }),
    );
}

/// 비-ASCII 식별자는 받지 않는다 — 유니코드 정규화 차이로 같은 사람이
/// 다른 사람인 척할 수 있다.
///
/// 정규화 규칙을 여기서 발명하지 않고, 정규화가 문제되지 않는 범위만 받는다.
#[test]
fn non_ascii_identifiers_are_refused_rather_than_normalised() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report_from("node-b", "member-\u{00e9}", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::NonCanonicalIdentifier {
            field: "reporter_member_id",
            value: "member-\u{00e9}".to_string(),
        }),
    );
}

/// 대소문자만 다른 두 신고자는 **추측하지 않고 거부한다.**
///
/// 같다고 보면 정당한 표를 잃고, 다르다고 보면 한 명이 정족수를 혼자 채운다.
#[test]
fn two_reporters_differing_only_in_case_are_ambiguous_not_two_votes() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report_from("node-a", "member-a", NOW - 500),
        report_from("Node-A", "member-b", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::AmbiguousIdentifier {
            field: "reporter_node_id",
            first: "Node-A".to_string(),
            second: "node-a".to_string(),
        }),
    );
}

/// ★ **정족수는 멤버가 아니라 기계 수다**(`ADR-033` §8 조건 3 "이웃 N 대").
///
/// 한 멤버가 기계 두 대로 신고하면 **두 표**다 — 초안은 멤버로 세서 한
/// 표였다(2026-08-30 독립 검수 5라운드 지적).
#[test]
fn the_quorum_counts_machines_not_members() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report_from("node-a1", "member-a", NOW - 500),
        report_from("node-a2", "member-a", NOW - 400),
    ];

    allowed(&request, 105);
}

/// 반대 방향 — 한 기계를 두 멤버가 자기 것이라 하면 **판정하지 않는다.**
///
/// ★ 처음엔 이 테스트를 "한 표다" 라고 불렀는데, 실제로 확인하는 것은
///   정족수가 아니라 소유권 충돌 거부다(2026-08-30 독립 검수 6라운드가
///   이름과 assert 의 불일치를 짚었다). 같은 기계·같은 소유자의 중복이
///   한 표가 되는 것은 `condition_3_counts_a_repeat_machine_once` 가
///   확인한다.
#[test]
fn two_members_claiming_one_machine_is_an_ownership_conflict() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report_from("node-a", "member-a", NOW - 500),
        report_from("node-a", "member-b", NOW - 400),
    ];

    // 같은 기계가 서로 다른 소유자로 신고됐다 — 어느 쪽이 맞는지 모른다.
    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ConflictingReporterOwnership {
            reporter_node_id: "node-a".to_string(),
            first_member_id: "member-a".to_string(),
            second_member_id: "member-b".to_string(),
        }),
    );
}

/// 소유자 ID 가 **대소문자만 다르면** "서로 다른 멤버" 라고 단정하지 않는다.
///
/// ★ 초안은 단순 `!=` 비교라 `Member-A` 와 `member-a` 를 다른 멤버로
///   보고했다 — 이 파일의 다른 식별자 비교와 규칙이 어긋났고 오류가
///   사실을 단정했다(2026-08-30 독립 검수 8라운드 지적).
#[test]
fn a_case_only_difference_in_machine_ownership_is_ambiguous_not_a_conflict() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report_from("node-a", "member-a", NOW - 500),
        report_from("node-a", "Member-A", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::AmbiguousIdentifier {
            field: "reporter_member_id",
            first: "Member-A".to_string(),
            second: "member-a".to_string(),
        }),
    );
}

/// 지목당한 기계나 받을 기계가 스스로 신고할 수 없다.
///
/// ★ 둘은 **다른 오류**로 보고된다. 지목당한 기계가 스스로 신고하는 것은
///   "이득 보는 쪽" 이 아니다 — 같은 이름을 쓰면 오류가 사실을 잘못
///   전한다(2026-08-30 독립 검수 6라운드 지적).
#[test]
fn the_stranded_machine_reporting_itself_is_not_called_a_beneficiary() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report_from("node-stranded", "member-c", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ReportFromStrandedNode {
            reporter_node_id: "node-stranded".to_string(),
        }),
    );
}

/// 받을 기계가 신고하는 것은 **이득 보는 쪽의 자기 신고**가 맞다.
#[test]
fn the_receiving_machine_cannot_report() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report_from("node-receiver", "member-c", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ReportFromReceivingNode {
            reporter_node_id: "node-receiver".to_string(),
        }),
    );
}

/// 사용 이력에 범위 **밖** 번호가 있으면 판정하지 않는다.
///
/// ★ 초안은 조용히 받아들여 `highest_seen` 을 끌어올렸다 — 범위 `101..=110`
///   에 `already_used={120}` 이면 어떤 번호도 통과 못 하는데 그 이유가
///   "범위 밖 이력" 이라고 드러나지 않았다(독립 검수 5라운드 지적).
#[test]
fn an_already_used_epoch_outside_the_range_is_an_input_error() {
    for outside in [100_u64, 120] {
        let mut request = gate_open();
        request.reserved_fence_range.already_used = BTreeSet::from([outside]);

        assert_eq!(
            evaluate_reassignment(&request, &policy()),
            Err(ReassignmentInputError::AlreadyUsedEpochOutsideRange {
                epoch: outside,
                first: 101,
                last: 110,
            }),
        );
    }
}

/// **인접하지 않은** 쌍도 잡는다.
///
/// 정렬 후 두 값 사이에 다른 신고자가 끼어도, 이전 원소를 전부 보므로
/// 놓치지 않는다.
#[test]
fn ambiguous_reporters_are_caught_even_when_not_adjacent_after_sorting() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report_from("NODE-a", "member-a", NOW - 500),
        report_from("Znode", "member-b", NOW - 450),
        report_from("node-a", "member-c", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::AmbiguousIdentifier {
            field: "reporter_node_id",
            first: "NODE-a".to_string(),
            second: "node-a".to_string(),
        }),
        "정렬하면 사이에 Znode 가 끼는데도 잡아야 한다"
    );
}

/// 지목당한 노드의 소유자가 낸 신고도 거부한다.
#[test]
fn a_report_from_the_stranded_member_is_rejected_outright() {
    let mut request = gate_open();
    request.neighbor_reports = vec![
        report("member-a", NOW - 500),
        report("member-stranded", NOW - 400),
    ];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ReportFromStrandedMember {
            member_id: "member-stranded".to_string(),
        }),
    );
}

// ---------------------------------------------------------------------------
// 입력 검증 — 조용히 무시하지 않는다
// ---------------------------------------------------------------------------

/// 다른 노드에 대한 신고는 **세지 않고 거부한다.**
#[test]
fn a_report_about_another_node_is_an_input_error_not_a_silent_skip() {
    let mut request = gate_open();
    request.neighbor_reports.push(NeighborUnreachableReport {
        reporter_node_id: "node-of-member-c".to_string(),
        reporter_member_id: "member-c".to_string(),
        unreachable_node_id: "node-someone-else".to_string(),
        observed_at_unix_ms: NOW - 100,
        resolution: NeighborReportResolution::SignatureAndMembershipVerifiedByCaller,
    });

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ReportTargetsAnotherNode {
            reporter_member_id: "member-c".to_string(),
            reported_node_id: "node-someone-else".to_string(),
            expected_node_id: "node-stranded".to_string(),
        }),
    );
}

#[test]
fn a_report_observed_in_the_future_is_an_input_error() {
    let mut request = gate_open();
    request.neighbor_reports = vec![report("member-a", NOW + 1), report("member-b", NOW - 400)];

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ReportObservedInFuture {
            reporter_member_id: "member-a".to_string(),
            observed_at_unix_ms: NOW + 1,
            now_unix_ms: NOW,
        }),
    );
}

/// 비거나 공백이 섞인 식별자로 검사를 우회할 수 없다.
#[test]
fn non_canonical_identifiers_fail_closed() {
    let cases: Vec<(
        &'static str,
        &'static str,
        Box<dyn Fn(&mut ReassignmentRequest)>,
    )> = vec![
        (
            "receiving_node_id",
            "  ",
            Box::new(|r: &mut ReassignmentRequest| r.receiving_node_id = "  ".to_string()),
        ),
        (
            "receiving_member_id",
            "",
            Box::new(|r: &mut ReassignmentRequest| r.receiving_member_id = String::new()),
        ),
        (
            "reporter_member_id",
            "\t",
            Box::new(|r: &mut ReassignmentRequest| {
                r.neighbor_reports[0].reporter_member_id = "\t".to_string()
            }),
        ),
        (
            "stranded_node_id",
            "node stranded",
            Box::new(|r: &mut ReassignmentRequest| {
                r.stranded_node_id = "node stranded".to_string()
            }),
        ),
    ];

    for (field, value, mutate) in cases {
        let mut request = gate_open();
        mutate(&mut request);

        assert_eq!(
            evaluate_reassignment(&request, &policy()),
            Err(ReassignmentInputError::NonCanonicalIdentifier {
                field,
                value: value.to_string(),
            }),
            "{field} = {value:?} 는 거부해야 한다"
        );
    }
}

/// 원래 있던 곳으로 "옮기는" 것은 재배정이 아니다 — 대소문자도 같이 본다.
#[test]
fn moving_the_job_to_the_node_it_is_already_on_is_an_input_error() {
    let mut request = gate_open();
    request.receiving_node_id = "node-stranded".to_string();
    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ReceivingNodeIsStrandedNode {
            node_id: "node-stranded".to_string(),
        }),
    );

    // 대소문자만 다르면 같은 노드인지 이 커널은 모른다 - 거부한다.
    let mut ambiguous = gate_open();
    ambiguous.receiving_node_id = "NODE-STRANDED".to_string();
    assert_eq!(
        evaluate_reassignment(&ambiguous, &policy()),
        Err(ReassignmentInputError::AmbiguousIdentifier {
            field: "receiving_node_id",
            first: "node-stranded".to_string(),
            second: "NODE-STRANDED".to_string(),
        }),
    );
}

#[test]
fn an_inverted_reserved_range_is_an_input_error() {
    let mut request = gate_open();
    request.reserved_fence_range.first = 110;
    request.reserved_fence_range.last = 101;

    assert_eq!(
        evaluate_reassignment(&request, &policy()),
        Err(ReassignmentInputError::ReservedFenceRangeInverted {
            first: 110,
            last: 101,
        }),
    );
}

// ---------------------------------------------------------------------------
// 결정성 · 전부 보고
// ---------------------------------------------------------------------------

/// 안 맞은 조건을 **전부** 돌려준다.
#[test]
fn every_unmet_condition_is_reported_not_just_the_first() {
    let mut request = everything_but_the_enforcement_gate();
    request.broker_silent_for_ms = 0;
    request.original_lease_expires_at_unix_ms = NOW + 10_000;
    request.neighbor_reports = vec![];
    request.receiving_consent = ReceivingNodeConsent::Withheld;
    request.proposed_fence_epoch = 999;
    request.audit_record = AuditRecord::NotRecorded;

    let unmet = refusals(&request);

    assert_eq!(
        unmet.len(),
        7,
        "여섯 조건 + 조건 2 의 강제 부재까지 전부 나와야 한다: {unmet:?}"
    );
    for expected in [
        UnmetCondition::BrokerSilenceTooShort {
            observed_ms: 0,
            required_ms: 60_000,
        },
        UnmetCondition::OriginalLeaseStillValid {
            expires_at_unix_ms: NOW + 10_000,
            now_unix_ms: NOW,
        },
        UnmetCondition::PartitionPauseNotEnforced,
        UnmetCondition::NotEnoughNeighborReports {
            distinct: 0,
            required: 2,
        },
        UnmetCondition::ReceivingNodeDidNotConsent,
        UnmetCondition::FenceEpochOutsideReservedRange {
            proposed: 999,
            first: 101,
            last: 110,
        },
        UnmetCondition::AuditRecordNotCommitted,
    ] {
        assert!(
            unmet.contains(&expected),
            "{expected:?} 가 빠졌다: {unmet:?}"
        );
    }
}

/// 신고 순서를 바꿔도 **성공과 실패 양쪽 다** 같은 답이 나온다.
#[test]
fn the_answer_does_not_depend_on_report_order() {
    let members = ["member-a", "member-b", "member-c"];

    for enforced in [
        PartitionPauseEnforcement::EnforcedByCaller,
        PartitionPauseEnforcement::NotEnforcedYet,
    ] {
        let mut baseline: Option<Result<ReassignmentDecision, ReassignmentInputError>> = None;

        // 3! = 6 개 순열.
        for permutation in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let mut request = everything_but_the_enforcement_gate();
            request.partition_pause = enforced;
            request.neighbor_reports = permutation
                .iter()
                .map(|&index| report(members[index], NOW - 500 + index as u64))
                .collect();

            let answer = evaluate_reassignment(&request, &policy());
            match &baseline {
                None => baseline = Some(answer),
                Some(first) => assert_eq!(
                    &answer, first,
                    "순서 {permutation:?} 에서 답이 달라졌다 (enforced={enforced:?})"
                ),
            }
        }
    }
}

/// 오류도 순서와 무관하다.
#[test]
fn errors_do_not_depend_on_report_order_either() {
    let bad_a = report("member-receiver", NOW - 100);
    let bad_b = report("member-stranded", NOW - 200);

    let mut forward = gate_open();
    forward.neighbor_reports = vec![bad_a.clone(), bad_b.clone()];

    let mut backward = gate_open();
    backward.neighbor_reports = vec![bad_b, bad_a];

    assert_eq!(
        evaluate_reassignment(&forward, &policy()),
        evaluate_reassignment(&backward, &policy()),
        "두 가지 오류가 동시에 있을 때 순서가 답을 바꾸면 안 된다"
    );
}
