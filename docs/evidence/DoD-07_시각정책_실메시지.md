---
id: DoD-07
claim: "signing.md §9 의 세 시각 정책이 실제 proto 메시지에서 서로 다르게 동작한다. §9 표에 없던 6종은 ADR-029 로 Lifetime::Evidence 를 부여했고, 만료되지 않으면서 observed_at 을 노출한다(강제는 타입이 아니라 테스트가 한다). 단수명 경로가 실메시지(ExecutionGrant · RenewLeaseRequest)로 처음 검증되었다"
status: PASS
commit: 3120688ad3f82c61c3c913d2fc18e2b7207c5932
binary_digests:
  toolchain: "cargo 1.97.1 (c980f4866 2026-06-30) / rustc 1.97.1 / ed25519-dalek 2 / prost 0.14"
  note: "라이브러리 크레이트라 실행 바이너리 없음"
protocol_versions:
  schema_version: "1"
  canonical_spec: "docs/protocol/signing.md v1 (§9 표 + §9.1 재작성, ADR-029)"
  vectors: "tests/vectors/canonical_v1.json (36건, 변경 없음)"
  schema_fingerprint: "blake3-256 0a34709f5599658f071ed8ac6df7c0ccca441cba89ce640db9ce061aa7866bcf (변경 없음 — .proto 를 건드리지 않았다)"
platform: "Microsoft Windows 11 Pro build 26200 / NTFS / x86_64-pc-windows-msvc"
hardware: "Intel Iris Xe Graphics / GPU 무관"
network_profile: "해당 없음 - 로컬 단위 테스트"
command: |
  cargo test -p gputeer-crypto --test lifetime_policy
  cargo test --workspace -- --nocapture
  cargo build --workspace --all-targets
raw_output: |
  Running tests\lifetime_policy.rs
  test execution_grant_is_short_lived ... ok
  test valid_grant_verifies ... ok
  test grant_rejects_message_from_the_future ... ok
  test default_ttl_masks_forward_skew_for_real_grant ... ok
  test forward_skew_is_reachable_with_longer_ttl ... ok
  test grant_requires_nonce ... ok
  test renew_lease_expiry_is_derived_from_ttl ... ok
  test renew_lease_without_issued_at_is_immediately_expired ... ok
  test all_six_evidence_messages_are_marked_evidence ... ok
  test evidence_does_not_expire ... ok
  test evidence_is_not_replay_checked ... ok
  test evidence_must_expose_observation_time ... ok
  test replica_ack_stays_valid_forever_even_if_replica_is_gone ... ok
  test the_three_lifetimes_actually_behave_differently ... ok
  test result: ok. 14 passed; 0 failed; 0 ignored

  Running tests\ed25519_verify.rs        20 passed
  Running tests\canonical_vectors.rs     15 passed
  Running tests\field_number_audit.rs    10 passed
  Running tests\prost_canonical.rs       25 passed
  Running tests\schema_evolution.rs       6 passed
  Running tests\schema_fingerprint.rs     2 passed
  Running tests\stream_ownership.rs       5 passed
  Running tests\t1_signing_targets.rs    11 passed
  Running tests\t1b_grant_and_control.rs 10 passed
  Running tests\durability_chaos.rs      16 passed
  Running tests\kill_chaos.rs             7 passed
  Doc-tests                               2 passed

  전체: 143 passed / 0 failed
  cargo build --all-targets  경고 0건
artifacts:
  - docs/evidence/_raw/DoD-07_test_output.txt
  - docs/decisions/ADR-029_증거_메시지의_시각_정책.md
  - crates/protocol/src/signable.rs
  - crates/protocol/src/signing.rs
  - crates/crypto/tests/lifetime_policy.rs
