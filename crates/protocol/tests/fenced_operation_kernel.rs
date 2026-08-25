use gputeer_protocol::pb::{Digest, FencedOperation, HashAlgorithm};
use gputeer_protocol::{
    derive_operation_id, evaluate_fenced_operation, FencedOperationDecision, FencedOperationKey,
    InvalidFencedOperation,
};

// `common.proto` global rule 5 — ID 는 ULID 26 자. 폭이 고정이라야
// 구분자 없는 연접이 모호하지 않다. 한 글자를 다바이트로 두어 UTF-8
// 인코딩 커버리지도 함께 유지한다(26 자 != 26 바이트).
const JOB_ID: &str = "01JBXJOB00000000000000000가";
const ATTEMPT_ID: &str = "01JBXATT00000000000000000나";

fn normative_id(job_id: &str, attempt_id: &str, operation_seq: u64) -> [u8; 32] {
    let mut input = Vec::new();
    input.extend_from_slice(job_id.as_bytes());
    input.extend_from_slice(attempt_id.as_bytes());
    input.extend_from_slice(&operation_seq.to_be_bytes());
    *blake3::hash(&input).as_bytes()
}

fn operation(fence_epoch: u64, operation_seq: u64) -> FencedOperation {
    FencedOperation {
        fence_epoch,
        operation_id: Some(Digest {
            algo: HashAlgorithm::Blake3256 as i32,
            value: normative_id(JOB_ID, ATTEMPT_ID, operation_seq).to_vec(),
        }),
        operation_seq,
    }
}

fn key(fence_epoch: u64, operation_seq: u64) -> FencedOperationKey {
    FencedOperationKey {
        fence_epoch,
        operation_id: normative_id(JOB_ID, ATTEMPT_ID, operation_seq),
    }
}

fn invalid(reason: InvalidFencedOperation) -> FencedOperationDecision {
    FencedOperationDecision::Invalid { reason }
}

#[test]
fn normative_utf8_concatenation_and_big_endian_sequence_are_derived_directly() {
    let operation_seq = 0x0102_0304_0506_0708;

    assert_eq!(
        derive_operation_id(JOB_ID, ATTEMPT_ID, operation_seq),
        normative_id(JOB_ID, ATTEMPT_ID, operation_seq)
    );

    let mut little_endian_input = Vec::new();
    little_endian_input.extend_from_slice(JOB_ID.as_bytes());
    little_endian_input.extend_from_slice(ATTEMPT_ID.as_bytes());
    little_endian_input.extend_from_slice(&operation_seq.to_le_bytes());
    assert_ne!(
        derive_operation_id(JOB_ID, ATTEMPT_ID, operation_seq),
        *blake3::hash(&little_endian_input).as_bytes()
    );
}

#[test]
fn a_new_valid_key_is_accepted_with_or_without_prior_observations() {
    let first = operation(7, 10);
    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &first, &[]),
        FencedOperationDecision::Accept { key: key(7, 10) }
    );

    let next = operation(7, 11);
    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &next, &[key(7, 10)]),
        FencedOperationDecision::Accept { key: key(7, 11) }
    );
}

#[test]
fn the_exact_fence_and_operation_id_pair_is_an_idempotent_retransmission() {
    let received = operation(7, 10);
    let observed = [key(7, 9), key(7, 10), key(7, 11)];

    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &received, &observed),
        FencedOperationDecision::IdempotentRetransmission { key: key(7, 10) }
    );
}

#[test]
fn a_lower_fence_is_stale_even_when_its_pair_was_previously_observed() {
    let received = operation(7, 10);
    let observed = [key(7, 10), key(9, 20)];

    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &received, &observed),
        FencedOperationDecision::StaleFence {
            received_fence_epoch: 7,
            max_seen_fence_epoch: 9,
        }
    );
}

#[test]
fn a_higher_fence_with_the_same_operation_id_is_a_new_pair() {
    let received = operation(8, 10);

    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &received, &[key(7, 10)]),
        FencedOperationDecision::Accept { key: key(8, 10) }
    );
}

#[test]
fn sequence_monotonicity_is_not_inferred() {
    let received = operation(7, 3);

    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &received, &[key(7, 10)]),
        FencedOperationDecision::Accept { key: key(7, 3) }
    );
}

