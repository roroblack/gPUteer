# 작업 이력

> ★ **추가만 한다. 기존 기록을 수정하지 않는다.**
> 최신이 위로 오도록 **역순**으로 쌓는다.

형식:

```markdown
## YYYY-MM-DD HH:mm — <작업 제목>
- 계획: <docs/plans/ 문서명> 의 <단계>
- 스트림: <RULE.md §4.1 의 스트림명>
- 수행: <핵심 변경 요약>
- 검증: <성공/실패 + 방법>
- 리포트: <docs/reports/ 파일명>
```

---

## 2026-08-20 04:15 — 백로그 정리 + 자동 재접속 루프·Job 시작 마커 설계 + REVOKED signed outcome (`DoD-27`)
- 계획: `docs/plans/2026-08-20_0357_revoked_signed_outcome_v1.md`
  (구현), `docs/plans/2026-08-20_0300_자동_재접속_루프_전체_설계_v1.md`·
  `docs/plans/2026-08-20_0310_job_시작_마커_최소_조각_v1.md`(설계
  전용, 구현 미착수).
- 스트림: Coordinator · Agent · QA · 문서.
- 수행: 사용자가 취침 전 "가능한 문서 작업 포함해서 코덱스 쿼터를
  최대한 태워라" 고 지시해, 세 갈래로 동시에 진행했다. (1) 백로그
  전체 조사(`p152`)가 오늘 조각들의 "Out" 절과 evidence
  limitations 를 전부 모아 우선순위를 매겼다 — 1순위
  `REVOKED` signed outcome, 2순위 레거시 경로 fail-closed, 3순위
  `check_schema.py` CI 연결. (2) 두 개의 read-only 설계 조사를
  병렬로 돌려(코드 충돌 없음) 큰 항목들을 문서화만 했다 — 자동
  재접속 루프 전체 설계(`p153`, 결론: 최소 6~7개 조각·6~8일 규모,
  proto 변경 필요, 로드맵만 저장)와 Job 실행을 향한 최소 첫 걸음
  설계(`p154`, 결론: `WRITING` 마커 후보가 하루 규모로 가능, 구현은
  다음 조각으로 등록만). (3) 1순위 후보 `REVOKED` signed outcome
  을 실제로 구현했다(`p155`, 코덱스 workspace-write) — proto 에
  `RENEW_OUTCOME_REVOKED=8` 순수 추가, Coordinator 가 갱신 경로의
  revoked 거부를 raw error 대신 서명된 결과로 응답, 구현자가 오늘
  이미 두 번(`DoD-22`·`DoD-23`) 나온 outcome-분기 교착 패턴을 스스로
  의식해 처음부터 올바르게 구현. 독립 검수(`p157`, 대화 기록 없는
  새 코덱스 인스턴스)가 1라운드 만에 `ACCEPTED`.
- 검증: `reference_canonical.py --self-test` PASS, `check_schema.py`
  오류 0건, `cargo build`/`test --workspace --exclude gputeer-runtime-windows`
  전체 회귀 없음(42개 스위트), `coordinator-agent-selftest` 5회
  연속 37개 시나리오 전부 exit=0, 뮤테이션(outcome 8 break 제거)이
  정확히 예상한 교착을 재현. `python scripts/verify_evidence.py`
  스키마 위반 없음(PASS 35/36).
- 리포트: `docs/reports/2026-08-20_0357_revoked_signed_outcome.md`
  (구현자가 작성).

---

## 2026-08-20 02:00 — 만료된 Lease 재접속 거부 — 구현 + 독립 검수 2라운드 + evidence 기록 (`DoD-26`)
- 계획: `docs/plans/2026-08-20_0136_만료_lease_재접속_거부_v1.md` 전체
  단계.
- 스트림: Coordinator · QA.
- 수행: `DoD-24` 가 명시적으로 이월한 "만료된 Lease 의 재접속 복원
  거부" 시나리오를 구현했다. `CoordinatorLeaseStore::get_or_issue()`
  에 만료 검사(`LeaseStoreError::Expired`)를 추가하고, 짧은 TTL 로
  Lease 를 실제로 만료시킨 뒤 재접속하면 거부되는 selftest 시나리오
  36 을 신설했다(구현 `p148`, 코덱스 workspace-write). 이 세션이
  독립 재검증 후 대화 기록이 없는 새 코덱스 인스턴스에게 독립
  검수를 요청했는데, 1라운드(`p149`)가 진짜 계층 간 결함을 찾아냈다
  — Coordinator 의 만료 판정이 엄격한 `<` 를 써서, 이미 이 저장소가
  정착시킨 "경계 포함"(`<=`) 만료 규칙(`crates/protocol/src/signing.rs`
  의 Lease 서명 검증, `crates/agent/src/lib.rs` 의 revoke 검사)과
  어긋났다 — 정확히 만료 시각과 같은 순간에 Coordinator 는 재발급을
  허용하는데 Agent 는 같은 Lease 를 즉시 거부하는 모순이 생길 뻔
  했다. `<` 를 `<=` 로 고치고 경계값 테스트 2건을 추가한 뒤(`p150`)
  2라운드(`p151`)에서 `ACCEPTED`.
- 검증: `cargo build`/`test --workspace --exclude gputeer-runtime-windows`
  전체 회귀 없음(42개 스위트, 0 failed, coordinator 유닛 테스트
  24→27개). `coordinator-agent-selftest` 2세트 x 5회(구현 직후 +
  경계 수정 직후, 각 60초 하드 타임아웃) 전부 36개 시나리오 exit=0.
  `python scripts/verify_evidence.py` 스키마 위반 없음(PASS 34/35).
- 리포트: `docs/reports/2026-08-20_0136_만료_lease_재접속_거부.md`
  (구현자가 작성).

---

## 2026-08-19 19:15 — Coordinator Lease revoke 영속화 — 구현 + 독립 검수 1라운드 + evidence 기록 (`DoD-25`)
- 계획: `docs/plans/2026-08-19_0110_coordinator_lease_revoke_영속화_v1.md`
  전체 단계.
- 스트림: Coordinator · QA.
- 수행: `DoD-24` 가 명시적으로 남긴 안전 공백("revoke 된 Lease 가
  재접속으로 되살아난다")을 닫았다. 설계 조사(코덱스 `p145`,
  read-only)가 `CoordinatorLeaseStore` 스키마·`get_or_issue()`·
  `send_revoke_notice()` 를 실측해 SQLite `ALTER TABLE` 마이그레이션이
  필요함을 짚었다. 구현(`p146`, 코덱스 workspace-write)이
  `revoked_at_unix_ms` 컬럼 추가(기존 DB 파일도 `open()` 시점에
  `PRAGMA table_info`+`ALTER TABLE` 로 보정), `mark_revoked()`(idempotent),
  `get_or_issue()`·갱신 경로 양쪽(정상 경로 + `renew_outcome_override`
  읽기 전용 경로) 의 revoked 거부, `send_revoke_notice()` 가 wire
  전송 전에 커밋을 먼저 확정하는 순서를 구현했다. 이번엔 구현자가
  evidence·CLAUDE.md·HISTORY 를 건드리지 않아 구현자/검수자 경계가
  이전 조각들보다 깔끔했다. 이 세션이 독립적으로 재검증(빌드·테스트·
  `coordinator-agent-selftest` 5회 반복, 각 90초 하드 타임아웃)한 뒤,
  대화 기록이 없는 새 코덱스 인스턴스(read-only)에게 독립 검수를
  요청했다 — 마이그레이션·두 갱신 경로·override 시 실제 lease_id
  기록 여부까지 전부 확인하고 1라운드(`p147`)에서 `ACCEPTED`(유일한
  지적은 코드가 아니라 검수 프롬프트의 시나리오 번호 오기였다).
- 검증: `cargo build`/`test --workspace --exclude gputeer-runtime-windows`
  전체 회귀 없음(42개 스위트, 0 failed, coordinator 유닛 테스트
  20→24개). `coordinator-agent-selftest` 5회 연속 35개 시나리오
  전부 exit=0. `python scripts/verify_evidence.py` 스키마 위반
  없음(PASS 33/34).
- 리포트: (구현자가 리포트를 작성하지 않았다 — 이 조각은 규모가
  작아 HISTORY 항목과 `DoD-25` evidence 로 기록을 갈음한다).

---

## 2026-08-19 18:30 — Lease 재접속 최소 조각 — 독립 검수 1라운드 + evidence 기록 (`DoD-24`)
- 계획: `docs/plans/2026-08-19_1814_lease_재접속_최소_조각_v1.md`
  (구현은 18:14 항목 참조 — 이 항목은 그 뒤의 독립 검수·기록).
- 스트림: Coordinator · CLI(selftest) · QA.
- 수행: 18:14 항목의 결과물(설계 조사 `p142` → 구현 `p143`)을 이
  세션이 독립적으로(빌드·테스트·`coordinator-agent-selftest` 5회
  반복, 각 60초 하드 타임아웃) 재검증했다. 오늘 이미 두 번(Lease
  revoke·SUPERSEDED 조각) "한쪽은 끝났는데 다른 쪽은 계속 기다리는"
  교착 버그가 나왔던 걸 감안해, 이번에도 같은 패턴이 있는지 특히
  의심하며 대화 기록이 없는 새 코덱스 인스턴스(read-only)에게 독립
  검수를 요청했다. 이번 설계는 애초에 그 위험 구조를 피했음을
  확인(Coordinator 가 연결을 끊으면 Agent 의 쓰기/읽기는 무한
  대기가 아니라 즉시 EOF 오류로 끝난다) — 1라운드(`p144`)에서
  `ACCEPTED`.
- 검증: `cargo build`/`test --workspace --exclude gputeer-runtime-windows`
  전체 회귀 없음(42개 스위트, 0 failed). `coordinator-agent-selftest`
  5회 연속 34개 시나리오 전부 exit=0. `python scripts/verify_evidence.py`
  스키마 위반 없음(PASS 32/33).
- 리포트: `docs/reports/2026-08-19_1814_lease_재접속_최소_조각.md`
  (구현자가 작성 — 이 세션은 별도 리포트를 새로 쓰지 않고 이
  HISTORY 항목과 `DoD-24` evidence 로 검수·기록 단계를 남긴다).

---

## 2026-08-19 18:14 — Active Lease process-restart rehydration 최소 조각

- 계획: `docs/plans/2026-08-19_1814_lease_재접속_최소_조각_v1.md`
- 스트림: Coordinator · CLI(selftest)
- 수행: ACK 직후 연결 단절 주입(`--disconnect-after-ack`), 동일 Lease/Fence DB를
  사용하는 새 프로세스 쌍의 Lease 복원 시나리오 33, 다른 holder identity 거부
  시나리오 34 추가. 기존 `holder_node_id` 검사는 변경하지 않음.
- 검증: build exit 0, workspace test exit 0, 60초 하드 타임아웃 selftest 5회 연속
  34/34 통과. 단절 분기 mutation은 시나리오 33에서 exit 1, 원복 후 재통과.
- 리포트: `docs/reports/2026-08-19_1814_lease_재접속_최소_조각.md`
- evidence: `DoD-24_lease_재접속_최소_조각.md`(이 세션이 독립 검수 후 기록)

---

## 2026-08-19 18:00 — Coordinator SUPERSEDED 정책 — 독립 검수 2라운드 + evidence 기록 (`DoD-23`)
- 계획: `docs/plans/2026-08-19_1725_lease_재발급_정책_superseded_v1.md`
  (구현은 17:25 항목 참조 — 이 항목은 그 뒤의 독립 검수·수정·기록).
- 스트림: Coordinator · Agent · CLI(selftest) · QA.
- 수행: 17:25 항목의 결과물을 이 세션이 독립적으로 재검증(빌드·
  테스트·`coordinator-agent-selftest` 5회 반복)한 뒤, 대화 기록이
  없는 새 코덱스 인스턴스(read-only)에게 독립 검수를 받았다. 코드를
  직접 읽던 중 이 세션 스스로도 의심스러운 지점(SUPERSEDED 응답 후
  `continue` — 오늘 이미 Lease revoke 조각에서 같은 부류의 결함이
  나왔었다)을 먼저 포착해, 미리 알리지 않고 블라인드로 독립 검수를
  돌려 교차 확인했다. 1라운드(`p139`)가 정확히 그 지점을 지적 —
  `renew_rounds > 1` 이고 SUPERSEDED 가 마지막이 아닌 회차에서
  발생하면 Coordinator 가 오지 않을 프레임을 기다리는 교착. 기존
  시나리오 25·26 은 renew_rounds 기본값 1 이라 이 조합을 우연히
  피해가 안 드러났었다. 워크스페이스에 write 권한을 준 다른 코덱스
  인스턴스가 `continue` 를 `break` 로 고치고, Agent 가 즉시 종료하는
  다른 outcome(QUARANTINED·MAX_DURATION_EXCEEDED)도 같은 위험이
  있음을 확인해 공통으로 일반화했다(`p140`) — 이전 조각들부터
  잠재했을 수 있는 위험을 부수적으로 닫은 것이다. 이 세션이 그
  수정을 직접 되돌려 뮤테이션을 재현해(정확히 시나리오 32 에서
  exit=1, 스트림 끊김) 코덱스의 자체 보고와 별개로 결함의 실재를
  재확인했다. 2라운드(`p141`)에서 `ACCEPTED`.
- 검증: `cargo build`/`test --workspace --exclude gputeer-runtime-windows`
  전체 회귀 없음(42개 스위트, 0 failed). `coordinator-agent-selftest`
  총 3세트(1차 5회 31개 시나리오 + 수정 후 5회 32개 시나리오 + 최종
  재확인 5회 32개 시나리오) 전부 exit=0. `python scripts/verify_evidence.py`
  스키마 위반 없음(PASS 31/32).
- 리포트: `docs/reports/2026-08-19_1725_lease_재발급_정책_superseded.md`
  (구현자가 작성 — 이 세션은 별도 리포트를 새로 쓰지 않고 이
  HISTORY 항목과 `DoD-23` evidence 로 검수·기록 단계를 남긴다).

---

## 2026-08-19 17:25 — 영속 Lease lower epoch SUPERSEDED 정책
- 계획: `docs/plans/2026-08-19_1725_lease_재발급_정책_superseded_v1.md`
- 스트림: Coordinator · Agent · CLI(selftest)
- 수행: `lease_store=Some`에서 저장 epoch보다 낮은 `RenewLeaseRequest.fence_epoch`를
  연결 종료 없이 signed `RENEW_OUTCOME_SUPERSEDED`로 응답. Agent 기존 정책 거부
  경로를 계산 결과에도 적용 확인. selftest 31개(새 lower epoch 및 same epoch 대조군).
- 결정: `lease_store=None`과 높은 epoch는 기존 hard error 유지. QUARANTINED 실제
  판정은 위험도/신뢰도 인프라가 없어 TODO_VISION V-11로 등록. 높은 epoch의 새
  재발급 정책은 후속 범위.
- 검증: build 성공, workspace test exit 0, selftest 5회 연속 31/31 성공.
  lower-epoch 분기와 outcome 값을 각각 무력화한 mutation test가 시나리오 25에서
  실패했고 두 변경 모두 원복.
- 리포트: `docs/reports/2026-08-19_1725_lease_재발급_정책_superseded.md`
- evidence: 사용자 지시에 따라 이번 세션에서는 작성하지 않음(독립 검수 이관)

## 2026-08-19 16:10 — Lease revoke 최소 조각 — 독립 검수 3라운드 + evidence 기록 (`DoD-22`)
- 계획: `docs/plans/2026-08-19_1517_lease_revoke_최소_조각_v1.md` 5단계
  (구현은 15:32 항목 참조 — 이 항목은 그 뒤의 독립 검수·수정·기록).
- 스트림: Coordinator · Agent · CLI(selftest) · QA.
- 수행: 사용자 요청("코덱스 cli 에 5.6 솔로 작업")에 따라 구현
  자체를 workspace-write 코덱스 인스턴스에 위임했던 15:32 항목의
  결과물을, 이 세션이 독립적으로(빌드·테스트·`coordinator-agent-selftest`
  5회 반복) 재검증한 뒤, 대화 기록을 공유하지 않는 새 코덱스
  인스턴스(read-only)에게 3라운드 독립 검수를 받았다. 1라운드
  (`p134`)가 진짜 교착 결함(`--revoke-after-round 0` + 양쪽
  `do_renew=true` 조합에서 Coordinator 가 오지 않을 프레임을 기다림)
  을 포함해 4건을 찾아 workspace-write 코덱스가 전부 수정했고
  (`p135`), 이 세션이 그 교착 방지 가드를 직접 되돌려 뮤테이션을
  재현해(정확히 시나리오 25 에서 exit=1) 코덱스의 자체 보고와
  별개로 결함의 실재를 재확인했다. 2라운드(`p136`)는 계획 문서
  개정 이력 누락만 지적해 이 세션이 직접 고쳤고, 3라운드(`p137`)
  에서 `ACCEPTED`.
- 검증: `cargo build`/`test --workspace --exclude gputeer-runtime-windows`
  전체 회귀 없음(42개 스위트, 0 failed). `coordinator-agent-selftest`
  총 2세트(구현 직후 5회 + 수정 직후 5회, 각 15~20초 하드 타임아웃)
  전부 exit=0, 29/29 시나리오. `python scripts/verify_evidence.py`
  스키마 위반 없음(PASS 30/31).
- 리포트: `docs/reports/2026-08-19_1532_lease_revoke_최소_조각.md`
  (구현자가 작성 — 이 세션은 별도 리포트를 새로 쓰지 않고 이
  HISTORY 항목과 `DoD-22` evidence 로 검수·기록 단계를 남긴다).

---

## 2026-08-19 15:32 — Lease revoke 최소 조각
- 계획: `docs/plans/2026-08-19_1517_lease_revoke_최소_조각_v1.md` 전체 단계.
- 스트림: Coordinator · Agent · CLI(selftest).
- 수행: Coordinator가 서명한 `RevokeLeaseNotice`를 같은 연결로 보내고,
  Agent가 서명·lease_id·fence_epoch·만료 상태를 검증한 뒤 revoked 상태로
  전이하도록 구현했다. revoke 뒤 다음 renew 회차를 요청 전에 차단하고
  selftest 시나리오 25~29를 추가했다.
- 검증: `cargo build --workspace --exclude gputeer-runtime-windows` 성공,
  `cargo test --workspace --exclude gputeer-runtime-windows` exit 0,
  `coordinator-agent-selftest` 29/29 5회 연속 성공. lease_id 검사와
  revoke 서명 변조 분기 각각을 임시 무력화한 뮤테이션이 해당 negative
  시나리오를 예상대로 실패시킨 뒤 원복했다.
- 리포트: `docs/reports/2026-08-19_1532_lease_revoke_최소_조각.md`

## 2026-08-19 13:00 — `write_once()` 동시 호출 계약 — 락 강제 + GC 조정 (`DoD-21`)

- 계획: `docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md`
  전체 5단계.
- 스트림: Checkpoint.
- 수행: `crates/checkpoint/src/atomic.rs::write_once()` 에
  `std::fs::File::try_lock()` 기반 프로세스 간 파일 잠금을 추가해
  같은 `(dir, name)` 동시 호출을 `CheckpointError::WriteInProgress`
  로 명시적으로 거부하도록 강제했다(정책 A, 코덱스 설계 `p127`).
  성공 시 락 파일을 자가 정리하고, `gc_partial()` 은 자신이
  `try_lock` 을 직접 시도해 아무도 안 쥔 죽은 락만 회수한다.
  `.write_once.lock` 접미사(대소문자·후행 점/공백 무관)를
  `validate_relative_name()` 에서 예약해 락 경로와 데이터 파일
  경로의 이름공간 충돌을 원천 차단했다. `k1c` 를 결정론적 테스트로
  재설계하고 `k1d`·`k1e`·`k1f`·`k4c` 4개를 신규 추가했다.
- 검증: 코덱스 독립 검수 5라운드(`p128`~`p132`) — 매 라운드가 실제
  결함을 찾았다: MSRV 불일치(`Cargo.toml` 1.85 vs `try_lock` 요구
  1.89) → GC 죽은 락 영구 보존으로 PARTIAL 디렉터리 청소 불능
  → 이름공간 충돌로 등록 데이터 파일 삭제 가능성(1차 부분 수정)
  → 그 근본 원인(락/데이터 경로 자체 충돌) → 대소문자·후행 점/공백
  우회. 전부 코드로 고치고 `p132` 에서 **ACCEPTED**. 뮤테이션
  테스트 6건 전부 정확히 예측한 테스트만 실패 확인 후 원복.
  `cargo test -p gputeer-checkpoint` 5회 연속 통과(53개),
  `cargo test --workspace --exclude gputeer-runtime-windows` 회귀
  없음(42개 스위트). `python scripts/verify_evidence.py` 스키마
  위반 없음.
- 리포트: `docs/reports/2026-08-19_1300_write_once_동시_호출_계약.md`

---

## 2026-08-19 11:05 — `remote5090` 원격 Linux+GPU 기계 실측 — D-3 부분 해소 (`ENV-03`)

- 계획: (사용자가 직접 원격 기계 접속 정보를 제공 — 별도 계획 문서
  없이 즉시 실측으로 진행).
- 스트림: QA · Checkpoint(addendum) · Tooling(신규 프로브).
- 수행: 사용자 소유의 임시 원격 기계(Ubuntu 24.04.3, RTX 5090)에
  Rust 를 사용자 권한으로 설치하고 `git bundle` 로 저장소를 옮겨
  이 저장소를 처음으로 Linux 에서 빌드·테스트했다
  (`gputeer-runtime-windows` 제외, k1c 한 건만 플랫폼 차이로 FAILED).
  `tools/probes/linux_cgroup_probe.py` 신설 — `sudo` 없이
  `systemd-run --user --scope` 로 위임된 cgroup v2(memory/CPU/PID/
  freeze) 강제를 실측했다.
- 검증: 코덱스 검수 3라운드(`p124` CHANGES_REQUESTED — cgroup 수치
  오기재·디스크 용량 오기재·다른 로그인 사용자 과장 등 7건 →
  raw 로그 재수집·문서 정정 → `p125` CHANGES_REQUESTED — 잔여 3건
  (freeze/thaw 과장·who/ps 시점·rustup 설치 로그 구분) → 정정 →
  `p126` **ACCEPTED**). `docs/evidence/P0-06_vram_enforcement.md` 에
  cgroup 결과 addendum, `docs/evidence/DoD-08_독립검수_시정.md` 에
  k1c 크로스플랫폼 관측 addendum(코드/테스트는 변경하지 않음) 추가.
  `CLAUDE.md`·`docs/plans/2026-08-15_1330_...v1.md` 의 D-3 상태를
  "미해결 — 최대 차단 요인" → "부분 해소 — 임시 접근(확보 아님)"
  으로 갱신.
- 리포트: (다음 세션 종료 리포트에 포함 예정 — 이 항목은 작업 직후
  즉시 기록해 §3.4 재발을 막는다).

---

## 2026-08-19 01:10 — 자율 세션 종료 리포트 제출 (`DoD-13`~`DoD-20`, 8건 소급)

- 계획: 아래 8개 항목 전체.
- 스트림: Coordinator · Agent · Protocol · CLI(selftest) · Tooling.
- 수행: `RULE.md` §3.4 가 요구하는 세션 종료 리포트를 8개 조각 모두
  제출하지 않고 진행해 왔던 것을 뒤늦게 발견 — 종합 리포트 1건과
  이 이력 항목들을 소급 작성했다.
- 검증: `git status --short` 클린, `verify_evidence.py` 스키마 위반 0(28건,
  PASS 27 · FAIL-SCOPE 1).
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

## 2026-08-19 01:02 — `check_schema.py` — 실행 환경 오류를 `exit(2)` 로 통일 (`DoD-20`)

- 계획: `docs/plans/2026-08-20_0000_check_schema_py_v1.md`.
- 스트림: Tooling.
- 수행: 코덱스 1라운드 검수(`p120`)가 자기 샌드박스에서 실제로
  실행해보다가 임시 파일 생성 실패가 처리되지 않은 예외로 새어나가
  `exit(1)` 이 되는 것을 재현. `build_descriptor_set()` 전 구간을
  `OSError`/`message.DecodeError` 로 감싸 `exit(2)` 로 통일.
- 검증: 수정 전/후 버전 대조로 재현·해소 확인. `python check_schema.py`
  정상 경로 오류 0건, `cargo test --workspace`·`coordinator-agent-selftest`
  24/24 회귀 없음. 2라운드(`p121`) `ACCEPTED`.
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

## 2026-08-19 00:53 — `tools/canonical/check_schema.py` 신규 구현 (`DoD-20`)

- 계획: `docs/plans/2026-08-20_0000_check_schema_py_v1.md`.
- 스트림: Tooling.
- 수행: `proto/README.md` 가 오래전부터 안내·전제해온 스키마 표
  정합성 검사기가 실제로는 없었다(코덱스 최종 확인 감사 `p118` 이
  발견). `protoc --descriptor_set_out` → `google.protobuf.descriptor_pb2`
  구조 파싱으로 `reference_canonical.py` 의 `SCHEMAS` 와 `.proto` 를
  대조. field 90(서명 필드)은 `canonical_encode()` 가 번호로만
  무조건 건너뛰므로 타입 비교에서 명시적으로 제외.
- 검증: 설계 실측(`p119`)이 실제 drift 3건(전부 field 90, 인코딩엔
  무영향) 발견. 뮤테이션 4건(타입/번호/이름/field-90-예외 무력화)
  전부 예측대로 검출. 현재 저장소 상태 오류 0건.
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

## 2026-08-19 00:33 — 오래된 테스트 공백 3건 보강 (`DoD-19`)

- 계획: (코덱스 감사 `p116` 이 직접 찾은 후보 — 별도 계획 문서 없이
  소규모 수정으로 진행).
- 스트림: Protocol · Coordinator · Crypto.
- 수행: `canonical_vectors.rs` 의 손으로 쓴 domain 배열이 24종으로
  뒤처져 있던 것(실제 25종, `DoD-13` 의 `LeaseRenewResult` 반영 안 됨),
  `CoordinatorLeaseStore::check_identity_conflict()` 의 4개 필드 중
  `job_id` 만 테스트되던 것, `RenewLeaseResult` framed 정상 테스트가
  payload 필드를 검증 안 하던 것(`DoD-17` 과 같은 패턴) — 3건 전부
  수정.
- 검증: 뮤테이션 2건(identity conflict 검사 제거, `detail` 기대값
  오염) 전부 예측대로 검출. 1라운드(`p117`) `ACCEPTED`.
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

## 2026-08-19 00:13 — `max_total_duration_seconds` 갱신 차단 정책 (`DoD-18`)

- 계획: `docs/plans/2026-08-19_2350_max_total_duration_seconds_갱신_차단_v1.md`.
- 스트림: Coordinator · Agent · CLI(selftest).
- 수행: 오래전부터 스키마에만 있던 `max_total_duration_seconds`/
  `RENEW_OUTCOME_MAX_DURATION_EXCEEDED` 를 처음으로 실제 판정하게
  만듦. `CoordinatorLeaseStore::renew_existing_within_duration()`
  이 조회→판정→조건부 UPDATE 를 트랜잭션 하나로 묶는다. selftest
  시나리오 22~24 추가(24개 시나리오 도달).
- 검증: 코덱스 1라운드(`p114`)가 `lease_store=Some`+override 조합의
  저장소 상태 불일치 결함 발견 → 수정 → 2라운드(`p115`) `ACCEPTED`.
  뮤테이션 4건 전부 예측대로 실패·원복.
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

## 2026-08-18 23:20 — `RevokeLeaseNotice` framed ingress 커버리지 (`DoD-17`)

- 계획: (코덱스 감사 `p110` 이 직접 찾은 오래된 커버리지 공백 —
  별도 계획 문서 없이 소규모 테스트 추가로 진행).
- 스트림: Crypto.
- 수행: `FrameType::LeaseRevoke` 배선은 있었지만 실제 서명 왕복
  테스트가 한 번도 없었다. 정상/위조 서명 테스트 2건 신설.
