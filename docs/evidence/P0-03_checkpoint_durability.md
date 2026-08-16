---
id: P0-03
claim: "체크포인트가 쓰기 도중 프로세스 강제 종료되어도 PARTIAL 로만 남고 COMMITTED 로 승격되지 않으며, 마지막 유효 체크포인트에서 재개할 수 있다"
status: PASS
commit: 45c1b43c94a7668be9bfb29e5de30d2b8defb289
binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
  writer_bin: "target/debug/ckpt_writer.exe (dev profile, 이 커밋에서 빌드)"
protocol_versions:
  schema_version: "1"
  state_machine: "docs/protocol/state-machines.md §4"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics (GPU 무관 - 파일시스템 계층 검증)"
network_profile: "해당 없음 - 로컬 파일시스템"
command: |
  cargo build --workspace
  cargo test --workspace
  cargo test -p gputeer-checkpoint --test kill_chaos
  # 중간 상태가 실제로 발생하는지 별도 검증 (아래 raw_output 후반부)
raw_output: |
  ##### cargo test --workspace #####
  canonical_vectors     15 passed / 0 failed
  durability_chaos      16 passed / 0 failed
  kill_chaos             7 passed / 0 failed
  전체                  38 passed / 0 failed

  kill_chaos 상세:
    test kill_during_write_never_produces_corrupt_checkpoint ... ok
    test pointer_always_references_a_valid_checkpoint ... ok
    test resume_point_is_valid_and_monotonic_across_restarts ... ok
    test startup_gc_removes_partial_but_keeps_committed ... ok
    test negative_tampered_committed_checkpoint_is_rejected_from_resume ... ok
    test negative_missing_data_file_is_rejected_from_resume ... ok
    test negative_pointer_to_nonexistent_checkpoint_does_not_break_resume ... ok

  GC removed 3 partial files, kept 4 valid checkpoints

  ##### 중간 상태 발생 검증 (writer: 50 ckpt x 4 files x 64KB, 3ms delay) #####
    kill@  40ms  확정  0  PARTIAL 1  .tmp 0  예: ckpt-00000100 -> [shard-0, shard-1, shard-2]
    kill@  90ms  확정  1  PARTIAL 1  .tmp 0  예: ckpt-00000200 -> [shard-0]
    kill@ 150ms  확정  1  PARTIAL 0  .tmp 0
    kill@ 230ms  확정  3  PARTIAL 1  .tmp 0  예: ckpt-00000400 -> [shard-0, shard-1]
    kill@ 310ms  확정  3  PARTIAL 1  .tmp 0  예: ckpt-00000400 -> [shard-0, shard-1, shard-2]
    kill@ 420ms  확정  4  PARTIAL 1  .tmp 1  예: ckpt-00000500 -> [shard-0..2, shard-3.bin.tmp]
    kill@ 560ms  확정  7  PARTIAL 1  .tmp 1  예: ckpt-00000800 -> [manifest.json.tmp, shard-0..3]
    kill@ 700ms  확정  9  PARTIAL 1  .tmp 0  예: ckpt-00001000 -> [shard-0, shard-1]

  총 8회: PARTIAL 발생 7회(88%), .tmp 2회, 누적 확정 28개
artifacts:
  - docs/evidence/_raw/P0-03_probe.txt
  - crates/checkpoint/tests/kill_chaos.rs
  - crates/checkpoint/src/bin/ckpt_writer.rs
  - crates/checkpoint/src/writer.rs
negative_tests:
  - "kill_during_write_never_produces_corrupt_checkpoint: kill 시점 8개를 고정 스윕(40~700ms). 매니페스트가 존재하는데 파일 해시가 틀린 경우가 0건임을 확인"
  - "negative_tampered_committed_checkpoint_is_rejected_from_resume: 확정된 체크포인트의 1바이트를 XOR 변조하면 재개 후보에서 제외되고 더 낮은 step 으로 내려가는 것을 확인"
  - "negative_missing_data_file_is_rejected_from_resume: 매니페스트는 있으나 데이터 파일이 삭제된 체크포인트가 재개 후보에서 제외됨"
  - "negative_pointer_to_nonexistent_checkpoint_does_not_break_resume: 포인터를 존재하지 않는 id 로 덮어써도 디렉터리 스캔으로 유효 지점을 찾음 — 포인터는 힌트일 뿐 신뢰의 근거가 아님을 확인"
  - "resume_point_is_valid_and_monotonic: 같은 디렉터리에서 kill->재시작을 5회 반복해 재개 지점이 뒤로 가지 않음을 확인"
  - "★ 테스트가 공허하지 않음을 별도 검증: 8회 중 7회에서 실제 PARTIAL 이 발생했고, kill@560ms 에서는 manifest.json.tmp(매니페스트 쓰기 도중) 상태까지 포착됨"
