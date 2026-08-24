# 2026-08-24_1238 terminal AttemptReport durable binding v1

- **조사 대상:** `DoD-50` 뒤 scheduler 로드맵의 다음 정직한 하루 조각
- **선행 완료:** `DoD-41`~`DoD-50`
- **오늘 착수 결론:** **검증된 terminal `AttemptReport`의 durable Attempt/reservation binding**
- **예상 규모:** coordinator production Rust 100~180줄, migration/단위·재시작·손상 테스트
  180~300줄, 문서/evidence 별도 — **1일**
- **proto 변경:** 없음
- **production wire 연결:** 없음

## 결론

authoritative device→member 해석은 현재 저장소에 없다. wire schema에는 membership action과
`DeviceRecord.member_id`가 있지만, 이를 승인·폐기 순서대로 적용해 committed revision에서
조회하는 ControlStore 구현이 없다. Agent inventory의 `owner_member_id`와 crypto keyring의
signer key도 각각 scheduler용 caller-supplied registry fact와 서명 검증용 key 상태일 뿐,
submitter membership의 권위가 아니다. 규범상 membership 변경은 `COMMITTED`가 필요하고
그 소유 crate인 `crates/control-store/` 자체가 없으므로, 이 공백 전체는 하루 조각이 아니다.

기본 `MIRRORED` durability도 단순 enum 정규화만으로 소비됐다고 할 수 없다. 현재 scheduler
snapshot에는 replica 목적지, failure domain, replica 저장 가능성, 전송 대역폭이 없고 inventory
producer도 이를 저장하지 않는다. `0 -> MIRRORED` 순수 정규화는 하루보다 작지만, 그 요구를
만족하거나 거부할 authoritative admission 입력이 없어 실제 hard-filter 공백을 닫지 못한다.

Job terminal 상태만 먼저 추가하는 것도 안전한 최소 조각이 아니다. 현재 durable Job은
`STAGING`, Attempt는 `CREATED`까지만 표현한다. 규범의 Job `RUNNING -> COMPLETED`는 canonical
Attempt와 final artifact `COMMITTED` guard를 요구하고, 실패/취소도 서로 다른 원인과 정리
효과를 요구한다. `STAGING -> COMPLETED/FAILED`를 새 API 하나로 기록하면 상태기계를
건너뛴 거짓 terminal truth가 된다.

반면 proto와 protocol에는 서명 대상 terminal `AttemptReport`가 이미 있고, coordinator에는
durable Attempt의 `(job_id, attempt_id, node_id, fence_epoch)`와 동일 owner의 reservation이
있다. 따라서 보고서를 해석해 Job/Attempt를 terminal로 바꾸거나 reservation을 지우지 않고,
**현재 reservation owner가 제출한 검증된 terminal evidence를 first-write durable fact로
보존하는 일**은 지금 독립적으로 닫힌다. 이 조각은 실행 종료를 최종 증명하지 않지만,
재시작 뒤 report를 잃는 공백과 stale/wrong-attempt report가 나중 release 입력으로 섞이는
공백을 닫는다. 이후 terminal reconciliation과 release가 공유할 첫 durable 입력이다.

## authoritative device→member 조사

### `proto/`

- `proto/control.proto:241-244`의 `ControlAction`에는 `AddMember`, `RemoveMember`,
  `ApproveDevice`, `RevokeDevice`가 있다.
- `ApproveDevice`는 `device_id`, `member_id`, public key, peer ID, key protection,
  ephemeral 여부를 표현한다(`proto/control.proto:300-308`).
- 조회 schema도 `ControlQuery.device_id`와 `DeviceRecord { device_id, member_id,
  approved, risk_state, ... }`를 표현한다(`proto/control.proto:455-465`, `:524-535`).
- 그러나 `ControlState`에는 member record 목록이 없고, query에도 `member_id` 조회가 없다.
  member active/removed 상태, device 승인·폐기 revision, team scope를 한 snapshot으로 반환할
  완전한 membership view는 schema에도 아직 없다.
- membership/Job/Lease action은 규범상 `COMMITTED` 대상이다
  (`proto/control.proto:275-281`). local durable row 하나는 이 권위를 대체하지 못한다.

### `crates/protocol`

