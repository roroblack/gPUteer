---
schema_version: 2
id: P0-03
claim: "체크포인트가 쓰기 도중 프로세스 강제 종료되어도 PARTIAL 로만 남고 COMMITTED 로 승격되지 않으며, 마지막 유효 체크포인트에서 재개할 수 있다"
status: PASS
commit: 6e9ac19f46ea531d7535cab863204d3802df5826

executor_id: "agent:claude-code"
executor_tool: "claude-code (Bash + cargo)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-18T00:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "문서 전체(원본 claim·negative_tests·limitations 8건) 재검수 · kill 메커니즘이 시간 기반→이벤트 기반으로 바뀐 것 정밀화 · 손상 없음 불변식 재확인 · cargo test 재실행 확인"
review_artifact: "docs/evidence/_raw/P0-03_review.txt"

raw_output_artifact: "docs/evidence/_raw/P0-03_v2_promotion_2026-08-18.txt"
raw_output_digest: "sha256:b2778486c55b71b726d0030b7d2f259c04889f076719b9d5ae68018373b4eddc"
raw_output_bytes: 1235

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
  - docs/evidence/_raw/P0-03_v2_promotion_2026-08-18.txt
  - docs/evidence/_raw/P0-03_review.txt
negative_tests:
  - "kill_during_write_never_produces_corrupt_checkpoint: kill 시점 8개를 고정 스윕(40~700ms). 매니페스트가 존재하는데 파일 해시가 틀린 경우가 0건임을 확인"
  - "negative_tampered_committed_checkpoint_is_rejected_from_resume: 확정된 체크포인트의 1바이트를 XOR 변조하면 재개 후보에서 제외되고 더 낮은 step 으로 내려가는 것을 확인"
  - "negative_missing_data_file_is_rejected_from_resume: 매니페스트는 있으나 데이터 파일이 삭제된 체크포인트가 재개 후보에서 제외됨"
  - "negative_pointer_to_nonexistent_checkpoint_does_not_break_resume: 포인터를 존재하지 않는 id 로 덮어써도 디렉터리 스캔으로 유효 지점을 찾음 — 포인터는 힌트일 뿐 신뢰의 근거가 아님을 확인"
  - "resume_point_is_valid_and_monotonic_across_restarts: 같은 디렉터리에서 kill->재시작을 5회 반복해 재개 지점이 뒤로 가지 않음을 확인"
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

---

## ★ 이후 변경 (2026-08-17 22:40) — 재개 정책이 이 문서 이후 바뀌었다

독립 검수(`agent:codex-cli`, read-only, P0-03 재검수 목적)가 위 claim의
2번째 절("마지막 유효 체크포인트에서 재개할 수 있다")이 **지금의
재개 정책과 어긋난다**고 지적했다. 재현했다 — 맞았다.

### 무엇이 바뀌었나

이 문서를 쓴 시점(commit `6e9ac19`)의 재개 판정은 이 문서가 관측한
그대로였다. 그 뒤 `writer.rs::is_resume_candidate()` 가 재설계됐다
(2026-08-17, 별도 세션 — DoD-04/P0-03 재검수와 무관하게 카오스 테스트가
부하 아래서 재개 지점을 통째로 잃는 문제를 잡아서 고친 것).

```text
기록 순서
  데이터 -> 매니페스트 -> HASH_VERIFIED -> LATEST 교체 -> COMMITTED
                                                           ^^^^^^^^^
                          여기 직전에 kill 되면 COMMITTED 마커가 없다
```

