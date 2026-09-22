---
schema_version: 2
id: DoD-54
claim: "holder/freshness/membership 해석을 명시적 입력으로 받는 `evaluate_effective_replicas()`가 시계·TTL·I/O·DB·network·난수·membership 조회·전역 상태 없이 입력 검증·정렬·`BTreeMap`/`BTreeSet` 집계만으로 effective replica report 전체를 입력 순서와 무관하게 계산하고, 같은 holder의 복수 selected를 후보 계산 전에 fail closed하며, 유효 서명·현재 승인·distinct failure-domain·same-device 중복 제거·ephemeral `WORKER_LOCAL` 제외와 `MIRRORED=1`/`REPLICATED=2` 요구치를 적용하되 규범에 없는 timestamp 최신값·TTL·`ONLINE` 조건을 추가하지 않음을 13개 테스트, production guard 뮤테이션 2건, 독립 검수 1라운드 ACCEPTED와 감독자 회귀 실행으로 확인했다"
status: PASS
commit: dc7158a3bdba924924e94b425232928ea2930e35

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — resolved-input effective replica 순수 kernel, 13개 테스트와 production guard 뮤테이션 2건"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-24T16:55:00+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 fresh-read-only 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "`durability.rs:178-359`의 순수 계산 경계와 후보 계산 전 duplicate-selected fail-closed, `effective_replica_kernel.rs:25-63`의 실제 4! 순열 생성과 `EffectiveReplicaReport` 전체 비교, timestamp 최대값/TTL 판정 부재, `ONLINE` 미추가, artifact count 규범과 ephemeral local의 양방향 정합, mixed scope·빈 식별자·미해석 membership fail-closed, 상태 전이·proto/protocol 변경 부재, legacy `ReplicaSet` 실행 코드 동일, `lib.rs:11` additive re-export만 존재함을 확인했다. DoD-41 hard-filter 33/33, DoD-45 best-fit 20/20, 상태표 parity 5/5, legacy ReplicaSet 2/2 회귀를 직접 재실행하고 잔여 수정 요청 없이 1라운드 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-54_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-54_effective_replica_count_kernel_2026-08-24.txt"
raw_output_digest: "sha256:fd1c158f35663c8b3efbe49790eb8afd56a3f719a953544320632da944e3bd87"
raw_output_bytes: 9594

binary_digests:
  toolchain: "C:\\Users\\playdata2\\.cargo\\bin\\cargo.exe 사용 — cargo/rustc version과 binary digest는 제공된 이력에 기록되지 않음"
protocol_versions:
  schema_version: "proto/schema 변경 없음 — 기존 Durability·ReplicaAck·DurabilityStatus 계약을 process-local resolved-input kernel 타입으로 해석"
  canonical_spec: "`state-machines.md:220-223`, `common.proto:148-153`, `artifact.proto:135-152`, `signing.md:514-527`를 규범 근거로 사용; 서명 canonical 구현과 상태 전이는 변경하지 않음"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test"
hardware: "GPU 미사용 — 합성 resolved observation으로 순수 count·결정성·fail-closed 경계를 검증"
network_profile: "네트워크 미사용 — clock/filesystem/DB/network/random/membership lookup 없는 in-process 순수 함수"
command: |
  C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-checkpoint
raw_output: |
  (docs/evidence/_raw/DoD-54_effective_replica_count_kernel_2026-08-24.txt,
   docs/evidence/_raw/DoD-54_review.txt 전문 참조)

  감독자 직접 확인: PASS — effective replica 13 + kill 7 + resume 7 +
  state-table parity 5 + write-failure 5, 각 0 failed, exit code 0
  evidence 작성 시 전체 checkpoint 재실행: PASS — 67 passed, 0 failed, exit code 0
  독립 검수 1라운드: ACCEPTED — 순수성·24개 순열/report 전체·duplicate holder 선행
  fail-closed·TTL/ONLINE 부재·ephemeral 양방향·범위·회귀 직접 확인
artifacts:
  - docs/plans/2026-08-24_1634_effective_replica_count_kernel_v1.md
  - crates/checkpoint/src/durability.rs
  - crates/checkpoint/tests/effective_replica_kernel.rs
  - crates/checkpoint/src/lib.rs
  - docs/evidence/_raw/DoD-54_effective_replica_count_kernel_2026-08-24.txt
  - docs/evidence/_raw/DoD-54_review.txt