- membership message는 canonical field/domain이 있다
  (`crates/protocol/src/to_fields.rs:777-832`,
  `crates/protocol/src/canonical.rs:340-343`).
- 하지만 `AddMember`/`RemoveMember`/`ApproveDevice`/`RevokeDevice`에 대한 `Signable`
  구현과 action authorization/apply state machine은 없다. 현재 coverage는 canonical encoding과
  domain separation까지다.
- `Verified<M>`는 실제 검증에 사용한 signer ID만 보존한다
  (`crates/protocol/src/signing.rs:623-643`). `Verified<JobManifest>`는
  `submitter_device_id` key possession을 증명하지만 member/team 귀속을 만들지 않는다.

### `crates/coordinator`

- membership store, `ControlStore` trait 구현, member/device action consumer가 없다.
- `CoordinatorInventoryStore`의 `AgentRegistry`에는 `device_id`, `owner_member_id`,
  verifying key가 있지만, 모듈 계약은 caller가 identity-checked normalized fact를 제공한다고
  명시한다(`crates/coordinator/src/inventory_store.rs:1-6`, `:20-31`).
- registry는 최초 payload를 immutable/idempotent하게 저장할 뿐
  (`inventory_store.rs:145-202`), team ID, member active/removed state, owner authorization,
  device approve/revoke/quarantine revision을 적용하지 않는다.
- registry의 유일한 조회 key는 `node_id`이고, `device_id` lookup은 중복 검사 내부 helper다
  (`inventory_store.rs:131-143`, `:462-476`). submitter device directory로 사용할 공개 계약도
  없다.
- 따라서 Agent candidate의 owner 비교에는 쓸 수 있어도 arbitrary submitter device의
  authoritative member 해석에는 쓸 수 없다.

### keyring 판정

범위 밖의 `crates/crypto`도 “키링이 membership source인가”를 확인하기 위해 최소 대조했다.
`PersistentKeyring`은 `signer_id -> key versions/status`만 저장한다
(`crates/crypto/src/keyring.rs:206-214`, `:546-563`). member ID, team ID, 승인 revision은 없다.
`KeyDirectory` 주석은 membership까지 판단한다고 표현하지만 실제 trait 반환값은 signer key
lookup뿐이다(`crates/crypto/src/lib.rs:66-107`). 따라서 active key 검증 성공을 active member
귀속으로 확대하면 안 된다.

### 하루 규모 판정

authoritative source를 만들려면 적어도 다음을 함께 결정해야 한다.

1. team-scoped member/device durable model과 active/removed/revoked 상태
2. Owner/threshold signature 검증과 action별 authorization
3. `AddMember -> ApproveDevice -> RevokeDevice/RemoveMember` apply 순서와 idempotency
4. committed revision/read consistency를 포함한 `(team_id, device_id) -> active member_id`
   조회 결과
5. 같은 revision의 key directory view와 membership view 결합
6. restart/corruption/concurrent propose 및 revoke race 테스트

이는 누락된 `crates/control-store/`의 일부를 실질적으로 시작하는 작업이다. local SQLite
directory kernel만 좁게 만들 수는 있지만 규범의 `COMMITTED` authority가 아니므로
`JobRequirements.submitter_member_id`의 authoritative source라고 부를 수 없다.
**전체 공백은 1일 초과**로 판정한다.

## 후보 판정

