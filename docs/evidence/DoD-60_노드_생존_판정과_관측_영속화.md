---
schema_version: 2
id: DoD-60
claim: "`crates/scheduler/src/liveness.rs` 의 `classify_node_liveness()` 가 노드 자기보고(heartbeat)의 신선도를 순수하게 분류하고, `crates/coordinator/src/node_liveness_store.rs` 의 `CoordinatorNodeLivenessStore` 가 검증된 heartbeat 를 재시작 너머로 남기며, 그 저장이 실제 wire 경로(시나리오 92)를 거쳐 도달함을 확인했다. 커널에는 `Dead` 판정이 **없다** — `ADR-033` §7 이 '연락이 안 된다 != 죽었다' 를 못박았고, 가장 나쁜 판정인 `Silent` 는 사실 진술이지 재배정 결정이 아니다. 저장소는 노드당 한 행만 유지하고(무한 이력을 만들지 않는다), 늦게 도착한 옛 관측이 최신을 밀어내지 않으며, 승인 없는 장치 교체를 거부하되 운영자 승인 경로(`rebind_device`/`cancel_rebind`)를 제공한다"
status: PASS
commit: 33182ed057244cb69b9a81b235cd9535ef8aa64e

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — liveness 순수 커널 신설, node_liveness_store 신설, coordinator heartbeat 수신부 연결, selftest 시나리오 92"
executor_model: "claude-opus-5"
executed_at: "2026-08-30T18:30:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 3라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "파일:줄로 지목된 지점 — `crates/scheduler/src/liveness.rs:201`·`:188`(오류 결과가 입력 순서에 의존 — `ConflictingDevice` 의 first/second 가 순서로 갈리고 빈 식별자도 먼저 만난 쪽이 보고됨), `:238`(`Dead` 부재가 강제 장치가 아님 — 소비자가 `Silent => 재배정` 으로 매핑하는 것을 타입이 막지 못함), `:345`(순서 테스트가 정상 관측만 뒤집고 오류 순열을 안 봄), `crates/coordinator/src/node_liveness_store.rs:160`(`:memory:` 와 빈 경로를 durable 로 수용), `:219`~`:241`(`rebind_device` 가 `new_device_id` 를 빈 값 검사에만 쓰고 행만 삭제 — 그 뒤 먼저 보고한 아무 등록 장치가 노드를 인수), `:260`(`DeviceChanged` 가 정상 신원 교체를 영구 차단), `:267`·`:239`(첫 rebind 후 재지정 불가 — 영구 교착), `:518`(`cancel_rebind` 가 바인딩 전체를 초기화), `crates/coordinator/src/lib.rs:781`·`:424`(신원 충돌을 `Storage` 로 포장해 accept loop 전체 종료), `crates/cli/src/coordinator_agent_selftest.rs:5077`(`advanced=true` 를 한 번이라도 있으면 통과 — 첫 관측이 항상 만족하므로 공허), `:5084`(파일 존재만 보고 재개방·복원은 안 함). 검수는 `BEGIN IMMEDIATE` 안에서 읽기·비교·쓰기를 하므로 별도 SQLite 연결 간 check-then-update 경쟁이 직렬화됨을 코드로 확인했다"
review_artifact: "docs/evidence/_raw/DoD-60_review_round1_verbatim.txt"

decision: "생존 판정을 **순수 커널**로 떼어낸다 — 시계·I/O·전역 상태 없이 `now_unix_ms` 를 인자로 받고, 처리 전에 입력을 정렬해 **성공과 오류 모두** 입력 순서와 무관하게 같은 답을 낸다. `Verified<NodeHeartbeat>` 를 직접 받지 않는다 — 그러면 `scheduler` 가 protocol/crypto 에 묶여 순수하지 않게 된다. 판정 구간을 **하나가 아니라 둘**로 나눈다(`Live`/`Suspect`/`Silent`) — 경계가 하나면 시계가 몇 밀리초만 흔들려도 판정이 오간다. 미래의 관측(`issued_at > now`)을 **버리지 않고** `Live` 로 본다 — 버리면 시계가 빠른 노드가 영영 `Silent` 로 보인다. 저장소는 이력이 아니라 **노드당 최신 한 행**을 유지한다 — 판정에 쓰이는 것은 마지막 소식 하나이고, 보존 정책 없이 무한 이력을 만드는 것은 남의 PC 를 채우는 일이다. 신원 충돌은 조용히 덮지 않되(등록된 B 가 A 의 노드를 인수할 수 있다) **되돌리는 길**을 만든다 — 거부만 하고 풀 방법이 없으면 운영자가 DB 를 손으로 지우게 되고 그게 훨씬 위험하다."
raw_output_artifact: "docs/evidence/_raw/DoD-60_node_liveness_2026-08-30.txt"
raw_output_digest: "sha256:e5901bf5c287d70ec3f5dcceeb2818ae8cc2c88d866eeed7b63af33aae8ec0fe"
raw_output_bytes: 5595