- 검증: 코덱스 1라운드(`p112`)가 정상 테스트를 payload 필드 미검증
  으로 지적 → 필드 assert 추가 → 2라운드(`p113`) `ACCEPTED`.
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

## 2026-08-18 22:39 — Coordinator 영속 Lease 저장소 (`DoD-16`)

- 계획: `docs/plans/2026-08-19_2300_coordinator_영속_lease_저장소_v1.md`.
- 스트림: Coordinator.
- 수행: `CoordinatorLeaseStore`(SQLite) 신설 — 재시작 후에도 발급한
  Lease 의 신원·epoch 를 기억한다(`--lease-db`, optional). selftest
  시나리오 20·21 로 별도 프로세스 재시작 경계에서 실측.
- 검증: 코덱스 1라운드(`p108`)가 `max_total_duration_seconds` 의
  `u64`→`u32` 무검사 캐스팅(silent truncation) 발견 → `u32_from_stored()`
  fail-closed 헬퍼로 수정 → 2라운드(`p109`) `ACCEPTED`.
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

## 2026-08-18 22:20 — 같은 연결에서 반복 Lease 갱신 (`DoD-15`)

- 계획: `docs/plans/2026-08-19_2330_같은_연결_반복_lease_갱신_v1.md`.
- 스트림: Coordinator · Agent · CLI(selftest).
- 수행: `derive_renew_nonce(lease_id, round)` 로 회차별 nonce 분리,
  `--renew-rounds` CLI 플래그. 설계 단계(`p105`, **코드 작성 전**)가
  "회차마다 nonce 가 같아 2회차부터 Replay 로 100% 거부된다" 는
  결함을 코드 경로 재확인만으로 미리 찾아 재작업을 막았다.
- 검증: 뮤테이션(`round` 고정)으로 그 실패가 예측대로 재현됨을 확인.
  코덱스 1라운드(`p107`)만에 `ACCEPTED` — 이 세션에서 유일하게
  1라운드 만에 추가 결함 없이 통과한 조각 중 하나.
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

## 2026-08-18 17:36 — Agent 쪽 durable FenceWatermark (`DoD-14`)

- 계획: `docs/plans/2026-08-19_2200_durable_fence_watermark_v1.md`.
- 스트림: runtime-policy · Agent · CLI(selftest).
- 수행: `crates/runtime-policy::DurableFenceWatermark`(SQLite) 신설 —
  최초 Grant·갱신 검증 두 호출부 모두 재시작을 넘는 epoch 강등
  방어를 갖춘다. `--fence-db :memory:` 는 fail closed.
- 검증: **자체 발견** — "갱신 경로 전용" restart-defense 시나리오가
  같은 프로세스의 최초 Grant 검증 때문에 저장소 진위와 무관하게
  공허하게 통과하는 함정을 뮤테이션 테스트로 스스로 잡아내
  시나리오를 다시 설계(§2.1). 코덱스 1라운드(`p102`)가 `:memory:`
  fail-open · 에러 메시지 접두사 충돌 2건 발견 → 수정 → 2라운드
  (`p103`) `ACCEPTED`.
- 리포트: `docs/reports/2026-08-19_0110_lease_영속화와_스키마_검사기_자율세션.md`

---

## 2026-08-19 05:00 — Lease 갱신 최소 조각 계획 수립 — `RenewLeaseResult` 미서명 공백 발견

- 계획: CLAUDE.md 백로그 1번의 다음 후보(Lease **갱신**,
  `RenewLeaseRequest` 왕복) — `2026-08-18_1800`(Lease 최소 조각)
  의 "Out" 절이 이미 예고한 항목.
- 스트림: —(계획 단계, 구현 아직 착수 안 함).
- 수행: 코덱스에게 설계를 요청했다(`p98` 프롬프트) — 1차 시도는
  `codex exec` 자체가 exit code 1 로 중간에 끊겼다(출력 파일 미생성,
  로그에 PowerShell `Get-Content` 로 읽은 한글 소스가 mojibake 로
  깨진 상태로 남음 — 원인은 확인 안 됨, 세션 스스로의 결함이
  아니라 codex 실행 환경 쪽 문제로 추정). 같은 프롬프트로 재시도해
  성공했다.
- ★ 설계 실측이 진짜 프로토콜 공백을 찾았다 — `RenewLeaseRequest`
  는 이미 `Signable` 이지만 **`RenewLeaseResult` 는 서명 필드도
  `Signable` 구현도 없다.** 결과 메시지(`RENEWED`/`SUPERSEDED`/
  `QUARANTINED` 판정과 새 Lease)가 인증되지 않으면, 공격자가 정상
  갱신 요청에 가짜 `QUARANTINED` 응답을 끼워 넣어 정당한 Agent 의
  작업을 강제 중단시킬 수 있다 — 이 조각의 In 범위에
  `RenewLeaseResult` 를 `Signable` 로 만드는 작업을 포함시켰다.
- `docs/plans/2026-08-19_0500_coordinator_agent_lease_갱신_최소_조각_v1.md`
  로 정리했다 — 같은 TCP 연결에 이어 붙이는 왕복(별도 연결 안 씀),
  거부/공격 경로 6종(위조 Request·위조 Result·nested Lease 위조·
  epoch 강등·SUPERSEDED·QUARANTINED), `FenceWatermark` 재사용
  (같은 epoch 허용은 이미 `same_epoch_reuse_is_allowed_by_design`
  이 보장), 키/시드는 새로 필요 없음, 8단계 계획표.
- 검증: 아직 없음 — 이 턴은 계획 수립까지다. 구현은 다음 단계.
- 리포트: 이 이력 항목 + 계획 문서 자체.

- 계획: CLAUDE.md 백로그 1번의 남은 항목(RULE.md §8 이 요구하는
  정식 `docs/evidence/` 기록). `2026-08-18_0800`(핸드셰이크)과
  `2026-08-18_1800`(Lease 최소 조각) 두 계획 문서 모두 구현·Codex
  독립 검수(당일, p57/p59/p69)까지 끝났지만 정식 evidence 문서가
  없었다.
- 스트림: Coordinator · Agent · CLI.
- 수행: `cargo build -p gputeer-cli` 로 오늘 HEAD 에서 바이너리를
  새로 빌드하고 `coordinator-agent-selftest` 를 5회 연속 실행 —
  6개 시나리오(정상·위조 Grant·위조 ACK·replay·위조 Lease·만료된
  Lease) 전부 매번 통과, 매번 다른 PID 3개 확인. `cargo test
  --workspace` 308 passed / 0 failed. 이 결과를
  `docs/evidence/_raw/DoD-11_selftest_2026-08-19.txt` 에 저장하고,
  **`DoD-11`(핸드셰이크 자체)과 `DoD-12`(Lease 최소 조각)** 두 새
  evidence 문서를 schema v2 로 직접 작성했다(신규 작성이라 v1
  단계 없이 바로 v2).
- 각 문서의 첫 라운드 독립 검수(`agent:codex-cli`, fresh-read-only,
  `p93`/`p94`)가 진짜 문제를 잡았다: **DoD-11** — (1) 존재하지
  않는 `review_artifact`, (2) `commit`(핸드셰이크 완성 시점)과
  `cli_bin`(오늘 빌드된, Lease 까지 반영된 바이너리) provenance
  혼동, (3) 뮤테이션 비공허성 주장에 오늘 재현한 로그가 없었음.
  **DoD-12** — `job_id` 를 "상관관계 검사"로 잘못 적음(실제로는
  단순 비공백 검사, watermark 키라서).
- 세 번째 지적(뮤테이션 미재현)은 실제로 오늘 다시
  재현했다 — `crates/coordinator/src/lib.rs` 의
  `corrupt_own_signature` 적용 분기를 `if false && ...` 로
  무력화 → selftest 가 정확히 예상대로 실패("위조된
  coordinator_signature 가 거부되지 않았다") → 원복 후 6개 시나리오
  재통과·`cargo test --workspace` 회귀 없음 재확인. `cli_bin`
  provenance 는 "오늘 HEAD(Lease commit `0be82e8` 까지 반영)에서
  빌드 — `commit` 필드는 핸드셰이크 완성 시점을 가리킬 뿐"로
  명확히 구분해 정정했다.
- `DoD-12` 의 `job_id` 정정은 한 번 더 라운드가 필요했다 — 처음
  고친 문구가 `holder_node_id` 를 "Grant 의 값과 일치"로 잘못
  묶었는데(실제로는 Agent 자신의 `config.agent_device_id` 와
  비교), 두 번째 재검수(`p96`)가 이를 잡았다. 세 번째 재검수
  (`p97`)에서 최종 `ACCEPTED`.
- v2 frontmatter 를 신규 작성(schema_version 2 부터 직접 시작,
  v1 유예 목록에 올릴 필요가 없었다), `review_artifact` 두 개
  (`DoD-11_review.txt`, `DoD-12_review.txt`)를 새로 만들었다 —
  최초 작성 시 파일:줄 인용 형식이 `verify_evidence.py` 의 비허위
  검사(`.txt` 확장자가 인용 정규식에 없어 "raw 로그:N" 류 인용이
  거부됨)에 걸려, 실제 `.rs` 소스 파일:줄 인용으로 다시 썼다.
  `docs/plans/2026-08-18_0800_...md`·`2026-08-18_1800_...md` 의
  DoD 체크박스도 완료로 갱신했다.
- 검증: `python scripts/verify_evidence.py` — 파일 20개(18→20),
  스키마 위반 0, PASS 19/20(`P0-06` 은 여전히 `FAIL-SCOPE`).
  `cargo test --workspace` 308 passed / 0 failed(회귀 없음),
  디스크 7.4GB 여유 유지 확인.
- 리포트: 이 이력 항목. CLAUDE.md 백로그 1번(coordinator/agent
  핸드셰이크 + Lease 최소 조각)의 evidence 기록 항목이 완전히
  해소됐다 — 다음 후보(`RenewLeaseRequest` 왕복 등)는 새 계획
  문서가 필요하다.

---

## 2026-08-19 03:40 — `AgentGrantAck` Python 참조 구현 교차검증 공백 해소, 부수 flaky 테스트 안정화

- 계획: CLAUDE.md 백로그 6번(DoD-05 v2 승격 재검수 중 발견한 공백,
  2026-08-18). v1→v2 승격 사이클 완료 직후 다음 백로그 항목으로
  자율 진행.
- 스트림: Protocol · Checkpoint.
- 수행: `tools/canonical/reference_canonical.py` 의 `SCHEMAS`·
  `DOMAIN_TAGS` 에 `AgentGrantAck` 추가(필드 1~8 + 서명 90,
  domain_tag `"gputeer/v1/grant-ack"`). 벡터 2건 생성 —
  `v32_agent_grant_ack`(전 필드, `missing_from_full` 로 완전성
  검사), `v32b_agent_grant_ack_different_nonce`(nonce 만 다름,
  canonical 이 달라야 함을 `MUST_DIFFER` 로 고정). `--emit-vectors`
  로 `tests/vectors/canonical_v1.json` 재생성(40→42건), `--verify`
  로 재생성 대조 통과 확인. Rust 쪽에
  `crates/protocol/tests/t1_signing_targets.rs::agent_grant_ack_matches_reference`
  를 추가해 Rust `to_canonical_fields()` 인코딩을 그 벡터와 바이트
  단위로 대조 — **통과**. Rust 와 Python 참조 구현이 `AgentGrantAck`
  에서도 일치함을 이번에 처음 확인했다(이전까지 이 메시지는
  참조 대조를 받은 적이 없었다 — 코드 결함은 아니었음이 확인됨).
- 부수 발견: `cargo test --workspace` 재실행 중
  `k1c_concurrent_same_name_writers_are_not_actually_safe`(P0-08
  승격 때 추가한 `write_once` 동시 호출 결함 고정 테스트)가 우연히
  `FAILED` 로 나왔다 — 재실행하니 다시 통과했다. 원인은 코드 회귀가
  아니라 **테스트 자체의 설계 결함**: 8스레드 단일 라운드 진짜
  경쟁에 의존하다 보니, 디스크 여유가 부족해 시스템 부하가 높을
  때 스케줄링이 우연히 직렬화돼 `ok_true==1` 이 나올 수 있었다.
  10라운드까지 반복해 **한 번이라도** 경합이 관측되면 통과하도록
  고쳐 재현 신뢰도를 높였다(3회 연속 재실행으로 안정성 확인) —
  경합 자체(더 넓은 결함)는 여전히 존재하며 이 수정은 그것을
  더 안정적으로 검출할 뿐이다.
- 검증: `cargo test --workspace` — 308 passed / 0 failed(신규
  `agent_grant_ack_matches_reference` 로 307→308). `python
  scripts/verify_evidence.py` — 스키마 위반 0, PASS 17/18 유지.
- ★ **디스크가 다시 타이트해졌다**(8.7GB → 7.4GB, 97% 사용,
  repo 밖 원인 계속 진행 중으로 추정). 이후 무거운 작업은 더욱
  신중하게 페이싱한다.
- 리포트: 이 이력 항목. CLAUDE.md 백로그 6번 완료 표시.

---

## 2026-08-19 03:05 — P0-08 schema v1 → v2 승격 완료 — ★ review-강제 대상 v1 evidence 부채 0건 달성

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). `P0-07` 에
  이은 다섯 번째이자 **이 저장소의 마지막 v1 evidence**.
- 스트림: Protocol.
- 수행: `cargo test -p gputeer-protocol --test schema_evolution`(6)
  + `--test schema_fingerprint`(2) + `cargo test --workspace`(307)
  를 직접 실행해 `docs/evidence/_raw/P0-08_v2_promotion_2026-08-18.txt`
  에 저장했다. 전체 재검수(`agent:codex-cli`, fresh-read-only, `p91`
  프롬프트, 원본 claim·negative_tests·limitations + 이전 addendum
  3라운드 전부 대상) — `CHANGES_REQUESTED`.
- **DoD 시리즈의 domain 수치 stale 패턴과 같은 뿌리(세션 중 계속된
  프로토콜 스키마 성장)가 이번엔 다른 지표에서 나타났다** — prost
  버전(limitation 의 "0.13 한 버전" → 실제 0.14, lockfile 0.14.4)
  과 schema fingerprint(frontmatter 의 "0a34709f...·66개 메시지·
  389개 필드" → 실제 "9638aba3...·67개 메시지·398개 필드", 이 세션
  중 `AgentGrantAck` 등 메시지 추가로 자연스럽게 늘어난 것). 새
  addendum(2026-08-18 03:00 경)으로 두 수치를 정정하고 q1·q4 를
  현재 prost 버전에서 재실행해 통과를 확인 — claim 자체는 지문의
  구체적 값과 무관하게 성립함을 명시했다. 원본 frontmatter 와
  이전 addendum 원문(옛 지문 값 포함)은 손대지 않았다. 좁은 후속
  재검수(`p92` 프롬프트) — **`ACCEPTED`.**
- v2 frontmatter(순수 additive) 추가, `artifacts:` 에 raw/review
  파일 2개 추가. `_schema_v1_grandfathered.txt` 에서 P0-08
  제거(3→2건, 이제 `ENV-01·02`(review 비강제)와
  `P0-06`(`status: FAIL-SCOPE`, 애초에 §7.3 대상 아님)만 남음).
  `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0,
  **"독립 검수 기록이 없는 P0/DoD PASS" 목록 자체가 완전히
  사라졌다(0건)** — DoD-01 부터 시작한 이 사이클의 목표가
  달성됐다(부채 13→11→10→9→8→7→6→5→4→3→2→1→**0**). `cargo test
  --workspace` 전체 재실행 — 307 passed / 0 failed(회귀 없음).
- ★ **디스크 여유가 다시 줄었다**(18GB → 8.7GB, 97% 사용) — 이
  repo 밖 어딘가에서 계속 공간을 소모하고 있다. `cargo clean` 으로
  1.4GiB 를 추가로 정리했으나 근본 원인은 여전히 세션 범위 밖이다.
  이후 작업은 무거운 빌드/테스트 사이클을 자제하고 디스크를 계속
  관찰하며 진행한다.
- 리포트: 이 이력 항목. **v1→v2 evidence 승격 사이클 전체가
  완료됐다** — DoD-01~08(8건) · P0-01·03·03a·07·08(5건), 총 13건
  전부 schema v2 로 승격됐고 전부 독립 검수 `ACCEPTED` 를 받았다.
  진짜 코드 결함 3건(DoD-02 도메인 커버리지 테스트·DoD-06
  all_domain_tags_are_distinct·DoD-08 write_once 경쟁 분기)을
  이 과정에서 찾아 고쳤다. 다음은 CLAUDE.md 백로그의 나머지 항목
  (예: `AgentGrantAck` Python 참조 구현 교차검증 공백, coordinator
  /agent 다음 확장 — Lease 갱신 등)으로 자율적으로 이어간다 —
  다만 디스크 여유를 먼저 확인하고 무거운 작업은 조절한다.

---

## 2026-08-19 02:35 — P0-07 schema v1 → v2 승격 완료 (기존 재실측 근거 재사용, 1라운드 ACCEPTED)

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). `P0-03a` 에
  이은 네 번째, `P0-08` 하나만 남기는 마지막 P0 승격.
- 스트림: —.
- 수행: `P0-07` 은 이미 같은 세션 안에서 두 번의 addendum 시퀀스
  (총 8라운드 재검수 — raw_output 수치 불일치 발견→status를
  PASS→INCONCLUSIVE 로 정정, 이어서 실제 x600 SSH 재실측→claim
  재확인→status 를 다시 PASS 로 복귀)를 거쳤다. 이번 v2 승격은
  그 기존 재실측(`_raw/P0-07_probe_2026-08-18_rerun.txt`, 2026-08-18
  06:48 실행)을 근거로 재사용하고 중복 실행하지 않았다 — 대신
  문서 전체(원본 YAML + 두 addendum 시퀀스 전부)를 v2 승격
  관점에서 세 번째로 처음부터 재검수시켰다(`agent:codex-cli`,
  fresh-read-only, `p90` 프롬프트) — **`ACCEPTED`**(1라운드 만에
  통과, 이 promotion 사이클에서 세 번째로 1라운드 통과).
- status(PASS)의 두 번 왕복이 일관되게 기록됐는지, claim 범위
  축소(재실측이 RUN1 만 반복했다는 제한 포함)가 정직한지,
  raw_output 수치 불일치가 "미해결로 남아 있다"는 구분이 얼버무려
  지지 않았는지, x600 원격 실행 자체의 진정성을 이 evidence
  스키마가 보장 못한다는 자기 인정이 `RULE.md` §7.3/`ADR-030` 의
  실제 요구사항과 맞는지 — 전부 재확인됐다. `python
  scripts/verify_evidence.py` 를 직접 실행해 exit 0·`status: PASS`
  집계·스키마 위반 0 도 재확인했다.
- v2 frontmatter(순수 additive, `executed_at` 을 실제 재실측
  시각인 2026-08-18 06:48 로 기록) 추가, `artifacts:` 에 기존
  rerun 파일 + 새 raw/review 파일 3개 추가.
  `_schema_v1_grandfathered.txt` 에서 P0-07 제거(4→3건),
  `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **2 → 1건**(`P0-08` 만 남음). `cargo test
  --workspace` 전체 재실행 — 307 passed / 0 failed(회귀 없음),
  디스크 18GB 여유 유지 확인.
- 리포트: 이 이력 항목. 다음은 `P0-08` 하나 — 승격하면 review-강제
  대상 evidence 부채가 **완전히 0건**이 된다(`ENV-01·02` 는 비강제
  라 별개로 남을 수 있다).

---

## 2026-08-19 02:05 — P0-03a schema v1 → v2 승격 완료 (축소 규모 재확인)

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). `P0-03` 에
  이은 세 번째.
- 스트림: —.
- 수행: `tools/probes/windows_fs_atomicity.py` 는 로컬에서 재실행
  가능한 순수 파일시스템 조사라 `P0-01`(원격 GPU) 과 달리 실제로
  다시 돌렸다 — 다만 원본 기본값(`--iterations 3000`, 약 10000회
  파일 연산)은 이 세션 중 있었던 디스크 100% 소진 사고를 감안해
  **축소 규모(`--iterations 300`)로 재실행**했다. 정확한 카운트
  재현이 아니라 정성적 패턴(PASS/FINDING/FAIL/FAIL/PARTIAL/PASS/
  PASS, "부분 내용 0건") 재현이 목적임을 명시하고
  `docs/evidence/_raw/P0-03a_v2_promotion_2026-08-18.txt` 에 저장했다.
  전체 재검수(`agent:codex-cli`, fresh-read-only, `p89` 프롬프트,
  원본 claim·negative_tests·limitations + 2026-08-18 addendum 3라운드
  전부 대상) — **`ACCEPTED`**(1라운드 만에 통과, 이 promotion
  사이클에서 두 번째로 1라운드 통과).
- claim·ADR-026 반영(`write_once`/`replace_with_retry`/Windows
  `sync_dir`)·프로브 구현·`FILE_SHARE_DELETE` limitation 정밀화가
  전부 재확인됐고, 축소 규모 재확인도 원본과 정성적으로 동일한
  패턴임이 확인됐다. Codex 자신의 read-only 샌드박스는
  `tempfile.mkdtemp()` 단계에서 쓸 수 있는 임시 디렉터리가 없어
  직접 재실행은 못 했다(샌드박스 제약) — 이 세션이 이미 로컬에서
  실행한 결과를 근거로 판단했다.
- v2 frontmatter(순수 additive, `review_scope` 에 "원본 넓은 claim
  이 아니라 addendum 의 좁힌 claim 을 근거로 삼는다"를 명시) 추가,
  `artifacts:` 에 raw/review 파일 2개 추가.
  `_schema_v1_grandfathered.txt` 에서 P0-03a 제거(5→4건),
  `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **3 → 2건**(`P0-07·08` 만 남음). `cargo test
  --workspace` 전체 재실행 — 307 passed / 0 failed(회귀 없음),
  디스크 18GB 여유 유지 확인.
- 리포트: 이 이력 항목. 다음은 `P0-07·08` — 이 둘만 남으면
  review-강제 대상 evidence 부채가 완전히 해소된다.

---

## 2026-08-19 01:35 — P0-03 schema v1 → v2 승격 완료 — kill_chaos 카오스 메커니즘이 시간 기반→이벤트 기반으로 바뀐 것을 정밀화

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). `P0-01` 에
  이은 두 번째. 2026-08-17 addendum 이 이미 "원본 claim·나머지
  limitations 는 아직 재검수 안 받았다"고 예고한 그 작업.
- 스트림: Checkpoint.
- 수행: `cargo test -p gputeer-checkpoint --test kill_chaos`(7) +
  `--features chaos-hooks` 대상 테스트 3회 연속(3/3) + `cargo test
  --workspace`(307)를 직접 실행해
  `docs/evidence/_raw/P0-03_v2_promotion_2026-08-18.txt` 에 저장했다.
  전체 재검수(`agent:codex-cli`, fresh-read-only, `p87` 프롬프트,
  원본 YAML claim·negative_tests·limitations 전부 대상) —
  `CHANGES_REQUESTED`.
- 2026-08-17 addendum이 이미 좁힌 claim 읽기("재개는 COMMITTED
  가 아니라 해시 유효 최고 체크포인트에서")는 재확인됐지만, **새
  지적**을 받았다 — "kill 시점 8개 고정값" limitation 이 stale
  하다. `crates/checkpoint/tests/kill_chaos.rs` 의 카오스 메커니즘
  자체가 이 evidence 를 쓴 시점(commit `45c1b43`) 이후 **시간
  기반에서 이벤트 기반으로 리팩터**됐다 — `[40,90,...,700]` 배열은
  여전히 있지만 그 값은 이제 실제 kill 시각이 아니라 `hard_timeout`
  상한(`ms*20`)일 뿐이고, 진짜 kill 은 stdout 에서 "COMMITTED" 를
  1회 관측한 직후 일어난다. 원본 raw_output 의 "총 8회: PARTIAL
  발생 7회(88%)" 표는 지금 이 구현이 재현하는 수치가 아니다 —
  결함은 아니다(다른 세션이 카오스 테스트의 부하 아래 재개 지점
  유실 문제를 잡으려고 의도적으로 바꾼 것), 다만 원래 evidence 의
  구체적 관측 통계는 지금 더 이상 유효하지 않다. 새
  addendum(2026-08-18 01:20 경)으로 이 변화를 정밀화하고, "손상
  없음"·"valid >= committed" 불변식은 지금도 매 kill 마다 검증됨을
  확인했다 — 원본 YAML 과 2026-08-17 addendum 원문(원본 raw_output
  표 포함)은 손대지 않았다. 좁은 후속 재검수(`p88` 프롬프트) —
  **`ACCEPTED`.**
- v2 frontmatter(순수 additive) 추가, `artifacts:` 에 raw/review
  파일 2개 추가. `_schema_v1_grandfathered.txt` 에서 P0-03
  제거(6→5건), `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **4 → 3건**(`P0-03a·07·08`). `cargo test
  --workspace` 전체 재실행 — 307 passed / 0 failed(회귀 없음),
  디스크 18GB 여유 안정적으로 유지 확인.
- 리포트: 이 이력 항목. 다음은 `P0-03a·07·08` — 이 셋만 남으면
  review-강제 대상 evidence 부채가 완전히 해소된다(`ENV-01·02` 는
  비강제라 별개).

---

## 2026-08-19 00:55 — P0-01 schema v1 → v2 승격 완료 — 하드웨어 재실측 없이, 디스크 100% 소진 사고 정리

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). `DoD-01`~`08`
  에 이어 `P0-*` 부채로 넘어간 첫 항목.
- 스트림: —.
- 수행: `P0-01` 은 원격 NVIDIA GPU 하드웨어(x600) 실측이라 `DoD`
  류에 쓴 절차("오늘 cargo test/python 재실행")를 그대로 쓸 수
  없었다 — 이 세션은 x600 에 SSH 로 접근할 자율 권한이 없고(원격
  시스템 접근은 세션 안전 정책이 막는 범주), 개발 기계에도 NVIDIA
  GPU 가 없다. **하드웨어 재실측을 지어내지 않고**, probe
  스크립트(`tools/probes/p0_01_windows_s1_cuda.py`)가 이전
  addendum(2026-08-18 01:30/01:40)이 인용한 파일:줄과 지금도
  일치하는지(소스 불변 확인)만으로 v2 승격 근거를 삼았다 — 그
  한계를 addendum 에 명시했다. 이 접근 자체의 타당성을 먼저
  Codex 에 검수시켰다(`p85` 프롬프트) — "재실측 없이 정직하게
  기록하는 접근은 수용 가능하나 실제 v2 frontmatter 필드가 아직
  없다"는 `CHANGES_REQUESTED`. frontmatter(`schema_version: 2` +
  executor/reviewer provenance)를 채우고 `artifacts:` 를 갱신한 뒤
  좁은 후속 재검수(`p86`) — 처음엔 receipt 파일 자체에
  `raw_output_digest`/`bytes` 가 빠져 있어 다시 `CHANGES_REQUESTED`,
  receipt 를 보완해 최종 **`ACCEPTED`.**
- `_schema_v1_grandfathered.txt` 에서 P0-01 제거(7→6건),
  `GRANDFATHER_DIGEST` 재계산·갱신.
- ★ **작업 도중 디스크가 100% 소진되는 사고가 있었다.** `cargo
  test --workspace` 가 `durability_chaos::adr026_write_once_succeeds_while_readers_hold_files_open`
  에서 실패했는데, 원인은 코드 회귀가 아니라 `rustc` 컴파일 중
  "디스크 공간이 부족합니다(os error 112)" — C: 드라이브가 223GB
  중 223GB 사용(가용 0)이었다. `cargo clean` 으로 target/ 6.2GiB
  를 정리해 19GB 여유를 확보했고, 그 뒤 `cargo test --workspace`
  가 다시 307 passed / 0 failed 로 통과함을 확인해 **코드 결함이
  아니었음을 확정**했다. 디스크 전체(사용자 홈 디렉터리만 약
  122GB — Documents 51GB·AppData 27GB·.cache 13GB·anaconda3
  12GB·VirtualBox VMs 6GB 등)의 근본 원인은 이 세션의 작업 범위
  밖이라 추가 정리는 하지 않았다 — 사용자가 깨어나면 직접 확인이
  필요하다.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **5 → 4건**(전부 `P0-*`). `cargo test
  --workspace` 전체 재실행(디스크 여유 확보 후) — 307 passed / 0
  failed(회귀 없음).
