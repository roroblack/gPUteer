---
schema_version: 2
id: DoD-17
claim: "RevokeLeaseNotice 는 실제로 서명해 FrameType::LeaseRevoke 프레임으로 왕복시키면 IngressMessage::LeaseRevoke variant 로 정확히 디스패치되고, 디코드된 페이로드 필드(lease_id/fence_epoch/cause/issued_at_unix_ms)가 원본과 정확히 일치한다. 위조된 coordinator_signature 는 프레이밍 계층(FramingError::Verify)에서 거부된다 — 배선 자체는 이전 세션부터 있었지만, 실제 서명·검증·payload 무결성을 확인하는 테스트는 이번이 처음이다"
status: PASS
commit: a02ea148bafe26d993a9ea202e25f8154e49dd6e

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-19T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high (1라운드) / medium (2라운드)"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "정상 경로 테스트의 payload 필드 검증 깊이, 위조 서명 테스트가 실제 서명 변조인지(대체 실패 원인 배제), revoke_directory() 별도 키링의 필요성(signer_id()==lease_id 대체 규약 확인), 기존 RenewLeaseResult 패턴과의 일관성, replay 케이스 범위 판단, Verified<M>::get() 공개 API 검증. 1라운드 — CHANGES_REQUESTED(정상 테스트가 variant 만 확인하고 필드값 미검증), 수정 후 2라운드 좁은 후속 검수 — ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-17_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-17_framed_ingress_2026-08-19.txt"
raw_output_digest: "sha256:1a60004ac1ca576d23f1aa591f6b035051a963c08b5387116f82435ad309ece5"
raw_output_bytes: 30180

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "RevokeLeaseNotice.schema_version = 1 (proto/lease.proto). read_frame() 은 max supported version 1 로 검증 — 이 조각은 스키마 자체를 바꾸지 않는다"
  canonical_spec: "docs/protocol/signing.md v1 (변경 없음)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 순수 in-process 프레임 왕복 테스트"
network_profile: "해당 없음 — 실제 소켓을 쓰지 않는다. std::io::Cursor 로 메모리 안에서 프레임을 왕복시킨다(기존 framed_ingress.rs 전체 테스트와 같은 방식)"
command: |
  cargo test -p gputeer-crypto --test framed_ingress
  cargo test --workspace
raw_output: |
  (docs/evidence/_raw/DoD-17_framed_ingress_2026-08-19.txt 전문 참조)

  test normal_revoke_lease_notice_frame_dispatches_to_the_right_variant ... ok
  test forged_revoke_lease_notice_signature_is_rejected ... ok

  framed_ingress.rs: test result: ok. 20 passed; 0 failed (18개 기존 + 2개 신규)
  cargo test --workspace: 전체 스위트 통과, 0 failed
artifacts:
  - crates/crypto/tests/framed_ingress.rs
  - docs/evidence/_raw/DoD-17_framed_ingress_2026-08-19.txt
  - docs/evidence/_raw/DoD-17_review.txt
negative_tests:
  - "forged_revoke_lease_notice_signature_is_rejected — coordinator_signature 첫 바이트를 실제로 XOR 변조(n.coordinator_signature[0] ^= 0xFF)한 뒤 read_frame() 이 FramingError::Verify 로 거부하는지 확인. 코덱스가 별도로 확인 — revoke_directory() 가 실제 서명자의 공개키를 등록하므로 '키가 없어서 실패'가 아니라 '위조된 서명이 검증에서 거부'되는 구조가 맞다(signer_id()가 lease_id 를 반환하는 실제 구현을 signable.rs 에서 직접 대조)"
  - "★ 코덱스 1라운드(p112)가 찾은 실제 결함 — 정상 경로 테스트가 IngressMessage::LeaseRevoke(_) variant 만 matches!() 로 확인하고 lease_id/fence_epoch/cause/issued_at_unix_ms 값은 assert 하지 않아, payload 가 잘못 디코드되거나 필드가 소실돼도 테스트가 통과할 수 있었다(RenewLeaseResult 쪽 기존 테스트도 같은 얕음이 있었음을 코덱스가 지적). Verified::get() 으로 내부 값을 꺼내 원본 4개 필드와 정확히 일치하는지 assert 하도록 수정 — 2라운드(p113)에서 ACCEPTED. schema_version 은 별도 assert 하지 않아도 read_frame() 의 max-version 검증과 서명 입력 포함으로 이미 간접 보증된다고 코덱스가 확인"
  - "revoke_directory() 가 표준 directory() 헬퍼(DEVICE 키 등록)와 별도로 필요한 이유를 코덱스가 signable.rs:489-491 에서 직접 확인 — RevokeLeaseNotice::signer_id() 가 서명자 ID 전용 필드 없이 lease_id 를 대체값으로 반환한다(TODO_VISION V-08 로 이미 알려진 장기 스키마 공백)"
