# 2026-08-21 10:02 — scheduler 순수 hard-filter kernel

## 목표

`docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md` 조각 1에 따라
`crates/scheduler`를 만들고 후보 적격/탈락 사유만 결정하는 순수 kernel을 구현한다.

## 수행 내용

- `crates/scheduler`: 독립 domain model, `evaluate_eligibility`, 결정적 report
- `Cargo.toml`/`Cargo.lock`: workspace 등록
- `docs/contracts/01_스트림_소유권.md` 및 ownership guard: Scheduler 스트림 등록
- `docs/contracts/proposals/2026-08-21_0955_scheduler_workspace_등록.md`: 공용 파일 변경 제안
- `crates/scheduler/tests/hard_filter.rs`: 33개 정상/경계/negative/결정성 테스트
- `docs/plans/2026-08-21_1002_scheduler_hard_filter_v1.md`: 구현 범위와 결과

Coordinator, Agent, CLI, proto, evidence, history 파일은 수정하지 않았다.

## 검증

- `cargo build --workspace --exclude gputeer-runtime-windows`: PASS
- `cargo test --workspace --exclude gputeer-runtime-windows`: PASS,
  388 passed / 0 failed / 1 ignored
- scheduler 단독 테스트: PASS, 33 passed / 0 failed
- VRAM `>=` → `>` 뮤테이션: 경계 테스트가 의도대로 FAIL, 원복 후 PASS
- `IsolationClass::Restricted` → `SecurityTier::S1` 뮤테이션: S2+Restricted
  회귀 테스트 3개가 의도대로 FAIL, 원복 후 PASS
- 빈 owner/submitter ID 검사 제거 뮤테이션: 회귀 테스트 2개가 의도대로 FAIL,
  원복 후 PASS
- `git diff --check`: PASS
- 외부 I/O/clock/random 관련 의존 문자열 검색: 없음

사용자 지시로 `docs/evidence/DoD-NN_*.md`는 만들지 않았다. 따라서 위 결과는
재현 로그 요약이지 DoD PASS evidence 주장이 아니다.

## 미해결 이슈 · 다음 작업

- `rustfmt` component가 없어 `cargo fmt --check`는 실행하지 못했다.
- CUDA/deadline/durability/checkpoint admission과 best-fit/reservation은 상위 로드맵
  조각 4 이후 범위다.
- live PoolSnapshot producer와 다중 Agent registry는 조각 3 범위다.