- 리포트: 이 이력 항목. 다음은 `P0-03·03a·07·08`.

---

## 2026-08-19 00:10 — DoD-08 schema v1 → v2 승격 완료 (여덟 번째, DoD 전체 완료) — write_once 경쟁 분기의 진짜 잔여 결함 발견·수정

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). `DoD-01`~`07`
  에 이은 여덟 번째이자 **DoD 문서 전체의 마지막** 승격.
- 스트림: Checkpoint · Protocol · Crypto.
- 수행: 같은 절차를 DoD-08 에 적용했다. Python self-test/verify +
  `cargo test -p gputeer-protocol/-checkpoint --test codex_findings`
  + `cargo test -p gputeer-crypto --test replay_binding` + `cargo
  test --workspace` + `cargo build --workspace --all-targets` 를
  직접 실행해 `docs/evidence/_raw/DoD-08_v2_promotion_2026-08-18.txt`
  에 저장했다. 전체 재검수(`agent:codex-cli`, fresh-read-only, `p83`
  프롬프트) — `CHANGES_REQUESTED`.
- **`DoD-02`·`DoD-06` 에 이은 세 번째 "재검수가 진짜 코드 결함을
  잡은" 사례다.** `write_once`(`crates/checkpoint/src/atomic.rs`)
  는 `final_path.exists()` 를 두 번 확인한다 — 함수 시작 시(K-1
  이 2026-08-16 에 이미 고침)와, tmp 파일을 쓴 뒤 다시 한번(경쟁
  분기). **두 번째는 여전히 내용 비교 없이 `Ok(false)` 를
  반환하고 있었다** — K-1 이 고쳐지기 전과 똑같은 결함이 다른
  코드 경로에 남아 있었다. 그 분기에서도 `final_path` 를 읽어
  대조하고 다르면 `ContentMismatch` 를 반환하도록 고쳤다.
  이 수정을 검증하려고 진짜 다중 스레드 동시 호출 테스트
  (`k1c_concurrent_same_name_writers_are_not_actually_safe`)를
  짜다가 **더 넓은, 더 근본적인 문제**를 발견했다 — 같은 이름에
  대한 동시 호출은 전부 같은 tmp 파일 이름을 공유해 근본적으로
  안전하지 않다(8스레드 동시 호출 중 3회 `Ok(true)` 관측, Windows
  `fs::rename` 이 기존 대상을 대체하는 시맨틱이라 발생). 이번엔
  고치지 않고 **결함을 고정하는 테스트**로만 등록했다 — 실제
  호출부(`writer.rs`)가 순차적 재시작 시나리오만 상정한다는 것을
  확인해 범위 축소가 정당함을 뒷받침했다. 새 addendum(2026-08-19
  00:00 경)으로 전부 기록 — 원본 frontmatter 와 이전 addendum
  원문은 손대지 않았다. 좁은 후속 재검수(`p84` 프롬프트) —
  **`ACCEPTED`.**
- v2 frontmatter(순수 additive) 추가, `artifacts:` 에 raw/review
  파일 2개 추가. `_schema_v1_grandfathered.txt` 에서 DoD-08
  제거(6→5건), `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **6 → 5건**(전부 `P0-*`). `cargo test
  --workspace` 전체 재실행(k1c 신설로 306→307) — 전 항목 0 failed
  (회귀 없음).
- 리포트: 이 이력 항목. **`DoD-01`~`DoD-08` 8건 전부 schema v2
  승격 완료** — 남은 부채 5건(`P0-01·03·03a·07·08`)만 같은 절차로
  이어가면 review-강제 대상 evidence 부채가 전부 해소된다.

---

## 2026-08-18 23:20 — DoD-07 schema v1 → v2 승격 완료 (일곱 번째) — coordinator/agent 신설로 stale 해진 limitation 5건 정정

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). `DoD-05` 에
  이어 나머지 v1 evidence 처리의 두 번째 항목.
- 스트림: Protocol · Crypto.
- 수행: 같은 절차를 DoD-07 에 적용했다. `cargo test -p
  gputeer-crypto --test lifetime_policy`(19) + `cargo test
  --workspace`(306) + `cargo build --workspace --all-targets`(경고
  0건)를 직접 실행해
  `docs/evidence/_raw/DoD-07_v2_promotion_2026-08-18.txt` 에 저장했다.
  전체 재검수(`agent:codex-cli`, fresh-read-only, `p80` 프롬프트) —
  `CHANGES_REQUESTED`.
- **이 evidence 는 domain_tag 개수를 직접 인용하지 않아 DoD-01~06
  의 stale 패턴과는 다른 원인으로 걸렸다** — 이 세션 중 새로 만든
  `crates/coordinator`·`crates/agent`(coordinator/agent 핸드셰이크)
  가 DoD-07 의 limitation 5건을 stale 하게 만들었다: (1) "단수명
  메시지는 ExecutionGrant·RenewLeaseRequest 둘뿐" → 이제
  `AgentGrantAck` 포함 셋, (2) "소비 측(Coordinator·Agent)이 없다"
  → 이제 존재하나 Evidence 6종을 아예 다루지 않는다, (3) replay
  limitation → CLI/crypto ingress 는 `DurableReplayGuard`, 새로
  생긴 coordinator/agent 는 `InMemoryReplayGuard`(DoD-04 승격 때
  정리한 구분과 같다), (4) keyring limitation → `PersistentKeyring`
  은 실재하고 coordinator/agent stub 만 InMemory 를 택했다, (5)
  negative_tests 테스트 파일이 14→19건으로 늘었다(manifest_hash
  관련 5건 추가, DoD-07 범위 밖이라 목록 미포함은 정상).
  새 addendum(2026-08-18 23:00 경)으로 5건을 전부 정정 — 원본
  frontmatter 와 이전 addendum 원문은 손대지 않았다. 좁은 후속
  재검수 1회차(`p81`) — 인용 1곳(`agent/src/lib.rs:111-123` →
  실제 전송까지 포함하려면 `:111-138`)만 지적. 고친 뒤 2회차
  재검수(`p82`) — **`ACCEPTED`.**
- v2 frontmatter(순수 additive) 추가, `artifacts:` 에 raw/review
  파일 2개 추가. `_schema_v1_grandfathered.txt` 에서 DoD-07
  제거(7→6건), `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **7 → 6건**. `cargo test --workspace` 전체
  재실행 — 전 항목 0 failed(회귀 없음).
- 리포트: 이 이력 항목. 다음은 `DoD-08`, `P0-01·03·03a·07·08`.

---

## 2026-08-18 22:40 — DoD-05 schema v1 → v2 승격 완료 (DoD-01·02·03·04·06 에 이은 여섯 번째) — 남은 참조 구현 공백 1건 발견

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). `DoD-03·04·06`
  사이클 종료 후 자율 루프가 다음으로 지정한 나머지 v1 evidence
  (`DoD-05·07·08`, `P0-01·03·03a·07·08`) 처리의 첫 항목.
- 스트림: Protocol.
- 수행: 같은 절차를 DoD-05 에 적용했다. `python
  reference_canonical.py --self-test`(12/12) + `--verify`(40/40) +
  `cargo test -p gputeer-protocol --test t1_signing_targets --test
  field_number_audit` + `cargo test --workspace`(306) + `cargo
  build --workspace --all-targets`(경고 0건) + `ControlAction` 21개
  arm 대 `to_fields.rs` 구현 직접 대조(9/21, 변화 없음)를 실행해
  `docs/evidence/_raw/DoD-05_v2_promotion_2026-08-18.txt` 에 저장했다.
  전체 재검수(`agent:codex-cli`, fresh-read-only, `p77` 프롬프트) —
  `CHANGES_REQUESTED`.
- **domain 수치가 여섯 번째로 stale 해진 패턴(23/19→24/20, Signable
  10→11종)에 더해, 새로운 종류의 지적을 받았다** — claim("참조
  구현과 바이트 단위로 일치한다")이 실제로는 `AgentGrantAck` 를
  검증하지 않는데도 그렇게 읽힐 여지가 있다는 지적이다. 확인해보니
  `tests/vectors/canonical_v1.json`(40건) 에 `AgentGrantAck` 벡터가
  **0건**이었다 — `GrantAck` 는 Python 참조 구현(`reference_canonical.py`
  의 `SCHEMAS`)에도 없다. `framed_ingress.rs` 가 서명·검증·dispatch
  는 확인하지만 **외부 Python 참조 구현과의 canonical/sig_input
  바이트 대조까지는 하지 않는다** — `AgentGrantAck` 의 참조 구현
  교차검증은 이 저장소 어디에도 없는 진짜 공백으로 남았다(코드
  결함이 아니라 **테스트 커버리지 공백** — 백로그에 등록).
  새 addendum(2026-08-18 22:20 경)으로 24/20·11종 실측치와 이
  claim 축소를 기록 — 원본 frontmatter 와 이전 addendum 원문은
  손대지 않았다. 좁은 후속 재검수 1회차(`p78`) — claim 축소 문구의
  구체적 숫자("벡터를 생성해 대조한 20종")가 부정확하다는 지적(40개
  벡터의 고유 message_type 은 14종이지 20이 아니다). 다시 고친 뒤
  2회차 재검수(`p79`) — **`ACCEPTED`.**
- v2 frontmatter(순수 additive) 추가 도중 `verify_evidence.py` 가
  `review_artifact`(`DoD-05_review.txt`) 의 file:line 인용이 저장소에
  하나도 없다며 **스키마 위반**을 잡았다 — receipt 를 요약 위주로
  쓰면서 구체적 `파일:줄` 인용을 충분히 박아 넣지 않은 내 실수였다
  (DoD-03/04/06 receipt 와 다른 형식). receipt 에 실제 검수
  결과(`canonical.rs:282-315` 등)의 파일:줄 인용을 추가해 재확인
  통과시켰다 — raw_output_artifact 파일 자체는 건드리지 않아
  digest 는 그대로 유효하다.
- `artifacts:` 에 raw/review 파일 2개 추가. `_schema_v1_grandfathered.txt`
  에서 DoD-05 제거(11→10건), `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **8 → 7건**. `cargo test --workspace` 전체
  재실행 — 전 항목 0 failed(회귀 없음).
- 리포트: 이 이력 항목. `AgentGrantAck` 의 Python 참조 구현 교차검증
  공백은 별도 후속 작업 후보로 CLAUDE.md 백로그에 기록한다. 다음은
  `DoD-07·08`, `P0-01·03·03a·07·08`.

---

## 2026-08-18 21:55 — DoD-06 schema v1 → v2 승격 완료 — DoD-03·04·06 3라운드 재검수 사이클 종료, 진짜 코드 결함 또 발견

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격) + 자율 루프가
  명시적으로 지정한 "DoD-03·04·06 3라운드 재검수" 사이클의 마지막
  항목. DoD-01·02·03·04 에 이은 다섯 번째 v2 승격.
- 스트림: Protocol.
- 수행: 같은 절차를 DoD-06 에 적용했다. `python
  reference_canonical.py --self-test`(12/12) + `--verify`(40/40) +
  `cargo test -p gputeer-protocol --test t1_signing_targets --test
  t1b_grant_and_control` + `cargo test --workspace`(306) + `cargo
  build --workspace --all-targets`(경고 0건)를 직접 실행해
  `docs/evidence/_raw/DoD-06_v2_promotion_2026-08-18.txt` 에 저장했다.
  전체 재검수(`agent:codex-cli`, fresh-read-only, `p75` 프롬프트) —
  `CHANGES_REQUESTED`.
- **DoD-01~04 와 같은 domain 수치 stale 패턴(19/23→20/24, Signable
  10→11종)에 더해, `DoD-02` 이후 두 번째로 진짜 코드 결함을 찾았다.**
  `crates/protocol/tests/t1b_grant_and_control.rs::all_domain_tags_are_distinct`
  가 `domain_coverage_is_explicit`(DoD-02 때 고친 것)와 똑같은
  구조적 결함을 갖고 있었다 — `Domain` enum 을 순회하지 않고 손으로
  쓴 23개 배열을 써서, `Domain::GrantAck` 의 tag 중복 여부를 **한
  번도 확인하지 않은 채** `assert_eq!(seen.len(), 23, ...)` 로 계속
  통과하고 있었다. 배열에 `GrantAck` 추가, assert 를 24로 갱신 —
  수정 전후 모두 테스트는 통과했다(회귀가 아니라 검사 범위 확장).
  새 addendum(2026-08-18 21:45 경)으로 20/24·11종 실측치와 이 코드
  수정을 기록했다 — 원본 frontmatter 와 이전 addendum 원문은
  손대지 않았다. 좁은 후속 재검수(`p76` 프롬프트) — **`ACCEPTED`.**
- v2 frontmatter(순수 additive) 추가, `artifacts:` 에 raw/review
  파일 2개 추가. `_schema_v1_grandfathered.txt` 에서 DoD-06
  제거(12→11건), `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **9 → 8건**. `cargo test --workspace` 전체
  재실행(코드 수정 반영) — 전 항목 0 failed(회귀 없음).
- 리포트: 이 이력 항목. **`DoD-03·04·06` 3라운드 재검수 사이클
  종료** — 셋 다 addendum ACCEPTED + schema v2 승격까지 완료. 자율
  루프의 다음 지시(CLAUDE.md 백로그)에 따라 나머지 v1 evidence
  (`DoD-05·07·08`, `P0-01·03·03a·07·08`)로 이어간다.

---

## 2026-08-18 21:10 — DoD-04 schema v1 → v2 승격 완료 (DoD-01·02·03 에 이은 네 번째)

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격), DoD-01·02·03 에
  이은 네 번째 사례. 자율 루프 계속(사용자 지시 — "테스트와 동시에
  개발할 수 있는 부분은 개발하면서 가야지" 의 연장, DoD-03·04·06
  3라운드 재검수 사이클의 다음 항목).
- 스트림: Protocol · Crypto.
- 수행: 같은 절차를 DoD-04 에 적용했다. `cargo test -p gputeer-crypto
  --test ed25519_verify`(20) + `-p gputeer-protocol --doc`(2) +
  `cargo test --workspace`(306) + `cargo build --workspace
  --all-targets`(경고 0건)를 직접 실행해
  `docs/evidence/_raw/DoD-04_v2_promotion_2026-08-18.txt` 에 저장했다.
  전체 재검수(`agent:codex-cli`, fresh-read-only, `p72` 프롬프트) —
  `CHANGES_REQUESTED`.
- **DoD-01~03 와 같은 패턴이 네 번째로 반복됐다** — 2026-08-17
  addendum 의 "Signable 구현 10종" 이 `Domain::GrantAck` 추가로
  11종이 됐다. 추가로 두 서술이 stale 했다: (a) "단수명 검증 경로가
  ExecutionGrant 로 한정된다" — 이제 `coordinator-agent-selftest`
  가 `AgentGrantAck` 도 매 실행 검증한다, (b) "아무도 replay 저장소를
  안 쓴다" — `gputeer selftest` 는 이미 `DurableReplayGuard` 를
  쓰고, **coordinator/agent handshake 만** 여전히
  `InMemoryReplayGuard` 다. 새 addendum(2026-08-18 21:00 경)으로
  세 가지를 다 정정했다 — 원본 frontmatter 와 2026-08-17 addendum
  원문은 손대지 않았다.
- 좁은 후속 재검수 1회차(`p73`) — 내용은 맞으나 새 addendum 의
  파일:줄 인용 4곳이 틀렸다는 지적(예: `coordinator/lib.rs:124-140`
  → 실제 `:135-150`). 지적대로 고친 뒤 2회차 재검수(`p74`) —
  **`ACCEPTED`.**
