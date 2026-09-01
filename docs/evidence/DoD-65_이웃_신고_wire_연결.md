---
schema_version: 2
id: DoD-65
claim: "`DoD-64` 가 만든 `CoordinatorNeighborReportStore` 를 **실제 wire 경로에 연결**했다 — Agent 가 서명한 `NeighborUnreachableReport` 를 보내고, Coordinator 가 받아 검증한 뒤 영속 저장소에 남긴다. `DoD-64` 자신이 '`wire` 수신부에서 이 저장소를 호출하는 경로를 만들지 않았다' 고 적어 둔 한계를 닫는다. ★ 이 조각의 무게중심은 송수신이 아니라 **거부**에 있다 — 이웃 신고를 **실제로 다루지 않는 실행 경로**가 그 옵션을 받아들이면 운영자는 신고가 모이는 줄 아는데 한 건도 안 모인다. 그래서 구현하지 않은 조합(multi-agent lane · resume 경로 · ACK 직후 끝날 수 있는 test hook 3종)은 **시작 전에 거부**한다. 관문은 `NeighborReportLane` 을 **인자로 받아** 자기가 어느 lane 인지를 설정에 묻지 않는다 — 진입점이 스스로 말한다. ★ 순서를 문구가 아니라 **값**으로 드러낸다: 점유된 포트를 줘서 관문이 bind 보다 먼저인지 재고, 테스트가 자기 listener 를 소유해 `accept()` 로 **연결 시도 자체가 없었음**을 관측한다. ★ **production 소비자는 만들지 않았다** — 모은 관측을 재배정에 쓰는 경로는 `ADR-033` §8 조건 2 의 강제 수단이 없는 한 켜면 안 된다(`DoD-62`)"
status: PASS
commit: PENDING

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — wire 연결, selftest 시나리오 5건, crate 테스트 10건, 뮤테이션 18건, 독립 검수 14라운드"
executor_model: "claude-opus-5"
executed_at: "2026-09-01T16:40:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 14라운드(11라운드는 세 갈래 동시)"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: >
  14라운드. ★ **11라운드는 세 갈래를 동시에 돌렸다**(종합 · 서술 감사 ·
  우회 조사) — 사용자가 "코덱스랑 병렬 작업 되는 건 다 걸고 돌려라" 고
  지시해서다. 같은 코드를 같은 시각에 셋이 봤는데 **넓게 본 종합은
  `ACCEPTED` 였고, 좁게 판 둘이 이 조각에서 가장 무거운 결함을 찾았다.**
  넓게 물으면 "관문이 있나" 에 답하고, 좁게 물으면 "그 관문이 옳은가" 에
  답한다. 순차로 돌렸으면 종합의 `ACCEPTED` 에서 멈췄을 것이다.

  ★ 아래 라운드별 열거가 정본이다 — 총계를 손으로 세지 않는다.

  1R: `Corrupt` 를 세션 거부로 분류(손상은 한 기계의 것이라는 내 근거가
  **사실이 아니었다** — 저장소는 상한/축출에서 다른 기계 행도 읽는다),
  `AddressedToAnotherCoordinator` 도 같은 원인, `:memory:` 는 막고 `None` 은
  허용.
  2R: **관문이 bind·Grant·ACK 뒤에 있어** "아예 시작하지 않는다" 가 거짓.
  3R: **multi-agent lane 이 두 관문을 통째로 우회**하고 플래그를 조용히 무시.
  4R: 관문을 옮긴 뒤 시나리오가 `READY` 부재만 봐서 순서를 증명 못 함.
  5R: 점유 포트 기법 도입 뒤에도 `:memory:` 경우에만 적용.
  6R: **`resume_protocol` lane 도 우회**, **공개 함수 직접 호출도 우회**
  (관문이 CLI 진입점에만 있었다), 시나리오 97 이 `READY` 부재로 "listener 를
  열었다" 를 주장.
  7R: 프로덕션 결함 0. **네 진입점에 관문을 뒀는데 셋이 한 번도 실행되지
  않았다** — 모든 시나리오가 CLI 를 거쳐 CLI 관문이 먼저 걸렸고, 지운 채로도
  전부 통과했다. coordinator resume 조합의 순서 미측정.
  8R: 프로덕션 결함 0. 순서 판별이 이름과 달리 multi-agent 만 쟀다,
  **`run()` 으로 옮긴 lane 분기를 아무 테스트도 고정하지 못함**(지우면 순차
  lane 이 accept 타임아웃으로 실패하며 통과), 서술 3건.
  9R: 프로덕션 결함 0 · 검증 장치 0. 서술 3건 — ★ 그중 하나는 **8라운드에서
  "고친" 문장이 엉뚱한 함수 위에 있었던 것**이다(파싱을 떼어내며 옛 문서
  블록이 밀려났는데 못 보고 그 자리를 고쳤다).
  10R: **실제 결함 1건(테스트)** — `127.0.0.1:1` 이 거부된다는 걸 **가정**했다
  (점유하지도, 닫혔음을 확인하지도 않았다). 서술 4건 — 그중 하나는 가드
  함수를 끼워 넣으며 **`run()` 의 rustdoc 를 가로챈 것**(`DoD-64` 에서
  `#[allow(dead_code)]` 를 가로챈 것과 같은 종류, 두 번째다).
  11R-종합: **`ACCEPTED`** — 실제 결함 0 · 서술 0.
  11R-우회조사: ★★ **관문이 자기가 어느 lane 인지를 설정에 물어봤다.**
  `run_multi_agent()` 는 그 함수 자체가 multi-agent lane 인데 관문은
  `config.multi_agent` 를 읽어 판정했다 — 플래그를 끄고 부르면 "순차 lane
  이구나" 하고 통과했다. 그리고 **순차 lane 안에도 신고 수신에 닿기 전에
  끝날 수 있는 모드가 셋 더** 있었는데 관문이 세지 않았다.
  11R-서술감사: 10건. 무거운 것은 **lane 충돌을 `kind=storage` 로 찍고
  테스트가 그 잘못된 분류를 고정한 것**, `:memory:` 가 "관측을 모아 둘 수
  없다"(사실이 아니다 — 잃는 것은 재시작 내구성이다), 개수 낡음, `Shared`
  공유 목록 누락, "타입으로 확인"(실행 중 검사다), 동시성 관문 오류가
  **원인을 단정**, 테스트 이름이 실제 단언보다 강함, `accept()` 의 다른
  오류를 "연결됨" 으로 셈.
  12R: 프로덕션 결함 0. 서술 3건 — ★ **셋 다 11라운드 정정이 새로 만든 것**
  이다("동시에 처리한다" 는 보장이 아니다, 세 플래그가 "항상" 끝내지는
  않는다, 루프의 경우 수 오산).
  13R: 프로덕션 결함 0. 서술 2건 — 둘 다 같은 병이다. **12라운드 정정을
  아래에 덧붙이고 위의 낡은 문장을 그대로 둬** 한 주석 안에서 앞뒤가
  모순됐고, 테스트 이름이 기본 설정에서 동작하지 않는 플래그까지
  "ACK-only mode" 라 불렀다.
  14R: **지적 없음 — `ACCEPTED`.** 실제 결함 0 · 서술 0.
