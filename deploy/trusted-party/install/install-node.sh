#!/bin/sh
# gPUteer 노드 설치 (팀원 PC · Linux) — install-node.ps1 과 같은 일을 한다. 설명은 그 파일 머리말.
#
# 예:
#   sh install-node.sh --invite ./invite.env --node-id bob-gpu0 --owner-member-id bob --gpu-pin 0 \
#     --cpu-cores 8 --ram-gib 16 --workspace-gib 100 \
#     --container-runtime "$(command -v podman)" --container-runtime-kind podman --container-gpu
#
# 일반 계정으로 돌린다(root 로 돌리면 멈춘다). --register 만 sudo 로 부른다 — /etc/gputeer 에 설정을 두고 gputeer-agent@<노드> 유닛을 켠다.
#   Agent 는 root 로 돌지 않는다: 인스턴스 drop-in 의 User= 를 sudo 를 부른 계정으로 채운다(결함 280 · 445).
# ★ 결함 455 (재검수 117) — --register 는 **두 단계**다. 사용자 단계(폴더 · 키 · 설정 · 점검 · 가입 파일)는 root 가 아니라 `sudo -u <부른 계정>` 으로
#   이 스크립트를 다시 돌려 그 계정 권한으로 쓰고, root 단계는 /etc 아래만 쓴다. root 는 사용자 폴더의 파일을 읽지도 쓰지도 넘기지도 않는다
#   (전에는 root 가 사용자 폴더 안에서 백업 · 쓰기 · chown 을 해, 그 계정으로 도는 워크로드가 심어 둔 링크를 따라 시스템 파일을 덮을 수 있었다).
# ★ 쓰는 파일은 전부 저장소 밖(기본 ~/.config/gputeer)이다. 자원 수치는 소유자가 내놓겠다고 알리는 양이다 — 기본값을 두지 않는다.
#   ★ 결함 510 · 511 — 배치 기준 숫자일 뿐 강제하지 않는다(가입 파일에만 들어간다). CPU · 디스크 · RAM 총량에 상한이 없다 — 런북 설치 절.
set -eu

# ★ 결함 505 (재검수 126) — root 로 돌 때는 **어떤 명령을 부르기 전에** PATH 를 시스템 폴더로 고정한다. 부른 계정의 PATH(예 $HOME/bin)를
#   물려받으면 그 계정으로 돈 워크로드가 심어 둔 dirname · id · sudo · systemctl 을 root 가 실행한다(계정 경계를 넘는다). root 인지 보는
#   id 도 절대 경로로 부른다. IFS 도 기본값으로 되돌린다. 사용자 단계(`sudo -u <계정>`)는 그 계정 권한이라 계정의 PATH 를 그대로 쓴다.
if [ "$(/usr/bin/id -u)" -eq 0 ]; then
    PATH=/usr/sbin:/usr/bin:/sbin:/bin
    export PATH
    IFS=$(printf ' \t\nX')
    IFS=${IFS%X}
fi

die() { echo "$*" >&2; exit 1; }

SELF="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
HERE=$(dirname "$SELF")
PHASE=${GPUTEER_INSTALL_PHASE:-}
case "$PHASE" in ""|user) ;; *) die "INSTALL_ARGS: GPUTEER_INSTALL_PHASE 는 설치기 안에서만 쓴다" ;; esac
WANT_REGISTER=
for arg in "$@"; do [ "$arg" != --register ] || WANT_REGISTER=1; done
USER_OUT=
if [ -n "$WANT_REGISTER" ] && [ -z "$PHASE" ]; then
    [ "$(id -u)" -eq 0 ] || die "REGISTER: --register 는 root 로 돌린다(sudo)"
    AGENT_USER=${SUDO_USER:-}
    [ -n "$AGENT_USER" ] && [ "$AGENT_USER" != root ] || die "REGISTER: sudo 로 부른다 — Agent 를 돌릴 계정(root 가 아닌)을 SUDO_USER 로 안다"
    # 사용자 단계 — 부른 계정으로 같은 인자를 다시 돌린다(--register 는 그 단계에서 무시한다).
    if USER_OUT=$(sudo -u "$AGENT_USER" -H env GPUTEER_INSTALL_PHASE=user sh "$SELF" "$@" 2>&1); then
        printf '%s\n' "$USER_OUT"
    else
        printf '%s\n' "$USER_OUT"
        die "USER_PHASE_FAILED: $AGENT_USER 계정으로 돈 설치 단계가 실패했다 — 위 출력을 고친 뒤 같은 명령을 다시 돌린다"
    fi
    PHASE=root
