---
schema_version: 2
id: DoD-67
claim: "`DoD-50` 이 `submit_verified_manifest()` 를 만들면서 'production wire 연결은 범위 밖' 이라 적어 둔 저장소에 **첫 소비자**를 만든다 — `gputeer import-manifest` 가 서명된 `JobManifest` 를 운영자 keyring 으로 검증한 뒤에만 필드를 읽고 durable job store 에 넣는다. ★ 이건 제출 **접수**가 아니라 **반입(import)** 이다 — 명령줄로 공개키를 받으면 신뢰 경계가 그 명령 한 줄이 되므로, 운영자가 **미리 provision 한 keyring 파일**에 있는 서명자만 받는다. 신뢰 경계가 운영자가 소유하는 파일이다. ★ 이 조각은 네트워크 제출을 받지 않고, 누가 제출할 자격이 있는지 판정하지 않으며(멤버십 권위는 여전히 없다), 스케줄링도 하지 않는다"
status: PASS
commit: 5b07a9d

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — CLI 명령 1건(production 240줄), 통합 테스트 8건, 뮤테이션 7건, 독립 검수 4라운드"
executor_model: "claude-opus-5"
executed_at: "2026-09-02T21:00:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 4라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: >
  4라운드. 실제 결함은 1·2라운드에서 나왔고 3·4라운드는 서술만 봤다.

  1R: **실제 결함 4건.** 가장 무거운 것은 `--job-db ""` 가 조용히
  성공한 것 — SQLite 는 빈 경로도 임시 DB 로 열어 준다. 원인은 내가
  `":memory:"` **문자열만 손으로 비교**한 것인데, 저장소는 이미
  `is_durable()` 로 빈 경로까지 판정하고 있었다. **같은 판정을 두 곳에
  두니 한쪽만 낡았다.** 그 밖에 거부 사유를 하나로 뭉갠 것(만료된 정상
  서명을 "서명 검증 실패" 라고 불렀다), 거부 테스트가 "이 job_id 가
  없다" 만 봐서 스키마·idempotency 행·다른 Job 행을 남겨도 통과하는 것.

  2R: **실제 결함 1건 + 서술 2건.** `UnknownSigner` 를 "신뢰 목록에
  없는 서명자" 로 **단정**했는데, 폐기·회전 만료·격리도 같은 결과로
  합류한다 — 목록에 **있는데** 폐기된 서명자를 오진해 운영자가 그를
  다시 넣으러 간다(그게 틀린 대응이다). 서술 둘은 help 삽입이
  `coordinator-stub` 사용법을 두 동강 낸 것과 계획서의 negative test
  목록이 실제 7건과 다른 것.
  ★ 지적으로 쓰지 않은 관찰이 더 무거웠다 — "테스트들이 모두 DB 가
  처음부터 없음 상태라 기존 DB 바이트 비교 분기는 실제로 실행되지
  않는다". `db_snapshot()` 이 사실상 `None == None` 만 확인하고 있었다.

  3R: **실제 결함 0 · 서술 4건.** 그중 하나가 **여섯 번째** 속성/문서
  가로채기다 — 8번째 테스트를 만료 테스트 앞에 끼워 넣으며 그 `///`
  를 가져갔다. 내가 검수 프롬프트에 "특히 이걸 보라" 고 적어 놓고 같은
  턴에 저질렀다. 나머지 셋 중 하나(**help 가 `agent-stub` 의 두 device
  ID 를 필수처럼 쓰지만 파서는 안 읽는다**)는 **반박했다** — 실행해
  보면 `필수 인자 누락: --coordinator-device-id` 가 나온다.

  4R: **지적 없음 — `ACCEPTED`.** 3라운드 반박이 맞다고 확인했고
  (`crates/agent/src/lib.rs:2124-2125` 가 둘 다 `require()` 한다),
  폐기·회전 만료·격리가 실제로 `UnknownSigner` 로 합류하는 것도
  코드로 확인했다.
review_artifact: "docs/evidence/_raw/DoD-67_review_all_rounds_verbatim.txt"

