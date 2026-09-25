# 신뢰망 파티 서비스 틀

절차와 이유는 [`docs/runbooks/신뢰망_설치_운영.md`](../../docs/runbooks/신뢰망_설치_운영.md) 에 있다. 여기는 **틀**뿐이다.

| 파일 | 무엇 |
|---|---|
| `gputeer.env.template` | 세 서비스가 읽는 값의 자리 표시 — 저장소 **밖**(`/etc/gputeer/` 등)에 복사해 채운다 |
| `gputeer-coordinator.service` · `gputeer-scheduler.service` · `gputeer-agent@.service` | Linux systemd 유닛 |
| `coordinator.ps1` · `scheduler.ps1` · `agent.ps1` | Windows — 작업 스케줄러에 "로그온 시" 로 등록한다(각 파일 머리말) |
| `install/` | 팀원 노드 붙이기 자동화 — `make-invite` · `install-node` · `admit-node` · `refresh-inventory`(각 `.ps1` · `.sh`). 런북 §9a |

★ `install/*.ps1` 은 **UTF-8 BOM 으로 저장한다** — Windows PowerShell 5.1 은 BOM 없는 파일을 시스템 코드 페이지로 읽어, 문자열 안의
  한글이 따옴표를 먹고 구문 오류가 난다(2026-09-25 실측). 위의 세 `.ps1` 은 한글이 주석에만 있어 BOM 없이도 해석된다.

★ 채운 파일을 이 저장소에 되돌려 넣지 않는다 — 주소 · 계정 · 키 경로가 들어간다(전역 규칙).
