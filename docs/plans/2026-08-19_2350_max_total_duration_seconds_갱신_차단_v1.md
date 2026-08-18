# 2026-08-19_2350_max_total_duration_seconds_갱신_차단_v1

- **기준선:** `docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md`
  (Coordinator 영속 Lease 저장소 완료, `DoD-16`)의 "Out" 절이 명시한
  다음 항목 중 하나. 코덱스 감사(`p110`)가 난이도 "중간"으로 재평가·
  추천.
- **대상 단계:** v0.1
- **선행 게이트:** `DoD-16`(Coordinator 영속 Lease 저장소) — 이 조각은
  저장소가 이미 영속 추적하는 `issued_at_unix_ms` 를 판정 근거로
  쓴다. `lease_store=None`(레거시) 경로는 이 조각에서 다루지 않는다.
- **상태:** 설계 완료(코덱스 `p111`). 구현 착수.

★ 이 문서는 코덱스(`agent:codex-cli`, read-only 샌드박스)의 설계
응답(`p111` 프롬프트)을 정리한 것이다.

## 왜 지금 이 조각인가

`Lease.max_total_duration_seconds`(`proto/lease.proto:61`, 기본
24시간)와 `RenewOutcome.RENEW_OUTCOME_MAX_DURATION_EXCEEDED = 6`
(`proto/lease.proto:132`)은 이미 스키마에 존재하지만, **어디서도
실제로 판정되지 않는다.** `max_total_duration_seconds` 는 저장·
전달만 될 뿐이고, outcome=6 은 현재 `--renew-outcome-override`
(테스트 전용 강제 주입)로만 나올 수 있다. `DoD-16` 이 완료돼
`CoordinatorLeaseStore` 가 최초 발급 시각(`issued_at_unix_ms`)을
재시작을 넘어 정확히 영속 추적하므로, 이제 실제 누적 시간 판정을
구현할 기반이 갖춰졌다.

## 범위

### In

- Coordinator 갱신 경로(`lease_store=Some` 인 경우에 한정)에서
  `now - stored.issued_at_unix_ms > max_total_duration_seconds * 1000`
  판정.
- 초과 시 저장소의 `expires_at_unix_ms`/`renew_after_unix_ms` 를
  **연장하지 않고**, 서명된 `RenewLeaseResult{ outcome:
  RENEW_OUTCOME_MAX_DURATION_EXCEEDED, lease: None }` 반환.
- 판정과 조건부 UPDATE 를 **하나의 SQLite 트랜잭션**으로 묶어
  get→판정→UPDATE 사이 TOCTOU 를 없앤다(`CoordinatorLeaseStore`
  에 `renew_existing_within_duration()` 신설).
- clock rollback(`now < issued_at_unix_ms`) 은 fail closed —
  경과시간을 0으로 취급해 갱신을 계속 허용하지 않고, 초과로 취급해
  거부한다.
- Agent 쪽 `RenewOutcome` 매치에 outcome=6 명시 분기 추가 —
  `RENEW_REFUSED:MAX_DURATION_EXCEEDED` 로 명시 거부(기존
  outcome=2/3 과 같은 패턴).
- `--max-total-duration-seconds` CLI 플래그(Coordinator) — 최초
  발급 시에만 후보값으로 쓰이고, 이미 저장소에 있는 Lease 의 값은
  저장소가 권위(기존 `get_or_issue()` 계약과 동일한 원칙).
- selftest 시나리오 추가 — 짧은(`--max-total-duration-seconds 2`)
  한도를 실제로 초과시켜 outcome=6 이 나오는지, 그리고 한도 안에서는
  여전히 `RENEWED` 가 나오는지 실측.

### Out (명시하지 않으면 범위가 샌다)

- 새 `lease_id` 발급, 초과 후 자동 재시도/전환 — `MAX_DURATION_EXCEEDED`
  수신 후 새 Lease 요청·재시도는 이 조각의 범위 밖이다.
- `lease_store=None`(레거시) 경로 — 매 갱신 요청마다
  `issued_at_unix_ms: now` 를 즉석에서 재구성하므로 실제 경과시간을
  의미 있게 추적하지 못한다. 이 경로는 계속 `RENEWED` 만 반환한다
  (legacy enforcement bypass 로 명시 문서화, 정책 회피가 아니라
  애초에 판정 근거가 없다는 사실의 인정).
- `SUPERSEDED`/`QUARANTINED`·revoke·`RevokeLeaseNotice` 발행·
  job cancel/completion/quorum unavailable 정책.
- scheduler·watchdog·다중 Agent·다중 Coordinator HA·TLS.
- Lease 이력/audit event store.

## 판정 위치와 비교식