negative_tests:
  - "★ the_three_lifetimes_actually_behave_differently: 같은 6시간 경과에 대해 LongLived 는 통과 · ShortLived 는 거부 · Evidence 는 10년 뒤에도 통과함을 확인. 전부 같은 동작이면 Lifetime 구분 자체가 의미가 없다"
  - "★ evidence_does_not_expire: 0·1·3·10년 뒤에도 통과. 발급 시각보다 **이전**에 검증해도 통과. 만료시키면 오래된 체크포인트에서 재개할 수 없게 되고 그것은 시스템의 존재 이유를 부순다"
  - "grant_rejects_message_from_the_future: 과거 방향 skew 경계값은 통과하고 경계 밖은 ClockSkew. **거부해야 할 때 거부하는가**와 **거부하면 안 될 때 거부하지 않는가**를 둘 다 본다"
  - "default_ttl_masks_forward_skew_for_real_grant: 기본 TTL(60s)==skew(60s)라 미래 방향 skew 가 만료 검사에 가려짐을 실메시지로 고정. §9 TTL 을 바꾸면 실패한다"
  - "forward_skew_is_reachable_with_longer_ttl: TTL 을 1시간으로 늘리면 미래 방향 skew 경로가 실제로 발동. 위 테스트만 있으면 '미래 방향 검사가 아예 없는' 구현과 구분되지 않는다"
  - "grant_nonce_comes_from_signed_message_field: nonce 없음 · 0/8/15/17바이트 전부 거부 (§10 은 16바이트 MUST) — ★ 2026-08-17 이름 정정. 당시 원문의 grant_requires_nonce 는 그 뒤 이 이름으로 바뀌었다"
  - "valid_grant_verifies: 단수명이므로 NoReplayCheck 로는 require_replay_checked() 가 Replay 를 반환. replay 미검사 Grant 로 Job 을 실행하면 안 된다"
  - "renew_lease_expiry_is_derived_from_ttl: expires_at 필드가 없는 메시지의 도출 만료가 경계값에서 정확한지 (만료 1ms 전 통과 / 만료 시점 거부)"
  - "renew_lease_without_issued_at_is_immediately_expired: issued_at=0 이면 즉시 만료. '시각 없는 갱신 요청은 무효'라는 올바른 동작임을 고정"
  - "evidence_must_expose_observation_time: 6종 전부가 각자의 observed_at(created_at · acked_at · decided_at · issued_at)을 정확히 노출하는지. 0 을 반환하면 '언제인지 모르는 증거'이고 그것은 증거가 아니다"
  - "★ replica_ack_stays_valid_forever_even_if_replica_is_gone: **결함을 고정하는 테스트다.** 통과한다는 것이 곧 '프로토콜이 이 상황을 막지 못한다'는 뜻이다. 10년 뒤에도 ACK 가 유효하며 fence_epoch 이 없음을 함께 단언한다"
  - "evidence_is_not_replay_checked: 증거는 §10 replay 대상이 아니므로 nonce 없이도 require_replay_checked() 가 통과 — ★ 2026-08-17 이 동작이 뒤집혔다. 아래 '이후 변경' 참조"
limitations:
  - "★ `ReplicaAck` 에 `fence_epoch` 이 없다는 결함을 **고치지 않았다.** 기록하고 테스트로 고정했을 뿐이다. 복제본이 삭제되어도 ACK 는 영원히 유효하며, 소비 측이 `acked_at` 만 보고 신선도를 판단해야 한다. **그 규약은 강제되지 않는다** — TODO_VISION V-07"
  - "★ 세 메시지(ArtifactRef · CanonicalDecision · RevokeLeaseNotice)에 서명자 ID 필드가 없어 attempt_id · job_id · lease_id 를 대체값으로 쓴다. 대체값은 키 조회 키이므로, 매핑을 모르는 검증자는 유효한 서명도 UnknownSigner 로 거부한다. 단일 Coordinator 에서만 무해하다 — TODO_VISION V-08"
  - "Evidence 메시지의 '신선도 판단' 은 프로토콜이 하지 않는다. observed_at 을 노출할 뿐이며, 소비 측이 fence_epoch 와 함께 판단해야 한다. **그 판단 로직은 구현되지 않았다** — 소비 측(Coordinator·Agent)이 없기 때문이다"
  - "RevokeLeaseNotice 는 명령인데 Evidence 로 두었다. stale 회수는 fence_epoch 이 막지만 **같은 epoch 의 회수 통지 반복 전송**은 막지 못한다. 멱등하므로 무해하다고 판단했으나 실측하지 않았다"
  - "§9 표의 Heartbeat / RPC 는 여전히 미구현이다. 단수명 메시지는 ExecutionGrant · RenewLeaseRequest 둘뿐이다"
  - "replay 캐시(§10)는 여전히 NoReplayCheck 뿐이다. 단수명 메시지가 실제로 생겼으므로 이제 replay 방어의 부재가 실질적 공백이 되었다 — 실행계획 v2 T4"
  - "키 관리(§11)는 여전히 InMemoryKeyring 뿐이다"
  - "★ 2026-08-16 정정 — 이 문서는 observed_at 노출이 '타입 수준에서 강제된다'고 적었으나 과장이었다. Signable::observed_at_unix_ms() 에 기본 구현이 있어 덮어쓰지 않아도 컴파일된다. 실제 강제는 evidence_must_expose_observation_time 테스트가 하며, 새 Evidence 메시지를 그 테스트에 넣지 않으면 기본 구현이 조용히 쓰인다"
  - "Windows 단일 플랫폼"
