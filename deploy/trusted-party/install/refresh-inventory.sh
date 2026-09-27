#!/bin/sh
# gPUteer 노드 등록 정보 새로 넣기 (운영자 · Linux) — refresh-inventory.ps1 과 같다(결함 301). cron 에 하루 한 번보다 자주 건다:
#   0 */6 * * *  sh <이 파일> --env-file /etc/gputeer/gputeer.env >> <로그 파일> 2>&1
#   ★ 결함 463 — 출력을 로그에 남긴다. 실패(ADMIT_INCOMPLETE · REFRESH_FAILED)는 로그와 종료 코드로 본다.
# ★ 노드가 살아 있는지는 확인하지 않는다 — 받은 선언을 다시 적을 뿐이다. 생존은 scheduler 의 --silent-after-ms 가 본다.
# ★ 결함 440 (재검수 115) — 이것은 하드웨어를 다시 읽지 않는다. 받아 둔 **선언**에 지금 시각을 찍을 뿐이라, 틀린 선언도 신선해진다.
#   그래서 **GPU 관측을 켜지 않은 노드**(CPU 전용 · 관측 전 판 Agent)용이다. 관측을 켠 노드는 인사의 관측이 신선도를 준다.
#   관측이 선언과 어긋난 노드는 이것으로 되살아나지 않는다 — Coordinator 가 불일치를 노드별로 기억하고, 맞는 관측이 올 때까지 배치에서 뺀다.
set -eu
die() { echo "$*" >&2; exit 1; }
[ "${1:-}" = --env-file ] && [ -n "${2:-}" ] || die "REFRESH_ARGS: --env-file <운영자 gputeer.env>"
ENV_FILE=$2
ADMITTED_DIR="$(cd "$(dirname "$ENV_FILE")" && pwd)/admitted"
# ★ 결함 452 (재검수 116) — admit-node 가 반입과 활성 사본 교체 사이에 끊겼으면 후보(.candidate-*)가 남는다. 그때 활성 사본(옛 선언)에 새 시각을 찍으면
#   방금 반입한 새 선언을 되돌린다 — 그 노드는 넣지 않는다(신선도가 끊겨 그 노드가 빠지는 쪽). admit-node 를 다시 돌리면 풀린다.
# ★ 결함 463 (재검수 117) — 후보가 남은 **그 노드만** 건너뛰고 나머지는 갱신한 뒤 0 이 아닌 코드로 끝낸다(전에는 후보 하나가 전체를 멈춰 풀 전체가 빠졌다).
[ -d "$ADMITTED_DIR" ] || die "REFRESH: $ADMITTED_DIR 가 없다 — admit-node 로 먼저 받는다"
# ★ 결함 466 (재검수 118) — admit-node · refresh-inventory 는 받아 둔 가입 파일을 같이 고친다. 동시에 돌면 refresh 가 옛 사본을 다시 활성 사본으로
#   썼다 — admitted/.lock 을 O_EXCL(noclobber)로 잡고 돈다. 잡혀 있으면 멈춘다. 끝나면(실패해도) 지운다.
LOCK="$ADMITTED_DIR/.lock"
( set -C; echo "$$" > "$LOCK" ) 2>/dev/null || die "ADMITTED_LOCKED: $LOCK 이 있다 — admit-node · refresh-inventory 가 이미 돈다. 도는 것이 없는데 남았으면 지우고 다시 돌린다"
trap 'rm -f "$LOCK"' EXIT
# ★ 결함 479 (재검수 120) — 환경 파일은 잠근 **뒤에** 읽는다(admit 이 바꾸는 중인 파일을 읽지 않게).
value() { sed -n "s/^[[:space:]]*$1=//p" "$ENV_FILE" | tail -n 1 | tr -d '\r'; }
BIN=$(value GPUTEER_BIN); DB=$(value GPUTEER_CONTROL_DB)
# ★ 결함 477 (재검수 120) — 사본을 바꿔 넣다 끊겨 남은 임시 파일(join-*.json.tmp)을 먼저 본다 — 활성 사본이 없으면 되살리고, 있으면 낡은 것이라 지운다.
for leftover in "$ADMITTED_DIR"/join-*.json.tmp; do
    [ -e "$leftover" ] || continue
    target=${leftover%.tmp}
    if [ -e "$target" ]; then rm -f "$leftover"; else mv "$leftover" "$target"; echo "RECOVERED $(basename "$target") — 바꿔 넣다 끊긴 임시 파일에서 되살렸다"; fi
done
count=0; failed=0; incomplete=
NOW_MS=$(($(date +%s) * 1000))
for file in "$ADMITTED_DIR"/join-*.json; do
    [ -e "$file" ] || continue
    count=$((count + 1))
    node=$(basename "$file" .json); node=${node#join-}
    if [ -e "$ADMITTED_DIR/.candidate-$node.json" ]; then
        incomplete="$incomplete $node"
        echo "ADMIT_INCOMPLETE $node — admitted/.candidate-$node.json 이 남아 있다. 그 노드의 가입 파일로 admit-node 를 다시 돌린다(이번 refresh 는 건너뛴다)" >&2
        continue
    fi
    # ★ 결함 467 — revision 은 max(지금, 이 사본의 revision + 1) · observed_at 은 지금.
    REV=$NOW_MS
    PREV=$(tr -d '\r' < "$file" | sed -n 's/.*"inventory_revision"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p' | head -n 1)
    [ -z "$PREV" ] || [ "$REV" -gt "$PREV" ] || REV=$((PREV + 1))
    sed -e "s/\"inventory_revision\"[[:space:]]*:[[:space:]]*[0-9]*/\"inventory_revision\": $REV/" \
        -e "s/\"observed_at_unix_ms\"[[:space:]]*:[[:space:]]*[0-9]*/\"observed_at_unix_ms\": $NOW_MS/" "$file" > "$file.tmp"
    mv "$file.tmp" "$file"
    if "$BIN" import-inventory --inventory "$file" --inventory-db "$DB"; then
        echo "REFRESHED $(basename "$file")"
    else
        failed=$((failed + 1)); echo "REFRESH_FAILED $(basename "$file")"
    fi
done
for candidate in "$ADMITTED_DIR"/.candidate-*.json; do
    [ -e "$candidate" ] || continue
    node=$(basename "$candidate" .json); node=${node#.candidate-}
    case " $incomplete " in *" $node "*) ;; *) incomplete="$incomplete $node"; echo "ADMIT_INCOMPLETE $node — 받아 둔 활성 사본 없이 후보만 남았다. admit-node 를 다시 돌린다" >&2 ;; esac
done
[ "$count" -gt 0 ] || [ -n "$incomplete" ] || die "REFRESH: $ADMITTED_DIR 에 받은 노드가 없다 — admit-node 로 먼저 받는다"
[ -z "$incomplete" ] || die "REFRESH: admit-node 가 끝나지 않은 노드가 있다(${incomplete# }) — 나머지는 갱신했다"
[ "$failed" -eq 0 ] || die "REFRESH: $failed 개 실패"
