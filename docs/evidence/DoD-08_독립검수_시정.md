---
schema_version: 2
id: DoD-08
claim: "독립 적대적 검수(Codex CLI, 2026-08-16)가 지적한 결함 7건을 실측으로 확인하고 시정했다. 그 중 3건은 Rust 와 Python 참조 구현이 동일하게 틀려 벡터 대조로는 잡히지 않던 것이다. 검증 도구 자체의 결함 1건(--verify 가 재생성 대조를 하지 않음)도 시정했다"
status: PASS
commit: 44ce31a6ea75508adc2688416fc370e81c25f6c6

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
review_scope: "claim 재확인(C-1/C-3/D-1·--verify·replay nonce 결속·K-2) · K-1 잔여 결함(경쟁 분기 내용 비교 누락) 발견·수정 · 동시 호출 안전성 한계 신규 테스트 · cargo test 재실행 확인"
review_artifact: "docs/evidence/_raw/DoD-08_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-08_v2_promotion_2026-08-18.txt"
raw_output_digest: "sha256:282c2211dc17d37c09cc42eac56b87d6a38a28a2f40b1b0f9a32b1a33a33a70c"
raw_output_bytes: 2467

binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 / python 3.12.7 / codex-cli 0.144.1"
  note: "라이브러리 크레이트라 실행 바이너리 없음"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1 (규칙 i-2 · i-3 · c-2 신설)"
  vectors: "tests/vectors/canonical_v1.json (40건, 36 -> 40)"
  schema_fingerprint: "blake3-256 0a34709f5599658f071ed8ac6df7c0ccca441cba89ce640db9ce061aa7866bcf (변경 없음 — .proto 를 건드리지 않았다)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "해당 없음 - 로컬 단위 테스트"
command: |
  codex exec --sandbox read-only -c model_reasoning_effort=high < <프롬프트>
  cargo test --workspace -- --nocapture
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  cargo build --workspace --all-targets
raw_output: |
  === 지적 확인 (시정 전) ===
  C-1  Message({})       ->        (0B)   생략
       Message({90:sig}) -> 0a00   (2B)   ★ 빈 중첩 메시지로 출력
       Python 도 동일:  manifest={} -> 0801 / manifest={90:sig} -> 08011a00

  K-1  write_once(d,"shard-0.bin",b"A") -> Ok(true)
       write_once(d,"shard-0.bin",b"B") -> Ok(false)   ★ 내용 비교 없음

  K-2  RetryPolicy{max_attempts:0} -> panicked at atomic.rs:139

  === 시정 후 ===
  python reference_canonical.py --self-test    all checks passed
  python reference_canonical.py --verify       재생성 대조: 40개 벡터 일치
                                               vector cross-checks: OK

  뮤테이션(관계 없는 벡터 v21 변조):
    FAIL 저장본이 구현과 어긋난다 (재생성 필요): v21_checkpoint_manifest.canonical_hex
    vector cross-checks: FAILED
  원복 후: 재생성 대조: 40개 벡터 일치

  cargo test --workspace
    codex_findings (protocol)      6 passed
    codex_findings (checkpoint)    6 passed
    replay_binding                10 passed
    t1b_grant_and_control         12 passed
    (그 외 기존 테스트 전부)
    전체: 167 passed / 0 failed
  cargo build --all-targets  경고 0건
  기존 벡터 canonical 변경 0건
artifacts:
  - docs/evidence/_raw/DoD-08_test_output.txt
  - crates/protocol/tests/codex_findings.rs
  - crates/checkpoint/tests/codex_findings.rs
  - crates/crypto/tests/replay_binding.rs
  - crates/protocol/src/canonical.rs
  - crates/protocol/src/signing.rs
  - crates/checkpoint/src/atomic.rs
  - tools/canonical/reference_canonical.py
  - docs/evidence/_raw/DoD-08_v2_promotion_2026-08-18.txt
  - docs/evidence/_raw/DoD-08_review.txt