- v2 frontmatter(순수 additive) 추가, `artifacts:` 에 raw/review
  파일 2개 추가. `_schema_v1_grandfathered.txt` 에서 DoD-04
  제거(13→12건), `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **10 → 9건**. `cargo test --workspace` 전체
  재실행 — 전 항목 0 failed(회귀 없음).
- 리포트: 이 이력 항목. 다음은 계획대로 `DoD-06` — 이미 addendum 은
  1라운드 만에 `ACCEPTED` 를 받아 두었으므로 같은 v2 승격 절차만
  남았다. `DoD-03·04·06` 3라운드 재검수 사이클이 끝나면 CLAUDE.md
  백로그의 나머지 v1 evidence(DoD-05·07·08, P0-01·03·03a·07·08)로
  이어간다.

---

## 2026-08-18 20:15 — DoD-03 schema v1 → v2 승격 완료 (DoD-01·DoD-02 에 이은 세 번째)

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격), DoD-01·DoD-02 에
  이은 세 번째 사례. 사용자 지시 — "코덱스 검수 결과 확인해서 DoD-01
  승격 마무리해줘. 그리고 코덱스 쿼터로 다음 작업 이어서 가봐.
  테스트와 동시에 개발할 수 있는 부분은 개발하면서 가야지." 의
  연장(자율 루프, DoD-03·04·06 3라운드 재검수 사이클의 다음 항목).
- 스트림: Protocol.
- 수행: 같은 절차(오늘 재실행 + 오늘 새 독립 검수를 v2 근거로 삼는다)
  를 DoD-03 에 적용했다. `cargo test -p gputeer-protocol --test
  canonical_vectors --test field_number_audit --test prost_canonical`
  (50/50) + Python self-test(8/8) + 벡터 40개 교차 일치를 직접
  실행해 `docs/evidence/_raw/DoD-03_v2_promotion_2026-08-18.txt` 에
  저장했다. 전체 재검수(`agent:codex-cli`, fresh-read-only, `p70`
  프롬프트) — `CHANGES_REQUESTED`.
- **DoD-01·DoD-02 와 같은 패턴이 세 번째로 반복됐다** — `Domain::GrantAck`
  추가로 domain 수치가 다시 stale 해졌다. 다만 이번엔 코드 결함이
  아니라(`t1_signing_targets.rs` 는 DoD-02 승격 때 이미 고쳐져
  24종/20개를 정확히 보고하고 있었다) evidence 문서 수치만 stale
  했다 — frontmatter limitations 1번의 "17종 중 13종"과 2026-08-17
  addendum 의 "41개·23종·19종" 이 둘 다 낡아 있었다(실제: Domain
  24종, `ToCanonicalFields` 42개 선언, coverage 24종 중 20개 구현).
  새 addendum(2026-08-18 20:00 경)으로 실측치를 기록하고, coverage
  테스트가 여전히 손으로 쓴 배열이라 새 enum variant 를 자동으로
  못 잡는다는 한계를 새로 명시했다 — 원본 frontmatter 와 이전
  addendum 의 원문 수치는 append-only 원칙(`P0-07` 선례)에 따라
  손대지 않았다. 좁은 후속 재검수(`p71` 프롬프트) — **`ACCEPTED`.**
- v2 frontmatter(`schema_version: 2`, executor/reviewer 메타데이터,
  `review_context: fresh-read-only`, `review_outcome: ACCEPTED`,
  raw_output digest/bytes)를 추가(순수 additive, 기존 필드는
  안 건드림). `artifacts:` 에 `DoD-03_v2_promotion_2026-08-18.txt`·
  `DoD-03_review.txt` 추가. `docs/evidence/_schema_v1_grandfathered.txt`
  에서 DoD-03 제거(14→13건), `scripts/verify_evidence.py` 의
  `GRANDFATHER_DIGEST` 재계산·갱신.
- 검증: `python scripts/verify_evidence.py` — 스키마 위반 0, 독립
  검수 없는 PASS 부채 **11 → 10건**. `cargo test --workspace` 전체
  재실행 — 전 항목 0 failed(회귀 없음).
- 리포트: 이 이력 항목. 다음은 계획대로 `DoD-04`·`DoD-06` — 이미
  addendum 은 각 1라운드·1라운드 만에 `ACCEPTED` 를 받아 두었으므로
  같은 v2 승격 절차만 남았다.

---

## 2026-08-18 19:20 — Lease 최소 조각 코덱스 검수 `ACCEPTED`

- 계획: `0be82e8`(Lease 최소 조각)에 대한 코덱스 독립 검수(`p69`
  프롬프트, 1라운드).
- 스트림: —
- 결과: **`ACCEPTED`.** 검증 순서(서명 확인 → 상관관계 검사),
  nested Lease 위조가 outer Grant 서명을 안 깨는 이유(규칙 i —
  서명 필드 90 은 canonical 에서 제외되지만 메시지 자체는 포함),
  `FenceWatermark` 가 매 프로세스 새로 생성되고 계획의 "Out" 범위와
  일치하는지, replay guard 공유가 안전한지(`Lease` 는 `LongLived`
  라 replay 대상이 아님), 시나리오 5 의 Agent-only 판정 기준,
  뮤테이션 인과관계, `crates/agent` 가 `gputeer-runtime-policy` 를
  의존해도 스트림 소유권 위반이 아닌지 — 전부 파일:줄로 확인받았다.
  발견 사항 없음.
- 검증: 코덱스는 `cargo test`/selftest 재실행이나 뮤테이션 재현은
  하지 않았다(코드 대조로만 논리 검증) — 실행 기반 확인은 이
  세션이 앞서 이미 5회 연속 수행했다.
- 리포트: 이 이력 항목. Lease 최소 조각도 이제 구현·실측·독립
  검수까지 완전히 끝났다.

---

## 2026-08-18 19:00 — coordinator/agent 에 Lease 최소 조각 추가 (테스트+개발 병행)

- 계획: `docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md`
  (신규). 사용자 지시 — "코덱스 검수 결과 확인해서 DoD-01 승격
  마무리해줘. 그리고 코덱스 쿼터로 다음 작업 이어서 가봐. 테스트와
  동시에 개발할 수 있는 부분은 개발하면서 가야지." — DoD-02 v2
  승격(evidence 검수)과 이 기능 개발(코드)을 코덱스 두 인스턴스로
  병렬 진행했다.
- 스트림: Coordinator, Agent, CLI.
- 수행: 기존 coordinator/agent 최소 핸드셰이크 계획의 "Out" 절이
  예고해 둔 다음 한 걸음 — Coordinator 가 `ExecutionGrant.lease`
  에 서명된 `Lease` 를 채워 보내고, Agent 가 그것을 outer Grant 와
  **독립적으로** 검증해(§6 규칙 i) `fence_epoch` 를
  `crates/runtime-policy::FenceWatermark` 에 기록한다. 코덱스에게
  범위 후보 3개(Grant+Lease / Renew 왕복 / 다중 Agent)를 비교시켜
  가장 작은 것을 추천받았다(`p67` 프롬프트).
  - `crates/coordinator/src/lib.rs::issue_lease()` — `Lease` 를
    독립적으로 서명. `corrupt_lease_signature` 는 서명 **후**
    마지막 바이트를 뒤집는다(outer Grant 서명 계산에 nested
    서명이 안 들어가므로 outer 는 안 깨진다 — 정확히 이 성질을
    시험하기 위해서다).
  - `crates/agent/src/lib.rs::verify_and_record_lease()` — Grant
    replay 검사 통과 **후에만** 호출. `gputeer_protocol::verify()`
    로 nested Lease 를 독립 검증하고, 서명 검증이 끝난 뒤에만
    상관관계(attempt_id·issuing_coordinator_id·holder_node_id·
    job_id)를 검사한다. `gputeer-runtime-policy` 를 Agent 의 신규
    의존성으로 추가(`FenceWatermark` 사용, 계획 설계 당시 "확인
    안 됨"으로 남겼던 것을 실제로 추가해 보니 자연스러웠다).
  - `crates/cli/src/coordinator_agent_selftest.rs` — 시나리오
    5(위조 nested Lease 서명)·6(만료된 Lease) 추가, 6개 시나리오
    체제로 확장.
- **실제로 처음 실행에서 6개 시나리오 전부 한 번에 통과했다** —
  API 를 코드로 미리 하나하나 검증(`Lease` proto 필드,
  `Signable` impl, `verify()` 시그니처, `Ed25519Verifier::new()`,
  `FenceWatermark::check_and_advance()`)한 뒤 구현했기 때문으로
  보인다. 5회 연속 재실행 — 매번 통과.
- 뮤테이션 테스트로 비공허성 증명: `verify_and_record_lease()` 호출을
  `if false { }` 로 무력화 → 시나리오 5 가 정확히 예상대로 실패
  ("위조된 nested Lease 서명이 거부되지 않았다") → 원복 후 6개
  전부 재통과 확인.
- 검증: `cargo build --workspace` 경고 0. `cargo test --workspace`
  306/0/1(ignored) 유지(coordinator/agent 는 selftest 실행으로
  검증하지, 자체 단위 테스트를 아직 추가하지 않았다 — 기존
  패턴과 동일). `gputeer coordinator-agent-selftest` 5회 연속
  6/6 시나리오 통과.
- 리포트: 이 이력 항목 +
  `docs/plans/2026-08-18_1800_coordinator_agent_lease_최소_조각_v1.md`
  "실제 구현 메모" 절. `docs/evidence/` 정식 기록은 아직 남은 작업.
  다음 단계 후보(RenewLeaseRequest 왕복, 다중 Agent)는 계획서
  "Out" 절에 명시.

---

## 2026-08-18 18:20 — DoD-02 schema v1 → v2 승격 + 진짜 코드 결함 발견·수정

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격), DoD-01 에 이은
  두 번째 사례. 사용자 지시 — "코덱스 검수 결과 확인해서 DoD-01
  승격 마무리해줘. 그리고 코덱스 쿼터로 다음 작업 이어서 가봐.
  테스트와 동시에 개발할 수 있는 부분은 개발하면서 가야지."
- 스트림: Protocol.
- 수행: DoD-01 과 같은 절차(오늘 재실행 + 오늘 새 독립 검수를 v2
  근거로 삼는다)를 DoD-02 에 적용했다. `cargo test -p
  gputeer-protocol --test prost_canonical --test field_number_audit
  --test canonical_vectors`(50/50) 를 직접 실행해
  `docs/evidence/_raw/DoD-02_v2_promotion_2026-08-18.txt` 에 저장.
  전체 재검수(`agent:codex-cli`, fresh-read-only, `p66` 프롬프트) —
  `CHANGES_REQUESTED`.
- **이번엔 evidence 문서만의 문제가 아니라 진짜 코드 결함이었다.**
  `DoD-01` 과 같은 이유(같은 세션 안에서 `Domain::GrantAck` 추가)로
  domain 수치가 "23종 중 19종"에서 "24종 중 20종"으로 stale 됐는데,
  이번엔 그 stale 을 실제로 만든 원인이 코드 자체에 있었다 —
  `crates/protocol/tests/t1_signing_targets.rs::domain_coverage_is_explicit`
  가 `Domain` enum 을 순회하지 않고 **손으로 쓴 배열**을 쓴다.
  `assert_eq!(coverage.len(), 23, ...)` 는 그 배열 자신의 길이를
  셀 뿐이라, `Domain::GrantAck` 를 추가했을 때 이 배열을 갱신하지
  않아도 테스트가 계속 통과했다 — `canonical_vectors.rs` 의 같은
  종류 테스트는 실제 enum 값을 순회해 이런 결함이 구조적으로 불가능한
  것과 대조된다. `coverage` 배열에
  `(Domain::GrantAck, Some("AgentGrantAck"), true)` 를 추가하고
  `assert_eq!(coverage.len(), 24, ...)` /
  `assert_eq!(implemented, 20, ...)` 로 고쳤다.
  `cargo test -p gputeer-protocol --test t1_signing_targets
  domain_coverage_is_explicit` 직접 실행 확인: "domain 24종 — 구현
  20 · proto 메시지 없음 4". `cargo test --workspace` 재실행해
  회귀 없음 확인.
- addendum(2026-08-18 18:00)으로 이 결함과 수정 내용을 기록. 좁은
  후속 확인 재검수(`p68` 프롬프트) — **`ACCEPTED`**.
  `docs/evidence/_raw/DoD-02_review.txt` 작성 후 frontmatter 에
  `schema_version: 2` + v2 필드 추가(기존 필드는 손대지 않음),
  `artifacts:` 에 새 raw 파일 2개 추가, 그랜드파더 목록에서 DoD-02
  제거 + `GRANDFATHER_DIGEST` 갱신.
- 검증: `python scripts/verify_evidence.py` — DoD-02 PASS(스키마
  위반 0). 독립 검수 기록 없는 P0/DoD PASS 부채 **12건 → 11건**.
  `cargo test --workspace` 306/0/1(ignored) 유지(코드 결함 수정은
  기존 테스트를 고친 것이라 개수 불변).
- 리포트: 이 이력 항목 + `DoD-02_prost_연동_계층.md` 의 "schema v1
  → v2 승격" addendum. 다음 후보는 `DoD-03`.

---

## 2026-08-18 17:20 — DoD-01 schema v1 → v2 첫 승격 (사용자 승인 후 재개)

- 계획: CLAUDE.md 백로그 5번(v1→schema v2 실제 승격). 사용자가 채팅에서
  직접 승인("승인해줄게 코덱스쿼터로 계속 진행해")한 뒤, 시스템 설정
  변경(WSL2·방화벽)은 여전히 자율 실행 금지 대상임을 설명하고 대신
  이 항목으로 방향을 틀었다.
- 스트림: —
- 수행: **DoD-01 을 실제로 schema v2 로 승격한 첫 사례.** 착수 전
  확인한 것 — v1 evidence 16건 중 어느 것도 `_raw/` 에 독립 검수
  receipt 파일이 없었다(이번 세션의 40+ 회 addendum 재검수는 결과를
  문서 본문에만 요약했지 receipt 를 별도 저장하지 않았다). 그래서
  과거 시점의 executor/reviewer 메타데이터를 **지어내지 않고**,
  오늘 새로 실행한 재검증 + 새로 받은 독립 검수를 v2 의
  executor/reviewer 로 삼기로 했다.
  1. `cargo test -p gputeer-protocol --test canonical_vectors`(15/15)
     와 `python tools/canonical/reference_canonical.py --verify`
     를 직접 실행해 `docs/evidence/_raw/DoD-01_v2_promotion_2026-08-18.txt`
     에 저장(sha256 계산).
  2. DoD-01 전체를 처음부터 다시 검수시켰다(`agent:codex-cli`,
     fresh-read-only, `p64` 프롬프트) — `CHANGES_REQUESTED`.
     domain_tag 개수가 이 **같은 세션 안에서** `Domain::GrantAck`
     추가로 17→23→**24**로 또 stale 이 됐음을 잡았다(원본 정정도
     검수 시점엔 이미 낡아 있었다 — canonical evidence 의 근본적
     한계). `cargo test` 는 검수자의 read-only 샌드박스가
     `.cargo-build-lock` 접근 거부로 직접 실행 못 함.
  3. 새 addendum(2026-08-18 16:40)으로 domain 24 를 반영하고 직접
     실행한 test 결과를 첨부.
  4. **처음에는 frontmatter `negative_tests` 원문 문구를 직접 "17종"
     에서 "24종"으로 고쳤는데, append-only 원칙(관측 기록은 고치지
     않는다)과 이 문서 자신의 기존 관례(P0-07 의 `status` 만 유일한
     예외)를 어긴 것임을 스스로 발견해 즉시 원복했다** — 정정은
     addendum 본문에만 남기고 원문 "17종"은 그대로 뒀다.
  5. 좁은 후속 확인 재검수(`p65` 프롬프트) — **`ACCEPTED`**. 두
     지적 다 해소 확인.
  6. `docs/evidence/_raw/DoD-01_review.txt` 에 검수 receipt 작성(이
     저장소 관례대로 파일명:줄 인용, `DoD-09_review.txt` 형식 참고).
  7. frontmatter 에 `schema_version: 2` + `executor_*`/`reviewer_*`/
     `review_*`/`raw_output_artifact`/`digest`/`bytes` **추가**(기존
     필드는 손대지 않음), `artifacts:` 리스트에 새 raw 파일 2개
     **추가**(v2 스키마 자체가 요구하는 구조적 필수 사항이라 append-only
     예외로 취급).
  8. `docs/evidence/_schema_v1_grandfathered.txt` 에서 DoD-01 을 빼고
     "16건" → "15건"으로 갱신.
  9. `scripts/verify_evidence.py` 의 `GRANDFATHER_DIGEST` 상수를 새
     목록의 실제 sha256 로 갱신(RULE.md §7.3 이 설계한 대로 — 유예
     축소가 diff 에 드러난다).
- 검증: `python scripts/verify_evidence.py` — DoD-01 PASS(스키마
  위반 0), "독립 검수 기록이 없는 P0/DoD PASS" 부채가 **13건 → 12건**
  으로 줄었다(RULE.md §7.3: "줄어드는 것이 진전이다"). `cargo test
  --workspace` 306/0/1(ignored) 유지.
- 리포트: 이 이력 항목 + `DoD-01_canonical_encode_교차검증.md` 의
  "schema v1 → v2 승격" addendum. 남은 v1 evidence 12건(review-required)
  + ENV-01·02(review 비강제)는 이후 사이클에서 같은 절차로 이어간다.

---

## 2026-08-18 16:25 — `artifact_beneath` 재검수: 표현 정밀화 후 코덱스 `ACCEPTED` 흐름 마무리

- 계획: `86340ad`(미연결 primitive 명시 + 침묵 스킵 제거)에 대한
  코덱스 재검수(`p63` 프롬프트).
- 스트림: Runtime.
- 결과: `CHANGES_REQUESTED`(경미) — 두 원래 지적(모듈 문서 추가,
  `make_junction` 의 조용한 스킵 제거)은 실질적으로 고쳐졌다고
  확인했지만, 모듈 문서의 "`ArtifactPolicy::check()` 를 호출하는
  곳도 ... 하나뿐이다" 라는 문장이 부정확하다고 지적했다 — 이
  크레이트 자신의 테스트(`artifact_beneath.rs:60-63`)도 검증용으로
  그 함수를 부른다. "운영 코드에서 호출하는 곳은 selftest 뿐"으로
  한정해야 정확하다.
- 수행: `crates/runtime-windows/src/beneath.rs` 의 해당 문장에
  "**운영 코드에서**" 를 명시하고, 테스트 호출은 "실제 쓰기 경로"가
  아니라는 괄호 설명을 덧붙였다.
- 코덱스가 이번 라운드에서도 확인한 것(회귀 없음, junction 조용한
  스킵 제거 타당성, VRAM/artifact_scope 둘 다 미연결이라는 CLAUDE.md
  설명의 정확성)은 전부 문제없다고 재확인했다 — 남은 지적은 이
  표현 정밀화 하나뿐이었다.
- 검증: `cargo build --workspace` 경고 0. 문서 문자열만 바뀐 변경이라
  `cargo test --workspace` 재실행은 생략(코드 로직 변경 없음).
- 리포트: 이 이력 항목. 이로써 `open_beneath`/`open_artifact` 관련
  전체 사이클(구현 → 검수 → 미연결 사실 명시 → 표현 정밀화)이
  실질적으로 마무리됐다 — 남은 것은 실제 Job 실행 계층이 생겼을 때
  이 primitive 를 호출하도록 연결하는 것뿐이며, 그것은 이 작업의
  범위가 아니라 scheduler/agent Job 실행 자체가 생길 때의 일이다.

---

## 2026-08-18 16:10 — `artifact_beneath` 코덱스 검수: 미연결 primitive 명시 + 침묵 스킵 제거

- 계획: `07151e1`(artifact_scope TOCTOU 방어)에 대한 코덱스 독립
  검수(`p62` 프롬프트). 사용자 지시 — 자율 루프 계속.
- 스트림: Runtime.
- 결과: `CHANGES_REQUESTED`. 핵심 unsafe 코드(핸들 정리·
  `FILE_FLAG_OPEN_REPARSE_POINT` 사용법·방어 순서·junction 대체
  타당성·뮤테이션 인과관계·정상 경로 테스트 유의미성)는 전부
  문제없음을 확인받았지만, 두 가지를 지적했다:
  1. **[높음] `open_artifact()`/`open_beneath()` 가 실제 artifact 쓰기
     경로 어디에도 연결돼 있지 않다.** Explore 서브에이전트로 직접
     확인했다 — `ArtifactPolicy::check()` 호출부는
     `crates/cli/src/selftest.rs` 의 합성 문자열 검사(파일시스템
     안 건드림) 하나뿐이고, `crates/checkpoint` 의 실제 파일 쓰기는
     모두 내부 root 아래 고정 경로만 쓴다 — job/attempt 가 통제하는
     임의 경로에 쓰는 코드 자체가 이 저장소에 아직 없다(scheduler·
     `crates/agent` Job 실행이 CLAUDE.md 에 여전히 미착수로 남아
     있다). 즉 이건 이 커밋의 결함이 아니라 **`runtime-windows`
     신설 전 `windows_commit_cap()` 이 처했던 것과 똑같은 상황**
     (primitive 는 있지만 부를 caller 가 아직 없다) — 다만 그 사실을
     모듈 문서에 명시하지 않은 것은 실제 정정 대상이었다.
  2. **[중간] junction 생성 실패 시 테스트가 조용히 "통과"로 끝났다.**
     환경이 바뀌어 junction 생성이 막히면 이 방어 테스트가 **아무것도
     검증하지 않고도 초록불**을 켤 수 있었다.
- 수행: `crates/runtime-windows/src/beneath.rs` 모듈 문서에 "이 모듈은
  아직 아무 실제 쓰기 경로에도 연결되지 않았다" 절 추가 —
  `windows_commit_cap()` 의 선례와 명시적으로 비교해 정직하게
  적었다. `CLAUDE.md` 백로그 2번도 "VRAM·artifact_scope 완료" 를
  "primitive 완료"로 정정하고, 두 mechanism 모두 실제 Job 실행
  경로에 아직 연결 안 됐다는 설명을 추가했다(VRAM 도 같은 처지임을
  이번에 알아챘다 — 코덱스는 artifact_scope 만 지적했지만 VRAM 도
  똑같이 미연결이다). `artifact_beneath.rs` 의 `make_junction()` 이
  생성 실패 시 조용히 `return` 하던 것을 `assert!` 로 바꿔 — 실패하면
  테스트가 크게 panic 한다("조용한 거짓 통과보다 시끄러운 실패가
  낫다"). 이 개발 기계에서는 이미 junction 생성이 승격 없이 성공함을
  실측으로 확인했으므로, 실패는 "정상적으로 건너뛸 상황"이 아니라
  환경이 바뀐 이례적 상황이다.
- 검증: `cargo build --workspace` 경고 0. `cargo test --workspace`
  306/0/1(ignored) 유지(로직만 바뀌고 테스트 개수는 그대로) — 3개
  전부 재통과 확인.
- 리포트: 이 이력 항목 + `beneath.rs` 모듈 문서 + CLAUDE.md 백로그
  2번 갱신.

---

## 2026-08-18 15:45 — `artifact_scope` TOCTOU 방어 실제 구현 — Windows reparse point 차단

- 계획: CLAUDE.md "다음에 할 일" 2번 나머지 절반(artifact.rs TOCTOU
  강제). 사용자 지시 — 자율 루프 계속 + "코덱스 최대한 쿼터 써서".
- 스트림: Runtime.
- 수행: `crates/runtime-policy/src/artifact.rs` 모듈 문서가 적어 둔
  한계 — "진짜 강제는 ... Windows 재분석 지점 차단 핸들이 필요하다
  — 이 크레이트에는 없다" — 를 코덱스 설계(`p61` 프롬프트)를 따라
  `crates/runtime-windows` 에 구현했다.
  - `crates/runtime-windows/src/beneath.rs`(신규) — `open_beneath(root,
    relative)`: 경로 컴포넌트를 하나씩 `CreateFileW(FILE_FLAG_OPEN_REPARSE_POINT)`
    로 열어 각 구성요소(중간 디렉터리 포함)가 reparse point(symlink·
    junction·mount point) 인지 열기 시점에 직접 확인한다. `open_artifact()`
    는 `ArtifactPolicy::check()`(문자열 검사) 와 이 함수를 순서대로
    적용하는 편의 함수.
  - 코덱스가 명시한 한계를 그대로 반영: 일반 Win32 API 로는 Linux
    `openat2(RESOLVE_BENEATH|NO_SYMLINKS)` 와 동등한 원자적 보장이
    없다 — 컴포넌트 확인과 다음 컴포넌트 open 사이에 짧은 경합 창이
    남는다(`NtCreateFile` 의 `RootDirectory` 상대 open 으로 승격하면
    더 강해지지만 미구현).
- **실측 전 확인이 실제로 설계를 바꿨다.** 착수 전
  `New-Item -ItemType SymbolicLink` 를 이 개발 기계에서 직접 시도해
  "Administrator privilege required" 로 실패함을 확인했고,
  `New-Item -ItemType Junction` 은 승격 없이 성공함을 확인했다 —
  그래서 테스트는 symlink 대신 **junction** 으로 방어를 실측한다.
  junction 도 `FILE_ATTRIBUTE_REPARSE_POINT` 를 가지므로 방어 대상과
  정확히 일치한다.
  - `crates/runtime-windows/tests/artifact_beneath.rs` — 3개 테스트:
    (1) allowed 디렉터리 안 junction 이 허용 밖을 가리키면 최종
    컴포넌트에서 거부, (2) junction 이 **중간** 디렉터리인 경우도
    거부(다른 코드 경로 — 루프 안 검사 vs 마지막 컴포넌트 검사),
    (3) 정상 경로(reparse point 없음)는 실제로 열림(비공허성).
    (1)은 `ArtifactPolicy::check()` 가 이 경로를 문자열상 "허용"으로
    먼저 판정한다는 것까지 재확인해, "문자열 검사가 이미 다 걸렀다"
    는 우연을 배제한다.
- 뮤테이션 테스트로 비공허성 증명: reparse point 거부 조건을
  `if false && ...` 로 무력화 → junction 시나리오 2개 모두 예상대로
  실패("junction 을 통한 허용 영역 밖 접근이 open_beneath 를
  통과했다", "중간 디렉터리 junction 을 거부하지 못했다") → 원복 후
  3개 전부 재통과 확인.
- `crates/runtime-policy/src/artifact.rs` 모듈 문서에 이 연결을
  기록하되, "일반 Win32 API 만으로는 Linux 와 동등한 원자적 보장이
  없다"는 결론은 바꾸지 않았다 — 과장하지 않는다.
- 검증: `cargo build --workspace` 경고 0. `cargo test --workspace`
  306/0/1(ignored) — 이전 303 + `artifact_beneath.rs` 신규 3건.
- 리포트: 이 이력 항목 + `crates/runtime-windows/src/beneath.rs` 모듈
  문서 + `crates/runtime-policy/src/artifact.rs` 갱신.

---

## 2026-08-18 15:05 — `runtime-windows` 수정 재검수 `ACCEPTED`

- 계획: `23e77fd`(quote_command_line·TerminateProcess 수정)에 대한
  코덱스 재검수(`p60` 프롬프트).
- 스트림: —
- 결과: **`ACCEPTED`.** `2n+1`(따옴표 직전)·`2n`(문자열 끝) 백슬래시
  규칙이 코드에 정확히 구현됐는지 손으로 재계산해 확인, 회귀
  테스트 2개의 기대값 자체가 옳은지도(테스트 통과 자체가 아니라)
  검증, `TerminateProcess` 수정이 1차 오류 반환 흐름을 깨지 않았는지,
  이전 라운드에서 `ACCEPTED` 받은 부분(핸들 정리 순서·`CREATE_SUSPENDED`
  경합 제거·`wide()` 쓰기 가능 버퍼·`guarantees_hard_limit()` 정직성·
  evidence append-only)이 이번 수정으로 회귀하지 않았는지 전부
  확인받았다. 발견 사항 없음.
- **이로써 `crates/runtime-windows`(VRAM Job Object 커밋 상한 연결)
  는 구현·실측·코덱스 독립 검수(2라운드: 1차 CHANGES_REQUESTED →
  수정 → 2차 ACCEPTED)까지 완전히 끝났다.** CLAUDE.md 백로그 2번의
  VRAM 부분은 완료 — network.rs(OS 방화벽, 사용자 승인 필요)와
  artifact.rs(TOCTOU, 추가 조사 필요)만 남는다.
- 검증: 코덱스 자신은 Windows 환경이 아니라 실제 빌드/테스트 재실행은
  하지 않았다(코드 대조로만 검증) — 실행 기반 확인은 이 세션이
  앞서 이미 여러 차례 했다(단위 테스트 4/4, 통합 테스트 3회 연속).
- 리포트: 이 이력 항목.

---

## 2026-08-18 14:50 — `runtime-windows` 코덱스 검수: 명령줄 인용 버그 2건 + `TerminateProcess` 미확인 수정

- 계획: `cb2d7c2`(VRAM Job Object 연결)에 대한 코덱스 독립 검수(`p59`
  프롬프트). 사용자 지시 — 자율 루프 계속 + "코덱스 최대한 쿼터
  써서" 재확인.
- 스트림: Runtime.
- 결과: `CHANGES_REQUESTED`(1라운드). unsafe FFI 코드라 특히 꼼꼼히
  봐 달라고 요청했는데, 실제로 진짜 결함 2건(P1)과 과장 주석 1건(P2)
  을 잡았다:
  1. **`quote_command_line` 의 백슬래시 이스케이프가 MSVC 규칙과
     다르다.** 따옴표 직전 백슬래시는 `2n+1` 개가 맞는데 초안은
     `n+1` 개만 출력했다(부족하거나 과다). 문자열 끝(닫는 따옴표
     직전) 백슬래시는 `2n` 개가 맞는데 초안은 `n` 개만 출력했다 —
     예를 들어 `C:\Program Files\` 처럼 공백을 포함하고 백슬래시로
     끝나는 인자는 닫는 따옴표를 이스케이프해 명령줄이 깨진다.
  2. **`TerminateProcess` 반환값을 확인하지 않고 바로 핸들을 닫았다.**
     종료가 실제로 실패하면 정지 상태 프로세스가 영구히 남을 수
     있는데 그 사실을 아무도 몰랐다.
  3. (P2) `alloc_fixture.rs` 의 주석이 "첫 바이트를 건드려 페이지를
     물리적으로 커밋시킨다"고 과장했다 — `JOB_OBJECT_LIMIT_JOB_MEMORY`
     는 애초에 물리 RSS 가 아니라 virtual commit 총량을 본다.
  검증 순서(성공/실패 각 분기의 핸들 정리), `CREATE_SUSPENDED` 경합
  제거, `wide()`/`lpCommandLine` 의 쓰기 가능 버퍼 요구사항, 뮤테이션
  테스트의 타당성, `guarantees_hard_limit()` 정직성, evidence
  append-only 준수는 전부 문제없음을 확인받았다.
- 수행: `quote_command_line` 을 UTF-16 코드 유닛 위에서 직접 조립하도록
  다시 짰다(`OsStr::to_string_lossy()` 를 거치던 것도 비정상 서로게이트
  손상 위험이 있어 제거) — 따옴표 앞은 `2n+1`, 문자열 끝은 `2n` 규칙을
  정확히 구현했다. 회귀 테스트 4개 추가(`simple_args_are_not_quoted`,
  `trailing_backslash_before_closing_quote_is_doubled`,
  `backslash_before_embedded_quote_uses_2n_plus_1_rule`,
  `empty_arg_is_wrapped_in_quotes`) — 뒤 두 개가 정확히 코덱스가
  잡은 버그 패턴이다. `kill_and_close` 클로저가 이제
  `TerminateProcess` 반환값을 확인하고 실패 시 `eprintln!` 으로
  PID·오류를 남긴다(두 개의 `io::Error` 를 표준 방법으로 합칠 수
  없어 최소한 눈에 보이게는 만들었다). `alloc_fixture.rs` 주석을
  정정해 "virtual commit 상한을 재는 것이지 물리 메모리 압박이
  아니다"라고 정확히 적었다.
- 검증: `cargo build --workspace` 경고 0. `cargo test -p
  gputeer-runtime-windows --lib` 4/4 통과. `commit_cap.rs` 통합
  테스트 3회 연속 통과(제약된 자식·negative control 둘 다). `cargo
  test --workspace` 303/0/1(ignored) — 이전 299 + 신규 단위 테스트 4건.
- 리포트: 이 이력 항목 + `crates/runtime-windows/src/lib.rs` 코드
  주석(수정 사유 명시).

---

## 2026-08-18 14:15 — `crates/runtime-windows` 신설 — VRAM 판정을 실제 Job Object 로 연결

- 계획: CLAUDE.md "다음에 할 일" 2번(runtime-policy 판정을 실제
  시스템 호출로 연결). 사용자 지시 — 자율 루프 계속 + "코덱스 최대한
  쿼터 써서" 재확인.
- 스트림: Runtime.
- 수행: `crates/runtime-policy/src/vram.rs` 모듈 문서가 스스로
  "`runtime-windows` 가 생기면 그쪽이 이 판정을 부르고 실제로
  `CreateJobObject`/`SetInformationJobObject` 를 호출해야 한다"고
  적어 둔 것을 그대로 이행했다. 착수 전 안전 판단: 이 작업은 시스템
  전역 설정(방화벽 규칙·OS 기능 활성화)이 아니라 **프로세스/세션
  범위** Win32 API(Job Object)라 사용자 명시적 승인 없이 자율 실행
  가능하다고 판단 — 코덱스에게도 이 전제를 검토시켰다(`p58`
  프롬프트, "동의" 판정, 근거: 이름 없는 Job 은 시스템 전역에 영향
  없고 마지막 프로세스가 끝나면 사라진다).
  - `crates/runtime-windows/src/lib.rs` — `create_constrained_child`:
    `CreateProcessW(CREATE_SUSPENDED)` → `CreateJobObjectW` →
    `SetInformationJobObject(JobMemoryLimit)` →
    `AssignProcessToJobObject` → `ResumeThread` 순서로 자식을 만든다.
    `std::process::Command` 대신 `CreateProcessW` 를 직접 부른 이유:
    안정 Rust 에는 자식의 주 스레드를 나중에 재개할 공개 API가 없다
    (`ChildExt::main_thread_handle()` 은 nightly 전용) — `CREATE_SUSPENDED`
    로 "자식이 Job 할당 전에 이미 메모리를 커밋하는" 경합을 없앤다.
  - `crates/runtime-windows/src/bin/alloc_fixture.rs` — 실측 테스트용
    fixture. `VirtualAlloc(MEM_COMMIT)` 를 청크 단위로 반복해 실패할
    때까지 커밋하고 결과를 파일에 적는다(stdout 파이프 상속을 신뢰할
    수 없어서 파일로 뺐다).
  - `crates/runtime-windows/tests/commit_cap.rs` — 제약된 자식과
    negative control(제약 없는 자식)을 비교하는 실측 테스트 2개.
- **실측으로 발견한 것**: `JOB_OBJECT_LIMIT_JOB_MEMORY` 는 딱딱한
  상한이 아니라 **소프트** 제한이다 — `PeakJobMemoryUsed` 가
  `JobMemoryLimit` 을 5회 연속 측정 모두에서 약 700~850KiB 만큼
  넘었다(64MiB 상한 기준, 4MiB 청크 크기보다 작은 오버슈트라 "청크
  하나가 더 통과했다"가 아니다). 이것이 바로
  `VramEnforcement::guarantees_hard_limit()` 가 `WindowsCommitCap`
  에도 미리 `false` 를 못박아 둔 판단을 실측으로 재확인한 것이다.
  테스트는 이 실측 여유(2MiB 허용치)를 문서화해 반영했다.
- 뮤테이션 테스트로 비공허성 증명: `AssignProcessToJobObject` 호출을
  일시 무력화 → 제약된 자식이 안전 상한(4096MiB)까지 아무 제약 없이
  전부 할당(negative control 과 동일 거동) → selftest 가 정확히 이
  결함을 잡음(할당 실패 없음을 감지) → 원복 후 재통과 확인.
- `docs/evidence/P0-06_vram_enforcement.md` 에 append-only addendum
  추가 — 이 evidence 가 이미 적어 둔 limitation("runtime-policy 는
  판정만 하고 실제 Job Object 를 생성·설정하지 않는다")이 부분적으로
  해소됐음을 기록. x600 실제 GPU 하드웨어 재실측은 아니다(로컬
  개발 기계는 GPU 가 없다) — "RAM 커밋 상한이 VRAM 에도 적용된다"는
  이 evidence 의 핵심 발견은 여전히 x600 실측에만 근거한다는 점을
  명시했다.
- `crates/protocol/tests/stream_ownership.rs::every_crate_is_covered_by_ownership_rules`
  의 `KNOWN` 목록에 `runtime-windows` 추가(이전 세션들과 같은 패턴 —
  안전망이 새 크레이트를 실제로 잡았다).
- 검증: `cargo build --workspace` 경고 0. `cargo test --workspace`
  299/0/1(ignored) — 이전 297 + `commit_cap.rs` 신규 2건.
  `python scripts/verify_evidence.py` 스키마 위반 0.
- 리포트: 이 이력 항목 + `crates/runtime-windows/src/lib.rs` 모듈
  문서 + P0-06 evidence addendum.

---

## 2026-08-18 13:05 — x600 WSL2 시도 — 시스템 설정 변경이라 자율 실행 보류

- 계획: CLAUDE.md "다음에 할 일" 3번(D-3 — Linux 검증 환경 확보).
- 스트림: —
- 결과: `ssh x600 "wsl --status"` 실행 — "Windows Subsystem for
  Linux 가 설치되어 있지 않다. `wsl.exe --install` 로 설치하라"는
  응답 확인. `wsl -l -v` 도 동일.
- **자율 실행하지 않고 보류했다.** `wsl --install` 은 Windows 선택적
  기능(가상화 플랫폼 등)을 활성화하고 통상 재부팅을 요구하는
  시스템 설정 변경이다. 이 세션의 안전 규칙은 "시스템/보안 설정
  변경"을 자율 실행 금지 항목이 아니라 **채팅에서 명시적 승인 필요**
  항목으로 분류하며, 이 규칙은 "사용자가 자고 있으니 질문하지 말고
  계속 진행"이라는 표준 루프 지시보다 우선한다. 원격 x600 은 사용자의
  실제 GPU 워크스테이션이라 재부팅이 다른 작업을 방해할 수도 있다.
- 남은 것: 사용자가 깨어나면 (1) 직접 `wsl --install` 실행 후 재부팅,
  또는 (2) 이 세션에 명시적으로 승인. 승인 전까지 D-3 은 계속 미해소
  상태로 CLAUDE.md 에 정직하게 남겨 둔다.
- 검증: 해당 없음(실행하지 않음).
- 리포트: 이 이력 항목 + CLAUDE.md "다음에 할 일" 3번 갱신.

---

## 2026-08-18 13:00 — coordinator/agent 핸드셰이크 단계 5 코덱스 검수 `ACCEPTED` — 계획 완전 종료

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  단계 6, 단계 5 커밋(`d7aacdc`) 대상. 사용자 지시 — 자율 루프 계속.
- 스트림: —
- 결과: **`ACCEPTED`**(1라운드, `p57` 프롬프트). 위조가 서명 **후**에
  일어나는지(오염된 데이터에 서명한 것이 아니라 서명 필드만 변조한
  것인지), replay 시나리오가 `grant.encode_to_vec()` 을 두 번 부르지
  않고 동일 `frame` 바이트를 재사용하는지, `signing.rs:826` 인용이
  정확한지, 위조 ACK 시나리오가 Agent 쪽 결과를 판정에 안 쓰는
  이유가 코드 주석에도 정직하게 반영됐는지, 뮤테이션 테스트 주장이
  논리적으로 타당한지, `run_handshake` 리팩터링이 기존(`ffe8d45`
  검수 통과) PID 구분·RESULT 상관관계·stdout/stderr 파이프 교착
  방지 로직을 그대로 보존했는지 — 전부 파일:줄 단위로 대조해
  확인받았다. 결함 없음.
- **이로써 `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  의 단계 1~6 전부 구현·검증·독립 검수까지 완전히 끝났다.** 남은
  것은 계획 DoD 의 마지막 항목(`docs/evidence/` 에 schema v2 형식
  정식 기록)뿐이며, 그 항목은 "구현이 안 됐다"가 아니라 "이 저장소의
  증거 문서화 관례(RULE.md §7.3)를 아직 안 따랐다"는 뜻이다.
