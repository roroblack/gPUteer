//! 프레이밍 계층의 negative test — `RULE.md` §6.
//!
//! 정상 dispatch 보다 **거부**가 중요하다: 프레임 상한 초과 · 잘린 프레임 ·
//! 타입 불일치 · 알 수 없는 타입 · 처리기 실패 시나리오를 각각 고정한다.

use std::io::Cursor;

use gputeer_crypto::{
    read_frame, sign, write_frame, Clock, FrameType, FramingError, InMemoryKeyring,
    InMemoryReplayGuard, IngressError, IngressMessage, KeyDirectorySource, SigningKey,
};
use gputeer_protocol::constants::MAX_INGRESS_FRAME_BYTES;
use gputeer_protocol::pb;
use gputeer_protocol::signing::{VerifyError, VerifyOutcome};
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

fn grant_ack(k: &SigningKey, nonce_seed: u8) -> pb::AgentGrantAck {
    let mut m = pb::AgentGrantAck {
        schema_version: 1,
        grant_id: "01JBXGRANT000000000000001".into(),
        attempt_id: "01JBXATTEMPT00000000000001".into(),
        agent_device_id: DEVICE.into(),
        issued_at_unix_ms: NOW,
        expires_at_unix_ms: NOW + 60_000,
        nonce: (0u8..16).map(|i| i.wrapping_add(nonce_seed)).collect(),
        accepted: true,
        ..Default::default()
    };
    m.agent_signature = sign(k, &m).to_vec();
    m
}

fn renew_result(k: &SigningKey, nonce_seed: u8) -> pb::RenewLeaseResult {
    let mut m = pb::RenewLeaseResult {
        outcome: 1, // RENEW_OUTCOME_RENEWED
        detail: "ok".into(),
        schema_version: 1,
        coordinator_id: DEVICE.into(),
        issued_at_unix_ms: NOW,
        request_nonce: (0u8..16).map(|i| i.wrapping_add(nonce_seed)).collect(),
        ..Default::default()
    };
    m.coordinator_signature = sign(k, &m).to_vec();
    m
}

fn session_hello(k: &SigningKey) -> pb::AgentSessionHello {
    let mut m = pb::AgentSessionHello {
        schema_version: 1,
        mode: 2,
        session_id: "session-1".into(),
        node_id: DEVICE.into(),
        connection_attempt: 2,
        issued_at_unix_ms: NOW,
        nonce: (0u8..16).collect(),
        ..Default::default()
    };
    m.node_signature = sign(k, &m).to_vec();
    m
}

fn resume_request(k: &SigningKey) -> pb::ResumeLeaseRequest {
    let mut m = pb::ResumeLeaseRequest {
        schema_version: 1,
        lease_id: "lease-1".into(),
        job_id: "job-1".into(),
        attempt_id: "attempt-1".into(),
        node_id: DEVICE.into(),
        fence_epoch: 4,
        session_id: "session-1".into(),
        connection_attempt: 2,
        issued_at_unix_ms: NOW,
        request_nonce: (16u8..32).collect(),
        ..Default::default()
    };
    m.node_signature = sign(k, &m).to_vec();
    m
}

fn resume_result(k: &SigningKey) -> pb::ResumeLeaseResult {
    let mut m = pb::ResumeLeaseResult {
        outcome: 1,
        detail: "resumed".into(),
        schema_version: 1,
        coordinator_id: DEVICE.into(),
        issued_at_unix_ms: NOW,
        request_nonce: (16u8..32).collect(),
        ..Default::default()
    };
    m.coordinator_signature = sign(k, &m).to_vec();
    m
}

const REVOKE_LEASE_ID: &str = "01JBXLEASE0000000000000001";

/// ★ `RevokeLeaseNotice` 에는 발급자 ID 필드가 없다 —
/// `signer_id()` 가 `lease_id` 를 대체값으로 쓴다(`TODO_VISION`
/// V-08, `crates/protocol/src/signable.rs:489-491`). 그래서 이
/// 메시지만 유일하게 `lease_id` 를 키 디렉터리 조회 키로 쓴다 —
/// `directory()` 가 등록하는 `DEVICE` 가 아니다.
fn revoke_notice(k: &SigningKey) -> pb::RevokeLeaseNotice {
    let mut m = pb::RevokeLeaseNotice {
        schema_version: 1,
        lease_id: REVOKE_LEASE_ID.into(),
        fence_epoch: 7,
        cause: 1,
        issued_at_unix_ms: NOW,
        ..Default::default()
    };
    m.coordinator_signature = sign(k, &m).to_vec();
    m
}

