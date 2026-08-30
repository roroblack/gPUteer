---
schema_version: 2
id: DoD-63
claim: "`ADR-033` §7 의 **이웃 신고**(관측 층) wire 메시지를 신설했다. `proto/lease.proto` 의 `NeighborUnreachableReport` 는 **판정을 담는 필드가 하나도 없다** — §7 이 '관측을 판정 결과로 승격하지 않는다' 고 못박았으므로 `is_dead` 같은 값을 넣지 않았고, 담기는 것은 신고자가 본 사실(관측 시각)뿐이다. 정족수 단위는 `ADR-033` §8 조건 3 원문대로 **기계**(`reporter_node_id`)다. `Lifetime::ShortLived` 로 재생을 막고, canonical/서명/프레이밍 체인 전체와 Python 참조 구현 바이트 교차검증 벡터 2건을 갖췄다. ★ 이 메시지가 증명하지 **않는** 것 — 신고자가 정당한 풀 이웃인지(멤버십 해소 없음), 그 장치가 주장한 기계 ID 의 실제 소유자인지, 수신 Coordinator 가 맞는지 — 은 전부 **테스트로 고정된 열린 구멍**이다. 부수적으로 이 저장소가 다섯 번 겪은 '손으로 쓴 domain 목록이 낡는' 결함의 **남은 두 자리**(규범 문서·Python 참조 구현)를 3자 대조 테스트로 닫았다"
status: PASS
commit: e13b15ad509092552b137c4acb7dae2109a311cc

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — proto 신설, canonical/signing 체인, framed_ingress dispatch, 거부 경로 테스트 8건, 3자 domain 대조 테스트"
executor_model: "claude-opus-5"
executed_at: "2026-08-31T01:20:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 12라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "12라운드에 걸쳐 파일:줄로 지목된 지점 — 1R: `signing.md:282` 종수 미갱신, `signable.rs:601` `GRANT_TTL_MS` 를 신고 TTL 로 쓸 규범 근거 없음, **`signable.rs:582` 등 세 곳의 '재생하면 이웃 하나가 정족수를 혼자 채운다' 서술이 `reassignment.rs:952` 의 실제 집계와 모순**, `lease.proto:324` 서명이 device 만 인증하고 주장된 `reporter_node_id` 는 인증 안 함, `lease.proto:333` `coordinator_device_id` 재사용 방지가 강제되지 않음, `lease.proto:338` `last_contact_at`·`failed_attempt_count` 가 YAGNI 이고 모순된 값도 통과, §7.3 근거 오기, `signable.rs:573` 문서 주석이 남의 impl 에 붙음. 2R: 그 세 서술이 여전히 남음, 주석 이동이 실제로 안 됨, `signing.md:494` 지문 규모 66/389 가 실제 72/449 와 다름. 3R: `framed_ingress.rs:1049` 'unknown signer' 가 실제로는 `InvalidSignature` 를 냄, `:1017` domain 분리를 재지 않는데 그렇게 주장, `:1092` 두 nonce 가 실제로 같고 replay guard 를 매번 새로 만들어 숨김. 4R: **`canonical_vectors.rs:349` 의 수동 배열이 30종 중 28종만 검사**(다섯 번째 같은 결함), 주석의 낡은 종수 2건. 5R: **`reference_canonical.py:679` 와 `signing.md:249` 가 독립 수동 목록이라 여섯 번째가 날 자리**, 낡은 개수 3건, 존재하지 않는 테스트 이름. 6R: 새 3자 대조 파서가 파일 전체를 훑어 목록 밖 언급이 삭제를 가림. 7R: 표/딕셔너리 범위 미한정, 블록 안 주석 미제외, **집합만 비교해 두 tag 를 맞바꿔도 통과**. 8R: **양쪽 문서를 함께 맞바꾸면 통과**, Python 파서가 값 리터럴 밖의 tag 를 읽음. 9R: coverage 배열 '권위' 서술 과장, 권위 대응 메시지가 문서에 없으면 건너뜀, 주석 미제외, `:` 뒤 값 미확인, `signing.md` '남은 자리를 닫는다' 과장. 10R: 우회 비용이 경우에 따라 3자리인데 '네 자리' 라고 씀, coverage 이름 대응이 30종이 아니라 26종. 11R: '30종 전부' 잔재, `t1b_grant_and_control.rs:466` 주석이 코드와 자기모순. 12R: `ACCEPTED` — 이웃 신고의 proto 필드·canonical 범위·독립 domain·서명자 선택·수명·nonce replay·frame dispatch·거부 경로에서 결함을 찾지 못함"
review_artifact: "docs/evidence/_raw/DoD-63_review_all_rounds_verbatim.txt"