elif [ "$(id -u)" -eq 0 ]; then
    die "INSTALL_AS_ROOT: 일반 계정으로 돌린다 — 키 · 설정은 Agent 를 돌릴 계정의 것이어야 한다(등록만 /usr/bin/sudo /bin/sh <root 소유 사본> <같은 인자> --register · 런북 --register 절)"
fi

INVITE= NODE_ID= OWNER= GPU_PIN= CPU= RAM_GIB= WS_GIB=
PANEL_PORT=7610 CONFIG_DIR= NODE_DIR= BIN= SHARED= RT= RT_KIND= RT_GPU= RT_GPU_REQUEST= PROBE_IMAGE=
# ★ 결함 444 — GPU 관측은 기본 끔(구버전 Coordinator 는 관측을 실은 인사를 거부한다). Coordinator 를 올린 뒤 --attest-gpus 로 켠다.
CLASSES=TRAINING GPU_MODEL= GPU_VRAM_MIB=0 KEY_PROTECTION=K0 ATTEST=
while [ $# -gt 0 ]; do
    case "$1" in
        --invite) INVITE=$2; shift 2 ;;
        --node-id) NODE_ID=$2; shift 2 ;;
        --owner-member-id) OWNER=$2; shift 2 ;;
        --gpu-pin) GPU_PIN=$2; shift 2 ;;
        --cpu-cores) CPU=$2; shift 2 ;;
        --ram-gib) RAM_GIB=$2; shift 2 ;;
        --workspace-gib) WS_GIB=$2; shift 2 ;;
        --owner-panel-port) PANEL_PORT=$2; shift 2 ;;
        --config-dir) CONFIG_DIR=$2; shift 2 ;;
        --node-dir) NODE_DIR=$2; shift 2 ;;
        --bin) BIN=$2; shift 2 ;;
        --shared-root) SHARED=$2; shift 2 ;;
        --container-runtime) RT=$2; shift 2 ;;
        --container-runtime-kind) RT_KIND=$2; shift 2 ;;
        --container-gpu) RT_GPU=true; shift ;;
        --container-gpu-request) RT_GPU_REQUEST=$2; shift 2 ;;
        --gpu-probe-image) PROBE_IMAGE=$2; shift 2 ;;
        --workload-classes) CLASSES=$2; shift 2 ;;
        --gpu-model) GPU_MODEL=$2; shift 2 ;;
        --gpu-vram-mib) GPU_VRAM_MIB=$2; shift 2 ;;
        --key-protection) KEY_PROTECTION=$2; shift 2 ;;
        --register) shift ;;
        --attest-gpus) ATTEST=true; shift ;;
        *) die "INSTALL_ARGS: 모르는 옵션 $1" ;;
    esac
done

for pair in "invite:$INVITE" "node-id:$NODE_ID" "owner-member-id:$OWNER" "gpu-pin:$GPU_PIN" \
    "cpu-cores:$CPU" "ram-gib:$RAM_GIB" "workspace-gib:$WS_GIB"; do
    [ -n "${pair#*:}" ] || die "INSTALL_ARGS: --${pair%%:*} 는 반드시 준다"
done
echo "$NODE_ID" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$' || die "INSTALL_ARGS: --node-id 는 영문 · 숫자 · . _ - 만(64자 이하) — 받은 값 '$NODE_ID'"
echo "$OWNER" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$' || die "INSTALL_ARGS: --owner-member-id 형식 오류 '$OWNER'"
# ★ 결함 439 — 장치 번호 **하나**. Grant 가 GPU 배정을 싣지 않아 여러 장을 고정하면 한 장만 예약된 작업에 전부 넘어간다 — GPU 마다 노드 하나로 설치한다.
echo "$GPU_PIN" | grep -Eq '^[0-9]+$' || die "INSTALL_ARGS: --gpu-pin 은 장치 번호 하나다(예 0). GPU 가 여러 장이면 GPU 마다 --node-id 를 달리해 따로 설치한다"
for n in "$CPU" "$RAM_GIB" "$WS_GIB" "$PANEL_PORT" "$GPU_VRAM_MIB"; do
    echo "$n" | grep -Eq '^[0-9]+$' || die "INSTALL_ARGS: 숫자가 아니다: $n"