decision: "ADR-029 채택 — Lifetime::Evidence 신설. signing.md §9 표에 증거 행을 추가하고 §9.1 을 재작성했다. .proto 를 건드리지 않았으므로 schema_version 상향과 벡터 재생성이 불필요하다. ReplicaAck 의 fence_epoch 부재는 V-07 로, 서명자 ID 부재는 V-08 로 등록했다 — 둘 다 schema_version 상향이 필요해 함께 처리하는 것이 싸다. 다음: T4(replay 캐시) -> T3(키 관리)"
---

# DoD-07 · 시각 정책의 실메시지 검증

## 무엇을 입증하려 했는가

`DoD-05` 가 남긴 것이다.

> `signing.md` §9 시각 정책 표에 **6종이 없고, `expires_at` 필드조차 없다.**
> `Lifetime` 을 추측으로 정하지 않기 위해 `Signable` 을 구현하지 않았다 —
> 따라서 이들은 아직 `verify()` 를 통과할 수 없다.

그리고 `DoD-04` 가 남긴 것.

> `Signable` 을 구현한 메시지가 전부 장수명이라
> **단수명 경로는 테스트 전용 타입으로만 검증했다.**

이 검증이 둘 다 닫는다.

## ★ 결정 — 증거는 만료되지 않는다

6종이 무엇인지부터 봤다.

```text
CheckpointManifest    "이 체크포인트의 내용이 이것이다"
ReplicaAck            "내가 이것을 저장하고 fsync 했다"
ArtifactRef           "이 산출물이 이 CAS 경로에 있다"
AttemptReport         "이 attempt 의 결과가 이것이다"
CanonicalDecision     "이 attempt 를 canonical 로 골랐다"
RevokeLeaseNotice     "이 lease 를 회수한다"
```

**전부 "권한" 이 아니라 "증거" 다.**
`JobManifest`(제출 권한)나 `Lease`(실행 권한)와 성격이 다르다.

### 과거의 사실은 만료되지 않는다

`CheckpointManifest` 를 만료시키면 **오래된 체크포인트에서 재개할 수 없다.**
그것은 이 시스템의 존재 이유 — "노드 장애 시 다른 GPU 에서 작업을 이어간다" —
를 정면으로 부순다.

→ `Lifetime::Evidence` 신설. 만료 검사를 하지 않는다.

### ★ 그러나 `Perpetual` 과 구분한다

```text
Perpetual   Genesis · Release Manifest. 시스템 상수에 가깝다.
            "지금도 참인가" 를 물을 필요가 없다.

Evidence    관측 시점의 사실이다. 소비 측이 **신선도를 판단해야 한다.**
```

동작은 같다(만료 검사 없음). **의미가 다르다.**
한 이름에 두 의미를 담으면 다음 사람이 `ReplicaAck` 를 시스템 상수처럼 다룬다.

그래서 `Evidence` 는 `observed_at_unix_ms()` 를 노출한다.
"언제인지 모르는 증거" 는 증거가 아니다.