- 검증: 코덱스 자신은 `cargo test`/selftest 재실행이나 뮤테이션
  재현은 하지 않았다고 명시했다(코드 대조로만 논리 검증) — 실행
  기반 확인은 이 세션이 앞서 이미 5회 연속 수행했다.
- 리포트: 이 이력 항목 + 계획 문서 "단계 6" 절.

---

## 2026-08-18 12:40 — coordinator/agent 핸드셰이크 단계 5: 거부 경로 3종 완료 — 계획 전 단계 종료

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  단계 5(마지막 남은 단계). 사용자 지시 — 자율 루프 계속 + "코덱스
  최대한 쿼터 써서" 재확인.
- 스트림: Coordinator, Agent, CLI.
- 수행: 코덱스에게 "정직한 프로세스는 자기 서명을 위조 못 한다" 는
  문제의 설계를 다시 맡겼다(`p56` 프롬프트) — TCP proxy 로 진짜
  중간자 변조를 만드는 안보다 **stub 자체에 테스트 전용
  self-corruption 플래그를 넣는 안**을 권장받아 채택했다.
  - `CoordinatorConfig`/`AgentConfig` 에 `corrupt_own_signature: bool`
    — 서명 직후 마지막 바이트를 뒤집는다.
  - `CoordinatorConfig::send_grant_twice`/`AgentConfig::expect_replay`
    — 같은 wire bytes(재인코딩 없이 동일 `frame`)를 한 TCP 연결에
    두 번 써서 실제 replay 를 재현한다. `signing.rs:826` 을 직접
    읽어 `verify()` 가 `Duplicate` 를 만나면 `read_frame` 단계에서
    이미 `Err` 를 반환함을 확인하고 그 성질에 기대 설계했다.
  - `coordinator_agent_selftest.rs` 를 `Fixture`/`HandshakeOutcome`/
    `run_handshake()` 로 리팩터링해 4개 시나리오(정상·위조 Grant·
    위조 ACK·replay)를 공통 오케스트레이션으로 돌린다.
- **replay DoD 문구를 정직하게 정정**: 원래 "`DurableReplayGuard` 가
  거부한다" 였으나, 두 stub 은 `InMemoryReplayGuard` 를 쓴다(실행
  간 replay 상태 비공유) — 이 selftest 가 증명하는 것은 "같은
  프로세스·같은 guard 수명 안에서 동일 wire bytes 두 번째가
  거부되는가" 이지 "재시작을 넘는 replay 방어" 가 아니다. 후자는
  이미 별도로 `durable_replay_process.rs`(같은 날 앞선 작업)가
  증명했다. 과장하지 않고 DoD 문구를 "replay guard 가 거부한다"로
  고쳤다.
- 뮤테이션 테스트로 비공허성 증명(대표 1건): 위조 Grant 시나리오의
  `corrupt_own_signature` 처리를 `if false && ...` 로 무력화 →
  selftest 가 예상대로 실패("Agent 가 ACK 를 발급했다") → 원복 →
  4개 시나리오 전부 재통과 확인. 이 세션 내내 쓴 패턴(백업→뮤테이션
  →실패 확인→원복) 그대로.
- 검증: 5회 연속 `coordinator-agent-selftest` 실행 — 4개 시나리오
  전부 매번 통과, 총 실행 시간 ~1.2초(위조 Grant 시나리오에서 Agent
  가 검증 실패로 즉시 종료 → TCP 연결도 즉시 닫혀 Coordinator 의
  ACK 대기가 10초 타임아웃을 다 기다리지 않는다 — 당초 "느릴 수
  있다"는 우려를 실측으로 기각). `cargo test --workspace`
  297/0/1(ignored) 유지. `cargo build --workspace` 경고 0.
- **이로써 이 계획의 단계 1~6 전부 완료됐다.** 남은 것은 계획
  문서 자체가 명시한 DoD 마지막 항목(`docs/evidence/` 에 schema v2
  형식으로 정식 기록) 뿐이며, 이는 이 계획이 처음부터 "완전한
  coordinator/agent 가 아니다" 라고 명시한 범위(lease·스케줄링·
  다중 Agent·운영용 key protection·TLS 등)와는 무관하다.
- 리포트: 이 이력 항목 + 계획 문서 "단계 5 수행 메모" 절.

---

## 2026-08-18 11:55 — coordinator-agent-selftest 코덱스 검수: stderr 파이프 교착 위험 수정

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  단계 6(코덱스 독립 검수). 사용자 지시 — 자율 루프 계속.
- 스트림: CLI, Coordinator, Agent.
- 결과: 1라운드 `CHANGES_REQUESTED`. 코덱스가
  `crates/cli/src/coordinator_agent_selftest.rs` 를 지적했다 —
  coordinator 의 stderr 를 메인 흐름과 **동시에** 비우지 않아서,
  coordinator 가 OS 파이프 버퍼를 채울 만큼 stderr 에 쓰면(에러 메시지가
  길어지는 경우 등) coordinator 가 쓰기에서 블로킹되고, agent 는
  coordinator 의 TCP 응답을 기다리느라 블로킹되어 이 selftest 전체가
  교착할 수 있다는 지적 — 정상 경로에서는 coordinator 가 stderr 에
  아무것도 안 쓰므로 지금까지 5회 연속 실행에서는 드러나지 않았지만,
  구조적으로는 진짜 결함이었다. 검증 순서·키 배분·PID 검사·
  `InMemoryReplayGuard` 사용·`derive_nonce` 안전성은 전부 문제없음을
  코드 대조로 확인받았다.
- 수행: coordinator 의 stderr 를 `thread::spawn` 으로 만든 별도
  스레드가 처음부터 끝까지 비우도록 고쳤다(`read_to_string`), 메인
  흐름은 `.join()` 으로 나중에 결과를 받는다. stdout 은 READY/RESULT
  줄을 순서대로 읽어야 하므로 메인 스레드에 남겼다 — stdout·stderr
  를 분리한 이유가 서로 다르다(하나는 순서 의존, 하나는 그냥 비우면
  됨). 부가로 `derive_nonce` 의 "운영 코드가 이 패턴을 쓰면 안 되는
  이유" 경고를 `coordinator`·`agent` 양쪽에 대칭적으로 명시했다.
- 검증: `cargo build --workspace` 경고 0. 수정 후 5회 연속
  `coordinator-agent-selftest` 재실행 — 매번 성공, 매번 다른 PID 3개.
  `cargo test --workspace` 297/0/1(ignored) 유지.
- 리포트: 이 이력 항목 + 계획 문서 "단계 6" 절.

---

## 2026-08-18 11:20 — coordinator/agent 핸드셰이크 단계 3·4: 별도 프로세스 실제 handshake 성공

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md` 단계 3·4.
  사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지" (자율 루프 계속).
- 스트림: Coordinator(신규), Agent(신규), CLI.
- 수행: `crates/coordinator`·`crates/agent` 신설, `gputeer
  coordinator-stub`/`agent-stub`/`coordinator-agent-selftest` 배선.
  단계 1·2 검증 결과를 그대로 따라 `PersistentKeyring` 대신
  `InMemoryKeyring`(검증용) + 호출자가 직접 쥔 `SigningKey`(서명용)
  패턴을 썼다 — `selftest.rs:540` 이미 쓰는 패턴, 새 keyring API
  불필요. 두 stub 은 서로 다른 OS 프로세스이므로 키를 hex 인자로
  주고받는다(`--own-seed`/`--peer-pubkey`) — 실제 키 프로비저닝은
  범위 밖(계획 "Out" 절). 자세한 내용은 계획 문서 "단계 3·4 수행 메모".
- 실제로 `gputeer coordinator-agent-selftest` 를 실행해 보고서야 잡은
  결함: `coordinator.stdout.take()` 로 READY 줄을 읽은 뒤
  `wait_with_output()` 을 또 부르면 이미 소비된 stdout 핸들 때문에
  RESULT 줄이 조용히 빈 문자열이 된다. `wait()` + 직접 읽기로 고쳤다.
  고친 뒤 5회 연속 실행해 매번 서로 다른 PID 3개(자기 자신·
  coordinator·agent)로 성공을 확인 — 타이밍 경합 없음.
- 구현 중 계획서에 없던 안전망(`stream_ownership.rs::
  every_crate_is_covered_by_ownership_rules`)이 새 크레이트를 감지해
  걸렸다 — `docs/contracts/01_스트림_소유권.md`·`RULE.md` §4.1 에는
  Coordinator/Agent 자리가 이미 예약돼 있었지만 이 테스트의 `KNOWN`
  목록엔 없었다. 추가해 해소.
- 이 단계가 실제로 증명하는 것: Coordinator·Agent 가 진짜 별도 PID다 ·
  127.0.0.1 실제 TCP 연결이 성립한다 · Coordinator 가 canonical
  `ExecutionGrant` 를 서명한다 · Agent 가 `framed_ingress::read_frame`
  으로 Grant 를 검증하고 `require_replay_checked()` 를 통과한 뒤에만
  ACK 를 만든다 · Coordinator 가 ACK 를 검증하고 grant_id/attempt_id/
  agent_device_id 를 대조한다. **증명하지 않는 것**: 거부 경로(위조
  Grant·위조 ACK·replay — 단계 5), Job 실행·스케줄링·lease·다중 Agent·
  운영용 key protection(계획 "Out" 절 그대로).
- 검증: `cargo build --workspace` 경고 0. `cargo test --workspace`
  297/0/1(ignored) — 이전과 동일(coordinator/agent 크레이트에는 아직
  자체 단위 테스트가 없다. 검증은 `coordinator-agent-selftest` 5회
  연속 실행으로 했다). `gputeer coordinator-agent-selftest` exit 0.
- 리포트: 이 이력 항목 + 계획 문서 "단계 3·4 수행 메모" 절. 남은
  단계(5: 거부 경로 3종, 6: 코덱스 독립 검수)는 계획서에 남겨 뒀다.

---

## 2026-08-18 10:05 — coordinator/agent 핸드셰이크 단계 1·2: `AgentGrantAck` 서명 대상 메시지 + framed_ingress 배선

- 계획: `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md` 단계 1·2.
  사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지" (자율 루프 계속).
- 스트림: Protocol, Crypto.
- 수행: 계획서 자신이 요구한 "확인 안 됨" 3건부터 실측 검증(계획 문서의
  "단계 1·2 수행 메모" 절 참조 — 요약: `PersistentKeyring` 은 서명키를
  나중에 다시 꺼내는 API가 없지만 `selftest.rs:198` 의 기존 패턴(호출자가
  `SigningKey` 를 별도 보관)으로 충분함을 확인, 새 message 필드 번호
  충돌 없음을 빌드로 확인, `canonical_vectors.rs:304-324` 가 domain_tag
  개수를 하드코딩하는 정확한 위치를 특정). 그 다음 실제 구현:
  - `proto/control.proto` — `AgentGrantAck`(필드 1~8 + 서명 90) 추가
  - `crates/protocol/src/canonical.rs` — `Domain::GrantAck`
    (`gputeer/v1/grant-ack`) 추가. `ReplicaAck` 재사용 안 함 — Evidence
    lifetime 이라 replay 를 검사하지 않으므로 재사용하면 ACK replay
    방어를 증명할 수 없다(계획서 "왜 ReplicaAck 를 재사용하지 않는가").
  - `crates/protocol/src/to_fields.rs` — `ToCanonicalFields for
    pb::AgentGrantAck` (필드 1~8, nonce=7 포함 — 서명 밖이면 replay
    캐시 우회 가능)
  - `crates/protocol/src/signable.rs` — `Signable for pb::AgentGrantAck`
    (`Lifetime::ShortLived`, replay_nonce = Some(&self.nonce))
  - `crates/crypto/src/framed_ingress.rs` — `FrameType::GrantAck = 10`,
    `IngressMessage::GrantAck`, dispatch 배선
  - `crates/crypto/tests/framed_ingress.rs` — `grant_ack()` 헬퍼 +
    `normal_grant_ack_frame_dispatches_to_the_right_variant` round-trip
  - `docs/protocol/signing.md` §5 — domain_tag 표 23→24종 갱신
- 구현 중 계획서가 예상 못 한 안전망 4개가 순서대로 걸렸다 — 이 저장소가
  스스로 만들어 둔 회귀 방지 그물이 실제로 동작함을 보여준다:
  `field_number_audit.rs::every_impl_is_audited`,
  `lifetime_consistency.rs::every_signable_is_covered`,
  `canonical_vectors.rs::domain_tags_are_32_bytes_and_unique`,
  `schema_fingerprint.rs::proto_schema_matches_recorded_fingerprint`(P0-08
  스키마 진화 가드 — `UPDATE_SCHEMA_FINGERPRINT=1` 로 정당하게 갱신. 필드
  추가가 아니라 **새 메시지 추가**라 schema_version 상향은 불필요).
- 검증: `cargo build --workspace` 성공. `cargo test -p gputeer-protocol`
  · `cargo test -p gputeer-crypto` 각각 전부 green. `cargo test
  --workspace` 297/0/1(ignored) — 이전 296 + GrantAck round-trip 1건.
  ★ `gputeer-checkpoint::write_failure::
  concurrent_startup_gc_treats_not_found_as_normal_race` 가 병렬 실행
  중 1회 우연히 실패 → `--test-threads=1` 단독 재실행 시 통과 확인 →
  이 작업과 무관한 기존 테스트의 타이밍 취약성으로 판단, 별도 조사
  과제로 남긴다(원인은 조사하지 않았다 — 추측하지 않는다).
- 리포트: 이 이력 항목 + 계획 문서 자체("단계 1·2 수행 메모" 절).
  남은 단계(3~6: coordinator/agent crate 신설·CLI 배선·거부 경로·
  독립 검수)는 계획서에 남긴 대로 별도 작업.

---

## 2026-08-18 09:10 — `DurableReplayGuard` 별도 프로세스 replay 경쟁 실측 추가

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). CLAUDE.md "다음에 할 일" 백로그 항목 — 별도
  **프로세스** replay 경쟁은 지금까지 스레드로만 측정했다는 공백.
- 스트림: 저장소 신뢰성 실측 (`crates/crypto`).
- 수행: 코덱스에게 설계를 시켰다(스크래치패드 `p53.md`) —
  `crates/crypto/tests/durable_replay_race.rs` 가 같은 프로세스
  내 스레드로만 경쟁을 만들던 공백을 지적하고, 별도 OS 프로세스로
  경쟁을 강제하는 fixture+테스트 구조를 설계받았다. 그 설계대로:
  - `crates/crypto/src/bin/durable_replay_process_fixture.rs`
    (신규) — `worker`/`holder` 두 서브커맨드. `holder` 는 별도
    `rusqlite::Connection` 으로 `BEGIN IMMEDIATE` 를 잡고, 모든
    worker 가 `check_and_record` 호출 직전 마커 파일을 남길 때까지
    기다린 뒤에만(파일 마커 barrier) 락을 놓거나(정상 경로) 1300ms
    쥐고 있다가(LockTimeout 경로, `BUSY_TIMEOUT`=1000ms 초과) 푼다.
  - `crates/crypto/tests/durable_replay_process.rs` (신규) — 8개
    worker 프로세스를 스폰해 (1) 같은 nonce → 정확히 1개만 Fresh,
    나머지 Duplicate, (2) 서로 다른 nonce → 전부 Fresh(비공허성),
    (3) holder 가 1300ms 락을 쥐면 → Duplicate 로 위장되지 않고
    LockTimeout 을 받는다, 3가지를 검증. 모든 worker 의
    `start_ns` 가 holder 의 `locked_ns`~`released_ns` 구간
    안이었는지 타임스탬프로 재확인해 "우연히 안 겹쳤을 수도
    있다"는 반례를 차단한다.
- 검증(뮤테이션 테스트로 비공허성 증명): `durable_replay.rs` 의
  중복 검사(`if duplicate.is_some() { ... Duplicate }`)를
  `if false && duplicate.is_some()` 로 무력화 → 예상대로
  `separate_processes_same_nonce_have_exactly_one_fresh` 가
  실패(`Duplicate` 개수 0, 기대 7) → 즉시 `.bak` 백업에서 원복 →
  재빌드 후 3개 테스트 재통과 확인. `cargo test --workspace`
  296/0/0(이전 293 + 신규 3), 스키마 위반 0.
- 리포트: 이 이력 항목. 별도 evidence 문서는 만들지 않았다 — 이
  테스트는 RULE.md §7.3 이 요구하는 "evidence 파일"이 아니라
  일반 회귀 테스트이며, Phase 3 `chaos-hooks` kill 테스트와 같은
  선례를 따라 코덱스 독립 리뷰는 선택 사항으로 남겨 뒀다.

---

## 2026-08-18 08:30 — ENV-02 도 ACCEPTED — 이 저장소의 v1 evidence 16건 전부 addendum 재검수 완료

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: —
- 결과: **`ACCEPTED`.** `:158` 이 정확히 D-1 문장을 가리킴을
  확인했고, 앞선 두 라운드 지적이 전부 해소됐다고 확인했다.
- **★ 이로써 이 저장소의 v1 evidence 16건(review-required 14건 +
  ENV-01·02) 전부가 addendum 독립 재검수 `ACCEPTED` 를 받았다.**
  `python scripts/verify_evidence.py` 는 17/18 을 PASS 로 계상하고
  (P0-06 은 FAIL-SCOPE 로 원래도 PASS 가 아니다), 스키마 위반은
  0건이다.
- 이 사이클 전체에서 반복적으로 확인된 것: (1) 코덱스는 정말
  형식적 승인을 하지 않고 매번 파일:줄을 직접 열어 검증했다 —
  라운드당 평균 2~4회. (2) 구현자(이 세션) 스스로도 정정하다가
  새 오류를 만든 사례가 여러 번 있었고(oneof 구현 개수 과장,
  HISTORY.md 줄 번호 자연 붕괴 2회, 통계량 계산 오류, 인용 줄
  번호 오류 다수) 전부 다음 라운드가 잡았다 — **재검수 사이클
  자체가 스스로를 검증하는 도구로 기능했다.**
- 남은 것: v1 → schema v2 실제 승격(frontmatter 전체 교체·정식
  executor/reviewer 메타데이터·raw_output digest)은 여전히 별도
  작업이다. addendum ACCEPTED 는 "정정 내용이 맞다"는 뜻이지
  "이 evidence 가 schema v2 다"가 아니다.
- 검증: `scripts/verify_evidence.py` 스키마 위반 0. `cargo test
  --workspace` 293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 08:20 — ENV-01 ACCEPTED, ENV-02 인용 오류 1건 더 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: —
- 결과: **`ENV-01` `ACCEPTED`.** `cargo.exe` 직접 경로 실행과 PATH
  미검출을 둘 다 재현해 확인했다. `ENV-02` 는 `CHANGES_REQUESTED`
  — 코덱스 샌드박스는 네트워크가 막혀 x600 접속을 재현하지 못했고
  (문서가 이미 그 사실을 명시), 남은 결함은 D-1 문장 인용이
  `:60`(엉뚱한 decision 필드)을 가리킨 것 — 실제는 `:158`
  (limitations 목록)이었다. 고쳤다.
- 검증: `scripts/verify_evidence.py` 재확인. `cargo test --workspace`
  293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 08:10 — ENV-01·ENV-02 재검수: "Rust 없음" 이 둘 다 stale — PATH 미등록과 미설치 혼동

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). review-required 대상은 아니지만 완결성을 위해
  남은 v1 evidence 2건 착수.
- 스트림: —(환경 기록)
- 결과: 둘 다 `CHANGES_REQUESTED` — 같은 패턴의 stale 이었다.
  둘 다 "Rust 툴체인이 없다"고 적었는데, 실제로는 **PATH 에
  없을 뿐 `.cargo\bin` 에 설치되어 있었다.** 이 개발 기계는
  `C:\Users\playdata2\.cargo\bin` 에, x600 은
  `C:\Users\<x600-user>\.cargo\bin` 에 각각 cargo 1.97.1 이 실재함을
  직접 확인했다 — 코덱스의 read-only 샌드박스는 네트워크가
  막혀 x600 재접속을 못 했지만, 이 세션은 이미 써 온 SSH 접속으로
  직접 재확인했다. D-1("Rust 설치를 사용자 결정으로 상신") 결정도
  둘 다 stale — 설치는 이미 되어 있었고, 남은 문제는 PATH
  등록이다. ENV-02 는 GPU/드라이버 스펙(RTX 4070 SUPER, driver
  595.79)도 다시 재서 evidence 기록과 일치함을 재확인했다.
  base64 전송 방식 limitation 도 그 뒤 `scp` 로 바뀌어 stale —
  이 세션의 P0-07 재실측이 실제로 `scp` 를 썼다.
- 검증: `scripts/verify_evidence.py` 재확인. `cargo test --workspace`
  293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 08:00 — coordinator/agent 최소 핸드셰이크 계획서 작성

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). evidence 재검수 사이클이 끝난 뒤 다음 작업
  단위로 CLAUDE.md 가 반복해서 지적한 가장 큰 공백(coordinator·
  agent 미착수)에 착수했다.
- 스트림: — (계획 문서, 아직 코드 없음)
- 수행: 코덱스(read-only)에게 "완전한 coordinator/agent 가 아니라
  다음 한 걸음만" 설계하도록 요청했다. 결과를
  `docs/plans/2026-08-18_0800_coordinator_agent_최소_핸드셰이크_v1.md`
  로 정리했다 — `gputeer coordinator-stub`/`agent-stub` 을
  `Command::current_exe()` 로 별도 PID 로 띄우고, 전용
  `AgentGrantAck` 서명 메시지(`ReplicaAck` 재사용 불가 — Evidence
  lifetime 이라 replay 검사가 없다)로 signed Grant 왕복 +
  위조/replay 거부를 증명하는 최소 설계다.
- ★ **이 계획은 아직 구현하지 않았다.** 새 crate(`crates/coordinator`,
  `crates/agent`) 신설과 proto 스키마 변경(`AgentGrantAck` 추가,
  domain_tag 23→24)은 이번 세션의 다른 작업들(문서 정정·기존
  코드에 대한 검증)보다 훨씬 큰 아키텍처 결정이라, 사용자가 깨어난
  뒤 방향을 확인받는 것이 맞다고 판단해 계획 문서로만 남겼다.
  설계 자체가 스스로 "확인 안 됨"이라 표시한 3가지(PersistentKeyring
  서명 핸들 API, AgentGrantAck 컴파일 여부, domain 개수 하드코딩
  갱신 필요)도 계획 문서에 그대로 옮겼다.
- 검증: 문서만 추가, 코드 변경 없음. `cargo test --workspace`
  293/0/0(불변).
- 리포트: 이 이력 항목

---

## 2026-08-18 07:55 — P0-07 재실측 addendum ACCEPTED (4라운드) — 이번 세션의 evidence 작업 마무리

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Runtime
- 결과: **`ACCEPTED`.** 제목·통계 수치를 PowerShell 로 독립
  재계산해 전부 일치함을 확인했고, `verify_evidence.py` 로
  `status: PASS`·스키마 위반 0 을 재확인했다.
- 이로써 P0-07 재실측 작업(x600 SSH 원격 실행 → 통계 정정 3라운드
  → 최종 ACCEPTED)이 끝났다. 4라운드에 걸쳐 이 addendum 자체에서
  스스로 만든 오류 3건을 순서대로 잡았다: (1) "재확인"과 "불일치
  해소"의 혼동, (2) 통계량 계산 오류(평균 대비 편차와 최솟값-최댓값
  상대차를 섞어 씀), (3) 절 제목이 본문 결론과 모순.
- ★ 이번 세션의 v1 evidence 재검수 작업은 여기서 일단락한다.
  `RULE.md` §7.3 review-required 14건 전부 addendum ACCEPTED,
  그 중 P0-07 은 실제 GPU 재실측까지 거쳐 원래 상태(PASS)로
  돌아왔다. 남은 것: `ENV-01`·`02`(review-required 아님, 미착수),
  v1 → schema v2 실제 승격(frontmatter 전체 교체) 작업.
- 검증: `scripts/verify_evidence.py` 스키마 위반 0. `cargo test
  --workspace` 293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 07:45 — P0-07 재실측 addendum 3라운드: 절 제목이 본문과 모순됐다

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Runtime
- 결과: `CHANGES_REQUESTED`. 통계 수치(평균 0.020167, 편차 +3.1%·
  −8.8%·+5.6%, 최솟값 대비 최댓값 15.76%)는 재검수가 직접 재계산해
  전부 정확하다고 확인했다. 다만 절 제목 "★ 재실측으로 해소"가
  본문 자신의 정직한 결론("원래 불일치의 원인은 해소되지 않았다")
  과 모순된다는 것을 지적받았다 — "재실측으로 claim 재확인 (원래
  불일치는 미해결)"로 고쳤다.
- 검증: `scripts/verify_evidence.py` 재확인. `cargo test --workspace`
  293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 07:35 — P0-07 재실측 addendum 2라운드: 통계량 자체도 잘못 계산했었다

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Runtime
- 결과: `CHANGES_REQUESTED`. 앞선 라운드의 핵심 지적(claim 재확인
  vs 원래 불일치 해소 구분, "정상 변동" 단정 제거)은 해소됐다고
  확인됐지만, 그 정정문 안에 있던 **수치 자체가 또 틀렸다** —
  "평균 대비 편차 14~16%"라고 썼는데, 재검수가 PowerShell 로 직접
  계산해 보니 실제 평균 대비 편차는 +3.1%·−8.8%·+5.6% 이고,
  "15.76%"는 최솟값 대비 최댓값의 상대 차이였다(평균 대비 편차가
  아니다) — 서로 다른 두 통계량을 섞어 쓴 것이었다. 정확한 값으로
  고쳤다.
- ★ 같은 addendum 안에서 "근거 없는 통계적 단정을 고친다"고 써
  놓고 그 정정문 자체에 또 다른 통계 계산 오류를 넣은 것 — 이
  세션 전체에서 반복된 패턴("정정하다가 새 오류를 만든다")의
  가장 미묘한 사례다.
- 검증: `scripts/verify_evidence.py` 재확인. `cargo test --workspace`
  293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 07:20 — P0-07 재실측 addendum 정정: "재확인" 과 "불일치 해소" 를 혼동했었다

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 방금 만든 재실측 addendum 도 곧바로 재검수에
  맡겼다.
- 스트림: Runtime
- 결과: `CHANGES_REQUESTED`. "세 σ 값이 15% 이내로 근접하고 정상적인
  실행 간 변동으로 설명 가능하다"는 문장이 **통계적 근거 없는
  단정**이었다 — 사전 정의된 허용 변동 범위가 이 evidence 에 없었고,
  "15% 이내"라는 표현도 기준(평균? 최솟값?)이 없어 값에 따라
  성립하기도 안 하기도 했다. 더 근본적으로 "재실측이 원래 불일치를
  해소했다"와 "재실측이 claim 을 재확인했다"를 하나로 뭉뚱그렸다 —
  후자만 참이고, 원래 두 기록이 왜 서로 달랐는지는 여전히 확인
  안 됨으로 남아야 했다. 이 구분을 명시하고 근거 없는 "정상 변동"
  단정을 제거했다 — `_raw/P0-07_probe_2026-08-18_rerun.txt` 에
  붙였던 같은 분석문도 raw 파일에서 빼고 .md 쪽 addendum 으로
  옮겼다(raw 파일은 원문만 남기는 것이 이 저장소의 관례다).
  `status: PASS` 자체(claim 재확인 근거)는 유지한다 — 재검수도
  그 점을 문제 삼지 않았다.
- 검증: `scripts/verify_evidence.py` 로 재확인. `cargo test
  --workspace` 293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 06:48 — P0-07 실제 재실측: x600 GPU 로 σ 재확인, status 를 PASS 로 복원

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 재검수 사이클이 끝난 뒤, 미뤄뒀던 실제 재측정을
  실행했다.
- 스트림: Runtime
- 수행: `~/.ssh/config` 의 `x600` 접속이 살아 있고 GPU(RTX 4070
  SUPER)·torch(2.13.0+cu126, CUDA)가 그대로 있음을 확인했다.
  `tools/probes/p0_07_runtime_estimation.py` 를 그대로 scp 로 복사해
  인자 없이(원래 RUN1 과 동일 설정) 실행했다.
- 결과: **σ=0.0213** (평균 1.9%, 최대 4.7%, DoD PASS). 기존 두 상충
  기록(evidence 본문 σ=0.0208, `_raw` 원문 σ=0.0184)과 비교하면
  세 값 모두 15% 이내로 근접하고 전부 DoD 를 여유 있게 통과한다 —
  정상적인 실행 간 변동으로 설명 가능하며 조작·계산 오류의 증거는
  없다. 원래 불일치의 정확한 원인(전사 오류 등)은 여전히 특정할
  수 없지만, **claim 자체는 독립적인 세 번째 실행으로 재확인됐다.**
- 조치: 새 raw artifact
  `docs/evidence/_raw/P0-07_probe_2026-08-18_rerun.txt` 로 원문을
  저장했다. **frontmatter `status` 를 `INCONCLUSIVE` 에서 다시
  `PASS` 로 정정했다** — 판정 불가 상태를 만들었던 근거 부재가
  실제 재측정으로 해소됐기 때문이다. `INCONCLUSIVE` 로 낮췄던
  판단 자체는 그 시점엔 옳았다(근거 없이 PASS 를 유지할 수
  없었다) — 지금은 근거가 생겼을 뿐이다.
- ★ 이 재실측·status 복원은 아직 독립 재검수를 거치지 않았다 —
  다음에 이 문서를 다루는 세션이 검수해야 한다.
- 검증: `scripts/verify_evidence.py` 로 `status: PASS` 재확인
  (파싱 오류 없음). `cargo test --workspace` 293/0/0(코드 변경 없음).
- 리포트: 이 이력 항목

---

## 2026-08-18 02:30 — P0-06·P0-07 도 ACCEPTED — RULE.md §7.3 review-required v1 evidence 전체(14건) 재검수 완료

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프). 이 사이클의 마지막 2건.
- 스트림: Runtime
- 결과: **둘 다 `ACCEPTED`.** `python scripts/verify_evidence.py --json`
  exit 0 과 PyYAML `safe_load` 로 `P0-07` 의 `status='INCONCLUSIVE'`
  파싱을 재검수가 직접 재현해 확인했다. "관측값을 고친 것이 아니라
  판정만 바꾼 것이므로 evidence 철학과 충돌하지 않는다."
- **★ 이로써 `RULE.md` §7.3 review-required v1 evidence(14건:
  `DoD-01`~`08`, `P0-01`·`03`·`03a`·`06`·`07`·`08`) 전부가 독립
  재검수 `ACCEPTED` 를 받았다.** 라운드 수 합계 34회(문서당
  1~4라운드). 그 과정에서:
  - 실질적 stale limitation·claim 범위 초과·negative_tests 이름
    오류를 수십 건 찾아 고쳤다.
  - **frontmatter 를 실제로 고친 것은 `P0-07` 의 `status` 필드
    단 하나** — 나머지는 전부 append-only 절로 처리했다.
  - 구현자(이 세션) 스스로 정정하다가 새 오류를 만든 사례가
    최소 3번 있었다(`ControlAction` oneof 구현 개수 과장,
    `HISTORY.md` 줄 번호 자연 붕괴 2회, "frontmatter 에 반영했다"
    는 거짓 문장) — 전부 다음 라운드가 잡았다.
  - `P0-01b`·`P0-07b` 등 "후속 스파이크로 분리한다"고 decision 에
    적었지만 실제로는 한 번도 실행되지 않은 약속이 최소 2건 있었다.
  - `docs/history/HISTORY.md` 처럼 계속 자라는 append-only 파일에
    줄 번호로 인용하면 그 인용이 세션 안에서도 저절로 틀려진다는
    것을 배웠다 — 이후 제목 기반 인용으로 전환했다.
  - **v1 → schema v2 승격(frontmatter 전체 교체·정식
    executor/reviewer 메타데이터·raw_output digest)은 여전히
    별도 작업으로 남아 있다** — addendum ACCEPTED 는 "정정 내용이
    맞다"는 뜻이지 "이 evidence 가 schema v2 다"가 아니다.
    `verify_evidence.py` 는 지금도 이 14건을 v1 로 계상한다.
  - `ENV-01`·`02` 는 `RULE.md` §7.3 의 강제 대상이 아니라 이번
    사이클에서 다루지 않았다.
- 검증: `scripts/verify_evidence.py` 스키마 위반 0. `cargo test
  --workspace` 293/0/0.
- 리포트: 이 이력 항목

---

## 2026-08-18 02:15 — P0-07 의 status 를 PASS → INCONCLUSIVE 로 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 2라운드 재검수 반영.
- 스트림: Runtime
- 결과: 둘 다 여전히 `CHANGES_REQUESTED` — 이번 라운드는 더
  근본적인 지적이었다.
  - **P0-07**: 재검수가 raw_output 불일치 표 자체는 정확하다고
    확인했지만, "두 수치 다 DoD 통과이니 PASS 유지"라는 판단이
    `RULE.md` §7.1 기준으로 틀렸다고 지적했다 — `INCONCLUSIVE`
    ("측정은 했으나 판정 불가")가 정확히 이 상황을 위한 상태값이다.
    ★ **`status` 필드를 `PASS` 에서 `INCONCLUSIVE` 로 정정했다** —
    이번 v1-evidence 재검수 사이클 전체에서 frontmatter 를 실제로
    고친 유일한 경우다. `claim`·`raw_output`·`decision` 등 나머지는
    당시 기록 그대로 보존했다 — `status` 는 관측이 아니라 그
    관측에 대한 판정이므로, 판정 근거(raw_output 신뢰성)가
    무너지면 판정도 정정하는 것이 옳다고 판단했다. YAML 안에
    inline 주석(`#`)을 달았다가 `verify_evidence.py` 의 최소
    파서가 그것까지 값으로 먹어버릴 뻔한 것을 스스로 잡아
    수정했다 — 설명은 frontmatter 밖(본문 addendum)으로 옮겼다.
    `scripts/verify_evidence.py` 로 재확인: PASS 계상 17→16건,
    파싱 오류 없음.
  - **P0-06**: "frontmatter claim 에도 반영했다"는 문장이 거짓이었다
    (재검수가 지적 — 실제로는 addendum 의 해석일 뿐 `claim` 필드는
    안 고쳤다). "이 addendum 이 명시적으로 해석해 보여주는 것"으로
    정정했다.
