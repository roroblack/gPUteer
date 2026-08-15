# proto/ — 스키마의 유일한 원본

이 디렉터리가 gPUteer 와이어 스키마의 **단일 원본(single source of truth)** 이다.
계획서(`gputeer_master_implementation_plan_v5.md`)에 적힌 protobuf 조각과 어긋나면
**이 디렉터리가 옳다.**

## 왜 분리했나

계획서 v5까지는 스키마가 마크다운 문서 안의 코드 블록으로만 존재했다.
그 결과 두 차례 검토에서 매번 같은 종류의 결함이 나왔다.

- 필드는 늘었지만 12개 타입이 **이름만 있고 정의가 없었다**
- `manifest_hash` 가 메시지 안에 있어 **자기참조 순환**이 생겼다
- **canonical serialization 규칙이 없어** 서명 검증이 성립하지 않았다
- 장수명/단수명 문서가 섞여 **정상 Job이 100% 거부되는** 시간 규칙이 들어갔다

산문 문서는 이런 것을 잡아내지 못한다. 컴파일되는 스키마와 실행되는 테스트만이 잡는다.

**역할 분담**

```text
gputeer_master_implementation_plan_v5.md   왜 그렇게 결정했는가
proto/*.proto                              정확히 무엇을 주고받는가
docs/protocol/signing.md                   무엇에 어떻게 서명하는가
docs/protocol/state-machines.md            어떤 전이가 허용되는가
```

## 파일

| 파일 | 내용 |
|---|---|
| `common.proto` | 공통 타입. 검토에서 "이름만 있다"고 지적된 타입 전부를 여기서 확정 |
| `job.proto` | `JobManifest`(장수명) / `ExecutionGrant`(단수명) / 배치 근거 |
| `lease.proto` | `Lease` / 갱신 프로토콜 / fence watermark / operation_id |
| `artifact.proto` | Durability Contract / `ReplicaAck` / 체크포인트 / canonical 결정 |
| `control.proto` | `ControlStore` 계약 / `ControlAction` / 조회·구독 |

## 전역 불변식

새 메시지·필드를 추가할 때 반드시 지킨다.

```text
MUST NOT   float / double 필드 추가
             → 비율은 ppm 정수, 시각은 밀리초 정수
             → IEEE-754 표현 차이가 canonical 인코딩을 깬다

MUST       서명 필드는 field number 90 을 쓴다
             → canonical 인코딩에서 자동 제외된다

MUST       서명 대상 메시지는 schema_version 을 갖는다
             → 검증자가 모르는 필드가 있으면 SCHEMA_TOO_NEW 를 반환한다

MUST NOT   메시지 안에 자기 자신의 해시를 저장한다
             → manifest_hash 는 signing.md §6.1 로 도출한다

MUST       새 서명 대상 메시지는 signing.md §5 의 domain_tag 표에 등록한다
             → 등록하지 않으면 다른 문맥의 서명을 재사용할 수 있다

MUST NOT   기존 field number 재사용 / 타입 변경 / 의미 변경
             → 제거한 번호는 reserved 로 표시한다
```

## 생성

```bash
# Rust
cargo build -p gputeer-protocol          # build.rs 가 prost-build 실행

# 스키마 표 정합성 (참조 구현의 필드 표 ↔ .proto)
python tools/canonical/check_schema.py   # TODO: M1-02

# canonical 테스트 벡터
python tools/canonical/reference_canonical.py --self-test
python tools/canonical/reference_canonical.py --emit-vectors \
    > tests/vectors/canonical_v1.json
```

## 주의: prost 기본 인코더를 서명에 쓰지 말 것

`prost::Message::encode()` 는 `signing.md` §3 의 canonical 규칙을 보장하지 않는다.
서명·해시 계산에는 **반드시** `canonical_encode` 를 쓴다.

참조 구현: `tools/canonical/reference_canonical.py`
Rust 구현은 `tests/vectors/canonical_v1.json` 에 대해 대조 검증한다.

## 현재 상태

```text
✅ 타입 정의 완료 (검토에서 지적된 12개 미정의 타입 해소)
✅ canonical 규칙 확정 + 참조 구현 + 테스트 벡터 12건
✅ 장수명/단수명 분리
✅ Lease 전체 필드 + 갱신 프로토콜
✅ Durability Contract + 서명된 ReplicaAck
✅ ControlStore 의미론 (ProposeOutcome 5분기)

⬜ prost-build 연동 (M1-01)
⬜ canonical_encode Rust 구현 (M1-02)
⬜ check_schema.py — 필드 표와 .proto 정합성 검사 (M1-02)
⬜ 임계 서명 방식 확정 (signing.md §14 항목 1)
```
