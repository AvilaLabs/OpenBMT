#!/usr/bin/env bash
# The counter's own-node check against Bitcoin Core, on regtest.
#
# Question: does the driver keep Bitcoin Core connected as the merchant's own
# node, apart from its random peers; does it report that Core holds a payment
# in its memory pool, and Core's tip; does the watcher count the payment as
# seen by the own node and the light client only while the node can be used;
# and when Core stops, does the counter fall back to the light check, with
# words that say why? (docs/NETWORK-RULES.md NR-98 to NR-103,
# docs/WATCHER-RULES.md WR-60 to WR-65.)
#
# What it does:
#   1. Starts a private regtest node and mines 150 blocks.
#   2. Runs the ignored test `own_node_core_regtest` of `openbmt-net-io`,
#      which starts the driver with Core as the own node (at 127.0.0.1) and
#      as one fixed peer (at 127.0.0.2: the driver never dials the own
#      node's address as a peer), and:
#        - waits for the own node to connect and show Core's height, and the
#          counter's own chain to reach it;
#        - pays an address: the own node reports the transaction, an unknown
#          transaction is not reported, and the watcher gives the own-node
#          mode when the light client's three groups are stood in for;
#        - mines a block: the own node's tip moves with it;
#        - stops Core: the driver says the own node is down, the watcher's
#          mode falls back to the light check, and the health line has words.
#
# The test makes its own node with a constructor that exists only with the
# cargo feature `clear-own-node-for-tests`, which only the driver's
# dev-dependency turns on (NR-99). A real Tor address needs a Tor proxy and
# is not part of this check.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another), jq, and
# cargo.
#
# Environment:
#   CORE_BIN        directory with bitcoind and bitcoin-cli
#   DRILL_DIR       scratch directory (default: a new directory under
#                   ~/.cache/openbmt)
#   DRILL_RPC_PORT  RPC port for the regtest node (default 18966)
#   DRILL_P2P_PORT  peer port for the regtest node (default 18967)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#
# Everything here is regtest on 127.0.0.1. No real keys or funds are
# involved. The node and the scratch directory are removed on every exit.

set -euo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_RPC_PORT:-18966}
P2P_PORT=${DRILL_P2P_PORT:-18967}
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/ownnode-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/node"
BLOCKS=150

say() { printf '\n== %s\n' "$*"; }
ok() { printf 'PASS  %s\n' "$*"; }
fail() {
  printf 'FAIL  %s\n' "$*" >&2
  exit 1
}
cli() { bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" "$@"; }

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

mkdir -p "$NODE_DIR"
NODE_PID=
cleanup() {
  if [ -n "$NODE_PID" ] && kill -0 "$NODE_PID" 2>/dev/null; then
    cli stop >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$NODE_PID" 2>/dev/null || true
  fi
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" \
  -listen=1 -bind="127.0.0.1:$P2P_PORT" -bind="127.0.0.2:$P2P_PORT" \
  -listenonion=0 -daemonwait \
  -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
  -fallbackfee=0.0001 -dbcache=50 >/dev/null
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")

say "1. A node and $BLOCKS blocks"
cli -named createwallet wallet_name=miner >/dev/null
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress "$BLOCKS" "$MINE" >/dev/null
[ "$(cli getblockcount)" = "$BLOCKS" ] || fail "the node is not at height $BLOCKS"
ADDR=$(cli -rpcwallet=miner getnewaddress)
ok "height $BLOCKS; paying $ADDR"

say "2. The driver with Core as the own node"
export OBM_CORE_CLI="$CORE_BIN/bitcoin-cli"
export OBM_CORE_DATADIR="$NODE_DIR"
export OBM_CORE_RPCPORT="$RPC_PORT"
export OBM_CORE_P2PPORT="$P2P_PORT"
export OBM_CORE_ADDR="$ADDR"
export OBM_CORE_HEIGHT="$BLOCKS"
# The test stops the node itself; the exit trap then finds it gone.
LOG="$BASE/test.log"
if cargo test --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
  -p openbmt-net-io own_node_core -- --ignored --nocapture --test-threads=1 \
  >"$LOG" 2>&1; then
  sed 's/^/      /' "$LOG" | grep -v '^      $' || true
  grep -q '^ALL PASS' "$LOG" || fail "the test did not print ALL PASS"
else
  sed 's/^/      /' "$LOG" >&2
  fail "the test failed"
fi

say "Result"
echo "ALL PASS"