limitations:
  - "이 조각은 순수 테스트 추가다 — 프로덕션 코드(framed_ingress.rs 본체, signable.rs)는 건드리지 않았다. 코덱스 검수도 이 변경 범위에서 별도 프로덕션 결함을 발견하지 못했다고 명시했다"
  - "replay(같은 nonce 재사용) 케이스는 범위 밖이다 — RevokeLeaseNotice 는 Evidence 수명(장수명)이라 replay 검사가 ShortLived 메시지에만 적용되는 기존 설계상 대상이 아니다(코덱스가 signing.rs:784-826, signable.rs:462-469 로 확인)"
  - "실제 revoke 정책(Coordinator 가 언제 발행하는지, Agent 가 수신 후 보유 Lease 를 어떻게 폐기하는지)은 여전히 범위 밖이다 — 이 조각은 framed ingress 계층의 서명·검증·dispatch 만 증명한다"
  - "코덱스는 read-only 샌드박스에 cargo 가 없어 cargo test 를 직접 실행하지 못했다 — 정적 코드 검토만 했다고 두 라운드 모두 명시. 실행 검증은 claude-code 세션이 직접 수행했다"
  - "Windows 단일 플랫폼에서만 실행했다"
decision: "RevokeLeaseNotice 의 framed ingress 커버리지 공백(코덱스 감사 p110 이 지적한 오래된 미보강 항목)을 메웠다. 1라운드 검수가 정상 테스트의 얕음(variant 만 확인)을 실제로 찾아냈고, 이를 payload 필드 4종 assert 로 고쳐 2라운드에서 ACCEPTED 를 받았다 — 이 세션의 다른 조각들과 같은 패턴(설계/구현 뒤 독립 검수가 실제 결함을 찾고, 좁은 후속 검수로 마무리)이 순수 테스트 추가에서도 동일하게 유효했다."
---

# DoD-17 · RevokeLeaseNotice framed ingress 커버리지

## 무엇을 입증하려 했는가

코덱스 감사(`p110`, `DoD-15`·`DoD-16` 완료 직후 저장소 전체를 훑어
다음 후보를 찾도록 요청한 실측)가 오래전부터 있었지만 손대지 않은
작은 커버리지 공백 하나를 찾았다.

`RevokeLeaseNotice` 는 `FrameType::LeaseRevoke`/
`IngressMessage::LeaseRevoke` 배선이 `crates/crypto/src/framed_ingress.rs`
에 이미 있었다(이전 세션에서 추가됨). 하지만 다른 모든 서명 대상
메시지 타입과 달리, **실제로 서명해 프레임으로 왕복시키고 위조
서명을 거부하는지 확인하는 테스트가 한 번도 없었다** —
`RenewLeaseResult`(가장 최근에 추가된 11번째 서명 대상 타입)는
정상/위조 테스트가 있는데, `RevokeLeaseNotice`(5번째 타입, 훨씬
먼저 추가됨)는 빠져 있었다.

## 구현

`crates/crypto/tests/framed_ingress.rs` 에 추가:

- `REVOKE_LEASE_ID` 상수, `revoke_notice()`/`revoke_directory()`
  헬퍼 — `RevokeLeaseNotice::signer_id()` 가 전용 필드 없이
  `lease_id` 를 대체값으로 쓰는 규약(`TODO_VISION` V-08) 때문에
  기존 `directory()`(DEVICE 키 등록) 대신 별도 키링이 필요하다.
- `normal_revoke_lease_notice_frame_dispatches_to_the_right_variant`
  — 정상 서명 → 프레임 왕복 → `IngressMessage::LeaseRevoke` 로
  디스패치 확인.
- `forged_revoke_lease_notice_signature_is_rejected` — 서명 바이트
  변조 후 `FramingError::Verify` 로 거부되는지 확인.

## 코덱스 1라운드(`p112`) — 실제 결함 발견

정상 경로 테스트가 `matches!(msg, IngressMessage::LeaseRevoke(_))`
로 variant 만 확인하고, 디코드된 페이로드 값(`lease_id`·
`fence_epoch`·`cause`·`issued_at_unix_ms`)은 전혀 assert 하지
않았다 — payload 가 잘못 디코드되거나 필드가 통째로 소실돼도
이 테스트는 계속 통과할 수 있는 구조였다. 위조 서명 테스트와
`revoke_directory()` 의 필요성은 문제없다고 확인했다.

## 수정

`Verified<M>::get()` 으로 검증된 내부 값을 꺼내 원본 4개 필드와
정확히 일치하는지 `assert_eq!` 로 확인하도록 고쳤다:

```rust
let IngressMessage::LeaseRevoke(verified) = msg else {
    panic!("잘못된 variant 로 디스패치됐다: {msg:?}");
};
let got = verified.get();
assert_eq!(got.lease_id, REVOKE_LEASE_ID, "lease_id 가 원본과 다르다");
assert_eq!(got.fence_epoch, 7, "fence_epoch 가 원본과 다르다");
assert_eq!(got.cause, 1, "cause 가 원본과 다르다");
assert_eq!(got.issued_at_unix_ms, NOW, "issued_at_unix_ms 가 원본과 다르다");
```

## 코덱스 2라운드(`p113`) — ACCEPTED

수정된 assert 가 `revoke_notice()` 원본 값과 정확히 일치함을
확인했다. `schema_version` 은 별도 assert 하지 않지만, `read_frame()`
의 max-version 검증과 서명 입력 포함으로 이미 간접 보증된다고
판단해 필수 지적 사항에서 제외했다. `Verified<M>::get()` 이 실제
검증된 내부 값을 반환하는 공개 API 임도 `signing.rs` 에서 직접
확인했다.

## 결과

```text
cargo test -p gputeer-crypto --test framed_ingress   20 passed (18 기존 + 2 신규)
cargo test --workspace                                전체 스위트 통과, 0 failed
```

## 이 실험이 증명하지 "않는" 것

- 프로덕션 코드(`framed_ingress.rs` 본체·`signable.rs`)는 이 조각의
  범위 밖이다 — 순수 테스트 추가다.
- replay 케이스는 범위 밖이다 — `RevokeLeaseNotice` 는 Evidence
  수명이라 replay 검사 대상이 아니다.
- 실제 revoke 정책(Coordinator 발행 시점, Agent 상태 폐기)은
  범위 밖이다.
- Windows 단일 플랫폼.

## 결정

1. 커버리지 공백을 메웠다 — 코덱스 감사(`p110`)가 지적한 대로.
2. 1라운드 검수가 실제 결함(얕은 assert)을 찾아냈고, 수정 후
   2라운드에서 `ACCEPTED` — 이 세션의 다른 조각들과 같은 검수
   패턴이 순수 테스트 추가에서도 유효했다.

관련: `docs/evidence/DoD-16_coordinator_영속_lease_저장소.md` ·
`CLAUDE.md` "다음에 할 일" 6번(참조 구현 교차검증, 별도 완료)