review_artifact: "docs/evidence/_raw/DoD-65_review_all_rounds_verbatim.txt"

decision: >
  **저장하는 것과 판정하는 것은 다르다.** `ADR-033` §7 이 층을 둘로 나눴고,
  이 조각은 관측이 **도착해서 남는** 데까지만 한다. 정족수도 생존 판정도
  하지 않는다 — 그건 `DoD-61` 의 관문이 하고, 그 관문은 §8 조건 2 의 강제
  수단이 없어 오늘 아무것도 허용하지 않는다.

  **구현하지 않은 조합은 거부한다.** 이 조각에서 가장 많은 시간을 쓴 곳이다.
  이웃 신고 수신·송신 루프가 **없는** 실행 경로가 그 옵션을 받아들이면,
  아무 일도 안 일어나는데 아무 오류도 안 난다 — 운영자는 신고가 모이는 줄
  안다. 조용한 무시는 실패보다 나쁘다.

  ★ **관문은 자기가 어디 있는지를 남에게 묻지 않는다.** 처음에는
  `config.multi_agent` 를 읽어 판정했는데, `run_multi_agent()` 는 **그 함수
  자체가 그 lane** 이다. 설정이 거짓말하면 관문이 따라 속는다. 이제
  `NeighborReportLane` 을 인자로 받아 **진입점이 스스로 말한다** — 아직
  분기하지 않은 자리(`run()`/`run_from_args()`)만 설정으로 고른다.

  **거부 대상은 lane 만이 아니다.** 순차 lane 안에도 ACK 직후 세션을 끝낼
  수 있는 test hook 이 셋 있다. 그중 하나는 특정 조건에서만 실제로 끊지만,
  **닿을 수도 있고 아닐 수도 있는 구성**을 받아 주는 것이 조용한 무시보다
  낫지 않다 — 조건 없이 거부한다.

  **원인이 다르면 이름도 달라야 한다.** lane 충돌을 `kind=storage` 로 찍고
  있었다 — 저장소 연산을 하나도 안 하는데. 그리고 테스트가 그 잘못된 분류를
  고정하고 있었다. `STARTUP_REFUSED` 로 나눴다(`CLAUDE.md` §3).

  **순서는 문구가 아니라 값으로 드러낸다.** `READY` 가 안 나왔다는 것만으로는
  "bind 전에 막았다" 를 증명하지 못한다 — bind 뒤 READY 전에 죽어도 같아
  보인다. 세 기법을 썼다:
    · 이미 점유된 포트를 준다 — 관문이 먼저면 lane 오류, 나중이면 bind 실패
    · 테스트가 listener 를 소유한다 — `accept()` 가 `WouldBlock` 이면 연결
      시도 자체가 없었다
    · 대조 테스트는 반대로 **연결이 실제로 일어났음**을 요구한다

  ★ **production 소비자는 만들지 않았다.** 모은 관측을 재배정에 쓰는 경로는
  `ADR-033` §8 조건 2 의 강제 수단이 생기기 전에는 켜면 안 된다(`DoD-62`).
