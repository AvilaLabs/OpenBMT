#!/usr/bin/env bash
# The owner node's live watch and deep check against Bitcoin Core, on regtest.
#
# Question: when the owner's wallet has issued more addresses than its live
# watch holds, does the node set the oldest quiet ones aside, keep every
# address with a coin, miss a payment to a set-aside address until a deep
# check, and then find it, record it as a coin and watch the address again
# (`docs/OWNER-COINS-RULES.md`, OC-22 to OC-27)?
#
# Three Bitcoin Core nodes (127.0.0.1, 127.0.0.2, 127.0.0.3) serve compact
# filters, as in `scripts/owner-spend-e2e-core.sh`, and the example
# `owner_spend_regtest` is the owner node with those nodes as its fixed
# peers. The live watch's bound is shrunk to 50 scripts (`--live-bound`), the
# quiet time to one hour and the time between deep checks to 30 days; the
# default numbers are not changed. The example's `clock` command moves the
# node's own clock forward, which ages the addresses; the nodes and the
# chain keep real time.
#
# What it does (the steps are numbered as in the printed table):
#   1. The node issues 60 receive addresses. Core pays address 10 and the
#      node's coin equals Core's `listunspent`. Nothing is set aside: the
#      addresses are not quiet yet, whatever the bound.
#   2. The node's clock moves two hours on. The oldest quiet addresses leave
#      the live watch: 50 are set aside, which is what brings the live watch
#      to its bound of 50. Address 10 holds a coin and stays; address 50
#      leaves in its place. The line about old addresses says so and that
#      no check has run.
#   3. Core pays set-aside address 3. The live watch does not see it: the
#      node's coins are one, Core's are two.
#   4. The owner asks for a deep check. It runs on a second driver, finds the
#      payment, and the node's coins equal Core's listunspent. Address 3 is
#      watched again, 49 stay set aside, and the line says the last check was
#      today.
#   5. The example is stopped and started again on the same directory: the
#      set aside and the schedule are as they were, nothing is lost, and the
#      coins still equal Core's.
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
#   DRILL_RPC_PORT  first RPC port; the next two follow (default 19180)
#   DRILL_P2P_PORT  peer port on the three loopback addresses (19190)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#   OWNER_BIN       the `owner_spend_regtest` example binary
#
# Everything here is regtest on 127.0.0.x, with keys made from fixed test
# bytes. No real keys or funds are involved. The nodes and the scratch
# directory are removed on every exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC0=${DRILL_RPC_PORT:-19180}
P2P=${DRILL_P2P_PORT:-19190}
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/owner-watch-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DATA_DIR="$BASE/owner"
FIFO="$BASE/stdin"
BOUND=50
QUIET=3600
DEEP=2592000

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
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/ownerwatch}
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
    --without-tor --refund-depth 6 --live-bound "$BOUND" --quiet-secs "$QUIET" \
    --deep-secs "$DEEP" <&3 >"$EX_LOG" 2>&1 &
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

say "1. Sixty addresses issued, one paid: nothing is set aside yet"
start_example first
await "^started " 30 || die "the example did not start"
DESC=$(sed -nE 's/^descriptor (.*)$/\1/p' "$EX_LOG" | head -1)
[ -n "$DESC" ] || die "no descriptor from the example"
res=$(a -rpcwallet=shop importdescriptors \
  "[{\"desc\":\"$DESC\",\"timestamp\":0,\"active\":true,\"range\":[0,90]}]")
chk 1 "Core imported the wallet description" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]
cmd "batch 0 60"
cmd "receive 3" && ADDR3=$(sed -nE 's/^receive 3 (.*)$/\1/p' <<<"$LAST")
cmd "receive 10" && ADDR10=$(sed -nE 's/^receive 10 (.*)$/\1/p' <<<"$LAST")
[ -n "$ADDR3" ] && [ -n "$ADDR10" ] || die "no addresses from the example"
a -rpcwallet=miner sendtoaddress "$ADDR10" 0.002 >/dev/null || die "no payment to 10"
mine 6
if await_synced 90; then
  node_coins
  ours=$OURS
  theirs=$(core_coins 1)
  note "node: $(tr '\n' ';' <<<"$ours")"
  note "core: $(tr '\n' ';' <<<"$theirs")"
  chk 1 "one coin, equal to listunspent" same "$ours" "$theirs"
  cmd watch
  note "$(grep '^watch' <<<"$LAST")"
  chk 1 "60 issued and 20 of gap and 20 of change are watched: 100, none set aside" \
    grep -q '^watch live=100 aside=0$' <<<"$LAST"
  cmd old
  chk 1 "there is no line about old addresses" grep -q '^old none$' <<<"$LAST"
