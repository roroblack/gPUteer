---
schema_version: 2
id: DoD-59
claim: "`crates/crypto/src/keyring.rs` 의 `K1OsProtected` 가 Linux 에서 `systemd-creds --with-key=host` 로 개인키를 실제로 봉인하고, 키링 파일에 개인키 평문이 남지 않으며, 재열기로 왕복함을 x600 WSL2(systemd 259)에서 실측했다. 파일 전체 무결성 표식(BLAKE3 체크섬)도 같은 OS 저장소로 봉인해, 파일을 쓸 수 있는 **K1 경계 밖의** 공격자가 신원을 바꿔치기하지 못한다 — 공개키만 옮기고 개인키 blob 을 비우는 우회까지 막는다. `systemd-creds` 가 없으면 조용히 K0 로 내려가지 않고 실패하며, '없다' 와 '있는데 실패했다' 를 구분한다"
status: PASS
commit: 33182ed057244cb69b9a81b235cd9535ef8aa64e

executor_id: "agent:claude-code"
executor_tool: "claude-code 세션 — Linux K1 구현(systemd-creds subprocess), signer 별 봉인 이름/DPAPI entropy, 파일 체크섬 봉인, 파일 형식 v1->v2"
executor_model: "claude-opus-5"
executed_at: "2026-08-30T18:10:00+09:00"

review_required: true
reviewer_id: "agent:codex-cli"
reviewer_tool: "codex exec --sandbox read-only — 대화 기록 없는 새 인스턴스, 3라운드"
reviewer_model: "gpt-5.6-sol"
review_context: "fresh-read-only"
review_outcome: "ACCEPTED"
review_scope: "파일:줄로 지목된 지점 — `crates/crypto/src/keyring.rs:612`(개인키를 signer 정보 없이 봉인), `:871`(Linux 모든 키가 같은 credential name), `:351`~`:365`(load 가 개인키·공개키 일치만 검사), `:347`(개인키 blob 이 비면 공개키 전용 엔트리로 수용 — 복호를 건너뛰어 signer 묶기가 무의미해지는 우회), `:660`(`lookup_candidates` 가 바뀐 공개키를 돌려줌), `:32`(파일 버전이 여전히 1), `:946`(모든 spawn 실패를 `UnsupportedPlatform` 으로), `:952`(write_all 실패 시 자식 미회수), `:1057`·`:1077`(stdout/stderr 를 배출하기 전에 종료만 폴링 — 파이프 포화 시 교착), `:1186`(reader thread 무제한 join — 후손이 fd 를 물면 영영 대기), `:631`(본문 digest 만 봉인 — 세대 번호·단조 카운터 부재로 과거 정상 파일 롤백 가능), `:1030`(Windows K1 이 같은 사용자를 막지 않음 — '파일을 쓸 수 있는 공격자는 봉인을 만들 수 없다' 는 과한 일반화), `:305`(K0 강등이 `PlaintextPolicy::Reject` 로 막힘을 확인). 검수는 봉인 검증 **순서**(길이·꼬리 봉인 길이·보호 등급 바이트만 검증 전에 읽고, signer·공개키·상태 등 본문 의미는 봉인 검증 뒤에 파싱)가 안전함을 코드로 확인했다"
review_artifact: "docs/evidence/_raw/DoD-59_review_round1_verbatim.txt"