done
[ "$CPU" -gt 0 ] && [ "$RAM_GIB" -gt 0 ] && [ "$WS_GIB" -gt 0 ] || die "INSTALL_ARGS: 자원 수치는 0 보다 커야 한다"
if [ -n "$RT" ]; then
    [ -n "$RT_KIND" ] || die "INSTALL_ARGS: --container-runtime 과 --container-runtime-kind 는 함께 준다"
else
    [ -z "$RT_KIND" ] || die "INSTALL_ARGS: --container-runtime 과 --container-runtime-kind 는 함께 준다"
fi
case "$RT_KIND" in ""|podman|docker) ;; *) die "INSTALL_ARGS: --container-runtime-kind 는 podman 또는 docker" ;; esac
[ -z "$RT_GPU" ] || [ -n "$RT" ] || die "INSTALL_ARGS: --container-gpu 는 --container-runtime 과 함께 준다"
# ★ 결함 469 (재검수 118) — 핀이 필수라, 컨테이너 런타임을 주면 GPU 를 컨테이너에 넘겨야(--container-gpu) 작업을 받는다. 빠뜨리면 모든 작업을
#   CONTAINER_GPU_OFF 로 거부하는 노드가 S3 로 등록됐다.
[ -z "$RT" ] || [ -n "$RT_GPU" ] || die "INSTALL_ARGS: --container-runtime 을 주면 --container-gpu 도 준다(GPU 를 고정한 노드는 컨테이너에 GPU 를 넘겨야 작업을 받는다)"
case "$RT_GPU_REQUEST" in ""|gpus|cdi|cdi-all) ;; *) die "INSTALL_ARGS: --container-gpu-request 는 gpus · cdi · cdi-all" ;; esac
[ -z "$RT_GPU_REQUEST" ] || [ -n "$RT_GPU" ] || die "INSTALL_ARGS: --container-gpu-request 는 --container-gpu 와 함께 준다"
[ -z "$PROBE_IMAGE" ] || [ -n "$RT_GPU" ] || die "INSTALL_ARGS: --gpu-probe-image 는 --container-gpu 와 함께 준다"
case "$KEY_PROTECTION" in K0|K1|K2) ;; *) die "INSTALL_ARGS: --key-protection 은 K0 · K1 · K2" ;; esac

# 초대 파일 — KEY=VALUE 만 읽는다(셸로 실행하지 않는다: source 하면 파일 속 명령이 돈다). root 단계는 부른 계정 권한으로 읽는다(455).
if [ "$PHASE" = root ]; then
    INVITE_TEXT=$(sudo -u "$AGENT_USER" cat -- "$INVITE") || die "INVITE_REJECTED: $INVITE 를 $AGENT_USER 권한으로 읽지 못했다"
else
    INVITE_TEXT=$(cat -- "$INVITE") || die "INVITE_REJECTED: $INVITE 를 읽지 못했다"
fi
invite_value() { printf '%s\n' "$INVITE_TEXT" | sed -n "s/^[[:space:]]*$1=//p" | tail -n 1 | tr -d '\r'; }
[ "$(invite_value GPUTEER_INVITE_VERSION)" = 1 ] || die "INVITE_REJECTED: 초대 판이 1 이 아니거나 없다"
CONNECT=$(invite_value GPUTEER_CONNECT); COORD_ID=$(invite_value GPUTEER_COORDINATOR_ID)
COORD_PUB=$(invite_value GPUTEER_COORDINATOR_PUBKEY); SUB_PUB=$(invite_value GPUTEER_SUBMITTER_PUBKEY)
POOL=$(invite_value GPUTEER_POOL_AGENTS)
[ -n "$SHARED" ] || SHARED=$(invite_value GPUTEER_SHARED_ROOT)
for pair in "GPUTEER_CONNECT:$CONNECT" "GPUTEER_COORDINATOR_ID:$COORD_ID" "GPUTEER_COORDINATOR_PUBKEY:$COORD_PUB" \
    "GPUTEER_SUBMITTER_PUBKEY:$SUB_PUB" "GPUTEER_SHARED_ROOT:$SHARED"; do
    [ -n "${pair#*:}" ] || die "INVITE_REJECTED: 초대 파일에 ${pair%%:*} 가 없다"
done