```text
max_duration_ms = u64::from(max_total_duration_seconds) * 1_000

exceeded =
  match now_unix_ms.checked_sub(stored.issued_at_unix_ms) {
      Some(elapsed_ms) => elapsed_ms > max_duration_ms,
      None => true,   // now < issued_at — clock rollback, fail closed
  }
```

경계는 `>` — 정확히 한도와 같은 순간은 아직 허용, 1ms 라도 초과하면
거부.

`CoordinatorLeaseStore::renew_existing_within_duration()` 이
get→판정→조건부 UPDATE 를 `TransactionBehavior::Immediate` 트랜잭션
하나로 묶는다. 초과 시에는 UPDATE 를 아예 실행하지 않고
(`expires_at_unix_ms`/`renew_after_unix_ms` 보존) 저장된 값 그대로
반환한다 — 연장해버리면 다음 판정 시각이 밀려 정책이 무력화된다.

## `renew_outcome_override` 와의 상호작용

기존 `--renew-outcome-override` 는 테스트 전용 강제 주입이다.
이 조각은 **실제 만료 판정이 override 보다 우선**하도록 설계한다 —
`lease_store=Some` 이고 실제로 초과했으면, override 값과 무관하게
outcome=6 을 반환한다. override 는 (a) `lease_store=None` 이거나
(b) 초과하지 않은 경우에만 적용된다. 이렇게 해야 테스트용 override
가 실제 만료 정책 검증을 가릴 수 없다.

## selftest 시나리오 설계

시나리오 22 (신설): 짧은 `--max-total-duration-seconds 2` 로 최초
발급 후, selftest 프로세스가 2.2~2.5 초 sleep 한 뒤 **별도 프로세스**
로 같은 `--lease-db` 를 열어 갱신을 시도한다.

기대 결과:
- Coordinator: 서명된 `outcome=6` 전송, `expires_at_unix_ms` 저장소
  값 불변(SQLite 재조회로 확인).
- Agent: `RENEW_REFUSED:MAX_DURATION_EXCEEDED` 로 실패 종료, 최종
  `RESULT ok=true` 없음.

시나리오 23 (신설, 대조군): `--max-total-duration-seconds` 를 충분히
크게(예: 3600) 주고 즉시 갱신 — 여전히 `RENEWED` 가 나오는지 확인해
이 정책이 정상 갱신을 오탐하지 않음을 증명한다.

`--renew-rounds` 를 단순히 늘리는 것만으로는 판정할 수 없다 — 현재
반복 요청 사이에 sleep 이 없어 모든 라운드가 한도 안에서 끝날 수
있다. 따라서 이번 시나리오는 두 프로세스 + 실제 sleep 방식을 쓴다.

## 단계

| # | 단계 | 완료 기준 | 상태 |
|---|---|---|---|
| 1 | `CoordinatorLeaseStore::renew_existing_within_duration()` 신설(`RenewDecision` enum) | 단위 테스트(초과/미초과/clock rollback) | ⬜ |
| 2 | `build_renew_result` 를 새 메서드로 전환, outcome=6 분기 추가 | override 우선순위 포함 | ⬜ |
| 3 | Coordinator `--max-total-duration-seconds` CLI 플래그 | 최초 발급에만 반영, 기존 Lease 는 저장값 우선 | ⬜ |
| 4 | Agent `RenewOutcome` 매치에 outcome=6 분기 | `RENEW_REFUSED:MAX_DURATION_EXCEEDED` | ⬜ |
| 5 | selftest 시나리오 22·23 추가 | 5회 연속 통과 | ⬜ |
| 6 | 뮤테이션 테스트 + 코덱스 독립 검수 + evidence 기록(DoD-18) | `ACCEPTED` | ⬜ |

## 완료 기준 (DoD)

- [ ] 실제로 시간을 초과시켜 서명된 `MAX_DURATION_EXCEEDED` 를
      받는 selftest 시나리오(negative test).
- [ ] 한도 안에서는 여전히 `RENEWED` 가 나오는 대조군 시나리오.
- [ ] 초과 시 저장소 `expires_at_unix_ms` 가 연장되지 않았음을
      직접 재조회로 확인.
- [ ] clock rollback 을 fail closed 로 처리하는 단위 테스트.
- [ ] 뮤테이션 테스트로 판정 로직의 비공허성 확인.
- [ ] 코덱스 독립 검수 `ACCEPTED`.

## 기준선과 다른 점

없음 — `docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md`
와 `docs/plans/2026-08-19_2330_같은_연결_반복_lease_갱신_v1.md` 둘 다
이미 예고한 다음 후보를 그대로 이행한다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-19 23:50 | 코덱스 설계(`p111`) 정리 — 구현 착수 |
