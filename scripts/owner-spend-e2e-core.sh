#!/usr/bin/env bash
# The owner node's coin view and sending against Bitcoin Core, on regtest.
#
# Question: does the owner's device learn the coins of the 2-of-3 wallet from
# checked blocks alone, send a refund that two signers made, and follow it
# through the peers, a restart, a block, a conflicting spend and a
# reorganization, with its coin view equal to Core's at each stop?
#
# Three Bitcoin Core nodes (127.0.0.1, 127.0.0.2, 127.0.0.3, so that the
# driver counts them as three network groups) serve compact filters and are
# connected to each other. The example `owner_spend_regtest` is the owner
# node with the Key app, key 1 and key 2 in the same process, and the three
# nodes as its fixed peers. It sends through the main driver (the setting
# "send without Tor", which regtest needs and which says so in a note).
#
# What it does (the steps are numbered as in the printed table):
#   1. The wallet description goes into a blank watch-only Core wallet. Two
#      payments from Core's funded wallet to receive addresses 0 and 1 are
#      mined and buried. The owner node's coins equal `listunspent`: the same
#      outpoints, amounts and confirmations.
#   2. A refund is queued and `spend` builds a session, the Key app (key 2)
#      approves, key 1 signs, the node takes the response. The transaction
#      pays the refund address, is held and was handed to the driver once.
#   3. All three Core mempools hold the transaction, and the node's state for
#      it is "seen back" from at least two network groups other than those it
#      was first announced to.
#   6. The example is stopped and started again on the same directory before
#      the block: it holds the transaction, sends it again, signs nothing
#      again, and the mempools still hold one transaction.
#   4. A block confirms it: the state is confirmed, one deep, and the node's
#      coins equal `listunspent` (the change is a coin, the spent coin is
#      not).
#   5. Five more blocks: the state is done at the refund depth of 6, the
#      refund is paid and the queue's outbox is empty.
#   7. A second refund is built, and a conflicting transaction that spends
#      the same coins, made on a copy of the queue, is mined in a block. The
#      state is replaced, the refund is back in the queue as returned and
#      nothing is signed again by itself.
#   8. A third refund is sent and mined. The block is invalidated on all
#      three nodes and two empty blocks take its place: one block leaves the
#      chain. The node's coins equal `listunspent` with 0 confirmations
#      counted.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another), jq, and
# cargo unless OWNER_BIN names a prebuilt `owner_spend_regtest` example
# binary. A build goes through ~/.cache/openbmt/guard.sh when that file
# exists.
#
# Environment:
#   CORE_BIN        directory with bitcoind and bitcoin-cli
#   DRILL_DIR       scratch directory (default: a new directory under
#                   ~/.cache/openbmt)
#   DRILL_RPC_PORT  first RPC port; the next two follow (default 18980)
#   DRILL_P2P_PORT  peer port on the three loopback addresses (18990)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#   OWNER_BIN       the `owner_spend_regtest` example binary
#
# Everything here is regtest on 127.0.0.x, with keys made from fixed test
# bytes. No real keys or funds are involved. The nodes and the scratch
# directory are removed on every exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC0=${DRILL_RPC_PORT:-18980}
P2P=${DRILL_P2P_PORT:-18990}
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/owner-spend-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DATA_DIR="$BASE/owner"
FIFO="$BASE/stdin"
RATE=2500

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
# cli NODE args: node 0, 1 or 2.
cli() {
  local n=$1
  shift
  bitcoin-cli -regtest -datadir="$BASE/core$n" -rpcport=$((RPC0 + n)) "$@"
}
a() { cli 0 "$@"; }

declare -a RESULT MESSAGE BAD
pass() { RESULT[$1]=PASS; MESSAGE[$1]=$2; printf 'PASS  step %s: %s\n' "$1" "$2"; }
bad() { RESULT[$1]=FAIL; MESSAGE[$1]=$2; printf 'FAIL  step %s: %s\n' "$1" "$2"; }
chk() {
  local n=$1 what=$2
  shift 2
  if "$@"; then note "ok:     $what"; else note "NOT ok: $what"; BAD[$n]=1; fi
}
done_step() {
  if [ -z "${BAD[$1]:-}" ]; then pass "$1" "$2"; else bad "$1" "a check failed: $2"; fi
}