★ **2026-08-16 정정 (독립 검수).** 원래 "타입으로 강제" 라고 적었는데 **과장이었다** —
기본 구현이 있어 덮어쓰지 않아도 컴파일된다. 실제 강제는 테스트가 한다
(`evidence_must_expose_observation_time`). 자세한 것은 ADR-029 정정 항목.

### 신선도는 시각이 아니라 fencing 이 판단한다

6종 중 5종이 `fence_epoch` 을 갖는다.
**시계는 어긋나지만 epoch 은 어긋나지 않는다.** 만료를 시각으로 거는 것은
잘못된 축이다.

## ★ `ReplicaAck` 만 `fence_epoch` 이 없다

```text
CheckpointManifest    created_at(30)   fence_epoch(32)
ReplicaAck            acked_at(30)     ★ 없음
ArtifactRef           created_at(21)   fence_epoch(22)
AttemptReport         issued_at(40)    fence_epoch(5)
CanonicalDecision     decided_at(13)   fence_epoch(12)
RevokeLeaseNotice     issued_at(5)     fence_epoch(3)
```

**6종 중 유일하게 신선도 판단 근거가 시각뿐이다.**
그런데 `ReplicaAck` 는 `REPLICATED(n)` 을 세는 근거이므로
**durability 주장의 뿌리**다. 가장 약한 곳이 가장 중요한 곳이다.

```text
복제본이 삭제되어도 이 ACK 는 영원히 유효하다.
```

`CLAUDE.md` §0.3 이 이미 같은 인식을 갖고 있다 —
"`COMMITTED` 이후 replica 가 유실되어도 상태를 되돌리지 않는다.
`COMMITTED_DEGRADED` 로 표시한다." 즉 **기준선은 "ACK 는 과거 사실" 을 전제**로
설계되어 있고, `Lifetime::Evidence` 는 그것을 타입에 적은 것이다.

### 결함을 고정하는 테스트

```rust
#[test]
fn replica_ack_stays_valid_forever_even_if_replica_is_gone() { … }
```

★ **통과한다는 것이 곧 "프로토콜이 이 상황을 막지 못한다" 는 뜻이다.**
10년 뒤에도 유효함을 확인하고, `fence_epoch` 이 없음을 함께 단언해
V-07 이 해소되면 테스트가 갱신을 요구하게 했다.

**고치지 않았다.** `.proto` 변경 → `schema_version` 상향(§7.3)이 필요하고,
복제 계층 자체가 미구현이라 오판 사례를 관측할 수 없다. → V-07.

## 단수명 경로가 실메시지로 처음 검증되었다

`ExecutionGrant` · `RenewLeaseRequest` 를 `ShortLived` 로 구현했다.

### `RenewLeaseRequest` 에는 `expires_at` 필드가 없다

§9 표는 단수명으로 분류하고 TTL 60초를 정하는데 **필드가 없다.**

```rust
fn expires_at_unix_ms(&self) -> u64 {
    self.issued_at_unix_ms.saturating_add(GRANT_TTL_MS)
}
```

★ 이것은 값을 지어내는 것이 아니라(`CLAUDE.md` §1)
**규범이 정한 TTL 을 적용**하는 것이다. 근거를 코드에 적었다.

`issued_at = 0` 이면 즉시 만료된다 — 결함이 아니라
**"시각 없는 갱신 요청은 무효"** 라는 올바른 동작이며, 테스트로 고정했다.

### TTL == skew 문제를 실메시지로 재확인

`DoD-04` 가 테스트 타입으로 찾은 것이 실메시지에서도 그대로였다.

```text
now = issued_at + 60s   ->  Expired   (ClockSkew 가 아니다)
now = issued_at - 60s-1 ->  ClockSkew (과거 방향은 도달 가능)
```

**TTL 을 늘려 미래 방향을 "살리는" 것은 하지 않았다.**
단수명 메시지의 수명을 늘리면 replay 창이 커진다 —
**보안 매개변수를 코드 경로 도달성 때문에 바꾸지 않는다.**

