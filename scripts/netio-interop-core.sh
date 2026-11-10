#!/usr/bin/env bash
# The counter's network driver against Bitcoin Core, on regtest.
#
# Question: does `openbmt-net-io` connect to Bitcoin Core, sync headers,
# report a payment seen before it is mined, hand over the block that holds
# it, follow a reorganization, and start again from the stored headers
# without downloading them again?
#
# What it does:
#   1. Starts a private regtest node that serves compact filters, and mines
#      150 blocks.
#   2. Starts the driver (example `regtest_node`) with one fixed peer, the
#      node, and one watched script taken from a fresh node address.
#      Checks the handshake and the first `Peers` event.
#   3. All 150 headers arrive as `Chain` events.
#   4. A payment to the watched address is a `Sighting` with its txid.
#   5. A block that holds it comes as a `Block` (one peer is fewer than two
#      filter peers, so the block is fetched in full).
#   6. `invalidateblock` on the tip and two new blocks: a `Chain` event with
#      a block that left.
#   7. The driver is stopped, one more block is mined, and the driver is
#      started again over the same data directory: it starts at the stored
#      tip and the node sends it about one header, not the whole chain.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another), jq, and
# cargo unless NODE_BIN names a prebuilt `regtest_node` example binary.
#
# Environment:
#   CORE_BIN        directory with bitcoind and bitcoin-cli
#   DRILL_DIR       scratch directory (default: a new directory under
#                   ~/.cache/openbmt)
#   DRILL_RPC_PORT  RPC port for the regtest node (default 18956)
#   DRILL_P2P_PORT  peer port for the regtest node (default 18957)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#   NODE_BIN        the `regtest_node` example binary
#
# Everything here is regtest on 127.0.0.1. No real keys or funds are
# involved. The node and the scratch directory are removed on every exit.

set -euo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_RPC_PORT:-18956}
P2P_PORT=${DRILL_P2P_PORT:-18957}
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/netio-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/node"
STORE_DIR="$BASE/driver"
FIFO="$BASE/stdin"
BLOCKS=150

say() { printf '\n== %s\n' "$*"; }
ok() { printf 'PASS  %s\n' "$*"; }
fail() {
  printf 'FAIL  %s\n' "$*" >&2
  exit 1
}
cli() { bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" "$@"; }
show() { sed 's/^/      /'; }

for tool in bitcoind bitcoin-cli jq; do
  command -v "$tool" >/dev/null || fail "$tool is not on PATH"
done
[ "$(command -v bitcoind)" = "$CORE_BIN/bitcoind" ] || fail "bitcoind is not the one in $CORE_BIN"
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) fail "this check is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