negative_tests:
  - "★ c1_signature_only_nested_message_leaks_into_canonical: 중첩 메시지가 서명 필드만 가질 때 canonical 이 달라지는지. 시정 전 0a00(2B) vs 생략(0B)로 실패했다"
  - "c1b_top_level_signature_field_is_correctly_excluded: 최상위는 원래 정상이었음을 고정 — 중첩만 문제였다는 사실을 남긴다"
  - "★ c1c_derived_hash_exclusion_is_top_level_only: D-1 시정 후 중첩 field 4 가 **진짜 필드로 취급되는지**. 이 테스트는 원래 반대로 쓰여 있었고, 시정 후 실패해서 전제가 틀렸음이 드러났다 — 약화하지 않고 올바른 의미로 다시 썼다"
  - "★ d1_derived_hash_fields_must_not_apply_to_nested_messages: DatasetRef.retention(field 4)이 manifest_hash(field 4)로 오인돼 서명에서 빠지는지"
  - "encoder_always_emits_minimal_varint: 디코더의 non-minimal 거부와 별개로 **인코더가 항상 최단을 내는가**. 경계값 7종 + 규칙 j 음수 3종"
  - "★ caller_cannot_choose_a_different_nonce: 서명 후 nonce 를 바꾸면 InvalidSignature. nonce 가 서명 대상임을 증명한다"
  - "★ failed_verification_does_not_consume_the_nonce: 서명 실패·만료·skew 위반이 캐시를 채우지 않는지. 채우면 공격자가 정상 nonce 를 소진시킬 수 있다(DoS)"
  - "★ cache_full_rejects_instead_of_evicting: §10 상한 도달 시 기존 유효 nonce 가 살아 있는지. 축출하면 replay 창이 열린다"
  - "storage_failure_is_distinguishable_from_replay: 저장소 장애 3종이 VerifyOutcome 으로 보고되지 않는지(outcome() == None). '재전송 공격'과 '디스크 장애'가 같은 값이면 운영자가 엉뚱한 곳을 본다"
  - "storage_failure_fails_closed: 장애 시 Verified 가 만들어지지 않는지"
  - "signer_can_issue_a_new_grant_with_a_fresh_nonce: guard 가 무조건 거부하는 게 아님을 확인 (비공허성)"
  - "no_replay_check_and_working_guard_differ: NoReplayCheck 와 실제 guard 가 다르게 동작하는지"
  - "★ k1_write_once_must_detect_content_mismatch: 같은 이름·다른 내용을 조용히 받아들이는지. 시정 전 Ok(false) 로 실패했다"
  - "k1b_write_once_is_idempotent_for_identical_content: 같은 내용은 여전히 멱등한지 — '무조건 거부'하는 구현을 막는다"
  - "★ k2_zero_attempts_returns_error_not_panic: max_attempts=0 이 panic 하는지. 시정 전 atomic.rs:139 에서 패닉했다"
  - "k2b_normal_policy_still_works: 정상 정책은 여전히 동작하는지 (비공허성)"
  - "★ --verify 뮤테이션: MUST_EQUAL/DIFFER 관계가 없는 벡터(v21) 1건을 변조해 재생성 대조가 **독립적으로** 발동하는지 확인"
  - "manifest_hash_formula_matches_spec: §6.1 공식이 실제로 성립하는지 + 서명 필드를 채워도 해시가 변하지 않는지(순환 방지) + 내용이 바뀌면 해시도 바뀌는지(비공허성)"
  - "grant_manifest_hash_can_be_wrong_without_breaking_signature: 규칙 i 로 제외되므로 Grant 서명이 manifest_hash 를 보증하지 않음을 고정 — 통과가 곧 '프로토콜이 막지 못한다'는 뜻이다"
