# scheduler 순수 deterministic resource best-fit kernel

## 목표

`docs/plans/2026-08-21_1253_scheduler_best_fit_v1.md`의 DoD-45 후보 범위대로
`crates/scheduler` 안에 I/O·시각·난수 의존이 없는 resource-tight ranking
kernel을 구현하고 malformed 입력을 fail-closed로 검증한다.

## 수행 내용

- `crates/scheduler/src/model.rs`: best-fit 정책·key·결과·오류 타입 추가
- `crates/scheduler/src/rank.rs`: `rank_best_fit()` 및 입력 검증/비교 구현
- `crates/scheduler/src/lib.rs`: module과 public API export
- `crates/scheduler/tests/best_fit.rs`: 결정성·동점·negative 테스트 14건
- `docs/plans/2026-08-21_1253_scheduler_best_fit_v1.md`: 구현 결과 기록

Coordinator 배선, staging, allocation/CAS, inventory revision, reservation,
Grant는 변경하지 않았다.

## 검증

```text
cargo build --workspace --exclude gputeer-runtime-windows
PASS (exit 0)

cargo test --workspace --exclude gputeer-runtime-windows
PASS (exit 0; 기존 ignored 1건 유지)

cargo test -p gputeer-scheduler
PASS (47 passed, 0 failed)
```

수동 뮤테이션은 CPU 비교 반전과 `node_id` tie-break 반전 2건 모두 해당 targeted
test가 실패해 검출했고, 원복 후 다시 통과했다. `git diff --check`는 공백 오류 없이
통과했다. toolchain에 `rustfmt` component가 없어 `cargo fmt`는 환경상 실행하지
못했다.

자체 재검토에서 stale report의 non-GPU 부족 subtraction 경로에 직접 negative
coverage가 없음을 발견해 CPU one-below 테스트를 추가했고, underflow 대신 명시적
`EligibleCandidateMismatch`가 반환됨을 확인했다.

물리 줄 수는 `rank.rs` 269줄과 `model.rs` 타입 추가 54줄로 계획의 production
추정 120~220줄을 넘었다. 후속 기능을 넣은 것이 아니라 malformed 입력과 overflow
fail-closed 검증이 예상보다 컸기 때문이다. 테스트 372줄은 추정 범위 안이다.

사용자가 `docs/evidence/` 수정을 금지했으므로 별도 DoD evidence는 만들지 않았고,
이 리포트와 계획서 구현 결과 절에 재현 명령과 결과를 기록했다.

## 미해결 이슈와 다음 작업

현재 report 타입에는 hard-filter에 사용한 snapshot/policy revision이 없어 동일
origin을 완전히 증명할 수 없다. 이번 kernel은 ID 집합과 ranking fact 정합성까지만
검사한다. 후속 조각은 계획대로 inventory version + allocation row + CAS reservation
계약을 먼저 만들고 나서 staging/Grant에 배선해야 한다.
