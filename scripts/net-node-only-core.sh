#!/usr/bin/env bash
# The driver with the shop's own node as the only peer, against Bitcoin
# Core on regtest.
#
# Question: with `NetMode::NodeOnly`, does the driver connect to Bitcoin
# Core as its own node and to nothing else; does it get headers, filters and
# a block that pays a watched script from Core alone; does it see a
# transaction that Core announces in its memory pool; does a transaction it
# broadcasts reach Core and get accepted; and does it report a node that
# does not serve compact filters? (docs/NETWORK-RULES.md NR-106 to NR-111.)
#
# What it does:
#   1. Starts a private regtest node with -blockfilterindex=1
#      -peerblockfilters=1 and mines 150 blocks, and a second regtest node
#      without compact filters.
#   2. Runs the ignored test `node_only_core_regtest` of `openbmt-net-io`,
#      which starts the driver in the mode with Core as the own node (at
#      127.0.0.1; a fixed peer is set to 127.0.0.2, where the same Core
#      listens, and must never be dialed) and:
#        - checks that Core sees exactly one peer and that the driver dialed
#          nothing but Core;
#        - waits for the headers and for every block to be checked by its one
#          filter, with no block downloaded;
#        - pays an address: Core announces the transaction, the driver
#          fetches it and reports that the own node holds it;
#        - mines a block that holds the payment: the filter matches and the
#          block is fetched from Core;
#        - broadcasts a signed transaction: it is in Core's memory pool;
#        - connects the mode to the second node: the driver reports the
#          missing compact filters and does not count the node as usable.
#
# The test makes its own node with a constructor that exists only with the
# cargo feature `clear-own-node-for-tests`, which only the driver's
# dev-dependency turns on (NR-99). A real Tor address needs a Tor proxy and
# is not part of this check.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another), jq, python3
# and cargo. Run it under the memory guard, for example:
#   ~/.cache/openbmt/guard.sh env OPENBMT_NO_SCCACHE=1 \
#     CARGO_TARGET_DIR=$HOME/.cache/openbmt/target/nodeonly \
#     scripts/net-node-only-core.sh
#
# Environment:
#   CORE_BIN        directory with bitcoind and bitcoin-cli
#   DRILL_DIR       scratch directory (default: ~/.cache/openbmt/tmp/node-only-core)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#
# The ports are free ones found at the start. Everything here is regtest on
# 127.0.0.1 and 127.0.0.2. No real keys or funds are involved. Both nodes and
# the scratch directory are removed on every exit.

set -euo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
BASE=${DRILL_DIR:-$HOME/.cache/openbmt/tmp/node-only-core}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE1="$BASE/node1"
NODE2="$BASE/node2"
BLOCKS=150

say() { printf '\n== %s\n' "$*"; }
ok() { printf 'PASS  %s\n' "$*"; }
fail() {
  printf 'FAIL  %s\n' "$*" >&2
  exit 1
}

for tool in bitcoind bitcoin-cli jq python3; do
  command -v "$tool" >/dev/null || fail "$tool is not on PATH"
done
[ "$(command -v bitcoind)" = "$CORE_BIN/bitcoind" ] || fail "bitcoind is not the one in $CORE_BIN"
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) fail "this check is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

# Four free ports: RPC and peer for each node.
read -r RPC1 P2P1 RPC2 P2P2 < <(python3 -I - <<'PY'
import socket
socks = [socket.socket() for _ in range(4)]
for s in socks:
    s.bind(("127.0.0.1", 0))
print(*[s.getsockname()[1] for s in socks])
PY
)

cli1() { bitcoin-cli -regtest -datadir="$NODE1" -rpcport="$RPC1" "$@"; }
cli2() { bitcoin-cli -regtest -datadir="$NODE2" -rpcport="$RPC2" "$@"; }