**이 문서 위쪽의 불변식 2번("kill ⇒ PARTIAL 만 남는다, COMMITTED 로
승격 안 된다")은 문자 그대로는 지금도 참이다** — kill 은 지금도
COMMITTED 마커를 만들지 않는다. 그러나 이 문서는 "PARTIAL"과
"COMMITTED" 두 상태만 다뤘다. **세 번째 상태가 있다**: 데이터·매니페스트·
해시가 전부 온전한데 COMMITTED 마커만 없는 상태. 지금의
`is_resume_candidate()` 는 `.publication-failed` 마커만 없으면 이
상태도 재개 후보로 받아들인다(마커를 요구하지 않는 이유는
`writer.rs:163` 이하 문서 주석 참조 — `CLAUDE.md` §0.3, 완결된 데이터를
COMMITTED 마커 하나 때문에 버리지 않는다는 의도적 설계다).

즉 claim의 정확한 의미는 지금 이렇게 좁혀 읽어야 한다:

> kill 은 COMMITTED 승격을 만들지 않는다. 재개는 "COMMITTED 된
> 체크포인트" 가 아니라 **"해시가 유효한 가장 높은 체크포인트"** 에서
> 이뤄진다 — 그 체크포인트가 COMMITTED 마커까지 받았는지는 재개
> 판정에 관여하지 않는다.

### 이 문서의 kill 스윕이 그 경계를 실제로 때렸는지는 확인되지 않았다

위 "테스트가 공허하지 않은지" 절의 8개 표본은 전부 **데이터 파일이
빠졌거나(`shard-N` 누락) `.tmp` 상태**였다 — `HASH_VERIFIED` 이후,
`COMMITTED` 이전의 **온전한** 상태(파일 다 있고 매니페스트도 완결,
마커만 없음)를 잡은 표본은 raw_output 에 없다. 그 구간은 기록 순서상
LATEST 교체 한 번과 상태 마커 기록 한 번 사이의 좁은 창이라, 40~700ms
간격의 고정 스윕이 우연히 맞히지 못했을 수 있다.

★ **위 문단을 쓴 지 얼마 지나지 않아 그 구간을 직접 겨냥하는 테스트를
추가했다** — `crates/checkpoint/tests/kill_chaos.rs` 의
`kill_after_latest_before_committed_is_resume_candidate` (`chaos-hooks`
feature 전용). 시간 스윕이 아니라 **코드 순서**로 겨냥한다:
`crates/checkpoint/src/writer.rs` 의 `replace_with_retry` 성공 직후 ·
`Committed` 상태 기록 직전에만 켜지는 `chaos_kill_after_latest()` 훅이
`std::process::abort()` 로 그 자리에서 프로세스를 끝낸다(코드 순서상
그 이후로는 `Committed` 기록 경로에 도달할 수 없다 — race 가 없다).

```text
명령: cargo test -p gputeer-checkpoint --features chaos-hooks \
        --test kill_chaos kill_after_latest_before_committed_is_resume_candidate \
        -- --exact --nocapture
결과: 8회 연속 실행 — 8/8 통과 (2026-08-17)

검증한 사후 상태:
  CHAOS_AFTER_LATEST ckpt-00000100  stdout 에서 관측(신호가 실제로 왔다)
  COMMITTED ...                     stdout 에 없음(self-kill 이 그 전에 끝냈다)
  LATEST                            ckpt-00000100 을 가리킴
  manifest.json                     존재, verify_files() 통과
  .durability.hash-verified         존재
  .durability.committed             ★ 없음 — 이것이 이 테스트의 핵심 관측
  .publication-failed               없음
  find_resume_point_for(root, "job-chaos", "att-1")
                                     ckpt-00000100 / step 100 반환 — COMMITTED
                                     마커 없이도 재개된다
```

**뮤테이션으로 비공허성 확인**: `chaos_kill_after_latest(&dir)` 호출을
`record_state_transition(..., Committed)` **뒤로** 옮기면(= 훅이 늦게
발동하는 결함을 흉내낸다), `!state_recorded(&dir, DurabilityState::Committed)`
단언이 정확히 실패로 뒤집힌다("self-kill 전에 이미 Committed 가
기록됐다"). 원복 후 8/8 재확인.

이제 이 절 서두의 "claim의 정확한 의미" 문단은 관측으로 뒷받침된다 —
더 이상 코드 주석과 설계 의도만 가리키는 것이 아니라, 그 정확한 구간을
직접 만들어 재개가 실제로 되는 것을 본 것이다.

**아직 남은 것**: 이 테스트 자체가 독립 검수를 거치지 않았다. 아래
"review_outcome" 은 이 확인이 반영되기 전(2026-08-17 22:40) 판정이다 —
재검수가 이 절을 확인한 뒤에야 최종 ACCEPTED 여부가 정해진다.

★ 2026-08-17 23:20 추가 — `agent:codex-cli` 의 4건 일괄 최종 재검수가
이 addendum 을 `ACCEPTED` 로 판정했다. 코드에서 직접 확인한 것:
`chaos-hooks` 는 `default` feature 에 없다(`Cargo.toml:8-15`), self-kill
함수는 `replace_with_retry` 성공 **직후** · `Committed` 기록 **이전**에
정확히 위치한다(`writer.rs:129-139` → `writer.rs:141-145` 사이),
새 테스트의 단언들이 LATEST·HashVerified·Committed 부재·재개 반환을
전부 검사한다(`kill_chaos.rs:216-262`). 다만 이 세션이 주장한 "8회
연속 실행" 결과 자체는 검수자의 read-only 샌드박스에서 재실행하지
않았으므로 **독립 재현은 확인 안 됨** — 코드 구조와 단언 로직의
정확성만 ACCEPTED 의 근거다.

```text
executor_id:      agent:claude-code
executor_tool:    claude-code (PowerShell + cargo)
reviewer_id:      agent:codex-cli
reviewer_tool:    codex exec --sandbox read-only -c model_reasoning_effort=high
review_context:   fresh-read-only
review_outcome:   ACCEPTED (이 addendum 에 한정 — 원본 P0-03 v1 claim 전체를
                   ACCEPTED 로 재분류하는 것이 아니다)
review_scope:     writer.rs 의 chaos_kill_after_latest 호출 위치,
                   Cargo.toml 의 chaos-hooks feature 격리,
                   kill_chaos.rs 의 새 테스트 단언 논리
```

이 addendum 은 독립 검수를 통과했지만, `P0-03` 문서 전체를 schema v2
로 승격하려면 원본 claim·나머지 limitations 도 같은 수준으로 재검수
받아야 한다 — 그 작업은 아직 하지 않았다.

---

## ★ 이후 변경 (2026-08-18) — 문서 전체 재검수, kill 메커니즘이 시간 기반에서 이벤트 기반으로 바뀌었다

위 문단이 예고한 "문서 전체 재검수"를 이번에 했다. 새로운 독립
검수(`agent:codex-cli`, read-only, `p87` 프롬프트, 원본 YAML
claim·negative_tests·limitations 전부를 대상)가 `CHANGES_REQUESTED`
로 판정했다 — 2026-08-17 addendum 이 이미 좁힌 claim 읽기는
정확하다고 재확인했지만, **원본 raw_output 의 "kill@40~700ms,
PARTIAL 7/8" 표가 지금의 테스트 구현과 더 이상 대응하지 않는다**
는 새 지적을 받았다.

### 1. claim — 2026-08-17 좁힌 읽기가 재확인됐다

`write_checkpoint()` 는 `LATEST` 교체 후 `Committed` 를 기록한다
(`crates/checkpoint/src/writer.rs:129,141`). 그 사이 kill 되면
데이터·매니페스트·해시는 온전한데 `.durability.committed` 만
없다. `is_resume_candidate()` 는 그 마커를 요구하지 않고
`.publication-failed` 만 배제한다(`writer.rs:241,246`), 유효
매니페스트를 해시 검증해 가장 높은 step 을 고른다(`writer.rs:264,332`).
정확한 claim: **"kill 은 COMMITTED 기록을 만들지 않으며, 재개는
COMMITTED 여부와 무관하게 해시가 유효한 가장 높은 체크포인트에서
이뤄진다"** — 2026-08-17 addendum 의 읽기와 동일, 재확인됐다.
원본 YAML `claim`(`P0-03:3`)은 그대로 두고 고치지 않는다
(append-only 원칙).

### 2. [새 지적] "kill 시점 8개 고정값" — 테스트 메커니즘이 그 사이 바뀌었다

`crates/checkpoint/tests/kill_chaos.rs` 는 여전히
`[40, 90, 150, 230, 310, 420, 560, 700]` 8개 값을 순회한다
(`kill_chaos.rs:271`). 그러나 그 값을 쓰는 방식이 원래 evidence를
쓴 시점(commit `6e9ac19`)과 지금은 다르다:

```text
당시(원본 raw_output)   각 ms 값을 실제 대기 시간으로 써서
                        그 시각에 프로세스를 kill 했다 —
                        kill@40ms, kill@90ms ... 마다 다른
                        진행 상태에서 죽었다(PARTIAL 7/8 관측).

지금(kill_chaos.rs:150-154,126-140)
                        run_and_kill() -> run_and_kill_after(root,
                        min_committed=1, hard_timeout=ms*20) 로 바뀌었다.
                        ms 값은 이제 "실제 kill 시각"이 아니라
                        hard_timeout 상한(ms*20)을 정할 뿐이고,
                        진짜 kill 시점은 stdout 에서 "COMMITTED"
                        를 1회 관측한 직후다 — 8개 반복 전부
                        사실상 같은 이른 시점(첫 확정 직후)에서
                        죽는다.
```

**이것은 결함이 아니다** — 이 리팩터(2026-08-17, 다른 세션)는 카오스
테스트가 부하 아래서 재개 지점을 통째로 잃는 문제를 잡으려고
시간 대신 이벤트로 동기화하도록 바꾼 것이다(위 "무엇이 바뀌었나"
절 참조). 두 불변식(손상 0건, `valid >= committed`)은 지금도
매 kill 마다 확인된다(`kill_chaos.rs:280-292`). **그러나 원본
raw_output 의 "총 8회: PARTIAL 발생 7회(88%)" 통계와 개별 kill 시점
표(`P0-03:38-48`)는 당시 시간 기반 구현의 관측이지, 지금 이벤트
기반 구현이 재현하는 것이 아니다.**

→ 그 표는 **역사적 기록**으로 원문 그대로 둔다(append-only) —
지금 그 정확한 88% 수치나 개별 kill 지점 상태를 재현한다고
주장하지 않는다. "테스트가 공허하지 않은가"(kill 이 실제로 쓰기
도중 걸리는가)라는 원래 질문 자체는 지금도 유효한 질문이고,
`valid >= committed` 부등식 검사와 `corrupt == 0` 단언이 그
공허하지-않음을 지금 방식대로 계속 보증한다 — 다만 "몇 %가
PARTIAL 이었는가"는 더 이상 이 테스트가 answer 하지 않는다.

### 3. negative_tests, limitations 8건 — 재확인, 전부 실재/유효

negative_tests 5개 함수 전부 실재 확인(`kill_chaos.rs:269,329,
385,411,430`). addendum 이 이미 지적한 "구 API(`find_resume_point`,
job/attempt 필터 없음) 를 대부분의 negative test 가 쓴다"도
재확인(`kill_chaos.rs:342,390,399,416,420,440`, 구 API 자체는
`writer.rs:336,339`) — `DoD-09` 가 다루는 교차 오염 위험이 이
문서 범위에서 검증되지 않았다는 좁힌 읽기는 여전히 정확하다.

limitations 8건(전원 차단 미검증·복제 계층 자료구조 수준뿐
(`durability.rs:260,286`)·NTFS 단일·단일 writer(`kill_chaos.rs:102`)·
소형 체크포인트(`kill_chaos.rs:104,106`)·delta 청크 미검증
(`ckpt_writer.rs:31,35`)·PyTorch 실학습 아님(`ckpt_writer.rs:31,39`)·
kill 시점 8개 고정값) 전부 지금도 실재/유효함을 재확인했다 — 마지막
항목은 위 2번 절이 이미 "값의 의미가 바뀌었다"로 정밀화했다.

### Rust 재실행 — 이 세션에서 직접 확인, Codex 샌드박스에서는 못함

Codex read-only 샌드박스는 `.cargo-build-lock` 접근이 거부돼 세
명령을 하나도 실행하지 못했다(샌드박스 제약이지 코드 결함이
아니다). 이 세션은 이미 로컬에서 직접 실행해 확인했다 —
`docs/evidence/_raw/P0-03_v2_promotion_2026-08-18.txt` 가 그
receipt 다: `kill_chaos` 7 passed, chaos-hooks 대상 테스트 3회
연속 통과, `cargo test --workspace` 307 passed / 0 failed.

### review_outcome

`CHANGES_REQUESTED` — 위 2번(kill 메커니즘 변화 명시)을 이
addendum으로 반영했다. 좁은 범위의 후속 확인을 별도로 요청해
`ACCEPTED` 를 받은 뒤에만 schema v2 로 승격한다.

### 추가 limitation

- ★ `find_resume_point()`(job/attempt 필터 없는 구 API)가 `kill_chaos.rs`
  의 negative test 대부분에서 여전히 쓰인다. 필터가 있는 새 기본 API는
  `find_resume_point_for()` 다 — 이 문서의 negative_tests 는 구 API
  기준이므로 `DoD-09_재개선택_필터.md` 가 다루는 job/attempt 교차 오염
  위험을 이 문서 범위에서는 검증하지 않은 것으로 읽어야 한다.

### review_outcome

`CHANGES_REQUESTED` → 위 내용으로 claim 의 두 번째 절을 좁혀 읽는다는
정정과, HASH_VERIFIED~COMMITTED 구간을 직접 겨냥한 테스트가 아직
없다는 사실을 반영해 재검수를 요청했다. 원본 YAML `claim`·`status`·
`limitations` 는 당시 기록이므로 고치지 않는다.

관련: `docs/evidence/DoD-09_재개선택_필터.md`