| 후보 | (a) 지금 가능한가 | (b) 하루인가 | (c) 닫는 진짜 공백 | 판정 |
|---|---|---:|---|---|
| authoritative device→member directory | schema 조각은 있으나 apply/authorization/committed store가 없다. Agent registry와 keyring은 대체재가 아니다. | **아니오** | submitter device를 active team member로 해석하는 identity authority | 가장 중요한 상위 blocker지만 오늘 조각 아님 |
| `JobRequirements` projection | resolved member와 projection 밖 제약 consumer를 외부 입력으로 받는 순수 supported-subset 함수만 가능하다. | 함수만 1일, 안전한 ingress는 초과 | protobuf default/unknown을 scheduler fact로 오인하는 위험 | membership과 durability가 풀린 뒤 |
| 기본 `MIRRORED` durability 소비 | `0 -> MIRRORED` 정규화는 가능하지만 replica/failure-domain/storage/network producer가 없어 만족 판정은 불가능하다. 모든 기본 Manifest를 `Unsupported`로 막는 것만 가능하다. | 정규화만 1일 미만, 실제 admission은 초과 | 기본 durability를 조용히 버리는 위험 | 정규화만으로는 소비 경로를 닫지 못하므로 선택 안 함 |
| Job terminal 상태 전이만 추가 | enum/column 추가는 가능하지만 현재 `STAGING`/`CREATED`에서 규범 guard를 만족하는 경로가 없다. | 코드만 1일, 정직한 terminal transaction은 초과 | terminal Job truth와 향후 release trigger | 지금 만들면 상태기계 우회이므로 금지 |
| reservation release | report/runtime-stop/never-published proof가 없고 Lease revoke도 process 종료가 아니다. | **아니오** | 영구 점유와 capacity 고갈 | proof와 terminal transaction 뒤 |
| verified terminal `AttemptReport` durable binding | `Verified<AttemptReport>`와 durable Attempt/reservation identity가 이미 있다. state/release를 건드리지 않고 evidence만 저장할 수 있다. | **예, 1일** | restart 시 terminal evidence 유실, wrong job/node/fence report 혼입, changed replay overwrite | **오늘 선택** |
| Grant/Lease scope 또는 production outbox | UUID provenance, 확정 CPU/RAM/workspace, session routing과 publish state가 없다. | 아니오 | 잘못된 권한/오배송 및 DB-send crash window | 후속 |

## 왜 Job terminal이 아니라 report binding인가

규범 상태기계는 다음을 구분한다.

```text
Attempt RUNNING -> COMPLETED
  guard: exit code 0 + final artifact HASH_VERIFIED

Job RUNNING -> COMPLETED
  guard: canonical attempt 확정 + final artifact COMMITTED

reservation release
  guard: 정확한 owner workload가 멈췄거나 안전하게 fenced 됐다는 사실
```

서명된 `AttemptReport`는 Agent의 terminal claim이며 첫 입력이지만, 세 결론 중 어느 것도
혼자 증명하지 않는다. 특히 `COMPLETED` report 안의 nested artifact/checkpoint는 별도 서명,
hash, durability, canonical validity를 재검증해야 하고, `CANCELLED`/`INTERRUPTED` report도
process tree 종료나 VRAM 반환 확인을 자동으로 뜻하지 않는다.

따라서 이번 조각은 report를 **증거 inbox**로만 저장한다. Job/Attempt state, Lease,
reservation은 그대로 둔다. 이것이 다음 조각에서 검증·reconciliation·runtime-stop proof를
원자 terminal transaction에 결합할 수 있게 하면서 현재 상태기계를 거짓으로 앞당기지 않는
최소 경계다.

## 오늘 착수할 최소 조각

### 이름

**조각 5e / verified terminal AttemptReport durable binding**

### In

1. raw `pb::AttemptReport`가 아니라 `&Verified<pb::AttemptReport>`만 받는 coordinator
   storage entrypoint를 추가한다.
2. protobuf unknown/`ATTEMPT_OUTCOME_UNSPECIFIED`는 거부하고, 현재 schema가 아는 다섯
   terminal Attempt outcome만 저장한다. outcome을 Job terminal outcome으로 변환하지 않는다.
3. 한 `BEGIN IMMEDIATE` transaction에서 report를 현재 durable Attempt와 reservation에
   대조한다.
   - report `job_id == StoredAttempt.job_id == reservation.job_id`
   - report `attempt_id == StoredAttempt.attempt_id == reservation.attempt_id`
   - report `node_id == Verified::signer_id() == single-node Attempt.node_id == reservation.node_id`
   - report `fence_epoch == StoredAttempt.fence_epoch`
4. reservation이 없거나 owner가 다르면 거부한다. 이는 향후 release가 report를 정확한
   reservation owner의 evidence로 사용할 수 있게 하는 ingestion-time binding이다.
5. node signature를 포함한 complete protobuf body와 submission-time verified signer ID를
   복구 가능한 bytes로 저장한다. wire byte-for-byte 동일성을 주장하지 않는다.