decision: "`lease.proto` 주석이 '이웃 신고는 pool membership 이 먼저 있어야 한다' 며 미뤄 뒀던 것을, **메시지 정의와 멤버십 해소가 다른 일**이라는 구분 위에서 앞의 것만 넣는다. 판정 필드를 하나도 두지 않는다 — `is_dead` 를 넣으면 관측이 판정으로 승격되고 `ADR-033` §7 이 금지한 것이 정확히 그것이다. 정족수 단위는 §8 조건 3 원문대로 **기계**다. `Lifetime::ShortLived` 를 쓰되 **전용 상수**(`NEIGHBOR_REPORT_TTL_MS`)로 분리한다 — `GRANT_TTL_MS` 는 'ExecutionGrant 기본 수명'(기준선 §15.4)이고 신고에 적용할 규범 근거가 없다. `last_contact_at_unix_ms`·`failed_attempt_count` 는 **뺐다** — §7·§8 이 요구하지 않고 소비자도 없는데 `last_contact_at > observed_at` 같은 모순된 값도 서명만 맞으면 통과해 공격 입력면만 넓혔다(`RULE.md` YAGNI). 강제되지 않는 것은 **테스트로 열린 채 고정한다** — 한 장치 키가 여러 기계 ID 를 서명할 수 있다는 것과 프레이밍 계층이 수신 Coordinator 를 대조하지 않는다는 것을 각각 통과하는 테스트로 남겨, 나중에 닫히면 그 테스트가 실패하며 문서도 같이 고치라고 알린다(`runtime-linux` 의 '탈출이 성공하기를 기대하는 테스트' 와 같은 장치). 부수적으로 발견한 다섯 번째·여섯 번째 목록 노후화 자리는 `Domain::ALL` 순회와 3자 대응 대조로 닫는다."
raw_output_artifact: "docs/evidence/_raw/DoD-63_neighbor_report_2026-08-31.txt"
raw_output_digest: "sha256:826ca796aecbefce98fdecdc5bef224fd2eba591af40697b0b380612bf912ea9"
raw_output_bytes: 3987

artifacts:
  - "docs/evidence/_raw/DoD-63_neighbor_report_2026-08-31.txt"
  - "docs/evidence/_raw/DoD-63_review_all_rounds_verbatim.txt"

binary_digests:
  toolchain: "Windows 개발 기계 cargo 1.97.1"
protocol_versions:
  schema_version: "★ 새 메시지 `NeighborUnreachableReport` 를 schema v1 로 신설. 기존 메시지의 `schema_version` 은 올리지 않았다 — `signing.md` §7.3 의 (b) 가 아니라 **'이 타입의 구버전 인스턴스가 없으므로 새 타입은 v1 에서 시작한다'** 가 정확한 근거다(독립 검수 1라운드 정정). domain `gputeer/v1/neighbor-unreachable` 추가, domain 29 → 30종"
  canonical_spec: "canonical 벡터 50 → **52건**(`v38_neighbor_unreachable_report`, `v38b_neighbor_unreachable_different_target`). 스키마 지문 갱신(72개 메시지·449개 필드)"