binary_digests:
  toolchain: "Windows 개발 기계 cargo 1.97.1 / x600 WSL2 cargo 1.89.0"
protocol_versions:
  schema_version: "proto 변경 없음 — 기존 `NodeHeartbeat`(schema_version 1)를 소비한다"
  canonical_spec: "canonical 벡터 50건 불변 — 새 서명 대상 메시지 없음"
platform: "Windows 11 개발 기계(단위 테스트 + 별도 프로세스 selftest) + x600 WSL2 회귀"
hardware: "GPU 무관"
network_profile: "시나리오 92 는 127.0.0.1 루프백 TCP. 커널 자체는 네트워크를 쓰지 않는다"
command: |
  cargo test -p gputeer-scheduler --lib
  cargo test -p gputeer-coordinator --test node_liveness_store
  cargo run -q --bin gputeer -- coordinator-agent-selftest   # 시나리오 92
  # 뮤테이션 6건 (raw 참조)
raw_output: |
  (docs/evidence/_raw/DoD-60_node_liveness_2026-08-30.txt 전문 참조)

  scheduler --lib:      test result: ok. 10 passed; 0 failed
  node_liveness_store:  test result: ok. 16 passed; 0 failed
  시나리오 92: heartbeat 관측이 실제 wire 를 거쳐 durable 저장소까지 도달

  뮤테이션 6건 전부 정확히 해당 테스트만 실패(원복 후 재검증 통과)

artifacts:
  - crates/scheduler/src/liveness.rs
  - crates/coordinator/src/node_liveness_store.rs
  - crates/coordinator/src/lib.rs
  - crates/coordinator/tests/node_liveness_store.rs
  - crates/cli/src/coordinator_agent_selftest.rs
  - docs/evidence/_raw/DoD-60_node_liveness_2026-08-30.txt
  - docs/evidence/_raw/DoD-60_review_round1_verbatim.txt
  - docs/evidence/_raw/DoD-60_review_round2_verbatim.txt