negative_tests:
  - "재귀 permutation 생성기가 4개 observation의 실제 4! = 24개 순열을 만들고 모든 순열의 `EffectiveReplicaReport` 전체를 `assert_eq!`로 비교"
  - "같은 holder의 selected observation이 2개 이상이면 후보 계산 전에 `MultipleSelectedObservations`로 전체 실패하고 조용히 하나를 선택하지 않음"
  - "오래된 observation을 selected, 최신 observation을 superseded로 주어도 selected만 count해 timestamp 최대값이나 TTL freshness 규칙이 없음을 고정"
  - "동일 failure-domain의 서로 다른 holder는 canonical holder 하나만 count하고 같은 device의 서로 다른 kind는 별도 replica로 세지 않음"
  - "invalid signature, 미승인 holder, unresolved/ambiguous membership·failure-domain과 `WORKER_LOCAL`의 미해석 ephemeral fact를 count하지 않음"
  - "ephemeral `WORKER_LOCAL`만 제외하고 ephemeral 또는 ephemeral-미해석 non-local kind는 세어 local-copy 규범의 양방향 경계를 고정"
  - "empty checkpoint/root/holder/domain과 mixed checkpoint/root를 typed error로 fail closed"
  - "`LOCAL=0`, `MIRRORED=1`, `REPLICATED=2` 요구치와 empty input 경계를 검증"
  - "뮤테이션 1 — production duplicate-selected 검사 제거 시 지정 테스트가 같은 holder의 count 2를 검출해 실패, 원복 후 통과"
  - "뮤테이션 2 — production `kind == WORKER_LOCAL` guard 제거 시 ephemeral non-local도 제외되어 기대 count 1, 실제 0으로 실패, 원복 후 통과"
limitations:
  - "authoritative membership/failure-domain resolver 와 durable ACK consumer 가 아직 없어 이 결과는 resolver 가 제공한 평가 시점의 count 이며 현재 liveness 나 `MIRRORED` 상태 전이를 증명하지 않는다"
  - "`MIRRORED` 판정 적용·상태 전이는 범위 밖"
  - "`ONLINE` 조건은 replica 규범에 없어 추가하지 않았다"
  - "freshness 판정은 kernel 밖 resolver 책임이며 kernel 은 holder 당 selected 가 둘 이상이면 fail closed 한다"
  - "raw durable ACK의 current signature/key 재검증, Job durability projection, durable count snapshot 저장, production routing과 liveness/degraded 복구는 범위 밖이다"
  - "실제 membership·failure-domain authority, DB·network·clock·GPU hardware를 사용하지 않은 합성 입력 검증이므로 resolver 입력의 진실성은 증명하지 않는다"
decision: "`state-machines.md:220-223`이 요구하는 것은 유효 replica 수와 요구치 비교이며 `MIRRORED`의 직접 수치 정의는 `common.proto:148`의 `REPLICATED(1)`이다. 따라서 이 조각은 durable ACK를 직접 소비하거나 checkpoint 상태를 바꾸는 대신, 외부 resolver가 holder별 freshness와 current signature/membership·ephemeral·failure-domain을 해소한 immutable observation만 받아 count와 근거 목록을 반환하는 순수 kernel로 제한했다. selected/superseded를 정렬하고 failure-domain을 `BTreeMap`, scope를 `BTreeSet`으로 canonicalize해 4개 입력의 모든 24개 순열에서 report 전체가 동일하며, holder별 복수 selected는 후보 계산 전에 typed error로 닫는다. kernel은 timestamp 최신값, TTL, 현재 시각 또는 `ONLINE` 조건을 만들지 않는다. 자체 재검토에서 미해석 ephemeral fact를 모든 kind에 요구하던 과잉 조건을 발견해 `WORKER_LOCAL`에만 적용하도록 수정했고, non-local count와 local 제외 양방향 회귀 테스트 및 실제 guard 뮤테이션으로 고정했다. legacy `ReplicaSet`은 실행 코드를 바꾸지 않고 표현 한계를 주석에만 명시했다. 독립 검수는 순수성·결정성·fail-closed·규범·변경 범위와 관련 회귀를 직접 확인해 1라운드 ACCEPTED했고 감독자는 checkpoint suite를 재확인했다. scheduler 로드맵 진행: `DoD-41`~`DoD-54` 완료. `DoD-52` anchor → `DoD-53` ack 저장 → `DoD-54` count kernel 로 `MIRRORED` 판정의 **입력과 계산**이 갖춰졌고, 남은 것은 authoritative resolver 와 전이 적용이다. membership/ControlStore 계열은 `Signable`·signer identity·lifetime·member 상태 규범이 없어 규범 확정 약 2일, durable 저장까지 누적 약 3일로 별도 과제다."
---

