//! 원시 protobuf 진입점의 정상·negative test.
//!
//! 검증 계층은 통과 테스트보다 거부 테스트가 중요하다.
//! 서명 위조·만료·replay·디코드 실패·락 타임아웃을 각각 고정한다.

use gputeer_crypto::{
    decode_and_verify, sign, Clock, DurableReplayGuard, InMemoryKeyring,
    IngressError, KeyDirectorySource, KeyProtection, PersistentKeyring,
    PlaintextPolicy, SigningKey, SystemClock,
};
use gputeer_protocol::signing::ReplayGuard;
use gputeer_protocol::{
    canonical::Domain,
    pb,
    signing::{
        ReplayDecision, ReplayStoreError, VerifyError, VerifyOutcome,
    },
};
use prost::Message;
use tempfile::tempdir;

const NOW: u64 = 1_755_200_000_000;
const DEVICE: &str = "01JBXR7Q0000000000000000DD";

#[derive(Debug, Clone, Copy)]
struct FixedClock(u64);

impl Clock for FixedClock {
    fn now_unix_ms(&self) -> u64 {
        self.0
    }
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn grant(key: &SigningKey) -> pb::ExecutionGrant {
    let mut message = pb::ExecutionGrant {
        schema_version: 1,
        grant_id: "01JBXGRANT000000000000001".into(),
        attempt_id: "01JBXATTEMPT00000000000001".into(),
        coordinator_device_id: DEVICE.into(),
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + 60_000,
        nonce: (0u8..16).collect(),
        ..Default::default()
    };

    message.coordinator_signature = sign(key, &message).to_vec();
    message
}

fn manifest(key: &SigningKey) -> pb::JobManifest {
    let mut message = pb::JobManifest {
        schema_version: 1,
        job_id: "01JBXJOB000000000000000001".into(),
        team_id: "01JBXTEAM00000000000000001".into(),
        entrypoint: "train.py".into(),
        submitter_device_id: DEVICE.into(),
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + 7 * 24 * 60 * 60 * 1_000,
        ..Default::default()
    };

    message.submitter_signature = sign(key, &message).to_vec();
    message
}

fn key_directory(key: &SigningKey) -> InMemoryKeyring {
    let mut directory = InMemoryKeyring::new();
    directory.insert(DEVICE, key.verifying_key());
    directory
}

fn protocol_outcome(error: IngressError) -> VerifyOutcome {
    match error {
        IngressError::Verification(VerifyError::Outcome(outcome)) => outcome,
        other => panic!("프로토콜 검증 결과가 아니다: {other:?}"),
    }
}

#[test]
fn valid_bytes_become_verified_with_effective_replay_guard() {
    let signing_key = key(1);
    let directory = key_directory(&signing_key);
    let message = grant(&signing_key);
    let raw = message.encode_to_vec();
    let mut replay = gputeer_crypto::InMemoryReplayGuard::new();

    let verified = decode_and_verify::<pb::ExecutionGrant>(
        &raw,
        1,
        KeyDirectorySource::Provided(&directory),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("정상 protobuf가 검증을 통과해야 한다");

    assert_eq!(
        verified.get().grant_id,
        "01JBXGRANT000000000000001"
    );
    assert!(verified.replay_checked());
    assert!(verified.require_replay_checked().is_ok());
}

#[test]
fn forged_signature_is_rejected() {
    let signing_key = key(1);
    let directory = key_directory(&signing_key);
    let mut message = grant(&signing_key);

    message.coordinator_signature[0] ^= 0xFF;

    let error = decode_and_verify::<pb::ExecutionGrant>(
        &message.encode_to_vec(),
        1,
        KeyDirectorySource::Provided(&directory),
        &mut gputeer_crypto::InMemoryReplayGuard::new(),
        &FixedClock(NOW),
    )
    .expect_err("위조된 서명이 통과했다");

    assert_eq!(
        protocol_outcome(error),
        VerifyOutcome::InvalidSignature
    );
}

#[test]
fn expired_message_is_rejected() {
    let signing_key = key(1);
    let directory = key_directory(&signing_key);
    let message = grant(&signing_key);

    let error = decode_and_verify::<pb::ExecutionGrant>(
        &message.encode_to_vec(),
        1,
        KeyDirectorySource::Provided(&directory),
        &mut gputeer_crypto::InMemoryReplayGuard::new(),
        &FixedClock(message.expires_at_unix_ms),
    )
    .expect_err("만료된 메시지가 통과했다");

    assert_eq!(protocol_outcome(error), VerifyOutcome::Expired);
}

#[test]
fn replay_is_rejected_on_the_second_ingress() {
    let signing_key = key(1);
    let directory = key_directory(&signing_key);
    let raw = grant(&signing_key).encode_to_vec();
    let mut replay = gputeer_crypto::InMemoryReplayGuard::new();

    decode_and_verify::<pb::ExecutionGrant>(
        &raw,
        1,
        KeyDirectorySource::Provided(&directory),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("첫 번째 메시지는 새 nonce라야 한다");

    let error = decode_and_verify::<pb::ExecutionGrant>(
        &raw,
        1,
        KeyDirectorySource::Provided(&directory),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect_err("같은 nonce가 두 번째로 통과했다");

    assert_eq!(protocol_outcome(error), VerifyOutcome::Replay);
}

#[test]
fn protobuf_decode_failure_is_not_a_verify_outcome() {
    let signing_key = key(1);
    let directory = key_directory(&signing_key);

    let error = decode_and_verify::<pb::ExecutionGrant>(
        &[0xFF],
        1,
        KeyDirectorySource::Provided(&directory),
        &mut gputeer_crypto::InMemoryReplayGuard::new(),
        &FixedClock(NOW),
    )
    .expect_err("잘못된 protobuf가 디코드되었다");

    assert!(matches!(error, IngressError::Decode(_)));
}

struct LockTimeoutGuard {
    calls: usize,
}

impl ReplayGuard for LockTimeoutGuard {
    fn check_and_record(
        &mut self,
        _signer_id: &str,
        _domain: Domain,
        _nonce: &[u8],
        _retain_until_ms: u64,
    ) -> Result<ReplayDecision, ReplayStoreError> {
        self.calls += 1;
        Err(ReplayStoreError::LockTimeout)
    }

    fn is_effective(&self) -> bool {
        true
    }
}

#[test]
fn lock_timeout_is_rejected_without_retry() {
    let signing_key = key(1);
    let directory = key_directory(&signing_key);
    let raw = grant(&signing_key).encode_to_vec();
    let mut replay = LockTimeoutGuard { calls: 0 };

    let error = decode_and_verify::<pb::ExecutionGrant>(
        &raw,
        1,
        KeyDirectorySource::Provided(&directory),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect_err("replay 저장소 락 타임아웃이 통과했다");

    assert!(matches!(
        error,
        IngressError::ReplayStoreUnavailable(
            ReplayStoreError::LockTimeout
        )
    ));
    assert_eq!(
        replay.calls, 1,
        "진입점이 LockTimeout을 자동 재시도했다"
    );
}

#[test]
fn not_applicable_is_visible_and_does_not_permit_side_effects() {
    let signing_key = key(1);
    let directory = key_directory(&signing_key);
    let raw = manifest(&signing_key).encode_to_vec();
    let mut replay = gputeer_crypto::InMemoryReplayGuard::new();

    let verified = decode_and_verify::<pb::JobManifest>(
        &raw,
        1,
        KeyDirectorySource::Provided(&directory),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("정상 장수명 메시지가 거부되었다");

    assert_eq!(
        verified.replay_status(),
        gputeer_protocol::signing::ReplayStatus::NotApplicable
    );
    assert!(!verified.replay_checked());
    assert_eq!(
        verified.require_replay_checked().unwrap_err(),
        VerifyOutcome::Replay
    );
}

#[test]
fn persistent_keyring_and_durable_replay_are_wired_at_the_boundary() {
    let signing_key = key(1);
    let temporary = tempdir().unwrap();
    let keyring_path = temporary.path().join("keyring.bin");
    let replay_path = temporary.path().join("replay.sqlite");

    let mut keyring = PersistentKeyring::new(
        &keyring_path,
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .unwrap();

    keyring
        .insert_public(DEVICE, signing_key.verifying_key())
        .unwrap();
    keyring.save().unwrap();

    let loaded_keyring =
        PersistentKeyring::load(&keyring_path, PlaintextPolicy::Allow)
            .unwrap();

    let raw = grant(&signing_key).encode_to_vec();

    {
        let mut replay = DurableReplayGuard::open(&replay_path).unwrap();

        let verified = decode_and_verify::<pb::ExecutionGrant>(
            &raw,
            1,
            KeyDirectorySource::Persistent(&loaded_keyring),
            &mut replay,
            &FixedClock(NOW),
        )
        .expect("영속 키링과 replay guard를 통한 검증이 실패했다");

        assert!(verified.replay_checked());
    }

    let mut restarted_replay =
        DurableReplayGuard::open(&replay_path).unwrap();

    let error = decode_and_verify::<pb::ExecutionGrant>(
        &raw,
        1,
        KeyDirectorySource::Persistent(&loaded_keyring),
        &mut restarted_replay,
        &FixedClock(NOW),
    )
    .expect_err("프로세스 재시작 뒤 같은 nonce가 다시 통과했다");

    assert_eq!(protocol_outcome(error), VerifyOutcome::Replay);
}

#[test]
fn system_clock_is_available_but_the_entrypoint_accepts_injected_clock() {
    let clock = SystemClock;
    assert!(clock.now_unix_ms() > 0);
}
