#!/bin/sh
# gPUteer 초대 파일 만들기 (운영자 · Linux) — make-invite.ps1 과 같다. 공개 정보뿐이지만 주소가 들어 있으니 저장소 · 공개 채널에 올리지 않는다.
# 예:  sh make-invite.sh --env-file /etc/gputeer/gputeer.env --out ./invite.env [--shared-root-for-nodes <팀원 PC 에서 보이는 경로>]
set -eu

# ★ 결함 554 (재검수 145) — root 로 돌면 **첫 명령 전에** PATH 를 시스템 폴더로 고정하고 IFS 를 기본값으로 되돌린다(install-node.sh 의 505 와 같다).
#   부른 계정의 PATH(예 $HOME/bin)에 그 계정의 작업이 심어 둔 dirname · date · sed 등을 root 가 실행하지 않게 한다. root 인지는 절대 경로로 본다.
if [ "$(/usr/bin/id -u)" -eq 0 ]; then
    PATH=/usr/sbin:/usr/bin:/sbin:/bin
    export PATH
    IFS=$(printf ' \t\nX')
    IFS=${IFS%X}
fi
die() { echo "$*" >&2; exit 1; }
ENV_FILE= OUT= SHARED_FOR_NODES=
while [ $# -gt 0 ]; do
    case "$1" in
        --env-file) ENV_FILE=$2; shift 2 ;;
        --out) OUT=$2; shift 2 ;;
        --shared-root-for-nodes) SHARED_FOR_NODES=$2; shift 2 ;;
        *) die "INVITE_ARGS: 모르는 옵션 $1" ;;
    esac
done
[ -n "$ENV_FILE" ] && [ -n "$OUT" ] || die "INVITE_ARGS: --env-file · --out 은 반드시 준다"
value() { sed -n "s/^[[:space:]]*$1=//p" "$ENV_FILE" | tail -n 1 | tr -d '\r'; }
for key in GPUTEER_CONNECT GPUTEER_COORDINATOR_ID GPUTEER_COORDINATOR_PUBKEY GPUTEER_SUBMITTER_PUBKEY GPUTEER_SHARED_ROOT; do
    v=$(value "$key")
    [ -n "$v" ] && [ "${v#<}" = "$v" ] || die "INVITE_ARGS: $ENV_FILE 에 $key 가 채워져 있지 않다"
done
[ ! -e "$OUT" ] || die "INVITE_ARGS: $OUT 가 이미 있다 — 덮지 않는다(다른 이름을 준다)"
SHARED=${SHARED_FOR_NODES:-$(value GPUTEER_SHARED_ROOT)}
POOL=$(value GPUTEER_POOL_AGENTS); [ "${POOL#<}" = "$POOL" ] || POOL=
cat > "$OUT" <<END
# gPUteer 초대 파일 — $(date '+%Y-%m-%d %H:%M') · 비밀 없음. 저장소 · 공개 채널에 올리지 않는다(주소가 들어 있다)
GPUTEER_INVITE_VERSION=1
GPUTEER_CONNECT=$(value GPUTEER_CONNECT)
GPUTEER_COORDINATOR_ID=$(value GPUTEER_COORDINATOR_ID)
GPUTEER_COORDINATOR_PUBKEY=$(value GPUTEER_COORDINATOR_PUBKEY)
GPUTEER_SUBMITTER_PUBKEY=$(value GPUTEER_SUBMITTER_PUBKEY)
GPUTEER_SHARED_ROOT=$SHARED
GPUTEER_POOL_AGENTS=$POOL
END
echo "INVITE_WRITTEN $OUT"