# DoD-54 · holder/freshness/membership 해석 입력 기반 순수 effective-replica kernel

## 무엇을 입증하려 했는가

외부 resolver가 holder별 freshness와 current signature/membership·ephemeral·failure-domain을
해소한 고정 observation을 입력하면, `evaluate_effective_replicas()`가 외부 상태나 시각 없이
유효 replica 수와 counted/excluded/superseded 근거 전체를 입력 순서와 무관하게 계산하고
malformed·ambiguous 입력을 fail closed하는지 검증했다.

이 claim은 현재 `MIRRORED` 상태나 durable consumer를 주장하지 않는다. kernel에 전달된
평가 시점의 resolved input에 대한 계산만 주장한다.

## 규범 근거와 의도적으로 만들지 않은 조건

`docs/protocol/state-machines.md:220-223`은 signed `ReplicaAck` 수신 시 유효 replica 수를
갱신하고, `REPLICATED -> COMMITTED`의 guard를 “유효 replica 수 >= 요구치”로 규정한다.
`docs/protocol/`에는 `MIRRORED`를 직접 수치로 정의한 문장이 없고,
`proto/common.proto:148-153`이 `MIRRORED=REPLICATED(1)`,
`REPLICATED=REPLICATED(2)`를 protobuf 계약으로 둔다.

count 규칙은 `proto/artifact.proto:135-152`의 유효 holder 서명, 동일 failure-domain 중복
제거, ephemeral node의 local copy 제외, 동일 device 중복 제거다. signer의 membership/승인은
Ed25519 검증 뒤 확인해야 한다는 `docs/protocol/signing.md:514-527`의 순서를 resolver 입력
계약에 반영했다.

이 규범에는 ACK TTL, timestamp 최대값, holder의 현재 `ONLINE` 조건이 없다. 따라서 kernel은
그 조건을 만들지 않았다. freshness는 외부 resolver가 holder당 하나를 `selected`로 정해
전달하며, kernel은 그 선택을 존중하고 둘 이상이면 전체를 fail closed한다.

## 구현 — 외부 효과 없는 계산 경계

`crates/checkpoint/src/durability.rs:24-127`에 resolved-input 타입과 report/error 타입을,
`:178-255`에 `evaluate_effective_replicas()`를 추가했다. 함수는 입력 검증, 정렬,
`BTreeMap`/`BTreeSet` 집계만 사용한다. 시계·TTL·I/O·filesystem·DB·network·난수·membership
조회·전역 상태를 읽지 않는다.

selected와 superseded를 각각 canonical 정렬하고, selected 후보의 signature/membership 승인,
필요 authority fact와 ephemeral local 규칙을 평가한다. 유효 후보는 resolved failure-domain별
canonical 첫 holder 하나만 센다. report는 count뿐 아니라 scope, 요구치, counted, excluded,
superseded를 모두 반환한다.

같은 holder의 selected가 둘 이상인지는 `durability.rs:302-315`에서 후보 계산 전에 검사한다.
오류 뒤 하나를 고르거나 계산을 재개하는 경로가 없다. public API 변경은
`crates/checkpoint/src/lib.rs:11`의 additive re-export뿐이다.

## 결정성, duplicate holder와 freshness 경계

전용 테스트의 재귀 생성기는 4개 observation의 실제 4! = 24개 순열을 모두 만든다. 각 순열의
count만이 아니라 `EffectiveReplicaReport` 전체를 `assert_eq!`로 비교한다.