대신 두 테스트로 사실을 고정했다.

```text
default_ttl_masks_forward_skew_for_real_grant   기본 TTL 에서 가려짐
forward_skew_is_reachable_with_longer_ttl        TTL 1시간이면 발동
```

두 번째가 없으면 **"미래 방향 검사가 아예 없는" 구현과 구분되지 않는다.**

## 세 정책이 실제로 다른가

전부 같은 동작이면 `Lifetime` 구분 자체가 의미가 없다.

| 경과 | LongLived (Manifest) | ShortLived (Grant) | Evidence (Checkpoint) |
|---|:---:|:---:|:---:|
| 6시간 | **통과** | **거부** | 통과 |
| 7일 후 | 거부(만료) | 거부 | 통과 |
| 10년 후 | 거부 | 거부 | **통과** |
| 발급 이전 시각 | 통과 | 거부(skew) | **통과** |

## 결과

```text
테스트         129 -> 143 passed / 0 failed        빌드 경고 0
Signable       2종 -> 10종
.proto 변경    없음 -> schema_version 상향 불필요, 벡터 재생성 불필요
SCHEMA_FINGERPRINT  불변
```

## 이 실험이 증명하지 "않는" 것

- ★ **`ReplicaAck` 의 `fence_epoch` 부재를 고치지 않았다.** 기록하고 고정했을 뿐이다.
- ★ **세 메시지에 서명자 ID 필드가 없다.** `attempt_id` · `job_id` · `lease_id` 를
  대체값으로 쓰는데, 이것은 **키 조회 키**이므로 매핑을 모르는 검증자는
  유효한 서명도 `UnknownSigner` 로 거부한다. 단일 Coordinator 에서만 무해하다.
- **"신선도 판단" 로직은 구현되지 않았다.** 프로토콜은 `observed_at` 을 노출할 뿐이고,
  판단은 소비 측(Coordinator·Agent)의 몫인데 **소비 측이 아직 없다.**
- **`RevokeLeaseNotice` 의 반복 전송이 무해하다는 것을 실측하지 않았다.**
  멱등할 것이라 판단했을 뿐이다.
- **replay 캐시가 여전히 없다.** 단수명 메시지가 실제로 생겼으므로
  이제 그 부재가 **실질적 공백**이 되었다.
- **키 관리(§11)가 여전히 `InMemoryKeyring` 뿐이다.**
- Windows 단일 플랫폼.

## 결정

1. **ADR-029 채택** — `Lifetime::Evidence` 신설.
2. `signing.md` §9 표에 증거 행 추가, §9.1 재작성.
3. **`.proto` 를 건드리지 않았으므로** `schema_version` 상향·벡터 재생성 불필요.
4. **V-07**(`ReplicaAck.fence_epoch`) · **V-08**(서명자 ID) 등록.
   둘 다 `schema_version` 상향이 필요하므로 **함께 처리하는 것이 싸다.**
5. 다음: **T4**(replay 캐시) — 단수명 메시지가 생겨 우선순위가 올라갔다 → **T3**(키 관리).

관련: `docs/decisions/ADR-029_증거_메시지의_시각_정책.md` ·
`docs/evidence/DoD-04_ed25519_검증순서.md` · `docs/evidence/DoD-05_T1_서명대상_확장.md` ·
`docs/protocol/signing.md` §9 · §9.1

---

## ★ 이후 변경 (2026-08-17) — 이 기록의 한 줄이 더 이상 사실이 아니다

> 이 evidence 는 **그 커밋에서 관측한 것**을 남긴 기록이므로 관측 자체는 고치지 않는다.
> 그러나 그 뒤 동작이 바뀐 부분을 표시하지 않으면 **읽는 사람을 오도한다.**

```text
당시                                        지금 (DoD-10 · 커밋 e188cce 이후)
──────────────────────────────────────────────────────────────────────────
evidence_is_not_replay_checked              evidence_has_no_replay_defense_and_says_so
증거는 require_replay_checked() 를 통과한다  ★ 거부한다
```

### 왜 뒤집혔나