limitations:
  - "★ 검수자가 지적한 것 중 **고치지 않은 것이 있다.** (a) RevokeLeaseNotice 를 Evidence 로 둔 것 — 같은 fence_epoch 의 회수 통지 반복 전송을 막지 못한다. 멱등하다고 판단했으나 **실측하지 않았다** (b) 증거 메시지 3종의 서명자 ID 대체값 — V-08 (c) ReplicaAck 의 fence_epoch 부재 — V-07. 셋 다 .proto 변경이 필요해 schema_version 상향과 함께 처리해야 한다"
  - "★ §8 의 5·6단계 순서가 규범과 구현이 다르다는 지적을 **수용하지 않고 유지했다.** 규범은 '5. Ed25519 검증 -> 6. 서명자 신원' 순인데, 실제로는 키를 찾아야 서명을 검증할 수 있다. 따라서 길이가 맞는 위조 서명을 가진 unknown signer 는 UnknownSigner 를 받는다. 둘 다 거부이므로 안전성은 같으나 규범 문구와 구현이 어긋나 있다 — 규범을 고쳐야 하는지 별도 판단이 필요하다"
  - "K-3(rename 성공 후 sync_dir 실패)은 현재 동작이 옳다고 판단했으나 **실패 주입 테스트를 하지 못했다**(권한 조작 필요). 코드 검토로만 확인했고 그 사실을 테스트 주석에 적었다"
  - "★ write_once 의 내용 대조는 파일 전체를 메모리로 읽는다. 체크포인트 shard 가 수 GB 인 경우 문제가 될 수 있다 — 측정하지 않았다. 스트리밍 비교나 해시 비교로 바꿔야 할 수 있다"
  - "Rust 의 public `Fields` API 는 규칙 h(알 수 없는 필드 제외)를 강제하지 않는다. `f.set(999, ..)` 를 하면 출력된다. `to_fields` 경로에서는 알려진 필드만 넣으므로 현재 문제가 아니지만, `canonical_encode` 자체가 규칙 h 를 보장한다고 볼 수 없다"
  - "벡터 커버리지 구멍이 남아 있다 — 규칙 a(필드 삽입 순서 무관), 규칙 c(UTF-8 경계 키), 규칙 g(float 거부), 규칙 h(unknown field), 규칙 j(int32 음수), §4(길이 0 · 127/128 경계)"
  - "검수는 Codex CLI(gpt-5.6-luna) 한 모델로만 받았다. 다른 검수자는 다른 것을 볼 수 있다"
  - "Windows 단일 플랫폼"
decision: "독립 검수 지적 중 실측으로 성립이 확인된 7건을 시정했다. signing.md 에 규칙 i-2 · i-3 · c-2 를 신설했으나 **기존 벡터 canonical 은 하나도 바뀌지 않았다** — 회귀 없이 규범 구멍만 메웠다. .proto 변경이 필요한 3건(V-07 · V-08 · RevokeLeaseNotice)은 schema_version 상향과 함께 처리한다. §8 5·6단계 순서 문제는 규범 수정 여부를 별도 판단한다"
---

# DoD-08 · 독립 검수 지적의 시정

## 왜 이 검증이 필요했는가

`CLAUDE.md` §4 — "중요한 판단은 **독립 검수자와 교차검증**한다. 반박당하면 실측으로 가린다."

이 세션의 결정들(ADR-028 · ADR-029, canonical 규칙, 서명 검증)은 **전부 내가 혼자
판단하고 내가 만든 테스트로 검증한 것**이다. 그 테스트가 놓친 것을 그 테스트로는
찾을 수 없다.

Codex CLI 에 **적대적 검토**를 맡겼다 — 동의가 아니라 **반박**을 요청했다.

## ★ 가장 중요한 결과 — 두 구현이 똑같이 틀렸다

`DoD-01` 이래 나는 "Rust 와 Python 참조 구현이 바이트 단위로 일치한다" 를
검증의 근거로 삼아 왔다. 검수자가 그 전제를 공격했다.

> **두 구현이 같은 바이트를 내지만 둘 다 signing.md 규칙을 어기는 경우를 찾아 주세요.
> 벡터 대조는 이런 경우를 잡지 못합니다.**

**찾았다. 3건이다.**

### C-1 — 규칙 i 가 중첩에서 새어나갔다

```text
Message({})        ->        (생략, 0B)
Message({90: sig}) ->  0a00  (빈 중첩 메시지로 출력, 2B)
```

규칙 i 는 "서명 필드가 canonical 에 **영향을 주지 않는다**" 를 뜻한다.
그런데 `is_default()` 검사가 field 90 제외보다 **먼저** 일어나,
서명 필드만 가진 중첩 메시지가 "비어 있지 않다" 로 판정됐다.

Python 도 같은 순서였다.

