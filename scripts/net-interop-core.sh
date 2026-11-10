#!/usr/bin/env bash
# The counter's peer session against Bitcoin Core, on regtest.
#
# Question: does `openbmt-net` complete the handshake with Bitcoin Core,
# and does it fetch headers, filters, blocks and unconfirmed transactions
# that the watcher's checks accept?
#
# What it does:
#   1. Starts a private regtest node that serves compact filters, and mines
#      2,105 blocks (one full headers message and a short one).
#   2. Handshake: the node's services include compact filters, and its
#      start height is 2,105.
#   3. Header sync to the node's tip, through the watcher's header store.
#   4. A new block, mined while the session waits, arrives unasked.
#   5. A payment to a fresh address is announced, fetched and parsed.
#   6. The block that holds it: its basic filter matches the payment's
#      script and does not match an unrelated one.
#   7. The same block, downloaded in full, proves the payment under the
#      watcher's block checks.
#   8. After the handshake the session sends nothing but answers to pings
#      (NR-55) and is not dropped.
#   8b. Message types: over the transport under test (v2 by default), the
#      session sees `headers`, `inv`, `tx`, `block`, `cfilter` and `addrv2`
#      arrive from the node and answers its `ping` with `pong`, and the
#      node's own counters (`getpeerinfo`) agree that it sent them.
#   9. A connection to a port that is not a Bitcoin peer ends within the
#      handshake limit and does not hang.
#
# Every step runs against a node with -v2transport=$DRILL_V2TRANSPORT (1 by
# default, 0 for a node that speaks the old transport only). Step 2 checks
# the transport (NR-80, NR-81) and step 2b compares the session ID that
# both ends derive (NR-84). `scripts/net-v2-interop-core.sh` runs both
# settings.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another), jq, and
# cargo unless PEER names a prebuilt `regtest_peer` example binary.
#
# Environment:
#   CORE_BIN        directory with bitcoind and bitcoin-cli
#   DRILL_V2TRANSPORT  1 (default) or 0: the node's -v2transport setting
#   DRILL_DIR       scratch directory (default: a new directory under
#                   ~/.cache/openbmt)
#   DRILL_RPC_PORT  RPC port for the regtest node (default 18946)
#   DRILL_P2P_PORT  peer port for the regtest node (default 18947)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#   PEER            command that runs the `regtest_peer` example
#
# Everything here is regtest on 127.0.0.1. No real keys or funds are
# involved.

set -euo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
V2T=${DRILL_V2TRANSPORT:-1}
case "$V2T" in 0 | 1) ;; *) echo "DRILL_V2TRANSPORT must be 0 or 1" >&2; exit 1 ;; esac
RPC_PORT=${DRILL_RPC_PORT:-18946}
P2P_PORT=${DRILL_P2P_PORT:-18947}
mkdir -p "$HOME/.cache/openbmt"
DIR=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/net-XXXXXX")}
# NR-81: the example remembers a peer that does not speak the encrypted
# transport in this file, as the counter's address book does.
export REGTEST_MEMORY="$DIR/v2-memory"
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PEER=${PEER:-cargo run --quiet --locked --manifest-path "$ROOT/Cargo.toml" -p openbmt-net --example regtest_peer --}
BLOCKS=2105
# A script that no transaction of this check pays.
OTHER=0014000102030405060708090a0b0c0d0e0f10111213

say() { printf '\n== %s\n' "$*"; }
ok() { printf 'PASS  %s\n' "$*"; }
fail() {
  printf 'FAIL  %s\n' "$*" >&2
  exit 1
}
cli() { bitcoin-cli -regtest -datadir="$DIR" -rpcport="$RPC_PORT" "$@"; }
field() { awk -v k="$1" -v n="$2" '$1 == k { print $n; exit }'; }
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
say "$VERSION (-v2transport=$V2T)"
if [ "$V2T" = 1 ]; then WANT=v2; else WANT=v1; fi

mkdir -p "$DIR"
NODE_PID=
cleanup() {
  if [ -n "$NODE_PID" ]; then
    bitcoin-cli -regtest -datadir="$DIR" -rpcport="$RPC_PORT" stop >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$NODE_PID" 2>/dev/null || true
  fi
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$DIR"; fi
}
trap cleanup EXIT

# Run the peer example against the node. Sets OUT (its lines) and CODE (its
# exit code), and prints the lines.
peer() {
  CODE=0
  OUT=$($PEER "$P2P_PORT" "$@") || CODE=$?
  printf '%s\n' "$OUT" | show
}
# Wait until the node's filter index has reached the chain tip.
wait_filters() {
  local want
  want=$(cli getblockcount)
  for _ in $(seq 1 60); do
    if [ "$(cli getindexinfo | jq -r '."basic block filter index".best_block_height // 0')" = "$want" ]; then
      return 0
    fi
    sleep 1
  done
  fail "the filter index did not reach height $want"
}