/// `revoke_notice()` 의 서명자 ID(`lease_id`)로 검증 키를 등록한
/// 별도 디렉터리 — `directory()` 는 `DEVICE` 로 등록하므로 이
/// 메시지 검증에는 쓸 수 없다.
fn revoke_directory(k: &SigningKey) -> InMemoryKeyring {
    let mut d = InMemoryKeyring::new();
    d.insert(REVOKE_LEASE_ID, k.verifying_key());
    d
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
    let frame = write_frame(FrameType::Grant, &g.encode_to_vec()).unwrap();
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

    assert!(
        matches!(msg, IngressMessage::Grant(_)),
        "잘못된 variant 로 디스패치됐다"
    );
}

/// 다른 타입(Lease)도 같은 스트림 구조에서 정상 동작하는가 — 비공허성.
#[test]
fn normal_lease_frame_dispatches_to_the_right_variant() {
    let k = key(2);
    let dir = directory(&k);
    let l = lease(&k);
    let frame = write_frame(FrameType::Lease, &l.encode_to_vec()).unwrap();
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

    assert!(
        matches!(msg, IngressMessage::Lease(_)),
        "잘못된 variant 로 디스패치됐다"
    );
}

/// `AgentGrantAck` 도 같은 스트림 구조에서 정상 동작하는가 — 비공허성.
///
/// coordinator/agent 최소 핸드셰이크(2026-08-18)가 추가한 10번째
/// `FrameType`. 이 테스트가 없으면 `framed_ingress` 의 dispatch 배선이
/// 컴파일만 되고 실제로 왕복하는지는 아무도 확인하지 않는다.
#[test]
fn normal_grant_ack_frame_dispatches_to_the_right_variant() {
    let k = key(3);
    let dir = directory(&k);
    let a = grant_ack(&k, 0);
    let frame = write_frame(FrameType::GrantAck, &a.encode_to_vec()).unwrap();
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    let msg = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("정상 GrantAck 프레임이 통과해야 한다");

    assert!(
        matches!(msg, IngressMessage::GrantAck(_)),
        "잘못된 variant 로 디스패치됐다"
    );
}

/// `RenewLeaseResult` 도 같은 스트림 구조에서 정상 동작하는가 — 비공허성.
///
/// Lease 갱신 최소 조각(2026-08-19)이 추가한 11번째 `FrameType`. 이
/// 테스트가 없으면 dispatch 배선이 컴파일만 되고 실제로 왕복하는지는
/// 아무도 확인하지 않는다.
#[test]
fn normal_renew_lease_result_frame_dispatches_to_the_right_variant() {
    let k = key(4);
    let dir = directory(&k);
    let r = renew_result(&k, 0);
    let expected_nonce: Vec<u8> = (0u8..16).collect();
    let frame = write_frame(FrameType::LeaseRenewResult, &r.encode_to_vec()).unwrap();
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    let msg = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("정상 RenewLeaseResult 프레임이 통과해야 한다");

    // ★ 코덱스 감사(2026-08-19, p116)가 지적 — 예전에는 variant 만
    //   확인하고 payload 필드값은 검증하지 않았다(DoD-17 이 고친
    //   RevokeLeaseNotice 테스트와 같은 얕음). Verified::get() 으로
    //   내부 값을 꺼내 원본 renew_result() 가 채운 값과 대조한다.
    let IngressMessage::LeaseRenewResult(verified) = msg else {
        panic!("잘못된 variant 로 디스패치됐다: {msg:?}");
    };
    let got = verified.get();
    assert_eq!(got.outcome, 1, "outcome 이 원본과 다르다");
    assert_eq!(got.detail, "ok", "detail 이 원본과 다르다");
    assert_eq!(got.schema_version, 1, "schema_version 이 원본과 다르다");
    assert_eq!(
        got.coordinator_id, DEVICE,
        "coordinator_id 가 원본과 다르다"
    );
    assert_eq!(
        got.issued_at_unix_ms, NOW,
        "issued_at_unix_ms 가 원본과 다르다"
    );
    assert_eq!(
        got.request_nonce, expected_nonce,
        "request_nonce 가 원본과 다르다"
    );
}