limitations:
  - "★ 전원 차단(power loss)을 검증하지 않았다. 프로세스 kill 만 측정했으므로 OS 캐시가 살아 있는 상태다. 물리적 내구성은 증명되지 않았다"
  - "★ 복제(replica) 계층을 검증하지 않았다. REPLICATED(n) / COMMITTED 판정은 단위 테스트(durability_chaos)에서 자료구조 수준으로만 확인했고, 실제 네트워크 전송·ReplicaAck 서명은 미구현이다"
  - "NTFS 만 검증했다. ReFS/exFAT/SMB 는 미검증이며 P0-03a 의 결과가 그대로 적용되지 않을 수 있다"
  - "단일 프로세스만 썼다. 여러 writer 가 같은 root 에 동시에 쓰는 상황은 미검증이다"
  - "체크포인트 크기가 작다(4 files x 64KB = 256KB). 기준선 §41.1 이 다루는 GB 급 체크포인트에서 동작이 다를 수 있다"
  - "delta 청크 동기화를 검증하지 않았다. 기준선 §18.5 의 전송량 절감은 미측정이다"
  - "PyTorch 실학습 체크포인트가 아니라 더미 바이트다. optimizer state / RNG / sampler 위치의 실제 저장·복원은 미검증이다 (Python adapter 범위)"
  - "kill 시점이 8개 고정값이다. 더 조밀한 스윕이나 반복 실행으로 희귀 경합을 찾지 않았다"
decision: "P0-03 을 PASS 로 판정한다. 기준선 §18.2 의 확정 절차(ADR-026 반영본)가 프로세스 강제 종료에 대해 불변식 4개를 모두 지킨다. local-first 원칙(§18.1)을 재검토하지 않는다. 다만 전원 차단과 복제 계층은 미검증이므로 COMMITTED 의 durability 주장은 아직 'HASH_VERIFIED 까지' 로만 입증되었다"
---

# P0-03 · Checkpoint Durability (카오스)

## 무엇을 입증하려 했는가

기준선 §43.6 은 P0-03 실패 시 **"아키텍처 재검토 — local-first 원칙(§18.1) 자체가 흔들린다"**
로 규정한다. 남은 P0 중 실패 영향이 가장 컸다.

검증 대상은 §18.2 와 `state-machines.md` §4 가 정한 **네 가지 불변식**이다.

```text
1. 매니페스트가 존재하면 그 파일들이 전부 온전하다 (해시 일치)
2. 쓰기 중 kill 은 PARTIAL 만 남긴다. COMMITTED 로 승격되지 않는다
3. 포인터는 항상 유효한 체크포인트를 가리킨다
4. 재개는 마지막 유효 체크포인트에서 이뤄지고 단조 증가한다
```

## 어떻게 측정했는가

**별도 프로세스를 띄워 실제로 죽였다.** 같은 프로세스 안에서 예외를 던지는 것으로는
OS 수준의 중단을 재현할 수 없다.

```text
ckpt_writer  체크포인트 50개를 순차 확정. 파일당 3ms 지연으로 kill 창을 만든다
테스트       40 / 90 / 150 / 230 / 310 / 420 / 560 / 700 ms 시점에 kill
```

★ **kill 시점을 난수가 아니라 고정 스윕으로 했다.**
재현 불가능한 테스트는 실패했을 때 디버깅할 수 없다.

### 테스트가 공허하지 않은지 먼저 확인했다

카오스 테스트는 **kill 이 실제로 쓰기 도중에 걸려야** 의미가 있다.
매번 안전한 지점에서만 죽으면 아무것도 증명하지 못한다. 그래서 별도로 세어봤다.

```text
총 8회 중 PARTIAL 발생 7회 (88%)
  kill@ 40ms  ckpt-00000100 -> [shard-0, shard-1, shard-2]        4개 중 3개만 쓰인 상태
  kill@ 90ms  ckpt-00000200 -> [shard-0]                          4개 중 1개
  kill@420ms  ckpt-00000500 -> [shard-0..2, shard-3.bin.tmp]      데이터 파일 쓰는 중
  kill@560ms  ckpt-00000800 -> [manifest.json.tmp, shard-0..3]    ★ 매니페스트 쓰는 중
```

