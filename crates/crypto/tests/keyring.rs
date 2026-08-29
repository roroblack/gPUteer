//! 영속 키 관리 negative test.
//!
//! `RULE.md` §6에 따라 정상 경로뿐 아니라 폐기·회전 종료·손상 파일·비밀 출력
//! 경로를 검사한다.

use std::fs;

use gputeer_crypto::{
    sign, Ed25519Verifier, KeyDirectoryStatus, KeyProtection, PersistentKeyring, PlaintextPolicy,
    SecretSigningKey, SigningKey,
};
use gputeer_protocol::{
    pb,
    signing::{verify, NoReplayCheck, VerifyOutcome},
};
use tempfile::tempdir;

const NOW: u64 = 1_755_200_000_000;

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn manifest(signer_id: &str) -> pb::JobManifest {
    pb::JobManifest {
        schema_version: 1,
        job_id: "01JBXR7Q0000000000000AAA".into(),
        team_id: "01JBXR7Q0000000000000TTT".into(),
        entrypoint: "train.py".into(),
        submitter_device_id: signer_id.into(),
        issued_at_unix_ms: NOW - 1_000,
        // ★ 7일. 초안은 24시간이었는데 회전 grace period 도 24시간이라
        //   "grace 종료 후 신 키는 통과한다" 를 검사하는 시점(rotation_at + 24h)에서
        //   매니페스트가 먼저 **만료**됐다. 키 검사에 도달하지 못한 것이다.
        //   MANIFEST_TTL_MS(7일)와 맞춘다.
        expires_at_unix_ms: NOW + 7 * 86_400_000,
        ..Default::default()
    }
}

fn signed_manifest(signer_id: &str, signing_key: &SigningKey) -> pb::JobManifest {
    let mut message = manifest(signer_id);
    message.submitter_signature = sign(signing_key, &message).to_vec();
    message
}

fn verifier<'a>(
    keyring: &'a PersistentKeyring,
    now_ms: u64,
) -> Ed25519Verifier<gputeer_crypto::KeyDirectoryView<'a>> {
    Ed25519Verifier::new(keyring.at(now_ms))
}

