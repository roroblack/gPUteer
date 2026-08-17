---
id: P0-03a
claim: "기준선 §18.2 의 체크포인트 확정 절차(tmp -> fsync -> rename -> fsync(dir))가 Windows/NTFS 에서 성립하는지 실측하고, 성립하지 않으면 수정안을 도출한다"
status: PASS
commit: 2fd847628b4cfec54cbafc41cb5c4b5a9c79f66a
binary_digests:
  probe_script: "tools/probes/windows_fs_atomicity.py (Python, 미컴파일)"
protocol_versions:
  none: "해당 없음 - 파일시스템 계층 조사"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS (C:, 222.4GB)"
hardware: "Intel Iris Xe Graphics / NVIDIA GPU 없음 (이 조사는 GPU 무관)"
network_profile: "해당 없음 - 로컬 파일시스템"
command: |
  python tools/probes/windows_fs_atomicity.py
raw_output: |
  [P1a] 동시 독자가 없을 때 rename 확정이 성공하는가
      결과: 성공 3000 / 3000   거부 0 · 기타 오류 0
      판정: PASS

  [P1b] 독자가 FILE_SHARE_DELETE 없이 열고 있으면 rename 이 실패하는가
      교체 시도 3000 · 성공 313 · 거부 2687 · 기타 0
      마지막 오류: WinError 5 (ACCESS_DENIED)
      판정: FINDING - POSIX 와 다르다

  [P1c] 독자가 FILE_SHARE_DELETE 로 열면 rename 이 원자적인가
      교체 시도 3000 · 성공 607 · 거부 2393 · 기타 0
      독자 관측 - 온전한 A 56898 · 온전한 B 66691 · 부분 내용 0 · 열기 실패 1
      판정: FAIL

  [P1d] POSIX 시맨틱 rename(FileRenameInfoEx)이 열린 파일을 대체하는가
      성공 87 / 1000 · ACCESS_DENIED 0 · 기타 913 (GetLastError=32 SHARING_VIOLATION)
      독자 관측 - 부분 내용 0
      판정: FAIL

  [P2] 디렉터리 fsync 대응물이 Windows 에 있는가
      POSIX os.open(dir)+os.fsync(): 실패 (PermissionError Errno 13)
      Win32 CreateFileW(BACKUP_SEMANTICS)+FlushFileBuffers(): 성공
        GENERIC_READ            -> FlushFileBuffers 실패 (err=5)
        GENERIC_READ|WRITE      -> FlushFileBuffers 성공
      판정: PARTIAL - 플랫폼별 구현 명시 필요

  [P3] 쓰기 도중 강제 종료 시 확정 파일이 생기는가
      남은 파일: ['READY', 'ckpt.bin.tmp']  (ckpt.bin 없음, tmp 5.7MB)
      판정: PASS

  [P4] 매니페스트 유무로 PARTIAL 을 식별할 수 있는가
      ckpt-good -> COMPLETE · ckpt-partial -> PARTIAL
      판정: PASS

  [P5] 파일시스템: NTFS

  probe 8개 · FAIL 2 · PARTIAL 1
artifacts:
  - docs/evidence/_raw/P0-03a_probe.txt
  - tools/probes/windows_fs_atomicity.py
negative_tests:
  - "P1b: 독자가 FILE_SHARE_DELETE 없이 파일을 연 최악 조건에서 rename 이 2687/3000 실패하는 것을 확인 (정상 경로만 봤다면 놓쳤을 것)"
  - "P1d: POSIX 시맨틱 API 로도 SHARING_VIOLATION(32) 로 913/1000 실패 - '대안 API 를 쓰면 된다'는 가설을 반증"
  - "P2: GENERIC_READ 로만 시도하면 실패(err=5)하고 쓰기 권한을 줘야 성공 - 1차 시도의 오판을 교차 확인으로 정정"
  - "P3: 쓰기 중 프로세스를 강제 kill 해 확정 파일이 조기 노출되지 않음을 확인"
