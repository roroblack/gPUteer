# 변경 제안 · scheduler workspace 등록

- 제안자 스트림: Coordinator / Scheduler
- 대상 파일: `Cargo.toml`, `Cargo.lock`, `docs/contracts/01_스트림_소유권.md`,
  `crates/protocol/tests/stream_ownership.rs`
- 유형: workspace | cross-stream

## 무엇을 바꾸려는가

마스터 플랜 §30과 scheduler 전체 로드맵 조각 1에 이미 정의된
`crates/scheduler` 순수 hard-filter 크레이트를 workspace member로 등록한다.
새 크레이트가 소유권 검사 밖에 놓이지 않도록 Scheduler 소유 영역과
`every_crate_is_covered_by_ownership_rules`의 알려진 크레이트 목록도 함께 갱신한다.

## 왜 필요한가 (이것 없이 막히는 것)

사용자가 조각 1 구현과 workspace 등록을 명시적으로 요청했다. workspace 등록이 없으면
요청된 workspace 빌드·테스트가 새 크레이트를 검증하지 않으며, 소유권 목록 갱신이 없으면
기존 경계 테스트가 의도대로 실패한다.

## 영향받는 스트림

- Scheduler: 새 크레이트의 구현과 단위 테스트 소유
- Protocol/QA guard: 새 크레이트가 소유권 경계 안에 있음을 검사
- 통합: workspace member 및 lockfile 갱신

## 호환성 영향

- schema_version 증가 필요? 아니오
- 기존 서명이 무효화되는가? 아니오
- 멀티 바이너리 스큐 창에 영향? 아니오
- 테스트 벡터 재생성 필요? 아니오

## 되돌리는 방법

새 크레이트 디렉터리와 workspace member를 제거하고, 소유권 표 및 KNOWN 목록에서
`scheduler` 항목을 제거한 뒤 Cargo.lock을 재생성한다.

## 검토 (영향받는 스트림이 채운다)

로드맵의 조각 1 In/Out과 마스터 플랜 §13.8·§30을 대조했다. proto 및
`docs/protocol/` 변경은 필요하지 않으며 scheduler는 protocol 크레이트에 의존하지 않는
독립 domain model을 사용한다.

## 승인 (통합 책임자)

2026-08-21 사용자 요청에서 `crates/scheduler` 신규 생성과 workspace 등록을 명시적으로
승인했다. git commit/push는 승인 범위에서 제외된다.
