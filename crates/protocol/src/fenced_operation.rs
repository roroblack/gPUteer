//! Pure validation and deduplication kernel for fenced operations.
//!
//! The caller supplies identity context and previously observed keys. This module does not read
//! I/O, clocks, randomness, caches, or global state, and it does not impose retention policy.

use crate::canonical::blake3_256;
use crate::pb::{FencedOperation, HashAlgorithm};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct FencedOperationKey {
    pub fence_epoch: u64,
    pub operation_id: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FencedOperationDecision {
    /// This `(fence_epoch, operation_id)` has not previously been observed.
    Accept { key: FencedOperationKey },
    /// The exact key was previously observed and must be handled idempotently.
    IdempotentRetransmission { key: FencedOperationKey },
    /// A higher fence has already been observed. Staleness takes precedence over deduplication.
    StaleFence {
        received_fence_epoch: u64,
        max_seen_fence_epoch: u64,
    },
    Invalid { reason: InvalidFencedOperation },
}

/// `common.proto` global rule 5 — "ID는 별도 명시가 없으면 ULID 26자 문자열".
const ULID_LEN: usize = 26;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidFencedOperation {
    MissingJobId,
    MissingAttemptId,
    /// `common.proto` global rule 5 fixes identities at 26 characters. Any other width would make
    /// the separator-free concatenation in [`derive_operation_id`] ambiguous.
    InvalidJobIdLength { actual: usize },
    InvalidAttemptIdLength { actual: usize },
    MissingOperationId,
    InvalidOperationIdAlgorithm { actual: i32 },
    InvalidOperationIdLength { actual: usize },
    OperationIdMismatch,
}

/// Derives the normative operation ID.
///
/// `job_id` and `attempt_id` are protobuf `string` identities elsewhere in the lease schema.
/// Because the formula does not define another text encoding or normalization, their exact UTF-8
/// bytes (`str::as_bytes`) are concatenated without separators or length prefixes, followed by the
/// eight-byte big-endian encoding of `operation_seq`.
///
/// The concatenation is only unambiguous because `common.proto` global rule 5 fixes both
/// identities at 26 characters. Callers must validate that width; [`evaluate_fenced_operation`]
/// does so before deriving. Feeding out-of-contract widths here would let `("ab", "c")` and
/// `("a", "bc")` collide.
pub fn derive_operation_id(job_id: &str, attempt_id: &str, operation_seq: u64) -> [u8; 32] {
    let mut input = Vec::new();
    input.extend_from_slice(job_id.as_bytes());
    input.extend_from_slice(attempt_id.as_bytes());
    input.extend_from_slice(&operation_seq.to_be_bytes());
    blake3_256(&input)
}

/// Validates and classifies one fenced operation against previously observed keys.
///
/// The supplied digest is never trusted: even a structurally valid BLAKE3-256 digest must equal a
/// fresh derivation from `job_id`, `attempt_id`, and `operation_seq`. Observation order is not
/// significant. This kernel does not infer sequence monotonicity because the protocol defines no
/// ordering rule for `operation_seq`.
pub fn evaluate_fenced_operation(
    job_id: &str,
    attempt_id: &str,
    operation: &FencedOperation,
    previously_observed: &[FencedOperationKey],
) -> FencedOperationDecision {
    if job_id.is_empty() {
        return invalid(InvalidFencedOperation::MissingJobId);
    }
    if attempt_id.is_empty() {
        return invalid(InvalidFencedOperation::MissingAttemptId);
    }
    // `common.proto` global rule 5: identities are 26-character ULIDs unless stated otherwise.
    // The fixed width is what makes the separator-free concatenation unambiguous, so a wrong
    // width is rejected rather than hashed.
    if job_id.chars().count() != ULID_LEN {
        return invalid(InvalidFencedOperation::InvalidJobIdLength {
            actual: job_id.chars().count(),
        });
    }
    if attempt_id.chars().count() != ULID_LEN {
        return invalid(InvalidFencedOperation::InvalidAttemptIdLength {
            actual: attempt_id.chars().count(),
        });
    }
    let digest = match operation.operation_id.as_ref() {
        Some(digest) => digest,
        None => return invalid(InvalidFencedOperation::MissingOperationId),
    };
    if HashAlgorithm::try_from(digest.algo) != Ok(HashAlgorithm::Blake3256) {
        return invalid(InvalidFencedOperation::InvalidOperationIdAlgorithm {
            actual: digest.algo,
        });
    }
    if digest.value.len() != 32 {
        return invalid(InvalidFencedOperation::InvalidOperationIdLength {
            actual: digest.value.len(),
        });
    }

    let derived = derive_operation_id(job_id, attempt_id, operation.operation_seq);
    if digest.value.as_slice() != derived {
        return invalid(InvalidFencedOperation::OperationIdMismatch);
    }

    let key = FencedOperationKey {
        fence_epoch: operation.fence_epoch,
        operation_id: derived,
    };
    if let Some(max_seen_fence_epoch) = previously_observed
        .iter()
        .map(|observed| observed.fence_epoch)
        .max()
    {
        if operation.fence_epoch < max_seen_fence_epoch {
            return FencedOperationDecision::StaleFence {
                received_fence_epoch: operation.fence_epoch,
                max_seen_fence_epoch,
            };
        }
    }
    if previously_observed.contains(&key) {
        return FencedOperationDecision::IdempotentRetransmission { key };
    }
    FencedOperationDecision::Accept { key }
}

fn invalid(reason: InvalidFencedOperation) -> FencedOperationDecision {
    FencedOperationDecision::Invalid { reason }
}
