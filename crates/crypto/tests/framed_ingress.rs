//! 프레이밍 계층의 negative test — `RULE.md` §6.
//!
//! 정상 dispatch 보다 **거부**가 중요하다: 프레임 상한 초과 · 잘린 프레임 ·
//! 타입 불일치 · 알 수 없는 타입 · 처리기 실패 시나리오를 각각 고정한다.

use std::io::Cursor;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, FramingError, InMemoryKeyring,
    InMemoryReplayGuard, IngressMessage, KeyDirectorySource, SigningKey,
};
use gputeer_protocol::constants::MAX_INGRESS_FRAME_BYTES;
use gputeer_protocol::pb;
use prost::Message;

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

fn grant(k: &SigningKey, nonce_seed: u8) -> pb::ExecutionGrant {
    let mut m = pb::ExecutionGrant {
        schema_version: 1,
        grant_id: "01JBXGRANT000000000000001".into(),
        attempt_id: "01JBXATTEMPT00000000000001".into(),
        coordinator_device_id: DEVICE.into(),
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + 60_000,
        nonce: (0u8..16).map(|i| i.wrapping_add(nonce_seed)).collect(),
        ..Default::default()
    };
    m.coordinator_signature = sign(k, &m).to_vec();
    m
}

fn lease(k: &SigningKey) -> pb::Lease {
    let mut m = pb::Lease {
        schema_version: 1,
        lease_id: "01JBXLEASE00000000000000001".into(),
        job_id: "01JBXJOB000000000000000001".into(),
        attempt_id: "01JBXATTEMPT00000000000001".into(),
        // ★ Lease::signer_id() 는 issuing_coordinator_id 다 —
        //   ExecutionGrant 의 coordinator_device_id 와 다른 필드다.
        //   빠뜨리면 UnknownSigner 로 거부된다.
        issuing_coordinator_id: DEVICE.into(),
        expires_at_unix_ms: NOW + 600_000,
        ..Default::default()
    };
    m.coordinator_signature = sign(k, &m).to_vec();
    m
}

fn directory(k: &SigningKey) -> InMemoryKeyring {
    let mut d = InMemoryKeyring::new();
    d.insert(DEVICE, k.verifying_key());
    d
}

/// 정상 dispatch — 비공허성. 이것이 실패하면 아래 거부 테스트들이
/// "애초에 아무것도 통과 못 하는" 조건에서 우연히 통과할 수 있다.
#[test]
fn normal_grant_frame_dispatches_to_the_right_variant() {
    let k = key(1);
    let dir = directory(&k);
    let g = grant(&k, 0);
    let frame = write_frame(FrameType::Grant, &g.encode_to_vec());
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    let msg = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("정상 프레임이 통과해야 한다");

    assert!(matches!(msg, IngressMessage::Grant(_)), "잘못된 variant 로 디스패치됐다");
}

/// 다른 타입(Lease)도 같은 스트림 구조에서 정상 동작하는가 — 비공허성.
#[test]
fn normal_lease_frame_dispatches_to_the_right_variant() {
    let k = key(2);
    let dir = directory(&k);
    let l = lease(&k);
    let frame = write_frame(FrameType::Lease, &l.encode_to_vec());
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    let msg = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("정상 Lease 프레임이 통과해야 한다");

    assert!(matches!(msg, IngressMessage::Lease(_)), "잘못된 variant 로 디스패치됐다");
}

/// ★ 프레임 상한 초과 — 상한을 **주장된 길이만으로** 거부해야 한다.
///
/// 실제로 그만큼의 바이트를 보내지 않는다 — 상한 검사가 몸통을 다 읽은
/// 뒤에야 발동하면, 그 사이에 메모리를 이미 소진당한다.
#[test]
fn oversized_frame_is_rejected_before_reading_body() {
    let mut header = vec![FrameType::Grant as u8];
    header.extend_from_slice(&(MAX_INGRESS_FRAME_BYTES + 1).to_be_bytes());
    // ★ 본문은 보내지 않는다. 상한 검사가 헤더만 보고 거부한다면
    //   스트림에 몸통이 없어도 여기서 이미 오류가 나야 한다.
    let mut stream = Cursor::new(header);

    let dir = InMemoryKeyring::new();
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    assert!(
        matches!(
            result,
            Err(FramingError::FrameTooLarge { max, .. }) if max == MAX_INGRESS_FRAME_BYTES
        ),
        "상한 초과가 거부되지 않았다: {result:?}"
    );
}

/// 상한과 **정확히 같은** 크기는 거부하지 않는다 — 비공허성(경계값).
///
/// 이 테스트는 몸통을 실제로 채우지 않고 짧게 끊는다. 목적은 "상한 자체를
/// 넘지 않았다는 이유로 FrameTooLarge 가 나지 않는가" 만 본다 — 그 뒤 단계
/// (Truncated)에서 실패하는 것은 이 테스트의 관심사가 아니다.
#[test]
fn frame_exactly_at_the_limit_is_not_rejected_for_size() {
    let mut header = vec![FrameType::Grant as u8];
    header.extend_from_slice(&MAX_INGRESS_FRAME_BYTES.to_be_bytes());
    let mut stream = Cursor::new(header);

    let dir = InMemoryKeyring::new();
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    assert!(
        !matches!(result, Err(FramingError::FrameTooLarge { .. })),
        "경계값(상한과 동일)이 크기 초과로 거부됐다: {result:?}"
    );
    // 몸통을 안 보냈으므로 Truncated 로 끝나는 것이 정상이다.
    assert!(matches!(result, Err(FramingError::Truncated)));
}

