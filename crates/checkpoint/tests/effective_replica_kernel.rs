use gputeer_checkpoint::{
    evaluate_effective_replicas, Durability, FactResolution, HolderValidation,
    ReplicaEvaluationError, ReplicaExclusionReason, ReplicaKind, ResolvedHolderObservation,
};

fn observation(
    holder_device_id: &str,
    failure_domain: &str,
    kind: ReplicaKind,
    acked_at_unix_ms: u64,
) -> ResolvedHolderObservation {
    ResolvedHolderObservation {
        checkpoint_id: "checkpoint-1".into(),
        root_digest: "validated-root-binding".into(),
        holder_device_id: holder_device_id.into(),
        acked_at_unix_ms,
        kind,
        selected: true,
        holder_validation: HolderValidation::Valid,
        is_ephemeral: FactResolution::Resolved(false),
        failure_domain: FactResolution::Resolved(failure_domain.into()),
    }
}

fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    fn visit<T: Clone>(remaining: Vec<T>, prefix: Vec<T>, output: &mut Vec<Vec<T>>) {
        if remaining.is_empty() {
            output.push(prefix);
            return;
        }
        for index in 0..remaining.len() {
            let mut next_remaining = remaining.clone();
            let item = next_remaining.remove(index);
            let mut next_prefix = prefix.clone();
            next_prefix.push(item);
            visit(next_remaining, next_prefix, output);
        }
    }

    let mut output = Vec::new();
    visit(items.to_vec(), Vec::new(), &mut output);
    output
}

#[test]
fn report_is_identical_for_every_input_permutation() {
    let counted = observation("device-b", "rack-b", ReplicaKind::Hub, 20);
    let same_domain = observation("device-c", "rack-b", ReplicaKind::TrustedPeer, 30);
    let mut superseded = observation("device-a", "rack-a", ReplicaKind::WorkerLocal, 10);
    superseded.selected = false;
    let mut unresolved = observation("device-d", "rack-d", ReplicaKind::SubmitterMirror, 40);
    unresolved.holder_validation = HolderValidation::MembershipUnresolved;
    let inputs = vec![counted, same_domain, superseded, unresolved];

    let expected = evaluate_effective_replicas(&inputs, Durability::Replicated).unwrap();
    for permutation in permutations(&inputs) {
        assert_eq!(
            evaluate_effective_replicas(&permutation, Durability::Replicated).unwrap(),
            expected
        );
    }
}

#[test]
fn duplicate_selected_holder_fails_closed_instead_of_counting() {
    let inputs = vec![
        observation("device-a", "rack-a", ReplicaKind::WorkerLocal, 10),
        observation("device-a", "rack-b", ReplicaKind::SubmitterMirror, 20),
    ];

    assert_eq!(
        evaluate_effective_replicas(&inputs, Durability::Mirrored),
        Err(ReplicaEvaluationError::MultipleSelectedObservations {
            holder_device_id: "device-a".into(),
            selected: 2,
        })
    );
}

#[test]
fn selected_old_observation_wins_over_newer_superseded_without_ttl_or_latest_rule() {
    let selected = observation("device-a", "rack-a", ReplicaKind::Hub, 1);
    let mut newer = observation("device-a", "rack-b", ReplicaKind::Hub, u64::MAX);
    newer.selected = false;

    let report =
        evaluate_effective_replicas(&[newer.clone(), selected.clone()], Durability::Mirrored)
            .unwrap();
    assert_eq!(report.effective_replica_count, 1);
    assert_eq!(report.counted[0].observation, selected);
    assert_eq!(report.superseded, vec![newer]);
}

#[test]
fn distinct_holders_in_one_failure_domain_count_once_deterministically() {
    let z = observation("device-z", "rack-a", ReplicaKind::Hub, 10);
    let a = observation("device-a", "rack-a", ReplicaKind::TrustedPeer, 20);

    let report = evaluate_effective_replicas(&[z, a], Durability::Replicated).unwrap();
    assert_eq!(report.effective_replica_count, 1);
    assert_eq!(report.counted[0].observation.holder_device_id, "device-a");
    assert_eq!(
        report.excluded[0].reasons,
        vec![ReplicaExclusionReason::DuplicateFailureDomain {
            counted_holder_device_id: "device-a".into(),
        }]
    );
}

