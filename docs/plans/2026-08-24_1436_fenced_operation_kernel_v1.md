# FencedOperation 판정 순수 kernel v1

- 작성 시각: 2026-08-25 14:36 KST
- 문서 파일명 날짜는 요청된 `2026-08-24_HHMM_...` 형식을 따른다.
- 구현 배치: `crates/protocol/src/fenced_operation.rs`
- 테스트 배치: `crates/protocol/tests/fenced_operation_kernel.rs`

## 1. 범위와 배치 근거

`proto/lease.proto`의 `FencedOperation`과 `proto/common.proto`의 `Digest`는
`gputeer-protocol`이 생성하는 `pb` 타입이다. 같은 크레이트에는 규범 공식에 필요한
`canonical::blake3_256`도 있고, 기존 순수 protocol 판정 조각인
`watch_continuity.rs`도 있다. 따라서 다른 저장소나 실행 계층에 hash 규칙을 복제하지 않고
`gputeer-protocol`에 additive 공개 API를 둔다.

이 조각은 Hub/CAS 저장, watermark 갱신, TTL, 캐시 만료, 재시도 횟수, 외부 API 호출,
Job/Attempt/Checkpoint 상태 전이를 구현하지 않는다. I/O, 시계, 난수, 전역 상태도 읽지
않는다.

## 2. 입력과 결과

핵심 함수는 다음 입력만 소비한다.

- `job_id: &str`
- `attempt_id: &str`
- 생성된 protobuf `FencedOperation`
- caller가 제공한 이전 `FencedOperationKey` 관측 slice

이전 관측을 단일 마지막 키가 아니라 slice로 받는 이유는 A, B를 관측한 뒤 A가 다시 온
경우도 exact tuple 재전송으로 찾기 위해서다. 이 slice의 보존 방식이나 기간은 규범에 없고
kernel이 정하지 않는다.

결과는 다음 네 종류다.

| 결과 | 조건 |
|---|---|
| `Accept` | 검증된 `(fence_epoch, operation_id)`가 새 키이고 더 낮은 fence가 아님 |
| `IdempotentRetransmission` | exact `(fence_epoch, operation_id)`가 이전 관측에 있음 |
| `StaleFence` | 입력 epoch보다 큰 이전 epoch가 하나라도 있음 |
| `Invalid` | identity/digest 누락, algorithm/길이 오류, 공식 재계산 불일치 |

낮은 epoch의 exact tuple이 이전 관측에 있더라도 `StaleFence`가 우선한다. 더 높은 fence를
이미 관측한 뒤 낮은 fence를 멱등 처리 대상으로 되살리지 않는 fail-closed 선택이다.

## 3. operation_id 도출

구현은 caller의 digest value를 키로 채택하지 않는다. 다음 바이트를 새 `Vec<u8>`에 정확한
순서로 붙이고 기존 `blake3_256`로 다시 계산한다.

```text
job_id.as_bytes()
|| attempt_id.as_bytes()
|| operation_seq.to_be_bytes()
```

규범은 `job_id`/`attempt_id`의 별도 byte encoding을 명시하지 않는다. 두 identity가 protobuf
스키마에서 `string`이므로 Rust `str`의 **그대로인 UTF-8 bytes**를 사용한다. Unicode
normalization, trimming, case folding을 하지 않는다. 그런 변환은 규범에 없고 서로 다른 wire
identity를 조용히 같은 값으로 만들 수 있기 때문이다. 길이 prefix나 separator도 공식에 없으므로
추가하지 않는다.

> ★ **2026-08-24 독립 검수 반영.** 초안은 구분자 없는 연접을 "이 구현이 임의 변경할 수 없는
> 규범 한계"로 기록했으나, 이는 **규범 해석 누락이었다.** `proto/common.proto` 전역 규칙 5가
> "ID는 별도 명시가 없으면 ULID 26자 문자열"로 폭을 고정하므로, 계약을 지키는 입력에서는
> 연접이 모호하지 않다. 규범 결함이 아니라 구현이 임의 폭을 허용한 것이 문제였다.
> 따라서 공식을 바꾸는 대신 **해싱 전에 26자 폭을 검증**하고, 실제 충돌 케이스
> (`("ab","c")` 와 `("a","bc")`)를 회귀 테스트로 고정했다. 구분자·길이 prefix 는 여전히
> 추가하지 않는다 — 폭이 고정이면 필요 없고, 추가하면 규범 공식과 달라진다.