limitations:
  - "NTFS 만 검증했다. ReFS/exFAT/네트워크 드라이브(SMB)는 미검증이며 동작이 다를 수 있다"
  - "Python 구현으로 측정했다. Rust std::fs 의 공유 모드 기본값은 별도 확인이 필요하다 (문서상 FILE_SHARE_DELETE 를 포함하나 실측하지 않았다)"
  - "P1c 에서 FILE_SHARE_DELETE 독자가 있는데도 2393건이 거부된 원인을 규명하지 못했다. Windows Defender 실시간 검사 등 외부 요인 가능성이 있으나 확인하지 않았다"
  - "전원 차단(power loss) 상황은 검증하지 않았다. 프로세스 kill 만 측정했으므로 물리적 내구성은 증명되지 않는다"
  - "단일 기계 1회 측정이다. 다른 하드웨어/드라이버 조합에서 재현성을 확인하지 않았다"
  - "이 조사는 파일시스템 계층만 다룬다. 체크포인트 전체 durability contract(replica ACK, COMMITTED 판정)는 P0-03 본 스파이크의 범위다"
decision: "ADR-026 신설. 기준선 §18.2 의 확정 절차를 (1) 데이터 파일은 write-once 고유 이름으로 rename-over-existing 을 회피 (2) 포인터 파일만 재시도 가능한 replace 사용 (3) fsync(dir) 을 플랫폼별로 정의 로 수정 요청"
---

# P0-03a · Windows 파일시스템 원자성 조사

## 무엇을 입증하려 했는가

기준선 §18.2 와 `docs/protocol/state-machines.md` §4 는 체크포인트 확정 절차를 규범으로 정한다.

```text
<name>.tmp 기록 -> fsync(file) -> rename(name) -> fsync(dir)
```

**이 절차는 POSIX 를 전제한다.** Windows 에서 성립하는지를 Rust 구현 착수 전에 확인해야 한다.
성립하지 않으면 `RULE.md` §3.5 에 따라 **규범 문서를 먼저 고쳐야 한다.**

## 어떻게 측정했는가

각 질문을 **최악 조건**으로 설계했다. 정상 경로만 보면 전부 통과하기 때문이다.

- rename 은 **동시 독자를 두고** 측정했다. 독자 없는 경우(P1a)를 기준선으로 삼아 대조했다.
- 독자의 **공유 모드를 두 가지**로 나눴다 (FILE_SHARE_DELETE 유/무).
- 디렉터리 fsync 는 **접근 권한 조합 3가지**를 순차 시도했다.
- kill 테스트는 별도 프로세스를 띄워 **실제로 강제 종료**했다.

## 결과

### 발견 1 — Windows 의 rename 은 열린 파일을 대체하지 못한다

| 시나리오 | rename 성공률 |
|---|---|
| 동시 독자 없음 | **3000 / 3000** |
| 독자가 `FILE_SHARE_DELETE` 없이 열기 | **313 / 3000** |
| 독자가 `FILE_SHARE_DELETE` 로 열기 | 607 / 3000 |
| POSIX 시맨틱 API (`FileRenameInfoEx`) | 87 / 1000 |

POSIX 에서는 다른 프로세스가 파일을 열고 있어도 rename 이 성공한다.
**Windows 에서는 `ERROR_ACCESS_DENIED(5)` 또는 `ERROR_SHARING_VIOLATION(32)` 으로 실패한다.**

`FileRenameInfoEx` + `FILE_RENAME_FLAG_POSIX_SEMANTICS` 로도 해결되지 않았다.
**"대안 API 를 쓰면 된다"는 가설은 반증되었다.**

### 발견 2 — 실패해도 원자성 자체는 깨지지 않는다

중요한 구분이다. 모든 시나리오에서 **`부분 내용` 관측은 0건**이었다.

```text
rename 이 성공하면 -> 독자는 항상 옛 내용 또는 새 내용만 본다 (원자적)
rename 이 실패하면 -> 아무 일도 일어나지 않는다 (옛 내용 유지)
```

즉 문제는 **"찢어진 데이터"가 아니라 "확정이 거부된다"** 는 것이다.
데이터 무결성 위험이 아니라 **가용성 위험**이다.

### 발견 3 — 디렉터리 fsync 는 가능하다. 단 쓰기 권한이 필요하다

```text
POSIX os.open(dir) + os.fsync()                     실패 (Errno 13)
Win32 CreateFileW(GENERIC_READ, BACKUP_SEMANTICS)   실패 (err=5)
Win32 CreateFileW(GENERIC_READ|WRITE, BACKUP_SEM.)  성공
```

★ 1차 시도에서 `GENERIC_READ` 만으로 열어 "불가능" 으로 판정했다가,
권한 조합을 늘려 재측정해 정정했다. **한 번의 실패를 결론으로 삼지 않은 것이 중요했다.**

### 발견 4 — 매니페스트-마지막 규칙은 유효하다