- 검증: `scripts/verify_evidence.py` 로 두 evidence 파일의 frontmatter
  가 여전히 유효 파싱됨을 확인. `cargo test --workspace` 293/0/0
  재확인(코드 변경 없음).
- 리포트: 이 이력 항목

---

## 2026-08-18 02:00 — P0-06·P0-07 1라운드 정정 — P0-07 에서 raw_output/artifact 수치 불일치 발견

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). `RULE.md` §7.3 review-required v1 evidence 의
  **마지막 2건**.
- 스트림: Runtime
- 결과: 둘 다 `CHANGES_REQUESTED`.
  - P0-06: claim("VRAM quota 강제 수단이 없다")이 evidence 본문
    자신이 찾은 사실(Windows Job Object 의 간접 총 커밋 상한)과
    표면적으로 충돌하는 것처럼 읽혔다 — "정밀한 hard VRAM quota
    는 없지만 코스한 간접 제한은 가능하다"로 명시했다.
    `runtime-policy` 크레이트가 VRAM 판정을 분류하지만 실제
    Job Object 를 생성·설정하지는 않는다는 limitation 을 추가했다.
  - P0-07: ★ **claim 범위 문제보다 심각한 것을 찾았다** — 이
    문서의 frontmatter `raw_output` 요약과 링크된
    `docs/evidence/_raw/P0-07_probe.txt` 원문의 **숫자가 서로
    다르다**(σ=0.0208 vs 0.0184, RUN2 최대오차 9.3% vs 6.1% 등).
    둘 다 DoD(σ<=0.20) 는 통과해 최종 판정(PASS)은 안 바뀌지만,
    이 문서에 적힌 구체적 수치를 그대로 신뢰할 수 없다는 뜻이다.
    재실측 없이는 어느 쪽이 맞는지 판별할 수 없어 **불일치 사실
    자체를 정직하게 기록**했다 — 임의로 하나를 골라 조용히
    통일하지 않았다. `P0-07b`(노드 간 외삽) 후속 검증도
    `P0-01b` 와 마찬가지로 저장소에 실제로 존재하지 않는다는
    것을 확인했다.
  - 둘 다 원본 YAML 은 당시 기록이므로 고치지 않고 append-only
    로 정정했다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:50 — P0-03a 도 3라운드 만에 ACCEPTED — v1 evidence 12건 addendum ACCEPTED 누적

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Checkpoint
- 결과: **`ACCEPTED`.** `write_failure.rs:5-20` 이 전부 모듈 문서
  주석일 뿐임을 재확인했고, `durability_chaos.rs:371-384` 가 공유
  모드와 무관하게 설계됨을, `write_failure.rs:52-66` 의 지금 실패
  주입이 디렉터리 rename 방식임을 재확인했다.
- 누적: 지금까지 `DoD-01`~`08`(8건 전부)·`P0-01`·`P0-03`·`P0-03a`·
  `P0-08` **12건**의 addendum 이 독립 재검수 `ACCEPTED` 를 받았다.
  남은 v1 evidence 는 `P0-06`·`P0-07` **2건**뿐이다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:40 — P0-01 ACCEPTED, P0-03a 는 근거 과장 1건 더 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Runtime · Checkpoint
- 결과: **`P0-01` `ACCEPTED`.** `P0-03a` 는 여전히
  `CHANGES_REQUESTED` — `FILE_SHARE_DELETE` 재확인 근거를 과장했다:
  "`durability_chaos.rs`/`write_failure.rs` 가 재확인 테스트"라고
  적었는데, 실제로는 `write_failure.rs` 모듈 문서에 **서술로만**
  남아 있고 독립 검증 테스트는 없었다. "서술로만 기록, 재검증
  테스트 없음"으로 좁혔다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:30 — P0-01·P0-03a 1라운드 정정 (P0-01 은 GPU 하드웨어라 재실측 불가)

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). `DoD-` 8건이 모두 끝나 `P0-` 계열로 넘어갔다.
- 스트림: Runtime · Checkpoint
- 결과: 둘 다 `CHANGES_REQUESTED`.
  - P0-01: ★ 이 evidence 는 실제 NVIDIA GPU 하드웨어 실측이라
    개발 기계(Intel Iris Xe)에서는 재실측할 수 없다 — 그 사실을
    명시하고 코드·문서의 내적 일관성만 확인했다. claim 을
    "`DISABLE_MAX_PRIVILEGE` 토큰 + 단일 CUDA 프로세스" 범위로
    좁혔다(관리자 SID 비활성·프로세스 트리는 미시험). negative_tests
    분류 정정(과거 오판 서술을 재실행 가능한 테스트처럼 나열했던
    항목 1건). **결정문이 약속한 P0-01b 후속 검증이 실제로는
    존재하지 않는다**는 것도 확인해 명시했다.
  - P0-03a: claim 을 "원래 절차가 Windows 에서 그대로 성립한다"가
    아니라 "원래 절차 실패를 실측하고 ADR-026 수정안을 도출했다"
    로 명시했다. stale limitation 1건 — "Rust std::fs 공유 모드
    별도 확인 필요"가 그 뒤 실제로 확인됐다(FILE_SHARE_DELETE 포함
    — 그 발견 경위 자체가 흥미롭다: 옛 실패 주입 테스트가 반대
    가정에 기대다가 스스로 넣은 "성공하면 무효" 단언에 걸려 잡혔다).
  - 둘 다 원본 YAML 은 당시 기록이므로 고치지 않고 append-only 로
    정정했다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:20 — DoD-01·P0-08 둘 다 3라운드 만에 ACCEPTED — v1 evidence 10건 addendum ACCEPTED 누적

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Protocol
- 결과: **둘 다 `ACCEPTED`.** `canonical_v1.json:7` 이 실제 `vectors`
  배열 시작임과 40건을 재확인했고, `HISTORY.md` 제목 인용도 실제
  내용과 일치함을 확인했다. `q6` 이 sig_input·canonical 결속을 각각
  직접 단언하는 두 assert 를 확인했다.
- 누적: 지금까지 `DoD-01`·`DoD-02`·`DoD-03`·`DoD-04`·`DoD-05`·`DoD-06`·
  `DoD-07`·`DoD-08`·`P0-03`·`P0-08` **10건**의 addendum 이 독립
  재검수 `ACCEPTED` 를 받았다 — **`DoD-` 접두사 evidence 는 이로써
  전부(8/8) 완료됐다.** 재확인해 보니 남은 것은 `P0-01`·`P0-03a`·
  `P0-06`·`P0-07` **4건**이다(`ENV-01`·`02` 는 `RULE.md` §7.3 의
  독립 검수 강제 대상 접두사 `DoD-`/`P0-` 에 포함되지 않는다 —
  CLAUDE.md 의 이전 "v1 13건" 수치가 이 구분을 정확히 반영하지
  않고 있었다는 것도 이번에 재확인하며 발견했다).
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:10 — DoD-01·P0-08 2라운드: 인용 정밀도 오류 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Protocol
- 결과: 둘 다 여전히 `CHANGES_REQUESTED` — 이번엔 사실관계가 아니라
  **인용의 정밀도** 문제였다.
  - DoD-01: 벡터 40건 인용이 `canonical_v1.json:2`(spec 선언)를
    가리켜 실제로 개수를 입증하지 못했다 — `:7`(vectors 배열 시작)
    로 고쳤다. `HISTORY.md` 제목 인용도 틀렸다 — "09:30 — prost
    연동 계층" 에는 12→20 기록이 없고, 실제로는 "10:40 — 서명 밖
    필드 6건 제거" 항목이었다.
  - P0-08: `schema_version` 의 canonical 결속 근거가 `canonical.rs:350,360`
    (sig_input 설명일 뿐)을 가리켰다 — 실제로는
    `schema_evolution.rs:249,254-256` 의 `q6` 이 canonical·sig_input
    결속을 **둘 다** 직접 단언한다. q6 을 q4·q5 와 같은 범위로
    뭉뚱그렸는데 q6 은 canonical 비교까지 포함해 범위가 더 넓었다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 01:00 — DoD-01·P0-08 1라운드 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 이 저장소에서 가장 오래된 evidence(DoD-01)와
  P0-08 착수.
- 스트림: Protocol
- 결과: 둘 다 `CHANGES_REQUESTED`.
  - DoD-01: claim("두 구현이 모든 범위에서 바이트 단위로 일치")이
    넓게 읽혔다 — `canonical_vectors.rs` 가 40개 벡터 전체가 아니라
    수동 구성한 부분집합만 순회한다고 좁혔다. domain 수치 17→23
    정정, stale limitation 4건(JobManifest 부분집합·prost 미구현·
    Ed25519 미검증·SCHEMA_TOO_NEW 미구현 — 전부 그 뒤 해소됨) 정정.
    이 evidence 당시 12건이던 벡터와 지금 40건을 명시적으로
    구분했다.
  - P0-08: claim 핵심은 유지. "SCHEMA_TOO_NEW 반환 경로 미구현"
    limitation 이 stale(지금 구현되어 있다). "지문 가드 뮤테이션"
    항목이 named test 가 아니라 수동 뮤테이션 실험이라는 분류
    정정. q4·q5·q6 이 실제 Ed25519 가 아니라 sig_input 비교라는
    범위 명시.
  - 둘 다 원본 YAML 은 당시 기록이므로 고치지 않고 append-only 로
    정정했다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:50 — DoD-07 도 2라운드 만에 ACCEPTED — v1 evidence 8건 addendum ACCEPTED 누적

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Crypto
- 결과: **`ACCEPTED`.** vectors 40건·`evidence_has_no_replay_defense_and_says_so`
  함수명·`DurableReplayGuard`/`PersistentKeyring` 인용을 전부 직접
  열어 재확인했다. `HISTORY.md` 인용도 제목 기반으로 바뀌어 문제
  없었다.
- 누적: 지금까지 `DoD-02`·`DoD-03`·`DoD-04`·`DoD-05`·`DoD-06`·`DoD-07`·
  `DoD-08`·`P0-03` **8건**의 addendum 이 독립 재검수 `ACCEPTED` 를
  받았다. 나머지 v1 evidence 는 **5건**.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:40 — DoD-08 1라운드 만에 ACCEPTED, DoD-07 1라운드 정정

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). 다음 v1 evidence 2건 착수.
- 스트림: Protocol · Crypto
- 결과: `DoD-08`(독립검수 시정)이 **1라운드 만에 `ACCEPTED`** — 이
  사이클에서 첫 즉시 통과 사례다. `DoD-07`(시각정책 실메시지)은
  `CHANGES_REQUESTED`: vectors 36→40 정정, negative_tests 이름 오류
  1건(`evidence_is_not_replay_checked` → 실제
  `evidence_has_no_replay_defense_and_says_so`), stale limitation
  2건(§10 replay·키 관리 — 둘 다 그 뒤 실제로 구현된 것을 "미구현"
  으로 남겨둔 채였다) 정정.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:30 — DoD-02 도 4라운드 만에 ACCEPTED — v1 evidence 6건 재검수 사이클 종료

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). DoD-02 의 마지막 남은 지적(HISTORY.md 인용
  방식)을 고친 버전을 네 번째로 재검수했다.
- 스트림: Protocol
- 결과: **`ACCEPTED`.** 제목 기반 인용으로 `HISTORY.md` 항목을
  다시 찾아 필드 편입 내용을 확인했고, 이 문서에 그동안 쌓인 모든
  정정(claim 범위·`ControlAction` 9/21·`Lease` 대조 인용·
  negative_tests 이름)을 처음부터 다시 훑어 "추가 수정 사항은
  확인되지 않았다"로 마무리했다.
- **이로써 이번 v1-evidence 재검수 사이클 `DoD-02`·`DoD-03`·`DoD-04`·
  `DoD-05`·`DoD-06`·`P0-03` 6건 전부가 addendum 독립 재검수
  `ACCEPTED` 를 받았다.** 라운드 수: `DoD-02`·`DoD-03` 각 4회,
  `P0-03` 2회, `DoD-04`·`DoD-05`·`DoD-06` 각 3회. 이 과정에서 확인된
  것: (1) 코덱스는 형식적 승인을 하지 않고 매번 file:line 을 직접
  열어 검증했다. (2) 구현자(이 세션) 스스로도 정정하다가 두 번
  새 오류를 만들었다 — `ControlAction` oneof 구현 개수 과장(9/21을
  "전부 구현"으로 잘못 정정)과 `HISTORY.md` 줄 번호 인용의 자연
  붕괴(append-at-top 파일에 줄 번호를 인용하면 세션이 진행될수록
  스스로 틀려진다) — 둘 다 재검수가 잡았다.
- **문서 전체를 schema v2 로 승격하는 것은 여전히 별도 작업**이다
  (frontmatter 교체 · 정식 executor/reviewer 메타데이터 ·
  raw_output digest). addendum ACCEPTED 와 혼동하지 않는다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:20 — DoD-05 ACCEPTED, DoD-02 는 HISTORY.md 줄 번호 불안정성 문제로 4라운드째

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Protocol
- 결과: **`DoD-05` `ACCEPTED`.** `DoD-02` 는 여전히
  `CHANGES_REQUESTED` — 그런데 이번엔 **이 세션 스스로가 만든
  구조적 문제**가 원인이었다: 직전 라운드에서 `docs/history/HISTORY.md:629-645`
  로 정확히 인용했는데, 그 사이 이 세션이 `HISTORY.md` 에 새 항목을
  더 추가하면서(최신 항목을 맨 위에 쌓는 append 방식) 대상 항목이
  `:651-667` 로 밀려나 인용이 **다시 틀린 상태**가 됐다. 줄 번호를
  또 맞추는 대신 **제목 기반 인용**으로 바꿨다 — append-only 로
  계속 자라는 파일에 줄 번호 인용을 쓰는 것 자체가 이 재검수
  사이클에서 두 번째로 문제를 일으켰다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-18 00:10 — DoD-02·DoD-05 2라운드 재검수 — 자체 만든 과장 하나를 발견

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속).
- 스트림: Protocol
- 결과: 둘 다 여전히 `CHANGES_REQUESTED`. 가장 중요한 지적은 **직전
  라운드에서 구현자 자신이 새로 만든 과장**이었다: `ControlAction`
  의 oneof 하위 메시지 21개 중 9개만 `ToCanonicalFields` 가 구현되어
  있는데, "각각 구현되어 있다"고 적었다. 재검수가 직접 세어 지적했고
  grep 으로 재확인했다(나머지 12개 = 0건). ★ 원래 evidence 의
  limitation("개별 구현이 필요하다")이 이 정정보다 오히려 더 정확한
  상태였다 — 정정하다가 새 과장을 만든 사례. "9/21 구현, 12개
  미구현"으로 다시 고쳤다.
- 그 외: `Lease` 전 필드 바이트 대조 테스트 인용 누락
  (`prost_canonical.rs:700-735` 추가), `HISTORY.md` 인용에 줄 번호
  없음(`:629-645` 추가), DoD-05 의 벡터 증가 이력 주장을 "40건
  이라는 사실만 확인됨, 증가 과정은 확인 안 됨"으로 낮췄다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-17 23:55 — 다음 v1 evidence 2건(DoD-02·DoD-05) 재검수 착수

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속, dynamic 모드). 앞선 4건 사이클이 끝나 다음 v1
  evidence 로 넘어갔다.
- 스트림: Protocol
- 수행: 코덱스(read-only) 1개 태스크로 `DoD-02`(prost 연동 계층)와
  `DoD-05`(T1 서명대상 확장)를 함께 재검수시켰다. 둘 다 첫 라운드에서
  `CHANGES_REQUESTED`.
- 발견과 조치:
  - DoD-02: claim("Python 참조 구현과 바이트 단위로 일치한다")이
    전수 비교가 있는 것처럼 넓게 읽혔다 — 실제 바이트 대조는
    `JobManifest` 중심이고 나머지는 필드 번호·이름 감사뿐이라고
    좁혔다. negative_tests 이름 오류(`common_` → 실제
    `common_message_field_numbers_match_proto`) 1건, stale
    limitation 5건(필드 6개 미구현·17종 중 7종·oneof 없음·SCHEMA_TOO_NEW
    미구현·Ed25519 미구현 — 전부 코드가 이미 해소했거나 범위를
    좁혀야 함) 정정.
  - DoD-05: claim 의 "17종 중 9종" 자체가 stale — 지금은 §5 domain
    이 23종이고 그 중 19종이 구현되어 있다(ADR-028 이 17→23으로
    늘렸다). negative_tests 이름 오류 2건(`renew_lease_request`,
    `revoke_lease_notice` — 실제로는 `_matches_reference` 접미사가
    붙는다), stale limitation 5건(6종 Signable 미구현·domain tag
    공유·ControlAction oneof 서술·17종 중 9종·Ed25519 미구현) 정정.
    vectors 메타데이터도 28건에서 지금 40건으로 정정.
  - 두 문서 모두 원본 YAML(claim/status/limitations)은 당시 기록이므로
    고치지 않고 append-only "이후 변경" 절로 정정했다. **재검수는
    아직 진행 중** — 다음 라운드 대상.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-17 23:45 — DoD-03 도 4라운드 만에 ACCEPTED — evidence 4건 재검수 사이클 종료

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프 계속). DoD-03 의 마지막 남은 지적(인용 모호함)을 고친
  버전을 네 번째로 재검수했다.
- 스트림: Protocol
- 결과: **`ACCEPTED`.** `field_number_audit.rs:249/259/269`(번호-이름
  대조)와 `:282`(`every_impl_is_audited`)의 구분이 정확한지, claim
  범위·vectors 40건·Ed25519 등 구분까지 문서 전체를 다시 훑어 "확인
  안 됨: 없음" 으로 마무리했다.
- **이로써 이번 v1-evidence 재검수 사이클(`DoD-03`·`DoD-04`·`DoD-06`·
  `P0-03`) 4건 모두 addendum 이 `ACCEPTED` 를 받았다** — `DoD-03` 은
  4라운드, `P0-03` 은 2라운드, `DoD-04`·`DoD-06` 은 3라운드 만에.
  문서 전체를 schema v2(frontmatter 승격 · executor/reviewer 정식
  메타데이터)로 올리는 것은 여전히 별도 작업으로 남아 있다 — addendum
  ACCEPTED 와 frontmatter v2 승격을 혼동하지 않는다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-17 23:35 — v1 evidence 3라운드 재검수 완료 — DoD-04·DoD-06 ACCEPTED, DoD-03 만 잔류

- 계획: 사용자 지시 — "코덱스 시켜서 작업 계속 하라고 나 일어날때까지"
  (자율 루프, 코덱스 위주). 직전 라운드에서 남은 인용 오류를 고친
  버전을 세 번째로 재검수했다.
- 스트림: Protocol
- 결과: **`DoD-04`·`DoD-06` 이 이번 라운드에서 `ACCEPTED`.** 인용
  교정(각각 K2/Linux limitation 인용, vectors 40건 대조)이 정확했다고
  확인됐다. `DoD-03` 은 세 라운드째 `CHANGES_REQUESTED` — 이번엔
  실질적 결함이 아니라 **표현의 모호함**이었다: "claim 을 좁혀 읽는다"
  절의 `field_number_audit.rs:249-274` 인용과 "그 외 확인" 절의
  `field_number_audit.rs:282` 인용이 나란히 있어 **같은 결함이 또
  남은 것처럼 읽혔다** — 실제로는 둘 다 진짜인, 서로 다른 함수
  (`common_message_field_numbers_match_proto` 류 vs
  `every_impl_is_audited`) 를 가리키고 있었다. 함수명을 명시해
  구분했다.
- 세 evidence 모두 문서 전체를 schema v2 로 승격하는 것은 별도
  판단이 필요하다고 검수자가 남겼다 — frontmatter 가 여전히 v1 이고,
  ACCEPTED 는 이번 addendum(정정 절)에 한정된다.
- 검증: 문서 전용 수정, `cargo test --workspace` 293/0/0 재확인.
- 리포트: 이 이력 항목

---

## 2026-08-17 23:20 — v1 evidence 4건 재검수 완료 라운드 — P0-03 addendum 최초 ACCEPTED

- 계획: 사용자 지시 — "코덱스로 다음 작업들 진행해" 의 연장. 직전 정정본
  (DoD-03·04·06, P0-03)이 실제로 지적을 해소했는지 코덱스(read-only)에게
  최종 재검수를 맡겼다.