`operation_seq`는 정확히 8바이트 big-endian u64다. hash 재계산 외의 순서 판단에는 쓰지
않는다.

## 4. fail-closed 경계와 규범의 빈틈

다음 입력은 `Invalid`로 닫는다.

- 빈 `job_id` 또는 빈 `attempt_id`
- `operation_id: None`
- `HashAlgorithm`이 `BLAKE3_256`이 아님: `UNSPECIFIED`, `SHA256`, unknown value 포함
- digest value 길이가 정확히 32바이트가 아님
- 구조적으로 올바른 32바이트 digest라도 직접 재계산한 값과 다름

검증은 stale/dedup 판정보다 먼저 수행하므로 malformed 요청이 다른 결과에 숨지 않는다.

정하지 않거나 판정할 수 없어 발명하지 않은 것은 다음과 같다.

- `operation_seq` 단조 증가, gap, 회귀, 시작값 규칙
- 관측 키 TTL, 캐시 크기, 만료, 재시도 횟수
- 더 높은 epoch에서 같은 `operation_id`를 금지하는 규칙. 규범의 key가 tuple이므로 이는 새
  tuple로 수용한다.
- proto3의 non-optional scalar인 `fence_epoch`와 `operation_seq`가 wire에서 생략됐는지 여부.
  생성 타입에서는 생략과 명시적 0이 모두 `0`이므로 kernel이 구분할 수 없고, 규범도 0을
  금지하지 않는다. 따라서 0 자체를 누락으로 추측해 거부하지 않는다.
- caller가 제공한 이전 관측 slice가 durable하거나 완전한지 여부. 이 kernel에는 이를 입증할
  authority나 I/O가 없다.

## 5. 테스트와 뮤테이션 결과

통합 테스트 10건은 다음을 포함한다.

- positive: 최초 새 키, 같은 epoch의 다른 새 키, higher epoch의 같은 operation ID, 규범에
  없는 sequence 회귀 수용
- idempotency: exact tuple 재전송
- stale fence: 더 높은 관측 이후 낮은 epoch, 과거 exact tuple이어도 stale 우선
- derivation: UTF-8 exact concatenation과 비대칭 u64 값의 big-endian 확인
- invalid: 빈 identity, digest 누락, unspecified/SHA256/unknown algorithm, 0/31/33/64-byte
  digest, 재계산 불일치
- precedence: malformed digest가 stale 판정에 숨지 않음

production 분기 뮤테이션은 실제로 순차 적용하고 각각 즉시 원복했다.

1. 재계산 digest 대조 분기 제거
   - 결과: **실패**, 8 passed / 2 failed
   - 감지 테스트: `a_structurally_valid_but_incorrect_operation_id_is_rejected`,
     `invalid_input_is_not_hidden_by_a_stale_fence`
   - 원복 후: 10 passed / 0 failed
2. stale-fence 분기 제거
   - 결과: **실패**, 9 passed / 1 failed
   - 감지 테스트: `a_lower_fence_is_stale_even_when_its_pair_was_previously_observed`
   - 원복 후: 10 passed / 0 failed

## 6. 검증 기록

- `cargo build -p gputeer-protocol`: PASS
- `cargo test -p gputeer-protocol --test fenced_operation_kernel`: PASS, 10/10
- `cargo test -p gputeer-protocol`: PASS, unit/integration/doc-test 합계 119 passed / 0 failed

현재 shell의 `PATH`에는 cargo가 없어 실제 명령은
`C:\Users\playdata2\.cargo\bin\cargo.exe` 절대 경로로 실행했다. 명령의 package/test 범위는
위와 동일하다. `cargo fmt`는 실행하지 않았다.