negative_tests:
  - "`the_worst_verdict_is_silence_not_death`: 아무리 오래 침묵해도 `Silent` 를 넘는 판정이 없음을 고정한다 — `ADR-033` §7 이 '연락 안 됨 != 죽었다' 를 못박았고, 그 상태에서 다른 GPU 에 다시 띄우면 두 번 돈다"
  - "`a_known_node_with_no_observation_still_appears`: 관측이 없는 알려진 노드가 결과에서 사라지지 않음을 확인한다 — 빠지면 '안 보이는 노드' 를 아무도 못 보는데 그게 가장 알아야 할 노드다. 뮤테이션(알려진 노드 병합 제거)으로 정확히 이 테스트만 실패"
  - "`a_clock_slightly_ahead_does_not_look_silent`: 미래 관측의 침묵 시간이 음수로 감싸돌지 않음을 확인한다. 뮤테이션(`saturating_sub` -> `wrapping_sub`)으로 정확히 이 테스트만 실패"
  - "`a_tie_on_time_is_broken_deterministically`: 같은 시각의 두 관측을 `fence_epoch` 으로 결정적으로 가른다. 뮤테이션(동점 정렬에서 `fence_epoch` 제거)으로 정확히 이 테스트만 실패"
  - "`errors_do_not_depend_on_input_order_either`: **오류 결과도** 입력 순서에 무관함을 확인한다 — 성공 경로만 순서 독립이면 반쪽이고, 같은 입력 집합에 다른 오류가 나오면 진단할 때 재현이 안 된다"
  - "`a_late_older_heartbeat_does_not_move_the_clock_backwards` / `an_identical_timestamp_does_not_advance`: 늦게 도착한 옛 관측이 최신을 밀어내지 않음을 확인한다. 뮤테이션(뒤로가기 방지 제거)으로 정확히 이 두 건이 실패"
  - "`a_same_millisecond_tie_uses_the_same_rule_as_the_kernel`: 같은 밀리초의 두 관측을 저장소가 커널과 **같은 규칙**으로 가른다 — 다르면 잠금 획득 순서가 결과를 바꾼다"
  - "`a_corrupted_row_fails_closed` / `a_tampered_body_is_caught_by_the_hash`: 저장된 시각을 손으로 바꾸거나 body 를 훼손하면 읽기가 거부한다. 뮤테이션(읽기 시 해시 대조 제거)으로 정확히 실패"
  - "`without_an_approval_a_device_change_is_still_refused` / `an_approved_rebind_only_admits_the_named_device`: 승인 없는 장치 교체는 거부하고, 승인 후에도 **지정한 장치만** 들어오며 대기가 한 번 쓰고 소진됨을 확인한다 — 승인 순간 제3 장치가 먼저 보고해 인수하는 것을 막는다"
  - "`a_mistaken_rebind_can_be_retargeted` / `a_pending_rebind_can_be_cancelled`: 잘못 지정한 rebind 를 재지정·취소할 수 있음을 확인한다 — 없으면 운영자가 DB 를 손으로 지우게 된다"
  - "`a_heartbeat_without_a_timestamp_is_refused`: `issued_at_unix_ms == 0` 을 거부한다 — 저장하면 그 노드는 영원히 '아주 오래 전에 봤다' 가 된다"
  - "시나리오 92: 서명·replay·3종 대조를 전부 통과한 heartbeat 가 저장소까지 도달하고, **두 회차 모두** 진행하며 시각이 서로 다름을 확인한다. 뮤테이션(저장 호출 제거)으로 '저장된 관측이 2건이 아니다(0건)' 로 실패"
limitations:
  - "★ **이것은 `ADR-033` §7 의 구현이 아니라 선행 조건이다.** §7 의 입력은 이웃이 서명한 '연락 실패' 신고인데, 이 커널은 노드 **자기보고**를 받는다. 자기보고는 도착하면 '살아 있다' 의 강한 증거지만 **안 오는 것은 약한 증거다** — 네트워크 분단인지 죽음인지 구분하지 못한다. 이웃 신고 메시지는 아직 없다"
  - "★ **`Dead` 부재는 강제 장치가 아니다.** 소비자가 `Silent => 재배정` 으로 매핑하는 것을 타입이 막지 못한다. 지금 안전한 이유는 **재배정 소비자가 아직 하나도 없어서**다. 재배정을 만들 때는 `ADR-033` §8 의 여섯 조건을 타입으로 요구하는 별도 관문이 필요하다"
  - "재배정을 하지 않는다 — 실패를 감지해 다른 GPU 에서 이어가는 것(로드맵 14번의 본체)은 이 조각 밖이다"
  - "`NodeRecord` 자체를 갱신하지 않는다 — 별도 `coordinator_node_liveness` 테이블에만 기록한다. proto 의 `NodeRecord.last_heartbeat_unix_ms` 를 채우는 소비자는 아직 없다"
  - "시나리오 92 는 **wire 경로**를 본다. 재시작을 넘는 복원은 저장소 단위 테스트(`an_observation_survives_a_restart`)의 일이고, 시나리오는 DB 를 재개방하지 않는다"
  - "`rebind_device`/`cancel_rebind` 는 **사람이 부르는 API 이고 production 호출부가 없다.** 권한 경계가 타입이나 토큰으로 강제되지 않는다 — 검수가 이 점을 짚었고, `cancel_rebind` 는 이름이 '취소' 지만 실제로는 바인딩 전체 초기화라 취소 후 아무 등록 장치나 그 `node_id` 를 주장할 수 있다"
  - "heartbeat 는 이 stub 프로토콜에서 ACK 뒤·갱신 앞의 **고정 위치**로만 온다 — 실제 운영의 주기적 비동기 전송을 표현하려면 프레임 다중화가 먼저 필요하다"
  - "다중 Agent lane 과 결합해 검증하지 않았다 — heartbeat 는 순차 lane 에서만 돈다"
