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