raw_output_artifact: "docs/evidence/_raw/DoD-65_neighbor_report_wire_2026-09-01.txt"
raw_output_digest: "sha256:449fae6cbb46ebcdc08effd351b1944edb945ccc53e46d991999ee69ab572641"
raw_output_bytes: 7099

artifacts:
  - "docs/evidence/_raw/DoD-65_neighbor_report_wire_2026-09-01.txt"
  - "docs/evidence/_raw/DoD-65_review_all_rounds_verbatim.txt"

binary_digests:
  toolchain: "Windows 개발 기계 cargo 1.97.1 / x600 WSL2 cargo 1.89.0"
protocol_versions:
  schema_version: "proto 변경 없음 — `DoD-63` 이 만든 `NeighborUnreachableReport`(schema v1)를 보내고 받는다"
  canonical_spec: "canonical 벡터 변경 없음(52건 그대로)"
platform: >
  ★ **측정 범위를 나눠 적는다.** selftest 시나리오 93~97 은 **Windows 에서만**
  측정됐다 — x600 WSL2 에서는 selftest 전체가 시나리오 80(cgroup memory 위임
  부재)에서 멈춘다(`DoD-57` 이 기록한 환경 조건, 이번 변경과 무관).
  반면 신규 crate 테스트 10건은 **양쪽 플랫폼에서** 측정됐다.
hardware: "GPU 무관"
network_profile: "127.0.0.1 TCP · 별도 OS 프로세스 2개"
command: |
  cargo build -p gputeer-cli
  ./target/debug/gputeer.exe coordinator-agent-selftest      # 3회 연속
  cargo test --workspace
  cargo test -p gputeer-coordinator -p gputeer-agent --test neighbor_report_lane_guard
  # x600 WSL2 (/mnt/e/gputeer-work/build/gputeer)
  cargo test -p gputeer-coordinator -p gputeer-agent
  # 뮤테이션 18건 (raw 4절)

raw_output: |
  === Windows coordinator-agent-selftest (3회 연속) ===
  run1 exit=0 scenarios=97
  run2 exit=0 scenarios=97
  run3 exit=0 scenarios=97

  === Windows cargo test --workspace ===
  passed=826  failed=0  errors=0

  === x600 WSL2 Linux (coordinator + agent) ===
  242 passed / 0 failed
    neighbor_report_lane_guard (coordinator)  6 passed
    neighbor_report_lane_guard (agent)        4 passed

  === 뮤테이션 18건 — 전부 지정 테스트를 동작 수준에서 실패시켰다 ===
  W1~W10  wire 경로        -> selftest 시나리오 93~97
  G1~G4   관문 호출 자리   -> crate 테스트
  R1      run() 의 lane 분기 삭제
  R2      resume 관문 무력화
  S1      관문을 "항상 거부" 로
  S2      관문 제거

  (자세한 표는 raw artifact 4절)