# 설정 파일 내용 — 사용자 단계는 사용자 폴더의 사본에, root 단계는 /etc 의 인스턴스 파일에 **같은 변수로** 쓴다(root 는 사용자 파일을 읽지 않는다).
emit_common() {
    cat <<EOF
GPUTEER_BIN=$BIN
GPUTEER_CONNECT=$CONNECT
GPUTEER_COORDINATOR_ID=$COORD_ID
GPUTEER_COORDINATOR_PUBKEY=$COORD_PUB
GPUTEER_SUBMITTER_PUBKEY=$SUB_PUB
GPUTEER_SHARED_ROOT=$SHARED
GPUTEER_POOL_AGENTS=$POOL
EOF
}
emit_agent() {
    cat <<EOF
GPUTEER_NODE_ID=$NODE_ID
GPUTEER_NODE_SEED_FILE=$SEED
GPUTEER_NODE_DIR=$NODE_DIR
GPUTEER_GPU_PIN=$GPU_PIN
GPUTEER_OWNER_PANEL_PORT=$PANEL_PORT
GPUTEER_ATTEST_GPUS=$ATTEST
GPUTEER_CONTAINER_RUNTIME=$RT
GPUTEER_CONTAINER_RUNTIME_KIND=$RT_KIND
GPUTEER_CONTAINER_GPU=$RT_GPU
GPUTEER_CONTAINER_GPU_REQUEST=$RT_GPU_REQUEST
EOF
}
STAMP=$(date +%Y-%m-%d_%H%M)
backup_if_exists() {
    if [ -e "$1" ]; then
        dest="$(dirname "$1")/_backup/$STAMP"
        mkdir -p "$dest" && cp -p "$1" "$dest/" && echo "BACKUP $1 -> $dest"
    fi
}