platform: "Windows 11 개발 기계. ★ 이번 조각에 Linux 회귀는 **없다** — 순수 프로토콜/직렬화 코드라 플랫폼 의존이 없지만 확인한 것은 아니다"
hardware: "GPU 무관"
network_profile: "프레이밍 테스트는 `Cursor` 인메모리 스트림. 실제 소켓을 쓰지 않는다"
command: |
  cargo test -p gputeer-protocol
  cargo test -p gputeer-crypto --test framed_ingress
  python tools/canonical/reference_canonical.py --self-test
  python tools/canonical/reference_canonical.py --verify tests/vectors/canonical_v1.json
  python tools/canonical/check_schema.py
  cargo test --workspace
  # 뮤테이션 8건 + 목록 노후화 우회 재현 9건 (raw 5·6절에 실제 stdout)
raw_output: |
  === cargo test -p gputeer-protocol ===
  전체 통과 (canonical_vectors 16 · t1_signing_targets 15 · lifetime_consistency 4 ·
             field_number_audit 10 · t1b_grant_and_control 등)

  === cargo test -p gputeer-crypto --test framed_ingress ===
  test result: ok. 31 passed; 0 failed   (이웃 신고 거부 경로 8건 포함)

  === canonical 체인 ===
  all checks passed
  재생성 대조: 52개 벡터 일치 / vector cross-checks: OK
  오류 0건 (SCHEMAS 48개 메시지 / .proto 90개 메시지) / schema 검사 통과

  === cargo test --workspace ===
  783 passed

  === 뮤테이션 8건 — 전부 지정 테스트를 **동작 수준에서** 실패시켰다 ===
  N1  지목 노드를 canonical 에서 제거     -> neighbor_unreachable_report_matches_reference
  N2  신고 기계를 canonical 에서 제거      -> neighbor_unreachable_report_matches_reference
  N3  replay nonce 를 보고하지 않음        -> replaying_a_neighbor_report_is_rejected
  N4  lifetime 을 LongLived 로 강등        -> messages_whose_replay_would_forge_facts_must_stay_shortlived
  N4b NodeHeartbeat 도 같이 강등           -> (같은 테스트) ★ 기존 구멍이었다
  N5  domain 을 heartbeat 와 공유          -> all_domain_tags_are_distinct
  N5b 같은 뮤테이션                        -> domain_tags_are_32_bytes_and_unique
  N6  서명자를 신고자가 아닌 지목 노드로    -> a_signed_neighbor_report_dispatches

  === 목록 노후화 우회 재현 9건 — 전부 잡혔다 ===
  A 규범 표 행 삭제                          F Python 값 틀리고 주석에 맞는 tag
  B Python 항목 주석 처리                    G Python 값이 함수 호출, 주석에 tag
  C Python 두 tag 맞바꾸기                   H 두 문서에서 메시지 이름 함께 변경
  D 규범 표 두 tag 맞바꾸기                  I 양쪽 문서 맞바꾸기 + coverage 주석 공격
  E 양쪽 문서를 함께 맞바꾸기

  ★ 위는 발췌·재구성이다. **필터링·수동 결합한 실제 출력 발췌**이며
    `raw_output_artifact` 파일이 원본이다(뮤테이션 두 절은 스크립트 stdout 그대로).

negative_tests:
  - "위조된 서명은 `FramingError::Verify` 로 거부"
  - "★ 같은 신고를 두 번 보내면 두 번째는 **정확히 `VerifyOutcome::Replay`** 로 거부 — `is_err()` 로 뭉뚱그리지 않는다"
  - "디렉터리에 없는 신고자는 `UnknownSigner`, 같은 ID 다른 키는 `InvalidSignature` — **둘을 나눠 각각 정확한 오류로** 고정"
  - "신고 몸통을 heartbeat 프레임으로 보내면 거부(단 이것은 domain 분리를 재는 것이 **아님**을 주석에 명시)"
  - "TTL 을 넘긴 신고는 거부"
  - "★ 한 장치 키가 서로 다른 `reporter_node_id` 를 서명할 수 있음을 **통과하는 테스트로 고정** — 구멍이 닫히면 실패해서 알린다"
  - "★ 프레이밍 계층이 `coordinator_device_id` 를 대조하지 않음을 같은 방식으로 고정"
  - "`NodeHeartbeat`·`NeighborUnreachableReport` 의 lifetime 강등을 값으로 고정(기존에 아무도 안 잡던 구멍)"
  - "지목 노드만 바꾼 canonical 대조쌍 — 대상 위조로 서명을 재사용할 수 없음"
  - "규범 문서·Python 참조 구현·코드의 메시지→tag 대응 3자 대조(우회 9건 재현 확인)"

