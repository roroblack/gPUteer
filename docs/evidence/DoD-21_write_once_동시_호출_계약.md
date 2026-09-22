---
schema_version: 2
id: DoD-21
claim: "write_once()(crates/checkpoint/src/atomic.rs)가 같은 (dir, name) 에 대한 동시 호출을 지원하지 않고 명시적으로 거부하는 계약을 실제로 강제하도록 고쳤다 — std::fs::File::try_lock() 기반 프로세스 간 파일 잠금을 final_path.exists() 첫 검사보다 먼저 잡고, 실패 시 CheckpointError::WriteInProgress 를 즉시 반환한다(무기한 대기 없음). 성공 시(Ok(true)/Ok(false) 둘 다) 락 파일을 자가 정리해 완결된 체크포인트 디렉터리에 흔적을 남기지 않는다. gc_partial() 은 자신이 try_lock 을 직접 시도해 아무도 쥐고 있지 않은 락 파일만 회수한다(활성 writer 보호). .write_once.lock 접미사(대소문자·후행 점/공백 무관)는 validate_relative_name() 에서 예약해, 이 락 경로와 실제 데이터 파일 경로가 충돌할 수 없게 원천 차단했다"
status: PASS
commit: 78f85ceda79e4e1d971915fdf46a832834eab0e8

executor_id: "agent:claude-code"
executor_tool: "claude-code (cargo test + Edit/Write)"
executor_model: "claude-sonnet-5"
executed_at: "2026-08-19T13:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only -c model_reasoning_effort=high"
reviewer_model: "gpt-5.6-luna (OpenAI Codex v0.144.1)"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "락 획득이 첫 존재 검사보다 먼저인지·try_lock 만 쓰는지·락 파일 경로 계산이 validate_relative_name 통과 이후인지·모든 반환 경로에서 락이 해제/정리되는지, gc_partial 의 락 파일 처리(무조건 보존 -> 활성 여부로 판단하는 회수로 재설계)가 모든 삭제 분기를 실제로 막는지, k1c~k1f·k4c 신규/수정 테스트가 주장을 실제로 증명하는지, 다른 write_once 호출부(writer.rs·durability.rs·selftest.rs·ckpt_writer.rs)의 WriteInProgress 처리, MSRV 선언과 실제 사용 API 의 일치. 5라운드 진행 — 1라운드(p128) CHANGES_REQUESTED(MSRV 불일치 Cargo.toml 1.85 vs try_lock 요구 1.89, k1c 의 타이밍 의존 비결정적 주장, gc_partial 이 죽은 락을 영구 보존해 PARTIAL 디렉터리가 청소 안 되는 회귀) -> 수정 -> 2라운드(p129) CHANGES_REQUESTED(GC 대 활성 writer 의 더 넓은 경쟁 — 범위 밖으로 판단해 문서화만 함, 그리고 이름이 우연히 .write_once.lock 로 끝나는 등록된 데이터 파일을 GC 가 오인 삭제할 수 있음 — 등록 여부 검사로 1차 수정) -> 3라운드(p130) CHANGES_REQUESTED(1차 수정으로도 못 막는 근본 원인 — 락 경로와 데이터 경로 자체가 이름공간에서 충돌할 수 있음을 발견, validate_relative_name 에서 접미사 자체를 예약하도록 재수정) -> 4라운드(p131) CHANGES_REQUESTED(예약 검사가 대소문자를 구분해 Windows/NTFS 의 대소문자 무시·Win32 후행 점/공백 정규화를 우회할 수 있음 — 정규화 추가) -> 5라운드(p132) ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-21_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-21_write_once_동시_호출_계약_2026-08-19.txt"
raw_output_digest: "sha256:f28e3e2f80b292d994c2119d6a53bd66ad9c56d53f0c38731328f640004c8241"
raw_output_bytes: 5550

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 (8bab26f4f 2026-07-14)"
protocol_versions:
  schema_version: "해당 없음 — 이 조각은 crates/checkpoint 의 파일 원자성 계약만 다룬다. proto/*.proto·canonical 인코딩은 바꾸지 않는다"
  canonical_spec: "해당 없음"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관 — 이 조각은 순수 파일시스템 동작이다"