if [ "$PHASE" = root ]; then
    # ── root 단계: /etc 아래만 쓴다 ──────────────────────────────────────────────────────────────
    # 사용자 단계가 끝에 찍은 실제 경로 · 실행 파일을 받는다(root 는 사용자 폴더를 다시 풀지 않는다 — 그 사이 바뀌어도 /etc 에는 문자열만 적힌다).
    user_out_value() { printf '%s\n' "$USER_OUT" | sed -n "s/^$1 //p" | tail -n 1; }
    CONFIG_DIR=$(user_out_value INSTALL_CONFIG_DIR); NODE_DIR=$(user_out_value INSTALL_NODE_DIR)
    BIN=$(user_out_value INSTALL_BIN); SEED=$(user_out_value INSTALL_SEED)
    for pair in "CONFIG_DIR:$CONFIG_DIR" "NODE_DIR:$NODE_DIR" "BIN:$BIN" "SEED:$SEED"; do
        case "${pair#*:}" in /*) ;; *) die "USER_PHASE_FAILED: 사용자 단계가 ${pair%%:*} 를 알려 주지 않았다" ;; esac
    done
    # 시험 전용 — 개발 기계에서 root 단계를 가짜 root 로 돌릴 때만 준다.
    # ★ 결함 470 (재검수 118) — 진짜 systemd 호스트(/etc/systemd/system 이 있다)에서는 받지 않는다. 받으면 설정은 시험 트리에 쓰고 systemctl 은 진짜
    #   시스템을 봐 거짓 REGISTERED 를 찍는다.
    if [ -n "${GPUTEER_TEST_ETC_PREFIX:-}" ] && [ -d /etc/systemd/system ]; then
        die "INSTALL_ARGS: GPUTEER_TEST_ETC_PREFIX 는 시험 전용이다 — systemd 가 있는 기계에서는 받지 않는다"
    fi
    ETC=${GPUTEER_TEST_ETC_PREFIX:-}/etc
    # ★ 결함 464 (재검수 118) — root 가 실행 · 복사하는 것(이 스크립트 · 유닛 틀)과 그 상위 폴더가 전부 **root 소유이고 다른 계정이 쓸 수 없어야**
    #   한다. 사용자 소유 체크아웃에서 sudo 로 부르면, 그 계정으로 도는 작업이 바꿔 둔 스크립트 · 틀을 root 가 실행 · 설치한다.
    #   ★ 이 검사는 스크립트 안에 있다 — **이미 바뀐** 스크립트는 막지 못한다. 막는 것은 사용자 소유 사본에서 부르는 운영자의 실수다.
    root_owned_chain() {
        path=$1
        while :; do
            [ -e "$path" ] || return 1
            owner_mode=$(stat -c '%u %a' "$path") || return 1
            [ "${owner_mode%% *}" = 0 ] || return 1
            mode=${owner_mode#* }
            [ $(( (mode / 10 % 10) & 2 )) -eq 0 ] && [ $(( mode % 10 & 2 )) -eq 0 ] || return 1
            [ "$path" != / ] || return 0
            path=$(dirname "$path")
        done
    }
    UNIT_SOURCE="$(cd -P "$HERE/.." && pwd -P)/gputeer-agent@.service"
    SELF_REAL="$(cd -P "$HERE" && pwd -P)/$(basename "$SELF")"
    if [ -z "${GPUTEER_TEST_ETC_PREFIX:-}" ]; then
        for path in "$SELF_REAL" "$UNIT_SOURCE"; do
            root_owned_chain "$path" || die "INSTALLER_NOT_ROOT_OWNED: $path (또는 그 상위 폴더)가 root 소유가 아니거나 다른 계정이 쓸 수 있다.
  --register 는 root 가 직접 받아 커밋 id 를 대조한 root 소유 사본에서 부른다(사용자 체크아웃을 복사하지 않는다 · 결함 507).
  절차는 런북 docs/runbooks/신뢰망_설치_운영.md 의 --register 절"
        done
    fi
    mkdir -p "$ETC/gputeer"
    # ★ 결함 445 · 453 — 공유 파일을 덮지 않는다. 이 노드의 설정은 **인스턴스 파일**에, 실행 계정은 **인스턴스 drop-in** 에 둔다.
    #   공유 틀은 **없을 때만** 놓고, 있고 다르면 멈춘다(같은 PC 의 다른 노드가 다음 재시작부터 바뀐 틀로 돈다).
    UNIT="$ETC/systemd/system/gputeer-agent@.service"
    if [ -e "$UNIT" ]; then
        cmp -s "$UNIT_SOURCE" "$UNIT" || die "UNIT_DIFFERS: $UNIT 가 이 설치기의 틀과 다르다 — 이 PC 의 다른 노드도 쓰는 틀이라 덮지 않는다.
  비교: diff $UNIT $UNIT_SOURCE — 바꾸려면 운영자가 직접 바꾸고 모든 gputeer-agent@ 를 다시 띄운다"
    fi
    INSTANCE_ENV="$ETC/gputeer/agent-$NODE_ID.env"
    COMMON_ETC="$ETC/gputeer/gputeer.env"
    if [ -e "$COMMON_ETC" ]; then
        # ★ 결함 458 (재검수 117) — 공통 파일이 있으면 그것이 이긴다(인스턴스 파일에 공통 값을 넣지 않는다 — 453). 그러니 그 값이 **초대와 같은지** 본다.
        #   다르거나 없으면 멈춘다 — 점검은 초대의 값으로 통과했는데 등록된 Agent 는 다른 값으로 붙게 된다. 풀 목록만 달라도 된다(공통 파일이 더 새것).
        common_value() { sed -n "s/^[[:space:]]*$1=//p" "$COMMON_ETC" | tail -n 1 | tr -d '\r'; }
        DIFFERS=
        for pair in "GPUTEER_CONNECT:$CONNECT" "GPUTEER_COORDINATOR_ID:$COORD_ID" "GPUTEER_COORDINATOR_PUBKEY:$COORD_PUB" \
            "GPUTEER_SUBMITTER_PUBKEY:$SUB_PUB" "GPUTEER_SHARED_ROOT:$SHARED"; do
            [ "$(common_value "${pair%%:*}")" = "${pair#*:}" ] || DIFFERS="$DIFFERS ${pair%%:*}"
        done
        [ -z "$DIFFERS" ] || die "COMMON_ENV_DIFFERS: $COMMON_ETC 의$DIFFERS 가 초대와 다르거나 없다 — 공통 파일을 초대에 맞추거나 초대를 다시 받는다"
        backup_if_exists "$INSTANCE_ENV"
        { echo "GPUTEER_BIN=$BIN"; emit_agent; } > "$INSTANCE_ENV.tmp"
        POOL_FILE=$COMMON_ETC
    else
        backup_if_exists "$INSTANCE_ENV"
        { emit_common; emit_agent; } > "$INSTANCE_ENV.tmp"
        POOL_FILE=$INSTANCE_ENV
    fi
    chmod 0644 "$INSTANCE_ENV.tmp" && mv "$INSTANCE_ENV.tmp" "$INSTANCE_ENV"
    [ -e "$UNIT" ] || install -m 0644 "$UNIT_SOURCE" "$UNIT"
    DROPIN="$ETC/systemd/system/gputeer-agent@$NODE_ID.service.d"
    mkdir -p "$DROPIN"
    printf '[Service]\nUser=%s\n' "$AGENT_USER" > "$DROPIN/10-user.conf"
    chmod 0644 "$DROPIN/10-user.conf"
    systemctl daemon-reload
    systemctl enable "gputeer-agent@$NODE_ID"
    # ★ 결함 459 (재검수 117) — 이미 돌고 있으면 **다시 띄우지 않는다**(실행 중인 작업을 끊지 않으려고). 바뀐 설정(예 관측 켜기)은 다시 띄워야 적용된다.
    if systemctl is-active --quiet "gputeer-agent@$NODE_ID"; then
        echo "RESTART_NEEDED gputeer-agent@$NODE_ID 가 이미 돈다 — 바뀐 설정은 다시 띄워야 적용된다. 실행 중인 작업이 끝난 뒤: /usr/bin/sudo /usr/bin/systemctl restart gputeer-agent@$NODE_ID"
    else
        systemctl start "gputeer-agent@$NODE_ID"
    fi
    echo "REGISTERED gputeer-agent@$NODE_ID (User=$AGENT_USER)"
    echo "POOL_AGENTS_FILE $POOL_FILE — 풀 노드 목록(GPUTEER_POOL_AGENTS)을 바꿀 때는 이 파일을 고치고 gputeer-agent@$NODE_ID 를 다시 띄운다"
    exit 0
fi

# ── 사용자 단계: Agent 를 돌릴 계정 권한으로만 쓴다 ─────────────────────────────────────────────────
[ -n "$BIN" ] || BIN=$(command -v gputeer || true)
[ -n "$BIN" ] && [ -x "$BIN" ] || die "INSTALL_ARGS: gputeer 실행 파일이 없다 — --bin <경로>(빌드: cargo build --release -p gputeer-cli)"
BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")
[ -n "$CONFIG_DIR" ] || CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/gputeer"
# ★ 결함 438 (재검수 115) — 설치 폴더는 **이 설치기가 쓰는 전용 폴더**만 받는다. 시스템 경로를 거부하고, 이미 있는 폴더는 비었거나 전에 이 설치기가 쓴
#   폴더(표식)여야 한다.
# ★ 결함 446 (재검수 116) — 검사는 **실제 경로**에 한다. 대상 자체가 링크면 거부하고, 상위(반드시 있어야 한다)를 `cd -P` 로 풀어 이름을 붙인다.
MARKER=.gputeer-install-dir
real_install_dir() {
    given=$1
    case "$given" in /*) ;; *) die "INSTALL_DIR_REFUSED: $given — 절대 경로로 준다" ;; esac
    trimmed=$(printf '%s' "$given" | sed 's:/*$::')
    [ -n "$trimmed" ] || die "INSTALL_DIR_REFUSED: / 는 시스템 경로다"
    name=$(basename "$trimmed")
    case "$name" in .|..) die "INSTALL_DIR_REFUSED: $given — 끝 이름이 . · .. 이다" ;; esac
    [ ! -L "$trimmed" ] || die "INSTALL_DIR_REFUSED: $given 는 링크다 — 실제 폴더를 준다"
    parent=$(dirname "$trimmed")
    # ★ 상위 폴더는 만들지 않는다.
    [ -d "$parent" ] || die "INSTALL_DIR_REFUSED: $parent 가 없다 — 먼저 만든다(설치기는 상위 폴더를 만들지 않는다)"
    real_parent=$(cd -P "$parent" && pwd -P) || die "INSTALL_DIR_REFUSED: $parent 를 풀지 못했다"
    printf '%s/%s' "${real_parent%/}" "$name"
}
guard_install_dir() {
    dir=$1
    case "$dir" in
        /bin|/boot|/dev|/etc|/home|/lib|/lib32|/lib64|/media|/mnt|/opt|/proc|/root|/run|/sbin|/srv|/sys|/tmp|/usr|/usr/*|/var|/var/lib|/var/log|/etc/*|/boot/*|/proc/*|/sys/*|/dev/*|/run/*)
            die "INSTALL_DIR_REFUSED: $dir 는 시스템 경로다 — 전용 폴더를 준다(기본 ~/.config/gputeer)" ;;
    esac
    if [ -d "$dir" ] && [ ! -e "$dir/$MARKER" ] && [ -n "$(ls -A "$dir" 2>/dev/null)" ]; then
        die "INSTALL_DIR_REFUSED: $dir 가 비어 있지 않고 이 설치기가 만든 폴더 표식($MARKER)이 없다 — 비어 있는 새 폴더를 준다"
    fi
}
CONFIG_DIR=$(real_install_dir "$CONFIG_DIR")
guard_install_dir "$CONFIG_DIR"
DEFAULT_NODES="$CONFIG_DIR/nodes"
if [ -z "$NODE_DIR" ]; then
    # 기본 노드 폴더 — nodes/ 만 설치기가 만든다. 있으면 링크가 아닌 폴더여야 한다.
    [ ! -L "$DEFAULT_NODES" ] || die "INSTALL_DIR_REFUSED: $DEFAULT_NODES 는 링크다"
    NODE_DIR="$DEFAULT_NODES/$NODE_ID"
    [ ! -L "$NODE_DIR" ] || die "INSTALL_DIR_REFUSED: $NODE_DIR 는 링크다"
else
    NODE_DIR=$(real_install_dir "$NODE_DIR")
fi
guard_install_dir "$NODE_DIR"
SEED="$CONFIG_DIR/$NODE_ID.seed"
COMMON="$CONFIG_DIR/gputeer.env"; AGENT_ENV="$CONFIG_DIR/agent-$NODE_ID.env"; JOIN="$CONFIG_DIR/join-$NODE_ID.json"
# ★ 결함 457 (재검수 117) — 다른 계정이 쓰던 노드를 이 계정으로 조용히 이어 쓰지 않는다. 설정 폴더 · 쓰는 파일 · 노드 폴더 안 전부가 이 계정의 것이어야 한다
#   (전에는 root 가 노드 폴더만 넘겨, 옛 계정이 만든 fence DB · 체크포인트를 새 계정이 못 열었다). 계정을 바꾸려면 새 노드 id · 새 폴더로 설치한다.
ME=$(id -u)
for f in "$CONFIG_DIR" "$SEED" "$COMMON" "$AGENT_ENV" "$JOIN" "$CONFIG_DIR/$MARKER" "$DEFAULT_NODES" "$NODE_DIR"; do
    [ ! -L "$f" ] || die "INSTALL_DIR_REFUSED: $f 가 링크다 — 지우고 다시 돌린다"
    [ ! -e "$f" ] || [ -O "$f" ] || die "NODE_OWNED_BY_OTHER: $f 가 이 계정($(id -un))의 것이 아니다 — 계정을 바꾸려면 새 노드 id · 새 폴더로 설치한다"
done
if [ -d "$NODE_DIR" ]; then
    OTHER=$(find "$NODE_DIR" ! -user "$ME" -print 2>/dev/null | head -n 1)
    [ -z "$OTHER" ] || die "NODE_OWNED_BY_OTHER: $OTHER 가 이 계정의 것이 아니다 — 이 노드는 다른 계정이 쓰던 것이다. 새 노드 id · 새 폴더로 설치한다"
fi
umask 077
mkdir -p "$CONFIG_DIR"
[ "$(dirname "$NODE_DIR")" != "$DEFAULT_NODES" ] || mkdir -p "$DEFAULT_NODES"
mkdir -p "$NODE_DIR"
touch "$CONFIG_DIR/$MARKER" "$NODE_DIR/$MARKER"

# 1. 키.
if [ -e "$SEED" ]; then echo "SEED_KEPT $SEED"; else "$BIN" keygen --out "$SEED"; fi

# 2. 설정.
backup_if_exists "$COMMON"; backup_if_exists "$AGENT_ENV"
{ echo "# install-node.sh 가 $STAMP 에 썼다. 저장소에 넣지 않는다."; emit_common; } > "$COMMON"
emit_agent > "$AGENT_ENV"
echo "CONFIG_WRITTEN $COMMON $AGENT_ENV"

# 3. 점검.
set -- node-doctor --seed-file "$SEED" --node-dir "$NODE_DIR" --connect "$CONNECT" \
    --shared-checkpoint-root "$SHARED" --owner-panel-port "$PANEL_PORT" --gpu-pin "$GPU_PIN"
[ -z "$RT" ] || set -- "$@" --container-runtime "$RT" --container-runtime-kind "$RT_KIND"
[ -z "$RT_GPU_REQUEST" ] || set -- "$@" --container-gpu-request "$RT_GPU_REQUEST"
[ -z "$PROBE_IMAGE" ] || set -- "$@" --container-gpu-probe-image "$PROBE_IMAGE"
REPORT=$("$BIN" "$@" 2>&1) && DOCTOR_OK=1 || DOCTOR_OK=
echo "$REPORT"
[ -n "$DOCTOR_OK" ] || die "NODE_DOCTOR_FAILED: 위 FAIL 을 고치고 같은 명령을 다시 돌린다(키 · 설정은 그대로 이어 쓴다)"
PUBLIC_KEY=$(echo "$REPORT" | sed -n 's/^CHECK seed .*\([0-9a-f]\{64\}\).*$/\1/p' | head -n 1)
[ -n "$PUBLIC_KEY" ] || die "node-doctor 출력에서 공개키를 읽지 못했다"

# 4. 가입 파일 — GPU 는 nvidia-smi 로 읽는다(못 읽으면 --gpu-model · --gpu-vram-mib. 지어내지 않는다).
json_escape() { printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'; }
model=$GPU_MODEL; vram=$GPU_VRAM_MIB
if [ -z "$model" ] || [ "$vram" -le 0 ]; then
    command -v nvidia-smi >/dev/null || die "GPU_UNKNOWN: nvidia-smi 가 없다 — --gpu-model · --gpu-vram-mib 로 직접 준다"
    row=$(nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits -i "$GPU_PIN") || die "GPU_UNKNOWN: GPU $GPU_PIN 을 읽지 못했다"
    [ -n "$model" ] || model=$(echo "$row" | cut -d, -f1 | sed 's/^ *//; s/ *$//')
    [ "$vram" -gt 0 ] || vram=$(echo "$row" | cut -d, -f2 | tr -d ' ')