- 쓰기 중 강제 종료 -> `.tmp` 만 남고 확정 이름 파일은 생기지 않음
- 매니페스트 유무로 `COMPLETE` / `PARTIAL` 판정 가능

**규범의 이 부분은 Windows 에서 그대로 성립한다.**

## 이 실험이 증명하지 "않는" 것

- **NTFS 만** 검증했다. ReFS · exFAT · SMB 네트워크 드라이브는 미검증이다.
- **Python 으로** 측정했다. Rust `std::fs` 의 공유 모드 기본값은 별도 확인이 필요하다.
- **P1c 의 2393건 거부 원인을 규명하지 못했다.** `FILE_SHARE_DELETE` 독자가 있는데도
  거부된 이유가 Windows Defender 실시간 검사인지 다른 요인인지 확인하지 않았다.
- **전원 차단을 검증하지 않았다.** 프로세스 kill 만 측정했으므로 물리적 내구성은 증명되지 않는다.
- **단일 기계 1회 측정**이다. 재현성을 확인하지 않았다.
- 체크포인트 **전체 durability contract**(replica ACK · COMMITTED 판정)는 이 조사 범위 밖이다.
  그것은 P0-03 본 스파이크가 다룬다.

## 결정

**ADR-026 을 신설하고 기준선 §18.2 수정을 요청한다.**

핵심 통찰: **기준선 §18.6 은 "모든 artifact 는 immutable" 이라고 이미 선언했다.**
불변 artifact 는 **기존 파일을 덮어쓸 일이 없다.** 따라서 데이터 파일에 대해서는
`rename-over-existing` 자체가 필요 없고, P1a(3000/3000 성공)의 조건만 만족하면 된다.

`replace-over-existing` 이 실제로 필요한 곳은 **canonical/latest 포인터 파일 하나뿐**이며,
이것은 작아서 재시도 비용이 무시할 만하다.

```text
데이터 파일   고유 이름으로 write-once. rename 대상이 존재하지 않는다  -> 문제 회피
포인터 파일   replace 필요. 지수 백오프 재시도 + 실패 시 명시적 오류
fsync(dir)    플랫폼별 구현 (POSIX: fsync / Windows: BACKUP_SEMANTICS + 쓰기권한)
```

관련: `docs/decisions/ADR-026_체크포인트_확정_절차_플랫폼_차이.md`
계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S1

---

## ★ 이후 변경 (2026-08-18 01:30) — claim 을 "원래 절차 실패 확인 + ADR-026 도출"로 명시, limitation 1건 정정

독립 검수(`agent:codex-cli`, read-only)가 재검수했다. **문자
그대로의 조사 claim(원래 절차를 실측했다)은 참**이지만, "원래
절차가 Windows 에서 성립한다"는 뜻으로 읽으면 거짓이다 — 이
evidence 자체가 P1b/P1c/P1d 에서 열린 대상 파일의 replace 가
실패하고, P2 는 쓰기 권한을 추가해야만 `FlushFileBuffers` 가
성공했음을 실측으로 보였다(`P0-03a:16-52`). 그래서 claim 은
아래처럼 명시적으로 읽는다:

> "Windows/NTFS 실측에서 기존 확정 파일의 replace 는 실패할 수
> 있고, 디렉터리 flush 에는 쓰기 권한이 필요함을 확인했다. 이에
> ADR-026 의 write-once 데이터·재시도 가능한 포인터 replace·
> 플랫폼별 `sync_dir` 수정안을 도출했다."

### ADR-026 수정안이 지금 코드에 반영되어 있다

- 데이터 파일: `write_once` — tmp 기록·`sync_all`·기존 대상 없는
  rename·`sync_dir`(`crates/checkpoint/src/atomic.rs:117-191`)
- 포인터 파일: `replace_with_retry` 지수 백오프(`atomic.rs:194-255`)
- Windows `sync_dir`: `GENERIC_READ|WRITE` + `FILE_FLAG_BACKUP_SEMANTICS`(`atomic.rs:258-283`)
- writer 경로도 데이터·매니페스트엔 `write_once`, 포인터엔
  replace 를 쓴다(`crates/checkpoint/src/writer.rs:100-135`)

### negative_tests/프로브 이름은 전부 실재 확인됨

`P1b`·`P1d`·`P2`·`P3` 전부 evidence·현재 Python 프로브·raw
artifact 에서 확인했다(`tools/probes/windows_fs_atomicity.py:148-183,225-344,356-428,455-478`,
`docs/evidence/_raw/P0-03a_probe.txt:11-56`).

