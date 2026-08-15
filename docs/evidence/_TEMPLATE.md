---
id: P0-03
claim: "체크포인트가 쓰기 중 프로세스 kill 되어도 PARTIAL 로만 남고 COMMITTED 로 승격되지 않는다"
status: PASS
commit: 0000000000000000000000000000000000000000
binary_digests:
  gputeer-agent: "blake3:0000...";
  gputeerd: "blake3:0000..."
protocol_versions:
  agent: "1.0"
  coordinator: "1.0"
platform: "Windows 11 26100 / Ubuntu 24.04.1"
hardware: "RTX 4090 24GB / driver 550.90 / CUDA 12.4"
network_profile: "LAN, netem 없음"
command: |
  cargo test -p gputeer-checkpoint --test chaos_kill -- --nocapture
raw_output: |
  (요약이 아니라 원문. 길어서 잘랐다면 자른 사실을 적는다)
artifacts:
  - docs/evidence/_raw/P0-03_stdout.txt
negative_tests:
  - "쓰기 중 SIGKILL -> PARTIAL 로 남고 부팅 시 GC 됨"
  - "매니페스트만 있고 데이터 파일 누락 -> PARTIAL 판정"
  - "해시 1바이트 변조 -> HASH_VERIFIED 실패"
limitations:
  - "단일 노드만 검증. 다중 replica 경합은 미검증"
  - "NTFS 만 검증. ReFS/ext4 미검증"
decision: "ADR-003 유지. §18.2 durability contract 변경 없음"
---

# P0-03 · Checkpoint Durability

> 이 파일은 템플릿이다. 복사해서 쓰고 이 문장은 지운다.
> **front-matter 15개 필드를 비우지 않는다.** `scripts/verify_evidence.py` 가 검사한다.

## 무엇을 입증하려 했는가

(claim 을 풀어서. 무엇이 참이면 통과인지)

## 어떻게 측정했는가

(실험 설계. 왜 이 방법이 claim 을 입증하는지)

## 결과

(표·수치. 평균만 적지 않는다. p50/p99 와 표본 수)

## 이 실험이 증명하지 "않는" 것

(front-matter `limitations` 를 풀어서. **이 절이 비면 반려된다.**)

## 결정

(이 결과로 무엇을 바꿨는가. ADR 링크)

---

## 판정 값 참고

| status | 의미 | 후속 |
|---|---|---|
| `PASS` | 입증됨 | 다음 게이트 |
| `FAIL-ARCHITECTURE` | 실패. **아키텍처를 바꿔야 한다** | `RULE.md` §8 전 절차 |
| `FAIL-SCOPE` | 실패. **범위를 줄이면 진행 가능** | ADR + 계획서 수정 |
| `INCONCLUSIVE` | 측정했으나 판정 불가 | 실험 설계 재작성 |
| `ENVIRONMENT-BLOCKED` | 하드웨어·환경 없어 미실행 | 차단 요인 명시 후 대기 |
| `SUPERSEDED` | 설계 변경으로 무의미해짐 | 기록만 보존 (**삭제 금지**) |

★ **`ENVIRONMENT-BLOCKED` 를 `PASS` 로 세지 않는다.** 가장 흔한 자기기만이다.
