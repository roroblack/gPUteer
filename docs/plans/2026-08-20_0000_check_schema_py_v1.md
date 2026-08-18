# 2026-08-20_0000_check_schema_py_v1

- **기준선:** `proto/README.md:70`(사용 안내)·`proto/README.md:97`(M1-02
  미완료로 명시). `tools/canonical/reference_canonical.py:80-84` 도
  "표와 .proto 가 어긋나면 CI 가 잡도록 tools/canonical/check_schema.py
  를 둔다"고 이미 명시하고 있었다 — 실제 파일이 없었다.
- **대상 단계:** v0.1 (도구 스크립트, 런타임 코드 아님)
- **선행 게이트:** 없음.
- **상태:** 설계 완료(코덱스 `p119`). 구현 착수.

★ 이 문서는 코덱스(`agent:codex-cli`, read-only 샌드박스)의 설계
응답(`p119` 프롬프트)을 정리한 것이다.

## 왜 지금 이 조각인가

이 세션이 오늘 완료한 마지막 후보들(`DoD-17`~`19`) 이후, 코덱스
감사(`p118`)가 이 저장소 안에서 진행 가능한 마지막 후보로 이 공백을
찾았다. `tools/canonical/reference_canonical.py` 의 `SCHEMAS`
딕셔너리는 각 서명 대상 메시지의 필드 표를 **손으로** 옮겨 적은
것이고("실제 .proto 를 파싱하지 않고 필드 표를 손으로 둔다",
`reference_canonical.py:81`), `.proto` 가 바뀌어도 자동으로 동기화
되지 않는다 — 이 세션이 오늘 발견한 여러 "손으로 쓴 목록이 실제와
어긋나는" 결함(`DoD-02`·`DoD-06`·`DoD-19`)과 같은 종류의 위험이다.

## 설계 단계 실측 (코덱스 `p119`)

- `.proto` 메시지 선언 85개, `SCHEMAS` 43개. `SCHEMAS` 에만 있는
  메시지는 0개 — 전부 `.proto` 에 실재한다.
- 표본 대조(`Digest`·`JobManifest`·`Lease`·`RenewLeaseResult`)는
  전부 필드 번호·이름·타입이 일치했다.
- **실제 drift 3건 발견**: `RevokeDevice.signatures`·
  `UpdatePolicy.signatures`·`QuarantineDevice.verdict_signatures` —
  `SCHEMAS` 는 전부 `bytes` 로 적혀 있지만 `.proto` 실제 선언은
  `repeated bytes` 다(`proto/control.proto:310-354`). 다만 이 세
  필드는 전부 field number 90(서명 필드)이고,
  `reference_canonical.py:546,877` 의 `canonical_encode()` 가
  **field number 로만** field 90 을 무조건 건너뛴다 — 선언된 kind 는
  이 세 필드에 한해 실제로 한 번도 소비되지 않는다. 그래서 지금까지
  아무 벡터도 이 drift 로 깨지지 않았다.
- `ExecutionGrant.manifest_hash` 는 `.proto` 에 있지만 `SCHEMAS` 에는
  의도적으로 빠져 있다(derived hash field) — 일반 누락과 구분해야
  한다.
- `.proto` 에만 있는 메시지 42개는 애초에 `SCHEMAS` 가 서명 대상과
  그 nested helper만 다루려는 설계라서 정상이다.

## 범위

### In

- `tools/canonical/check_schema.py` 신설 — `protoc
  --descriptor_set_out`(`--include_imports --include_source_info`)
  로 `FileDescriptorSet` 을 뽑아 `google.protobuf.descriptor_pb2`
  로 파싱하고, `reference_canonical.py` 를 import 해 `SCHEMAS` 와
  대조한다.
- 검사: field number 일치, field name 일치, kind 일치(정규화 매핑
  표 기준), nested message 참조 일치. `SCHEMAS` 에 있는데 `.proto`
  에 없으면 오류.
- **field number 90(서명 필드)은 타입 비교에서 제외**한다 —
  `canonical_encode()` 가 field number 로만 무조건 건너뛰므로,
  선언된 kind 가 실제 wire 타입과 달라도 인코딩 결과에 영향이
  없다. `SCHEMAS` 를 건드리지 않고 이 사실을 검사기 쪽에서
  명시적으로 반영한다(설계 p119 가 제안한 "derived-field 예외"와
  같은 종류의 명시적 예외 처리).
- `ExecutionGrant.manifest_hash` 같은 의도된 생략은 별도 허용
  목록(`DERIVED_HASH_FIELDS` 등 기존에 있다면 재사용, 없으면
  스크립트 내 상수)으로 명시 예외 처리 — "표에 없으면 전부 오류"로
  구현하지 않는다.
- `.proto` 에만 있고 `SCHEMAS` 에 없는 메시지는 기본 **경고**(오류
  아님) — 지금 42개가 여기 해당하고, `SCHEMAS` 는 원래 서명 대상만
  다루는 설계이기 때문이다.
- 종료 코드: `0`=오류 없음(경고 허용), `1`=schema mismatch,
  `2`=실행 환경 오류(protoc 없음 등).
- human 출력(기본) + `--json`(선택).

### Out

- `reference_canonical.py` 의 `SCHEMAS`/canonical 인코딩 로직 자체
  변경.
- `DOMAIN_TAGS` 의 의미·값 변경 — 이번 조각은 field 표 정합성만
  본다. `DOMAIN_TAGS` coverage 는 경고로만 다루고 기본 오류로
  만들지 않는다(코덱스 지적 — `Audit`/`Genesis`/`Invite`/`Release`
  가 `DOMAIN_TAGS` 에는 있지만 메시지 자체가 없어, 기본 오류로
  만들면 저장소가 즉시 실패한다).
- Rust `build.rs` 변경, `prost-build` 설정 변경.
- CI 파이프라인에 실제로 연결(스크립트만 만든다 — 언제 CI 에
  연결할지는 이 저장소의 CI 설정 자체가 범위 밖이다).

## 단계

| # | 단계 | 완료 기준 | 상태 |
|---|---|---|---|
| 1 | `check_schema.py` 뼈대 — descriptor 생성·파싱 함수 | `protoc` 실행 성공, `FileDescriptorSet` 파싱 성공 | ⬜ |
| 2 | `SCHEMAS` 대조 로직 + field 90/derived-field 예외 처리 | 현재 저장소 상태에서 오류 0건(경고는 허용) | ⬜ |
| 3 | 실패 케이스 뮤테이션 — `SCHEMAS` 를 일부러 틀리게 해서 검사기가 잡는지 확인 | 뮤테이션 3종 모두 정확한 이유로 실패 보고 | ⬜ |
| 4 | 코덱스 독립 검수 + evidence 기록(`DoD-20`) | `ACCEPTED` | ⬜ |

## 완료 기준 (DoD)

- [ ] 현재 저장소 상태에서 `check_schema.py` 실행 시 종료 코드 0
      (오류 0건).
- [ ] field number 불일치·이름 불일치·타입 불일치를 각각 실제로
      잡아내는 뮤테이션 테스트(negative test).
- [ ] field 90 예외·derived-field 예외가 실제로 오탐을 만들지
      않음을 확인.
- [ ] 코덱스 독립 검수 `ACCEPTED`.

## 기준선과 다른 점

없음 — `reference_canonical.py` 자신이 이미 이 도구의 존재를
전제하고 있었다(`:83-84`). 이 조각은 그 전제를 실제로 채운다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-20 00:00 | 코덱스 설계(`p119`) 정리 — 구현 착수 |
