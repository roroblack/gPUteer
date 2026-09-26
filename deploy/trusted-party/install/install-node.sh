#!/bin/sh
# gPUteer 노드 설치 (팀원 PC · Linux) — install-node.ps1 과 같은 일을 한다. 설명은 그 파일 머리말.
#
# 예:
#   sh install-node.sh --invite ./invite.env --node-id bob-gpu0 --owner-member-id bob --gpu-pin 0 \
#     --cpu-cores 8 --ram-gib 16 --workspace-gib 100 \
#     --container-runtime "$(command -v podman)" --container-runtime-kind podman --container-gpu
#
# --register 는 root 로 돌린다(sudo) — /etc/gputeer 에 설정을 두고 gputeer-agent@<노드> 유닛을 켠다.
#   Agent 는 root 로 돌지 않는다: 유닛의 User= 를 sudo 를 부른 계정으로 채운다(결함 280).
# ★ 쓰는 파일은 전부 저장소 밖(기본 ~/.config/gputeer)이다. 자원 수치는 소유자가 내놓는 양이다 — 기본값을 두지 않는다.
set -eu

die() { echo "$*" >&2; exit 1; }

INVITE= NODE_ID= OWNER= GPU_PIN= CPU= RAM_GIB= WS_GIB=
PANEL_PORT=7610 CONFIG_DIR= NODE_DIR= BIN= SHARED= RT= RT_KIND= RT_GPU= RT_GPU_REQUEST= PROBE_IMAGE=
# ★ 결함 444 — GPU 관측은 기본 끔(구버전 Coordinator 는 관측을 실은 인사를 거부한다). Coordinator 를 올린 뒤 --attest-gpus 로 켠다.
CLASSES=TRAINING GPU_MODEL= GPU_VRAM_MIB=0 KEY_PROTECTION=K0 REGISTER= ATTEST=
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
        --register) REGISTER=1; shift ;;
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
case "$RT_GPU_REQUEST" in ""|gpus|cdi|cdi-all) ;; *) die "INSTALL_ARGS: --container-gpu-request 는 gpus · cdi · cdi-all" ;; esac
[ -z "$RT_GPU_REQUEST" ] || [ -n "$RT_GPU" ] || die "INSTALL_ARGS: --container-gpu-request 는 --container-gpu 와 함께 준다"
[ -z "$PROBE_IMAGE" ] || [ -n "$RT_GPU" ] || die "INSTALL_ARGS: --gpu-probe-image 는 --container-gpu 와 함께 준다"
case "$KEY_PROTECTION" in K0|K1|K2) ;; *) die "INSTALL_ARGS: --key-protection 은 K0 · K1 · K2" ;; esac

# 초대 파일 — KEY=VALUE 만 읽는다(셸로 실행하지 않는다: source 하면 파일 속 명령이 돈다).
invite_value() { sed -n "s/^[[:space:]]*$1=//p" "$INVITE" | tail -n 1 | tr -d '\r'; }
[ "$(invite_value GPUTEER_INVITE_VERSION)" = 1 ] || die "INVITE_REJECTED: 초대 판이 1 이 아니거나 없다"
CONNECT=$(invite_value GPUTEER_CONNECT); COORD_ID=$(invite_value GPUTEER_COORDINATOR_ID)
COORD_PUB=$(invite_value GPUTEER_COORDINATOR_PUBKEY); SUB_PUB=$(invite_value GPUTEER_SUBMITTER_PUBKEY)
POOL=$(invite_value GPUTEER_POOL_AGENTS)
[ -n "$SHARED" ] || SHARED=$(invite_value GPUTEER_SHARED_ROOT)
for pair in "GPUTEER_CONNECT:$CONNECT" "GPUTEER_COORDINATOR_ID:$COORD_ID" "GPUTEER_COORDINATOR_PUBKEY:$COORD_PUB" \
    "GPUTEER_SUBMITTER_PUBKEY:$SUB_PUB" "GPUTEER_SHARED_ROOT:$SHARED"; do
    [ -n "${pair#*:}" ] || die "INVITE_REJECTED: 초대 파일에 ${pair%%:*} 가 없다"
done

[ -n "$BIN" ] || BIN=$(command -v gputeer || true)
[ -n "$BIN" ] && [ -x "$BIN" ] || die "INSTALL_ARGS: gputeer 실행 파일이 없다 — --bin <경로>(빌드: cargo build --release -p gputeer-cli)"
BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")
if [ -z "$CONFIG_DIR" ]; then
    # ★ sudo 로 부르면 HOME 이 root 의 것일 수 있다 — 그러면 root 폴더에 **새 키**를 만든다(신원이 바뀐다). 부른 계정의 집을 쓴다.
    if [ "$(id -u)" -eq 0 ] && [ -n "${SUDO_USER:-}" ]; then
        USER_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
        CONFIG_DIR="$USER_HOME/.config/gputeer"
    else
        CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/gputeer"
    fi