mkdir -p "$NODE_DIR" "$STORE_DIR"
NODE_PID=
DRIVER_PID=
cleanup() {
  exec 3>&- || true
  if [ -n "$DRIVER_PID" ] && kill -0 "$DRIVER_PID" 2>/dev/null; then
    for _ in $(seq 1 20); do
      kill -0 "$DRIVER_PID" 2>/dev/null || break
      sleep 0.5
    done
    kill -9 "$DRIVER_PID" 2>/dev/null || true
  fi
  if [ -n "$NODE_PID" ]; then
    bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" stop >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$NODE_PID" 2>/dev/null || true
  fi
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

if [ -z "${NODE_BIN:-}" ]; then
  say "Build the driver example"
  cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
    -p openbmt-net-io --example regtest_node
  NODE_BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/examples/regtest_node"
fi
[ -x "$NODE_BIN" ] || fail "no driver binary at $NODE_BIN"

# Start the driver. Sets DRIVER_PID and LOG; its standard input is a fifo
# that stays open on descriptor 3 so that `quit` can be sent.
start_driver() {
  LOG="$BASE/driver-$1.log"
  : >"$LOG"
  [ -p "$FIFO" ] || mkfifo "$FIFO"
  exec 3<>"$FIFO"
  "$NODE_BIN" "$P2P_PORT" "$STORE_DIR" "$BLOCKS" "$SCRIPT" <&3 >"$LOG" 2>&1 &
  DRIVER_PID=$!
}
stop_driver() {
  echo quit >&3
  await "^stopped\$" 30 || fail "the driver did not stop"
  wait "$DRIVER_PID" || fail "the driver ended with an error"
  DRIVER_PID=
  exec 3>&-
}
# Wait for a line of the current log that matches the pattern.
await() {
  local pattern=$1 secs=$2 i
  for i in $(seq 1 $((secs * 4))); do
    grep -Eq -- "$pattern" "$LOG" && return 0
    if [ -n "$DRIVER_PID" ] && ! kill -0 "$DRIVER_PID" 2>/dev/null; then break; fi
    sleep 0.25
  done
  grep -Eq -- "$pattern" "$LOG"
}
# The sum of `joined=` over the chain lines of the current log.
joined_sum() { awk '/^chain /{ split($2, a, "="); s += a[2] } END { print s + 0 }' "$LOG"; }
# The `tip=` of the last peers line.
last_tip() { grep '^peers ' "$LOG" | tail -1 | sed -E 's/.* tip=([0-9]+).*/\1/'; }
# Bytes of `headers` messages the node has sent to its inbound peer.
headers_bytes() {
  cli getpeerinfo | jq '[.[] | select(.inbound == true)] | (.[0].bytessent_per_msg.headers // 0)'
}

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" \
  -listen=1 -bind="127.0.0.1:$P2P_PORT" -listenonion=0 -daemonwait \
  -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
  -fallbackfee=0.0001 -dbcache=50 >/dev/null
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")

say "1. A node that serves filters, and $BLOCKS blocks"
cli -named createwallet wallet_name=miner >/dev/null
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress "$BLOCKS" "$MINE" >/dev/null
[ "$(cli getblockcount)" = "$BLOCKS" ] || fail "the node is not at height $BLOCKS"
ADDR=$(cli -rpcwallet=miner getnewaddress)
SCRIPT=$(cli -rpcwallet=miner getaddressinfo "$ADDR" | jq -r '.scriptPubKey')
ok "height $BLOCKS; watching $ADDR (script $SCRIPT)"

say "2. Handshake and the first Peers event"
start_driver first
await "^peers 0 .* tip=0 " 10 || fail "no first Peers event at the genesis tip"
await "^peers 1 filters=1 " 30 || fail "no Peers event with one ready peer that offers filters"
ok "start at the genesis tip, then a ready peer with compact filters: $(grep -E '^peers 1 ' "$LOG" | head -1)"

say "3. Headers"
await "tip=$BLOCKS " 30 || fail "the tip did not reach $BLOCKS"
[ "$(joined_sum)" = "$BLOCKS" ] || fail "the chain events add up to $(joined_sum), not $BLOCKS"
! grep -q '^block ' "$LOG" || fail "blocks at or below the watch height were downloaded"
ok "$BLOCKS headers joined; no block at or below height $BLOCKS was downloaded"
HEADERS_FIRST=$(headers_bytes)
echo "      the node sent $HEADERS_FIRST bytes of headers messages to the driver"

say "4. A payment before it is mined"
TXID=$(cli -rpcwallet=miner sendtoaddress "$ADDR" 0.001)
await "^sighting $TXID from=1 pays 100000\$" 30 || fail "no sighting of $TXID"
ok "sighting of $TXID, 100000 sat, from 1 peer"

say "5. The block that holds it"
cli generatetoaddress 1 "$MINE" >/dev/null
BLOCK=$(cli getbestblockhash)
await "^block $BLOCK [0-9]+ pays .*$TXID:100000" 30 || fail "no block $BLOCK holding $TXID"
await "^chain joined=1 top=$((BLOCKS + 1)) left=0\$" 10 || fail "no chain event for height $((BLOCKS + 1))"
ok "block $BLOCK came in full and pays $TXID 100000 sat at height $((BLOCKS + 1))"

say "6. A reorganization"
cli invalidateblock "$BLOCK" >/dev/null
# Another address, so that the new block is not the block just invalidated
# (same parent, same transactions, same second).
OTHER=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 2 "$OTHER" >/dev/null
NEWTIP=$(cli getbestblockhash)
await "^chain joined=2 top=$((BLOCKS + 2)) left=1\$" 30 || fail "no chain event with a block that left"
await "^block $NEWTIP [0-9]+ " 30 || fail "no block event for the new tip $NEWTIP"
ok "chain event: 2 joined, top $((BLOCKS + 2)), 1 left; new tip $NEWTIP fetched"
grep -E '^(chain|block) ' "$LOG" | tail -4 | show

say "7. Stop, mine one block, start again"
stop_driver
[ -s "$STORE_DIR/headers.bin" ] || fail "no stored headers"
[ -s "$STORE_DIR/book.bin" ] || fail "no stored address book"
cli generatetoaddress 1 "$MINE" >/dev/null
TIP_AFTER=$((BLOCKS + 3))
NODE_TIP=$(cli getblockcount)
[ "$NODE_TIP" = "$TIP_AFTER" ] || fail "the node is at $NODE_TIP, not $TIP_AFTER"
before=$(headers_bytes)
start_driver second
await "^peers 0 .* tip=$((BLOCKS + 2)) " 10 || fail "the driver did not start at the stored tip $((BLOCKS + 2))"
await "^peers 1 .* tip=$TIP_AFTER " 30 || fail "the driver did not reach $TIP_AFTER"
[ "$(joined_sum)" = 1 ] || fail "$(joined_sum) headers joined after the restart, not 1"
SECOND=$(headers_bytes)
SENT=$((SECOND - before))
echo "      the node sent $SENT bytes of headers messages to the driver on the second run (first run: $HEADERS_FIRST)"
[ "$SENT" -le 400 ] || fail "the node sent $SENT bytes of headers: the chain was downloaded again"
ok "started at the stored tip $((BLOCKS + 2)), one header joined, $SENT bytes of headers sent (the first run took $HEADERS_FIRST)"
stop_driver

say "Result"
echo "ALL PASS"
