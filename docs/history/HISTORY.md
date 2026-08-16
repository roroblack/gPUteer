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