fi
[ -n "$NODE_DIR" ] || NODE_DIR="$CONFIG_DIR/nodes/$NODE_ID"

# ★ 결함 438 (재검수 115) — 설치 폴더는 **이 설치기가 쓰는 전용 폴더**만 받는다. 전에는 아무 경로나 받아 --register 끝에 root 로 chown -R 했다
#   (--config-dir /etc 면 /etc 전체가 일반 사용자 소유가 됐다). 시스템 경로를 거부하고, 이미 있는 폴더는 비었거나 전에 이 설치기가 쓴 폴더(표식)여야 한다.
MARKER=.gputeer-install-dir
guard_install_dir() {
    dir=$1
    case "$dir" in /*) ;; *) die "INSTALL_DIR_REFUSED: $dir — 절대 경로로 준다" ;; esac
    clean=$(printf '%s' "$dir" | sed 's:/*$::'); [ -n "$clean" ] || clean=/
    case "$clean" in
        /|/bin|/boot|/dev|/etc|/home|/lib|/lib32|/lib64|/media|/mnt|/opt|/proc|/root|/run|/sbin|/srv|/sys|/tmp|/usr|/usr/*|/var|/var/lib|/var/log|/etc/*|/boot/*|/proc/*|/sys/*|/dev/*)
            die "INSTALL_DIR_REFUSED: $dir 는 시스템 경로다 — 전용 폴더를 준다(기본 ~/.config/gputeer)" ;;
    esac
    if [ -d "$dir" ] && [ ! -e "$dir/$MARKER" ] && [ -n "$(ls -A "$dir" 2>/dev/null)" ]; then
        die "INSTALL_DIR_REFUSED: $dir 가 비어 있지 않고 이 설치기가 만든 폴더 표식($MARKER)이 없다 — 비어 있는 새 폴더를 준다"
    fi
}
guard_install_dir "$CONFIG_DIR"
guard_install_dir "$NODE_DIR"
# ★ 상위 폴더는 만들지 않는다 — sudo 로 부르면 mkdir -p 가 ~/.config 같은 상위까지 root 소유 0700 으로 만든다. 기본 노드 폴더의 nodes/ 만 예외다.
[ -d "$(dirname "$CONFIG_DIR")" ] || die "INSTALL_DIR_REFUSED: $(dirname "$CONFIG_DIR") 가 없다 — 먼저 만든다(설치기는 상위 폴더를 만들지 않는다)"
DEFAULT_NODES="$CONFIG_DIR/nodes"
if [ "$(dirname "$NODE_DIR")" != "$DEFAULT_NODES" ]; then
    [ -d "$(dirname "$NODE_DIR")" ] || die "INSTALL_DIR_REFUSED: $(dirname "$NODE_DIR") 가 없다 — 먼저 만든다"
fi
umask 077
mkdir -p "$CONFIG_DIR"
[ "$(dirname "$NODE_DIR")" != "$DEFAULT_NODES" ] || mkdir -p "$DEFAULT_NODES"
mkdir -p "$NODE_DIR"
touch "$CONFIG_DIR/$MARKER" "$NODE_DIR/$MARKER"
STAMP=$(date +%Y-%m-%d_%H%M)

backup_if_exists() {
    if [ -e "$1" ]; then
        dest="$(dirname "$1")/_backup/$STAMP"
        mkdir -p "$dest" && cp -p "$1" "$dest/" && echo "BACKUP $1 -> $dest"
    fi
}

# 1. 키.
SEED="$CONFIG_DIR/$NODE_ID.seed"
if [ -e "$SEED" ]; then echo "SEED_KEPT $SEED"; else "$BIN" keygen --out "$SEED"; fi

# 2. 설정.
COMMON="$CONFIG_DIR/gputeer.env"; AGENT_ENV="$CONFIG_DIR/agent-$NODE_ID.env"
backup_if_exists "$COMMON"; backup_if_exists "$AGENT_ENV"
cat > "$COMMON" <<EOF
# install-node.sh 가 $STAMP 에 썼다. 저장소에 넣지 않는다.
GPUTEER_BIN=$BIN
GPUTEER_CONNECT=$CONNECT
GPUTEER_COORDINATOR_ID=$COORD_ID
GPUTEER_COORDINATOR_PUBKEY=$COORD_PUB
GPUTEER_SUBMITTER_PUBKEY=$SUB_PUB
GPUTEER_SHARED_ROOT=$SHARED
GPUTEER_POOL_AGENTS=$POOL
EOF
cat > "$AGENT_ENV" <<EOF
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
GPUS=
for index in $(echo "$GPU_PIN" | tr ',' ' '); do
    model=$GPU_MODEL; vram=$GPU_VRAM_MIB
    if [ -z "$model" ] || [ "$vram" -le 0 ]; then
        command -v nvidia-smi >/dev/null || die "GPU_UNKNOWN: nvidia-smi 가 없다 — --gpu-model · --gpu-vram-mib 로 직접 준다"
        row=$(nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits -i "$index") || die "GPU_UNKNOWN: GPU $index 를 읽지 못했다"
        [ -n "$model" ] || model=$(echo "$row" | cut -d, -f1 | sed 's/^ *//; s/ *$//')
        [ "$vram" -gt 0 ] || vram=$(echo "$row" | cut -d, -f2 | tr -d ' ')
    fi
    echo "$vram" | grep -Eq '^[0-9]+$' || die "GPU_UNKNOWN: VRAM 값이 숫자가 아니다($vram)"
    [ -z "$GPUS" ] || GPUS="$GPUS, "
    GPUS="$GPUS{ \"gpu_id\": \"$NODE_ID-gpu-$index\", \"model\": \"$(json_escape "$model")\", \"healthy\": true, \"available_vram_bytes\": $((vram * 1048576)) }"
done
if [ -n "$RT" ]; then TIER=S3; ISOLATION=CONTAINED; else TIER=S0; ISOLATION=RESTRICTED; fi
CLASSES_JSON=$(echo "$CLASSES" | tr ',' '\n' | sed 's/^ *//; s/ *$//; s/.*/"&"/' | paste -sd, -)
NOW_MS=$(($(date +%s) * 1000))
JOIN="$CONFIG_DIR/join-$NODE_ID.json"
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

# 5. 등록.
HERE=$(cd "$(dirname "$0")" && pwd)
if [ -n "$REGISTER" ]; then
    [ "$(id -u)" -eq 0 ] || die "REGISTER: root 로 돌린다(sudo)"
    AGENT_USER=${SUDO_USER:-}
    [ -n "$AGENT_USER" ] && [ "$AGENT_USER" != root ] || die "REGISTER: sudo 로 부른다 — Agent 를 돌릴 계정(root 가 아닌)을 SUDO_USER 로 안다"
    mkdir -p /etc/gputeer
    # ★ 결함 445 (재검수 115) — 공유 파일을 덮지 않는다. 이 노드의 설정은 **인스턴스 파일 하나**에(공통 값 + 노드 값), 실행 계정은 **인스턴스 drop-in** 에 둔다.
    #   전에는 공유 틀의 User= 와 /etc/gputeer/gputeer.env 를 설치마다 덮어, 같은 PC 의 앞 노드가 뒤 사용자로 실행돼 자기 시드를 못 읽었다
    #   (운영자 PC 라면 운영자의 gputeer.env 까지 덮었다).
    INSTANCE_ENV="/etc/gputeer/agent-$NODE_ID.env"
    backup_if_exists "$INSTANCE_ENV"
    { cat "$COMMON"; cat "$AGENT_ENV"; } > "$INSTANCE_ENV.tmp" && chmod 0644 "$INSTANCE_ENV.tmp" && mv "$INSTANCE_ENV.tmp" "$INSTANCE_ENV"
    UNIT=/etc/systemd/system/gputeer-agent@.service
    if ! cmp -s "$HERE/../gputeer-agent@.service" "$UNIT" 2>/dev/null; then
        backup_if_exists "$UNIT"
        install -m 0644 "$HERE/../gputeer-agent@.service" "$UNIT"
    fi
    DROPIN="/etc/systemd/system/gputeer-agent@$NODE_ID.service.d"
    mkdir -p "$DROPIN"
    printf '[Service]\nUser=%s\n' "$AGENT_USER" > "$DROPIN/10-user.conf"
    chmod 0644 "$DROPIN/10-user.conf"
    # ★ 결함 438 — 설치기가 쓴 것만 넘긴다(폴더 전체를 재귀로 넘기지 않는다). 노드 폴더는 Agent 가 쓰는 곳이라 재귀로 넘긴다(위에서 전용 폴더로 확인했다).
    #   -h: 링크면 링크 자체만 바꾼다(가리키는 대상의 소유를 넘기지 않는다).
    chown -h "$AGENT_USER" "$CONFIG_DIR" "$CONFIG_DIR/$MARKER" "$SEED" "$COMMON" "$AGENT_ENV" "$JOIN"
    [ "$(dirname "$NODE_DIR")" != "$DEFAULT_NODES" ] || chown -h "$AGENT_USER" "$DEFAULT_NODES"
    [ ! -d "$CONFIG_DIR/_backup" ] || chown -hR "$AGENT_USER" "$CONFIG_DIR/_backup"
    chown -hR "$AGENT_USER" "$NODE_DIR"
    systemctl daemon-reload
    systemctl enable --now "gputeer-agent@$NODE_ID"
    echo "REGISTERED gputeer-agent@$NODE_ID (User=$AGENT_USER)"
else
    echo "NOT_REGISTERED — 등록하려면: sudo sh $0 <같은 인자> --register"
fi
echo "NEXT 운영자에게 $JOIN 를 보낸다(비밀 없음). 운영자가 admit-node 로 받은 뒤 Agent 가 일을 받는다."