#[test]
fn a_structurally_valid_but_incorrect_operation_id_is_rejected() {
    let mut received = operation(7, 10);
    received.operation_id.as_mut().unwrap().value = [0xA5; 32].to_vec();

    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &received, &[]),
        invalid(InvalidFencedOperation::OperationIdMismatch)
    );
}

#[test]
fn missing_identity_or_digest_fails_closed() {
    let received = operation(7, 10);
    assert_eq!(
        evaluate_fenced_operation("", ATTEMPT_ID, &received, &[]),
        invalid(InvalidFencedOperation::MissingJobId)
    );
    assert_eq!(
        evaluate_fenced_operation(JOB_ID, "", &received, &[]),
        invalid(InvalidFencedOperation::MissingAttemptId)
    );

    let mut missing_digest = received;
    missing_digest.operation_id = None;
    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &missing_digest, &[]),
        invalid(InvalidFencedOperation::MissingOperationId)
    );
}

#[test]
fn only_an_exact_blake3_256_digest_is_accepted() {
    for algorithm in [
        HashAlgorithm::Unspecified as i32,
        HashAlgorithm::Sha256 as i32,
        99,
    ] {
        let mut received = operation(7, 10);
        received.operation_id.as_mut().unwrap().algo = algorithm;
        assert_eq!(
            evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &received, &[]),
            invalid(InvalidFencedOperation::InvalidOperationIdAlgorithm {
                actual: algorithm,
            })
        );
    }

    for length in [0, 31, 33, 64] {
        let mut received = operation(7, 10);
        received.operation_id.as_mut().unwrap().value = vec![0; length];
        assert_eq!(
            evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &received, &[]),
            invalid(InvalidFencedOperation::InvalidOperationIdLength { actual: length })
        );
    }
}

#[test]
fn invalid_input_is_not_hidden_by_a_stale_fence() {
    let mut received = operation(1, 10);
    received.operation_id.as_mut().unwrap().value = [0x5A; 32].to_vec();

    assert_eq!(
        evaluate_fenced_operation(JOB_ID, ATTEMPT_ID, &received, &[key(9, 20)]),
        invalid(InvalidFencedOperation::OperationIdMismatch)
    );
}

/// 검수가 지적한 실제 충돌 케이스를 고정한다. 구분자 없는 연접은 폭이
/// 고정일 때만 모호하지 않다. 계약을 벗어난 폭이 들어오면 서로 다른
/// 연산이 같은 operation_id 를 만들어 멱등 재전송으로 흡수될 수 있으므로,
/// 해싱 전에 거부해야 한다.
#[test]
fn out_of_contract_identity_widths_are_rejected_before_hashing() {
    // 같은 바이트열로 연접되므로 도출 자체는 실제로 충돌한다.
    assert_eq!(
        derive_operation_id("ab", "c", 1),
        derive_operation_id("a", "bc", 1),
        "구분자 없는 연접은 폭이 고정이 아니면 충돌한다"
    );

    // 그러나 kernel 은 둘 다 해싱 전에 거부한다.
    for (job, attempt) in [("ab", "c"), ("a", "bc")] {
        let op = FencedOperation {
            fence_epoch: 1,
            operation_id: Some(Digest {
                algo: HashAlgorithm::Blake3256 as i32,
                value: normative_id(job, attempt, 1).to_vec(),
            }),
            operation_seq: 1,
        };
        assert_eq!(
            evaluate_fenced_operation(job, attempt, &op, &[]),
            invalid(InvalidFencedOperation::InvalidJobIdLength {
                actual: job.chars().count()
            }),
            "계약을 벗어난 job_id 폭은 거부되어야 한다"
        );
    }
}

/// attempt_id 폭도 독립적으로 검증된다.
#[test]
fn an_out_of_contract_attempt_id_width_is_rejected() {
    let op = operation(1, 1);
    assert_eq!(
        evaluate_fenced_operation(JOB_ID, "short", &op, &[]),
        invalid(InvalidFencedOperation::InvalidAttemptIdLength { actual: 5 }),
    );
}