bitcoind -regtest -datadir="$DIR" -rpcport="$RPC_PORT" -port="$P2P_PORT" \
  -listen=1 -bind=127.0.0.1 -listenonion=0 -daemonwait \
  -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
  -fallbackfee=0.0001 -dbcache=50 -v2transport="$V2T" >/dev/null
NODE_PID=$(cat "$DIR/regtest/bitcoind.pid")

say "1. A node that serves filters, and $BLOCKS blocks"
cli -named createwallet wallet_name=miner >/dev/null
MINE=$(cli -rpcwallet=miner getnewaddress)
MINED=0
while [ "$MINED" -lt "$BLOCKS" ]; do
  STEP=$((BLOCKS - MINED))
  [ "$STEP" -gt 100 ] && STEP=100
  cli generatetoaddress "$STEP" "$MINE" >/dev/null
  MINED=$((MINED + STEP))
done
wait_filters
[ "$(cli getblockcount)" = "$BLOCKS" ] || fail "the node is not at height $BLOCKS"
BEST=$(cli getbestblockhash)
ok "height $BLOCKS, best block $BEST, filter index caught up"

say "2. Handshake"
# Core states the height of its last block-tip notice, which can trail the
# chain for a moment after a burst of blocks. A height below the chain is
# printed and tried again; one that stays wrong fails.
for TRY in 1 2 3 4 5 6 7 8 9 10; do
  peer handshake
  [ "$CODE" = 0 ] || fail "the handshake did not finish"
  [ "$TRY" = 1 ] && FIRST=$OUT
  [ "$(field ready 3 <<<"$OUT")" = "$BLOCKS" ] && break
  echo "      start height $(field ready 3 <<<"$OUT") on try $TRY, not $BLOCKS yet"
  sleep 1