decision: "Linux K1 을 라이브러리가 아니라 **`systemd-creds` subprocess** 로 부른다 — systemd 의 credential 형식과 TPM 정책을 재구현하면 남의 PC 에서 도는 코드에 암호 구현이 하나 더 늘어난다. `dpapi_protect` 가 Win32 API 를 부르는 것과 같은 자리다. 개인키는 **stdin 으로만** 넘긴다 — 명령줄 인자는 `/proc/<pid>/cmdline` 으로 같은 기계의 아무나 읽고, 임시 파일은 지우기 전에 죽으면 남는다. 봉인 이름을 **signer 마다** 다르게 묶고(Windows 는 DPAPI optional entropy), 파일 전체 체크섬도 **봉인**한다 — 키 없는 BLAKE3 체크섬은 파일을 쓸 수 있으면 누구나 다시 계산하므로 위조를 전혀 막지 못했다. 파일 버전을 2 로 올리고 **v1 을 안 받는다** — legacy fallback 은 원래 이식 공격을 되살린다. 롤백 방지는 파일 밖 단조 카운터가 필요해 **구현하지 않고 비보장으로 적었다**(§0.4 — 반쯤 동작하는 방어를 만들지 않는다)."
raw_output_artifact: "docs/evidence/_raw/DoD-59_linux_k1_systemd_creds_x600_2026-08-30.txt"
raw_output_digest: "sha256:67ea249b56f99f210f2e17bb3a3fa1adf2e70f19e327fdcd1754d3c401c034c9"
raw_output_bytes: 4092

binary_digests:
  toolchain: "x600 WSL2 cargo 1.89.0 / Windows 개발 기계 cargo 1.97.1"
protocol_versions:
  schema_version: "proto 변경 없음 — 키링은 wire 메시지가 아니다"
  canonical_spec: "canonical 무관. 단 **키링 파일 형식**은 v1 -> v2 로 올렸고 v1 은 읽지 않는다"
platform: "실측: x600 WSL2(커널 6.18.33.2, systemd 259 가 PID 1). 교차: Windows 11 개발 기계(DPAPI 경로)"
hardware: "GPU 무관 — 키 보관은 하드웨어 GPU 와 관계없다. TPM 은 쓰지 않는다(그건 K2 이고 미구현)"
network_profile: "네트워크 미사용. x600 으로의 SSH/SCP 만"
command: |
  # x600 WSL2 — 작업 드라이브는 F: 다
  ssh x600 "wsl -e bash /mnt/f/gputeer-work/lxk1raw.sh"
  #   내부: cargo test -p gputeer-crypto --test keyring -- --nocapture
  #         뮤테이션 A(--name 을 signer 무관하게) / B(체크섬 봉인 제거)
  # Windows
  cargo test -p gputeer-crypto --test keyring
raw_output: |
  (docs/evidence/_raw/DoD-59_linux_k1_systemd_creds_x600_2026-08-30.txt 전문 참조)

  systemd 259 (259.5-0ubuntu3.4), PID 1 = systemd
  -r-------- 1 root root 4112 /var/lib/systemd/credential.secret

  기준선: test result: ok. 10 passed; 0 failed
  뮤테이션 A(--name signer 무관): 10 passed  <- ★ 실패하지 않는다(아래 한계 참조)
  뮤테이션 B(체크섬 봉인 제거): a_public_key_only_entry_cannot_be_swapped_in_either FAILED
                              test result: FAILED. 9 passed; 1 failed
  원복: test result: ok. 10 passed

artifacts:
  - crates/crypto/src/keyring.rs
  - crates/crypto/tests/keyring.rs
  - docs/evidence/_raw/DoD-59_linux_k1_systemd_creds_x600_2026-08-30.txt
  - docs/evidence/_raw/DoD-59_review_round1_verbatim.txt
  - docs/evidence/_raw/DoD-59_review_round2_verbatim.txt