limitations:
  - "★ **신고자가 정당한 풀 이웃인지 확인하지 못한다.** 멤버십 해소가 이 저장소에 없다 — `crates/scheduler/src/reassignment.rs` 가 그것을 호출부 진술로 요구하는 이유이고, wire→커널 어댑터도 아직 없다"
  - "★ **서명은 장치만 인증한다.** 유효한 장치 키 하나가 서로 다른 `reporter_node_id` 를 N 개 서명할 수 있다 — 정족수를 기계 수로 세므로, 소비자가 authoritative device→node 결합을 해소하기 전에 세면 장치 하나가 N 표를 만든다. **테스트로 고정된 열린 구멍**이다"
  - "★ **프레이밍 계층은 `coordinator_device_id` 를 자기 ID 와 대조하지 않는다.** canonical 에 들어가므로 변조는 막히지만 **대조는 소비자 몫**이다(`CLAUDE.md` §0.4). 이것도 테스트로 고정했다"
  - "★ 재생 방어가 막는 것은 **신선도 위조와 중복 부작용**이지 정족수 조작이 아니다 — `reassignment.rs` 가 `reporter_node_id` 로 중복 제거하므로 같은 신고를 N 번 넣어도 한 표다(초안이 세 곳에서 반대로 서술했고 독립 검수가 정정했다)"
  - "★ `NEIGHBOR_REPORT_TTL_MS = 60_000` 은 **규범이 정한 값이 아니다.** `ADR-033` §7 은 신고 TTL 을 정하지 않았다. 풀 정책이 정하게 되면 상수가 아니라 정책에서 와야 한다"
  - "★ 3자 domain 대조는 **낡음을 잡는 장치이지 위조를 막는 장치가 아니다.** 관련된 자리를 동시에 같은 방향으로 고치면 통과한다 — `Signable` 구현 메시지는 네 자리, membership 계열은 세 자리다(Rust 쪽 메시지→domain 대응이 없어 한 자리가 빠진다). 올리는 것은 **우회 비용**이다"
  - "★ **이 조각에 Linux 회귀가 없다.** 순수 프로토콜/직렬화라 플랫폼 의존이 없지만 확인한 것은 아니다"
  - "이 메시지를 **소비하는 production 경로가 없다** — `ADR-033` §7 의 판정(Broker 가 신고를 모아 상태를 정하는 것)도, §8 관문에 신고를 넣는 어댑터도 아직 없다"
  - "★ `raw_output` 절은 **필터링·수동 결합한 실제 출력 발췌**다. 원본은 `raw_output_artifact` 파일이다"
---

# DoD-63 · 이웃 신고 wire 메시지 (`ADR-033` §7 관측 층)

## 미뤄져 있던 이유와, 그 이유를 어떻게 나눴는가

`proto/lease.proto` 의 주석이 이렇게 적어 두고 있었다.

> 이웃 신고(`ADR-033` §7 의 관측 층)는 여기 없다. 그건 같은 풀의 다른
> 노드를 관측하는 것이라 **pool membership 이 먼저 있어야 한다.**

정확히는 **메시지 정의와 멤버십 해소가 다른 일**이다.

```text
지금 할 수 있는 것    서명된 관측을 주고받는 형식을 정하는 것
지금 못 하는 것       "이 신고자가 정당한 이웃인가" 를 판정하는 것
```

앞의 것만 넣고, 뒤의 것은 **한계로 적고 테스트로 고정**했다.

## 판정을 담는 필드가 하나도 없다

`ADR-033` §7 의 핵심 문장이다.

> "연락이 안 된다" 는 "죽었다" 가 아니다 ... 신고는 관측 사실로만
> 기록하고 **판정 결과로 승격하지 않는다.**