**`kill@560ms` 가 가장 위험한 순간이다** — 데이터 파일은 전부 확정됐고
매니페스트를 쓰는 도중에 죽었다. 이때 `manifest.json` 이 아니라 `manifest.json.tmp` 만
남았으므로 §18.2 규칙 3(매니페스트 없음 = PARTIAL)이 정확히 작동했다.

## 결과

```text
cargo test --workspace       38 passed / 0 failed

  canonical_vectors   15    (DoD-01)
  durability_chaos    16
  kill_chaos           7    ← 이번 추가
```

### 불변식별 결과

| 불변식 | 테스트 | 결과 |
|---|---|---|
| 1. 매니페스트 ⇒ 파일 온전 | `kill_during_write_never_produces_corrupt_checkpoint` | 8개 kill 시점 전부 손상 0건 |
| 2. kill ⇒ PARTIAL 만 | 위 + 중간 상태 검증 | `.tmp` / 불완전 파일셋만 남음 |
| 3. 포인터 유효성 | `pointer_always_references_a_valid_checkpoint` | 5개 시점 전부 통과 |
| 4. 재개 단조성 | `resume_point_is_valid_and_monotonic_across_restarts` | 5회 재시작에서 역행 0건 |

### GC 동작

```text
GC removed 3 partial files, kept 4 valid checkpoints
```

`startup_gc` 가 PARTIAL 만 제거하고 유효 체크포인트는 건드리지 않았다.

### 포인터는 신뢰의 근거가 아니다

`negative_pointer_to_nonexistent_checkpoint_does_not_break_resume` 에서
포인터를 `ckpt-99999999` 로 덮어써도 재개가 가능했다.
`find_resume_point` 가 포인터를 읽지 않고 **디렉터리를 스캔해 해시를 검증**하기 때문이다.

이것이 ADR-026 의 설계 의도와 맞는다 — 포인터는 replace-over-existing 이 필요한
유일한 파일이고 Windows 에서 실패할 수 있으므로, **포인터 손상이 재개를 막으면 안 된다.**

## 이 실험이 증명하지 "않는" 것

- **★ 전원 차단을 검증하지 않았다.** 프로세스 kill 만 했으므로 OS 페이지 캐시가 살아 있다.
  `fsync` 를 호출하고는 있으나 **물리적 내구성은 증명되지 않았다.**
  진짜 검증은 실제 전원 차단이나 가상머신 강제 리셋이 필요하다.
- **★ 복제 계층을 검증하지 않았다.** `REPLICATED(n)` → `COMMITTED` 판정은
  단위 테스트에서 자료구조 수준으로만 확인했고, **네트워크 전송·`ReplicaAck` 서명은 미구현**이다.
  따라서 `COMMITTED` 의 durability 주장은 현재 **`HASH_VERIFIED` 까지만 입증**되었다.
- **NTFS 만** 검증했다.
- **단일 writer** 만 썼다. 여러 프로세스가 같은 root 에 동시에 쓰는 경합은 미검증이다.
- **체크포인트가 작다** (256KB). 기준선 §41.1 이 다루는 GB 급에서는 다를 수 있다.
- **delta 청크 동기화 미검증** (§18.5 전송량 절감).
- **PyTorch 실학습 체크포인트가 아니다.** optimizer state · RNG · sampler 위치의
  실제 저장·복원은 Python adapter 범위이며 미구현이다.
- kill 시점이 8개 고정값이라 희귀 경합은 못 찾았을 수 있다.

## 결정

1. **P0-03 을 `PASS` 로 판정한다.** 불변식 4개가 모두 지켜졌다.
2. **local-first 원칙(§18.1)을 재검토하지 않는다.** §43.6 의 "아키텍처 재검토" 조건이 발동하지 않는다.
3. **`COMMITTED` 의 durability 주장 범위를 명확히 한다.**
   현재 입증된 것은 `HASH_VERIFIED` 까지다. `REPLICATED`/`COMMITTED` 는
   복제 계층 구현 후 `P0-03b` 로 별도 검증한다.
4. **전원 차단 검증을 `P0-03c` 로 등록한다.** 가상머신 강제 리셋으로 가능하다.

관련: `docs/decisions/ADR-026_체크포인트_확정_절차_플랫폼_차이.md` · `docs/evidence/P0-03a_windows_fs_atomicity.md`
계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S3
