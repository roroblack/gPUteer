# 신뢰망 파티 서비스 틀

절차와 이유는 [`docs/runbooks/신뢰망_설치_운영.md`](../../docs/runbooks/신뢰망_설치_운영.md) 에 있다. 여기는 **틀**뿐이다.

| 파일 | 무엇 |
|---|---|
| `gputeer.env.template` | 세 서비스가 읽는 값의 자리 표시 — 저장소 **밖**(`/etc/gputeer/` 등)에 복사해 채운다 |
| `gputeer-coordinator.service` · `gputeer-scheduler.service` · `gputeer-agent@.service` | Linux systemd 유닛 |
| `coordinator.ps1` · `scheduler.ps1` · `agent.ps1` | Windows — 작업 스케줄러에 "로그온 시" 로 등록한다(각 파일 머리말) |

★ 채운 파일을 이 저장소에 되돌려 넣지 않는다 — 주소 · 계정 · 키 경로가 들어간다(전역 규칙).