negative_tests:
  - "94) 다른 Coordinator 앞으로 서명된 신고는 세션 거부(kind=protocol)되고 DB 가 불변이다"
  - "95) 저장소 구성이 잘못되면(비영속 · 경로 부재) listener 를 열기 전에 멈춘다 — 점유 포트로 순서를 잰다"
  - "95) multi-agent · resume lane 조합도 같은 방식으로 bind 전에 거부한다"
  - "96) 신고 대상이 없거나 공백이면 Agent 가 연결조차 하지 않는다 — 테스트 소유 listener 의 accept() 로 확인"
  - "96) multi-agent · resume 조합도 연결 전에 거부한다"
  - "97) 구현하지 않은 lane 조합을 조용히 무시하지 않고 STARTUP_REFUSED 로 거부한다"
  - "coordinator run() 을 직접 불러도 multi-agent · resume 조합은 거부된다"
  - "coordinator::run_multi_agent() 는 multi_agent 플래그가 꺼져 있어도 신고 옵션을 거부한다 — 11라운드 우회 반례"
  - "agent::run_multi_agent_session() 도 같다(플래그 꺼짐 · 대상 없음 포함)"
  - "ACK 직후 세션을 끝낼 수 있는 세 플래그도 신고 옵션과 함께 주면 거부된다"
  - "관문이 과하지 않은지 대조 — 신고를 안 쓰는 lane 은 거부되지 않고 연결이 실제로 일어난다"
  - "뮤테이션 18건(W1~W10 · G1~G4 · R1 · R2 · S1 · S2)이 각각 지정 테스트를 동작 수준에서 실패시켰다"

limitations: >
  - **production 소비자가 없다.** 모은 관측을 읽어 재배정을 결정하는 경로는
    만들지 않았다 — `ADR-033` §8 조건 2 의 강제 수단이 없다(`DoD-62`).
  - **시나리오 93~97 은 Windows 에서만 측정됐다**(위 `platform` 참조).
  - **장치→기계 결합은 여전히 강제되지 않는다** — 한 장치 키가 여러 기계
    ID 를 주장할 수 있다는 `DoD-63`·`DoD-64` 의 한계 그대로다. 저장소가 각
    행에 서명 장치를 실어 보내 호출부가 값으로 보게 할 뿐이다.
  - **관문은 조합을 거부할 뿐 lane 을 구현하지 않는다.** multi-agent lane 과
    resume 경로에 이웃 신고를 붙이는 일은 별도 조각이다.
  - `--drop-connection-after-ack-once` 는 `max_connections > 1` 이고 첫
    연결일 때만 실제로 세션을 끊는다. 관문은 그래도 조건 없이 거부한다 —
    닿는다고 **보장할 수 없는** 구성을 받아 주지 않는다.
---

# DoD-65 — 이웃 신고 wire 연결

`DoD-64` 가 만든 저장소를 실제 경로에 연결한다. 자세한 내용은 위
frontmatter 의 `claim` · `decision` · `limitations` 를 본다.

## 이 조각에서 배운 것

**정정이 새 오류를 만든다.** 9·10·12·13라운드가 **네 번 연속** 그것을
잡았고 매번 형태가 달랐다 —
문서를 고치다 엉뚱한 함수 위를 고쳤고, 가드 함수를 끼워 넣다 `run()` 의
rustdoc 를 가로챘고, "동시에 처리한다" 는 과장을 고치면서 다른 과장을
새로 썼고, 정정을 아래에 덧붙이면서 위의 낡은 문장을 남겨 한 주석 안에서
앞뒤가 모순됐다. `DoD-64` 에서 모듈 한 줄을 끼워 넣다 `#[allow(dead_code)]`
를 가로챈 것과 같은 종류가 **두 번째로** 나왔다 — **삽입 위치가 위쪽 속성·
문서의 소속을 바꾼다.**

**넓게 묻는 검수와 좁게 묻는 검수는 다른 것을 찾는다.** 11라운드에서 셋을
동시에 돌렸더니 종합은 `ACCEPTED`, 좁게 판 둘은 `CHANGES_REQUESTED` 였다.
넓게 물으면 "관문이 있나" 에 답하고, 좁게 물으면 "그 관문이 옳은가" 에
답한다.

**서술 지적은 열네 라운드 내내 한 방향이었다** — 코드보다 강하게 주장하거나,
다른 대상의 설명을 옮겨 붙였거나, 코드가 바뀐 뒤 낡았거나. 실제보다 약하게
쓴 것은 한 건도 없다. `DoD-61`·`DoD-63`·`DoD-64` 에 이어 **네 번째**로 같은
패턴이다.
