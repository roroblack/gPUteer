# 2026-08-20_0357 — revoked Lease signed renew outcome

- 계획: `docs/plans/2026-08-20_0357_revoked_signed_outcome_v1.md`
- 스트림: Protocol · Coordinator · Agent · CLI

## 수행

- `proto/lease.proto`의 `RenewOutcome`에 기존 번호를 유지한 채
  `RENEW_OUTCOME_REVOKED = 8`만 추가했다.
- Coordinator의 갱신 override 조회 경로와 정상 저장소 갱신 경로에서
  `LeaseStoreError::Revoked`를 `lease=None` signed `RenewLeaseResult`
  로 변환했다. `lease_store.rs`의 오류 타입과 초기 Grant 발급 거부는
  변경하지 않았다.
- Agent에 `RENEW_REFUSED:REVOKED` 분기를 추가하고 Coordinator의 다회차
  outcome break 목록에 8을 추가했다.
- selftest 37을 추가했다. ACK 뒤 같은 프로세스 쌍에서 저장소 revoke를
  확정하고 다음 renew 요청이 signed outcome 8을 받아 즉시 종료되는지
  확인한다. 초기 Grant 거부 시나리오 35와 분리했다.
- enum 추가는 field/canonical 계약에 영향을 주지 않아 생성 파일,
  `proto/SCHEMA_FINGERPRINT.txt`, Python 참조 구현, canonical 벡터는
  수정하지 않았다.

## 검증

- `python tools/canonical/reference_canonical.py --self-test` — PASS
- `python tools/canonical/check_schema.py` — 오류 0건, schema 검사 통과
- `cargo build --workspace --exclude gputeer-runtime-windows` — PASS
- `cargo test --workspace --exclude gputeer-runtime-windows` — PASS
  (최종 원복 후 재실행도 PASS, ignored 1건은 기존 테스트)
- `target/debug/gputeer.exe coordinator-agent-selftest` — 37/37 PASS
- selftest 5회 연속 — 각 회차 60초 하드 타임아웃, 모두 `exit=0`
- mutation: Coordinator의 `matches!(..., 2 | 3 | 6 | 8)`에서 `| 8`을
  임시 제거했다. 37번이 outcome 8을 보낸 뒤 Coordinator가 다음
  요청을 기다려 실패했으며(실측 exit 1), 원복 후 build/test/selftest를
  다시 통과시켰다.
- `cargo fmt --check`는 로컬 toolchain에 `cargo-fmt`가 설치되지 않아
  실행 불가했다. 대신 `git diff --check`는 통과했다.

## 미해결/제한

- selftest 37은 정상적인 Agent revoke-notice 수신 후 renew 차단을
  재현하는 시나리오 27과 분리해, 저장소 revoke를 ACK 직후 직접 확정한
  뒤 renew outcome을 시험한다. 따라서 한 시나리오 안에서 revoke notice
  wire frame과 signed revoked renew outcome을 동시에 검증하지는 않는다.
- 작업 중 다른 세션에서 생성된 것으로 보이는 `docs/plans/2026-08-20_0300_*`,
  `docs/plans/2026-08-20_0310_*` 미추적 파일은 건드리지 않았다.