#[test]
fn invalid_unresolved_and_ambiguous_membership_never_count() {
    let states = [
        (
            HolderValidation::InvalidSignature,
            ReplicaExclusionReason::InvalidSignature,
        ),
        (
            HolderValidation::NotApproved,
            ReplicaExclusionReason::HolderNotApproved,
        ),
        (
            HolderValidation::MembershipUnresolved,
            ReplicaExclusionReason::MembershipUnresolved,
        ),
        (
            HolderValidation::MembershipAmbiguous,
            ReplicaExclusionReason::MembershipAmbiguous,
        ),
    ];

    for (index, (validation, reason)) in states.into_iter().enumerate() {
        let mut input = observation(
            &format!("device-{index}"),
            &format!("rack-{index}"),
            ReplicaKind::Hub,
            index as u64,
        );
        input.holder_validation = validation;
        let report = evaluate_effective_replicas(&[input], Durability::Mirrored).unwrap();
        assert_eq!(report.effective_replica_count, 0);
        assert_eq!(report.excluded[0].reasons, vec![reason]);
    }
}

#[test]
fn unresolved_or_ambiguous_authority_facts_never_count() {
    let mut unresolved_ephemeral = observation("device-a", "rack-a", ReplicaKind::WorkerLocal, 10);
    unresolved_ephemeral.is_ephemeral = FactResolution::Unresolved;
    let mut ambiguous_ephemeral = observation("device-b", "rack-b", ReplicaKind::WorkerLocal, 20);
    ambiguous_ephemeral.is_ephemeral = FactResolution::Ambiguous;
    let mut unresolved_domain = observation("device-c", "rack-c", ReplicaKind::Hub, 30);
    unresolved_domain.failure_domain = FactResolution::Unresolved;
    let mut ambiguous_domain = observation("device-d", "rack-d", ReplicaKind::Hub, 40);
    ambiguous_domain.failure_domain = FactResolution::Ambiguous;

    let report = evaluate_effective_replicas(
        &[
            unresolved_ephemeral,
            ambiguous_ephemeral,
            unresolved_domain,
            ambiguous_domain,
        ],
        Durability::Mirrored,
    )
    .unwrap();
    assert_eq!(report.effective_replica_count, 0);
    assert_eq!(report.excluded.len(), 4);
    assert!(report
        .excluded
        .iter()
        .any(|excluded| excluded.reasons == [ReplicaExclusionReason::EphemeralStatusUnresolved]));
    assert!(report
        .excluded
        .iter()
        .any(|excluded| excluded.reasons == [ReplicaExclusionReason::EphemeralStatusAmbiguous]));
    assert!(report
        .excluded
        .iter()
        .any(|excluded| excluded.reasons == [ReplicaExclusionReason::FailureDomainUnresolved]));
    assert!(report
        .excluded
        .iter()
        .any(|excluded| excluded.reasons == [ReplicaExclusionReason::FailureDomainAmbiguous]));
}

#[test]
fn non_local_kind_does_not_require_irrelevant_ephemeral_resolution() {
    let mut unresolved = observation("device-a", "rack-a", ReplicaKind::Hub, 10);
    unresolved.is_ephemeral = FactResolution::Unresolved;
    let mut ambiguous = observation("device-b", "rack-b", ReplicaKind::SubmitterMirror, 20);
    ambiguous.is_ephemeral = FactResolution::Ambiguous;

    let report =
        evaluate_effective_replicas(&[unresolved, ambiguous], Durability::Replicated).unwrap();
    assert_eq!(report.effective_replica_count, 2);
    assert!(report.requirement_met);
    assert!(report.excluded.is_empty());
}

#[test]
fn only_ephemeral_worker_local_is_excluded() {
    let mut local = observation("device-a", "rack-a", ReplicaKind::WorkerLocal, 10);
    local.is_ephemeral = FactResolution::Resolved(true);
    let mut mirror = observation("device-b", "rack-b", ReplicaKind::SubmitterMirror, 20);
    mirror.is_ephemeral = FactResolution::Resolved(true);

    let report = evaluate_effective_replicas(&[local, mirror], Durability::Mirrored).unwrap();
    assert_eq!(report.effective_replica_count, 1);
    assert_eq!(
        report.counted[0].observation.kind,
        ReplicaKind::SubmitterMirror
    );
    assert_eq!(
        report.excluded[0].reasons,
        vec![ReplicaExclusionReason::EphemeralWorkerLocal]
    );
}

