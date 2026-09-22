---
schema_version: 2
id: DoD-44
claim: "scheduler 로드맵 조각 3 을 'durable Agent inventory 저장소 커널(3a)'로 좁혀 완료했다. single-Coordinator SQLite의 CoordinatorInventoryStore가 복수 Agent registry를 충돌 방지·멱등 등록하고, 한 BEGIN IMMEDIATE transaction으로 parent/GPU/workload inventory를 원자 교체하며, 저장된 normalized fact를 node ID 순의 결정적 gputeer_scheduler::PoolSnapshot으로 투영함을 자체 재검토 수정 5건, rollback·revision·손상·별도 connection 경쟁·뮤테이션 검증, 독립 검수 1라운드 ACCEPTED 및 감독자 cargo test로 확인했다"
status: PASS
commit: 41d599ee7379ffadfa5a1b4d6579732e2300b843

executor_id: "agent:implementation-author"
executor_tool: "workspace-write 구현 세션 — 자체 재검토 5건 포함"
executor_model: "제공된 이력 요약에 모델 식별자 없음"
executed_at: "2026-08-21T12:45:54+09:00"

review_required: true
reviewer_id: "agent:independent-reviewer"
reviewer_tool: "대화 기록 없는 독립 검수 세션 — 1라운드"
reviewer_model: "제공된 이력 요약에 모델 식별자 없음"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "1라운드(ACCEPTED) — 단일 transaction 원자성(inventory_store.rs:223, rollback test 1213), register_agent 멱등성·충돌 검사(inventory_store.rs:147, conflict test 1132), revision 비교와 동일 revision 뮤테이션 판별력(inventory_store.rs:232), registry SQL ORDER BY 부재와 projection Rust sort 한 곳의 결정성(inventory_store.rs:366,588, test 1090), 자체 수정의 key validation(inventory_store.rs:425)과 경쟁 후보 판별력(test 1404), 제한된 변경 범위와 1,514줄 자기 보고를 확인해 잔여 지적 없이 ACCEPTED"
review_artifact: "docs/evidence/_raw/DoD-44_review.txt"

raw_output_artifact: "docs/evidence/_raw/DoD-44_scheduler_inventory_store_2026-08-21.txt"
raw_output_digest: "sha256:f12ce770fd36104f5dcc1a643dbf86dd09644834832345cec14445ca77aedd1b"
raw_output_bytes: 7058

binary_digests:
  toolchain: "C:\\Users\\playdata2\\.cargo\\bin\\cargo.exe 사용 — 제공된 이력과 이번 실행에 rustc/cargo version·binary digest는 기록되지 않음"
protocol_versions:
  schema_version: "proto 변경 없음 — 검증·정규화된 입력을 받는 Coordinator 내부 SQLite inventory 저장소 kernel"
  canonical_spec: "crates/scheduler의 기존 PoolSnapshot·CandidateSnapshot domain model을 path dependency로 재사용 — wire heartbeat/session 계약은 후속"
platform: "Microsoft Windows workspace / PowerShell / Rust cargo test / SQLite"
hardware: "GPU 미사용 — 합성 Agent/GPU inventory로 저장·투영·transaction·동시성 검증"
network_profile: "네트워크 미사용 — 같은 SQLite 파일의 별도 connection 두 개를 사용한 in-process 경쟁 테스트"
command: |
  C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-coordinator
raw_output: |
  (docs/evidence/_raw/DoD-44_scheduler_inventory_store_2026-08-21.txt,
   docs/evidence/_raw/DoD-44_review.txt 전문 참조)

  감독자 직접 확인: PASS — unit 67 + integration 4, 0 failed
  독립 검수 1라운드: ACCEPTED — 잔여 수정 요청 없음
artifacts:
  - docs/plans/2026-08-21_1208_scheduler_inventory_v1.md
  - Cargo.lock
  - crates/coordinator/Cargo.toml
  - crates/coordinator/src/lib.rs
  - crates/coordinator/src/inventory_store.rs
  - docs/evidence/_raw/DoD-44_scheduler_inventory_store_2026-08-21.txt
  - docs/evidence/_raw/DoD-44_review.txt