rm -rf -- "$BASE"
mkdir -p "$NODE1" "$NODE2"
PID1=
PID2=
stop_node() { # pid, cli function name
  local pid=$1 stopper=$2
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    "$stopper" stop >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$pid" 2>/dev/null || true
  fi
}
cleanup() {
  stop_node "$PID1" cli1
  stop_node "$PID2" cli2
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

# The first node serves compact filters and listens on a second loopback
# address too, so that a fixed peer can be pointed at it.
bitcoind -regtest -datadir="$NODE1" -rpcport="$RPC1" \
  -listen=1 -bind="127.0.0.1:$P2P1" -bind="127.0.0.2:$P2P1" \
  -listenonion=0 -dnsseed=0 -fixedseeds=0 -daemonwait \
  -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
  -fallbackfee=0.0001 -dbcache=50 >/dev/null
PID1=$(cat "$NODE1/regtest/bitcoind.pid")
# The second node does not serve compact filters.
bitcoind -regtest -datadir="$NODE2" -rpcport="$RPC2" \
  -listen=1 -bind="127.0.0.1:$P2P2" \
  -listenonion=0 -dnsseed=0 -fixedseeds=0 -daemonwait \
  -blockfilterindex=0 -peerblockfilters=0 -txindex=0 -dbcache=50 >/dev/null
PID2=$(cat "$NODE2/regtest/bitcoind.pid")

say "1. A node with filters and $BLOCKS blocks, and a node without filters"
cli1 -named createwallet wallet_name=miner >/dev/null
MINE=$(cli1 -rpcwallet=miner getnewaddress)
cli1 generatetoaddress "$BLOCKS" "$MINE" >/dev/null
[ "$(cli1 getblockcount)" = "$BLOCKS" ] || fail "the node is not at height $BLOCKS"
cli1 getblockchaininfo >/dev/null
SERVICES1=$(cli1 getnetworkinfo | jq -r '.localservicesnames | join(",")')
SERVICES2=$(cli2 getnetworkinfo | jq -r '.localservicesnames | join(",")')
case "$SERVICES1" in *COMPACT_FILTERS*) ;; *) fail "node 1 does not serve filters: $SERVICES1" ;; esac
case "$SERVICES2" in *COMPACT_FILTERS*) fail "node 2 serves filters: $SERVICES2" ;; esac
ok "node 1 serves $SERVICES1; node 2 serves $SERVICES2"

ADDR=$(cli1 -rpcwallet=miner getnewaddress)
# A signed transaction that the node has not broadcast; its coins are locked
# so that the payment of the test does not spend them.
RAW=$(cli1 -rpcwallet=miner createrawtransaction '[]' "{\"$(cli1 -rpcwallet=miner getnewaddress)\":0.002}")
FUNDED=$(cli1 -rpcwallet=miner fundrawtransaction "$RAW" '{"lockUnspents":true}' | jq -r .hex)
RAWTX=$(cli1 -rpcwallet=miner signrawtransactionwithwallet "$FUNDED" | jq -r .hex)
RAWTXID=$(cli1 decoderawtransaction "$RAWTX" | jq -r .txid)
ok "paying $ADDR; a transaction $RAWTXID is ready to broadcast"

say "2. The driver with Core as the own node and nothing else"
export OBM_CORE_CLI="$CORE_BIN/bitcoin-cli"
export OBM_CORE_DATADIR="$NODE1"
export OBM_CORE_RPCPORT="$RPC1"
export OBM_CORE_P2PPORT="$P2P1"
export OBM_CORE2_P2PPORT="$P2P2"
export OBM_CORE_ADDR="$ADDR"
export OBM_CORE_HEIGHT="$BLOCKS"
export OBM_CORE_RAWTX="$RAWTX"
export OBM_CORE_RAWTXID="$RAWTXID"
LOG="$BASE/test.log"
if cargo test --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
  -p openbmt-net-io node_only_core -- --ignored --nocapture --test-threads=1 \
  >"$LOG" 2>&1; then
  sed 's/^/      /' "$LOG" | grep -v '^      $' || true
  grep -q '^ALL PASS' "$LOG" || fail "the test did not print ALL PASS"
else
  sed 's/^/      /' "$LOG" >&2
  fail "the test failed"
fi

say "Result"
echo "ALL PASS"