#[test]
fn resume_frames_round_trip_and_preserve_payloads() {
    let k = key(12);
    let dir = directory(&k);

    let cases = vec![
        (FrameType::SessionHello, session_hello(&k).encode_to_vec()),
        (FrameType::LeaseResume, resume_request(&k).encode_to_vec()),
        (
            FrameType::LeaseResumeResult,
            resume_result(&k).encode_to_vec(),
        ),
    ];
    for (frame_type, body) in cases {
        let frame = write_frame(frame_type, &body).unwrap();
        let mut stream = Cursor::new(frame);
        let mut replay = InMemoryReplayGuard::new();
        let message = read_frame(
            &mut stream,
            1,
            KeyDirectorySource::Provided(&dir),
            &mut replay,
            &FixedClock(NOW),
        )
        .expect("Resume frame should verify");
        match message {
            IngressMessage::SessionHello(v) => {
                assert_eq!(v.get().session_id, "session-1");
                assert_eq!(v.get().mode, 2);
            }
            IngressMessage::LeaseResume(v) => {
                assert_eq!(v.get().lease_id, "lease-1");
                assert_eq!(v.get().fence_epoch, 4);
            }
            IngressMessage::LeaseResumeResult(v) => {
                assert_eq!(v.get().outcome, 1);
                assert_eq!(v.get().detail, "resumed");
            }
            other => panic!("unexpected Resume dispatch variant: {other:?}"),
        }
    }
}

#[test]
fn forged_resume_signature_is_rejected() {
    let k = key(13);
    let dir = directory(&k);
    let mut message = resume_request(&k);
    message.node_signature[0] ^= 1;
    let frame = write_frame(FrameType::LeaseResume, &message.encode_to_vec()).unwrap();
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();
    assert!(matches!(
        read_frame(
            &mut stream,
            1,
            KeyDirectorySource::Provided(&dir),
            &mut replay,
            &FixedClock(NOW)
        ),
        Err(FramingError::Verify(_))
    ));
}

/// `RevokeLeaseNotice` 도 같은 스트림 구조에서 정상 동작하는가 —
/// 비공허성.
///
/// ★ 오래된 커버리지 공백(2026-08-19, 코덱스 감사 `p110` 이 지적) —
///   `FrameType::LeaseRevoke`/`IngressMessage::LeaseRevoke` 배선
///   자체는 이미 있었지만(`crates/crypto/src/framed_ingress.rs:89,
///   105, 173, 285`), 실제로 서명해 프레임으로 왕복하는 테스트가
///   한 번도 없었다 — `RenewLeaseResult`(11번째 타입)는 있는데
///   `RevokeLeaseNotice`(5번째 타입, 훨씬 먼저 추가됨)는 빠져
///   있었다.
#[test]
fn normal_revoke_lease_notice_frame_dispatches_to_the_right_variant() {
    let k = key(6);
    let dir = revoke_directory(&k);
    let n = revoke_notice(&k);
    let frame = write_frame(FrameType::LeaseRevoke, &n.encode_to_vec()).unwrap();
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    let msg = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("정상 RevokeLeaseNotice 프레임이 통과해야 한다");

    let IngressMessage::LeaseRevoke(verified) = msg else {
        panic!("잘못된 variant 로 디스패치됐다: {msg:?}");
    };
    let got = verified.get();
    assert_eq!(got.lease_id, REVOKE_LEASE_ID, "lease_id 가 원본과 다르다");
    assert_eq!(got.fence_epoch, 7, "fence_epoch 가 원본과 다르다");
    assert_eq!(got.cause, 1, "cause 가 원본과 다르다");
    assert_eq!(
        got.issued_at_unix_ms, NOW,
        "issued_at_unix_ms 가 원본과 다르다"
    );
}

/// `RevokeLeaseNotice` 의 위조된 서명이 프레이밍 계층에서 거부되는가.
#[test]
fn forged_revoke_lease_notice_signature_is_rejected() {
    let k = key(7);
    let dir = revoke_directory(&k);
    let mut n = revoke_notice(&k);
    n.coordinator_signature[0] ^= 0xFF;
    let frame = write_frame(FrameType::LeaseRevoke, &n.encode_to_vec()).unwrap();
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
        "위조된 RevokeLeaseNotice 서명이 프레이밍 계층을 통과했다: {result:?}"
    );
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
    let full = write_frame(FrameType::Grant, &g.encode_to_vec()).unwrap();
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