negative_tests:
  - "multiple_agents_reopen_and_project_in_node_order: 서로 다른 Agent 2개를 저장·reopen하고 역순 삽입에도 node/GPU가 결정 순서로 투영되며 Rust node sort 제거 뮤테이션이 실패"
  - "registry_retry_is_idempotent_and_every_identity_conflict_preserves_row: registry 10회 retry가 멱등이고 node/device/key/owner 충돌은 기존 행의 모든 필드를 보존"
  - "same_revision_requires_byte_equivalent_normalized_payload: 낮은 revision과 동일 revision의 다른 payload를 거부하고 GPU 입력 순서만 다른 normalized 동등 payload만 멱등 성공; <를 <=로 바꾼 뮤테이션이 실패"
  - "every_replacement_fault_rolls_back_parent_and_all_children: parent 기록 뒤·GPU child 교체 중·workload child 교체 중 오류를 주입해 이전 parent/GPU/workload 전체 보존"
  - "invalid_registry_and_inventory_inputs_have_no_side_effects: 공백 identity, 31-byte key, 빈·중복 GPU ID, 공백 GPU model, 미등록 node를 상태 무변경으로 거부"
  - "unknown_and_explicit_zero_facts_remain_distinct_in_projection: 미관측 None과 관측된 zero·empty·false를 구분해 projection"
  - "stale_and_future_observation_times_are_preserved_and_rejected_by_scheduler: stale·future observed_at을 보존하고 기존 scheduler freshness 판정으로 둘 다 거부"
  - "two_connections_competing_for_one_revision_leave_one_complete_payload: 별도 connection 두 개가 같은 next revision을 경쟁해 정확히 한 필드별 완전 payload만 남고 child가 섞이지 않음"
  - "corrupted_enum_key_revision_and_gpu_child_fail_closed_after_reopen: 손상 enum/key/revision/GPU child를 reopen 뒤 default 복구 없이 CorruptData로 거부"
  - "corrupt_payload_and_observation_markers_fail_closed: redundant normalized payload와 관측 marker boolean 손상을 fail closed"
limitations:
  - "scheduler 로드맵 조각 3 전체가 아니라 single-Coordinator durable Agent inventory repository kernel인 조각 3a만 증명한다"
  - "실제 다중 Agent 연결, 병렬 accept/task, wire heartbeat/capability producer, enrollment·identity·signature 검증은 없다"
  - "active-session owner, reconnect generation, 이전 session fencing·takeover·cleanup과 ONLINE→SUSPECT→UNREACHABLE→LOST 상태 판정은 없다"
  - "SQLite 로컬 durable truth만 제공하며 Raft·다중 Coordinator 합의·ControlStore COMMITTED 보증은 없다"
  - "scheduler winner/ranking, GPU reservation·admission, Grant dispatch와 Agent entrypoint 실행은 없다"
  - "telemetry 정확성이나 실제 GPU hardware를 실측하지 않았으며 저장소는 상위 검증 계층이 준 normalized fact만 보존한다"
  - "inventory_store.rs는 물리 1,514줄이고 production은 경계 기준 약 1,027~1,028줄로 계획 추정 350~600줄을 초과했다 — 명시적 enum codec, unknown/zero marker와 손상 대조가 원인이며 범위 밖 wire/session 기능을 넣은 결과는 아니다"
decision: "scheduler 로드맵 조각 3 전체나 live 다중 Agent 지원을 완료했다고 과장하지 않고, 검증·정규화가 끝난 복수 Agent registry/inventory를 single-Coordinator SQLite에 원자·영속 기록하고 기존 scheduler PoolSnapshot으로 결정 투영하는 조각 3a로 제한했다. register_agent는 동일 normalized payload만 멱등 처리하고 identity 충돌을 무변경으로 거부하며, update_inventory의 한 BEGIN IMMEDIATE는 parent/GPU/workload 교체와 revision 비교를 소유한다. 자체 재검토로 공개키를 Vec<u8>+명시적 32-byte 검사로 바꾸고 경쟁 후보 판별력을 전 필드로 보강했으며, SQL/Rust 이중 정렬을 projection의 Rust sort 한 곳으로 통일하고 fail-closed 경계·기존 API·교착 경로를 재확인했다. 독립 검수는 코드·테스트·뮤테이션·변경 범위·줄 수 자기 보고를 대조해 1라운드 만에 ACCEPTED했고 감독자는 coordinator 테스트 71건을 직접 재확인했다. scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a 완료 — 남은 조각 3 의 나머지(실제 다중 연결·heartbeat wire·session owner/fencing)와 조각 4~9 는 후속"
---

# DoD-44 · scheduler durable Agent inventory 저장소 kernel (로드맵 조각 3a)

## 무엇을 입증하려 했는가

상위 계층에서 identity·signature·capability 검증과 정규화를 끝낸 복수 Agent의
registry와 최신 inventory를 single-Coordinator SQLite에 영속 저장하고, 경쟁과
오류에서도 parent/GPU/workload가 섞이지 않으며 기존 scheduler의
`PoolSnapshot`으로 결정적으로 투영되는지를 검증했다.