for tool in bitcoind bitcoin-cli jq; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH"
done
[ "$(command -v bitcoind)" = "$CORE_BIN/bitcoind" ] || die "bitcoind is not the one in $CORE_BIN"
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) die "this check is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

mkdir -p "$BASE/core0" "$BASE/core1" "$BASE/core2" "$DATA_DIR"
declare -a NODE_PID
EX_PID=
EX_LOG=
EX_N=0
LAST=

cleanup() {
  exec 3>&- 2>/dev/null || true
  if [ -n "$EX_PID" ] && kill -0 "$EX_PID" 2>/dev/null; then
    kill "$EX_PID" 2>/dev/null || true
    sleep 1
    kill -9 "$EX_PID" 2>/dev/null || true
  fi
  for n in 0 1 2; do
    if [ -n "${NODE_PID[$n]:-}" ]; then
      cli "$n" stop >/dev/null 2>&1 || true
    fi
  done
  for n in 0 1 2; do
    if [ -n "${NODE_PID[$n]:-}" ]; then
      for _ in $(seq 1 30); do
        kill -0 "${NODE_PID[$n]}" 2>/dev/null || break
        sleep 1
      done
      kill -9 "${NODE_PID[$n]}" 2>/dev/null || true
    fi
  done
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

if [ -z "${OWNER_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/ownerspend}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner-node --example owner_spend_regtest ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner-node --example owner_spend_regtest ||
      die "the build failed"
  fi
  OWNER_BIN="$TARGET/debug/examples/owner_spend_regtest"
fi
[ -x "$OWNER_BIN" ] || die "no example binary at $OWNER_BIN"

# ---- helpers for the example ------------------------------------------------

start_example() { # tag
  EX_LOG="$BASE/example-$1.log"
  : >"$EX_LOG"
  EX_N=0
  rm -f "$FIFO"
  mkfifo "$FIFO"
  exec 3<>"$FIFO"
  "$OWNER_BIN" "$DATA_DIR" "127.0.0.1:$P2P" "127.0.0.2:$P2P" "127.0.0.3:$P2P" \
    --without-tor --refund-depth 6 <&3 >"$EX_LOG" 2>&1 &
  EX_PID=$!
}
cmd() { # command [seconds]
  local before i
  EX_N=$((EX_N + 1))
  before=$(wc -l <"$EX_LOG")
  echo "$1" >&3
  for i in $(seq 1 $((${2:-30} * 4))); do
    if grep -q "^done $EX_N\$" "$EX_LOG"; then
      LAST=$(tail -n +$((before + 1)) "$EX_LOG")
      return 0
    fi
    kill -0 "$EX_PID" 2>/dev/null || break
    sleep 0.25
  done
  LAST=$(tail -n +$((before + 1)) "$EX_LOG")
  return 1
}
await() { # pattern seconds
  local i
  for i in $(seq 1 $(($2 * 4))); do
    grep -Eq -- "$1" "$EX_LOG" && return 0
    kill -0 "$EX_PID" 2>/dev/null || break
    sleep 0.25
  done
  grep -Eq -- "$1" "$EX_LOG"
}
stop_example() {
  echo quit >&3
  await "^stopped\$" 30 || return 1
  wait "$EX_PID"
  local rc=$?
  EX_PID=
  exec 3>&-
  return $rc
}
# Wait until the node is at Core's tip, has checked up to it and is ready.
await_synced() { # seconds
  local want i
  want=$(a getblockcount)
  for i in $(seq 1 $(($1 * 2))); do
    cmd status || return 1
    if grep -q "^status tip=$want checked=$want view=ready\$" <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
# Wait until a state line for the txid matches the pattern.
await_send() { # txid pattern seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    cmd states || return 1
    if grep -Eq "^send $1 $2 " <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
# The coins as "txid:vout sats confs", sorted: the node's, then Core's.
# Not run in a subshell: `cmd` counts. The result is in OURS.
OURS=
node_coins() {
  OURS=
  cmd coins || return 1
  OURS=$(sed -nE 's/^coin ([0-9a-f]+:[0-9]+) sats=([0-9]+) confs=([0-9]+)$/\1 \2 \3/p' <<<"$LAST" | sort)
}
core_coins() { # minconf
  a -rpcwallet=shop listunspent "$1" 9999999 |
    jq -r '.[] | "\(.txid):\(.vout) \(.amount * 100000000 | round) \(.confirmations)"' | sort
}
mempools_hold() { # txid
  local n
  for n in 0 1 2; do
    cli "$n" getrawmempool | jq -e --arg t "$1" 'index($t) != null' >/dev/null || return 1
  done
}
mempool_sizes() {
  local n out=
  for n in 0 1 2; do out="$out $(cli "$n" getmempoolinfo | jq .size)"; done
  echo "$out"
}
all_at() { # height seconds
  local n i
  for i in $(seq 1 $(($2 * 2))); do
    local ok=1
    for n in 0 1 2; do [ "$(cli "$n" getblockcount)" = "$1" ] || ok=0; done
    [ "$ok" = 1 ] && return 0
    sleep 0.5
  done
  return 1
}
mine() { # blocks
  a generatetoaddress "$1" "$MINE" >/dev/null
  all_at "$(a getblockcount)" 30
}
# same A B: not empty, and equal.
same() { [ -n "$1" ] && [ "$1" = "$2" ]; }
spent_txid() { sed -nE 's/^spent txid=([0-9a-f]{64})$/\1/p' <<<"$LAST" | head -1; }

# ---- the three Core nodes ---------------------------------------------------

for n in 0 1 2; do
  bitcoind -regtest -datadir="$BASE/core$n" -rpcport=$((RPC0 + n)) \
    -listen=1 -bind="127.0.0.$((n + 1)):$P2P" -listenonion=0 -daemonwait \
    -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
    -fallbackfee=0.0001 -dbcache=50 >/dev/null || die "bitcoind $n did not start"
  NODE_PID[$n]=$(cat "$BASE/core$n/regtest/bitcoind.pid")
done
cli 0 addnode "127.0.0.2:$P2P" onetry
cli 1 addnode "127.0.0.3:$P2P" onetry
cli 2 addnode "127.0.0.1:$P2P" onetry
a -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
a -named createwallet wallet_name=shop disable_private_keys=true blank=true >/dev/null ||
  die "no watch-only wallet"
MINE=$(a -rpcwallet=miner getnewaddress)
REFUND_TO=$(a -rpcwallet=miner getnewaddress)
a generatetoaddress 110 "$MINE" >/dev/null || die "could not mine"
all_at 110 60 || die "the nodes did not reach height 110"
say "Three nodes that serve filters, at height 110"

# ---- 1 ----------------------------------------------------------------------

say "1. The coins the node learns equal Core's listunspent"
start_example first
await "^started " 30 || die "the example did not start"
DESC=$(sed -nE 's/^descriptor (.*)$/\1/p' "$EX_LOG" | head -1)
[ -n "$DESC" ] || die "no descriptor from the example"
res=$(a -rpcwallet=shop importdescriptors \
  "[{\"desc\":\"$DESC\",\"timestamp\":0,\"active\":true,\"range\":[0,40]}]")
chk 1 "Core imported the wallet description" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]
cmd "receive 0" && ADDR0=$(sed -nE 's/^receive 0 (.*)$/\1/p' <<<"$LAST")
cmd "receive 1" && ADDR1=$(sed -nE 's/^receive 1 (.*)$/\1/p' <<<"$LAST")
core_derived=$(a deriveaddresses "$DESC" "[0,1]" | jq -r '.[0][]' 2>/dev/null | paste -sd' ')
chk 1 "Core and the node derive the same first two receive addresses" \
  [ "$core_derived" = "$ADDR0 $ADDR1" ]
a -rpcwallet=miner sendtoaddress "$ADDR0" 0.003 >/dev/null || die "no payment 0"
mine 1
a -rpcwallet=miner sendtoaddress "$ADDR1" 0.002 >/dev/null || die "no payment 1"
mine 5
if await_synced 90; then
  node_coins
  ours=$OURS
  theirs=$(core_coins 1)
  note "node: $(tr '\n' ';' <<<"$ours")"
  note "core: $(tr '\n' ';' <<<"$theirs")"
  chk 1 "two coins, equal to listunspent" same "$ours" "$theirs"
  chk 1 "there are two" [ "$(wc -l <<<"$ours")" = 2 ]
else
  note "status: $LAST"
  BAD[1]=1
fi
done_step 1 "the node's two coins equal listunspent (outpoints, amounts, confirmations)"

# ---- 2 ----------------------------------------------------------------------

say "2. A refund through the Key app and key 1, taken and sent"
cmd "batch 0 3"
cmd "refund 1 50000 $REFUND_TO"
ID1=$(sed -nE 's/^queued refund id=([0-9]+)$/\1/p' <<<"$LAST")
cmd "spend $RATE" 60
TX1=$(spent_txid)
note "$(grep -E '^(spent|refused|error|skipped|outcome)' <<<"$LAST" | head -6)"
[ -n "$TX1" ] || note "the example said: $(tail -8 "$EX_LOG" | tr '\n' '|')"
chk 2 "a transaction came out" [ -n "$TX1" ]
if [ -n "$TX1" ]; then
  raw=$(a getrawtransaction "$TX1" true 2>/dev/null || true)
  cmd states
  note "$(grep -E '^(send|entry|queue)' <<<"$LAST" | tr '\n' ';')"
  chk 2 "the refund entry is Sent" grep -q "^entry $ID1 Sent\$" <<<"$LAST"
  chk 2 "the node says it sent without Tor" grep -q 'sending without Tor' "$EX_LOG"
  chk 2 "it was handed over once" [ "$(grep -c "^spent txid=$TX1" "$EX_LOG")" = 1 ]
fi
done_step 2 "the refund was signed by both keys, taken and handed to the driver"

# ---- 3 ----------------------------------------------------------------------

say "3. The mempools and the peers' announcements"
if [ -n "$TX1" ]; then
  seen=0
  for _ in $(seq 1 60); do
    mempools_hold "$TX1" && seen=1 && break
    sleep 1
  done
  chk 3 "all three mempools hold the transaction" [ "$seen" = 1 ]
  await_send "$TX1" "seenback-[2-9]" 90
  chk 3 "the node's state is seen back from at least two groups" [ $? = 0 ]
  cmd "relay $TX1"
  note "$(grep '^relay' <<<"$LAST")"
else
  BAD[3]=1
fi
done_step 3 "the transaction is in three mempools and seen back by two groups or more"

# ---- 6 ----------------------------------------------------------------------

say "6. A restart before the block: no double spend, nothing signed again"
[ -n "$TX1" ] || BAD[6]=1
if [ -n "$TX1" ]; then
  stop_example || note "the example did not stop cleanly"
  start_example second
  await "^started " 30
  await_synced 90
  await_send "$TX1" "(sent|seenback-[0-9]+)" 60
  chk 6 "the transaction is held and followed again" [ $? = 0 ]
  cmd states
  chk 6 "one transaction in the outbox, no session open" grep -q '^queue outbox=1 sessions=0$' <<<"$LAST"
  chk 6 "the refund is still Sent" grep -q "^entry $ID1 Sent\$" <<<"$LAST"
  chk 6 "nothing was spent again" [ "$(cat "$BASE"/example-*.log | grep -c '^spent txid=')" = 1 ]
  sizes=$(mempool_sizes)
  note "mempool sizes:$sizes"
  chk 6 "each mempool holds exactly one transaction" [ "$sizes" = " 1 1 1" ]
fi
done_step 6 "after a restart the node follows the same transaction and signs nothing again"

# ---- 4 ----------------------------------------------------------------------

say "4. A block confirms it"
[ -n "$TX1" ] || BAD[4]=1
if [ -n "$TX1" ]; then
  mine 1
  await_send "$TX1" "confirmed-1" 60
  chk 4 "the state is confirmed, one deep" [ $? = 0 ]
  await_synced 60
  node_coins
  ours=$OURS
  theirs=$(core_coins 1)
  note "node: $(tr '\n' ';' <<<"$ours")"
  note "core: $(tr '\n' ';' <<<"$theirs")"
  chk 4 "the coins equal listunspent" same "$ours" "$theirs"
fi
done_step 4 "confirmed one deep, and the coin view equals listunspent"

# ---- 5 ----------------------------------------------------------------------

say "5. Done at the refund depth"
[ -n "$TX1" ] || BAD[5]=1
if [ -n "$TX1" ]; then
  mine 4
  await_send "$TX1" "confirmed-5" 60
  chk 5 "five deep is not done" [ $? = 0 ]
  mine 1
  await_send "$TX1" "done" 60
  chk 5 "six deep is done" [ $? = 0 ]
  cmd states
  chk 5 "the refund is Paid" grep -q "^entry $ID1 Paid\$" <<<"$LAST"
  chk 5 "the outbox is empty" grep -q '^queue outbox=0 ' <<<"$LAST"
fi
done_step 5 "the transaction is done at the refund depth and the refund is paid"

# ---- 7 ----------------------------------------------------------------------

say "7. A conflicting spend"
await_synced 60
cmd "refund 2 40000 $REFUND_TO"
ID2=$(sed -nE 's/^queued refund id=([0-9]+)$/\1/p' <<<"$LAST")
cmd "conflict 3 41000 $REFUND_TO $RATE" 60
HEX=$(sed -nE 's/^conflict ([0-9a-f]+)$/\1/p' <<<"$LAST" | head -1)
chk 7 "a conflicting transaction was made" [ -n "$HEX" ]
cmd "spend $RATE" 60
TX2=$(spent_txid)
chk 7 "the real transaction was made and sent" [ -n "$TX2" ]
if [ -n "$HEX" ] && [ -n "$TX2" ]; then
  res=$(a generateblock "$MINE" "[\"$HEX\"]" 2>&1)
  note "$(head -c 160 <<<"$res")"
  all_at "$(a getblockcount)" 30
  await_send "$TX2" "replaced" 60
  chk 7 "the state is replaced" [ $? = 0 ]
  cmd states
  note "$(grep -E '^(send|entry|queue)' <<<"$LAST" | tr '\n' ';')"
  chk 7 "the refund is back as Returned, not signed again" grep -q "^entry $ID2 Returned\$" <<<"$LAST"
  chk 7 "the outbox is empty and no session is open" grep -q '^queue outbox=0 sessions=0$' <<<"$LAST"
fi
done_step 7 "the transaction is replaced, its refund returned, nothing signed again"

# ---- 8 ----------------------------------------------------------------------

say "8. A one-block reorganization"
await_synced 60
cmd "refund 4 30000 $REFUND_TO"
cmd "spend $RATE" 60
TX3=$(spent_txid)
chk 8 "a third transaction was made" [ -n "$TX3" ]
if [ -n "$TX3" ]; then
  for _ in $(seq 1 60); do mempools_hold "$TX3" && break; sleep 1; done
  mine 1
  await_send "$TX3" "confirmed-1" 60
  chk 8 "it is confirmed one deep" [ $? = 0 ]
  HEIGHT=$(a getblockcount)
  BLOCK=$(a getblockhash "$HEIGHT")
  for n in 0 1 2; do cli "$n" invalidateblock "$BLOCK" >/dev/null; done
  a generateblock "$MINE" '[]' >/dev/null
  a generateblock "$MINE" '[]' >/dev/null
  all_at $((HEIGHT + 1)) 30
  chk 8 "the chain moved on from the old block" \
    [ "$(a getblockhash "$HEIGHT")" != "$BLOCK" ]
  await_synced 90
  await_send "$TX3" "(sent|seenback-[0-9]+)" 60
  chk 8 "the transaction is held again and sent again" [ $? = 0 ]
  sleep 2
  node_coins
  ours=$OURS
  theirs=$(core_coins 0)
  note "node: $(tr '\n' ';' <<<"$ours")"
  note "core: $(tr '\n' ';' <<<"$theirs")"
  chk 8 "the coins equal listunspent" same "$ours" "$theirs"
fi
done_step 8 "after one block left the chain the coin view equals Core's"

# ---- table ------------------------------------------------------------------

stop_example || true
say "Summary"
FAILED=0
for n in 1 2 3 4 5 6 7 8; do
  printf '%s  step %s: %s\n' "${RESULT[$n]:-MISSING}" "$n" "${MESSAGE[$n]:-}"
  [ "${RESULT[$n]:-}" = PASS ] || FAILED=1
done
exit $FAILED