/// ★ **타입 불일치 — nested-decode 실패로 막히는 경우.**
///
/// ★ 2026-08-17 이름 정정 (독립 검수). 전에는 이 테스트가
/// "domain_tag 방어가 핵심" 이라고 주장했는데, 검수자가 실제로 확인하니
/// `ExecutionGrant` 의 필드 3 이 **중첩 메시지**(`JobManifest`) 라
/// `Lease` 의 필드 3(`string job_id`) 을 그 자리에 넣으면 **prost decode
/// 단계에서** 이미 실패한다 — `domain_tag` 방어를 시험하지 못한다.
///
/// 이 테스트는 그 경로(디코드 실패로 막히는 경우)를 이름 그대로 고정한다.
/// `domain_tag` 방어 자체는 아래 `distinct_types_with_colliding_wire_fields_are_rejected_by_domain_tag`
/// 가 검사한다.
#[test]
fn frame_type_mismatch_fails_at_nested_message_decode() {
    let k = key(4);
    let dir = directory(&k);
    // Lease 로 실제 서명된 바이트를, Grant 라고 주장하는 헤더에 담는다.
    let l = lease(&k);
    let disguised = write_frame(FrameType::Grant, &l.encode_to_vec()).unwrap();
    let mut stream = Cursor::new(disguised);
    let mut replay = InMemoryReplayGuard::new();

    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    // 검증이 통과해서는 안 된다 — 통과하면 위조 방어가 뚫린 것이다.
    // (이유가 decode 실패든 서명 실패든, 여기서는 "통과하지 않는다" 만 본다.)
    assert!(
        matches!(result, Err(FramingError::Verify(_))),
        "★ 헤더 타입을 속였는데 검증을 통과했다: {result:?}"
    );
}

/// ★ **진짜 domain_tag 방어 시험** — canonical 필드까지 우연히 일치하는 경우.
///
/// 독립 검수(2026-08-17)가 찾았다: `AttemptReport` 와 `ArtifactRef` 는
/// 필드 1(uint32)·2(string)·4(string)·90(bytes) 의 **와이어 타입이 겹친다.**
///
/// ```text
/// AttemptReport  1=schema_version  2=job_id     4=node_id     90=node_signature
/// ArtifactRef    1=schema_version  2=artifact_id 4=attempt_id 90=producer_signature
/// ```
///
/// 그리고 두 타입의 `signer_id()` 가 각각 `node_id`/`attempt_id` —
/// **같은 필드 번호(4)** 를 쓴다. 나머지 필드(3·5 등)를 기본값으로 두면
/// canonical 인코딩(규칙 b — 기본값 생략)이 **바이트까지 동일**해진다.
///
/// 그런데도 검증은 실패해야 한다 — `M::DOMAIN` 이 타입에서 정적으로
/// 오기 때문이다. 이 테스트가 실패한다면 서명 재사용이 실제로 가능하다는
/// 뜻이고, 그것은 이 모듈의 핵심 안전 주장이 깨졌다는 뜻이다.
#[test]
fn distinct_types_with_colliding_wire_fields_are_rejected_by_domain_tag() {
    let k = key(7);
    let dir = directory(&k);

    // AttemptReport 로 서명한다: field2=job_id, field4=node_id(=DEVICE).
    // field3(attempt_id)·field5(fence_epoch) 는 기본값으로 비워 둔다 —
    // 채우면 ArtifactRef 쪽 canonical 과 갈라져 이 시나리오가 성립하지 않는다.
    let mut report = pb::AttemptReport {
        schema_version: 1,
        job_id: "shared-string-value".into(),
        node_id: DEVICE.into(),
        ..Default::default()
    };
    report.node_signature = sign(&k, &report).to_vec();

    // 같은 바이트를 ArtifactRef 라고 주장하는 헤더에 담는다.
    let disguised = write_frame(FrameType::Artifact, &report.encode_to_vec()).unwrap();
    let mut stream = Cursor::new(disguised);
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
        "★★ canonical 필드가 겹치는 두 타입 사이에 서명이 재사용됐다 \
         — domain_tag 방어가 실제로 뚫렸다: {result:?}"
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
    let frame = write_frame(FrameType::Grant, &g.encode_to_vec()).unwrap();
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

    assert!(
        result.is_err(),
        "빈 프레임이 통과했다 — 있을 수 없는 값이다"
    );
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

    let mut combined = write_frame(FrameType::Grant, &g1.encode_to_vec()).unwrap();
    combined.extend(write_frame(FrameType::Grant, &g2.encode_to_vec()).unwrap());
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

// ══════════════════════════════════════════════════════════════════
// ★ 독립 검수(2026-08-17) — 오류 뒤 스트림 상태
// ══════════════════════════════════════════════════════════════════

/// `UnknownFrameType` 은 몸통을 비워서 스트림을 다음 프레임과 맞춘다.
///
/// 검수자가 지적했다: 전에는 이 오류가 몸통을 안 읽고 반환해서, 다음
/// `read_frame` 호출이 남은 몸통 바이트를 새 헤더로 오해했다. 지금은
/// 길이가 상한 이내로 확인된 뒤에만 타입을 검사하므로 안전하게 비울 수
/// 있다 — 그 사실을 이 테스트가 증명한다.
#[test]
fn unknown_frame_type_does_not_desync_the_stream() {
    let k = key(8);
    let dir = directory(&k);
    let g = grant(&k, 20);

    let mut stream_bytes = Vec::new();
    // 1번째 프레임: 알 수 없는 타입 + 실제로 존재하는 몸통(더미).
    let dummy_body = vec![0xABu8; 37];
    stream_bytes.push(0xFFu8);
    stream_bytes.extend_from_slice(&(dummy_body.len() as u32).to_be_bytes());
    stream_bytes.extend_from_slice(&dummy_body);
    // 2번째 프레임: 정상 Grant.
    stream_bytes.extend(write_frame(FrameType::Grant, &g.encode_to_vec()).unwrap());

    let mut stream = Cursor::new(stream_bytes);
    let mut replay = InMemoryReplayGuard::new();

    let first = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );
    assert!(
        matches!(first, Err(FramingError::UnknownFrameType(0xFF))),
        "{first:?}"
    );

    // ★ 스트림이 맞아떨어진다면 두 번째 프레임이 정상 통과해야 한다.
    let second = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );
    assert!(
        matches!(second, Ok(IngressMessage::Grant(_))),
        "★ UnknownFrameType 뒤 스트림이 어긋났다 — 다음 프레임을 못 읽는다: {second:?}"
    );
}