fi
echo "$vram" | grep -Eq '^[0-9]+$' || die "GPU_UNKNOWN: VRAM 값이 숫자가 아니다($vram)"
GPUS="{ \"gpu_id\": \"$NODE_ID-gpu-$GPU_PIN\", \"model\": \"$(json_escape "$model")\", \"healthy\": true, \"available_vram_bytes\": $((vram * 1048576)) }"
if [ -n "$RT" ]; then TIER=S3; ISOLATION=CONTAINED; else TIER=S0; ISOLATION=RESTRICTED; fi
CLASSES_JSON=$(echo "$CLASSES" | tr ',' '\n' | sed 's/^ *//; s/ *$//; s/.*/"&"/' | paste -sd, -)
NOW_MS=$(($(date +%s) * 1000))
backup_if_exists "$JOIN"
cat > "$JOIN" <<EOF
{
  "schema_version": 1,
  "agents": [
    {
      "registry": {
        "node_id": "$NODE_ID", "device_id": "$NODE_ID", "owner_member_id": "$OWNER",
        "verifying_key_hex": "$PUBLIC_KEY", "node_state": "ONLINE", "risk_state": "NORMAL",
        "security_tier": "$TIER", "isolation_class": "$ISOLATION", "key_protection": "$KEY_PROTECTION"
      },
      "inventory": {
        "inventory_revision": $NOW_MS, "observed_at_unix_ms": $NOW_MS,
        "gpus": [ $GPUS ],
        "available_cpu_cores": $CPU, "available_ram_bytes": $((RAM_GIB * 1073741824)),
        "available_workspace_bytes": $((WS_GIB * 1073741824)),
        "allowed_workload_classes": [ $CLASSES_JSON ],
        "third_party_workloads_opt_in": true
      }
    }
  ]
}
EOF
echo "JOIN_FILE $JOIN"
echo "PUBLIC_KEY $PUBLIC_KEY"

if [ "$PHASE" = user ]; then
    # root 단계에 넘길 값 — 마지막 줄들이다(root 단계는 같은 이름의 마지막 줄을 읽는다).
    echo "INSTALL_CONFIG_DIR $CONFIG_DIR"
    echo "INSTALL_NODE_DIR $NODE_DIR"
    echo "INSTALL_SEED $SEED"
    echo "INSTALL_BIN $BIN"
else
    echo "NOT_REGISTERED — 등록하려면 root 가 직접 받아 대조한 root 소유 사본에서: /usr/bin/sudo /bin/sh <그 사본의 install-node.sh> <같은 인자> --register (런북 --register 절 · 결함 507 · 509)"
fi
echo "NEXT 운영자에게 $JOIN 를 보낸다(비밀 없음). 운영자가 admit-node 로 받은 뒤 Agent 가 일을 받는다."