done
SERVICES=$(field ready 2 <<<"$OUT")
[ "$(field ready 3 <<<"$OUT")" = "$BLOCKS" ] || fail "the start height is not $BLOCKS"
[ "$(field filters 2 <<<"$OUT")" = yes ] || fail "the compact filter service is not offered"
[ $((16#$SERVICES & 0x40)) -ne 0 ] || fail "services $SERVICES do not have bit 6"
ok "ready, services $SERVICES, start height $BLOCKS, compact filters offered"
[ "$(field transport 2 <<<"$FIRST")" = "$WANT" ] || fail "the first connection did not end on $WANT"
if [ "$WANT" = v1 ]; then
  [ "$(field fallback 2 <<<"$FIRST")" = yes ] || fail "the first connection to a v1 node was not the NR-81 fallback"
  peer handshake
  [ "$CODE" = 0 ] || fail "the second handshake did not finish"
  [ "$(field transport 2 <<<"$OUT")" = v1 ] || fail "a later connection was not on v1"
  [ "$(field fallback 2 <<<"$OUT")" = no ] || fail "a later connection tried the encrypted transport again"
  ok "the first connection fell back to v1 (NR-81); the next went straight to v1"
else
  [ "$(field fallback 2 <<<"$FIRST")" = no ] || fail "a fallback to v1 against a node that speaks v2"
  ok "the first connection used the encrypted transport (NR-80), no fallback"
fi

say "2b. The transport as the node sees it (NR-82, NR-84)"
SIL="$DIR/silent.out"
: >"$SIL"
$PEER "$P2P_PORT" silent 12 >"$SIL" 2>&1 &
SIL_PID=$!
for _ in $(seq 1 30); do
  grep -q '^session_id ' "$SIL" && break
  sleep 0.5
done
grep -q '^session_id ' "$SIL" || fail "the session did not get ready"
INFO=$(cli getpeerinfo | jq -c '[.[] | select(.inbound == true)] | .[0] | {transport_protocol_type, session_id}')
echo "      node: $INFO"
CODE=0
wait "$SIL_PID" || CODE=$?
[ "$CODE" = 0 ] || fail "the silent session ended with exit code $CODE"
OURS=$(field session_id 2 <"$SIL")
echo "      ours: transport $(field transport 2 <"$SIL"), session_id $OURS"
[ "$(jq -r .transport_protocol_type <<<"$INFO")" = "$WANT" ] || fail "the node reports a transport other than $WANT"
[ "$(field transport 2 <"$SIL")" = "$WANT" ] || fail "the example reports a transport other than $WANT"
if [ "$WANT" = v2 ]; then
  THEIRS=$(jq -r .session_id <<<"$INFO")
  [ -n "$OURS" ] && [ "$OURS" != none ] || fail "no session ID on our side"
  [ "$OURS" = "$THEIRS" ] || fail "the session IDs differ: ours $OURS, the node's $THEIRS"
  ok "transport v2 on both sides, and the session ID is the same: $OURS"
else
  [ "$OURS" = none ] || fail "a v1 connection has a session ID"
  ok "transport v1 on both sides; no session ID on either"
fi

say "3. Header sync (NR-30, NR-31)"
peer sync "$BEST"
[ "$CODE" = 0 ] || fail "the sync did not reach the node's tip"
[ "$(field tip 3 <<<"$OUT")" = "$BEST" ] || fail "the tip is not the node's best block"
[ "$(field tip 2 <<<"$OUT")" = "$BLOCKS" ] || fail "the tip height is not $BLOCKS"
[ "$(field headers 2 <<<"$OUT")" = "$BLOCKS" ] || fail "not all $BLOCKS headers were accepted"
[ "$(field messages 2 <<<"$OUT")" -ge 2 ] || fail "the sync did not take two headers messages"
ok "$BLOCKS headers accepted by the header store in $(field messages 2 <<<"$OUT") messages; tip is the node's best block"

say "4. A new block arrives unasked"
ANN="$DIR/announce.out"
: >"$ANN"
$PEER "$P2P_PORT" announce 20 >"$ANN" 2>&1 &
ANN_PID=$!
for _ in $(seq 1 30); do
  grep -q '^waiting' "$ANN" && break
  sleep 1
done
grep -q '^waiting' "$ANN" || fail "the session did not reach the waiting state"
sleep 3
cli generatetoaddress 1 "$MINE" >/dev/null
NEW=$(cli getbestblockhash)
CODE=0
wait "$ANN_PID" || CODE=$?
show <"$ANN"
OUT=$(cat "$ANN")
[ "$CODE" = 0 ] || fail "the session ended with exit code $CODE"
[ "$(field tip 3 <<<"$OUT")" = "$NEW" ] || fail "the tip is not the new block $NEW"
if grep -q '^announced-headers ' <<<"$OUT"; then
  ok "the new block $NEW came as a headers message, after sendheaders"
elif grep -q "^announced-inv-block $NEW" <<<"$OUT"; then
  ok "the new block $NEW came as an inv, not a headers message (reported, not a failure)"
else
  fail "the new block was not announced"
fi

say "5. An unconfirmed payment (NR-50, NR-51)"
ADDR_A=$(cli -rpcwallet=miner getnewaddress)
SCRIPT_A=$(cli -rpcwallet=miner getaddressinfo "$ADDR_A" | jq -r '.scriptPubKey')
TXF="$DIR/tx.out"
: >"$TXF"
$PEER "$P2P_PORT" tx 60 "$SCRIPT_A" >"$TXF" 2>&1 &
TX_PID=$!
sleep 3
TXID=$(cli -rpcwallet=miner sendtoaddress "$ADDR_A" 0.001)
CODE=0
wait "$TX_PID" || CODE=$?
show <"$TXF"
OUT=$(cat "$TXF")
[ "$CODE" = 0 ] || fail "the session ended with exit code $CODE"
grep -q "^announced $TXID\$" <<<"$OUT" || fail "the payment $TXID was not announced"
grep -q "^tx $TXID 100000 yes\$" <<<"$OUT" || fail "the payment was not fetched as 100000 sat with a witness"
ok "announced, fetched and parsed: $TXID pays 100000 sat, has witness data"

say "6. The block's compact filter (NR-40, NR-41)"
cli generatetoaddress 1 "$MINE" >/dev/null
BLOCK_B=$(cli getbestblockhash)
HEIGHT_B=$(cli getblockcount)
[ "$(cli getblock "$BLOCK_B" | jq -r '.tx | index("'"$TXID"'") != null')" = true ] || fail "block $BLOCK_B does not hold the payment"
wait_filters
peer filter "$HEIGHT_B" "$BLOCK_B" "$SCRIPT_A"
[ "$CODE" = 0 ] || fail "the filter request failed"
[ "$(field match 2 <<<"$OUT")" = yes ] || fail "the filter does not match the payment's script"
ok "the filter of block $HEIGHT_B ($(field filter-bytes 2 <<<"$OUT") bytes) matches the payment's script"
peer filter "$HEIGHT_B" "$BLOCK_B" "$OTHER"
[ "$CODE" = 0 ] || fail "the filter request failed"
[ "$(field match 2 <<<"$OUT")" = no ] || fail "the filter matches a script that is not in the block"
ok "the same filter does not match an unrelated script"

say "7. The full block (NR-45, WR-20 to WR-22, WR-30)"
peer block "$BLOCK_B" "$SCRIPT_A"
[ "$CODE" = 0 ] || fail "the block was not fetched or was refused"
[ "$(field payments 2 <<<"$OUT")" = 1 ] || fail "the block does not prove exactly one payment"
grep -q "^payment $TXID [0-9]* 100000\$" <<<"$OUT" || fail "the proven payment is not $TXID for 100000 sat"
ok "the watcher's block checks prove $TXID for 100000 sat in block $HEIGHT_B"

say "8. Silence after the handshake (NR-55)"
peer silent 15
echo "      node tip $(cli getbestblockhash)"
[ "$CODE" = 0 ] || fail "the session ended with exit code $CODE, not a quiet run"
[ "$(field sent-bytes-after-handshake 2 <<<"$OUT")" = 0 ] || fail "the session sent bytes other than pong after the handshake"
ok "15 s without a drop; nothing sent after the handshake except answers to pings"

say "8b. Message types over $WANT (the one-byte IDs of BIP324 when v2)"
# Routable addresses in the node's address book. The node answers `getaddr`
# with at most 23 percent of its book, rounded down, so a few dozen are
# needed for an `addrv2` message that is not empty.
ADDED=0
for I in $(seq 1 60); do
  if [ "$(cli addpeeraddress "$((60 + I)).$((10 + I)).$((100 + I)).7" 8333 true | jq -r .success)" = true ]; then
    ADDED=$((ADDED + 1))
  fi
done
[ "$ADDED" -ge 20 ] || fail "only $ADDED addresses were added to the node's book"
wait_filters
MIX="$DIR/mix.out"
: >"$MIX"
$PEER "$P2P_PORT" mix 30 "$(cli getblockcount)" "$(cli getbestblockhash)" >"$MIX" 2>&1 &
MIX_PID=$!
for _ in $(seq 1 60); do
  grep -q '^ready-for-actions' "$MIX" && break
  sleep 0.5
done
grep -q '^ready-for-actions' "$MIX" || fail "the mixed session did not get going"
cli ping
ADDR_M=$(cli -rpcwallet=miner getnewaddress)
cli -rpcwallet=miner sendtoaddress "$ADDR_M" 0.001 >/dev/null
# The payment stays unconfirmed until the node has announced it: a block
# that holds it first would make the announcement pointless.
KINDS="headers inv tx block cfilter addrv2 ping"
SENT_BY_NODE=
for _ in $(seq 1 25); do
  SENT_BY_NODE=$(cli getpeerinfo | jq -c '[.[] | select(.inbound == true)] | .[0]')
  ALL=yes
  for K in $KINDS; do
    [ "$(jq -r ".bytessent_per_msg.$K // 0" <<<"$SENT_BY_NODE")" -gt 0 ] || ALL=no
  done
  [ "$ALL" = yes ] && break
  sleep 1
done
CODE=0
wait "$MIX_PID" || CODE=$?
show <"$MIX"
[ "$CODE" = 0 ] || fail "the mixed session ended with exit code $CODE"
[ "$(field transport 2 <"$MIX")" = "$WANT" ] || fail "the mixed session is not on $WANT"
[ "$(jq -r .transport_protocol_type <<<"$SENT_BY_NODE")" = "$WANT" ] || fail "the node does not report $WANT for this connection"
for K in $KINDS; do
  N=$(jq -r ".bytessent_per_msg.$K // 0" <<<"$SENT_BY_NODE")
  [ "$N" -gt 0 ] || fail "the node did not send a $K message on this connection"
  echo "      the node sent $K: $N bytes"
done
for K in headers inv tx block cfilter addrv2; do
  [ "$(awk -v k="$K" '$1 == "seen" && $2 == k { print $3 }' <"$MIX")" -gt 0 ] || fail "the session did not see a $K message arrive"
done
[ "$(field pongs 2 <"$MIX")" -ge 1 ] || fail "the session did not answer the node's ping"
[ "$(jq -r '.bytesrecv_per_msg.pong // 0' <<<"$SENT_BY_NODE")" -gt 0 ] || fail "the node did not receive a pong"
ok "over $WANT: headers, inv, tx, block, cfilter, addrv2 and ping arrived from the node, each read by the session; the pong was read by the node"

say "9. A port that is not a Bitcoin peer (NR-14)"
START=$(date +%s)
CODE=0
OUT=$(timeout 80 $PEER "$RPC_PORT" handshake) || CODE=$?
SECS=$(($(date +%s) - START))
printf '%s\n' "$OUT" | show
echo "      exit code $CODE after $SECS s"
case "$CODE" in
  1 | 2) grep -Eq '^(dropped|failed) ' <<<"$OUT" || fail "no dropped or failed line" ;;
  124) fail "the connection to the RPC port hung past 80 s" ;;
  *) fail "unexpected exit code $CODE" ;;
esac
ok "the RPC port ended the session with a clear line after $SECS s"

say "Result"
echo "ALL PASS"
