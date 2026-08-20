# 2026-08-20_1615 · Ambiguous Renew 복구 v1

- 기준선: `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`
- 선행 구현: `DoD-35`(bounded reconnect), `DoD-36`(Resume wire),
  `DoD-37`(Coordinator dispatcher), `DoD-38`(Agent Resume outcome 분류)
- 재정의한 로드맵 조각: 5번의 원래 durable request ledger 전체가 아니라
  **durable Lease 현재 상태를 이용한 Ambiguous Renew 가용성 복구**

## 설계 조사 결론

현재 결함은 정확히 한 번 처리 보장의 부재보다 가용성 결함이다.
Coordinator는 `renew_existing_within_duration()` 트랜잭션을 커밋한 뒤
`RenewLeaseResult`를 인코딩·전송한다. Agent가 요청을 보낸 뒤 결과를
받기 전에 연결이 끊기면 `AMBIGUOUS_RENEW`로 올바르게 분류되지만,
최상위 reconnect loop가 이를 즉시 fatal로 반환해 같은 프로세스가
복구할 기회를 잃었다.

요청 원장이 없어도 단일 Agent의 가용성은 복구할 수 있다. 새 연결에서
기존 Grant/ACK 경로를 처음부터 다시 수행하면 Coordinator의 기존
`get_or_issue()`가 같은 identity의 저장 Lease를 그대로 돌려준다. 따라서
Agent는 이전 nonce가 적용됐는지 추론하지 않고, 권위 있는 현재 Lease를
검증한 뒤 새 `connection_attempt`와 새 round 입력으로 만든 새 nonce의
heartbeat를 보낸다.

이 방식은 같은 요청의 정확한 idempotent replay가 아니다. 같은 nonce를
재전송하면 기존 `InMemoryReplayGuard`가 계속 Duplicate로 거부한다.
Coordinator 재시작 뒤 같은 nonce의 결과를 복원하는 durable request
ledger, 응답 바이트 캐시, Resume ledger는 범위 밖이다.

## 최종 구현 범위

### Agent

- `SessionError::AmbiguousRenew`를 durable 복구 게이트가 켜진 경우에만
  기존 bounded reconnect의 retryable 분기로 보낸다. attempt 수와 총
  시간, backoff+jitter, Lease safety deadline은 기존 `DoD-35` 정책을
  그대로 쓴다.
- 재접속 뒤 별도 Resume/조회 RPC를 만들지 않는다. 기존
  `run_one_connection()`의 Grant 수신 뒤, 서명된 durable 출처 비트를
  확인하고 nested Lease 검증 → ACK → Renew 순서를 그대로 다시 실행한다.
- `RenewLeaseRequest`의 `flush()` 실패도 `AMBIGUOUS_RENEW`로 분류한다.
- 로컬 `--recover-ambiguous-renew-from-durable-lease true`는 운영자 의도만
  나타내며 단독 권위가 아니다. Ambiguous Renew 뒤의 재접속 Grant가
  `lease_from_durable_store=true`를 서명해 전달해야만 복구를 계속한다.
  false 또는 필드가 없는 Grant는 ACK·checkpoint·새 Renew 전에
  `DURABLE_LEASE_RECOVERY_REFUSED`로 fatal 종료한다. 로컬 게이트 기본값은
  계속 false이며 이 경우에는 `AMBIGUOUS_RENEW_RECOVERY_DISABLED`다.

### Coordinator

- `ExecutionGrant`에 순수 추가 필드 25
  `lease_from_durable_store`를 넣고 Grant schema/domain을 v2로 올렸다.
  실제 `lease_store.is_some()`일 때만 true를 넣은 뒤 Grant 서명에
  포함한다. legacy opt-in 여부로 이 값을 추론하지 않는다.
- 정상 renew·Grant 발급·`get_or_issue()` 의미는 바꾸지 않는다.
- 테스트 전용
  `--drop-after-renew-commit-before-result-once true`를 추가한다.
  `build_renew_result()`가 RENEWED Lease를 반환한 직후, 즉 SQLite commit
  뒤이면서 result frame 인코딩·전송 전인 첫 연결에서만 종료한다.
  durable `--lease-db`와 RENEWED 결과가 아니면 hook 자체가 fail closed한다.
- expiry 회귀를 짧고 결정적으로 만들기 위한 테스트 전용
  `--renew-extension-ms`를 추가했다. 기본 60,000ms는 기존 동작과 같다.

## 통합 검증 시나리오

`coordinator-agent-selftest` 65~71:

1. commit/result 사이 drop 뒤 동일 Agent 프로세스가 재접속하고, 두 번째
   Grant의 expiry가 첫 commit 로그와 정확히 같으며, 새 nonce Renew가
   성공한다.
2. 첫 commit 직후 durable revoke를 기록하면 두 번째 `get_or_issue()`가
   revoked로 거부한다.