negative_tests:
  - "`a_public_key_only_entry_cannot_be_swapped_in_either`: alice 엔트리를 (bob 공개키, **빈** private blob) 으로 바꾸고 평문 체크섬을 다시 계산해도 열리지 않음을 확인한다. 개인키가 없으므로 복호가 아예 안 일어나 signer 묶기로는 못 막는 경로다 — 뮤테이션 B(체크섬 봉인 제거)로 정확히 이 테스트만 실패함을 Windows·Linux 양쪽에서 확인했다"
  - "`another_signers_keypair_cannot_be_transplanted_into_this_slot`: 공개키와 봉인 blob 을 **함께** 옮겨도 열리지 않음을 확인한다. ★ 이 테스트는 두 번 공허했다 — 1차는 blob 만 옮겨 기존 '개인키·공개키 불일치' 검사가 잡았고, 2차는 파일 체크섬이 먼저 막았다. 체크섬도 다시 계산해서야 봉인을 실제로 쟀다"
  - "`linux_k1_round_trips_through_systemd_creds`: 재열기로 봉인·복호 왕복을 확인하고, 키링 파일 바이트에 개인키 seed 가 **없고** 공개키는 **있음**을 확인한다 — 후자가 없으면 '빈 파일이라 평문도 없다' 를 봉인으로 오인한다. `systemd-creds` 가 없는 환경에서는 `ENVIRONMENT-BLOCKED` 를 찍고 건너뛰되, 그 경우에도 K1 저장이 조용히 성공하지 않음을 확인한다"
  - "`unsupported_platform_and_call_failure_are_different_errors`: '이 플랫폼은 지원 안 함' 과 '지원하는데 이번 호출이 실패함' 이 서로 다른 문자열임을 고정한다 — 둘은 고치는 방법이 전혀 다르다(§3)"
  - "`plaintext_storage_is_rejected_without_explicit_opt_in`: K0 를 기본 허용하지 않는다. 이것이 K0 강등 공격(보호 등급을 K0 로 낮춰 적어 평문 검증을 유도)도 함께 막는다 — 검수가 그 순서를 코드로 확인했다"
  - "기존 5건(개인키 Debug/Display 미출력·손상 파일 거부·quarantine·revoke·회전 grace) 회귀 없음"
limitations:
  - "★ **롤백을 막지 못한다.** 봉인은 본문 변조를 막지만, 공격자가 예전의 정상 v2 파일을 통째로 되돌리면 그때 정상적으로 봉인된 파일이므로 검증을 그대로 통과한다 — 폐기·회전된 키가 되살아난다. 막으려면 파일 밖의 단조 카운터(TPM monotonic counter 등)가 필요하고 이 조각 밖이다. 반쯤 동작하는 카운터를 만들지 않고 못 막는다고 적는다(§0.4)"
  - "★ **K1 경계 안쪽은 봉인을 만들 수 있다.** Windows DPAPI 는 다른 **사용자**를, Linux host key 는 **비-root** 를 막는다. 그 경계 안의 공격자(Windows 의 같은 사용자, Linux 의 root)는 알려진 entropy/이름으로 직접 봉인을 만들 수 있다 — 처음부터 이 등급의 정의다. '파일을 쓸 수 있는 공격자는 봉인을 만들 수 없다' 는 과한 일반화이며, 정확히는 '경계 **밖**의 공격자는 못 만든다'"
  - "★ **signer 별 봉인 묶기가 이제 독립적으로 측정되지 않는다.** 파일 체크섬 봉인이 파일 변조를 먼저 잡으므로, 뮤테이션 A(`--name` 을 signer 무관하게)를 걸어도 테스트가 통과한다(실측 로그에 그대로 남아 있다). 그 방어는 여전히 코드에 있고 defense-in-depth 로 의미가 있지만, **이 evidence 는 그것이 작동함을 증명하지 않는다**"
  - "Windows·Linux 의 K1 은 **같은 이름이지만 다른 경계**다. 표로 나눠 적었으며 하나의 보증으로 읽으면 안 된다"
  - "Linux K1 은 **Agent 가 root 여야 성립한다** — `credential.secret` 이 `-r-------- root root` 다(실측 로그 참조). 비-root Agent 는 `systemd-creds` 가 설치돼 있어도 실패한다"
  - "**암호화 안 된 디스크는 보호하지 않는다** — systemd 자신이 경고한다(`Credential secret file ... is not located on encrypted media, using anyway`). TPM 봉인(`--with-key=tpm2`)이 그것까지 막지만 그건 K2 이고 미구현이다"
  - "복호 뒤의 **프로세스 메모리·크래시 덤프**는 두 플랫폼 다 보호하지 않는다. Linux 는 평문이 커널 파이프와 `systemd-creds` 프로세스 메모리에도 한 번 더 존재한다 — '명령줄·임시 파일 누출을 피했다' 가 정확하지 '누출 없음' 이 아니다"
  - "**v1 키링 파일을 못 읽는다.** 이 저장소는 배포된 적이 없어 마이그레이션 대상이 없지만, v1 파일이 있는 환경에서는 키를 새로 만들어야 한다"
  - "WSL2 한 대에서만 실측했다 — 네이티브 Linux, systemd 없는 배포판, TPM 있는 기계에서는 돌린 적이 없다"
  - "`raw_output_artifact` 는 실제 출력이지만 뮤테이션 구간은 `test result:`/`test ` 줄만 남긴 것이다(전체는 컴파일 로그로 길다). 남긴 줄 자체는 편집하지 않았다"