```text
Python  manifest={}        -> 0801
Python  manifest={90:sig}  -> 08011a00
```

→ **규칙 i-2 신설**: 중첩 메시지는 먼저 재귀 인코딩하고 결과가 비면 필드를 생략한다.
`repeated` 는 원소를 버리지 않는다(규칙 d — 순서가 의미를 갖는다).

### C-3 — map 엔트리 안의 규칙 b 가 정의되지 않았다

`{"k": ""}` 를 어떻게 인코딩하는가? 규범에 없었다.
proto3 map 시맨틱에서 "값 없음" 과 "빈 값" 은 같으므로 규칙 b 의 논리가 그대로 적용된다.

→ **규칙 c-2 신설**. 키의 존재 자체는 정보이므로 엔트리는 남는다.

### D-1 — 도출 해시 제외가 재귀 적용됐다

```text
ExecutionGrant.manifest_hash              = field 4   -> 제외해야 한다
ExecutionGrant.manifest.dataset.retention = field 4   -> 제외하면 **안 된다**
```

**field number 는 메시지마다 의미가 다르다.**
`canonical_encode(&f, &[4])` 가 **데이터셋 삭제 정책을 서명에서 지웠다.**

→ **규칙 i-3 신설**: 도출 해시 제외는 최상위에만. 서명 필드(90)는 모든 메시지에서
같은 의미이므로 재귀 유지.

## ★★ 검증 도구 자체가 검증하지 않고 있었다

가장 뼈아픈 지적이다.

> Python 의 `--verify` 는 실제 canonical 을 재생성하지 않습니다.
> JSON 에 저장된 `canonical_hex` 끼리 `MUST_EQUAL/DIFFER` 관계만 확인합니다.

**맞다.** 저장본이 구현과 어긋나도 통과한다.

그런데 나는 `DoD-01` 이래 evidence 문서마다
`vector cross-checks: OK` 를 **검증 근거로 인용해 왔다.**
실제보다 강한 보증처럼 읽혔다.

→ `build_vectors()` 를 재실행해 40개 벡터를 바이트 대조하도록 고쳤다.

```text
뮤테이션: 관계(MUST_EQUAL/DIFFER)가 없는 벡터 v21 의 canonical_hex 를 변조
결과:    FAIL 저장본이 구현과 어긋난다 (재생성 필요): v21_checkpoint_manifest.canonical_hex
원복:    재생성 대조: 40개 벡터 일치
```

관계 검사에 가려지지 않고 **재생성 대조가 독립적으로 발동**함을 확인했다.

## ★★ replay nonce 가 메시지와 결속되지 않았다

가장 보안 영향이 큰 지적이다.

```rust
// 예전
verify(msg, ..., nonce: Option<&[u8]>, replay)
```

**호출자가 nonce 를 골라서 넘겼다.** 메시지 안에도 nonce 필드가 있고
그것은 서명 대상인데, `verify()` 가 그것을 쓰지 않았다.

```text
=> 서명은 통과하는데 replay 방어만 무력화된다.
   매번 새 값을 넘기면 같은 메시지를 몇 번이든 재생할 수 있다.
```

→ `Signable::replay_nonce()` 신설. nonce 는 **서명된 메시지 필드**에서 온다.
바꾸려면 메시지를 바꿔야 하고 그러면 서명이 깨진다.

검수자의 조언이 결정적이었다.

> 핵심 우선순위는 `SQLite 선택` 보다 먼저 `verify()` 의 nonce 결속과
> `ReplayGuard` 의 오류/보존기간 API 를 고치는 것입니다.
> **지금 상태에서 SQLite 만 추가하면 잘못된 외부 nonce 를 영속 기록하는 구현이 됩니다.**

저장소 구현(T4)에 들어가기 **전에** 계약을 고친 것이 이 지적 덕분이다.

### 부수 개선

```text
ReplayGuard -> Result<ReplayDecision, ReplayStoreError>
              저장소 장애 · 락 타임아웃 · 캐시 포화를 "이미 봤다" 와 구분
VerifyError  프로토콜 결과와 로컬 장애를 분리
              섞으면 "재전송 공격" 과 "디스크 장애" 가 같은 값이 된다
retain_until §10 의 보존 시한을 guard 에 전달
```

