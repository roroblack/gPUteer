#!/bin/sh
# gPUteer 노드 등록 정보 새로 넣기 (운영자 · Linux) — refresh-inventory.ps1 과 같다(결함 301). cron 에 하루 한 번보다 자주 건다:
#   0 */6 * * *  sh <이 파일> --env-file /etc/gputeer/gputeer.env
# ★ 노드가 살아 있는지는 확인하지 않는다 — 받은 선언을 다시 적을 뿐이다. 생존은 scheduler 의 --silent-after-ms 가 본다.
# ★ 결함 440 (재검수 115) — 이것은 하드웨어를 다시 읽지 않는다. 받아 둔 **선언**에 지금 시각을 찍을 뿐이라, 틀린 선언도 신선해진다.
#   그래서 **GPU 관측을 켜지 않은 노드**(CPU 전용 · 관측 전 판 Agent)용이다. 관측을 켠 노드는 인사의 관측이 신선도를 준다.
#   관측이 선언과 어긋난 노드는 이것으로 되살아나지 않는다 — Coordinator 가 불일치를 노드별로 기억하고, 맞는 관측이 올 때까지 배치에서 뺀다.
set -eu
die() { echo "$*" >&2; exit 1; }
[ "${1:-}" = --env-file ] && [ -n "${2:-}" ] || die "REFRESH_ARGS: --env-file <운영자 gputeer.env>"
ENV_FILE=$2
value() { sed -n "s/^[[:space:]]*$1=//p" "$ENV_FILE" | tail -n 1 | tr -d '\r'; }
BIN=$(value GPUTEER_BIN); DB=$(value GPUTEER_CONTROL_DB)
ADMITTED_DIR="$(cd "$(dirname "$ENV_FILE")" && pwd)/admitted"
NOW_MS=$(($(date +%s) * 1000))
count=0; failed=0
for file in "$ADMITTED_DIR"/join-*.json; do
    [ -e "$file" ] || continue
    count=$((count + 1))
    sed -e "s/\"inventory_revision\"[[:space:]]*:[[:space:]]*[0-9]*/\"inventory_revision\": $NOW_MS/" \
        -e "s/\"observed_at_unix_ms\"[[:space:]]*:[[:space:]]*[0-9]*/\"observed_at_unix_ms\": $NOW_MS/" "$file" > "$file.tmp"
    mv "$file.tmp" "$file"
    if "$BIN" import-inventory --inventory "$file" --inventory-db "$DB"; then
        echo "REFRESHED $(basename "$file")"
    else
        failed=$((failed + 1)); echo "REFRESH_FAILED $(basename "$file")"
    fi
done
[ "$count" -gt 0 ] || die "REFRESH: $ADMITTED_DIR 에 받은 노드가 없다 — admit-node 로 먼저 받는다"
[ "$failed" -eq 0 ] || die "REFRESH: $failed 개 실패"