/// ★ `FrameTooLarge` 뒤에는 스트림을 **계속 읽으면 안 된다** — 위험을
/// 감추지 않고 고정한다.
///
/// 이 오류는 몸통을 읽지 않는다(그 자체가 DoS 이므로). 그래서 다음
/// `read_frame` 호출은 잔여 몸통 바이트를 헤더로 오해한다.
/// 이 테스트가 통과하는 것은 "안전하다" 는 뜻이 **아니라** —
/// `read_frame` 의 문서가 적은 대로 "호출자가 연결을 닫아야 한다" 는
/// 계약이 실제로 지켜지지 않으면 무슨 일이 나는지 보여줄 뿐이다.
#[test]
fn frame_too_large_leaves_the_stream_desynced_by_design() {
    let mut stream_bytes = Vec::new();
    stream_bytes.push(FrameType::Grant as u8);
    stream_bytes.extend_from_slice(&(MAX_INGRESS_FRAME_BYTES + 1).to_be_bytes());
    // 실제로는 그렇게 큰 몸통을 보내지 않는다 — 상한 초과는 몸통을
    // 안 읽으므로, 이 뒤에 온 바이트는 다음 read_frame 이 헤더로 읽는다.
    stream_bytes.extend_from_slice(b"XXXXX-not-a-real-frame-header");

    let mut stream = Cursor::new(stream_bytes);
    let dir = InMemoryKeyring::new();
    let mut replay = InMemoryReplayGuard::new();

    let first = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );
    assert!(matches!(first, Err(FramingError::FrameTooLarge { .. })));

    // ★ 이후 호출은 "XXXXX" 를 헤더로 오해해 의미 없는 결과를 낸다.
    //   여기서는 그것이 read_frame 의 정상 성공으로 보이지 않는다는 것만
    //   확인한다 — 이 상태에서 스트림을 계속 쓰면 안 된다는 계약의 근거다.
    let second = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );
    assert!(
        !matches!(second, Ok(_)),
        "★ 오염된 스트림에서 read_frame 이 뭔가를 '성공' 으로 반환했다 — \
         우연히 유효해 보이는 프레임을 만들 위험이 있다: {second:?}"
    );
}

/// `write_frame` 자체도 상한을 지켜야 한다 — 쓰기와 읽기가 같은 규칙을
/// 어길 수 있다는 것이 독립 검수(2026-08-17) 지적이었다.
#[test]
fn write_frame_rejects_bodies_over_the_limit() {
    // 실제로 8MiB+1 바이트를 할당하지 않는다 — 상한 검사가 몸통 길이만
    // 보고 거부하는지 확인하는 것이 목적이다. 정확히 상한을 넘는 벡터를
    // 만들되, 값 채우기 비용을 최소화하려고 0으로 채운다.
    let oversized = vec![0u8; (MAX_INGRESS_FRAME_BYTES + 1) as usize];
    let result = write_frame(FrameType::Grant, &oversized);
    assert!(
        matches!(result, Err(FramingError::FrameTooLarge { max, .. }) if max == MAX_INGRESS_FRAME_BYTES),
        "write_frame 이 자기 상한을 어겼다: {result:?}"
    );
}

