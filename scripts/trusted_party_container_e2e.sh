#!/bin/sh
# gPUteer — 실제 컨테이너 런타임으로 신뢰망 풀 한 회차 끝에서 끝까지(2026-09-29 x600 WSL · podman 5.7.0 에서 처음 통과).
#   사용: sh scripts/trusted_party_container_e2e.sh <gputeer 실행 파일> [podman|docker] [작업 폴더 부모]
#   키 셋 · alpine:3.20(digest 고정) · 노드 등록 · 제출자 등록 · 제출 · 예약 · 풀 Coordinator · agent-loop 한 회차(원장 켬 · 컨테이너만).
#   만드는 컨테이너는 Agent 가 붙이는 gputeer- 접두어 이름뿐이다. 다른 컨테이너 · 이미지는 건드리지 않는다(alpine 을 받기만 한다).
set -u
BIN=${1:?gputeer 실행 파일 경로}
KIND=${2:-podman}
RUNTIME=$(command -v "$KIND") || { echo "런타임 $KIND 가 없다"; exit 2; }
W=${3:-$HOME/gputeer-e2e}/run-$(date +%H%M%S)
mkdir -p "$W" && cd "$W" || exit 2
echo "WORKDIR=$W KIND=$KIND"
NODE=01JE2EPODMANNODE0000001
COORD=01JE2EPODMANCOORD000001
SUB=01JE2EPODMANSUBMIT00001
JOB=01JE2EPODMANJOB00000001

"$BIN" keygen --out coord.seed > coord.out || exit 3
"$BIN" keygen --out node.seed > node.out || exit 3
"$BIN" keygen --out sub.seed > sub.out || exit 3
CPUB=$(sed -n 's/^PUBLIC_KEY //p' coord.out)
NPUB=$(sed -n 's/^PUBLIC_KEY //p' node.out)
SPUB=$(sed -n 's/^PUBLIC_KEY //p' sub.out)
SSEED=$(cat sub.seed)

"$RUNTIME" pull -q docker.io/library/alpine:3.20 > /dev/null || exit 4
DIGEST=$("$RUNTIME" image inspect docker.io/library/alpine:3.20 --format '{{.Digest}}' | sed 's/^sha256://')
echo "IMAGE_DIGEST=$DIGEST"

NOW=$(( $(date +%s) * 1000 ))
cat > bootstrap.json <<EOF
{
  "schema_version": 1,
  "agents": [{
    "registry": {
      "node_id": "$NODE", "device_id": "$NODE",
      "owner_member_id": "owner-e2e", "verifying_key_hex": "$NPUB",
      "node_state": "ONLINE", "risk_state": "NORMAL",
      "security_tier": "S2", "isolation_class": "CONTAINED",
      "key_protection": "K1"
    },
    "inventory": {
      "inventory_revision": 1, "observed_at_unix_ms": $NOW,
      "gpus": [{ "gpu_id": "$NODE-gpu-0", "model": "RTX 4070 SUPER",
                 "healthy": true, "available_vram_bytes": 12884901888 }],
      "available_cpu_cores": 16, "available_ram_bytes": 34359738368,
      "available_workspace_bytes": 107374182400,
      "allowed_workload_classes": ["TRAINING"],
      "third_party_workloads_opt_in": true
    }
  }]
}
EOF
P="--i-understand-plaintext-keyring-is-unsafe true"
"$BIN" import-inventory --inventory bootstrap.json --inventory-db control.sqlite3 || exit 5
"$BIN" submitter-add --keyring subs.keyring --submitter-id "$SUB" --public-key "$SPUB" $P || exit 6
ISSUED=$((NOW - 60000)); EXPIRES=$((NOW + 604800000))
"$BIN" submit --job-id "$JOB" --entrypoint /bin/echo --args hello-from-podman \
  --image-ref docker.io/library/alpine --image-sha256 "$DIGEST" \
  --submitter-device-id "$SUB" --submitter-seed "$SSEED" \
  --issued-at-unix-ms "$ISSUED" --expires-at-unix-ms "$EXPIRES" --out job.pb \
  --workload-class TRAINING --side-effect-class PURE --durability LOCAL --dataset-sensitivity INTERNAL \
  --minimum-security-tier S2 --minimum-isolation-class CONTAINED --minimum-key-protection K1 \
  --gpu-count 1 --gpu-min-vram-bytes 8589934592 --cpu-cores 4 --ram-bytes 8589934592 --workspace-bytes 10737418240 || exit 7
"$BIN" import-manifest --manifest job.pb --submitter-keyring subs.keyring --job-db control.sqlite3 --idempotency-key 0f0102030405060708090a0b0c0d0e0f $P || exit 8
"$BIN" plan-job --job-id "$JOB" --control-db control.sqlite3 --submitter-keyring subs.keyring --submitter-member owner-e2e --max-snapshot-age-ms 86400000 $P || exit 9
"$BIN" scheduler-tick --control-db control.sqlite3 --submitter-keyring subs.keyring --submitter-member owner-e2e --max-snapshot-age-ms 86400000 \
  --best-fit-axes vram,gpu_count,cpu,ram,workspace --coordinator-id "$COORD" --coordinator-term 3 --lease-ttl-ms 600000 \
  --lease-renew-after-ms 1000 --lease-max-total-duration-seconds 86400 $P || exit 10

"$BIN" coordinator-stub --pool-mode true --pool-agents "$NODE=$NPUB" --listen 127.0.0.1:0 --own-seed-file coord.seed \
  --coordinator-device-id "$COORD" --grant-from-control-db control.sqlite3 --lease-db control.sqlite3 --liveness-db control.sqlite3 \
  --submitter-keyring subs.keyring $P --accept-report-sessions true --release-on-exit-report true \
  --max-connections 0 --accept-timeout-ms 0 > coord.log 2>&1 &
CPID=$!
ADDR=""
for i in $(seq 1 100); do ADDR=$(sed -n 's/^READY //p' coord.log | head -1); [ -n "$ADDR" ] && break; sleep 0.1; done
echo "COORDINATOR=$ADDR"

"$BIN" agent-loop --interval-ms 100 --max-rounds 1 -- --connect "$ADDR" --own-seed-file node.seed --peer-pubkey "$CPUB" \
  --coordinator-device-id "$COORD" --agent-device-id "$NODE" --fence-db fence.sqlite3 --checkpoint-root "$W/ck" \
  --submitter-pubkey "$SPUB" --i-understand-this-executes-untrusted-code true --report-over-session true \
  --max-reconnect-attempts 1 --require-ack-receipt true --renew-during-execution-ms 1000 \
  --container-runtime "$RUNTIME" --container-runtime-kind "$KIND" --container-only true --run-ledger true > agent.log 2>&1
echo "AGENT_EXIT=$?"
sleep 2
kill $CPID 2>/dev/null
echo "--- agent.log (끝 40줄)"; tail -40 agent.log
echo "--- status"; "$BIN" status --control-db control.sqlite3 2>&1 | head -20
echo "--- gputeer- 컨테이너 남은 것"; "$RUNTIME" ps -a --filter name=gputeer- --format '{{.Names}} {{.Status}}'
