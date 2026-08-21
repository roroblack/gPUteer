# 2026-08-21 15:14 — scheduler local placement-to-staging orchestration

## 목표

`docs/plans/2026-08-21_1502_scheduler_grant_dispatch_v1.md`의 오늘 최소 조각인
crate-internal local placement-to-staging orchestration kernel을 구현한다.

## 수행

- `crates/coordinator/src/orchestrate.rs`: caller-supplied issuance input, typed
  0/1/N outcome와 typed subsystem error, 한 번의 snapshot에서 filter/rank한 뒤
  한 번 stage하는 조합 함수 및 실제 파일 DB 테스트 7건 추가.
- `crates/coordinator/src/lib.rs`: private module만 등록. accept-loop/wire 경로는 미변경.
- 계획 문서에 구현 결과, 7개 계약 불일치 처리, 검증과 제한 추가.

## 검증

- `cargo build --workspace --exclude gputeer-runtime-windows`: 성공.
- `cargo test --workspace --exclude gputeer-runtime-windows`: 449 passed,
  0 failed, 1 ignored.
- 단일 후보 ranking 호출 뮤테이션: 대응 테스트 실패 확인 후 원복.
- 복수 후보 first-eligible 선택 뮤테이션: 대응 테스트 실패 확인 후 원복.
- `cargo fmt -p gputeer-coordinator`: stable toolchain에 rustfmt component가 없어
  실행 불가(`cargo-fmt.exe is not installed`).

사용자 지시에 따라 `docs/evidence/`, `docs/history/HISTORY.md`, `CLAUDE.md`는
수정하지 않았다.

## 미해결 이슈와 다음 작업

- inventory revision + allocation/resource CAS가 없으므로 이 API는 reservation이
  아니며, 서로 다른 Job의 순차 호출도 unchanged inventory에서 같은 GPU를 고를 수 있다.
- 원본 Manifest/`JobRequirements` adapter, selected GPU UUID, Grant plan/rationale,
  node/device/session routing, durable outbox, ACK 실패 rollback/requeue는 계획대로 범위 밖이다.
- operation retry 전에 inventory가 바뀐 경우 orchestration-level replay를 선행 조회하는
  API가 없다. 현재 검증은 동일 입력·unchanged inventory의 exact-request replay 범위다.