### stale limitation

"Rust `std::fs` 의 공유 모드 기본값은 별도 확인이 필요하다"(`:63`)
는 ★ stale 이다. 그 뒤 실제로 확인됐다 — Rust `File::open` 은
Windows 에서 `FILE_SHARE_DELETE` 를 **포함**한다(`crates/checkpoint/tests/write_failure.rs:5-20`
모듈 문서). 이 사실은 처음에 **역방향으로** 발견됐다 — 옛 실패
주입 테스트가 "핸들을 점유하면 rename 이 실패한다"는 가정에
기댔는데, 그 가정 자체가 틀려서 실패 주입이 무효였다(테스트가
스스로 "성공하면 실패 주입이 무효하다" 단언을 넣어 뒀던 덕에
잡혔다). 지금은 `LATEST` 를 디렉터리로 만들어 플랫폼 독립적으로
실패를 주입한다. 이 재확인 자체가 **당시 P0-03a 가 "별도 확인이
필요하다"고 정직하게 남겨 둔 미검증 항목이 나중에 실제로 뒤집힌
사례**다 — 원래 문서에 적혔던 "문서상 FILE_SHARE_DELETE 를 포함하나
실측하지 않았다"는 추측이 맞았던 것으로 확인됐다.

★ 2026-08-18 01:40 재검수가 이 문단의 근거 과장을 잡았다. 원래
"`durability_chaos.rs:29-86,361-384` 와 `write_failure.rs` 가 그
재확인을 담은 테스트"라고 적었는데 **틀렸다** — `FILE_SHARE_DELETE`
포함 사실은 `write_failure.rs:5-20` **모듈 문서**(주석)에 서술만
되어 있을 뿐, 그것을 독립적으로 검증하는 테스트는 아니다.
`durability_chaos.rs:371-373` 은 공유 모드와 무관하게 동작하도록
설계됐고(`:374-384` 도 기존 파일 대체가 아니라 다른 이름의
write-once 만 검사), `write_failure.rs` 의 현재 실패 주입도 공유
모드가 아니라 **디렉터리 rename** 이다(`:52-66`). 그래서 정확히는:

> "Rust `File::open` 이 Windows 에서 `FILE_SHARE_DELETE` 를
> 포함한다"는 사실은 `write_failure.rs:5-20` 모듈 문서에 **서술로만**
> 기록되어 있다. 그 사실 자체를 독립적으로 재검증하는 테스트는
> 이 저장소에 없다 — fresh 재실측도, 재검증 테스트도 확인 안 됨이다.

나머지 limitation(NTFS 한정·P1c 원인 미규명·전원 차단 미검증·단일
기계 측정·durability contract 범위 제외)은 지금도 유효하다.

### review_outcome

`CHANGES_REQUESTED` → 위 정정으로 claim 명시·stale limitation·
ADR-026 반영 사실을 담았다. 원본 YAML 은 당시 기록이므로 고치지
않는다.

★ 2026-08-18 01:40 두 번째 재검수 — `agent:codex-cli` 가 claim
명시·ADR-026 반영 확인은 정확하다고 인정했지만, `FILE_SHARE_DELETE`
재확인 근거를 과장했다고 지적했다 — `durability_chaos.rs`/
`write_failure.rs` 를 "재확인 테스트"라고 적었는데, 실제로는 그
사실이 `write_failure.rs` **모듈 문서에 서술로만** 남아 있을 뿐
독립적으로 검증하는 테스트가 아니었다(`durability_chaos.rs:371-373`
은 공유 모드와 무관하게 설계됐고, `write_failure.rs` 의 지금 실패
주입은 디렉터리 rename 이다). 위에서 "서술로만 기록되어 있고,
재검증 테스트는 없다"로 다시 좁혔다.

★ 2026-08-18 01:50 세 번째(최종) 재검수 — `agent:codex-cli` 가
**`ACCEPTED`** 로 판정했다. `write_failure.rs:5-20` 전체가 모듈
문서 주석일 뿐임을 확인했고, `durability_chaos.rs:371-384` 가
공유 모드와 무관하게 설계됨을, `write_failure.rs:52-66` 의 지금
실패 주입이 디렉터리 rename 방식임을 재확인했다. "저장소 전체
검색에서도 Rust `File::open` 의 공유 모드를 독립 검증하는 테스트는
확인되지 않았다." 세 라운드에 걸친 지적이 모두 해소됐다.
