# runbooks/ — 운영·복구 절차

**"장애가 났을 때 무엇을 어떤 순서로 하는가"** 를 담는다.
설계 근거(계획서)나 검증 기록(evidence)이 아니라 **실행 절차**다.

> **최종 갱신** 2026-09-06 · **성격** 색인 ·
> **짝** `RULE.md` §3.6(역할) · **검사** `check_docs.py` 가 경로 존재를 본다

## 있는 런북

| 파일 | 내용 |
|---|---|
| [`ai-workflow.md`](ai-workflow.md) | AI 도구 매핑 — 역할 -> 실제 도구, **어떻게** 검수하는가 |
| [`검수_대기열.md`](검수_대기열.md) | 지금 밀린 검수 8건을 **무엇부터** 돌리는가(2026-09-07 15:43~) |

## 필요한 런북 (미작성)

| 파일 | 내용 | 선행 |
|---|---|---|
| `coordinator-quorum-loss.md` | quorum 상실 시 진단·복구 | v0.2 |
| `emergency-reconfiguration.md` | 다수 Coordinator 상실 시 강제 재구성 | v0.2 · 기준선 §43.4 P1-10 |
| `key-rotation.md` | 디바이스·Owner 키 회전 절차 | v0.2 |
| `owner-key-recovery.md` | Owner Key 분실 시 Recovery Key 사용 | v0.2 |
| `agent-rollback.md` | 에이전트 업데이트 실패 시 롤백 | v0.2 |
| `checkpoint-recovery.md` | PARTIAL 체크포인트 정리·복구 | v0.1 |

★ `ai-workflow.md` 를 따로 두는 이유: `RULE.md` 는 **역할**만 정의하고 도구를 지정하지 않는다.
도구는 바뀌지만 역할은 남기 때문이다.