6. `(attempt_id, node_id)`의 exact semantic replay는 최초 durable report를 반환한다. 같은 key의
   다른 outcome/body/signature는 overwrite하지 않고 typed conflict로 거부한다.
7. reopen/load는 `Verified<AttemptReport>`를 만들어내지 않는다. raw durable binding을 decode해
   저장된 attempt identity/fence/signer/body의 자체 일관성을 fail closed로 검사하고, 실제
   terminal consumer는 당시 authoritative key directory로 다시 검증해야 한다.
8. report insert 직후 fault injection으로 transaction rollback을 검증한다. 부분 row나 성공
   결과가 남아서는 안 된다.

### Out

- Agent의 실제 entrypoint/process tree 실행과 terminal report producer
- production session의 `FrameType::AttemptReport` routing
- Attempt `CREATED -> STARTING -> RUNNING -> terminal` 상태 전이
- Job `STAGING -> RUNNING -> COMPLETED/FAILED/CANCELLED/INTERRUPTED` 상태 전이
- nested artifact/checkpoint 독립 검증, durability/canonical 결정
- runtime-stop/VRAM 반환 ACK 또는 lease expiry+grace proof
- Lease revoke/종료와 node/GPU reservation release
- membership/KeyDirectory authoritative 동기화
- Raft/ControlStore `COMMITTED`, durable outbox, production orchestration 연결

### 완료 조건

1. `Verified<AttemptReport>`가 아니면 production API를 호출할 수 없다.
2. job ID, attempt ID, node/signer ID, fence epoch 중 하나라도 durable Attempt/reservation과
   다르면 report row를 만들지 않는다.
3. unknown/unspecified outcome은 fail closed하고 terminal evidence로 저장되지 않는다.
4. exact replay는 최초 body/signer를 반환하고 row 수가 늘지 않는다. 같은
   `(attempt_id, node_id)`의 변경 report는 conflict이며 최초 evidence를 보존한다.
5. store를 닫고 다시 열어도 signature를 포함한 semantic report와 owner/fence binding을
   복원한다.
6. 빈/truncated/undecodable body, body/row identity mismatch, signer mismatch, fence mismatch를
   typed corruption으로 거부한다.
7. report insert 뒤 injected failure는 report row를 남기지 않는다.
8. 저장 성공 전후 Job은 `STAGING`, Attempt는 `CREATED`, Lease와 reservation은 동일하다.
   이 invariant를 직접 검사해 evidence 저장이 terminal/release 부수효과를 가장하지 않게 한다.
9. mutation으로 signer/node 대조 또는 fence 대조를 제거하면 지정 negative test가 실패한다.
10. 기존 coordinator Job/Manifest/inventory/staging/Lease 테스트가 회귀하지 않는다.

### 예상 변경 소유권

- `crates/coordinator/src/staging_store.rs` 또는 작은 전용
  `crates/coordinator/src/attempt_report_store.rs`
- `crates/coordinator/src/lib.rs`의 module export만 필요한 범위
- coordinator unit/integration tests
- 구현 완료 후 별도 evidence/report/history

`proto`, `crates/protocol`, `crates/scheduler`, `crates/agent`, production `run()`은 이 조각에서
바꾸지 않는다.

## 다음 순서

```text
오늘: verified terminal AttemptReport + exact Attempt/reservation owner/fence durable binding
  -> Agent terminal report producer + production session routing
  -> Attempt STARTING/RUNNING 전이와 report validity/artifact guards
  -> runtime-stop 또는 safe-fencing proof
  -> Attempt/Job/Lease terminal transaction + atomic reservation release

병행 상위 경로:
authoritative committed membership directory
  -> strict Manifest normalization(MIRRORED 포함)
  -> JobRequirements projection + durability admission
  -> Grant/Lease scope + outbox/routing
```

이 순서는 report 저장을 “실행이 끝났다”로 과장하지 않으며, membership과 durability의 큰
선행 조건을 억지로 하루 조각으로 축소하지 않는다.

## 구현 결과 (2026-08-24)

### 구현

- `crates/coordinator/src/attempt_report_store.rs`를 추가했다.
  `CoordinatorAttemptReportStore::store_verified_terminal_report()`는 raw protobuf가 아니라
  `&Verified<pb::AttemptReport>`만 받고, `Verified::get()` 뒤에만 report 필드를 읽는다.