## checkpoint — 아직 검토받은 적 없던 크레이트

### K-1 — `write_once` 의 전제가 지켜지지 않았다

주석은 이렇게 적혀 있었다.

> content-addressed 이름이므로 존재한다는 것은 내용이 같다는 뜻이다.

그런데 `writer.rs` 가 넘기는 이름은 `shard-0.bin` — **위치 기반**이다.

```text
1. writer-A 가 shard-0.bin 을 쓴다
2. 매니페스트 쓰기 전에 죽는다
3. writer-B 가 같은 checkpoint_id 로 다른 내용을 쓴다
4. 내용 비교 없이 Ok(false)
   -> 매니페스트엔 B 의 해시, 디스크엔 A 의 데이터
5. write_checkpoint 가 **성공을 반환한다**
```

`find_resume_point` 가 나중에 해시 불일치로 제외하므로 **데이터 손상은 아니다.**
그러나 **writer 가 "확정했다" 고 거짓 보고한다** — `CLAUDE.md` §3 위반이다.

### K-2 — `max_attempts == 0` 이 panic 했다

ADR-026 은 "최종 실패는 **명시적 오류**" 를 계약으로 정한다.
`RetryPolicy` 가 공개 구조체라 `0` 을 넣을 수 있고, 그러면 루프가 안 돌아
`last_err.expect(..)` 에서 패닉했다.

**panic 은 오류가 아니다** — 호출자가 처리할 수 없고 데이터 경로에서 프로세스를 죽인다.

## 시정하지 않은 지적

★ **검수자가 맞는데 안 고친 것을 숨기지 않는다.**

| 지적 | 왜 안 고쳤나 |
|---|---|
| `RevokeLeaseNotice` 반복 전송 | 멱등하다고 판단했으나 **실측하지 않았다.** 명령용 replay 정책 분리는 설계 변경이라 별도 판단 |
| 증거 메시지 3종의 서명자 ID 대체값 | `.proto` 변경 → `schema_version` 상향 필요 (V-08) |
| `ReplicaAck` 의 `fence_epoch` 부재 | 동상 (V-07) |
| §8 5·6단계 순서 | 규범과 구현이 어긋나 있는 것은 맞다. 둘 다 거부이므로 안전성은 같다. **규범을 고쳐야 하는지 별도 판단** |
| `write_once` 의 메모리 사용 | 파일 전체를 읽는다. 수 GB shard 에서 문제가 될 수 있으나 **측정하지 않았다** |

## 결과

```text
테스트         143 -> 167 passed / 0 failed      빌드 경고 0
벡터           36 -> 40건
기존 벡터 canonical 변경: 0건   <- 회귀 없이 규범 구멍만 메웠다
SCHEMA_FINGERPRINT 불변         <- .proto 를 건드리지 않았다
```

## 이 검증이 증명하지 "않는" 것

- **검수자가 한 모델뿐이다.** 다른 검수자는 다른 것을 볼 수 있다.
- **벡터 커버리지 구멍이 남아 있다** — 규칙 a(삽입 순서 무관) · c(UTF-8 경계 키) ·
  g(float 거부) · h(unknown field) · j(int32 음수) · §4(길이 경계).
- **Rust `Fields` API 가 규칙 h 를 강제하지 않는다.** `f.set(999, ..)` 는 출력된다.
- **`write_once` 의 메모리 특성을 측정하지 않았다.**
- **K-3 실패 주입을 못 했다** (권한 조작 필요).
- Windows 단일 플랫폼.

## 결정

1. 실측으로 성립이 확인된 **7건을 시정**했다.
2. `signing.md` 에 **규칙 i-2 · i-3 · c-2** 신설.
   **기존 벡터 canonical 은 하나도 바뀌지 않았다** — 회귀 없이 구멍만 메웠다.
3. `.proto` 변경이 필요한 3건은 `schema_version` 상향과 **함께** 처리한다.
4. **§8 5·6단계 순서**는 규범 수정 여부를 별도 판단한다.
5. ★ **`--verify` 가 검증하지 않던 기간의 evidence 를 재해석해야 한다** —
   `DoD-01`~`DoD-07` 의 "vector cross-checks: OK" 는 관계 검사만 뜻했다.
   다만 Rust 테스트가 저장된 hex 와 직접 대조하므로 **교차검증 자체는 유효**했다.

