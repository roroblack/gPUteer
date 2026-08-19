# 2026-08-20_0357_revoked_signed_outcome_v1

- 기준선: `docs/plans/2026-08-19_0110_coordinator_lease_revoke_영속화_v1.md`,
  `docs/plans/2026-08-19_1725_lease_재발급_정책_superseded_v1.md`,
  `docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md`
- 대상 단계: v0.1
- 범위: 초기 Grant가 아닌 같은 연결의 Lease 갱신 경로에서만 revoked 상태를
  signed `RenewLeaseResult`의 `RENEW_OUTCOME_REVOKED`로 전달한다.

## 목표와 정책

`CoordinatorLeaseStore`의 `LeaseStoreError::Revoked`와 기존 raw 오류를
갱신 경로에서만 `RenewLeaseResult{ outcome: 8, lease: None, detail }`로
변환한다. 결과는 기존 `SUPERSEDED`/`MAX_DURATION_EXCEEDED`와 같은
Coordinator 서명·framed 전송·로그 경로를 사용하고, 연결을 끊지 않는다.
초기 `issue_lease()`/`get_or_issue()`의 revoked 거부는 그대로 둔다.

Agent는 서명·nonce·coordinator 상관관계 검증 뒤 outcome 8을
`RENEW_REFUSED:REVOKED`로 즉시 종료한다. Coordinator의 다회차 갱신
루프도 2/3/6과 함께 8에서 `break`하여 상대가 다음 요청을 기다리는
교착을 막는다.

## 변경 대상과 예상 영향

- `proto/lease.proto`: 기존 enum 번호를 유지하고 값 8만 추가한다.
- `crates/coordinator/src/lib.rs`: override의 `store.get()` 분기와 정상
  `renew_existing_within_duration()` 분기에서 `Revoked`를 signed outcome으로
  변환한다. `lease_store.rs`의 오류 정의는 수정하지 않는다.
- `crates/agent/src/lib.rs`: outcome 8 즉시 거부를 추가한다.
- `crates/cli/src/coordinator_agent_selftest.rs`: revoke 후 같은 프로세스 쌍이
  추가 갱신을 시도하는 37번 시나리오를 추가하고, 하드 타임아웃으로 연결
  유지·signed outcome·교착 부재를 확인한다.
- `crates/protocol`/canonical: enum 값 추가가 field/canonical 표에 영향을
  주는지 `cargo build`와 schema/canonical 검사로 실측한다. 영향이 없으면
  생성 파일·fingerprint·참조 벡터는 수정하지 않는다.

## 검증 계획

- `python tools/canonical/reference_canonical.py --self-test`
- `python tools/canonical/check_schema.py`
- `cargo build --workspace --exclude gputeer-runtime-windows`
- `cargo test --workspace --exclude gputeer-runtime-windows`
- `coordinator-agent-selftest` 37개 시나리오를 회차별 하드 타임아웃으로
  5회 연속 실행
- 정상 revoked outcome 변환 또는 Coordinator의 outcome-8 break를 임시
  무력화하는 뮤테이션을 최소 1건 실행해 새 시나리오가 실패하는지 확인한
  뒤 원복하고 회귀 검증을 재실행한다.

## 제외

초기 Grant 발급 거부의 signed 응답화, `LeaseStoreError::Revoked` 자체의
변경, 새 Grant/reconnect 프로토콜, evidence 문서, `CLAUDE.md`,
`docs/history/HISTORY.md`, git commit/push는 포함하지 않는다.

## 구현 결과

- enum 값 8은 `proto/lease.proto`에만 추가됐다. `cargo build`와
  `check_schema.py`가 통과했고, `crates/protocol`의 schema fingerprint,
  canonical 참조 구현, 테스트 벡터에는 실제 영향이 없어 수정하지 않았다.
- Coordinator의 저장소 override 조회 경로와 정상 갱신 경로 모두
  `LeaseStoreError::Revoked`를 signed outcome 8로 변환한다. Agent는
  `RENEW_REFUSED:REVOKED`로 종료하고 Coordinator도 outcome 8에서 break한다.
- selftest 37은 ACK 뒤 저장소 revoke를 확정하고 같은 연결의 다음 갱신에서
  outcome 8을 받도록 구성했다. 초기 Grant 거부 시나리오 35와 분리했다.
- 최종 build/test/canonical/schema 검증 및 60초 회차 하드 타임아웃 5회가
  통과했다. outcome 8 break 제거 mutation은 시나리오 37에서 Coordinator
  교착/실패를 재현했고 원복했다.