3. 300ms 연장 뒤 next accept를 600ms 늦추면 두 번째 `get_or_issue()`가
   expiry `<=` 경계로 거부한다.
4. 최초 `issued_at`부터 1초가 지난 뒤의 새 heartbeat가 signed
   `MAX_DURATION_EXCEEDED`로 끝난다.
5. 재접속 뒤 이전 Renew nonce를 강제로 재사용하면 replay guard가
   Duplicate로 거부한다.
6. unsafe legacy 모드에서 Agent가 request flush 뒤 스스로 연결을 끊어
   동일한 ambiguous 구간을 만들지만 durable 복구 게이트가 꺼져 두 번째
   connection attempt를 만들지 않는다.
7. commit/result drop 한 번으로 Coordinator의 `max-connections=1`이
   소진되면 Agent가 connect refusal을 bounded retry한 뒤
   `ReconnectExhausted`로 종료한다.

모든 프로세스 하네스는 기존 90/120초 hard deadline과 stdout/stderr
동시 drain 순서를 유지한다. Coordinator가 다음 연결을 기다리는데 Agent가
오지 않는 방향과, Coordinator가 이미 종료돼 Agent 연결만 거부되는 반대
방향 모두 무한 대기 없이 끝나야 한다.

## 범위 밖

- `lease_requests` 테이블, request digest, signed response bytes 캐시
- 같은 nonce 요청에 이전 응답 재전송
- Coordinator 재시작/HA를 넘는 ambiguous renew 자동 복구
- Resume 요청 ledger 및 FRESH/RESUME 자동 전환
- 다중 Agent 경쟁, ProgressReport durable side effect

## 안전성 불변식

- durable 복구 gate가 없으면 Ambiguous Renew는 기존처럼 fatal이다.
- gate가 있어도 기존 bounded retry budget을 소진하면 fatal
  `ReconnectExhausted`다.
- 두 번째 연결은 기존 Grant/ACK 검증을 생략하지 않는다.
- revoke·expiry는 `get_or_issue()`에서, max-duration은 저장된 최초
  `issued_at`을 사용하는 `renew_existing_within_duration()`에서 다시
  판정한다.
- 새 heartbeat는 새 connection attempt nonce를 쓰며, 같은 nonce replay는
  계속 거부된다.

## 구현 후 검증

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS
  (실패 0, 기존 ignored 1)
- `coordinator-agent-selftest`: 71개 시나리오를 5회 연속 PASS
  (각 120초 외부 hard timeout, 실측 39.8~41.2초, timeout 0)
- 뮤테이션: Agent의 `AmbiguousRenew` 분기를 기존 fatal 반환으로 잠시
  되돌리자 두 번째 `LEASE_ACCEPTED connection_attempt=1`이 사라져 신규
  시나리오 65가 exit 1로 실패했다. 원복 뒤 71/71 PASS를 다시 확인했다.

## 2026-08-20 독립 검수 후속 수정

독립 검수에서 로컬 복구 게이트와 Coordinator의 실제 `--lease-db`가
결합되지 않았음을 확인했다. 옵션 A의 시각 비교는 채택하지 않았다.
정상 durable 갱신 커밋도 `expires_at`을 바꾸므로 두 시각의 동일성은
요구할 수 없고, `issued_at`만 비교하면 같은 millisecond의 legacy 재발급
또는 시계 rollback을 안전하게 배제할 수 없기 때문이다.

따라서 옵션 B를 적용했다. `ExecutionGrant`의 새 bool 필드는 field 25로
추가했고 기존 번호는 바꾸지 않았다. canonical field 집합, schema version
2, `gputeer/v2/grant` domain tag, 독립 Python reference/vector,
schema fingerprint를 함께 갱신한다. 구버전/필드 누락은 protobuf 기본값
false가 되어 복구에서 fail closed한다.

회귀 시나리오 72는 `--lease-db` 없는 legacy Coordinator와
`--recover-ambiguous-renew-from-durable-lease true` Agent를 의도적으로
조합한다. 첫 Renew 전송 직후 Agent 연결을 끊어 ambiguity를 만들고,
재접속 Grant의 signed durable=false를 받은 Agent가 두 번째 ACK,
`LEASE_ACCEPTED`, 새 Renew 없이 fatal 종료하는지 검증한다.

후속 검증 결과:

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS
  (실패 0, 기존 ignored 1)
- `coordinator-agent-selftest`: 외부 120초 hard timeout을 건 72개
  시나리오를 5회 연속 PASS (39.015, 39.641, 39.219, 41.297,
  39.890초, timeout 0)
- 뮤테이션: Agent의 signed durable 비트 검사 조건을 임시로 false로
  만들자 시나리오 72가 exit 1로 실패했다. 두 번째 legacy Grant가 새
  `issued_at`/`expires_at`으로 수락되고 새 Renew까지 성공하는 원래 결함이
  실제 재현됐다. 조건 원복·재빌드 뒤 72/72 PASS를 다시 확인했다.