decision: >
  **이미 아는 것에게 물어봐야지 다시 만들면 어긋난다.** 이 조각의 가장
  무거운 결함이 그것이었다 — 영속성 판정을 저장소가 이미 하고 있는데
  CLI 에서 문자열 비교로 다시 만들었고, 그래서 `--job-db ""` 가 조용히
  성공했다. `DoD-65` 의 "관문이 자기가 어느 lane 인지 설정에 묻지
  않는다" 와 같은 교훈의 두 번째 얼굴이다.

  ★ **이름을 정직하게 골랐다.** 독립 설계 조사가 먼저 경고했다 — "임의
  공개키까지 같은 명령에서 받아 'verified/accepted' 라고 부르는 것은
  정직하지 않다". 명령줄로 공개키를 받으면 아무나 자기 키를 붙여
  "검증됨" 을 만들 수 있고, 그건 검증이 아니라 서명 확인일 뿐이다.
  그래서 이 명령은 **운영자가 미리 provision 한 keyring 파일**만 믿고,
  이름도 "접수" 가 아니라 "반입" 이다.

  ★ **거부 사유를 나눈 이유.** 처음에는 전부 "서명 검증 실패" 였다.
  만료된 **정상 서명**을 그렇게 부르면 운영자가 키를 의심하러 간다
  (`CLAUDE.md` §3). 그런데 그 정정이 또 과했다 — `UnknownSigner` 를
  "목록에 없다" 로 좁혔는데 검증 계층은 미등록·폐기·회전 만료·격리를
  **같은 결과로 합친다.** 이 명령은 그 넷을 나눌 수 없으므로 나눌 수
  있는 척하지 않고 넷을 다 말한다.

  ★ **거부 테스트는 "거부했다" 만 보면 안 된다.** 거부하면서 흔적을
  남기는 구현도 통과한다. 그래서 파일 바이트 전체를 비교하는데,
  2라운드가 짚었듯 **거부 테스트가 전부 빈 DB 였으면 그 비교는 아무것도
  안 잰다.** 운영 중인 job store 는 비어 있지 않다.

  ★ **여섯 번째 속성/문서 가로채기.** Rust 에서 `#[...]` 와 `///` 는
  바로 아래 항목에 붙고, 사이에 무언가를 끼워 넣으면 소속이 조용히
  바뀌는데 **컴파일도 테스트도 통과한다.** 눈으로는 안 걸린다.

  ★ **검수가 틀릴 수도 있다.** 3라운드의 help 지적은 사실이 아니었고,
  받아들였으면 맞는 문서를 틀리게 고칠 뻔했다. 실행해서 반박했고
  4라운드가 확인했다(`CLAUDE.md` §4 — 반박당하면 실측으로 가린다).
raw_output_artifact: "docs/evidence/_raw/DoD-67_import_manifest_2026-09-02.txt"
raw_output_digest: "sha256:516d47cf3aeef1d8edd8de19a434e2f43b07798de8e04e31f61ea9a14f52e67a"
raw_output_bytes: 3622

artifacts:
  - "docs/evidence/_raw/DoD-67_import_manifest_2026-09-02.txt"
  - "docs/evidence/_raw/DoD-67_review_all_rounds_verbatim.txt"
  - "docs/plans/2026-09-01_2000_제출된_manifest_운영자_반입_v1.md"

binary_digests:
  toolchain: "Windows 개발 기계 cargo 1.97.1"
protocol_versions:
  schema_version: "proto 변경 없음"
  canonical_spec: "canonical 벡터 변경 없음(52건 그대로)"
platform: >
  Windows 개발 기계에서 측정했다. 신규 테스트 8건은 실제 `gputeer.exe`
  프로세스를 띄워 CLI 경계를 넘으므로 플랫폼 의존이 없고 Linux 에서도
  같은 crate 테스트로 돈다 — 다만 이 조각은 **Linux 에서 측정하지
  않았다**(`CLAUDE.md` §4 — 한 플랫폼 통과를 다른 플랫폼 통과로 세지
  않는다).
hardware: "GPU 없음 — 이 조각과 무관하다"
network_profile: "없음 — 파일과 SQLite 만 쓴다"
command: |
  cargo test -p gputeer-cli --test import_manifest
  cargo test --workspace
  # 뮤테이션 7건 (raw 5절)

