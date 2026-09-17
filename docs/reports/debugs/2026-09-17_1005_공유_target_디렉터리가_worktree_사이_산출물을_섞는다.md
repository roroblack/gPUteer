# 공유 target 디렉터리가 worktree 사이 산출물을 섞는다 (결함 130)

> **작성** 2026-09-17 10:05 · **고치기 전에 쓴다**(RULE.md §5.3) · 발견: 옛 상대 조합 시험(test/old-peer-combination)을 준비하다 이 세션이 찾았다

## 1. 위치

```text
이 세션의 측정 절차  CARGO_TARGET_DIR=<gputeer>/target 를 **모든 worktree** 가 함께 쓴다(C: 여유 공간 때문에)
                    gputeer-be · gputeer-be-report · gputeer-be-gc · gputeer-be-95 · gputeer-be-mem · gputeer-be-compat
cargo 의 판단        같은 워크스페이스 구성원은 worktree 가 달라도 산출물 이름(해시)이 같다. 신선도는 **지금 빌드하는 worktree 의 파일 시각**으로
                    잰다 — 다른 worktree 에서 최근에 빌드한 산출물이 이 worktree 의 (더 오래된) 소스보다 새것이면 "Fresh" 로 재사용한다
debug/gputeer.exe   통합 테스트 · selftest 가 쓰는 실행 파일. 재사용(Fresh)이면 다시 링크하지 않아 **다른 worktree 의 실행 파일**이 남는다
뮤테이션 스크립트     변이를 넣고 빌드 · 시험한 뒤 소스를 되돌리지만 **산출물은 변이된 채** 남는다 — 같은 worktree 는 되돌린 소스의 시각이 새로워
                    다시 빌드하지만, 다른 worktree 는 그 변이 산출물을 Fresh 로 재사용할 수 있다
```

## 2. 재현 (2026-09-17, 실측)

```text
1  옛 커밋(7d5a9fe) 을 분리 worktree 로 꺼내 같은 target 으로 cargo build -p gputeer-cli -> 25초, 출력 뒷부분에 agent · cli 만 Compiling
   (★ 출력을 tail 로 잘라 그 앞은 보지 못했다)
2  그 뒤 gputeer-be-compat 에서 cargo test(통합 테스트) -> 컴파일 없이 실행. debug/gputeer.exe 의 sha256 이 옛 복사본과 **같았다**(70af08ed…)
   -> "새 · 옛" 조합 시험이 실제로는 옛 · 옛이었다(둘 다 성공)
3  compat 의 cli 를 touch 해 다시 링크(41af4237…)한 뒤, 옛 worktree 에서 cargo build -v -> **모든 로컬 crate 가 Fresh**(coordinator 포함 —
   7d5a9fe 와 D2 커밋 사이에 coordinator lib.rs 가 65줄 다르다). 즉 옛 worktree 빌드가 다른 worktree 의 산출물을 그대로 썼다
4  cargo 출력 감사(Compiling 줄의 경로) — 오늘 기록한 측정 중 일부는 **일부 crate 만** 자기 worktree 에서 컴파일했다:
     rs_64b(결함 111~113, gputeer-be)   coordinator · cli 만 — agent 는 다른 worktree(직전 뮤테이션을 돌린 report)의 산출물일 수 있다
     rs_85(결함 85, gputeer-be-gc)      agent · cli 만 — coordinator 는 다른 worktree(직전 뮤테이션 M42 를 돌린 be)의 산출물일 수 있다
     rs_64(REPORT 세션, report)        cli 만 — 같은 worktree 의 직전 빌드였을 가능성이 높지만 확인하지 못했다
```

## 4. 위험도

- [x] **증거 무결성** — 커밋 메시지 · HISTORY · 원본에 적은 "N passed · selftest 97" 이 그 브랜치 코드가 아니라 섞인 산출물로 잰 값일 수 있다.
  변이된 산출물이 섞였다면 결함을 숨기거나 거짓 실패를 낼 수 있다
- [x] 오늘 이 세션이 만든 브랜치 전부가 대상이다: feat/b-e-contract(bbbf53b · 6c1eb19 · 63b2ed7) · feat/b-e-report-session(95af3f5) ·
  feat/agent-startup-gc(1627c15) · fix/lease-max-duration-boundary(5fbbcd7) · fix/memory-observation-cause(0c45c41)
- [x] 뮤테이션 판정도 대상이다 — 변이한 crate 는 다시 빌드되지만 나머지 crate 가 다른 worktree 것일 수 있다
- ★ 이 결함은 **어떤 수치가 틀렸다는 증거가 아니다** — 틀렸을 수 있다는 것이다. 그래서 다시 잰다

## 6. 조치

- [x] 고치기 전에 쓴다
- [x] (2026-09-17 10:29) 측정 절차 — 측정마다 로컬 crate 11개를 `cargo clean -p` 로 지우고 빌드한다. 원본에 "Compiling 줄이 로컬 crate 전부 · 전부 이 worktree 경로" 를 기계로 확인한 결과를
  싣고, selftest 전에 debug/gputeer.exe 의 sha256 을 남긴다
- [x] (2026-09-17 10:29) 다섯 브랜치 머리를 새 절차로 다시 쟀다 — **전부 0 failed · 경고 0 · selftest 97**. 범위를 넓혀 쟀기 때문에 전에 적은 수치와 개수는 직접 비교되지 않는다
  (각 브랜치는 앞 브랜치 + 그 조각의 새 테스트 수만큼 늘었다 — 435 = 418 + REPORT 17 · 439 = 435 + 결함 85 의 4 · 442 = 439 + 79 · 81 의 3):

```text
feat/b-e-contract 2c9e1a1(코드 = 63b2ed7)                    passed 418 · failed 0 · ignored 0 · warning 줄 0 · selftest exit 0 줄 97 (이전 기록 323 · 범위 다름: coordinator + cli 두 파일)
feat/b-e-report-session 7c4a4eb(코드 = 95af3f5)              passed 435 · failed 0 · ignored 0 · warning 줄 0 · selftest exit 0 줄 97 (이전 기록 414 · 범위 다름: issue_grant · shared_control_db 없음)
feat/agent-startup-gc 1627c15                              passed 439 · failed 0 · ignored 0 · warning 줄 0 · selftest exit 0 줄 97 (이전 기록 113 · 범위 다름: agent + cli 두 파일)
fix/lease-max-duration-boundary 5fbbcd7                    passed 439 · failed 0 · ignored 0 · warning 줄 0 · selftest exit 0 줄 97 (이전 기록 coordinator 305 · cli 29 · 범위 다름)
fix/memory-observation-cause 0c45c41                       passed 442 · failed 0 · ignored 0 · warning 줄 0 · selftest exit 0 줄 97 (이전 기록 116 · 범위 다름)
```
  원본 `docs/evidence/_raw/결함130_깨끗한_재측정_2026-09-17.txt`
- [ ] ★ **뮤테이션 판정은 다시 돌리지 않았다** — 그 판정들은 섞인 산출물에서 났을 수 있다. 다음 조각부터 새 절차 뒤에 돌리고, 지난 판정은 "재확인 안 됨" 으로 둔다
- [ ] 옛 상대 조합 시험의 옛 바이너리는 **별도 target 디렉터리**로 빌드한다