/// 비공허성 — 상한 이내 몸통은 정상적으로 프레임이 된다.
#[test]
fn write_frame_accepts_bodies_within_the_limit() {
    let ok_body = vec![0u8; 128];
    assert!(write_frame(FrameType::Grant, &ok_body).is_ok());
}

// ═══════════════════════════════════════════════════════════════════════
// 이웃 신고 (ADR-033 §7 관측 층, 2026-08-30)
// ═══════════════════════════════════════════════════════════════════════

fn neighbor_report(k: &SigningKey, nonce_seed: u8) -> pb::NeighborUnreachableReport {
    let mut m = pb::NeighborUnreachableReport {
        schema_version: 1,
        reporter_node_id: "01JBXNODE0000000000000001".into(),
        // ★ signer_id() 는 reporter_device_id 다 — 빠뜨리면 UnknownSigner 다.
        reporter_device_id: DEVICE.into(),
        unreachable_node_id: "01JBXNODE0000000000000002".into(),
        coordinator_device_id: "01JBXCOORD000000000000001".into(),
        observed_at_unix_ms: NOW,
        request_nonce: (0u8..16).map(|i| i.wrapping_add(nonce_seed)).collect(),
        ..Default::default()
    };
    m.reporter_signature = sign(k, &m).to_vec();
    m
}

/// 정상 신고가 프레이밍 계층을 통과한다 — 비공허성.
///
/// 이것이 없으면 아래 거부 테스트들이 "애초에 아무것도 통과 못 하는"
/// 조건에서 우연히 통과할 수 있다.
#[test]
fn a_signed_neighbor_report_dispatches() {
    let k = key(21);
    let dir = directory(&k);
    let report = neighbor_report(&k, 0);
    let frame = write_frame(
        FrameType::NeighborUnreachableReport,
        &report.encode_to_vec(),
    )
    .unwrap();
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    let message = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("정상 신고는 통과해야 한다");

    match message {
        IngressMessage::NeighborUnreachableReport(verified) => {
            assert_eq!(
                verified.get().unreachable_node_id,
                "01JBXNODE0000000000000002"
            );
            assert_eq!(verified.signer_id(), DEVICE);
        }
        other => panic!("다른 메시지로 dispatch 됐다: {other:?}"),
    }
}

/// 위조된 서명은 거부된다.
#[test]
fn a_forged_neighbor_report_is_rejected() {
    let k = key(22);
    let dir = directory(&k);
    let mut report = neighbor_report(&k, 1);
    report.reporter_signature[0] ^= 0xFF;
    let frame = write_frame(
        FrameType::NeighborUnreachableReport,
        &report.encode_to_vec(),
    )
    .unwrap();
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
        "위조된 신고가 통과했다: {result:?}"
    );
}

/// ★ **같은 신고를 두 번 보내면 두 번째는 재생으로 거부된다.**
///
/// 재생 방어가 막는 것은 **신선도 위조와 중복 부작용**이다 — 어제의
/// "연락이 안 된다" 를 오늘 다시 보내는 것.
///
/// ★ 초안 주석은 "재생하면 이웃 하나가 `ADR-033` §8 조건 3 의 정족수를
///   혼자 채울 수 있다" 고 썼는데 **틀렸다**(2026-08-30 독립 검수 지적).
///   `crates/scheduler/src/reassignment.rs` 가 `reporter_node_id` 를
///   집합으로 중복 제거하므로 같은 신고를 N 번 넣어도 한 표다.
///
/// ★ `is_err()` 만 보면 저장소 오류·정책 오류도 성공으로 세므로
///   **정확한 오류**로 고정한다(같은 검수 지적).
#[test]
fn replaying_a_neighbor_report_is_rejected() {
    let k = key(23);
    let dir = directory(&k);
    let report = neighbor_report(&k, 2);
    let body = report.encode_to_vec();

    let mut replay = InMemoryReplayGuard::new();

    let mut first = Cursor::new(write_frame(FrameType::NeighborUnreachableReport, &body).unwrap());
    read_frame(
        &mut first,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("첫 신고는 통과해야 한다");

    // 같은 바이트를 그대로 다시 보낸다.
    let mut second = Cursor::new(write_frame(FrameType::NeighborUnreachableReport, &body).unwrap());
    let result = read_frame(
        &mut second,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    );

    assert!(
        matches!(
            result,
            Err(FramingError::Verify(IngressError::Verification(
                VerifyError::Outcome(VerifyOutcome::Replay)
            )))
        ),
        "재생이 정확히 Replay 로 거부되지 않았다 — 다른 이유로 실패하면 \
         이 테스트는 재생 방어를 재는 게 아니다: {result:?}"
    );
}

/// 신고 몸통을 **heartbeat 프레임으로** 보내면 거부된다.
///
/// ★ 초안은 이 테스트가 "domain 분리" 를 증명한다고 썼는데 **틀렸다**
///   (2026-08-30 독립 검수 3라운드 지적). 두 메시지는 canonical 필드
///   배치 자체가 달라서, domain tag 가 같아져도 서명은 어차피 실패한다 —
///   즉 이 테스트는 domain 분리 회귀를 고정하지 못한다.
///
///   domain tag 가 실제로 서로 다른지는
///   `crates/protocol/tests/t1b_grant_and_control.rs::all_domain_tags_are_distinct`
///   가 고정한다(뮤테이션으로 확인함). 여기서 고정하는 것은 **프레임
///   타입을 바꿔 보낸 몸통이 통과하지 못한다** 는 사실 하나다.
#[test]
fn a_neighbor_report_body_sent_as_a_heartbeat_frame_is_rejected() {
    let k = key(24);
    let dir = directory(&k);
    let report = neighbor_report(&k, 3);
    let frame = write_frame(FrameType::NodeHeartbeat, &report.encode_to_vec()).unwrap();
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
        result.is_err(),
        "신고 몸통이 heartbeat 프레임으로 통과했다: {result:?}"
    );
}

