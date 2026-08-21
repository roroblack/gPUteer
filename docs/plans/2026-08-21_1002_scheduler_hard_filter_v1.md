# 2026-08-21_1002_scheduler_hard_filter_v1

- 기준선: `../gputeer_master_plan_FINAL.md` §8.4, §9.1~§9.3, §10.1,
  §11.5, §13.2, §13.8, §27.1, §28, §30, §31, §33.2
- 상위 로드맵: `docs/plans/2026-08-21_0949_scheduler_전체_설계_v1.md` 조각 1
- 상태: 구현 및 요청 검증 완료
- 스트림: Scheduler

## 목표

외부 상태를 읽지 않는 `crates/scheduler` hard-filter kernel을 만든다. 입력으로
고정 `PoolSnapshot`, `JobRequirements`, `Policy`를 받고 후보별 적격 여부와 모든
확인된 탈락 사유를 반환한다. 적격 후보가 여러 개면 winner를 선택하지 않고
`RankingRequired`를 반환한다.

## 구현 범위

### 입력 domain model

- `PoolSnapshot`: 평가 기준 시각과 후보 목록
- `CandidateSnapshot`: node/risk/freshness, tier/isolation/key, GPU 목록,
  CPU/RAM/workspace, owner 및 workload 허용 정책
- `GpuSnapshot`: health, available VRAM, model
- `JobRequirements`: submitter/workload/side-effect/sensitivity, 보안 최소값,
  GPU count/VRAM/model, CPU/RAM/workspace
- `Policy`: snapshot 최대 나이

proto의 `UNSPECIFIED`/기본값을 domain의 “제약 없음”과 “관측 없음”으로 동시에
쓰지 않는다. 확인이 필요한 값은 `Option`으로 표현하고 `None`이면
`MissingFact`로 fail-closed한다. GPU 모델 허용 목록의 빈 벡터만 proto
`GpuRequest.allowed_gpu_models` 계약대로 명시적인 “모델 제약 없음”이다.

### 출력

- `EligibilityReport { eligible, rejected, resolution }`
- `RejectedCandidate { node_id, reasons }`
- `EligibilityResolution::{NoEligibleCandidates, SingleEligible, RankingRequired}`

후보와 탈락 사유를 정렬하고 중복 사유를 제거한다. 동일 `node_id`가 중복된
비정상 입력에서도 전체 `RejectedCandidate`를 다시 정렬해 입력 순서가 결과를
바꾸지 못하게 한다.

## hard gate와 근거

| gate | 판정 | 근거 |
|---|---|---|
| Node 상태 | `ONLINE`만 허용 | 마스터 플랜 §27.1, 로드맵 조각 1 |
| Risk | `NORMAL`만 허용 | §8.4, §13.2 |
| Freshness | 미래 시각 또는 정책 최대 나이 초과 거부 | §14.5, 로드맵 조각 1 |
| Security | tier/isolation/key protection 각각 최소값 이상 | §8.4, §9.1, §13.2 |
| GPU | count, health, GPU별 VRAM, 허용 model 충족 | §10.1, §13.2, proto `GpuRequest` |
| CPU/RAM/workspace | 각 가용량이 요구량 이상 | §10.1, §13.2 |
| workload allowlist | Job class가 owner 허용 목록에 포함 | §11.5, §13.2 |
| S0 제3자 | 타인 Job 무조건 거부 | §9.3 |
| `RESTRICTED` 제3자 | 장치 opt-in + `pure` + `SENSITIVE` 아님 | §9.2, §13.2 |
| unknown | 필요한 사실이 없으면 `MissingFact` | 로드맵 조각 1, `CLAUDE.md` §1 |

§8.4에 따라 tier/isolation/key/risk를 하나의 신뢰 점수로 합치지 않는다.
§13.8과 §31에 따라 kernel은 순수 함수이고 고정 snapshot으로 테스트한다.

## 명시적 비범위

- CUDA/architecture compatibility
- availability window, `T_est`, deadline, chance-constrained 확률
- durability/failure-domain, checkpoint budget
- reliability/locality/fairness/load 점수, best-fit, exploration
- Coordinator/Agent/CLI/proto 연결, SQLite/ControlStore
- reservation, Lease/Grant, entrypoint 실행, 실제 GPU 실측

이는 상위 로드맵 조각 1의 Out을 그대로 따른다.

## negative test

`crates/scheduler/tests/hard_filter.rs`에 다음을 둔다.

1. `ONLINE`이 아닌 Node 및 `NORMAL`이 아닌 Risk 거부
2. freshness 한계보다 1ms 오래된 snapshot 및 미래 시각 거부
3. tier/isolation/key protection 바로 아래 경계 거부
4. GPU count 부족, unhealthy GPU, model 불일치, VRAM 1 byte 부족 거부
5. CPU 1 core, RAM 1 byte, workspace 1 byte 부족 거부
6. owner workload allowlist 불일치 거부
7. S0 타인 Job 거부와 owner Job 통과
8. `RESTRICTED` 타인 Job의 opt-in 없음, non-pure, sensitive data 각각 거부
9. candidate/job/GPU telemetry unknown 및 빈 owner/submitter ID의 `MissingFact` fail-closed
10. 0/1/복수 적격 구분 및 복수 적격 `RankingRequired`
11. 후보 역순·동일 node ID 중복에도 동일 report/reason 순서
12. 모든 최소 경계가 정확히 같을 때 정상 통과

## 뮤테이션 테스트

VRAM 적격 비교를 임시로 `available >= required`에서 `available > required`로
변경했다. `exact_boundaries_pass`가 `GpuVramInsufficient`로 실패(exit 1)해
경계 assertion의 판별력을 확인했다. 원복 후 같은 테스트는 통과했다.

제3자 제한 분기를 임시로 `IsolationClass::Restricted`에서
`SecurityTier::S1`로 되돌리자 S2+Restricted 회귀 테스트 3개가 모두 실패했다.
빈 owner/submitter ID 검사를 임시로 제거하자 해당 회귀 테스트 2개가 모두
실패했다. 두 뮤테이션 모두 원복 후 scheduler 전체 테스트가 통과했다.

## 검증 결과

```text
cargo build --workspace --exclude gputeer-runtime-windows
  PASS — exit 0, dev profile 완료

cargo test --workspace --exclude gputeer-runtime-windows
  PASS — exit 0, 388 passed / 0 failed / 1 ignored

cargo test -p gputeer-scheduler
  PASS — 33 passed / 0 failed

git diff --check
  PASS — whitespace 오류 없음 (기존 Windows CRLF 전환 경고만 출력)
```

`cargo`가 현재 PowerShell `PATH`에 없어 최초 명령은 시작되지 않았다. 이후
설치된 `C:\Users\playdata2\.cargo\bin\cargo.exe` 절대 경로로 같은 명령을
실행했다. `rustfmt` component는 설치되어 있지 않아 `cargo fmt --check`는
실행 불가였고, 이는 위 필수 검증 두 명령의 성공과 별개의 환경 한계다.

## 제한과 다음 조각

이 결과는 합성 snapshot에 대한 hard gate만 입증한다. live telemetry의 정확성,
실제 자동 매칭, winner 선택, 자원 원자 예약은 입증하지 않는다. DoD-41 evidence와
독립 검수도 이번 사용자 범위에 없으며, 특히 사용자가 `docs/evidence/DoD-NN_*.md`와
`docs/history/HISTORY.md` 수정을 금지했으므로 DoD PASS를 주장하지 않는다.