## 범위 결정 — 조각 3을 3a로 축소

조각 3 전체에는 실제 다중 Agent 연결, heartbeat/capability wire producer,
active-session owner와 이전 session fencing이 포함된다. 현재 Coordinator는 단일
Agent identity/key와 순차 accept 구조이고 wire schema도 충분한 resource inventory를
싣지 않으므로, 이번에는 그 전체가 아니라 **durable Agent inventory 저장소
kernel(3a)**만 구현했다. 저장소 입력은 이미 검증·정규화된 사실이라는 경계를 갖는다.

## 구현 — `CoordinatorInventoryStore`

`crates/coordinator/src/inventory_store.rs`를 신설했다. `register_agent()`는 non-empty
node/device/owner identity와 정확히 32-byte Ed25519 공개키를 포함한 registry를
저장한다. 동일 normalized payload retry만 멱등이며 node/device/key/owner 충돌은
기존 행을 바꾸지 않고 거부한다.

`update_inventory()`는 한 `BEGIN IMMEDIATE` transaction에서 caller-provided 단조
revision을 비교하고 parent inventory와 GPU/workload child를 전량 교체한다. 낮은
revision과 동일 revision의 다른 payload는 fail closed하고 normalized 동등 payload만
멱등 성공한다. `pool_snapshot(evaluated_at_unix_ms)`은 한 read snapshot의 registry와
inventory를 기존 `gputeer_scheduler::PoolSnapshot`으로 투영하고 node ID 순으로
Rust에서 한 번 정렬한다. 미관측 값은 `None`이고 관측 시각은 stale/future여도
저장된 값을 그대로 보존한다.

## 자체 재검토 — 수정·확인 5건

공개키 API를 `[u8; 32]`에서 `Vec<u8>`로 바꿔 실제 잘못된 길이 입력을 명시적으로
검사하고 31-byte 입력과 손상 DB key를 fail closed했다. 두 connection 경쟁 후보는
GPU/workload/third-party opt-in까지 전부 다른 값으로 보강했다. SQL과 Rust의 중복
정렬은 제거해 projection의 Rust sort 한 곳만 결정성의 권위로 남겼다. 최대값,
unknown/zero, stale/future, 손상 row와 기존 API 무회귀를 재확인했고 wire/session
대기 루프가 없어 새 교착 경로가 없음을 추적했다.

## 독립 검수 1라운드 — **ACCEPTED**

검수자는 transaction 경계와 rollback test, registry 멱등·충돌 검사, 정확한 revision
비교와 `<`→`<=` 뮤테이션의 판별력, registry SQL의 `ORDER BY` 부재와 projection의
유일한 Rust sort, 32-byte key validation과 전 필드 경쟁 후보를 코드와 테스트로
확인했다. `job_store`·`lease_store`·`staging_store`·scheduler·agent·CLI 무변경과
dependency/module 등록 3줄·`Cargo.lock` 1줄의 추적 변경, 1,514줄 자기 보고도
대조해 잔여 지적 없이 1라운드 `ACCEPTED`했다.

## 결과

```text
C:\Users\playdata2\.cargo\bin\cargo.exe test -p gputeer-coordinator
  PASS — unit 67 + integration 4, 0 failed
```

위 결과는 이 evidence 작성 중 감독자가 직접 재확인했다.

## 이 실험이 증명하지 "않는" 것

- 실제 다중 Agent 연결과 heartbeat/capability wire producer는 없다.
- enrollment·signature 검증과 Agent hardware/NVML 수집은 없다.
- active-session owner·generation·fencing·takeover와 상태 판정은 없다.
- Raft·다중 Coordinator 합의와 분산 `COMMITTED` 보증은 없다.
- ranking·reservation/admission·Grant dispatch·Agent 실행은 없다.

## 결정

1. scheduler 로드맵 조각 3 전체가 아니라 durable Agent inventory 저장소 kernel인
   조각 3a를 완료했다.
2. 원자 교체·멱등/충돌·revision·결정성·손상·경쟁·뮤테이션 검증과 자체 재검토
   5건 반영을 독립 검수가 1라운드 만에 `ACCEPTED`했고 감독자가 coordinator
   테스트 71건을 직접 재확인했다.
3. scheduler 로드맵 9단계 중 조각 1·2a·2b-1·3a 완료 — 남은 조각 3 의 나머지(실제 다중 연결·heartbeat wire·session owner/fencing)와 조각 4~9 는 후속.

관련: `docs/plans/2026-08-21_1208_scheduler_inventory_v1.md`