raw_output: |
  === cargo test -p gputeer-cli --test import_manifest ===
  8 passed / 0 failed

  === cargo test --workspace ===
  passed=841  failed=0
    (이 조각 착수 전 833 — 신규 8건이 늘어난 전부이고 회귀 0)

  === 뮤테이션 7건 ===
  I1  keyring 대신 아무 서명이나 받아들임  -> unknown_signer 실패
  I2  평문 keyring 을 항상 허용            -> plaintext_keyring 실패
  I4  만료 검사 우회(now=0)                -> expired 실패
  I5  is_durable() 위임 제거               -> non_durable 실패(:memory: 와 빈 경로 둘 다)
  I6  거부 사유를 하나로 뭉갬              -> 세 거부 테스트가 각각 실패
  I7  거부 전에 job store 를 **열기만** 함
        -> 빈 DB 3건 실패 · ★ 8번째는 **통과**(여는 것만으론 바이트가 안 바뀐다)
  I8  거부 경로가 audit 행을 남김           -> 4건 전부 실패

  ★ I3(":memory:" 문자열 관문 제거)은 I5 로 대체됐다 — 그 관문 자체를
    없애고 저장소에 위임했으므로 뮤테이션 대상이 사라졌다.

negative_tests:
  - "keyring 에 없는 서명자를 거부한다 + DB 무변경(I1)"
  - "서명 바이트 한 개를 뒤집으면 거부한다 + DB 무변경"
  - "만료된 Manifest 를 거부한다 + DB 무변경(I4) — 서명은 정상이다"
  - "평문 keyring 은 명시적 opt-in 없이 거부한다(I2) — 신뢰 목록 자체가 위조 가능하다"
  - "`:memory:` 와 **빈 경로** 둘 다 비영속으로 거부한다(I5) — 저장소에 물어본다"
  - "세 거부 사유가 각각 다른 문장을 낸다(I6) — 공통 문자열만 보면 뭉갠 구현도 통과한다"
  - "이미 Job 이 든 DB 에 나쁜 Manifest 를 들이밀어도 파일 바이트가 그대로다(I8)"
  - "같은 입력 재반입은 행을 늘리지 않고 created=false 로 보고한다"

limitations: >
  - **멤버십 권위가 없다.** keyring 은 운영자의 **로컬** 목록이지 팀
    합의가 아니다. `PersistentKeyring` 이 `revoke()`·`quarantine()` 을
    갖지만 그건 이 기계의 운영자가 정하는 것이다.
  - **네트워크 제출을 받지 않는다.** 파일로만 받는다. wire ingress 는
    별도 조각이다.
  - **스케줄링하지 않는다.** 저장만 한다 — `SUBMITTED` 상태로 들어가고
    `QUEUED` 로 올리는 production 호출자는 아직 없다.
  - **거부 사유 넷을 나눌 수 없다.** 미등록·폐기·회전 만료·격리가
    검증 계층에서 `UnknownSigner` 로 합류하므로 메시지도 넷을 다 말한다.
  - **revoke 반례를 만들지 않았다.** 위 이유로 "모르는 서명자" 반례와
    구분되는 관측을 만들 수 없다.
  - **Linux 에서 측정하지 않았다.** 플랫폼 의존이 없다고 판단하지만
    측정하지 않은 것은 측정하지 않은 것이다.
  - **비영속 DB 거부는 DB 무변경을 확인하지 않는다.** `:memory:` 와 빈
    경로에는 대조할 파일이 없다.
---

# DoD-67 — 제출된 Manifest 의 운영자 반입

`gputeer import-manifest` 가 서명된 `JobManifest` 를 운영자 keyring 으로
검증한 뒤에만 필드를 읽고 durable job store 에 넣는다.

## 이 조각에서 배운 것

### 이미 아는 것에게 물어봐야지 다시 만들면 어긋난다

가장 무거운 결함이 이것이었다.

```text
내가 쓴 것        if job_db_path == ":memory:" { 거부 }
저장소가 아는 것   is_durable() — :memory: **와 빈 경로**를 둘 다 판정한다
결과              --job-db "" 가 조용히 성공했다
```

SQLite 는 빈 경로도 임시 DB 로 열어 준다. 넣은 것이 프로세스와 함께
사라지는데 로그에는 성공으로 찍혔다.

`DoD-65` 의 "관문이 자기가 어느 lane 인지 **설정에 묻지 않는다**" 와 같은
교훈의 다른 얼굴이다 — 그때는 이미 아는 것을 **남에게 물어서** 틀렸고,
이번엔 이미 아는 것을 **다시 만들어서** 틀렸다.