/// ★ 잘린 프레임 — 헤더 도중에 스트림이 끊긴다.
#[test]
fn truncated_header_is_rejected() {
    let stream_bytes = vec![FrameType::Grant as u8, 0x00, 0x00]; // 5바이트 헤더 중 3바이트만
    let mut stream = Cursor::new(stream_bytes);

    let dir = InMemoryKeyring::new();
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    assert!(
        matches!(result, Err(FramingError::Truncated)),
        "헤더가 잘렸는데 Truncated 가 아니다: {result:?}"
    );
}

/// 몸통 도중에 스트림이 끊긴다.
#[test]
fn truncated_body_is_rejected() {
    let k = key(3);
    let g = grant(&k, 1);
    let full = write_frame(FrameType::Grant, &g.encode_to_vec());
    // 헤더는 온전히 두고 몸통만 잘라 보낸다.
    let cut = full[..full.len() - 5].to_vec();
    let mut stream = Cursor::new(cut);

    let dir = InMemoryKeyring::new();
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    assert!(
        matches!(result, Err(FramingError::Truncated)),
        "몸통이 잘렸는데 Truncated 가 아니다: {result:?}"
    );
}

/// ★ 알 수 없는 프레임 타입.
#[test]
fn unknown_frame_type_is_rejected() {
    let mut header = vec![0xFFu8]; // 정의되지 않은 타입
    header.extend_from_slice(&0u32.to_be_bytes());
    let mut stream = Cursor::new(header);

    let dir = InMemoryKeyring::new();
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    assert!(
        matches!(result, Err(FramingError::UnknownFrameType(0xFF))),
        "알 수 없는 타입이 거부되지 않았다: {result:?}"
    );
}

/// ★ **타입 불일치** — 헤더는 Grant 라고 주장하지만 몸통은 Lease 다.
///
/// 이것이 모듈 문서가 설명하는 "헤더 태그를 신뢰하지 않는다" 의 핵심이다.
/// 헤더가 무엇이라 주장하든, 실제 서명은 그 타입(Lease)으로 만들어졌으므로
/// `decode_and_verify::<ExecutionGrant>()` 로 검증을 시도하면
/// domain_tag 불일치로 **반드시 실패해야 한다.**
#[test]
fn frame_type_mismatch_between_header_and_signature_is_rejected() {
    let k = key(4);
    let dir = directory(&k);
    // Lease 로 실제 서명된 바이트를, Grant 라고 주장하는 헤더에 담는다.
    let l = lease(&k);
    let disguised = write_frame(FrameType::Grant, &l.encode_to_vec());
    let mut stream = Cursor::new(disguised);
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    // prost 는 필드 번호만 맞으면 관대하게 디코드할 수 있으므로,
    // "디코드 자체가 실패한다" 를 요구하지 않는다. 대신 **검증이 통과해서는
    // 안 된다** 는 것만 요구한다 — 통과하면 위조 방어가 뚫린 것이다.
    assert!(
        matches!(result, Err(FramingError::Verify(_))),
        "★ 헤더 타입을 속였는데 검증을 통과했다 — domain_tag 방어가 뚫렸다: {result:?}"
    );
}

/// ★ 서명 자체가 위조된 경우도 프레이밍 계층을 통해 거부되는가.
///
/// `verification_ingress.rs` 가 `decode_and_verify` 수준에서 이미 검사하지만,
/// 프레이밍 계층이 그 결과를 **삼키지 않고 전달**하는지 여기서 확인한다.
#[test]
fn forged_signature_is_rejected_through_the_framing_layer() {
    let k = key(5);
    let dir = directory(&k);
    let mut g = grant(&k, 2);
    g.coordinator_signature[0] ^= 0xFF;
    let frame = write_frame(FrameType::Grant, &g.encode_to_vec());
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    assert!(
        matches!(result, Err(FramingError::Verify(_))),
        "위조된 서명이 프레이밍 계층을 통과했다: {result:?}"
    );
}

/// **길이 0** 프레임 — 몸통이 없는 프레임을 시도하면 디코드 실패로
/// 이어져야 한다(패닉이 아니라).
#[test]
fn zero_length_frame_does_not_panic() {
    let header = {
        let mut h = vec![FrameType::Grant as u8];
        h.extend_from_slice(&0u32.to_be_bytes());
        h
    };
    let mut stream = Cursor::new(header);

    let dir = InMemoryKeyring::new();
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    assert!(result.is_err(), "빈 프레임이 통과했다 — 있을 수 없는 값이다");
}

/// 한 스트림에 프레임 여러 개가 이어져도 각각 독립적으로 읽히는가.
///
/// ★ 이것은 실제 사용 형태(연속 스트림)에 대한 비공허성이다.
/// 프레임 하나만 테스트하면 "길이 필드를 무시하고 스트림 끝까지 읽는"
/// 구현도 통과할 수 있다.
#[test]
fn two_consecutive_frames_are_read_independently() {
    let k = key(6);
    let dir = directory(&k);
    let g1 = grant(&k, 10);
    let g2 = grant(&k, 11);

    let mut combined = write_frame(FrameType::Grant, &g1.encode_to_vec());
    combined.extend(write_frame(FrameType::Grant, &g2.encode_to_vec()));
    let mut stream = Cursor::new(combined);
    let mut replay = InMemoryReplayGuard::new();

    let first = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("첫 프레임이 통과해야 한다");
    let second = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("두 번째 프레임이 통과해야 한다");

    assert!(matches!(first, IngressMessage::Grant(_)));
    assert!(matches!(second, IngressMessage::Grant(_)));
}