- `coordinator_attempt_reports` 테이블은 `(attempt_id, node_id)`를 primary key로 하고
  `job_id`, u64 big-endian BLOB `fence_epoch`, submission-time `verified_signer_id`, complete
  signed protobuf `report_body`, caller 입력이 아닌 body 재계산 BLAKE3 `report_hash`를 보존한다.
- 한 `BEGIN IMMEDIATE` transaction에서 exact replay를 확인한 뒤 신규 report에 대해 durable
  single-node Attempt와 현재 node reservation을 읽고 job/attempt/node/signer/fence 및 reservation
  owner를 모두 대조한 후에만 INSERT한다. changed body/outcome/signature는
  `ReportConflict`로 최초 evidence를 보존한다.
- load는 `Verified<AttemptReport>`를 만들지 않고 `StoredAttemptReportBinding`을 반환한다.
  body/hash decode, row↔body identity/signer/fence, row↔durable Attempt owner/fence를 검사하며,
  terminal consumer는 authoritative key directory로 signature를 다시 검증해야 한다.
- 기존 staging schema 초기화를 `pub(crate)` helper로 추출해 같은 control DB schema를 중복
  정의하지 않았다. Job/Attempt/Lease/reservation 전이·삭제 코드는 추가하지 않았다.

### negative test와 뮤테이션

- known terminal outcome 5종 저장, unspecified/unknown outcome 무행 생성 거부.
- wrong job/attempt, stale fence, Attempt node/signer binding 불일치, reservation 없음/owner 불일치
  무행 생성 거부.
- exact replay 1행 유지, changed outcome/body/signature conflict와 최초 evidence 보존.
- reopen 뒤 signature 포함 report/binding 복원, Job `STAGING`·Attempt `CREATED`·Lease·reservation
  동일성 직접 검사.
- report INSERT 직후 injected failure에서 transaction 전체 rollback.
- empty/undecodable body, body hash, job/attempt/node identity, signer, fence 손상을 typed corruption으로
  fail closed.
- 뮤테이션 1: Attempt node와 verified signer 대조를 제거하면
  `attempt_node_and_verified_signer_are_bound_to_durable_owner`가 FAILED(exit 101).
- 뮤테이션 2: fence 대조를 제거하면
  `wrong_job_attempt_and_stale_fence_create_no_row`가 FAILED(exit 101). 두 가드는 원복 후 지정
  테스트가 다시 통과했다.

### 검증

- `cargo test -p gputeer-coordinator`: unit 104 + integration 4, 모두 통과.
- `cargo build --workspace --exclude gputeer-runtime-windows`: 통과.
- `cargo test --workspace --exclude gputeer-runtime-windows --no-fail-fast`: 전체 통과.
  기존 crypto crash-child fixture 1건만 의도적으로 ignored.
- `cargo fmt -p gputeer-coordinator -- --check`: stable toolchain에 `rustfmt` component가 없어
  실행 불가. 별도 설치나 파일 정리는 하지 않았고, Rust compiler 경고는 없었다. Cargo는
  기존 환경의 `could not canonicalize path C:\Users\playdata2` 경고를 출력했다.
- 디스크 부족 오류는 발생하지 않았다.

### 자체 재검토와 제한

- public 저장 entrypoint에 raw `pb::AttemptReport` 경로가 없는지, INSERT가 하나뿐인지,
  report 저장 전후 상태 부수효과가 없는지, load가 `Verified`를 재구성하지 않는지 재검토했다.
- 첫 집중 테스트에서 사용하지 않는 fixture field 경고를 발견해 제거했고, 긴 오류 문자열 형식을
  정리했다. 안전성 가드의 추가 결함은 발견하지 못했다.
- 이 구현은 report를 terminal evidence inbox로만 보존한다. Job/Attempt terminal 전이,
  nested artifact/checkpoint 검증, runtime-stop 증명, Lease revoke/reservation release,
  production wire routing은 구현하지 않았다.

## 개정 이력

| 날짜 | 변경 |
|---|---|
| 2026-08-24 | 최초 작성 |
| 2026-08-24 | verified terminal AttemptReport durable binding 구현 결과 추가 |