network_profile: "해당 없음 — 로컬 파일시스템 테스트만 수행한다"
command: |
  cargo test -p gputeer-checkpoint          # 5회 연속, 13개(codex_findings.rs) + 전체 53개
  cargo test --workspace --exclude gputeer-runtime-windows   # 회귀 없음, 42개 스위트 전부 통과
  cargo build (workspace 전체 rust-version 상속 확인)
raw_output: |
  (docs/evidence/_raw/DoD-21_write_once_동시_호출_계약_2026-08-19.txt 전문 참조)

  cargo test -p gputeer-checkpoint: 53 passed, 0 failed (codex_findings.rs 13개 포함)
  cargo test --workspace --exclude gputeer-runtime-windows: 42개 스위트 전부 test result: ok, FAILED/error[ 검색 결과 없음
  뮤테이션 테스트 6건 — 전부 예측한 테스트만 정확히 실패, 원복 후 재통과 확인
artifacts:
  - docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md
  - crates/checkpoint/src/atomic.rs
  - crates/checkpoint/src/lib.rs
  - crates/checkpoint/tests/codex_findings.rs
  - Cargo.toml
  - docs/evidence/_raw/DoD-21_write_once_동시_호출_계약_2026-08-19.txt
  - docs/evidence/_raw/DoD-21_review.txt
negative_tests:
  - "k1c_concurrent_same_name_writers_only_one_writer_wins — 8스레드가 Barrier 로 같은 순간에 같은 (dir, name) 을 호출해도 정확히 하나만 Ok(true), 나머지는 전부 WriteInProgress 또는 ContentMismatch(둘 다 승자를 덮어쓰지 않는 안전한 결과) — 절대 두 번째 성공이나 Ok(false) 는 없음"
  - "k1d_lock_is_released_when_holder_is_dropped_so_next_writer_proceeds — 락을 쥔 핸들을 drop 하면(crash 시뮬레이션) 다음 호출이 즉시 성공 — 무기한 대기가 되지 않는다는 crash-safety 를 결정론적으로 증명"
  - "k1e_gc_partial_reclaims_dead_locks_but_preserves_held_ones — 아무도 안 쥔 죽은 락 파일은 매니페스트 유무와 무관하게 결국 회수되고(PARTIAL 디렉터리가 영원히 안 남는다), 지금 쥐고 있는 락 파일은 매니페스트가 없어도 보존된다(활성 writer 보호)"
  - "k1f_registered_file_named_like_a_lock_file_is_preserved — 이름이 우연히 .write_once.lock 로 끝나는 매니페스트 등록 데이터 파일은 GC 가 가짜 락으로 오인해 지우지 않는다"
  - "k4c_data_file_named_like_a_lock_file_is_rejected — .write_once.lock 로 끝나는 이름(정확한 대소문자·전체 대문자·후행 점·후행 공백 4가지 변형)은 write_once() 가 즉시 UnsafePath 로 거부한다"
  - "뮤테이션 1 — try_lock 검사 무력화 -> k1c·k1d 만 정확히 예측대로 FAILED"
  - "뮤테이션 2 — gc_partial 의 락 제외를 무조건 보존으로 되돌림 -> k1e 의 '쥐고 있는 락 보존' 케이스만 FAILED"
  - "뮤테이션 3 — gc_partial 의 try_lock 결과를 무시하고 무조건 삭제 -> 뮤테이션 2 와 같은 케이스만 FAILED"
  - "뮤테이션 4 — write_once() 의 성공 시 자가 정리(cleanup_lock)를 무력화 -> durability_chaos.rs 의 기존 테스트 2건(완결된 체크포인트는 GC 가 절대 안 건드려야 한다)만 정확히 FAILED — 이 자가 정리가 왜 필요한지 기존 테스트 스스로가 회귀 가드 역할을 한다는 것을 확인"
  - "뮤테이션 5 — gc_partial 의 등록 여부(registered_tmp) 검사를 무력화 -> k1f 만 FAILED"
  - "뮤테이션 6 — validate_relative_name 의 대소문자·후행 점/공백 정규화를 원래 대소문자 구분 비교로 되돌림 -> k4c 의 'LOCK' 변형 케이스만 FAILED"
limitations:
  - "gc_partial() 은 checkpoint 디렉터리 전체를 보호하지 않는다 — write_once() 의 락은 그 호출이 쓰고 있는 한 파일만 보호한다. 매니페스트 없는(PARTIAL) 디렉터리에서 GC 가 활성 writer 와 동시에 돌면, writer 가 지금 쓰고 있는 .tmp 파일을 GC 가 지울 수 있다(코덱스 2라운드 지적, 코드 문서화만 하고 의도적으로 고치지 않음). 진짜 해법은 checkpoint 디렉터리 단위 락이며, 이는 이 조각의 명시적 범위 밖(write_checkpoint() 전체를 감싸는 락)이다. 현재 실제 호출부(startup_gc)는 프로세스 부팅 시 한 번만 돌아 이 프로세스 자신의 쓰기와 겹치지 않고, 다중 프로세스/다중 Agent 동시 접근은 아직 실제 호출 경로가 없다 — 다중 Agent 실행이 시작되는 시점이 이 문제를 다시 열어야 할 트리거다"
  - "진짜 동시 쓰기 지원(선택지 B — 호출마다 고유 tmp 이름 + 승자 판정 정책)은 하지 않는다 — 동시 동일-이름 호출은 여전히 지원되지 않고 명시적으로 거부될 뿐이다. 트리거: 실제 Agent 실행 계층이 생긴 뒤 정당한 운영 중 WriteInProgress 가 관측되거나, 동일 attempt 를 여러 Agent 가 동시에 publish 해야 하는 요구사항이 제품 계약으로 승인되는 시점"
  - "write_checkpoint() 전체(여러 파일·매니페스트·포인터 교체를 아우르는 상위 함수)를 하나의 락으로 감싸는 것은 범위 밖이다 — write_once() 자체의 계약만 고쳤다"
  - "LATEST 포인터 교체(replace_with_retry)는 이번 락과 무관하다 — validate_relative_name 은 공유하지만 파일 잠금 자체는 적용하지 않는다(포인터는 원래 replace-over-existing 방식이라 write-once 동시성 문제가 다르다)"
  - "rename() 은 성공했는데 그 다음 sync_dir() 이 실패하는 극히 드문 경우(K-3, 이전부터 문서화된 한계) 락 파일 자가 정리가 호출되지 않아 락 파일이 남는다 — 데이터는 이미 커밋됐지만 API 는 오류를 반환하는 기존 한계와 일관되게, 이 디렉터리는 매니페스트를 못 받아 PARTIAL 로 남고 gc_partial 이 나중에 회수한다. 코덱스 4라운드가 이 경로를 확인하고 기존 K-3 의미와 일관됨을 인정했다"
  - "Windows 단일 플랫폼에서만 검증했다 — Linux 에서 std::fs::File::try_lock() 이 flock(2) 기반으로 같은 crash-safety·핸들 단위 시맨틱을 갖는지는 이 세션에서 재확인하지 않았다(표준 라이브러리 문서상 flock 기반이라 이론적으로는 일치해야 하나 실측하지 않음)"
  - "write_failure.rs::concurrent_startup_gc_treats_not_found_as_normal_race 가 워크스페이스 전체 부하 아래 1/5 회 드물게 실패하는 것을 관측했다 — 이 조각의 락 처리 코드 경로(빈 디렉터리라 .write_once.lock 분기 자체가 실행되지 않는다)와 무관한, 이 저장소가 이미 알고 있던 기존 Windows 삭제 경합 플레이키함으로 판단했고 코덱스 5라운드도 동의했다. 단독 15회 재실행은 15/15 통과"
decision: "정책 A(동시 동일-이름 호출을 지원하지 않고 명시적으로 거부)를 코드로 강제했다 — 코덱스 자신의 설계 응답(p127)이 추천한 방향이다. 구현 자체는 1라운드 만에 핵심 로직(락 획득 순서·WriteInProgress 반환)은 맞았으나, 5라운드에 걸쳐 코덱스가 실제 결함 5건을 순차로 찾아냈다: MSRV 선언 불일치, 결함 고정 테스트의 비결정적 주장, GC 의 죽은 락 영구 보존으로 인한 PARTIAL 디렉터리 청소 불능, 이름공간 충돌로 인한 등록 데이터 파일 삭제 가능성(1차 부분 수정), 그리고 그 근본 원인(락 경로와 데이터 경로의 이름공간 자체 충돌, 대소문자·후행 점/공백 우회 포함). 전부 실제 코드 수정과 새 회귀 테스트(k1c 재설계 + k1d·k1e·k1f·k4c 신규 5건)로 닫았고, 뮤테이션 테스트 6건 전부로 각 방어 로직의 비공허성을 확인했다. GC 대 활성 writer 의 더 넓은 디렉터리 단위 경쟁 문제는 이번 조각(write_once() 자체의 계약)의 명시적 범위 밖으로 판단해 코드는 고치지 않고 문서화만 했다 — 계획 문서가 이미 'write_checkpoint() 전체 락은 범위 밖'이라고 예고했었다."
---

# DoD-21 · write_once() 동시 호출 계약

## 무엇을 입증하려 했는가

`write_once()`(`crates/checkpoint/src/atomic.rs`)는 같은 `(dir,
name)` 으로 동시에 호출되면 안전하지 않다는 결함이 있었다 — 모든
호출자가 같은 tmp 경로(`{name}.tmp`)를 공유해 경쟁했다(`DoD-08` 이
발견해 결함 고정 테스트로만 등록하고 코드는 안 고쳤던 것,
`ENV-03` 이 Linux 에서 증상만 다르게 재확인). 코드를 고치기 전에
"동시 동일-이름 호출을 지원할지"부터 계약으로 정해야 한다는 게 두
evidence 문서의 공통 결론이었다.

`docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md` 가 코덱스
자신의 설계 응답(`p127`)을 정리해, 정책 A(동시 호출 미지원 + 명시적
거부, 파일 잠금으로 강제)를 채택했다.

## 구현 — 1단계

`write_once()` 에 `std::fs::File::try_lock()`(Rust 1.89+ 표준
라이브러리) 기반 프로세스 간 락을 추가했다. `final_path.exists()`
첫 검사보다 먼저 락을 잡고, `try_lock()` 이 실패하면(`WouldBlock`)
새 `CheckpointError::WriteInProgress` 를 즉시 반환한다 — 무기한
대기는 장애를 숨기므로 `try_lock`(non-blocking)만 쓴다. 별도
크레이트(`fs4` 등)는 필요 없었다 — 이 저장소 실제 툴체인(1.97.1)이
`try_lock` 을 이미 안정적으로 지원한다.

## 구현 — 2단계 (GC 제외)

락 파일(`{name}.write_once.lock`)이 GC(`gc_partial`)에서 데이터
artifact 로 오인되지 않도록 처리했다.

## 구현 — 3단계 (`k1c` 재설계)

원래 결함 고정 테스트(`k1c_concurrent_same_name_writers_are_not_actually_safe`)
를 결정론적 테스트 2개로 재설계했다.

## 구현 — 4단계 (뮤테이션 테스트)

락 로직·GC 로직·자가 정리 로직·이름 검증 로직 각각을 무력화해
정확히 예측한 테스트만 실패하는지 확인 후 원복.

## 구현 — 5단계 (코덱스 5라운드 검수 — 실제로 결함 5건 발견)

여기부터가 이 evidence 의 핵심이다. **1라운드 만에 통과하지
않았고, 매 라운드가 실제 코드 결함을 찾아냈다.**

### 1라운드(`p128`) — CHANGES_REQUESTED, 3건

1. **MSRV 불일치.** `Cargo.toml` 이 Rust 1.85 를 선언했는데
   `File::try_lock()` 은 1.89+ 필요. `rust-version` 을 `1.89` 로
   올렸다(모든 크레이트가 `rust-version.workspace = true` 로
   상속받으므로 한 곳만 고치면 됐다).
2. **`k1c` 가 결정론적이지 않은 주장을 했다.** `Barrier` 는 8스레드가
   같은 순간에 **출발**하는 것만 보장하지 **도착**은 보장하지
   않는데, "패자 7개는 전부 정확히 `WriteInProgress`" 를 주장했다
   — 느린 스레드가 승자가 이미 끝난 뒤 도착하면 `ContentMismatch`
   를 받을 수 있어 스케줄링에 우연히 의존하는 주장이었다. 주장을
   "정확히 하나만 성공, 나머지는 `WriteInProgress` 또는
   `ContentMismatch`(둘 다 안전)" 로 약화하고, 결정론적인 "A 가
   쥔 동안 B 는 반드시 `WriteInProgress`" 주장은 새로 만든
   `k1d_lock_is_released_when_holder_is_dropped_so_next_writer_proceeds`
   (락을 직접 쥐고 확인해 타이밍에 안 기댄다)가 대신 증명하도록
   분리했다.
3. **`gc_partial` 이 죽은 락 파일을 영원히 안 지워서 PARTIAL
   디렉터리가 절대 청소되지 않는 회귀.** 첫 구현(무조건 보존)이
   만든 문제였다. `gc_partial` 자신이 `try_lock` 을 직접 시도해
   "지금 아무도 안 쥐고 있다" 를 확인한 뒤에만 지우도록 재설계했다.

### 2라운드(`p129`) — CHANGES_REQUESTED, 2건

1. **GC 가 활성 writer 의 다른 파일까지 지울 수 있다.** 락은 락
   파일 하나만 보호하지, 매니페스트 없는(PARTIAL) 디렉터리 전체를
   보호하지 않는다는 지적. **일부러 고치지 않았다** — 이건
   `write_checkpoint()` 전체를 감싸는 디렉터리 단위 락이 필요한,
   이 조각의 명시적 범위 밖 문제다(계획 문서가 이미 예고했다).
   대신 `gc_partial` 위에 이 한계를 명시적으로 문서화하는 doc
   comment 를 추가했다.
2. **이름이 우연히 `.write_once.lock` 로 끝나는 등록된 데이터
   파일을 GC 가 가짜 락으로 오인해 지울 수 있다.** `.tmp` 접미사가
   이미 겪던 같은 문제(등록됐으면 보존)와 같은 패턴으로, 기존
   `registered_tmp` 목록에 있으면 락 처리 분기 진입 전에 보존하도록
   1차 수정했다.

### 3라운드(`p130`) — CHANGES_REQUESTED, 1건 (근본 원인)

**1차 수정으로도 못 막는 진짜 이름공간 충돌.** `write_once(dir,
"foo", ..)` 의 락 파일 경로는 정확히 `dir/foo.write_once.lock` 이다
— 어떤 호출자가 데이터 파일 이름으로 **정확히 그 문자열**을 쓰면,
그 데이터 파일의 경로와 "foo" 호출의 락 파일 경로가 같아진다.
"foo" 쓰기가 성공해 자기 락을 정리하면 그 경로의 파일이
지워지는데, 그게 남의 진짜 데이터 파일일 수 있다 — 등록 여부
검사로는 매니페스트가 아직 없는 상태를 못 막는다. `validate_relative_name()`
에서 `.write_once.lock` 접미사 자체를 예약해 거부하도록 근본적으로
닫았다 — 이제 이 접미사로 끝나는 이름으로는 애초에 아무것도 쓸 수
없다.

### 4라운드(`p131`) — CHANGES_REQUESTED, 1건

**대소문자·후행 점/공백 우회.** 접미사 예약 검사가 대소문자를
구분해서 `foo.write_once.LOCK` 같은 이름은 안 걸렸다. NTFS 는
대소문자를 구분하지 않고, Win32 파일 API 는 레거시 DOS 호환을 위해
마지막 경로 성분의 후행 점·공백을 자동으로 잘라낸다(`...lock.` 도
결국 `...lock` 을 가리킨다). 검사 전에 `trim_end_matches(['.', ' '])`
+ `to_ascii_lowercase()` 로 정규화해 네 변형(정확한 대소문자·전체
대문자·후행 점·후행 공백) 모두 거부하도록 고쳤다.

### 5라운드(`p132`) — **ACCEPTED**

수정이 정확한 위치에 적용됐는지, 테스트가 네 변형을 전부 거부하는지,
그리고 부수적으로 관측된 무관한 플레이키 테스트(`write_failure.rs::concurrent_startup_gc_treats_not_found_as_normal_race`)
가 이번 조각의 코드 경로와 정말 무관한지(빈 디렉터리라 락 처리
분기 자체가 실행되지 않는다는 판단)까지 확인하고 동의했다.

## 결과

```text
cargo test -p gputeer-checkpoint                       53 passed, 0 failed (5회 연속)
cargo test --workspace --exclude gputeer-runtime-windows  42개 스위트 전부 통과, 0 failed
```

### 뮤테이션 테스트 6건 — 전부 비공허성 확인

| # | 무력화한 것 | 예측대로 실패한 테스트 |
|---|---|---|
| 1 | `try_lock` 검사 | `k1c`·`k1d` |
| 2 | GC 락 제외(무조건 보존으로 되돌림) | `k1e` |
| 3 | GC 의 `try_lock` 결과 무시(무조건 삭제) | `k1e` |
| 4 | 성공 시 자가 정리(`cleanup_lock`) | `durability_chaos.rs` 기존 2건 |
| 5 | 등록 여부(`registered_tmp`) 검사 | `k1f` |
| 6 | 대소문자·후행 점/공백 정규화 | `k4c` |

뮤테이션 4는 특히 흥미롭다 — 락 파일을 영원히 남기는 첫 설계가
`durability_chaos.rs` 의 **기존** 테스트 2개("완결된 체크포인트는
GC 가 절대 안 건드려야 한다")를 실제로 깨뜨렸었다. 자가 정리를
추가해 고쳤고, 그 기존 테스트 2개가 이제 이 자가 정리 로직의
회귀 가드 역할을 겸한다.

## 이 실험이 증명하지 "않는" 것

- 진짜 동시 쓰기 지원(선택지 B)은 여전히 없다 — 동시 호출은
  거부될 뿐이다.
- GC 대 활성 writer 의 디렉터리 단위 경쟁은 여전히 열려 있다
  (범위 밖, 문서화만 함).
- `write_checkpoint()` 전체를 감싸는 락은 범위 밖이다.
- Linux 에서 `try_lock` 의 crash-safety·핸들 단위 시맨틱을 실측
  재확인하지 않았다.

## 결정

1. 정책 A(동시 동일-이름 호출 미지원 + 명시적 거부)를 코드로
   강제했다.
2. 코덱스 5라운드 검수가 실제 결함 5건(MSRV·비결정적 테스트
   주장·GC 영구 보존 회귀·이름공간 충돌 1차·이름공간 충돌 근본
   원인+대소문자)을 순차로 찾아냈고, 전부 코드 수정과 새 회귀
   테스트로 닫았다.
3. GC 대 활성 writer 의 더 넓은 경쟁 문제는 계획 문서가 이미
   예고한 범위 밖 결정과 일관되게, 코드가 아니라 문서로만
   남겼다 — 다중 Agent 실행이 시작되는 시점이 다시 열어야 할
   트리거다.

관련: `docs/evidence/DoD-08_독립검수_시정.md` ·
`docs/evidence/ENV-03_remote5090_리눅스_GPU_기계_실측.md` ·
`docs/plans/2026-08-19_1200_write_once_동시_호출_계약_v1.md`