#[test]
fn revoked_key_rejects_a_message_signed_before_revocation() {
    let directory = "device-revoked";
    let signing_key = key(1);

    let mut keyring = PersistentKeyring::new(
        tempdir().unwrap().path().join("keys.bin"),
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .unwrap();

    keyring
        .insert_private(
            directory,
            SecretSigningKey::from_signing_key(signing_key.clone()),
        )
        .unwrap();

    let message = signed_manifest(directory, &signing_key);

    verify(
        &message,
        1,
        &verifier(&keyring, NOW),
        NOW,
        &mut NoReplayCheck,
    )
    .expect("폐기 전 메시지는 통과해야 한다");

    keyring.revoke(directory).unwrap();

    assert_eq!(
        keyring.status_at(directory, NOW),
        KeyDirectoryStatus::Revoked
    );

    let result = verify(
        &message,
        1,
        &verifier(&keyring, NOW + 1),
        NOW + 1,
        &mut NoReplayCheck,
    );

    assert_eq!(
        result.unwrap_err().outcome(),
        Some(VerifyOutcome::UnknownSigner),
        "폐기된 키의 과거 서명이 현재 검증에서 통과했다"
    );
}

#[test]
fn rotation_accepts_both_keys_during_grace_and_rejects_old_key_afterward() {
    let directory = "device-rotating";
    let old_key = key(2);
    let new_key = key(3);

    let mut keyring = PersistentKeyring::new(
        tempdir().unwrap().path().join("keys.bin"),
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .unwrap();

    keyring
        .insert_private(
            directory,
            SecretSigningKey::from_signing_key(old_key.clone()),
        )
        .unwrap();

    let rotation_at = NOW + 10_000;

    keyring
        .rotate(
            directory,
            SecretSigningKey::from_signing_key(new_key.clone()),
            rotation_at,
        )
        .unwrap();

    let old_message = signed_manifest(directory, &old_key);
    let new_message = signed_manifest(directory, &new_key);

    verify(
        &old_message,
        1,
        &verifier(&keyring, rotation_at + 1),
        rotation_at + 1,
        &mut NoReplayCheck,
    )
    .expect("grace period 중 구 키가 거부되었다");

    verify(
        &new_message,
        1,
        &verifier(&keyring, rotation_at + 1),
        rotation_at + 1,
        &mut NoReplayCheck,
    )
    .expect("grace period 중 신 키가 거부되었다");

    let after_grace = rotation_at + 24 * 60 * 60 * 1_000;

    assert_eq!(
        verify(
            &old_message,
            1,
            &verifier(&keyring, after_grace),
            after_grace,
            &mut NoReplayCheck,
        )
        .unwrap_err()
        .outcome(),
        Some(VerifyOutcome::UnknownSigner),
        "grace period 종료 후 구 키가 계속 통과했다"
    );

    verify(
        &new_message,
        1,
        &verifier(&keyring, after_grace),
        after_grace,
        &mut NoReplayCheck,
    )
    .expect("grace period 종료 후 신 키까지 거부되었다");
}

#[test]
fn quarantine_is_distinguishable_in_directory_but_rejects_verification() {
    let directory = "device-quarantine";
    let signing_key = key(4);

    let mut keyring = PersistentKeyring::new(
        tempdir().unwrap().path().join("keys.bin"),
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .unwrap();

    keyring
        .insert_private(
            directory,
            SecretSigningKey::from_signing_key(signing_key.clone()),
        )
        .unwrap();

    keyring.quarantine(directory).unwrap();

    assert_eq!(
        keyring.status_at(directory, NOW),
        KeyDirectoryStatus::Quarantined
    );

    let message = signed_manifest(directory, &signing_key);
    assert_eq!(
        verify(
            &message,
            1,
            &verifier(&keyring, NOW),
            NOW,
            &mut NoReplayCheck,
        )
        .unwrap_err()
        .outcome(),
        Some(VerifyOutcome::UnknownSigner)
    );
}

#[test]
fn private_key_never_appears_in_debug_or_display_output() {
    let seed = 0xA5;
    let private = SecretSigningKey::from_signing_key(key(seed));

    let keyring = PersistentKeyring::new(
        tempdir().unwrap().path().join("keys.bin"),
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .unwrap();

    // ★ hex 한 가지만 보면 **공허해질 수 있다.**
    //   Debug 파생 구현은 바이트를 10진수 배열로 찍는다 — `[165, 165, ...]`.
    //   hex 만 검사하면 그것을 통과시킨다.
    //   실제로 새어 나갈 수 있는 모든 표기를 검사한다.
    let raw = [seed; 32];
    let leaks: Vec<String> = vec![
        raw.iter().map(|b| format!("{b:02x}")).collect(), // a5a5...
        raw.iter().map(|b| format!("{b:02X}")).collect(), // A5A5...
        format!("{raw:?}"),                               // [165, 165, ...]
        raw.iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(", "), // 165, 165, ...
    ];

    let secret_debug = format!("{private:?}");
    let secret_display = format!("{private}");
    let keyring_debug = format!("{keyring:?}");

    for (label, out) in [
        ("SecretSigningKey Debug", &secret_debug),
        ("SecretSigningKey Display", &secret_display),
        ("PersistentKeyring Debug", &keyring_debug),
    ] {
        for leak in &leaks {
            assert!(
                !out.contains(leak.as_str()),
                "★ {label} 에 개인키 바이트가 들어갔다 (표기: {}...): {out}",
                &leak[..leak.len().min(16)]
            );
        }
    }

    assert!(secret_debug.contains("REDACTED"));
    assert!(secret_display.contains("REDACTED"));
}

#[test]
fn corrupted_key_file_is_rejected_instead_of_silently_loaded() {
    let directory = "device-corrupted";
    let directory_path = tempdir().unwrap();
    let key_path = directory_path.path().join("keys.bin");

    let mut keyring = PersistentKeyring::new(
        &key_path,
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Allow,
    )
    .unwrap();

    keyring
        .insert_private(directory, SecretSigningKey::from_signing_key(key(5)))
        .unwrap();
    keyring.save().unwrap();

    let mut bytes = fs::read(&key_path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    fs::write(&key_path, bytes).unwrap();

    let result = PersistentKeyring::load(&key_path, PlaintextPolicy::Allow);

    assert!(result.is_err(), "손상된 키 파일이 조용히 로드되었다");
}

#[test]
fn plaintext_storage_is_rejected_without_explicit_opt_in() {
    let result = PersistentKeyring::new(
        tempdir().unwrap().path().join("keys.bin"),
        KeyProtection::K0Plaintext,
        PlaintextPolicy::Reject,
    );

    assert!(result.is_err(), "K0 평문 저장이 기본 허용되었다");
}

#[cfg(not(windows))]
#[test]
fn k1_is_explicitly_unavailable_on_unverified_linux() {
    let result = PersistentKeyring::new(
        tempdir().unwrap().path().join("keys.bin"),
        KeyProtection::K1OsProtected,
        PlaintextPolicy::Reject,
    );

    assert!(matches!(
        result,
        Err(gputeer_crypto::KeyringError::UnsupportedPlatform)
    ));
}