#[test]
fn same_device_different_kinds_are_never_separate_replicas() {
    let selected = observation("device-a", "rack-a", ReplicaKind::WorkerLocal, 20);
    let mut superseded = observation("device-a", "rack-a", ReplicaKind::SubmitterMirror, 10);
    superseded.selected = false;

    let report =
        evaluate_effective_replicas(&[superseded, selected], Durability::Replicated).unwrap();
    assert_eq!(report.effective_replica_count, 1);
    assert_eq!(report.superseded.len(), 1);
    assert!(!report.requirement_met);
}

#[test]
fn durability_thresholds_use_effective_distinct_domain_count() {
    let one = vec![observation("device-a", "rack-a", ReplicaKind::Hub, 10)];
    assert!(
        evaluate_effective_replicas(&one, Durability::Local)
            .unwrap()
            .requirement_met
    );
    assert!(
        evaluate_effective_replicas(&one, Durability::Mirrored)
            .unwrap()
            .requirement_met
    );
    assert!(
        !evaluate_effective_replicas(&one, Durability::Replicated)
            .unwrap()
            .requirement_met
    );

    let two = vec![
        one[0].clone(),
        observation("device-b", "rack-b", ReplicaKind::TrustedPeer, 20),
    ];
    assert!(
        evaluate_effective_replicas(&two, Durability::Replicated)
            .unwrap()
            .requirement_met
    );
}

#[test]
fn empty_input_is_zero_and_only_local_requirement_is_met() {
    let local = evaluate_effective_replicas(&[], Durability::Local).unwrap();
    assert_eq!(local.scope, None);
    assert_eq!(local.effective_replica_count, 0);
    assert!(local.requirement_met);
    assert!(
        !evaluate_effective_replicas(&[], Durability::Mirrored)
            .unwrap()
            .requirement_met
    );
}

#[test]
fn mixed_checkpoint_or_root_fails_closed_with_canonical_error() {
    let a = observation("device-a", "rack-a", ReplicaKind::Hub, 10);
    let mut b = observation("device-b", "rack-b", ReplicaKind::Hub, 20);
    b.checkpoint_id = "checkpoint-2".into();
    b.root_digest = "other-validated-root".into();

    let forward = evaluate_effective_replicas(&[a.clone(), b.clone()], Durability::Mirrored);
    let reverse = evaluate_effective_replicas(&[b, a], Durability::Mirrored);
    assert_eq!(forward, reverse);
    assert!(matches!(
        forward,
        Err(ReplicaEvaluationError::MixedEvaluationScope { scopes }) if scopes.len() == 2
    ));
}

#[test]
fn empty_required_identifiers_fail_closed() {
    let mut input = observation("device-a", "rack-a", ReplicaKind::Hub, 10);
    input.checkpoint_id = " ".into();
    assert_eq!(
        evaluate_effective_replicas(&[input], Durability::Mirrored),
        Err(ReplicaEvaluationError::EmptyCheckpointId)
    );

    let mut input = observation("device-a", "rack-a", ReplicaKind::Hub, 10);
    input.root_digest.clear();
    assert_eq!(
        evaluate_effective_replicas(&[input], Durability::Mirrored),
        Err(ReplicaEvaluationError::EmptyRootDigest)
    );

    let mut input = observation("device-a", "rack-a", ReplicaKind::Hub, 10);
    input.holder_device_id = "\t".into();
    assert_eq!(
        evaluate_effective_replicas(&[input], Durability::Mirrored),
        Err(ReplicaEvaluationError::EmptyHolderDeviceId)
    );

    let mut input = observation("device-a", "rack-a", ReplicaKind::Hub, 10);
    input.failure_domain = FactResolution::Resolved(" ".into());
    assert_eq!(
        evaluate_effective_replicas(&[input], Durability::Mirrored),
        Err(ReplicaEvaluationError::EmptyFailureDomain {
            holder_device_id: "device-a".into(),
            acked_at_unix_ms: 10,
        })
    );
}