관련: `docs/protocol/signing.md` §3.1(i-2 · i-3 · c-2) ·
`docs/evidence/DoD-01_canonical_encode_교차검증.md` ·
`docs/decisions/ADR-026_체크포인트_확정_절차_플랫폼_차이.md`

---

## ★ 이후 변경 (2026-08-18 00:40) — 독립 재검수 ACCEPTED (1라운드)

독립 검수(`agent:codex-cli`, read-only)가 이 문서를 재검수해
`ACCEPTED` 로 판정했다 — 이 재검수 사이클에서 첫 라운드에 바로
통과한 evidence 다. Rust/Python 공통 canonical 결함 3종(중첩
signature-only 메시지·map 빈 값·nested derived-hash 제외)이 지금
테스트·구현과 대응함을 확인했다(`crates/protocol/tests/codex_findings.rs:51,146,231`).
`--verify` 가 지금도 `build_vectors()` 를 재실행해 저장값과 실제로
대조함을 확인했고(`tools/canonical/reference_canonical.py:1596,1618`),
직접 실행해 40개 벡터가 전부 일치함도 확인했다. vectors 메타데이터
"40건"(`:12`)도 지금과 일치한다.

한 가지만 참고 사항으로 남겼다: negative_tests 의 `--verify 뮤테이션`
항목(`:79`)은 함수명이 아니라 시나리오 서술이다 — 별도 `fn` 이름은
확인 안 됨. raw_output 에 변조 후 실패·원복 후 성공이 기록되어
있어(`:39`) 시나리오 자체는 실재하지만, 추적성을 위해 실제 테스트
함수로 대체하면 더 좋다는 제안이다 — review_outcome 을 바꾸는
문제는 아니다.

원래 Codex 검수(이 evidence 의 주제 자체)의 실행자 독립성은 코드로
증명할 수 없고 `docs/history/HISTORY.md` 의 "2026-08-16 18:20 — ★
독립 검수(Codex) 지적 7건 시정" 항목(제목 기반 인용 — 줄 번호는
세션 안에서도 바뀐다) 으로만 뒷받침된다는 점은 재검수도 "확인
안 됨"으로 남겼다 — 이 문서의 claim 범위 밖이다.

---

## ★ 이후 변경 (2026-08-18) — schema v2 승격 전 재검수, K-1 잔여 결함 발견·수정

새로운 독립 검수(`agent:codex-cli`, read-only, v2 승격용 재검수,
`p83` 프롬프트)가 이 evidence 를 다시 검토해 `CHANGES_REQUESTED`
로 판정했다. claim 대부분은 지금도 정확하다고 확인했지만, **K-1
이 완전히 고쳐지지 않았다**는, 이번 세션에서 두 번째로 심각한
지적을 받았다.

### [진짜 잔여 결함] K-1 은 "이미 존재할 때" 분기만 고쳐졌다

`write_once`(`crates/checkpoint/src/atomic.rs`)는 검사를 두 번
한다:

```text
1. 함수 시작 시 final_path.exists() 확인 -> 존재하면 내용 대조
   (K-1 로 이미 고쳐짐, atomic.rs:153-164)
2. tmp 파일을 쓴 뒤 다시 final_path.exists() 확인 -> "경쟁: 우리가
   쓰는 사이에 다른 쪽이 확정했을 수 있다" 분기 (atomic.rs:175-179,
   당시엔 내용 비교 없이 Ok(false) 반환)
```

2번 분기는 **1번과 정확히 같은 문제**를 갖고 있었다 — 위치 기반
이름(`shard-0.bin`)을 쓰는 다른 쪽이 다른 내용으로 먼저 확정했어도
비교 없이 `Ok(false)` 를 반환했다. K-1 원래 시정(2026-08-16)이
1번만 고치고 2번을 놓쳤다.

**고쳤다.** 2번 분기에서도 `final_path` 를 읽어 우리 `data` 와
비교하고, 다르면 `ContentMismatch` 를 반환한다
(`crates/checkpoint/src/atomic.rs`, 2026-08-18 수정).