/// **디렉터리에 없는 신고자**는 `UnknownSigner` 로 거부된다.
///
/// ★ 이건 "정당한 이웃인가" 를 판정하는 것이 **아니다** — 키 디렉터리에
///   있는지만 본다. 멤버십 해소는 여전히 없다.
///
/// ★ 초안은 이 테스트를 "unknown signer" 라고 불렀지만 실제로는 **같은
///   signer ID 에 다른 공개키**를 등록해서 `InvalidSignature` 가 나왔다
///   (2026-08-30 독립 검수 3라운드 지적). `is_err()` 만 봐서 그 불일치가
///   가려졌다. 둘은 다른 방어이므로 테스트도 둘로 나눈다.
#[test]
fn a_report_from_a_signer_absent_from_the_directory_is_rejected() {
    let k = key(25);
    // 디렉터리에는 **다른 ID** 를 등록한다 — 이 신고자는 아예 없다.
    let mut dir = InMemoryKeyring::new();
    dir.insert("01JBXOTHERDEVICE000000000A", key(26).verifying_key());

    let report = neighbor_report(&k, 4);
    let frame = write_frame(
        FrameType::NeighborUnreachableReport,
        &report.encode_to_vec(),
    )
    .unwrap();
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
        matches!(
            result,
            Err(FramingError::Verify(IngressError::Verification(
                VerifyError::Outcome(VerifyOutcome::UnknownSigner)
            )))
        ),
        "디렉터리에 없는 신고자가 UnknownSigner 로 거부되지 않았다: {result:?}"
    );
}

/// **등록은 됐지만 키가 다른** 신고자는 `InvalidSignature` 로 거부된다.
///
/// 위와 다른 방어다 — 이쪽은 "이 ID 는 아는데 이 서명은 그 키로 만든 게
/// 아니다" 다.
#[test]
fn a_report_signed_by_the_wrong_key_is_rejected() {
    let k = key(25);
    // 같은 ID 에 **다른 키**를 등록한다.
    let dir = directory(&key(26));

    let report = neighbor_report(&k, 4);
    let frame = write_frame(
        FrameType::NeighborUnreachableReport,
        &report.encode_to_vec(),
    )
    .unwrap();
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
        matches!(
            result,
            Err(FramingError::Verify(IngressError::Verification(
                VerifyError::Outcome(VerifyOutcome::InvalidSignature)
            )))
        ),
        "잘못된 키로 서명한 신고가 InvalidSignature 로 거부되지 않았다: {result:?}"
    );
}

