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

## 2026-08-16 18:20 — ★ 독립 검수(Codex) 지적 7건 시정 (DoD-08 PASS, 167 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T4 착수 전 계약 정비
- 스트림: Protocol · Crypto · Checkpoint · QA
- 배경: 이 세션의 결정(ADR-028 · ADR-029 · canonical 규칙)은 **전부 내가 혼자 판단하고
  내가 만든 테스트로 검증한 것**이다. 그 테스트가 놓친 것은 그 테스트로 못 찾는다.
  `CLAUDE.md` §4 대로 Codex CLI 에 **적대적 검토**를 맡겼다 — 동의가 아니라 반박을 요청
- ★★ **두 구현이 똑같이 틀린 곳 3건 발견** — 벡터 대조로는 원리적으로 못 잡던 것들
  1. **규칙 i-2** 중첩 메시지가 서명 필드만 가지면 `0a00`(빈 중첩)으로 **출력**됐다.
     `is_default()` 검사가 field 90 제외보다 먼저 일어나 규칙 i 가 새어나갔다.
     Python 도 동일 (`manifest={}` → `0801` vs `{90:sig}` → `08011a00`)
  2. **규칙 c-2** map 엔트리 안의 규칙 b 가 규범에 없었다
  3. **규칙 i-3** 도출 해시 제외가 **재귀 적용**돼
     `DatasetRef.retention`(field 4)이 `manifest_hash`(field 4)로 오인돼
     **데이터셋 삭제 정책이 서명에서 지워졌다**
- ★★ **검증 도구 자체가 검증하지 않고 있었다.**
  `--verify` 가 저장된 hex 끼리 관계만 봤다 — **저장본이 구현과 어긋나도 통과**한다.
  나는 `DoD-01` 이래 "vector cross-checks: OK" 를 검증 근거로 인용해 왔다.
  -> `build_vectors()` 재실행 대조로 고쳤다. 관계 없는 벡터 변조 뮤테이션으로 실효성 확인.
  -> **`DoD-01` 에 후속 정정을 추가**했다 (교차검증 자체는 Rust 테스트가 했으므로 유효)