freshness 경계 테스트는 `acked_at_unix_ms=1`인 오래된 관측을 selected로,
`u64::MAX`인 최신 관측을 superseded로 제공한다. 결과가 오래된 selected를 세고 최신 행을
superseded로 보존하므로 timestamp 최대값이나 TTL 판정이 없음을 직접 고정한다.

## 자체 재검토 — ephemeral 과잉 조건 수정

초안은 `is_ephemeral` 해석이 unresolved/ambiguous이면 kind와 무관하게 모든 관측을 제외했다.
그러나 규범은 ephemeral의 **local copy만** 제외한다. production 분기를
`ReplicaKind::WorkerLocal`에만 적용하도록 고치고, 미해석 ephemeral인 Hub와
SubmitterMirror가 count되는 전용 회귀 테스트를 추가했다.

독립 검수는 non-local이 세어져야 하는 방향과 ephemeral `WORKER_LOCAL`이 제외돼야 하는
반대 방향을 모두 확인했다. `kind == WORKER_LOCAL` guard 제거 뮤테이션도 ephemeral non-local을
과잉 제외해 기대 count 1, 실제 0으로 실패했다.

기존 `ReplicaSet`의 “규칙 1~4 적용” 주석은 실제 타입 표현력과 맞지 않았다. 실행 코드는
바꾸지 않고 holder freshness·kind·current membership을 표현하지 못하는 legacy 한계와 신규
결정 경로만 주석으로 명시했다.

## 뮤테이션과 독립 검수 — **ACCEPTED**

production duplicate-selected 검사를 제거하면 같은 holder가 서로 다른 두 domain에서 count 2가
되어 지정 테스트가 실패했다. production `kind == WORKER_LOCAL` guard를 제거하면 ephemeral
non-local까지 제외되어 지정 테스트가 기대 count 1, 실제 0으로 실패했다. 둘 다 원복한 뒤
전체 suite가 통과했다.

독립 검수는 순수성, 실제 24개 순열과 report 전체 비교, duplicate selected의 후보 계산 전
fail-closed와 우회 부재, timestamp/TTL 판정 부재, `ONLINE` 미추가, ephemeral 양방향 정합,
mixed scope·빈 식별자·미해석 membership 경계를 확인했다. 상태 전이·proto/protocol 변경이
없고 legacy 실행 코드가 동일하며 public API가 additive re-export뿐임도 대조했다. DoD-41
hard-filter 33/33, DoD-45 best-fit 20/20, 상태표 parity 5/5, legacy `ReplicaSet` 2/2를 직접
재실행하고 1라운드 `ACCEPTED`했다.

## 결과

```text
cargo test -p gputeer-checkpoint
  PASS — effective replica 13 + kill 7 + resume 7 +
         state-table parity 5 + write-failure 5, 0 failed

evidence 작성 시 같은 checkout 전체 재실행
  PASS — checkpoint 전체 67 passed, 0 failed
```

## 이 evidence가 증명하지 않는 것

- authoritative membership/failure-domain resolver와 durable ACK consumer가 아직 없어 이
  결과는 resolver가 제공한 평가 시점의 count이며 현재 liveness나 `MIRRORED` 상태 전이를
  증명하지 않는다.
- `MIRRORED` 판정 적용·상태 전이는 범위 밖이다.
- `ONLINE` 조건은 replica 규범에 없어 추가하지 않았다.
- freshness 판정은 kernel 밖 resolver 책임이며 kernel은 holder당 selected가 둘 이상이면
  fail closed한다.
- raw ACK의 current signature/key 재검증, Job durability projection, durable count 저장,
  production routing과 liveness/degraded 복구는 후속이다.

## 결정

순수 kernel의 입력과 계산 계약은 완료됐다. count 결과를 현재 durable truth나 checkpoint
상태로 승격하는 authority와 transaction은 추가하지 않았다.

scheduler 로드맵 진행: `DoD-41`~`DoD-54` 완료. `DoD-52` anchor → `DoD-53` ack 저장 →
`DoD-54` count kernel 로 `MIRRORED` 판정의 **입력과 계산**이 갖춰졌고, 남은 것은
authoritative resolver 와 전이 적용이다. membership/ControlStore 계열은 `Signable`·signer
identity·lifetime·member 상태 규범이 없어 규범 확정 약 2일, durable 저장까지 누적 약 3일로
별도 과제다.