- 스트림: Checkpoint · Protocol
- 결과: **P0-03 의 새 addendum(HASH_VERIFIED~COMMITTED 결정적 kill
  테스트) 은 `ACCEPTED`** — 이 저장소에서 독립 검수가 명시적으로
  ACCEPTED 를 준 첫 v1-evidence 정정이다. 나머지 셋은 다시
  `CHANGES_REQUESTED`(이번엔 훨씬 작은 흠):
  - DoD-04: K2/Linux limitation 의 파일:줄 인용이 틀렸다
    (`lib.rs:11-16,47-48` → 실제는 `lib.rs:28-32`). 고쳤다.
  - DoD-06: frontmatter 의 vectors 수(36)가 지금 파일(40)과
    불일치한다는 것을 짚었다 — DoD-03 이 같은 파일을 20→40 으로 이미
    정정한 사실과 연결해 명시했다.
  - DoD-03: (a) `field_number_audit.rs:282` 인용이 실제 필드번호
    대조(`:249-274`)가 아니라 감사망 등록 테스트를 가리켰다. (b)
    Ed25519/SCHEMA_TOO_NEW/runtime-policy limitation 정정이 "구현이
    존재한다"와 "이 evidence 가 직접 실행해 확인했다"를 충분히
    구분하지 않아 과장으로 읽힐 수 있었다 — 세 항목 모두 그 구분을
    명시하도록 다시 썼다.
  - P0-03 의 addendum 자체는 코드에서 직접 확인됐다: `chaos-hooks` 가
    `default` feature 밖에 있고, self-kill 훅이 `replace_with_retry`
    성공 직후·`Committed` 기록 이전에 정확히 있고, 새 테스트 단언이
    필요한 사후 상태를 전부 검사한다는 것. 다만 "8회 연속 실행"
    결과 자체는 검수자가 재실행하지 않아 그 수치는 확인 안 됨으로
    남았다.
- 세 건(DoD-03·04·06)은 이번 라운드에서 지적된 것만 다시 고쳤고,
  **재재검수는 아직 하지 않았다** — CLAUDE.md 에 명시.
- 검증: 이번 라운드는 문서 전용 수정이라 `cargo test --workspace`
  293/0/0 재확인만 했다(코드 변경 없음).
- 리포트: 이 이력 항목

---

## 2026-08-17 23:10 — v1 evidence 4건 독립 재검수(코덱스), HASH_VERIFIED~COMMITTED 결정적 kill 테스트 신설

- 계획: 사용자 지시 — "코덱스로 다음 작업들 진행해" (v1 evidence 부채
  축소를 계속한다).
- 스트림: Checkpoint · Protocol
- 수행: 코덱스(read-only) 3개 태스크 — (1) DoD-04·P0-03 수정본 재검수,
  (2) DoD-03·DoD-06 신규 검수, (3) P0-03 이 지적한 "HASH_VERIFIED~
  COMMITTED 구간을 직접 겨냥한 kill 테스트가 없다"는 공백을 메우는
  설계. 넷 다 최초 라운드에서 `CHANGES_REQUESTED`.
