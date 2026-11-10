#!/usr/bin/env bash
# Fee sample against Bitcoin Core, on regtest.
#
# Question: does `block_fee_sample` (docs/FEE-RULES.md) get a block's total
# fee and size right, from the block's bytes alone, as Bitcoin Core counts
# them?
#
# What it does:
#   1. Starts a private regtest node in a scratch directory.
#   2. Mines 101 blocks, sends transactions at several fee rates, mines a
#      block, and compares the sample of that block with `getblockstats`:
#      the fees with `totalfee`, the size with `total_weight`, the rate with
#      `avgfeerate`, which must be equal.
#   3. Does the same for a block with no transactions.
#   4. Mines past the regtest halving (block 150), sends again, and compares
#      once more, so that the halved subsidy is used.
#
# Needs: bitcoind and bitcoin-cli (Bitcoin Core 29.0 or newer) and jq on PATH
# (the default is Core 32.1 in ~/.local/opt). Set FEE_SAMPLE to a prebuilt
# `fee_sample` example binary, or the script runs it with cargo.
#
# Environment:
#   FEE_DIR       scratch directory (default: ~/.cache/openbmt/tmp/fees/run)
#   FEE_RPC_PORT  RPC port for the regtest node (default 18947)
#   FEE_KEEP=1    keep the scratch directory afterwards
#   FEE_SAMPLE    command that runs the `fee_sample` example
#   CORE_BIN      directory of the Bitcoin Core binaries
#
# Everything here is regtest. No real keys or funds are involved.

set -euo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
PATH="$CORE_BIN:$PATH"
RPC_PORT=${FEE_RPC_PORT:-18947}
DIR=${FEE_DIR:-$HOME/.cache/openbmt/tmp/fees/run}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
FAILED=0

say() { printf '\n== %s\n' "$*"; }
ok() { printf 'PASS  %s\n' "$*"; }
bad() {
  printf 'FAIL  %s\n' "$*" >&2
  FAILED=1
}
die() {
  printf 'FAIL  %s\n' "$*" >&2
  exit 1
}
cli() { bitcoin-cli -regtest -datadir="$DIR" -rpcport="$RPC_PORT" "$@"; }
# Check that two values are equal.
same() {
  if [ "$2" = "$3" ]; then ok "$1: $2"; else bad "$1: sample $2, Core $3"; fi
}

for tool in bitcoind bitcoin-cli jq; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH"
done
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) die "this check is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

mkdir -p "$DIR"
NODE_PID=
cleanup() {
  # Stop the node by its own pid, and only if that pid is still bitcoind.
  if [ -n "$NODE_PID" ] && [ "$(ps -p "$NODE_PID" -o comm= 2>/dev/null || true)" = bitcoind ]; then
    cli stop >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 1
    done
    if kill -0 "$NODE_PID" 2>/dev/null; then kill -9 "$NODE_PID" 2>/dev/null || true; fi
  fi
  if [ "${FEE_KEEP:-0}" != 1 ]; then rm -rf -- "$DIR"; fi
}
trap cleanup EXIT

bitcoind -regtest -datadir="$DIR" -rpcport="$RPC_PORT" -listen=0 -daemonwait \
  -fallbackfee=0.0001 -dbcache=50 >/dev/null
NODE_PID=$(cat "$DIR/regtest/bitcoind.pid")

SAMPLE=${FEE_SAMPLE:-cargo run --quiet --locked --manifest-path "$ROOT/Cargo.toml" -p openbmt-watch --example fee_sample --}

cli -named createwallet wallet_name=shop >/dev/null
w() { cli -rpcwallet=shop "$@"; }
MINER=$(w getnewaddress)

# The value after the word `$2` in the sample line `$1`.
field() { awk -v k="$2" '{for (i = 1; i < NF; i++) if ($i == k) print $(i + 1)}' <<<"$1"; }

# Compare the sample of a block with Core's numbers for it.
compare() {
  local label=$1 hash=$2 height stats weight cbw txs line fees total cbv vsize rate want avg diff
  height=$(cli getblockheader "$hash" | jq -r '.height')
  line=$(cli getblock "$hash" 0 | $SAMPLE regtest "$height") || {
    bad "$label: the sample was refused"
    return
  }
  stats=$(cli getblockstats "$hash")
  weight=$(cli getblock "$hash" 1 | jq -r '.weight')
  cbw=$(cli getblock "$hash" 2 | jq -r '.tx[0].weight')
  txs=$(jq -r '.txs' <<<"$stats")
  fees=$(field "$line" fees)
  same "$label: fees" "$fees" "$(jq -r '.totalfee' <<<"$stats")"
  same "$label: block hash" "$(field "$line" block)" "$hash"
  same "$label: height" "$(field "$line" height)" "$height"
  # Core's total_weight leaves out the coinbase; the block is that, the
  # coinbase, the 80-byte header and a one-byte transaction count.
  total=$(jq -r '.total_weight' <<<"$stats")
  same "$label: weight without the coinbase" "$((weight - cbw - 324))" "$total"
  vsize=$(((weight + 3) / 4))
  same "$label: vsize" "$(field "$line" vsize)" "$vsize"
  rate=$(field "$line" rate_milli)
  if [ "$txs" -le 1 ]; then
    same "$label: rate" "$rate" none
  else
    # FE-05: fees times 4,000 over the weight of the transactions but the
    # coinbase, which is Core's `total_weight`; Core's `avgfeerate` is the
    # same measure in whole sat/vB.
    want=$((fees * 4000 / total))
    same "$label: rate from Core's numbers" "$rate" "$want"
    same "$label: rate in whole sat/vB against Core's avgfeerate" \
      "$((rate / 1000))" "$(jq -r '.avgfeerate' <<<"$stats")"
  fi
}

# Send to fresh addresses at different fee rates (sat/vB), in one go.
send_some() {
  local r
  for r in 1 3 10 40 150; do
    w -named sendtoaddress address="$(w getnewaddress)" amount=0.01 fee_rate="$r" >/dev/null
  done
}

say "1. 101 blocks, then a block with transactions at five fee rates"
cli generatetoaddress 101 "$MINER" >/dev/null
send_some
[ "$(cli getrawmempool | jq 'length')" -ge 5 ] || die "the transactions did not reach the mempool"
HASH=$(cli generatetoaddress 1 "$MINER" | jq -r '.[0]')
compare "block with transactions" "$HASH"

say "2. A block with no transactions"
HASH=$(cli generatetoaddress 1 "$MINER" | jq -r '.[0]')
compare "empty block" "$HASH"

say "3. Past the regtest halving (block 150)"
HEIGHT=$(cli getblockcount)
cli generatetoaddress $((155 - HEIGHT)) "$MINER" >/dev/null
send_some
HASH=$(cli generatetoaddress 1 "$MINER" | jq -r '.[0]')
H=$(cli getblockheader "$HASH" | jq -r '.height')
[ "$H" -ge 150 ] || die "the block is below the halving"
SUBSIDY=$(cli getblock "$HASH" 2 | jq -r '.tx[0].vout | map(.value) | add')
same "the halved block pays about 25 coins" "$(jq -n --argjson v "$SUBSIDY" '$v >= 25 and $v < 26')" true
compare "halved block" "$HASH"

if [ "$FAILED" = 0 ]; then
  say "all steps passed"
else
  die "some steps failed"
fi
