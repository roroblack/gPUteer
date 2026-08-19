# 2026-08-19_1725_lease_재발급_정책_superseded_v1

- **기준선:** `docs/plans/2026-08-19_2350_max_total_duration_seconds_갱신_차단_v1.md`,
  `docs/plans/2026-08-19_1517_lease_revoke_최소_조각_v1.md`,
  `docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md`
- **대상 단계:** v0.1
- **선행 게이트:** Coordinator 영속 Lease 저장소(`DoD-16`)와 같은 연결 반복
  갱신(`DoD-15`) 완료. `RenewLeaseResult` 서명·framed ingress·Agent의
  `SUPERSEDED` 분류가 이미 존재해야 한다.

## 왜 지금 이 조각인가

`RenewOutcome.RENEW_OUTCOME_SUPERSEDED`는 더 높은 `fence_epoch`의 Lease가
이미 존재해 현재 노드가 stale인 정상 failover 경합을 나타내도록 정의돼 있다.
그러나 Coordinator의 `renew_req.fence_epoch != expected_epoch` 검사는 낮은
epoch와 높은 epoch를 모두 raw error로 처리해 연결을 끊는다. Coordinator
영속 저장소가 있는 경우에는 저장된 epoch가 재시작을 넘는 권위 있는 사실이므로,
낮은 요청만 서명된 정책 결과로 바꾸고 정상적인 연결 왕복을 보존한다.

## 범위

### In

- `lease_store=Some`일 때 저장된 `expected_epoch`보다 낮은
  `RenewLeaseRequest.fence_epoch`를 `RENEW_OUTCOME_SUPERSEDED`로 판정한다.
- `lease: None`, 요청 nonce echo, Coordinator 서명을 포함한
  `RenewLeaseResult`를 같은 TCP 연결로 보내고 다음 왕복을 받을 수 있게 한다.
- `lease_store=None` 레거시 경로의 epoch 불일치는 기존 hard error/연결 종료를
  유지한다.
- 저장된 epoch와 같은 epoch는 현재 동작대로 `RENEWED`이고, Agent는 기존의
  서명 검증 후 `RENEW_REFUSED:SUPERSEDED` 분류를 계산된 결과에도 적용한다.
- selftest에 영속 저장소의 낮은 epoch 대조군(서명된 SUPERSEDED, Coordinator
  연결 유지)과 같은 epoch 대조군(RENEWED)을 추가한다.
- 핵심 분기(영속 저장소 조건 또는 낮은 epoch 결과 생성)를 임시 무력화하는
  mutation test를 실행해 새 negative test의 비공허성을 확인하고 원복한다.

### Out

- `RENEW_OUTCOME_QUARANTINED`의 실제 트리거. 현재 Coordinator에는 기기
  위험도/신뢰도 입력, 관측 이력, 다중 Agent 선택 계층이 없으므로 이번 조각에서
  임의의 위험 판정은 만들지 않는다. `docs/vision/TODO_VISION.md`에 도입
  트리거·근거·비용·폐기 조건을 등록한다.
- 요청 epoch가 저장된 값보다 높은 경우의 새 정책 또는 새 Lease 발급. 현재
  Coordinator는 epoch 불일치를 hard error로 처리하고 Agent는 갱신 결과의
  epoch 상승을 명시적으로 거부한다. 이 조각에서 이를 바꾸면 실제 epoch
  할당/재발급 계약과 failover 경합 범위가 함께 확장되므로 기존 fail-closed
  동작을 유지한다.
- 새 `lease_id` 발급, 재접속/failover 전달, 다중 Agent·다중 Coordinator HA,
  스케줄러, TLS, Lease revoke 정책 자체.

## 정책 결정

```text
lease_store = Some
  request_epoch < stored_epoch  -> signed SUPERSEDED, lease=None, keep connection
  request_epoch == stored_epoch -> existing signed RENEWED path
  request_epoch > stored_epoch  -> existing hard error (fail closed)

lease_store = None
  any request_epoch != config.fence_epoch -> existing hard error
```