else
  note "status: $LAST"
  BAD[1]=1
fi
done_step 1 "nothing is set aside before the addresses are quiet, and the coin equals listunspent"

# ---- 2 ----------------------------------------------------------------------

say "2. Two hours on: the oldest quiet addresses leave the live watch"
cmd "clock 7200"
SET=0
for _ in $(seq 1 60); do
  cmd watch || break
  if grep -q '^watch live=50 aside=50$' <<<"$LAST"; then SET=1; break; fi
  sleep 0.5
done
note "$(grep '^watch' <<<"$LAST")"
chk 2 "the live watch is at its bound of 50 and 50 are set aside" [ "$SET" = 1 ]
cmd old
note "$(grep '^old' <<<"$LAST")"
chk 2 "the line says 50 old addresses and that no check has run" \
  grep -q '^old first count=50 ' <<<"$LAST"
await_synced 60
node_coins
ours=$OURS
theirs=$(core_coins 1)
chk 2 "the coin at address 10 is still held, equal to listunspent" same "$ours" "$theirs"
chk 2 "no check ran by itself" [ "$(grep -c 'checking .* old addresses' "$EX_LOG")" = 0 ]
done_step 2 "50 quiet addresses are set aside, the one with a coin stays, the line says so"

# ---- 3 ----------------------------------------------------------------------

say "3. A payment to a set-aside address is not seen by the live watch"
a -rpcwallet=miner sendtoaddress "$ADDR3" 0.004 >/dev/null || die "no payment to 3"
mine 6
await_synced 90
node_coins
ours=$OURS
theirs=$(core_coins 1)
note "node: $(tr '\n' ';' <<<"$ours")"
note "core: $(tr '\n' ';' <<<"$theirs")"
chk 3 "Core holds two coins" [ "$(wc -l <<<"$theirs")" = 2 ]
chk 3 "the node holds one: the payment is not seen yet" [ "$(wc -l <<<"$ours")" = 1 ]
cmd watch
chk 3 "still 50 set aside" grep -q '^watch live=50 aside=50$' <<<"$LAST"
done_step 3 "the node does not see the payment to the set-aside address before a deep check"

# ---- 4 ----------------------------------------------------------------------

say "4. The deep check finds it"
cmd deepcheck
chk 4 "the check was asked for" grep -q '^deepcheck asked=true$' <<<"$LAST"
DONE4=0
for _ in $(seq 1 240); do
  cmd old || break
  if grep -q '^old monthly ' <<<"$LAST"; then DONE4=1; break; fi
  sleep 0.5
done
note "$(grep '^old' <<<"$LAST")"
chk 4 "the check finished and the line says monthly, 49 set aside, last check today" \
  grep -Eq '^old monthly count=49 .* days=0$' <<<"$LAST"
chk 4 "the node found a payment to an old address" \
  grep -q 'a payment to an old address was found' "$EX_LOG"
await_synced 90
node_coins
ours=$OURS
theirs=$(core_coins 1)
note "node: $(tr '\n' ';' <<<"$ours")"
note "core: $(tr '\n' ';' <<<"$theirs")"
chk 4 "the coins equal listunspent: two" same "$ours" "$theirs"
chk 4 "there are two" [ "$(wc -l <<<"$ours")" = 2 ]
cmd watch
chk 4 "address 3 is watched again: 51 live, 49 set aside" \
  grep -q '^watch live=51 aside=49$' <<<"$LAST"
done_step 4 "the deep check found the payment, it is a coin, and the address is watched again"

# ---- 5 ----------------------------------------------------------------------

say "5. A restart: the set aside and the schedule are kept"
stop_example || note "the example did not stop cleanly"
start_example second
await "^started " 30
await_synced 90
cmd watch
chk 5 "51 live, 49 set aside, as before" grep -q '^watch live=51 aside=49$' <<<"$LAST"
cmd old
note "$(grep '^old' <<<"$LAST")"
chk 5 "the line still says 49 old addresses, checked monthly" \
  grep -q '^old monthly count=49 ' <<<"$LAST"
node_coins
ours=$OURS
theirs=$(core_coins 1)
chk 5 "the coins still equal listunspent" same "$ours" "$theirs"
done_step 5 "after a restart nothing is lost and the schedule is as it was"

# ---- table ------------------------------------------------------------------

stop_example || true
say "Summary"
FAILED=0
for n in 1 2 3 4 5; do
  printf '%s  step %s: %s\n' "${RESULT[$n]:-MISSING}" "$n" "${MESSAGE[$n]:-}"
  [ "${RESULT[$n]:-}" = PASS ] || FAILED=1
done
exit $FAILED