- 발견과 조치:
  1. [실제 결함, 자체 재현] DoD-04 재검수가 지적: 앞선 수정에서 붙인
     raw receipt 가 재현되지 않은 옛 실행 그대로였다. 새로 실행해
     `docs/evidence/_raw/DoD-04_replay_status_doctest_2026-08-17.txt` 로
     교체(sha256 digest 포함). `HISTORY.md` 인용에 전체 경로·줄
     번호가 없었던 것도 소스 파일:줄 직접 인용으로 바꾸고, "단수명
     검증 경로가 매 실행된다"를 `ExecutionGrant` 로 좁혔다.
  2. [설계+구현] P0-03 재검수가 지적한 공백 — `write_checkpoint()` 의
     `LATEST` 교체 직후·`COMMITTED` 마커 기록 직전이라는 좁은 구간을
     기존 8개 시간 기반 kill 시점이 겨냥한 적이 없었다. 코덱스 설계를
     받아 `chaos-hooks` feature(비기본)로 `writer.rs::chaos_kill_after_latest()`
     self-kill 훅을 추가했다 — `replace_with_retry` 성공 뒤 `abort()`
     로 그 자리에서 프로세스를 끝내 코드 순서로 그 구간을 결정적으로
     겨냥한다(race 없음). 새 테스트
     `kill_after_latest_before_committed_is_resume_candidate` 가
     "COMMITTED 마커 없이도 재개된다"를 8/8 연속 직접 관측했고,
     훅 위치를 Committed 뒤로 옮기는 뮤테이션으로 비공허성을 확인했다.
     기본 빌드·`cargo test --workspace` 에는 포함되지 않는다.
  3. `writer.rs:157` 의 stale한 주석("포인터 또는 COMMITTED 마커로
     공개된") 도 함께 고쳤다 — 실제로는 COMMITTED 를 요구하지 않는다.
  4. [claim 범위 정정] DoD-03: vectors 메타데이터가 20 인데 실제는
     40(`python -c "..."` 로 재현). claim 의 "각 필드가 실제로
     영향을 준다"는 `JobManifest` 최상위 필드 전부와 `Lease.scope`
     로 좁혀 읽어야 한다(나머지는 field_number_audit 의 번호-이름
     대조만 있다). Ed25519/SchemaTooNew/runtime-policy 관련 stale
     limitation 3건도 정정.
  5. [claim 범위 정정] DoD-06: claim 자체는 유지되나 "framed ingress
     의 모든 타입 혼동을 domain_tag 가 막는다"로 확장해 읽으면 안
     된다 — Lease→Grant 위장은 nested-message decode 단계에서
     먼저 실패한다(domain_tag 이전). negative_tests 의
     `change_coordinator_set` 은 실제로는
     `change_coordinator_set_matches_reference_and_preserves_order`.
     stale limitation 3건 정정.
- 네 evidence 모두 원본 YAML(claim/status/limitations)은 고치지 않고
  append-only "이후 변경" 절로 정정했다. **네 건 다 이 정정 자체는
  아직 재검수를 거치지 않은 상태로 남아 있다** — 다음 라운드 대상.
- 검증: `cargo test --workspace` 293/0/0 (변화 없음). `cargo test -p
  gputeer-checkpoint --features chaos-hooks --test kill_chaos` 8/8.
  새 kill 테스트 단독 8회 연속 실행 8/8. 뮤테이션 2건(훅 위치 이동,
  DoD-04 doctest 필드명)으로 비공허성 확인 후 원복.
- 리포트: 이 이력 항목

---

## 2026-08-17 22:20 — gputeer selftest 에 127.0.0.1 루프백 TCP 왕복 추가, 종료 코드 결함 자체 발견·수정 (293 tests green, selftest 25개 검사)

- 계획: 사용자 지시 — "코덱스로 다음 작업들 진행해". 코덱스(read-only)에게
  전송 계층 설계를 맡겼다(§4c까지는 프로세스 안에서만 꿰어져 있었다 —
  CLAUDE.md 가 스스로 적어 둔 공백).
- 스트림: CLI · Crypto
- 수행:
  1. 코덱스 설계를 받아 `selftest.rs` §5 로 구현: `TcpListener::bind(("127.0.0.1", 0))`
     로 커널이 고른 포트를 얻고, 서버 스레드가 `read_frame` 으로 검증,
     클라이언트가 `write_frame` 으로 전송. 서버는 **개인키 없이 공개키만
     가진 InMemoryKeyring** 을 쓴다 — 검증자 역할을 흉내낸다.
  2. 정상 Grant 는 실제 소켓을 왕복해도 검증 통과(grant_id 에코 확인),
     위조 서명은 실제 소켓을 왕복해도 거부됨을 확인 — 양쪽 모두
     `set_read_timeout`/`set_write_timeout` 을 걸어, framed_ingress 모듈
     문서가 명시한 "타임아웃은 호출자 책임" 경고를 실제 코드로 재확인했다.
  3. [자체 발견, 코덱스 아님] `main.rs` 가 `report.contains("실패")` 로
     종료 코드를 정하고 있었는데, 요약 줄이 항상 `"통과 X · 실패 Y"` 를
     적기 때문에 **Y=0 이어도 그 문자열이 항상 존재해서 정상 실행도
     종료 코드 1이었다.** `cargo run -- selftest` 를 직접 실행해 exit
     code 1 을 재현하고서야 발견했다 — 이전까지는 사람이 눈으로 "실패 0"
     을 읽고 통과로 판단했을 뿐, 자동화가 실제로 그 신호를 쓴 적이 없었다.
     `SelftestReport { text, failed, blocked }` 로 리팩터해 사람이 읽는
     텍스트와 기계가 읽는 상태를 분리했다.
  4. `RULE.md` §7.1 ENVIRONMENT-BLOCKED != FAIL 을 selftest 에도 반영 —
     `Report::blocked()` 신설(루프백 바인드 자체가 막힌 환경을 실패와
     구분). 지금 실행 환경에서는 0건.
- 검증:
  - `cargo test --workspace` 293 passed / 0 failed (변화 없음 — 새 검사는
    `#[test]` 가 아니라 selftest 런타임 체크라 이 카운트에 안 잡힌다).
  - `gputeer selftest` 8회 연속 25/0/0, exit code 0.
  - 뮤테이션 테스트 2건으로 새 검사의 비공허성 증명: (a) 위조 코드를
    빼면 "위조 서명 거부" 검사가 정확히 실패로 뒤집힘(exit 1). (b) 서버
    keyring 에 엉뚱한 공개키를 넣으면 "정상 Grant 검증" 이 정확히
    실패로 뒤집힘(exit 1, InvalidSignature 로 보고됨). 두 뮤테이션 모두
    되돌린 뒤 25/0/0 재확인.
  - 종료 코드 수정 자체도 위 뮤테이션 실행에서 함께 검증됐다 — `failed>0`
    일 때 실제로 `ExitCode::FAILURE` 가 나오는지 그 실행들이 증명한다.
- 이것이 증명하지 않는 것: coordinator·agent·scheduler 는 여전히 없다.
  서버 쪽은 같은 프로세스 안 스레드 하나가 여는 소켓이다 — 별도 프로세스
  간, 하물며 별도 기계 간 통신은 실측하지 않았다.
- 리포트: 이 이력 항목

## 2026-08-17 21:30 — runtime-policy 독립 검수 반영 (293 tests green)

- 계획: 사용자 지시 — "코덱스로 ㄱ" (앞 세션에서 중단된 runtime-policy 검수 확인)
- 스트림: Runtime
- 수행: crates/runtime-policy 5개 파일을 독립 검수(코덱스, read-only)에 맡겼다.
  "정책 강제 계층이라기보다 일부 문자열 판정과 메모리상 상태 판정" 이라는
  총평과 함께 **중대 6건**을 찾았다.
- 발견과 조치:
  1. [중대] ArtifactPolicy::check() 실제 우회 5종을 검수자가 직접 만들었다 —
     전각 마침표(U+FF0E) 두 개로 ".." 위장, 키릴 동형 문자, Windows 예약
     장치명(NUL·CON 등), trailing dot/space. ASCII 전용 강제 + 예약어
     차단 + trailing dot/space 차단을 추가했다. 뮤테이션(ASCII 검사 제거)
     이 정확히 그 2건에서만 실패해 공허하지 않음을 확인했다.
  2. [중대] 빈 문자열/"." 접두사가 check("")·check(".") 를 통과시켰다.
     생성자에서 의미 없는 접두사를 걸러내고, 빈 요청은 어떤 접두사로도
     정당화되지 않게 했다.
  3. [중대] NetworkDecision 의 catch-all 매치가 NoEnforcementBackend 를
     조용히 "실행 허용" 으로 흘려보낼 수 있었다. permits_execution() 을
     추가해 Allowed 일 때만 true 를 반환하게 했다 — 이름이 아니라 타입이
     실수를 막게 한다.
  4. [중대] FenceWatermark 가 재시작 후 stale epoch 를 통과시키는 정확한
     시나리오를 검수자가 재현했다. 이미 문서화된 한계였지만 재현 테스트가
     없었다 — restart_resets_watermark_and_lets_stale_epoch_through 로
     "이 위험이 아직 존재한다" 를 통과가 곧 그 뜻이 되도록 고정했다.
     같은 epoch 재사용(<vs<=) 이 의도적임도 별도 테스트로 명시했다.
  5. [중대] VramEnforcement::guarantees_hard_limit() 을 부르는 곳이
     테스트 말고 없다 — 죽은 API 라는 지적. 모듈 문서에 "판정만 하고
     아무데도 연결 안 됐다" 를 명시했다(수정 아님, 정직한 표시).
  6. [중대] EnforcementClass 를 어떤 모듈도 실제로 안 썼다(각자 자기
     enum 을 씀) — 문서화되지 않은 사문화. YAGNI 원칙에 따라 **삭제**하고
     lib.rs 에 삭제 이유를 남겼다.
- gputeer selftest 4c 절도 permits_execution() 사용으로 갱신. 23개 검사.
- 검증: `cargo test --workspace` **293 passed / 0 failed**, 빌드 경고 0
  (3회 연속). runtime-policy 자체 29건(11건 신설).
- 리포트: 이 이력 항목

## 2026-08-17 20:10 — framed_ingress 독립 검수 반영 (283 tests green)

- 계획: 사용자 지시 — "코덱스로 ㄱㄱ"
- 스트림: Crypto
- 수행: framed_ingress.rs 를 독립 검수에 맡겼다(코덱스, read-only). 실제
  검증 우회는 못 찾았지만 문서·테스트의 과장과 진짜 결함 3건을 찾았다.
- 발견과 조치:
  1. [중대] AttemptReport·ArtifactRef 는 필드 1·2·4·90 의 와이어 타입이
     겹쳐 서명된 바이트가 양쪽으로 유효 디코드되고 canonical 도 같아질
     수 있다. domain_tag 로는 막히지만, 기존 테스트(Lease→Grant 위장)는
     사실 **nested-message decode 실패**로 막힌 것이라 이 방어를 실제로
     시험하지 못했다. distinct_types_with_colliding_wire_fields_are_
     rejected_by_domain_tag 를 신설해 진짜 시나리오로 domain_tag 방어를
     시험한다. 기존 테스트는 frame_type_mismatch_fails_at_nested_message_
     decode 로 이름을 바로잡았다.
  2. [중대] FrameTooLarge/UnknownFrameType 이 몸통을 안 읽어 다음
     read_frame 호출이 잔여 바이트를 헤더로 오인했다. UnknownFrameType 은
     길이가 이미 상한 이내로 확인된 뒤라 안전하게 비울 수 있어 그렇게
     고쳤다. FrameTooLarge 는 상한을 넘는 길이를 실제로 읽는 것 자체가
     DoS 이므로 비우지 않는다 — 대신 "이 오류 뒤 스트림은 못 쓴다" 를
     문서화하고 그 위험을 재현하는 테스트로 고정했다.
  3. [경미] write_frame 이 자기 상한을 검사하지 않고 usize->u32 를
     무검사 캐스팅했다 — 4GiB 넘는 body 는 길이 필드가 잘려 다른 프레임이
     됐다. Result 를 반환하도록 바꾸고 모든 호출부를 고쳤다.
  4. [문서] 모듈 문서가 "타입 위조 실패는 domain_tag 때문" 이라고
     뭉뚱그렸다 — 실제로는 nested-decode 실패와 domain_tag 실패 두
     경로가 있다는 것을 검수자가 지적해 정정했다.
  5. [알려진 한계, 미수정] claimed_len 이 상한 이내면 read_exact 에
     타임아웃이 없어 상대가 몸통을 안 보내면 무기한 블로킹한다.
     std::io::Read 에는 타임아웃이 없어 이 계층에서 못 막는다 —
     호출자가 소켓 타임아웃을 걸어야 한다. 전송 계층이 없어 아직
     아무도 그 책임을 안 진다.
- 검증: `cargo test --workspace` **283 passed / 0 failed**, 빌드 경고 0
  (4회 연속). framed_ingress 자체 16건(6건 신설).
- DoD-09 도 재발 경위를 반영해 재정정했다 (GC 경합 수정이 처음엔
  writer.rs 만 고쳐 8회 중 5회 재발했던 것).
- 리포트: 이 이력 항목 · CLAUDE.md 공백 목록에 DoS 한계 추가

## 2026-08-17 18:40 — 프레이밍·디스패치 · 정책 강제 계층 (278 tests green)

- 계획: 사용자 지시 — "코덱스로 이어서 작업"
- 스트림: Crypto · Runtime(신설)
- 수행:
  1. `crates/crypto/src/framed_ingress.rs` — [type][len][body] 프레이밍,
     헤더 타입으로 decode_and_verify<M> 디스패치. 헤더는 서명 대상이
     아니므로 위조 가능하다는 전제로 다룬다 — 타입을 속이면 실제 서명의
     domain_tag 가 달라 검증이 반드시 실패한다는 성질에 기댄다.
  2. `crates/runtime-policy` (신설, Runtime 스트림) — 서명된 정책 필드가
     Enforceable/Suppressible/Unenforceable 중 어디인지 판정하는 순수
     함수 계층(V-06). artifact_scope(문자열 검사, TOCTOU는 못 막음) ·
     network(OS 백엔드 없으면 항상 거부) · Lease.scope(소유 자원은
     watermark, 외부 API는 억제뿐) · VRAM/S1(CLAUDE.md §0.4 그대로 고정).
- ★ 코덱스에 이 두 작업을 설계로 맡겼는데 **둘 다 read-only 샌드박스라
  파일을 못 썼다** — 설계 논의만 돌아왔다. 설계 자체는 타당해서 그대로
  구현했다(코덱스 원안: [u32_be len][body] 프레이밍 · YAGNI로 tokio 배제 ·
  3분류 강제성 체계 · 8개 negative test 이름).
- 검증: `cargo test --workspace` **278 passed / 0 failed**, 빌드 경고 0
  뮤테이션(상한 검사 제거 · 경로 탈출 검사 제거 · stale epoch 검사 제거)
  모두 의도한 테스트에서만 실패.
- `gputeer selftest` 에 4b(프레이밍) · 4c(정책) 단계 추가. 통과 18 -> 22.
- 안 남은 것: OS 방화벽 호출 · 커널 경로 강제(openat2 등) · 실제 시스템
  조작 — runtime-policy 는 판정만 하고 아무것도 강제로 실행하지 않는다.
- 리포트: 이 이력 항목 · TODO_VISION V-06 갱신 · 소유권 표에 runtime-policy 등록

## 2026-08-17 16:20 — 검증 진입점 · 체크포인트 실패 경로 (248 tests green)

- 계획: 사용자 지시 — "코덱스로 ㄱ"
- 스트림: Crypto · Checkpoint
- 수행:
  1. **`crates/crypto/src/ingress.rs`** — raw bytes -> `Verified<M>` 단일 진입점.
     ★ 만든 방어(verify · keyring · replay 저장소)를 **처음으로 실제 연결**했다.
     Clock 주입 · decode 실패 분리 · LockTimeout 은 재시도 없이 거부(fail-open 금지).
  2. **체크포인트 실패 경로 4건** — `.publication-failed` 불변 마커로 배제,
     등록된 `.tmp` 보존, 동시 GC 경합 판별, DurabilityState 사이드카 기록.
  3. **스테일 evidence 탐지** — `verify_evidence.py` 가 negative test 의
     함수 정의가 아직 있는지 본다.
- 검증: `cargo test --workspace` **248 passed / 0 failed**, 빌드 경고 0 (3회 반복 동일)
- ★ 코덱스 설계를 그대로 받지 않은 것:
  초안은 "COMMITTED 마커 또는 현재 LATEST" 만 재개 후보로 삼았다. **과하다** —
  마커 직전에 kill 된 온전한 체크포인트를 버린다. 카오스 테스트가
  `--workspace` 부하에서 이것을 잡았다(단독 실행은 통과해 부하 의존이었다).
  완결 신호는 **매니페스트의 존재**로 남기고, 실패는 마커로만 배제한다.
- ★ 코덱스 자신의 테스트 2건이 실패했고 둘 다 진짜 발견이었다:
  실패 주입이 무효했다(Rust `File::open` 은 Windows 에서 `FILE_SHARE_DELETE` 를
  포함해 연다) · 동시 GC 가 Windows 에서 `Access Denied` 를 낸다.
- ★ 내가 만든 검사에서 세 번 틀렸다: 자기 주석과 매칭 · raw string 아님(`` 가
  백스페이스) · 소스 캐시가 임시 저장소를 스캔. 셋 다 부정 테스트가 잡았다.
- 안 고친 것: 네트워크 전송·coordinator 없음(진입점을 부르는 것이 없다) ·
  별도 프로세스 replay 경쟁 미측정 · LATEST 포인터 미사용(의도적)
- evidence: `DoD-09` · `DoD-10` 에 '이후 변경' 절 추가 (스테일 방지)
- 리포트: 이 이력 항목으로 갈음

## 2026-08-17 14:10 — 영속 replay 저장소 · 키 관리 · 두 구현의 계약 일치 (DoD-10, 229 tests green)

- 계획: 사용자 지시 — "같이 할 수 있는 코드 작업을 코덱스에 의뢰해서 진행"
- 스트림: Crypto
- 수행:
  1. **DurableReplayGuard** (§10 3단계) — 코덱스에 초안 의뢰, SQLite(rusqlite bundled) 채택.
     ★ 의존성을 **먼저 측정**했다 — 14.55초, SQLite 3.46.0. 추정으로 고르지 않았다.
  2. **PersistentKeyring** (§11 K0/K1) — K1 은 Windows DPAPI.
     Linux 는 조용히 K0 로 내려가지 않고 `UnsupportedPlatform` 으로 실패한다.
  3. **replay_contract.rs** — 두 구현이 같은 답을 내는지 검사하는 계약 테스트 9건.
- 검증: `cargo test --workspace` **229 passed / 0 failed**, 빌드 경고 0
- 코덱스 코드에서 내가 찾은 것:
  - `is_durable()` 이 **없었다.** 만들고 나서 `true` 를 하드코딩했더니
    영속성 제거 뮤테이션에도 `true` 였다 — **거짓말을 했다.**
    `Connection::path()` 에서 도출하도록 고쳤다.
  - "동시 프로세스 지원" 을 문서가 주장했는데 테스트가 없었다.
  - `let _ = now_sql;` 로 미사용 경고를 눌러 놨다 (`CLAUDE.md` §3 위반).
  - 회전 grace 종료 후 구 키를 `InvalidSignature`(위조)로 보고했다 —
    서명은 진짜다. `lookup_retired()` 로 구분한다.
  - 개인키 유출 테스트가 hex 한 가지만 봤다. Debug 는 10진수 배열로 찍는다.
- 검수(codex15)가 찾은 것 — **두 구현이 같은 계약을 만족하지 않았다** (4건).
  각 구현을 따로 시험하면 영원히 안 보인다. 계약 테스트로 닫았다.
  ★ 그 계약 테스트가 **내 기댓값의 오류도 잡았다.**
- 안 고친 것 (`DoD-10` limitations):
  소비 측 미착수(아무도 안 쓴다) · 실제 다중 프로세스 경쟁 미측정 ·
  torn write 미검증 · LockTimeout 재시도 정책 없음 · WAL 비교 근거 없음 ·
  ★ 검수자가 이 코드의 초안을 썼다 (완전한 독립 검수가 아니다)
- evidence: `docs/evidence/DoD-10_영속_replay_저장소.md` (schema v2)
- 리포트: 이 이력 항목으로 갈음

## 2026-08-17 12:40 — ★ 독립 검수 강제(schema v2) · replay 방어 4건 · 재개 선택 필터 (203 tests green)

- 계획: 사용자 지시 — "해결 안 된 4가지를 코덱스와 논의해" + 코덱스 쿼터 소진
- 스트림: Crypto · Checkpoint · 프로세스(공용)
- 수행:
  1. **재개 지점 필터** — 검수자가 `writer.rs:102-133` 에서 반례 4건 제시.
     `find_resume_point` 가 job/attempt 를 안 걸러 **남의 체크포인트에서 재개**할 수 있었다.
     `find_resume_point_for()` 신설 (job/attempt · 빈 매니페스트 · id≠디렉터리명 제외).
  2. **replay 방어 4건** — `require_replay_checked()` 가 replay 검사를 **안 한** 메시지를
     통과시켰다(`_ => true`). `ReplayStatus` 3상태로 갈랐다.
     `MAX_SHORTLIVED_TTL_MS`(15분) · 서명자별 quota · `MAX_GC_ADVANCE_MS`(5분) 추가.
  3. **evidence schema v2** (ADR-030 · `RULE.md` §7.3) — 미해결 4항목을 검수자와 논의해
     기계가 막을 것과 사람 책임을 갈랐다. `scripts/test_verify_evidence.py` 40건 신설.
- 검증:
  - `cargo test --workspace` **203 passed / 0 failed**, 빌드 경고 0
  - 뮤테이션 M1+M2+M3 -> 부정 테스트 4건 FAILED (공허하지 않음)
  - `MAX_GC_ADVANCE_MS` 를 1시간으로 잡았다가 **테스트가 잡아냈다** —
    최대 보존 시한(16분)보다 길면 아무것도 못 막는다. 5분으로 고쳤다.
  - 구현 직후 **2차 검수**에서 우회 7건(치명 1 · 중대 6)을 실제로 통과당했다. 전부 고쳤다.
  - 검수자가 내 부정 테스트의 **공허성 5건**도 지적했다. 전부 고쳤다.
- 안 고친 것 (`DoD-09` limitations):
  `LATEST` 포인터 미사용 · `write_checkpoint` 실패 후 잔여물 ·
  `startup_gc` 가 등록된 `.tmp` 삭제 · `DurabilityState` 미연결
- evidence: `docs/evidence/DoD-09_재개선택_필터.md` (**schema v2 최초 적용**)
- 리포트: 이 이력 항목과 ADR-030 · `docs/runbooks/ai-workflow.md` 갱신으로 갈음

## 2026-08-16 18:20 — ★ 독립 검수(Codex) 지적 7건 시정 (DoD-08 PASS, 167 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T4 착수 전 계약 정비
- 스트림: Protocol · Crypto · Checkpoint · QA
- 배경: 이 세션의 결정(ADR-028 · ADR-029 · canonical 규칙)은 **전부 내가 혼자 판단하고
  내가 만든 테스트로 검증한 것**이다. 그 테스트가 놓친 것은 그 테스트로 못 찾는다.
  `CLAUDE.md` §4 대로 Codex CLI 에 **적대적 검토**를 맡겼다 — 동의가 아니라 반박을 요청
- ★★ **두 구현이 똑같이 틀린 곳 3건 발견** — 벡터 대조로는 원리적으로 못 잡던 것들
  1. **규칙 i-2** 중첩 메시지가 서명 필드만 가지면 `0a00`(빈 중첩)으로 **출력**됐다.
     `is_default()` 검사가 field 90 제외보다 먼저 일어나 규칙 i 가 새어나갔다.
     Python 도 동일 (`manifest={}` → `0801` vs `{90:sig}` → `08011a00`)
  2. **규칙 c-2** map 엔트리 안의 규칙 b 가 규범에 없었다
  3. **규칙 i-3** 도출 해시 제외가 **재귀 적용**돼
     `DatasetRef.retention`(field 4)이 `manifest_hash`(field 4)로 오인돼
     **데이터셋 삭제 정책이 서명에서 지워졌다**
- ★★ **검증 도구 자체가 검증하지 않고 있었다.**
  `--verify` 가 저장된 hex 끼리 관계만 봤다 — **저장본이 구현과 어긋나도 통과**한다.
  나는 `DoD-01` 이래 "vector cross-checks: OK" 를 검증 근거로 인용해 왔다.
  -> `build_vectors()` 재실행 대조로 고쳤다. 관계 없는 벡터 변조 뮤테이션으로 실효성 확인.
  -> **`DoD-01` 에 후속 정정을 추가**했다 (교차검증 자체는 Rust 테스트가 했으므로 유효)
- ★★ **replay nonce 가 메시지와 결속되지 않았다** (보안 영향 최대)
  `verify(msg, ..., nonce, ...)` 로 **호출자가 nonce 를 골랐다.**
  => 서명은 통과하는데 replay 방어만 무력화. 매번 새 값이면 무한 재생 가능
  -> `Signable::replay_nonce()` — **서명된 메시지 필드**에서 가져온다
  -> 검수자 조언대로 **SQLite 착수 전에 계약부터 고쳤다** ("지금 SQLite 를 추가하면
     잘못된 외부 nonce 를 영속 기록하는 구현이 된다")
- checkpoint (첫 독립 검토):
  - **K-1** `write_once` 가 "content-addressed 이름" 을 전제했는데 실제는 `shard-0.bin`.
    같은 이름·다른 내용을 조용히 수락 → **writer 가 "확정했다"고 거짓 보고**
  - **K-2** `RetryPolicy{max_attempts:0}` 이 `atomic.rs:139` 에서 **panic**.
    ADR-026 의 "최종 실패는 명시적 오류" 계약 위반 — panic 은 오류가 아니다
- 부수: `ReplayGuard` → `Result<ReplayDecision, ReplayStoreError>`,
  `VerifyError` 로 프로토콜 결과와 로컬 장애 분리,
  §6.1 `manifest_hash` **공식 자체**를 처음으로 검증
- 검증: **cargo test --workspace = 167 passed / 0 failed** (143 → 167). 빌드 경고 0.
  ★ **기존 벡터 canonical 변경 0건** — 회귀 없이 규범 구멍만 메웠다.
  `SCHEMA_FINGERPRINT` 불변(`.proto` 미변경)
- ★ **시정하지 않은 지적을 숨기지 않았다** — `RevokeLeaseNotice` 반복 전송(미실측),
  증거 3종 서명자 ID(V-08), `ReplicaAck.fence_epoch`(V-07),
  §8 5·6단계 순서(규범 수정 여부 별도 판단), `write_once` 메모리 사용(미측정)
- evidence: `DoD-08_독립검수_시정.md`

## 2026-08-16 16:40 — T2 증거 메시지 시각 정책 (ADR-029 · DoD-07 PASS, 143 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T2
- 스트림: Protocol · Crypto
- ★ **결정: `signing.md` §9 표에 없던 6종은 "권한" 이 아니라 "증거" 다.
  시각으로 만료시키지 않는다.** `Lifetime::Evidence` 신설 (ADR-029)
  - 과거의 사실은 만료되지 않는다. `CheckpointManifest` 를 만료시키면
    **오래된 체크포인트에서 재개할 수 없고**, 그것은 시스템의 존재 이유를 부순다
  - 그러나 "그 시점의 사실" != "지금의 사실" -> `Perpetual` 과 구분한다.
    `Evidence` 는 **`observed_at` 노출을 타입으로 강제**한다.
    "언제인지 모르는 증거" 는 증거가 아니다
  - 신선도는 시각이 아니라 `fence_epoch` 이 판단한다.
    **시계는 어긋나지만 epoch 은 어긋나지 않는다**
- ★ **`ReplicaAck` 만 `fence_epoch` 이 없다** — 6종 중 유일하다.
  그런데 `REPLICATED(n)` 을 세는 근거이므로 **durability 주장의 뿌리**다.
  **복제본이 삭제되어도 ACK 는 영원히 유효하다.**
  `replica_ack_stays_valid_forever_even_if_replica_is_gone` 이 이 결함을 고정한다 —
  **통과한다는 것이 곧 "프로토콜이 막지 못한다" 는 뜻이다.** -> V-07
- 수행: `Signable` 2종 -> **10종**. `ExecutionGrant` · `RenewLeaseRequest` 를
  `ShortLived` 로 구현 — **§9 단수명 경로가 실메시지로 처음 검증**되었다
  (`DoD-04` 는 테스트 전용 타입뿐이었다)
  - `RenewLeaseRequest` 는 `expires_at` 필드가 없어 `issued_at + GRANT_TTL_MS` 로 도출.
    값을 지어내는 게 아니라 §9 가 정한 TTL 적용이며 근거를 코드에 적었다
  - `Signable` 을 `signable.rs` 로 분리 — `to_fields` 는 "어떤 필드",
    `signable` 은 "어떤 domain·수명". **틀렸을 때의 증상이 달라** 섞으면 리뷰가 흐려진다
- 검증: **cargo test --workspace = 143 passed / 0 failed** (129 -> 143). 빌드 경고 0
  ★ `the_three_lifetimes_actually_behave_differently` — 세 정책이 실제로 다른
  동작을 하는지 확인. 전부 같으면 `Lifetime` 구분이 의미가 없다
- ★ TTL==skew 문제: **TTL 을 늘려 미래 방향 skew 를 "살리는" 것은 하지 않았다.**
  단수명 수명을 늘리면 replay 창이 커진다 —
  보안 매개변수를 코드 경로 도달성 때문에 바꾸지 않는다.
  대신 두 테스트로 사실을 고정(기본 TTL 에서 가려짐 + TTL 1시간이면 발동)
- ★ `.proto` 를 건드리지 않았으므로 `schema_version` 상향·벡터 재생성 불필요.
  `SCHEMA_FINGERPRINT` 불변
- 신규 등록: **V-07**(`ReplicaAck.fence_epoch`) · **V-08**(증거 메시지 서명자 ID).
  둘 다 `schema_version` 상향이 필요해 **함께 처리하는 것이 싸다**
- evidence: `DoD-07_시각정책_실메시지.md` · ADR: `ADR-029`

## 2026-08-16 15:40 — ★ 서명 재사용 취약점 발견·시정 (ADR-028 · DoD-06 PASS, 129 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T1b
- 스트림: Protocol · QA(벡터)
- ★★ **스펙 취약점 발견 — `signing.md` §5 가 자기 MUST 를 어기고 있었다.**
  §5 는 "메시지마다 새 domain_tag 를 등록해야 한다(MUST)" 라고 적어 놓고
  membership(6종) · policy · quarantine(2종) 을 **공유**하게 두었다.
  공유하면 §5 의 방어("tag 가 달라 반드시 실패한다")가 사라지고
  canonical 차이만 남는데, 규칙 b(기본값 생략) 때문에 **공격자가 필드를 비우면
  서로 다른 메시지가 같은 바이트가 된다.**
- 실측 (참조 구현 전수 대조) — **충돌 5쌍**:
  ```
  AddMember        == RemoveMember       공통필드[1]    28바이트 동일
  ApproveDevice    == RemoveMember       공통필드[1]    28바이트 동일
  ApproveDevice    == RevokeDevice       공통필드[1,2]  56바이트 동일
  RemoveMember     == RevokeDevice       공통필드[1]    28바이트 동일
  QuarantineDevice == ReleaseQuarantine  공통필드[1]    동일
  ```
  → 소유자의 `RemoveMember` 서명이 `RevokeDevice` 로 통과한다.
  → ★ **격리 판정 m-of-n 서명이 격리 해제로 재사용된다**
- 조치: **ADR-028** — 메시지별 domain_tag 분리 (17 → 23종).
  ★ **canonical bytes 는 하나도 바뀌지 않았다** (tag 는 sig_input 에만 들어간다).
  `schema_version` 상향 불필요, `SCHEMA_FINGERPRINT` 불변, 기존 벡터 회귀 0건
- ★ 회귀 방지 테스트는 **canonical 이 아니라 `sig_input`** 을 본다 —
  ADR-028 이후에도 canonical 은 여전히 같기 때문이다.
  canonical 을 검사하면 "우연히 필드가 달라서 통과"하는 약한 보증만 얻는다.
  전제(canonical 동일)도 `assert_eq!` 로 고정해 전제가 바뀌면 근거를 재확인하게 했다
- 수행: T1b — grant · membership · policy · quarantine. domain 9 → **19/23**.
  벡터 28 → 36건
- 부수:
  - **`DERIVED_HASH_FIELDS` 신설** — 규칙 i 의 의도적 제외(`manifest_hash`)와
    실수 누락(`UNIMPLEMENTED`)을 분리. 뜻이 정반대인데 섞으면 구분할 수 없다
  - `ExecutionGrant` 는 규칙 i 가 **두 번** 적용되는 유일한 메시지
    (도출 해시 + 중첩 manifest/lease 서명) → Agent 의 독립 검증·재계산이 필수
  - `PlacementRationale` 을 서명 대상에 넣었다. 처음엔 "설명용" 이라며 미뤘는데
    가드가 "위조 가능" 으로 실패시켰다 — **약화하지 않고 구현했다.**
    서명 밖이면 Coordinator 가 배치 근거를 사후 조작할 수 있다
  - `every_impl_is_audited` 가 신규 impl 15종을 잡아 등록을 강제했다
- 검증: **cargo test --workspace = 129 passed / 0 failed**. 빌드 경고 0
- evidence: `DoD-06_domain_tag_충돌.md` · ADR: `ADR-028`

## 2026-08-16 14:30 — T1 서명 대상 확장 + 규칙 j 신설 (DoD-05 PASS, 117 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T1
- 스트림: Protocol · QA(벡터)
- 수행: **계약 우선** — 참조 구현 확장 -> 벡터 생성 -> Rust 구현 -> 대조.
  `artifact.proto` 8종 + `lease.proto` 3종의 `ToCanonicalFields`.
  domain 커버리지 **2 -> 9 / 17**. 벡터 20 -> 28건
- 검증: **cargo test --workspace = 117 passed / 0 failed** (105 -> 117). 빌드 경고 0
- ★ **규범 공백 3건**을 찾아 전부 규범 문서에 기록:
  1. **규칙 j 신설** — `int64 value_micro` 가 스키마 전체에서 **유일한 부호 있는 필드**인데
     하필 서명 대상 안에 있었다. 규칙 없이는 구현마다 갈린다(2의 보수 10B vs zigzag 2B).
     canonical 은 유효한 protobuf 인코딩의 부분집합이어야 하므로 **2의 보수** 채택.
     ★ **기존 벡터 20건이 하나도 바뀌지 않았다** — 회귀 없음
  2. **§5 의 4종(genesis·audit·release·invite)은 proto 메시지가 아예 없다.**
     membership·policy·quarantine 은 여러 메시지가 한 tag 를 공유 -> §5.1 기록
  3. **§9 시각 정책 표에 6종이 없고 `expires_at` 필드조차 없다.**
     ★ **추측으로 채우지 않았다**(`CLAUDE.md` §1) — `Signable` 을 구현하지 않고
     §9.1 에 결정할 질문 3개를 적어 T2 로 미뤘다.
     따라서 그 6종은 **아직 `verify()` 를 통과할 수 없다**
- ★ 중첩 서명의 귀결 고정: 규칙 i 재귀로 중첩 서명은 바깥 canonical 에 들어가지 않는다.
  **검증자는 중첩 서명 메시지를 독립 검증해야 한다(MUST)** — 안 하면 REPLICATED(n) 이 거짓이 된다.
  "중첩 전체가 무시되는" 결함과 구분하려고 반대 방향 테스트도 함께 넣었다
- evidence: `DoD-05_T1_서명대상_확장.md`

## 2026-08-16 13:30 — 스트림 소유권 위반 시정 + 실행계획 v2 (105 tests green)

- 계획: 이 커밋으로 `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` 착수
- 스트림: Protocol · Crypto · QA
- ★ **내가 `RULE.md` §4.1 을 어겼다.** `crates/protocol/src/signing.rs` 가
  `ed25519-dalek` 을 직접 썼는데 소유권 표는 Ed25519 를 Crypto 스트림 소유로 정한다.
  **테스트 100건이 전부 통과했고 아무도 알아채지 못했다.**
  테스트가 통과한다고 규칙을 고치지 않고 **코드를 규칙에 맞췄다** (§4.3 마지막 줄)
- 수행:
  - `crates/protocol`: `SignatureVerifier` trait 신설. **암호 라이브러리 의존 0**
    (blake3 만 예외 — canonical 의 일부)
  - `crates/crypto` 신설: `Ed25519Verifier` · `sign()` · `InMemoryKeyring`
    (이름이 "운영에 쓰면 안 됨"을 드러낸다 — §11 K0~K2 미구현)
  - `crates/protocol/tests/stream_ownership.rs` 5건 — 경계를 **코드로 강제**.
    뮤테이션(ed25519 재추가)으로 실효성 확인
  - **실행계획 v2 작성** — v1 범위가 소진됐는데 작업이 계획 밖에서 이어지고 있었다.
    T1~T6 확정. D-5 를 4건으로 갱신
  - `CLAUDE.md` §5 상태표를 디스크·빌드 실측으로 갱신
- 검증: **cargo test --workspace = 105 passed / 0 failed**. 빌드 경고 0. 문서 검사 통과
- 부수 정정: `UnknownSigner` 와 `InvalidSignature` 를 구분해 반환하도록 trait 계약에 명시.
  뭉뚱그리면 운영자가 "팀 멤버가 아니다"와 "위조되었다"를 구분할 수 없다
- 리포트: `docs/reports/2026-08-16_1330_프로토콜_계층_완주_자율세션.md` (세션 종합)

## 2026-08-16 12:40 — Ed25519 서명·검증 + Verified<M> (DoD-04 PASS, 100 tests green)

- 계획: 계획 밖 — `DoD-01`~`DoD-03` 이 모두 limitations 에 남긴 "Ed25519 미구현"
- 스트림: Protocol
- 수행: `crates/protocol/src/signing.rs` — `signing.md` §8 검증 순서 · §9 시각 정책 · §13.2.
  `tests/ed25519_verify.rs` 20건 + 독테스트 2건
- 검증: **cargo test --workspace = 100 passed / 0 failed** (78 -> 100). 빌드 경고 0건
  - §8 의 9단계가 **각각 실제로 발동**함을 확인. 발동하지 않는 단계는 없는 것과 같다
  - 보안 필드 변조 **9종 전부 거부** (DoD-03 이 서명에 넣은 것들이 실제로 지켜지는가)
  - ★ `Verified<M>` 우회 생성 차단을 `compile_fail` 독테스트로 검증.
    **필드를 pub 으로 바꾸는 뮤테이션**에서 FAILED, 원복 시 통과
- 설계 결정: `VerifyOutcome` 에 **`VALID` 를 두지 않았다.** 성공은 다른 타입이다.
  열거형에 VALID 를 두면 새 실패 값이 조용히 통과한다
- ★ 발견 2건 (둘 다 테스트가 처음에 실패해서 드러났다):
  1. `minimum_security_tier = 0` 변조가 **no-op** 이었다 (기준값이 이미 0).
     -> 모든 변조 케이스에 `assert_ne!(변조본, 원본)` 비공허성 단언 추가
  2. Grant 기본 TTL(60초) == skew 허용치(60초) 라서
     **미래 방향 skew 경로가 만료 검사에 가려져 도달 불가능**하다.
     안전성 문제는 아니나 "skew 검사가 동작한다"고 잘못 믿게 된다
  3. `compile_fail` 독테스트를 처음에 `tests/` 에 두었는데 **실행되지 않는다.**
     검증하지 않는 것을 주장하고 있었다 -> `src/` 로 이동
- 미구현 명시: **§8-8 replay 는 저장소 계층이 없어 실제 방어가 없다.**
  `NoReplayCheck` 라는 이름으로 사실을 드러내고 `require_replay_checked()` 가
  미검사 값의 부작용 경로 사용을 막는다. 조용히 빠뜨리지 않았다
- evidence: `DoD-04_ed25519_검증순서.md`

## 2026-08-16 11:30 — P0-08 스키마 진화 (PASS, 78 tests green)

- 계획: 계획 밖 — `DoD-02` 가 제기한 "CLAUDE.md §0.2 vs prost 기본 동작 충돌"
- 스트림: Protocol
- 수행: `tests/schema_evolution.rs` 6건(q1~q6) + `tests/schema_fingerprint.rs` 2건 +
  `proto/SCHEMA_FINGERPRINT.txt`(66 메시지 · 389 필드)
- 검증: **cargo test --workspace = 78 passed / 0 failed** (70 -> 78)
  - **prost 는 미지 필드를 조용히 버린다** — 118B -> 148B(주입) -> 118B(재인코딩).
    오류도 경고도 없다. canonical 에도 흔적이 없다
  - => 구버전은 본문만 보고는 새 필드의 존재를 알 수 없다. **유일한 신호는 schema_version**
  - `schema_version` 은 canonical(필드 1)과 sig_input **양쪽**에 묶여 강등은 서명을 깬다
  - => **§7.2 SCHEMA_TOO_NEW 는 구현 가능하다. 아키텍처 재검토 불필요**
- ★ 새 발견: **§7.3 "schema_version 증가 없는 필드 추가 금지" 를 프로토콜은 강제하지 못한다.**
  한 줄짜리 실수가 조용한 보안 우회가 된다 (구버전이 새 보안 제약을 무시한 채 통과)
  -> `SCHEMA_FINGERPRINT.txt` + 대조 테스트로 **빌드 시점 강제**.
  뮤테이션(`bool require_attestation = 63` 추가)으로 실효성 확인 —
  "추가된 줄: 63 bool require_attestation" 을 출력하며 실패
- 부수 발견: 버전 검사를 서명 검사보다 먼저 해야 하는 이유는 **안전성이 아니라 진단 정확성**.
  순서를 뒤집으면 "업그레이드 필요" 를 "서명 위조" 로 보고한다
- 결정: `signing.md` §7.2 변경 없음. §7.3 에 강제 장치 규범 추가. §8 근거 정정.
  **V-05 등록** — `oneof` 도입 시 지문 파서가 무력해진다.
  **V-06 등록** — 서명된 정책 필드의 강제 계층 (v0.1 필수)
- evidence: `P0-08_스키마_진화.md`

## 2026-08-16 10:40 — 서명 밖 필드 6건 제거 (DoD-03 PASS, 70 tests green)

- 계획: 계획 밖 — `DoD-02` 가 찾은 "위조 가능한 보안 필드 3건" 을 닫는 작업
- 스트림: Protocol · QA(벡터)
- 수행: **계약 우선 순서 준수** — 참조 구현 확장 -> 벡터 생성 -> Rust 구현 -> 대조.
  `reference_canonical.py` SCHEMAS 에 6종 추가 + JobManifest 를 전 필드로 + Lease 신설.
  벡터 12 -> 20건. `to_fields.rs` 에 6종 impl + JobManifest 10/11/12/54/55 · Lease 40 편입
- 검증: **cargo test --workspace = 70 passed / 0 failed** (55 -> 70).
  `v02_full_manifest` **896바이트가 Python 참조 구현과 바이트 일치**(BLAKE3 까지).
  ★ `every_field_in_full_manifest_affects_canonical` — 27개 필드를 하나씩 지워
  canonical 이 반드시 변하는지 확인. **27/27 전부 서명 반영**.
  벡터 대조만으로는 "두 구현이 사이좋게 같은 필드를 빠뜨린" 경우를 못 잡는다
- 발견: **`v02_full_manifest` 는 "모든 필드" 라고 적혀 있었지만 16개 부분집합이었다.**
  주장과 실제가 어긋난 만큼은 아무도 검증하지 않는다.
  -> `missing_from_full()` 로 벡터 생성 시점에 코드가 검사하게 했다
- 결정: `UNIMPLEMENTED_FIELDS` **비었다**. DoD-02 의 "위조 가능한 보안 필드 3건" 해소.
  단 **"서명에 들어갔다"와 "Agent 가 그 정책을 강제한다"는 다르다** — 강제 계층 미구현
- 리포트: `docs/reports/2026-08-16_0930_prost_연동계층_자율세션.md` (§12 갱신)
- evidence: `DoD-03_서명대상_완전성.md`

## 2026-08-16 09:30 — prost 연동 계층 (DoD-02 PASS, 55 tests green)

- 계획: 계획 밖 — `DoD-01` limitations 1·2번을 닫는 작업
- 스트림: Protocol
- 수행: `build.rs`(protoc-bin-vendored) · `src/to_fields.rs`(ToCanonicalFields 수동 구현) ·
  `tests/prost_canonical.rs` 11건 · `tests/field_number_audit.rs` 6건
- 검증: **cargo test --workspace = 55 passed / 0 failed** (38 -> 55).
  실제 prost 메시지가 Python 참조 구현과 바이트 일치(BLAKE3 까지).
  ★ map 500회 재구축에서 **prost 495종 vs canonical 1종** — 비공허성 단언 포함.
  field_number_audit 은 **뮤테이션 2종**(중복 경로·이름 불일치 경로)으로 실효성 확인
- 발견:
  1. `job.proto` 에 `import "lease.proto"` 누락 — **5개 proto 를 한 번도 컴파일한 적이 없었다**
  2. 내 negative test 주장이 틀렸다. map 없으면 prost==canonical(113B/113B).
     테스트를 느슨하게 고치지 않고 **근거를 다시 세웠다**
  3. ★ **서명에서 빠진 필드 6건, 그 중 3건이 보안 필드**
     (JobManifest 54 network · 55 artifact_scope · Lease 40 scope). 현재 **위조 가능**
  4. 계획서 §15.2 `bytes submitter_device_id` vs proto `string` 드리프트
- 결정: `signing.md` §3 규칙 변경 없음. §13.1 **근거 문구 정정**(규범 아님) +
  수동 구현 채택 명시 + `UNIMPLEMENTED_FIELDS` 선언 의무화.
  **P0-08 신규 등록** — `SCHEMA_TOO_NEW` × prost unknown-field.
  `CLAUDE.md` §0.2 와 prost 기본 동작이 정면 충돌한다.
  D-5 기준선 수정 요청 **3건 -> 4건**
- 리포트: `docs/reports/2026-08-16_0930_prost_연동계층_자율세션.md`
- evidence: `DoD-02_prost_연동_계층.md`

## 2026-08-16 08:10 — P0-03 카오스 테스트 완주 (38 tests green)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S3
- 스트림: Checkpoint
- 수행: `crates/checkpoint/src/writer.rs` (write_checkpoint / find_resume_point / startup_gc),
  `src/bin/ckpt_writer.rs` 카오스용 바이너리, `tests/kill_chaos.rs` 7건.
  별도 프로세스를 띄워 8개 고정 시점(40~700ms)에 실제로 kill
- 검증: **cargo test --workspace = 38 passed / 0 failed** (기존 31 + kill_chaos 7)
  불변식 4개 전부 통과. ★ 테스트가 공허하지 않음을 별도 검증 —
  8회 중 7회에서 PARTIAL 발생, `kill@560ms` 에서 **manifest.json.tmp**(매니페스트 쓰는 도중) 포착
- 결정: **P0-03 PASS.** local-first 원칙(§18.1) 재검토 안 함.
  단 COMMITTED durability 주장은 **HASH_VERIFIED 까지만 입증** — 복제 계층 미구현.
  P0-03b(복제) · P0-03c(전원 차단) 신규 등록
- 리포트: `docs/reports/2026-08-16_0700_P0스파이크_3건_자율세션.md` (§7 갱신)
- evidence: `P0-03_checkpoint_durability.md`

## 2026-08-16 07:00 — P0 스파이크 3건 (P0-01 PASS · P0-07 PASS · P0-06 FAIL-SCOPE)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md`
- 스트림: QA · Runtime
- 수행: x600 에 Rust 1.97.1 설치. SSH 전달을 base64 -> scp+`-File` 로 교체.
  P0-01(Windows S1+CUDA) · P0-07(추정 정확도) · P0-06(VRAM 강제) 실측
- 검증:
  **P0-01 PASS** — Restricted Token 에서 CUDA 완전 동작. Job Object 종료 시 VRAM 168->489->168 반환
  **P0-07 PASS** — sigma=0.021 (DoD 0.20). warmup 10->300 으로 오차 9.3%->1.2%
  **P0-06 FAIL-SCOPE** — 기준선 §10.3 의 "Job Object 는 VRAM 무관" 이 Windows 에서 **틀렸다**.
  5x5 스윕으로 `VRAM 최대 ~= RAM 제한 - 2000MiB` 확인
- 결정: ADR-005 유지 · ADR-007 유지 · **ADR-015 유지** · **ADR-027 신설**.
  기준선 수정 3건 승인 대기 (ADR-026 · ADR-027 · §12.3 warmup)
- ★ 오판 5건 정정 기록. 특히 "빈 출력 -> CUDA 실패" 오판을 잡지 못했으면
  ADR-005 를 뒤집고 Windows S1 을 로드맵에서 제거했을 것
- 리포트: `docs/reports/2026-08-16_0700_P0스파이크_3건_자율세션.md`
- evidence: `P0-01` · `P0-07` · `P0-06`

## 2026-08-16 06:20 — Rust 구현 착수: protocol + checkpoint (31 tests green)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S2 · S4
- 스트림: Protocol · Checkpoint
- 수행: Rust 1.97.1 설치(로컬). Cargo workspace + 크레이트 2종 구현.
  `crates/protocol` — canonical_encode(규칙 a~i) · sig_input · Domain 17종 · merkle · constants
  `crates/checkpoint` — ADR-026 write_once/replace_with_retry · sync_dir · 상태전이 · ReplicaSet
- 검증: **cargo test --workspace = 31 passed / 0 failed**
  canonical 15건이 Python 참조 구현과 **바이트 단위 일치**(BLAKE3 다이제스트까지).
  checkpoint 16건 중 `adr026_write_once_succeeds_while_readers_hold_files_open` 이
  P0-03a 에서 313/3000 실패하던 조건에서 **500/500 성공**
- 결정: signing.md §3 규칙 변경 없음. 다음 공백은 **prost 연동 계층**
- 리포트: `docs/reports/2026-08-16_0620_Rust구현_protocol_checkpoint.md`
- evidence: `DoD-01_canonical_encode_교차검증.md`

## 2026-08-16 05:30 — 원격 GPU 기계(x600) 실측, BLOCKED 7건 중 4건 해제

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S0 (재실측)
- 스트림: —
- 수행: `~/.ssh/config` 의 x600 · runpod-gpu 두 호스트 조사.
  x600 = **RTX 4070 SUPER 12GB · driver 595.79 · CUDA 13.2 · Windows 11 · 가상화 ON**.
  runpod-gpu 는 Connection refused (인스턴스 종료)
- 검증: `nvidia-smi` + `Win32_VideoController` 교차 확인. `wsl --list` 로 배포판 0개 확인.
  **P0-01·02·07 해제 · P0-06 부분 해제 · P0-04/04b/05 BLOCKED 유지**
- 결정: x600 을 GPU 검증 기계로 지정. 작업 디스크 **F:** (C: 는 8.1GB 뿐).
  실행계획 D-2 해소, **D-1(Rust)이 유일한 착수 차단 요인**으로 남음
- 리포트: (S0 재실측이므로 `docs/evidence/ENV-02_원격_GPU_기계_실측.md` 로 갈음)

## 2026-08-15 14:10 — P0-03a Windows 파일시스템 원자성 조사

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S1
- 스트림: QA
- 수행: `tools/probes/windows_fs_atomicity.py` 작성(프로브 8종).
  기준선 §18.2 의 `rename -> fsync(dir)` 절차가 Windows/NTFS 에서 성립하는지 실측
- 검증: **FAIL 2 · PARTIAL 1 · PASS 5.**
  Windows `MoveFileEx` 가 열린 파일을 대체하지 못함(313/3000). POSIX 시맨틱 API 로도 실패(87/1000).
  디렉터리 fsync 는 쓰기 권한을 주면 가능. 부분 내용 관측은 전 시나리오 0건
- 결정: **ADR-026 신설** — 데이터 파일은 write-once 로 rename-over-existing 회피,
  포인터 파일만 재시도 replace, `sync_dir` 플랫폼별 정의
- 리포트: `docs/reports/2026-08-15_1410_P0-03a_파일시스템_조사.md`

## 2026-08-15 13:45 — 환경 실측 (ENV-01)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S0
- 스트림: —
- 수행: 툴체인·GPU·파일시스템 실측. `Get-Command` + 표준경로 + WMI 3중 교차 확인
- 검증: **NVIDIA GPU 없음(Intel Iris Xe) · Rust 툴체인 없음.**
  P0 스파이크 8종 중 **7종이 ENVIRONMENT-BLOCKED**. P0-03 만 실행 가능
- 리포트: (S0/S1 을 묶어 위 리포트에 기록)

## 2026-08-15 — 저장소 골격 수립

- 계획: (기준선 통합 직후. 실행계획서 이전)
- 스트림: —
- 수행: `RULE.md`·`CLAUDE.md`·`docs/` 10개 폴더·템플릿 5종·`scripts/verify_evidence.py` 생성.
  `proto/`·`docs/protocol/`·`tools/`·`tests/vectors/` 를 저장소 안으로 이동
- 검증: `verify_evidence.py` 가 템플릿의 자리표시자를 정상 검출(commit·raw_output 2건).
  `reference_canonical.py --self-test` 12/12 통과
- 리포트: (골격 수립이라 리포트 생략. 다음 세션부터 필수)
