#!/bin/sh
# gPUteer 노드 받기 (운영자 · Linux) — admit-node.ps1 과 같다. 설명(서명 검증 없음 · 받은 뒤 다시 띄울 것)은 그 파일 머리말.
# 예:  sh admit-node.sh --env-file /etc/gputeer/gputeer.env --join-file ./join-bob-gpu0.json
set -eu
die() { echo "$*" >&2; exit 1; }
ENV_FILE= JOIN_FILE=
while [ $# -gt 0 ]; do
    case "$1" in
        --env-file) ENV_FILE=$2; shift 2 ;;
        --join-file) JOIN_FILE=$2; shift 2 ;;
        *) die "ADMIT_ARGS: 모르는 옵션 $1" ;;
    esac
done
[ -n "$ENV_FILE" ] && [ -n "$JOIN_FILE" ] || die "ADMIT_ARGS: --env-file · --join-file 은 반드시 준다"
value() { sed -n "s/^[[:space:]]*$1=//p" "$ENV_FILE" | tail -n 1 | tr -d '\r'; }
BIN=$(value GPUTEER_BIN); DB=$(value GPUTEER_CONTROL_DB)
[ -n "$BIN" ] && [ "${BIN#<}" = "$BIN" ] || die "ADMIT_ARGS: $ENV_FILE 에 GPUTEER_BIN 이 채워져 있지 않다"
[ -n "$DB" ] && [ "${DB#<}" = "$DB" ] || die "ADMIT_ARGS: $ENV_FILE 에 GPUTEER_CONTROL_DB 가 채워져 있지 않다"

field() { tr -d '\r' < "$JOIN_FILE" | sed -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p"; }
[ "$(grep -c '"verifying_key_hex"' "$JOIN_FILE")" -eq 1 ] || die "JOIN_REJECTED: 노드 하나짜리 가입 문서가 아니다"
NODE_ID=$(field node_id | head -n 1); KEY=$(field verifying_key_hex)
echo "$NODE_ID" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$' || die "JOIN_REJECTED: 노드 id 형식 오류 '$NODE_ID'"
echo "$KEY" | grep -Eq '^[0-9a-f]{64}$' || die "JOIN_REJECTED: 공개키가 64자리 16진수가 아니다"

POOL=$(value GPUTEER_POOL_AGENTS); [ "${POOL#<}" = "$POOL" ] || POOL=
EXISTING=; OLD_IFS=$IFS; IFS=';'
for entry in $POOL; do
    [ -n "$entry" ] || continue
    id=${entry%%=*}; k=${entry#*=}
    if [ "$id" = "$NODE_ID" ]; then
        [ "$k" = "$KEY" ] || { IFS=$OLD_IFS; die "JOIN_REJECTED: $NODE_ID 가 다른 공개키로 이미 풀에 있다 — 키를 바꾸려면 운영자가 직접 정리한다"; }
        EXISTING=1
    elif [ "$k" = "$KEY" ]; then
        IFS=$OLD_IFS; die "JOIN_REJECTED: 같은 공개키를 $id 가 이미 쓴다(POOL_AGENTS_DUPLICATE_KEY)"
    fi
done
IFS=$OLD_IFS

ENV_DIR=$(cd "$(dirname "$ENV_FILE")" && pwd)
ADMITTED_DIR="$ENV_DIR/admitted"; STAMP=$(date +%Y-%m-%d_%H%M)
mkdir -p "$ADMITTED_DIR"
ADMITTED="$ADMITTED_DIR/join-$NODE_ID.json"
# ★ 결함 443 (재검수 115) — **반입이 성공한 뒤에만** 활성 사본을 바꾼다. 전에는 먼저 덮고 반입했다 — 거부될 가입 파일이 활성 사본이 되면
#   refresh-inventory 가 매번 실패하고 정상 선언이 낡아 노드가 빠졌다. 후보는 join-*.json 이 아닌 이름이라 refresh 가 줍지 않는다.
CANDIDATE="$ADMITTED_DIR/.candidate-$NODE_ID.json"
# 관측 시각 · revision 을 지금으로(결함 301).
NOW_MS=$(($(date +%s) * 1000))
sed -e "s/\"inventory_revision\"[[:space:]]*:[[:space:]]*[0-9]*/\"inventory_revision\": $NOW_MS/" \
    -e "s/\"observed_at_unix_ms\"[[:space:]]*:[[:space:]]*[0-9]*/\"observed_at_unix_ms\": $NOW_MS/" "$JOIN_FILE" > "$CANDIDATE"
if ! "$BIN" import-inventory --inventory "$CANDIDATE" --inventory-db "$DB"; then
    rm -f "$CANDIDATE"
    die "import-inventory 실패 — 풀 목록도 받아 둔 가입 파일(admitted/)도 바꾸지 않았다"
fi
if [ -e "$ADMITTED" ]; then mkdir -p "$ADMITTED_DIR/_backup/$STAMP"; cp -p "$ADMITTED" "$ADMITTED_DIR/_backup/$STAMP/"; fi
mv "$CANDIDATE" "$ADMITTED"

if [ -z "$EXISTING" ]; then
    NEW_POOL=${POOL:+$POOL;}$NODE_ID=$KEY
    mkdir -p "$ENV_DIR/_backup/$STAMP"; cp -p "$ENV_FILE" "$ENV_DIR/_backup/$STAMP/"
    if grep -q '^[[:space:]]*GPUTEER_POOL_AGENTS=' "$ENV_FILE"; then
        sed "s|^[[:space:]]*GPUTEER_POOL_AGENTS=.*|GPUTEER_POOL_AGENTS=$NEW_POOL|" "$ENV_FILE" > "$ENV_FILE.tmp"
    else
        { cat "$ENV_FILE"; echo "GPUTEER_POOL_AGENTS=$NEW_POOL"; } > "$ENV_FILE.tmp"
    fi
    cat "$ENV_FILE.tmp" > "$ENV_FILE"; rm -f "$ENV_FILE.tmp"
    echo "BACKUP $ENV_FILE -> $ENV_DIR/_backup/$STAMP"
    echo "POOL_AGENTS $NEW_POOL"
else
    echo "POOL_AGENTS_UNCHANGED $NODE_ID 는 같은 키로 이미 있다(등록 정보만 새로 넣었다)"
fi
echo "ADMITTED $NODE_ID"
echo "NEXT Coordinator · scheduler 를 다시 띄운다(systemctl restart gputeer-coordinator gputeer-scheduler). 다른 Agent 들의 GPUTEER_POOL_AGENTS 도 위 줄로 바꾸고 다시 띄운다."