낮은 epoch의 정상적인 정책 거부는 Agent가 서명 검증을 통과한 뒤
`RENEW_REFUSED:SUPERSEDED`로 분류한다. 이는 연결 장애나 서명 실패와 구별되며,
proto 주석의 “정상적인 failover 경합” 의미와 일치한다.

## 단계

| # | 단계 | 스트림 | 완료 기준 | 상태 |
|---|---|---|---|---|
| 1 | 계획·TODO_VISION에 범위와 보류 결정 기록 | 문서 | 현재 정책, QUARANTINED 보류 이유, epoch 상승 보류 이유가 추적 가능 | ✅ |
| 2 | Coordinator 영속 epoch 분기 및 signed SUPERSEDED 응답 | Coordinator | 낮은 epoch만 정상 응답, 같은 epoch 회귀 없음, 레거시 불일치 hard error 유지 | ✅ |
| 3 | Agent 계산 결과 처리 확인/필요 수정 | Agent | 계산된 SUPERSEDED가 override SUPERSEDED와 같은 서명된 정책 거부 경로를 통과 | ✅ (기존 코드 확인) |
| 4 | selftest 시나리오 추가 | CLI | 영속 낮은 epoch에서 Coordinator 성공·outcome=2·Agent 정책 거부, 같은 epoch에서 RENEWED | ✅ |
| 5 | 단위/전체 검증과 5회 연속 selftest | Coordinator · Agent · CLI | build/test 통과, selftest 하드 타임아웃 포함 5회 통과 | ✅ |
| 6 | mutation test 후 원복·재통과 | Coordinator · CLI | 핵심 분기 무력화 시 새 negative test 실패, 원복 후 지정 검증 재통과 | ✅ |

## 완료 기준 (DoD)

이 세션에서는 사용자 지시에 따라 `docs/evidence/DoD-NN_*.md`를 작성하지
않는다. 독립 검수 세션이 실행 로그와 함께 evidence를 기록해야 최종 DoD로
간주한다.

- [ ] 영속 저장소에서 낮은 요청 epoch가 연결을 끊지 않고 서명된
      `RENEW_OUTCOME_SUPERSEDED`/`lease=None`으로 도착한다.
- [ ] 저장된 epoch와 같은 요청 epoch가 서명된 `RENEWED`로 처리된다.
- [ ] `lease_store=None`의 기존 불일치 hard error와 높은 epoch의 기존
      fail-closed 동작이 회귀하지 않는다.
- [ ] Agent가 계산된 SUPERSEDED를 `RENEW_REFUSED:SUPERSEDED`로 분류한다.
- [ ] **negative test:** 영속 저장소 낮은 epoch 경합과 mutation test가
      정상 경로와 구별되는 실패를 만든다.
- [ ] 빌드·workspace 회귀·selftest 5회 연속·하드 타임아웃 결과를 최종
      보고에 그대로 기록한다.

## 기준선과 다른 점

기존 계획은 Coordinator의 실제 `SUPERSEDED`/`QUARANTINED` 판정을 Out으로
남겼다. 이번 조각은 그중 저장소가 이미 제공하는 가장 좁고 관측 가능한
판정(낮은 요청 epoch)을 영속 저장소 모드에 한정해 구현한다. `QUARANTINED`와
높은 epoch의 새 정책은 위 Out 사유대로 기존 범위를 보존한다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-19 17:25 | 최초 작성. 영속 낮은 epoch의 signed SUPERSEDED 정책과 두 대조군 정의 |
| 2026-08-19 17:25 | Coordinator/CLI 구현, 31개 selftest·workspace 회귀·mutation 검증 완료. 사용자 지시에 따라 DoD evidence는 독립 검수 세션으로 이관 |
| 2026-08-19 17:25 | 검수 지적 반영: Agent가 즉시 종료하는 SUPERSEDED·QUARANTINED·MAX_DURATION_EXCEEDED 후 Coordinator도 갱신 루프를 break하고, renew_rounds=2 첫 회차 SUPERSEDED selftest를 추가 |