---

# DoD-59 — Linux 운영용 키 보관 (`systemd-creds`)

## 왜 지금 가능해졌는가

`CLAUDE.md` 가 "키 보관이 Windows 전용이다" 를 위험 공백으로 적어
뒀지만, Linux 를 반복적으로 돌릴 환경이 없어 실측할 수 없었다.
x600 의 WSL 이 응답하게 되면서 systemd 259 가 PID 1 로 도는 것을
확인했고, `systemd-creds` 왕복이 실제로 되는 것도 확인했다.

## 같은 "K1" 이 두 플랫폼에서 다른 것을 막는다

```text
           막는다                          못 막는다
Windows    같은 기계의 다른 **사용자**      같은 사용자, 관리자
(DPAPI)
Linux      같은 기계의 **비-root**          root
(host key) 키링만 훔쳐 다른 기계에서 열기
```

같다고 쓰면 안 된다. 그래서 표로 나눴다.

## 검수와 내가 독립적으로 같은 결함을 찾았다

봉인이 signer 에 안 묶여 있어 남의 키쌍을 이식할 수 있었다. 고친 뒤
검수가 **더 깊은 우회**를 냈다 — 개인키 blob 을 **비워** 공개키 전용
엔트리로 만들면 복호 자체가 안 일어나 그 방어를 통째로 건너뛴다.

```text
alice 엔트리를 (bob 공개키, 빈 private blob) 으로 바꾼다
  -> 복호 없음 -> signer 묶기가 작동하지 않는다
  -> lookup("alice") 가 bob 공개키를 돌려준다
  -> bob 의 서명이 alice 의 것으로 받아들여진다
```

근본 원인은 파일 체크섬이 **키 없는** BLAKE3 였다는 것이다 — 파일을
쓸 수 있으면 누구나 다시 계산한다. 체크섬을 봉인해 위조에 OS 비밀이
필요하게 만들었다.

## ★ 그 수정이 다른 검사를 가렸다

체크섬 봉인이 파일 변조를 **먼저** 잡으므로, signer 별 봉인 묶기를
무력화하는 뮤테이션(`--name` 을 signer 무관하게)을 걸어도 테스트가
그대로 통과한다. 실측 로그에 그 결과가 그대로 남아 있다.

그 방어는 여전히 코드에 있고 defense-in-depth 로 의미가 있지만,
**이 evidence 는 그것이 작동함을 증명하지 않는다.** 증명하려면 봉인된
체크섬까지 만들 수 있는 공격자를 가정해야 하는데, 그건 K1 경계 안쪽이라
어차피 봉인을 마음대로 만들 수 있다.

## 이 실험이 증명하지 않는 것

```text
롤백 방지        과거의 정상 파일을 되돌리면 그대로 통과한다
경계 안쪽 공격    Windows 같은 사용자·Linux root 는 봉인을 만들 수 있다
signer 묶기      체크섬 봉인이 먼저 잡아 독립 측정이 안 된다
프로세스 메모리   복호 뒤의 평문은 두 플랫폼 다 보호하지 않는다
디스크 도난      credential.secret 이 암호화 안 된 디스크에 있으면 못 막는다
TPM(K2)          구현하지 않았다
네이티브 Linux    WSL2 한 대에서만 돌렸다
```