독립 검수(2026-08-17)가 지적했다.
`replay_checked: bool` 이 **"검사했다" 와 "검사 대상이 아니다" 를 같은 값**으로 뭉갰다.

`require_replay_checked()` 는 부작용 게이트다.
증거를 재전송하면 매번 통과한다 — 그것으로 과금이나 기여도 집계를 하면 **중복 계상된다.**
"replay 대상이 아니다" 는 "안전하다" 가 아니다.

지금은 [`ReplayStatus::NotApplicable`] 로 **보고하고 게이트는 막는다.**
증거로 부작용을 실행해야 하는 소비자는 `get()` 을 쓰고 자기 멱등성을 갖춰야 한다 —
그 선택이 코드에 보이게 하는 것이 목적이다.

관련: `docs/evidence/DoD-10_영속_replay_저장소.md` · `crates/protocol/src/signing.rs`

---

## ★ 이후 변경 (2026-08-18 00:40) — 벡터 수치·테스트명·limitation 2건 stale

독립 검수(`agent:codex-cli`, read-only)가 재검수해 `CHANGES_REQUESTED`
로 판정했다. **claim 핵심은 지금도 참**이다 — `Lifetime` 세 정책
구분, `verify()` 가 Evidence 에 만료·skew 검사를 생략하는 것,
Evidence 6종·`observed_at`·`ExecutionGrant`/`RenewLeaseRequest` 단수명
경로 전부 지금 코드로 확인됐다(`crates/protocol/src/signing.rs:124,763`,
`crates/protocol/src/signable.rs:98,175,227,262,289,320,347,381`).

### vectors 메타데이터

`36건`(`:12`)은 지금과 다르다 — `tests/vectors/canonical_v1.json`
을 직접 파싱하면 지금 **40건**이다.

### negative_tests 이름 정정

`evidence_is_not_replay_checked` 는 실재하지 않는다. 지금 함수명은
`evidence_has_no_replay_defense_and_says_so`(`crates/crypto/tests/lifetime_policy.rs:331`,
재현 확인함).

### stale limitations

| 원래 서술 | 지금 |
|---|---|
| "§10 은 여전히 NoReplayCheck 뿐"(`:79`) | ★ 거짓이다. `DurableReplayGuard` 가 지금 존재한다(`crates/crypto/src/durable_replay.rs:147`). 다만 Evidence 메시지 자체는 replay 방어를 받지 않는다는 현재 claim 은 그대로 유효하다 — `ReplayStatus::NotApplicable` 로 확인된다(`lifetime_policy.rs:331`) |
| "InMemoryKeyring 뿐"(`:80`) | ★ 거짓이다. `PersistentKeyring` 의 load·revoke·rotate·save 가 구현되어 있다(`crates/crypto/src/keyring.rs:209,245,456,502,575`) |

`ReplicaAck.fence_epoch` 부재(`proto/artifact.proto:101` 로 재확인),
서명자 ID 대체·소비 측 신선도 판단 미구현·RevokeLeaseNotice 반복
전송 미측정·Windows 단일 플랫폼 limitation 은 지금도 유효하다.

### claim 을 읽을 때 주의

이 문서 자체가 위 "이후 변경 (2026-08-17)" 절에서 밝히듯 replay
동작이 그 뒤 한 번 뒤집혔다 — claim 을 "당시 관측"이 아니라 "지금
코드 상태"로 읽으려면 그 절과 위 표를 함께 봐야 한다.

### review_outcome

`CHANGES_REQUESTED` → 위 정정으로 vectors·negative_tests 이름·
stale limitations 를 반영했다. 원본 YAML 은 당시 기록이므로 고치지
않는다.

★ 2026-08-18 00:50 두 번째(최종) 재검수 — `agent:codex-cli` 가
**`ACCEPTED`** 로 판정했다. vectors 40건, `evidence_has_no_replay_defense_and_says_so`
함수명, `DurableReplayGuard`·`PersistentKeyring` 인용을 전부 직접
열어 재확인했다. `HISTORY.md` 인용도 이번엔 줄 번호가 아니라 제목
기반이라 문제없다고 확인했다.
