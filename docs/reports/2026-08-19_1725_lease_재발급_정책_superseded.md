# 2026-08-19 17:25 — Lease 재발급 정책 SUPERSEDED 구현 리포트

- **계획:** `docs/plans/2026-08-19_1725_lease_재발급_정책_superseded_v1.md`
- **스트림:** Coordinator · Agent · CLI(selftest)
- **커밋:** 작업 중인 변경이며 커밋하지 않음

## 1. 목표

영속 `CoordinatorLeaseStore`가 저장한 epoch보다 낮은 갱신 요청을 연결 종료가
아닌 서명된 `RENEW_OUTCOME_SUPERSEDED`/`lease=None` 결과로 응답한다. 같은
epoch의 기존 `RENEWED`, 레거시 `lease_store=None` hard error, 높은 epoch
fail-closed 동작은 유지한다.

## 2. 수행 내용

변경 파일:

```text
crates/coordinator/src/lib.rs
crates/cli/src/coordinator_agent_selftest.rs
docs/plans/2026-08-19_1725_lease_재발급_정책_superseded_v1.md
docs/vision/TODO_VISION.md
docs/reports/2026-08-19_1725_lease_재발급_정책_superseded.md
docs/history/HISTORY.md
```

- 영속 저장소 모드에서만 lower-than-stored epoch를 signed SUPERSEDED로
  응답하고 같은 연결의 Coordinator 루프를 유지하도록 했다.
- Agent의 기존 outcome=2 처리(결과 서명 검증 후
  `RENEW_REFUSED:SUPERSEDED`)가 계산 결과에도 그대로 적용됨을 확인했다.
- selftest를 29개에서 31개로 확장했다. 새 25번은 저장 epoch 5에 요청 4,
  새 26번은 저장 epoch와 같은 기본 renew를 검증한다.
- `QUARANTINED` 실제 트리거는 위험도/신뢰도 인프라 부재로 구현하지 않고
  TODO_VISION V-11에 등록했다. 높은 epoch는 기존 Coordinator 대조 오류와
  Agent의 명시적 epoch 상승 거부를 유지하고 재발급 정책 단계로 미뤘다.

## 3. 검증

실행 명령과 결과:

```text
cargo build --workspace --exclude gputeer-runtime-windows
Finished `dev` profile; exit 0

cargo test --workspace --exclude gputeer-runtime-windows
전체 테스트 스위트 exit 0; ignored 1건 유지

cargo run -p gputeer-cli -- coordinator-agent-selftest
31개 시나리오 exit 0
```

`coordinator-agent-selftest`는 도구 실행 제한 120초 안에서 5회 연속 실행했고
각 회차 31개 시나리오가 모두 통과했다. 새 시나리오 25는 Coordinator stdout의
`RENEW_RESULT ... outcome=2` 및 `RESULT ok=true`와 Agent의
`RENEW_REFUSED:SUPERSEDED`를 확인했다. 시나리오 26은 양쪽 `RENEWED`를
확인했다.

Mutation 검증:

- lower-epoch 분기를 `if false`로 임시 무력화 → 시나리오 25가 Coordinator의
  raw `fence_epoch 불일치`/Agent의 truncated frame으로 실패. 원복했다.
- SUPERSEDED outcome `2`를 `1`로 임시 변경 → 시나리오 25가 Agent의
  `outcome=RENEWED 인데 Lease가 없다`로 실패. 원복했다.
- 두 mutation 모두 원복 후 build와 5회 selftest를 재실행했다.

`cargo fmt`는 설치된 stable toolchain에 `rustfmt` 컴포넌트가 없어 실행하지
못했다(`cargo-fmt.exe` 미설치). 외부 crate 추가·`CLAUDE.md` 수정·커밋·푸시는
하지 않았다.

사용자 지시에 따라 `docs/evidence/DoD-NN_*.md`는 생성하지 않았다. 독립 검수
세션이 이 리포트의 명령과 raw 실행 결과를 바탕으로 evidence를 기록해야 한다.

## 4. 미해결 · 다음 작업

- `QUARANTINED` 실제 판정, 새 `lease_id` 재발급, 높은 epoch의 운영 정책,
  failover 재접속·다중 Agent는 범위 밖이다.
- rustfmt 컴포넌트 설치 없이는 자동 포맷 검사를 재현할 수 없다.
- 독립 검수 및 DoD evidence 기록이 남아 있다.

## 5. 검수 기록

이 세션은 구현자 검증만 수행했다. mutation test는 새 negative test가
무력화에 실제로 실패하는지 확인하는 용도로 수행했으며, 최종 소스는 원복했다.