### [발견했으나 고치지 않은 더 넓은 결함] 동시 다중 호출은 여전히 안전하지 않다

이 수정을 검증하려고 진짜 다중 스레드 동시 호출 테스트를 짰다가
**더 근본적인 문제**를 발견했다 — 같은 이름으로 동시에 `write_once`
를 부르는 모든 호출자가 **같은 tmp 파일 이름**(`{name}.tmp`)을
공유한다. 서로의 tmp 쓰기를 덮어쓰고, `fs::rename` 이 Windows 에서
기존 대상을 대체하는 시맨틱이라 **하나가 아니라 여러 호출이 각각
`Ok(true)` 를 반환**할 수 있다 — 8스레드 동시 호출에서 실제로 3회
`Ok(true)` 를 관측했다.

이것은 이번에 고친 것(경쟁 분기의 내용 비교 누락)과는 **다른, 더
넓은 결함**이다 — `write_once` 는 애초에 같은 이름에 대한 동시
다중 호출을 지원하도록 설계된 적이 없다. 이 저장소의 실제
호출부(`writer.rs`)는 순차적 재시작 시나리오(프로세스 A 가 죽은
**뒤** 프로세스 B 가 재개)만 상정하고, 지금은 Job 실행 자체가
미착수라 동시 호출 경로가 없다 — 그래도 함수 자체가 "동시 호출
안전" 을 주장한 적은 없으므로, **결함을 고정하는 테스트**로
등록했다: `crates/checkpoint/tests/codex_findings.rs::k1c_concurrent_same_name_writers_are_not_actually_safe`.
통과가 곧 이 위험이 아직 존재한다는 뜻이다(`lease_scope.rs` 의
`restart_resets_watermark_and_lets_stale_epoch_through` 와 같은
정신). 진짜 동시 호출 안전(호출마다 고유한 tmp 이름 + 재설계)은
이 evidence 의 원래 범위를 넘는 별도 작업으로 CLAUDE.md 백로그에
등록한다.

### 그 외 재확인 — 대부분 실재 확인됨

C-1·C-3·D-1(canonical 인코딩 결함 3건), `--verify` 재생성 대조,
replay nonce 결속(`AgentGrantAck` 도 포함해 재확인), K-2(max_attempts=0)
는 전부 코드로 재확인됐다. negative_tests 목록도 실재 확인됐다.
시정하지 않은 지적 5건(RevokeLeaseNotice 반복 전송·서명자 ID
대체값·`ReplicaAck.fence_epoch` 부재·§8 5·6단계 순서·`write_once`
메모리 사용)은 지금도 전부 유효하다 — coordinator/agent 신설이
이 다섯 항목을 해소하지 않았음도 재확인됐다(새 경로는 Grant/Lease/
GrantAck 만 처리하고 Evidence 메시지를 다루지 않는다). vectors
40건도 재확인됐다.

### Rust/Python 재실행 — 이 세션에서 직접 확인, Codex 샌드박스에서는 못함

Codex read-only 샌드박스는 `.cargo-build-lock` 접근이 거부돼
cargo 명령을 실행하지 못했다(샌드박스 제약이지 코드 결함이
아니다) — Python 명령은 직접 실행해 확인했다. 이 세션은 이미
로컬에서 cargo 전체도 K-1 수정을 반영해 직접 실행했다 —
`docs/evidence/_raw/DoD-08_v2_promotion_2026-08-18.txt` 가 그
receipt 다: `codex_findings`(protocol) 6, `codex_findings`(checkpoint)
9(k1c 신설 포함), `replay_binding` 10, `cargo test --workspace`
307 passed / 0 failed(k1c 신설로 306→307), `cargo build
--all-targets` 경고 0건.

### review_outcome

`CHANGES_REQUESTED` — K-1 잔여 결함을 실제로 고치고(atomic.rs),
동시성 negative test 를 추가하고(k1c), 이 addendum 으로 기록했다.
좁은 범위의 후속 확인을 별도로 요청해 `ACCEPTED` 를 받은 뒤에만
schema v2 로 승격한다.
