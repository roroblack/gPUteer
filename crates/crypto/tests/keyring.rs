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

/// K1 이 **지원되지 않는 플랫폼**에서 명시적으로 실패하는가.
///
/// ★ 조용히 K0 로 내려가지 않는다. 강등되면 "보호받는다" 고 믿는
///   상태에서 평문으로 놓이고, 그건 §0.4 가 금지하는 종류의 거짓말이다.
#[cfg(not(any(windows, target_os = "linux")))]
#[test]
fn k1_is_explicitly_unavailable_on_platforms_without_a_backend() {
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

/// Linux 의 K1 이 `systemd-creds` 로 실제 왕복하는가.
///
/// # 왜 이 테스트가 조건부인가
///
/// ★ `systemd-creds` 는 systemd 가 도는 기계에만 있다. 없는 환경에서
///   실패시키면 "환경 없음" 이 "코드 결함" 으로 보고된다 —
///   `CLAUDE.md` §4 가 `ENVIRONMENT-BLOCKED` 를 `PASS` 로 계상하지 말라고
///   한 것과 같은 이유다.
///
///   대신 **없으면 없다고 말하고** 건너뛴다. 그 경우에도 K1 요청이
///   조용히 통과하지 않는지는 확인한다.
#[cfg(target_os = "linux")]
#[test]
fn linux_k1_round_trips_through_systemd_creds() {
    let available = std::process::Command::new("systemd-creds")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);

    let dir = tempdir().unwrap();
    let path = dir.path().join("keys.bin");
    let signer = "01JDEVICESELFTEST0000000001";
    let secret = SecretSigningKey::from_signing_key(key(9));
    let public = secret.verifying_key();

    let created = PersistentKeyring::new(
        &path,
        KeyProtection::K1OsProtected,
        PlaintextPolicy::Reject,
    );

    if !available {
        eprintln!("ENVIRONMENT-BLOCKED: systemd-creds 가 없다 — K1 왕복은 측정하지 않았다");
        // ★ 그래도 이것만은 확인한다. 없을 때 조용히 통과하면
        //   보호받는다고 믿으면서 보호 없이 도는 것이다.
        //
        //   `new()` 는 파일을 아직 안 만들므로 여기서는 통과할 수 있다.
        //   실제 봉인은 `save()` 에서 일어나므로 거기서 막혀야 한다.
        if let Ok(mut keyring) = created {
            keyring.insert_private(signer, secret).expect("등록");
            assert!(
                keyring.save().is_err(),
                "systemd-creds 가 없는데 K1 저장이 성공했다 — 조용한 강등이다"
            );
        }
        return;
    }

    let mut keyring = created.expect("systemd-creds 가 있는데 K1 키링을 못 만들었다");
    keyring.insert_private(signer, secret).expect("등록");
    keyring.save().expect("봉인 저장");

    // ★ 재열기로 왕복을 증명한다. 같은 객체에서 읽으면 메모리에 남은
    //   값을 보는 것이라 실제로 봉인·복호가 됐는지 알 수 없다.
    let reopened =
        PersistentKeyring::load(&path, PlaintextPolicy::Reject).expect("재열기");
    assert_eq!(
        reopened.protection(),
        KeyProtection::K1OsProtected,
        "재열기 후 보관 등급이 달라졌다"
    );
    let verifier = Ed25519Verifier::new(reopened);
    let _ = &verifier;

    // ★ 저장된 파일에 개인키 평문이 **없어야** 한다. 이게 K1 의 요점이다 —
    //   봉인했다고 말하면서 평문을 옆에 두면 아무 의미가 없다.
    let raw = fs::read(&path).expect("키링 파일 읽기");
    assert!(
        !contains_subslice(&raw, &[9u8; 32]),
        "키링 파일에 개인키 평문(seed)이 그대로 들어 있다 — 봉인이 안 됐다"
    );
    // 공개키는 평문이어도 된다 — 그게 공개키다. 파일이 실제로 그 키를
    // 담고 있는지 확인해 "빈 파일이라 평문도 없다" 를 배제한다.
    assert!(
        contains_subslice(&raw, public.as_bytes()),
        "키링 파일에 공개키가 없다 — 저장 자체가 안 된 것을 봉인으로 오인할 뻔했다"
    );
}

/// `haystack` 안에 `needle` 이 그대로 들어 있는가.
#[cfg(target_os = "linux")]
fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// ★ "플랫폼이 지원 안 한다" 와 "지원하는데 실패했다" 를 구분하는가.
///
/// `CLAUDE.md` §3 — 오류 메시지가 사실을 잘못 전하지 않게 한다. 둘을
/// 같은 오류로 보고하면 고치는 사람이 엉뚱한 곳을 본다.
#[test]
fn unsupported_platform_and_call_failure_are_different_errors() {
    let unsupported = gputeer_crypto::KeyringError::UnsupportedPlatform.to_string();
    let failed =
        gputeer_crypto::KeyringError::OsProtectionFailed("권한 없음".into()).to_string();
    assert_ne!(unsupported, failed);
    assert!(unsupported.contains("사용할 수 없다"));
    assert!(failed.contains("실패했다") && failed.contains("권한 없음"));
}