---

# DoD-60 — 노드 생존 판정과 관측 영속화

## 규범이 먼저 답을 정해 뒀다

`ADR-033` §7 이 이 층을 두 개로 나눴다.

```text
관측(신고)   이웃이 "저 노드에 연락이 안 된다" 고 서명해 보고한다
판정(결정)   Broker 가 그 보고를 모아 노드 상태를 정한다
```

그리고 못박았다 — **"연락이 안 된다" 는 "죽었다" 가 아니다.**
네트워크가 갈라졌으면 대상 노드는 멀쩡히 계속 실행 중이다. 그 상태에서
다른 GPU 에 같은 작업을 다시 띄우면 **두 번 돈다.** `side_effecting`
작업이면 되돌릴 수 없다.

그래서 이 커널에는 `Dead` 가 **없다.**

## ★ 그러나 `Dead` 부재는 강제 장치가 아니다

검수가 정정했다. 소비자가 `Silent => 재배정` 으로 매핑하는 것을 타입이
막지 못한다. 지금 안전한 이유는 "`Dead` 가 없어서" 가 아니라 **재배정
소비자가 아직 하나도 없어서**다.

재배정을 만들 때 `ADR-033` §8 의 여섯 조건을 타입으로 요구하는 관문이
필요하다 — 그건 이 조각 밖이고, 여기 적는 이유는 그때 이 문장을 읽으라는
것이다.

## 오류도 결과다

★ 검수가 성공 경로만 순서 독립인 것을 지적했다.

```text
[A, B] -> ConflictingDevice{first: A, second: B}
[B, A] -> ConflictingDevice{first: B, second: A}
```

같은 입력 집합에 다른 답이 나오면 순수 커널이 아니고, 진단할 때 재현이
안 된다. 처리 전에 정렬한다.

## 거부에는 되돌리는 길이 있어야 한다

같은 노드를 다른 장치가 주장하면 거부한다 — 조용히 덮으면 등록된 B 가
A 의 노드를 인수할 수 있다.

★ 그런데 초안은 거부만 하고 풀 방법이 없었다. 첫 `rebind` 가 행을 지운
뒤로는 **재지정도 취소도 불가능**했다 — 지정한 장치가 고장 나거나 ID 를
잘못 쳤으면 운영자가 DB 를 손으로 지우게 되고, 그게 훨씬 위험하다.

그리고 초안 `rebind_device()` 는 **새 장치에 묶지도 않았다.** 행만 지워서
A→B 교체를 승인한 순간 **C 가 먼저 보고하면 C 가 인수**했다. 승인이
있으나 마나였다. 대기 바인딩을 남겨 지정한 하나만 들어오게 고쳤다.

## 테스트의 전제가 현실과 달랐던 것

시나리오 92 의 `advanced=true` 검사를 조이자 두 heartbeat 가 **같은
밀리초**에 나가 두 번째가 진행하지 않았다. 저장소는 옳게 판정한 것이고,
실제 시스템은 초 단위로 보내니 테스트가 현실과 달랐던 것이다.
`--heartbeat-interval-ms` 를 넣어 고쳤다.

## 이 실험이 증명하지 않는 것

```text
§7 구현        이웃 신고 메시지가 없다 — 이건 선행 조건이다
재배정         §8 의 여섯 조건이 필요하다
NodeRecord     proto 필드를 채우는 소비자가 아직 없다
재시작 복원    시나리오 92 는 wire 경로만 본다(저장소 단위 테스트의 일)
권한 경계      rebind/cancel 은 사람이 부르는 API 이고 강제 수단이 없다
주기적 전송    고정 위치로만 온다 — 프레임 다중화가 선행이다
다중 Agent     결합 검증하지 않았다
```