- ★★ **replay nonce 가 메시지와 결속되지 않았다** (보안 영향 최대)
  `verify(msg, ..., nonce, ...)` 로 **호출자가 nonce 를 골랐다.**
  => 서명은 통과하는데 replay 방어만 무력화. 매번 새 값이면 무한 재생 가능
  -> `Signable::replay_nonce()` — **서명된 메시지 필드**에서 가져온다
  -> 검수자 조언대로 **SQLite 착수 전에 계약부터 고쳤다** ("지금 SQLite 를 추가하면
     잘못된 외부 nonce 를 영속 기록하는 구현이 된다")
- checkpoint (첫 독립 검토):
  - **K-1** `write_once` 가 "content-addressed 이름" 을 전제했는데 실제는 `shard-0.bin`.
    같은 이름·다른 내용을 조용히 수락 → **writer 가 "확정했다"고 거짓 보고**
  - **K-2** `RetryPolicy{max_attempts:0}` 이 `atomic.rs:139` 에서 **panic**.
    ADR-026 의 "최종 실패는 명시적 오류" 계약 위반 — panic 은 오류가 아니다
- 부수: `ReplayGuard` → `Result<ReplayDecision, ReplayStoreError>`,
  `VerifyError` 로 프로토콜 결과와 로컬 장애 분리,
  §6.1 `manifest_hash` **공식 자체**를 처음으로 검증
- 검증: **cargo test --workspace = 167 passed / 0 failed** (143 → 167). 빌드 경고 0.
  ★ **기존 벡터 canonical 변경 0건** — 회귀 없이 규범 구멍만 메웠다.
  `SCHEMA_FINGERPRINT` 불변(`.proto` 미변경)
- ★ **시정하지 않은 지적을 숨기지 않았다** — `RevokeLeaseNotice` 반복 전송(미실측),
  증거 3종 서명자 ID(V-08), `ReplicaAck.fence_epoch`(V-07),
  §8 5·6단계 순서(규범 수정 여부 별도 판단), `write_once` 메모리 사용(미측정)
- evidence: `DoD-08_독립검수_시정.md`

## 2026-08-16 16:40 — T2 증거 메시지 시각 정책 (ADR-029 · DoD-07 PASS, 143 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T2
- 스트림: Protocol · Crypto
- ★ **결정: `signing.md` §9 표에 없던 6종은 "권한" 이 아니라 "증거" 다.
  시각으로 만료시키지 않는다.** `Lifetime::Evidence` 신설 (ADR-029)
  - 과거의 사실은 만료되지 않는다. `CheckpointManifest` 를 만료시키면
    **오래된 체크포인트에서 재개할 수 없고**, 그것은 시스템의 존재 이유를 부순다
  - 그러나 "그 시점의 사실" != "지금의 사실" -> `Perpetual` 과 구분한다.
    `Evidence` 는 **`observed_at` 노출을 타입으로 강제**한다.
    "언제인지 모르는 증거" 는 증거가 아니다
  - 신선도는 시각이 아니라 `fence_epoch` 이 판단한다.
    **시계는 어긋나지만 epoch 은 어긋나지 않는다**
- ★ **`ReplicaAck` 만 `fence_epoch` 이 없다** — 6종 중 유일하다.
  그런데 `REPLICATED(n)` 을 세는 근거이므로 **durability 주장의 뿌리**다.
  **복제본이 삭제되어도 ACK 는 영원히 유효하다.**
  `replica_ack_stays_valid_forever_even_if_replica_is_gone` 이 이 결함을 고정한다 —
  **통과한다는 것이 곧 "프로토콜이 막지 못한다" 는 뜻이다.** -> V-07
- 수행: `Signable` 2종 -> **10종**. `ExecutionGrant` · `RenewLeaseRequest` 를
  `ShortLived` 로 구현 — **§9 단수명 경로가 실메시지로 처음 검증**되었다
  (`DoD-04` 는 테스트 전용 타입뿐이었다)
  - `RenewLeaseRequest` 는 `expires_at` 필드가 없어 `issued_at + GRANT_TTL_MS` 로 도출.
    값을 지어내는 게 아니라 §9 가 정한 TTL 적용이며 근거를 코드에 적었다
  - `Signable` 을 `signable.rs` 로 분리 — `to_fields` 는 "어떤 필드",
    `signable` 은 "어떤 domain·수명". **틀렸을 때의 증상이 달라** 섞으면 리뷰가 흐려진다
- 검증: **cargo test --workspace = 143 passed / 0 failed** (129 -> 143). 빌드 경고 0
  ★ `the_three_lifetimes_actually_behave_differently` — 세 정책이 실제로 다른
  동작을 하는지 확인. 전부 같으면 `Lifetime` 구분이 의미가 없다
- ★ TTL==skew 문제: **TTL 을 늘려 미래 방향 skew 를 "살리는" 것은 하지 않았다.**
  단수명 수명을 늘리면 replay 창이 커진다 —
  보안 매개변수를 코드 경로 도달성 때문에 바꾸지 않는다.
  대신 두 테스트로 사실을 고정(기본 TTL 에서 가려짐 + TTL 1시간이면 발동)
- ★ `.proto` 를 건드리지 않았으므로 `schema_version` 상향·벡터 재생성 불필요.
  `SCHEMA_FINGERPRINT` 불변
- 신규 등록: **V-07**(`ReplicaAck.fence_epoch`) · **V-08**(증거 메시지 서명자 ID).
  둘 다 `schema_version` 상향이 필요해 **함께 처리하는 것이 싸다**
- evidence: `DoD-07_시각정책_실메시지.md` · ADR: `ADR-029`

## 2026-08-16 15:40 — ★ 서명 재사용 취약점 발견·시정 (ADR-028 · DoD-06 PASS, 129 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T1b
- 스트림: Protocol · QA(벡터)
- ★★ **스펙 취약점 발견 — `signing.md` §5 가 자기 MUST 를 어기고 있었다.**
  §5 는 "메시지마다 새 domain_tag 를 등록해야 한다(MUST)" 라고 적어 놓고
  membership(6종) · policy · quarantine(2종) 을 **공유**하게 두었다.
  공유하면 §5 의 방어("tag 가 달라 반드시 실패한다")가 사라지고
  canonical 차이만 남는데, 규칙 b(기본값 생략) 때문에 **공격자가 필드를 비우면
  서로 다른 메시지가 같은 바이트가 된다.**
- 실측 (참조 구현 전수 대조) — **충돌 5쌍**:
  ```
  AddMember        == RemoveMember       공통필드[1]    28바이트 동일
  ApproveDevice    == RemoveMember       공통필드[1]    28바이트 동일
  ApproveDevice    == RevokeDevice       공통필드[1,2]  56바이트 동일
  RemoveMember     == RevokeDevice       공통필드[1]    28바이트 동일
  QuarantineDevice == ReleaseQuarantine  공통필드[1]    동일
  ```
  → 소유자의 `RemoveMember` 서명이 `RevokeDevice` 로 통과한다.
  → ★ **격리 판정 m-of-n 서명이 격리 해제로 재사용된다**
- 조치: **ADR-028** — 메시지별 domain_tag 분리 (17 → 23종).
  ★ **canonical bytes 는 하나도 바뀌지 않았다** (tag 는 sig_input 에만 들어간다).
  `schema_version` 상향 불필요, `SCHEMA_FINGERPRINT` 불변, 기존 벡터 회귀 0건
- ★ 회귀 방지 테스트는 **canonical 이 아니라 `sig_input`** 을 본다 —
  ADR-028 이후에도 canonical 은 여전히 같기 때문이다.
  canonical 을 검사하면 "우연히 필드가 달라서 통과"하는 약한 보증만 얻는다.
  전제(canonical 동일)도 `assert_eq!` 로 고정해 전제가 바뀌면 근거를 재확인하게 했다
- 수행: T1b — grant · membership · policy · quarantine. domain 9 → **19/23**.
  벡터 28 → 36건
- 부수:
  - **`DERIVED_HASH_FIELDS` 신설** — 규칙 i 의 의도적 제외(`manifest_hash`)와
    실수 누락(`UNIMPLEMENTED`)을 분리. 뜻이 정반대인데 섞으면 구분할 수 없다
  - `ExecutionGrant` 는 규칙 i 가 **두 번** 적용되는 유일한 메시지
    (도출 해시 + 중첩 manifest/lease 서명) → Agent 의 독립 검증·재계산이 필수
  - `PlacementRationale` 을 서명 대상에 넣었다. 처음엔 "설명용" 이라며 미뤘는데
    가드가 "위조 가능" 으로 실패시켰다 — **약화하지 않고 구현했다.**
    서명 밖이면 Coordinator 가 배치 근거를 사후 조작할 수 있다
  - `every_impl_is_audited` 가 신규 impl 15종을 잡아 등록을 강제했다
- 검증: **cargo test --workspace = 129 passed / 0 failed**. 빌드 경고 0
- evidence: `DoD-06_domain_tag_충돌.md` · ADR: `ADR-028`

## 2026-08-16 14:30 — T1 서명 대상 확장 + 규칙 j 신설 (DoD-05 PASS, 117 tests green)

- 계획: `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` T1
- 스트림: Protocol · QA(벡터)
- 수행: **계약 우선** — 참조 구현 확장 -> 벡터 생성 -> Rust 구현 -> 대조.
  `artifact.proto` 8종 + `lease.proto` 3종의 `ToCanonicalFields`.
  domain 커버리지 **2 -> 9 / 17**. 벡터 20 -> 28건
- 검증: **cargo test --workspace = 117 passed / 0 failed** (105 -> 117). 빌드 경고 0
- ★ **규범 공백 3건**을 찾아 전부 규범 문서에 기록:
  1. **규칙 j 신설** — `int64 value_micro` 가 스키마 전체에서 **유일한 부호 있는 필드**인데
     하필 서명 대상 안에 있었다. 규칙 없이는 구현마다 갈린다(2의 보수 10B vs zigzag 2B).
     canonical 은 유효한 protobuf 인코딩의 부분집합이어야 하므로 **2의 보수** 채택.
     ★ **기존 벡터 20건이 하나도 바뀌지 않았다** — 회귀 없음
  2. **§5 의 4종(genesis·audit·release·invite)은 proto 메시지가 아예 없다.**
     membership·policy·quarantine 은 여러 메시지가 한 tag 를 공유 -> §5.1 기록
  3. **§9 시각 정책 표에 6종이 없고 `expires_at` 필드조차 없다.**
     ★ **추측으로 채우지 않았다**(`CLAUDE.md` §1) — `Signable` 을 구현하지 않고
     §9.1 에 결정할 질문 3개를 적어 T2 로 미뤘다.
     따라서 그 6종은 **아직 `verify()` 를 통과할 수 없다**
- ★ 중첩 서명의 귀결 고정: 규칙 i 재귀로 중첩 서명은 바깥 canonical 에 들어가지 않는다.
  **검증자는 중첩 서명 메시지를 독립 검증해야 한다(MUST)** — 안 하면 REPLICATED(n) 이 거짓이 된다.
  "중첩 전체가 무시되는" 결함과 구분하려고 반대 방향 테스트도 함께 넣었다
- evidence: `DoD-05_T1_서명대상_확장.md`

## 2026-08-16 13:30 — 스트림 소유권 위반 시정 + 실행계획 v2 (105 tests green)

- 계획: 이 커밋으로 `docs/plans/2026-08-16_1330_프로토콜_완성_실행계획_v2.md` 착수
- 스트림: Protocol · Crypto · QA
- ★ **내가 `RULE.md` §4.1 을 어겼다.** `crates/protocol/src/signing.rs` 가
  `ed25519-dalek` 을 직접 썼는데 소유권 표는 Ed25519 를 Crypto 스트림 소유로 정한다.
  **테스트 100건이 전부 통과했고 아무도 알아채지 못했다.**
  테스트가 통과한다고 규칙을 고치지 않고 **코드를 규칙에 맞췄다** (§4.3 마지막 줄)
- 수행:
  - `crates/protocol`: `SignatureVerifier` trait 신설. **암호 라이브러리 의존 0**
    (blake3 만 예외 — canonical 의 일부)
  - `crates/crypto` 신설: `Ed25519Verifier` · `sign()` · `InMemoryKeyring`
    (이름이 "운영에 쓰면 안 됨"을 드러낸다 — §11 K0~K2 미구현)
  - `crates/protocol/tests/stream_ownership.rs` 5건 — 경계를 **코드로 강제**.
    뮤테이션(ed25519 재추가)으로 실효성 확인
  - **실행계획 v2 작성** — v1 범위가 소진됐는데 작업이 계획 밖에서 이어지고 있었다.
    T1~T6 확정. D-5 를 4건으로 갱신
  - `CLAUDE.md` §5 상태표를 디스크·빌드 실측으로 갱신
- 검증: **cargo test --workspace = 105 passed / 0 failed**. 빌드 경고 0. 문서 검사 통과
- 부수 정정: `UnknownSigner` 와 `InvalidSignature` 를 구분해 반환하도록 trait 계약에 명시.
  뭉뚱그리면 운영자가 "팀 멤버가 아니다"와 "위조되었다"를 구분할 수 없다
- 리포트: `docs/reports/2026-08-16_1330_프로토콜_계층_완주_자율세션.md` (세션 종합)

## 2026-08-16 12:40 — Ed25519 서명·검증 + Verified<M> (DoD-04 PASS, 100 tests green)

- 계획: 계획 밖 — `DoD-01`~`DoD-03` 이 모두 limitations 에 남긴 "Ed25519 미구현"
- 스트림: Protocol
- 수행: `crates/protocol/src/signing.rs` — `signing.md` §8 검증 순서 · §9 시각 정책 · §13.2.
  `tests/ed25519_verify.rs` 20건 + 독테스트 2건
- 검증: **cargo test --workspace = 100 passed / 0 failed** (78 -> 100). 빌드 경고 0건
  - §8 의 9단계가 **각각 실제로 발동**함을 확인. 발동하지 않는 단계는 없는 것과 같다
  - 보안 필드 변조 **9종 전부 거부** (DoD-03 이 서명에 넣은 것들이 실제로 지켜지는가)
  - ★ `Verified<M>` 우회 생성 차단을 `compile_fail` 독테스트로 검증.
    **필드를 pub 으로 바꾸는 뮤테이션**에서 FAILED, 원복 시 통과
- 설계 결정: `VerifyOutcome` 에 **`VALID` 를 두지 않았다.** 성공은 다른 타입이다.
  열거형에 VALID 를 두면 새 실패 값이 조용히 통과한다
- ★ 발견 2건 (둘 다 테스트가 처음에 실패해서 드러났다):
  1. `minimum_security_tier = 0` 변조가 **no-op** 이었다 (기준값이 이미 0).
     -> 모든 변조 케이스에 `assert_ne!(변조본, 원본)` 비공허성 단언 추가
  2. Grant 기본 TTL(60초) == skew 허용치(60초) 라서
     **미래 방향 skew 경로가 만료 검사에 가려져 도달 불가능**하다.
     안전성 문제는 아니나 "skew 검사가 동작한다"고 잘못 믿게 된다
  3. `compile_fail` 독테스트를 처음에 `tests/` 에 두었는데 **실행되지 않는다.**
     검증하지 않는 것을 주장하고 있었다 -> `src/` 로 이동
- 미구현 명시: **§8-8 replay 는 저장소 계층이 없어 실제 방어가 없다.**
  `NoReplayCheck` 라는 이름으로 사실을 드러내고 `require_replay_checked()` 가
  미검사 값의 부작용 경로 사용을 막는다. 조용히 빠뜨리지 않았다
- evidence: `DoD-04_ed25519_검증순서.md`

## 2026-08-16 11:30 — P0-08 스키마 진화 (PASS, 78 tests green)

- 계획: 계획 밖 — `DoD-02` 가 제기한 "CLAUDE.md §0.2 vs prost 기본 동작 충돌"
- 스트림: Protocol
- 수행: `tests/schema_evolution.rs` 6건(q1~q6) + `tests/schema_fingerprint.rs` 2건 +
  `proto/SCHEMA_FINGERPRINT.txt`(66 메시지 · 389 필드)
- 검증: **cargo test --workspace = 78 passed / 0 failed** (70 -> 78)
  - **prost 는 미지 필드를 조용히 버린다** — 118B -> 148B(주입) -> 118B(재인코딩).
    오류도 경고도 없다. canonical 에도 흔적이 없다
  - => 구버전은 본문만 보고는 새 필드의 존재를 알 수 없다. **유일한 신호는 schema_version**
  - `schema_version` 은 canonical(필드 1)과 sig_input **양쪽**에 묶여 강등은 서명을 깬다
  - => **§7.2 SCHEMA_TOO_NEW 는 구현 가능하다. 아키텍처 재검토 불필요**
- ★ 새 발견: **§7.3 "schema_version 증가 없는 필드 추가 금지" 를 프로토콜은 강제하지 못한다.**
  한 줄짜리 실수가 조용한 보안 우회가 된다 (구버전이 새 보안 제약을 무시한 채 통과)
  -> `SCHEMA_FINGERPRINT.txt` + 대조 테스트로 **빌드 시점 강제**.
  뮤테이션(`bool require_attestation = 63` 추가)으로 실효성 확인 —
  "추가된 줄: 63 bool require_attestation" 을 출력하며 실패
- 부수 발견: 버전 검사를 서명 검사보다 먼저 해야 하는 이유는 **안전성이 아니라 진단 정확성**.
  순서를 뒤집으면 "업그레이드 필요" 를 "서명 위조" 로 보고한다
- 결정: `signing.md` §7.2 변경 없음. §7.3 에 강제 장치 규범 추가. §8 근거 정정.
  **V-05 등록** — `oneof` 도입 시 지문 파서가 무력해진다.
  **V-06 등록** — 서명된 정책 필드의 강제 계층 (v0.1 필수)
- evidence: `P0-08_스키마_진화.md`

## 2026-08-16 10:40 — 서명 밖 필드 6건 제거 (DoD-03 PASS, 70 tests green)

- 계획: 계획 밖 — `DoD-02` 가 찾은 "위조 가능한 보안 필드 3건" 을 닫는 작업
- 스트림: Protocol · QA(벡터)
- 수행: **계약 우선 순서 준수** — 참조 구현 확장 -> 벡터 생성 -> Rust 구현 -> 대조.
  `reference_canonical.py` SCHEMAS 에 6종 추가 + JobManifest 를 전 필드로 + Lease 신설.
  벡터 12 -> 20건. `to_fields.rs` 에 6종 impl + JobManifest 10/11/12/54/55 · Lease 40 편입
- 검증: **cargo test --workspace = 70 passed / 0 failed** (55 -> 70).
  `v02_full_manifest` **896바이트가 Python 참조 구현과 바이트 일치**(BLAKE3 까지).
  ★ `every_field_in_full_manifest_affects_canonical` — 27개 필드를 하나씩 지워
  canonical 이 반드시 변하는지 확인. **27/27 전부 서명 반영**.
  벡터 대조만으로는 "두 구현이 사이좋게 같은 필드를 빠뜨린" 경우를 못 잡는다
- 발견: **`v02_full_manifest` 는 "모든 필드" 라고 적혀 있었지만 16개 부분집합이었다.**
  주장과 실제가 어긋난 만큼은 아무도 검증하지 않는다.
  -> `missing_from_full()` 로 벡터 생성 시점에 코드가 검사하게 했다
- 결정: `UNIMPLEMENTED_FIELDS` **비었다**. DoD-02 의 "위조 가능한 보안 필드 3건" 해소.
  단 **"서명에 들어갔다"와 "Agent 가 그 정책을 강제한다"는 다르다** — 강제 계층 미구현
- 리포트: `docs/reports/2026-08-16_0930_prost_연동계층_자율세션.md` (§12 갱신)
- evidence: `DoD-03_서명대상_완전성.md`

## 2026-08-16 09:30 — prost 연동 계층 (DoD-02 PASS, 55 tests green)

- 계획: 계획 밖 — `DoD-01` limitations 1·2번을 닫는 작업
- 스트림: Protocol
- 수행: `build.rs`(protoc-bin-vendored) · `src/to_fields.rs`(ToCanonicalFields 수동 구현) ·
  `tests/prost_canonical.rs` 11건 · `tests/field_number_audit.rs` 6건
- 검증: **cargo test --workspace = 55 passed / 0 failed** (38 -> 55).
  실제 prost 메시지가 Python 참조 구현과 바이트 일치(BLAKE3 까지).
  ★ map 500회 재구축에서 **prost 495종 vs canonical 1종** — 비공허성 단언 포함.
  field_number_audit 은 **뮤테이션 2종**(중복 경로·이름 불일치 경로)으로 실효성 확인
- 발견:
  1. `job.proto` 에 `import "lease.proto"` 누락 — **5개 proto 를 한 번도 컴파일한 적이 없었다**
  2. 내 negative test 주장이 틀렸다. map 없으면 prost==canonical(113B/113B).
     테스트를 느슨하게 고치지 않고 **근거를 다시 세웠다**
  3. ★ **서명에서 빠진 필드 6건, 그 중 3건이 보안 필드**
     (JobManifest 54 network · 55 artifact_scope · Lease 40 scope). 현재 **위조 가능**
  4. 계획서 §15.2 `bytes submitter_device_id` vs proto `string` 드리프트
- 결정: `signing.md` §3 규칙 변경 없음. §13.1 **근거 문구 정정**(규범 아님) +
  수동 구현 채택 명시 + `UNIMPLEMENTED_FIELDS` 선언 의무화.
  **P0-08 신규 등록** — `SCHEMA_TOO_NEW` × prost unknown-field.
  `CLAUDE.md` §0.2 와 prost 기본 동작이 정면 충돌한다.
  D-5 기준선 수정 요청 **3건 -> 4건**
- 리포트: `docs/reports/2026-08-16_0930_prost_연동계층_자율세션.md`
- evidence: `DoD-02_prost_연동_계층.md`

## 2026-08-16 08:10 — P0-03 카오스 테스트 완주 (38 tests green)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md` S3
- 스트림: Checkpoint
- 수행: `crates/checkpoint/src/writer.rs` (write_checkpoint / find_resume_point / startup_gc),
  `src/bin/ckpt_writer.rs` 카오스용 바이너리, `tests/kill_chaos.rs` 7건.
  별도 프로세스를 띄워 8개 고정 시점(40~700ms)에 실제로 kill
- 검증: **cargo test --workspace = 38 passed / 0 failed** (기존 31 + kill_chaos 7)
  불변식 4개 전부 통과. ★ 테스트가 공허하지 않음을 별도 검증 —
  8회 중 7회에서 PARTIAL 발생, `kill@560ms` 에서 **manifest.json.tmp**(매니페스트 쓰는 도중) 포착
- 결정: **P0-03 PASS.** local-first 원칙(§18.1) 재검토 안 함.
  단 COMMITTED durability 주장은 **HASH_VERIFIED 까지만 입증** — 복제 계층 미구현.
  P0-03b(복제) · P0-03c(전원 차단) 신규 등록
- 리포트: `docs/reports/2026-08-16_0700_P0스파이크_3건_자율세션.md` (§7 갱신)
- evidence: `P0-03_checkpoint_durability.md`

## 2026-08-16 07:00 — P0 스파이크 3건 (P0-01 PASS · P0-07 PASS · P0-06 FAIL-SCOPE)

- 계획: `docs/plans/2026-08-15_1330_P0_스파이크_실행계획_v1.md`
- 스트림: QA · Runtime
- 수행: x600 에 Rust 1.97.1 설치. SSH 전달을 base64 -> scp+`-File` 로 교체.
  P0-01(Windows S1+CUDA) · P0-07(추정 정확도) · P0-06(VRAM 강제) 실측
- 검증:
  **P0-01 PASS** — Restricted Token 에서 CUDA 완전 동작. Job Object 종료 시 VRAM 168->489->168 반환
  **P0-07 PASS** — sigma=0.021 (DoD 0.20). warmup 10->300 으로 오차 9.3%->1.2%
  **P0-06 FAIL-SCOPE** — 기준선 §10.3 의 "Job Object 는 VRAM 무관" 이 Windows 에서 **틀렸다**.
  5x5 스윕으로 `VRAM 최대 ~= RAM 제한 - 2000MiB` 확인
- 결정: ADR-005 유지 · ADR-007 유지 · **ADR-015 유지** · **ADR-027 신설**.
  기준선 수정 3건 승인 대기 (ADR-026 · ADR-027 · §12.3 warmup)
- ★ 오판 5건 정정 기록. 특히 "빈 출력 -> CUDA 실패" 오판을 잡지 못했으면
  ADR-005 를 뒤집고 Windows S1 을 로드맵에서 제거했을 것
- 리포트: `docs/reports/2026-08-16_0700_P0스파이크_3건_자율세션.md`
- evidence: `P0-01` · `P0-07` · `P0-06`

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