### 이름이 주장을 결정한다

독립 설계 조사가 구현 전에 경고했고 그대로 받아들였다.

> 임의 공개키까지 같은 명령에서 받아 "verified/accepted" 라고 부르는 것은
> 정직하지 않다.

명령줄로 공개키를 받으면 **신뢰 경계가 그 명령 한 줄**이 된다. 아무나
자기 키를 붙여 "검증됨" 을 만들 수 있고, 그건 검증이 아니라 서명 확인일
뿐이다. 그래서 이 명령은 운영자가 미리 provision 한 keyring 파일에 있는
서명자만 받고, 이름도 "제출 접수" 가 아니라 **"반입"** 이다.

### 정정이 또 과했다

거부 사유를 나누는 과정이 두 단계였다.

```text
1차   전부 "서명 검증 실패"
      -> 만료된 **정상 서명**을 그렇게 부르면 운영자가 키를 의심하러 간다

2차   UnknownSigner = "신뢰 목록에 없는 서명자"
      -> 폐기·회전 만료·격리도 같은 결과다. 목록에 **있는데** 폐기된
         서명자를 오진해, 다시 넣으러 가게 만든다 — 그게 틀린 대응이다

3차   "신뢰 목록에서 쓸 수 있는 키를 찾지 못했다(미등록·폐기·회전 만료·격리)"
      -> 나눌 수 없으므로 나눌 수 있는 척하지 않는다
```

### "거부했다" 만 보는 테스트는 절반만 잰다

거부하면서 흔적을 남기는 구현도 통과한다. 그래서 파일 바이트 전체를
비교하는데 — 2라운드가 짚었듯 **거부 테스트가 전부 빈 DB 였으면 그
비교는 `None == None` 만 확인한다.** 운영 중인 job store 는 비어 있지
않다.

뮤테이션이 그 판별력의 경계를 정확히 보여줬다.

```text
I7  거부 전에 store 를 **열기만** 함   빈 DB 3건 실패 · 채워진 DB 는 통과
I8  거부가 audit 행을 남김             4건 전부 실패
```

여는 것만으로는 바이트가 안 바뀐다. 채워진 DB 테스트의 판별력은
**"행 쓰기"** 에 있지 "열기" 에 있지 않다 — evidence 에 그렇게 적었다.

### 여섯 번째 속성/문서 가로채기

8번째 테스트를 만료 테스트 **앞에** 끼워 넣으며 그 `///` 를 가져갔다.
내가 검수 프롬프트에 "특히 이걸 보라" 고 적어 놓고 같은 턴에 저질렀다.

Rust 에서 `#[...]` 와 `///` 는 바로 아래 항목에 붙는다. 사이에 무언가를
끼워 넣으면 소속이 조용히 바뀌는데 **컴파일도 테스트도 통과한다.**

### 검수가 틀릴 수도 있다

3라운드가 "help 는 `agent-stub` 에 두 device ID 를 필수처럼 쓰지만 실제
파서는 안 읽는다" 고 했다. 실행해 보면:

```text
agent-stub 실패: 필수 인자 누락: --coordinator-device-id
```

`crates/agent/src/lib.rs:2124-2125` 가 둘 다 `require()` 한다. 검수는
구조체 앞부분(`:2065`)만 보고 멈춘 것으로 보인다 — 유효한
`--peer-pubkey` 를 줘야 여기까지 온다. 받아들였으면 **맞는 문서를
틀리게 고칠 뻔했다.** 4라운드가 반박을 확인했다.

## 이 실험이 증명하지 않는 것

```text
멤버십 권위        keyring 은 운영자의 로컬 목록이다. 팀 합의가 아니다
네트워크 제출      파일로만 받는다. wire ingress 는 별도 조각이다
스케줄링           SUBMITTED 로 저장만 한다. QUEUED 로 올리는 production
                   호출자가 없다
거부 사유 넷의 구분 미등록·폐기·회전 만료·격리가 검증 계층에서 합류한다
Linux 동작         측정하지 않았다. 플랫폼 의존이 없다고 판단할 뿐이다
비영속 거부의 무변경 :memory: 와 빈 경로에는 대조할 파일이 없다
```