/// ★ **한 장치 키가 서로 다른 `reporter_node_id` 를 서명할 수 있다.**
///
/// 서명은 "이 **장치**가 보냈다" 만 증명하고, 그 장치가 주장한 **기계
/// ID** 의 실제 소유자인지는 증명하지 않는다(2026-08-30 독립 검수 지적).
/// `ADR-033` §8 조건 3 의 정족수는 기계 수로 세므로, 이 결합을 해소하지
/// 않고 세면 장치 하나가 N 표를 만들 수 있다.
///
/// 이 테스트는 **그 구멍이 열려 있음을 고정한다** — 나중에 닫히면 이
/// 테스트가 실패하며 문서도 같이 고치라고 알린다. `runtime-linux` 의
/// "탈출이 성공하기를 기대하는 테스트" 와 같은 장치다.
#[test]
fn one_device_key_can_sign_many_reporter_node_ids_today() {
    let k = key(28);
    let dir = directory(&k);

    // ★ replay guard 를 **하나만** 쓴다. 반복마다 새로 만들면 nonce 실수를
    //   숨긴다(2026-08-30 독립 검수 3라운드 지적) — 초안은 두 nonce 의 앞
    //   16바이트가 같았는데도 매번 새 guard 를 만들어 통과했다.
    let mut replay = InMemoryReplayGuard::new();

    for (index, node_id) in ["01JBXNODE000000000000000A", "01JBXNODE000000000000000B"]
        .into_iter()
        .enumerate()
    {
        let mut report = neighbor_report(&k, 6);
        report.reporter_node_id = node_id.into();
        // nonce 를 실제로 다르게 만든다 — 같은 문맥에서 둘 다 통과해야 하므로.
        report.request_nonce = (0u8..16).map(|b| b ^ (index as u8 + 1)).collect();
        report.reporter_signature = sign(&k, &report).to_vec();

        let frame = write_frame(
            FrameType::NeighborUnreachableReport,
            &report.encode_to_vec(),
        )
        .unwrap();
        let mut stream = Cursor::new(frame);

        let message = read_frame(
            &mut stream,
            1,
            KeyDirectorySource::Provided(&dir),
            &mut replay,
            &FixedClock(NOW),
        )
        .expect("서명 계층은 기계 ID 주장을 검사하지 않는다 — 지금은 통과한다");

        match message {
            IngressMessage::NeighborUnreachableReport(verified) => {
                assert_eq!(verified.get().reporter_node_id, node_id);
                assert_eq!(
                    verified.signer_id(),
                    DEVICE,
                    "서명자는 장치 하나인데 기계 ID 만 바뀌었다"
                );
            }
            other => panic!("{other:?}"),
        }
    }
}

/// ★ **프레이밍 계층은 `coordinator_device_id` 를 자기 ID 와 대조하지
///   않는다.**
///
/// proto 주석이 초안에 "다른 Coordinator 로 보낸 신고를 재사용할 수 없게
/// 한다" 고 썼는데 강제되지 않는 보장이었다(`CLAUDE.md` §0.4).
/// canonical 에 들어가므로 **변조**는 막히지만 **대조**는 소비자 몫이다.
///
/// 이 테스트도 그 사실을 고정한다.
#[test]
fn the_framing_layer_does_not_check_the_target_coordinator_today() {
    let k = key(29);
    let dir = directory(&k);
    let mut report = neighbor_report(&k, 7);
    report.coordinator_device_id = "01JBXCOORD00000000000OTHER".into();
    report.reporter_signature = sign(&k, &report).to_vec();

    let frame = write_frame(
        FrameType::NeighborUnreachableReport,
        &report.encode_to_vec(),
    )
    .unwrap();
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    let message = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW),
    )
    .expect("프레이밍 계층은 수신 Coordinator 를 대조하지 않는다 — 지금은 통과한다");

    match message {
        IngressMessage::NeighborUnreachableReport(verified) => assert_eq!(
            verified.get().coordinator_device_id,
            "01JBXCOORD00000000000OTHER",
            "값은 그대로 전달된다 — 대조는 소비자가 해야 한다"
        ),
        other => panic!("{other:?}"),
    }
}

/// **너무 오래된 신고는 거부된다** — `Lifetime::ShortLived`.
///
/// 오래된 신고가 통과하면 "지금 연락이 안 된다" 를 어제 관측으로 주장할
/// 수 있다.
#[test]
fn a_stale_neighbor_report_is_rejected() {
    let k = key(27);
    let dir = directory(&k);
    let report = neighbor_report(&k, 5);
    let frame = write_frame(
        FrameType::NeighborUnreachableReport,
        &report.encode_to_vec(),
    )
    .unwrap();
    let mut stream = Cursor::new(frame);
    let mut replay = InMemoryReplayGuard::new();

    // TTL 을 훌쩍 넘긴 시각에 읽는다.
    let result = read_frame(
        &mut stream,
        1,
        KeyDirectorySource::Provided(&dir),
        &mut replay,
        &FixedClock(NOW + 3_600_000),
    );

    assert!(result.is_err(), "만료된 신고가 통과했다: {result:?}");
}