그래서 `is_dead` 같은 필드를 넣지 않았다. 초안에는
`last_contact_at_unix_ms`·`failed_attempt_count` 도 있었는데 **뺐다** —
§7·§8 이 요구하지 않고 소비자도 없는데, `last_contact_at > observed_at`
같은 모순된 값도 서명만 맞으면 통과해 공격 입력면만 넓혔다.

## 강제하지 못하는 것은 **테스트로 열어 둔 채 고정한다**

`runtime-linux` 에서 "탈출이 성공하기를 기대하는 테스트" 를 남긴 것과
같은 장치다.

```text
one_device_key_can_sign_many_reporter_node_ids_today
    한 장치 키가 서로 다른 기계 ID 를 서명하는 것이 **통과한다**

the_framing_layer_does_not_check_the_target_coordinator_today
    다른 Coordinator 앞으로 온 신고가 **통과한다**
```

둘 다 지금은 사실이고, 나중에 닫히면 이 테스트들이 실패하며 문서도 같이
고치라고 알린다.

## ★ 내 서술이 반복해서 틀렸다 — 12라운드 중 절반이 그것이었다

가장 무거운 것은 **재생 방어의 이유를 반대로 적은 것**이다.

```text
내가 쓴 것   재생하면 이웃 하나가 §8 조건 3 의 정족수를 혼자 채운다
사실         reassignment.rs 가 reporter_node_id 로 중복 제거하므로
             같은 신고를 N 번 넣어도 한 표다.
             재생이 막는 것은 신선도 위조와 중복 부작용이다
```

세 곳에 그렇게 적었고 두 라운드에 걸쳐 고쳤다. 그 밖에도 —

```text
"어느 계층도 ULID 를 강제 안 한다"   fenced_operation.rs 가 강제한다
"§7.3 (b) 가 근거다"                 새 타입은 v1 시작이 근거다
"unknown signer 테스트"              실제로는 InvalidSignature 였다
"domain 분리를 잰다"                 필드 배치가 달라 어차피 실패한다
"nonce 도 달리했다"                  앞 16바이트가 같아 안 달랐다
"세 목록이 정확히 같다"              집합만 비교해 맞바꾸기를 못 잡았다
"네 자리를 고쳐야 우회된다"          경우에 따라 세 자리다
"30종 전부의 이름 대응"              None 4종을 빼면 26종이다
```

## 부수 소득 — 같은 결함의 남은 자리를 닫았다

이 저장소가 "손으로 쓴 domain 목록이 낡는" 결함을 **다섯 번** 겪었다고
기록해 뒀는데, 이번에 **다섯 번째와 여섯 번째 자리**가 드러났다.

```text
5번째  canonical_vectors.rs 의 수동 배열이 30종 중 28종만 검사하고 있었다
       (2026-08-19 에 한 번 고쳤는데 배열로 고쳐서 또 낡았다)
6번째  reference_canonical.py 와 signing.md 는 대조 장치가 아예 없었다
```

배열을 고치면 일곱 번째가 온다. 그래서 `Domain::ALL` 순회로 바꾸고,
규범 문서·Python·코드의 **메시지 → tag 대응**을 실제로 파싱해 대조하는
테스트를 만들었다. 우회 9가지를 재현해 전부 잡히는 것을 확인했다.

## 이 실험이 증명하지 않는 것

```text
신고자의 정당성        멤버십 해소가 없다. 이 메시지는 그 자리를 잡아 둘 뿐이다
device → node 결합     장치 하나가 여러 기계 ID 를 주장할 수 있다
수신 Coordinator 대조   프레이밍 계층은 안 한다. 소비자 몫이다
정족수 조작 방어        재생 방어는 신선도를 지킬 뿐이다
TTL 값의 규범성         60초는 ADR 이 정한 값이 아니다
목록 위조              3자 대조는 낡음을 잡지 위조를 막지 않는다
Linux 동작             이번 조각은 Windows 에서만 돌렸다
실제 판정·재배정        이 신고를 소비하는 production 경로가 없다
```
