#!/usr/bin/env bash
# The counter node's receive path against Bitcoin Core, on regtest.
#
# Question: do the pieces of the counter's receive path work together? A
# sale is opened on an address from a signed batch of a real 2-of-3 wallet
# description, a payment from Core's wallet is seen, mined, buried and
# reorganized away, and a restart of the node shows the same states, repeats
# no report and does not ask for what it already checked.
#
# What it does (the steps are numbered as in the printed table):
#   1. Starts a private regtest node that serves compact filters and mines
#      150 blocks. Starts the example `counter_regtest` with that node as its
#      one fixed local peer. The node becomes able to open a sale.
#      THIS STEP IS NOT A CHECK OF THE HEALTH RULES. WR-50 and WR-51 ask for
#      peers in 3 network groups and 2 filter peers, and one local node is
#      one peer. The example is started with `--regtest-single-peer`, which
#      replaces the health answer with "healthy" in the example only. The
#      library is untouched, and the example prints what the real rules say.
#   2. `open` gives the first address of the wallet description as Core
#      derives it (the description is imported into a blank watch-only
#      wallet; `deriveaddresses` gives index 0).
#   3. Core's funded wallet pays the address exactly what the sale needs.
#      The sale shows Waiting: seen by one peer, not by three groups, so no
#      instant acceptance.
#   4. One block: Confirmed, and PosPaid is reported once.
#   5. Five more blocks: Final, and BooksFinal is reported once.
#   6. A second sale is paid and confirmed once. The block is invalidated
#      and two blocks that do not hold the payment are mined with
#      `generateblock <address> '[]'`: DoNotHandOver, and PosUnpaid is
#      reported once. (Two blocks, not one: a block of equal work does not
#      take the best chain from the block it replaces.)
#   7. The example is stopped and started again on the same directory: both
#      sales show the same states, no report is repeated, and the node does
#      not ask for old blocks. What Core received from the node is compared
#      with a start on a copy of the directory whose checked height was
#      deleted, which has to check every block from the first.
#   8. The blank watch-only wallet sees the first payment as its own coin:
#      the money is in the 2-of-3 wallet.
#   9. A third sale is paid and confirmed once, and the example is killed
#      at that moment with no clean stop. A start with no peer to ask
#      shows the sale Confirmed from the disk alone, a start with the peer
#      back shows the same, and nothing is reported again: the header of
#      a block is saved before the block is handed to the counter
#      (NR-73). (Run in the script before step 8.)
#  10. An own node as a second check (CR-100 to CR-108), with Core as the
#      own node. A payment of $20, over the light check's $10 and under the
#      own node's $25, waits on the light check; once the own node has it
#      too it is accepted at once, and the sale says which check was done
#      ("Paid (your node checked it)"). A payment of $30 is over both: it
#      waits, and the line says "Your node and light check".
#  11. Core is stopped: the own node is down. The $30 sale says "Light
#      check, not your node" and a $20 payment that the own node had not
#      reported stays waiting on the light check, each with words.
#  12. Core is started again: the sales return to the own-node check, and the
#      $20 payment is accepted once the own node reports it.
#      STEPS 10 TO 12 ARE A CHECK OF THE COUNTER'S WIRING OF THE OWN NODE,
#      NOT OF THE OWN NODE'S CONNECTION. The library reaches an own node only
#      through Tor, and `openbmt-net-io` lets no other crate use its
#      clear-net test constructor (NR-99), so the example is started with
#      `--regtest-own-node`: what the own node would say (`own up`,
#      `own down`, `own has <txid>`) is said by this script, from what Core
#      really holds (`own has` only for a transaction in Core's mempool, and
#      `own down` while Core is stopped). Two more stand-ins in the example:
#      each payment is offered as seen from three network groups, and the
#      owner's heartbeat is signed with the test owner key (`heartbeat self`).
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another), jq, and
# cargo unless NODE_BIN names a prebuilt `counter_regtest` example binary.
# A build goes through ~/.cache/openbmt/guard.sh when that file exists.
#
# Environment:
#   CORE_BIN        directory with bitcoind and bitcoin-cli
#   DRILL_DIR       scratch directory (default: a new directory under
#                   ~/.cache/openbmt)
#   DRILL_RPC_PORT  RPC port for the regtest node (default 18966)
#   DRILL_P2P_PORT  peer port for the regtest node (default 18967)
#   DRILL_DEAD_PORT a port on 127.0.0.1 that nothing listens on (18968)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#   NODE_BIN        the `counter_regtest` example binary
#
# Everything here is regtest on 127.0.0.1, with keys made from fixed test
# bytes. No real keys or funds are involved. The node and the scratch
# directory are removed on every exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_RPC_PORT:-18966}
P2P_PORT=${DRILL_P2P_PORT:-18967}
DEAD_PORT=${DRILL_DEAD_PORT:-18968}
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/counter-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/core"
DATA_DIR="$BASE/counter"
FRESH_DIR="$BASE/counter-fresh"
LOST_DIR="$BASE/counter-lost"
FIFO="$BASE/stdin"
SALE_AMOUNT=10000     # $100.00 in cents
SALE_RATE=10000000    # $100,000 per bitcoin, in cents

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
cli() { bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" "$@"; }
btc() { printf '%d.%08d' $(($1 / 100000000)) $(($1 % 100000000)); }

declare -a RESULT MESSAGE
pass() { RESULT[$1]=PASS; MESSAGE[$1]=$2; printf 'PASS  step %s: %s\n' "$1" "$2"; }
bad() { RESULT[$1]=FAIL; MESSAGE[$1]=$2; printf 'FAIL  step %s: %s\n' "$1" "$2"; }

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

mkdir -p "$NODE_DIR" "$DATA_DIR"
NODE_PID=
EX_PID=
EX_LOG=
EX_N=0
EX_FLAGS=
LAST=

cleanup() {
  exec 3>&- 2>/dev/null || true
  if [ -n "$EX_PID" ] && kill -0 "$EX_PID" 2>/dev/null; then
    kill "$EX_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$EX_PID" 2>/dev/null || break
      sleep 0.25
    done
    kill -9 "$EX_PID" 2>/dev/null || true
  fi
  if [ -n "$NODE_PID" ]; then
    bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" stop >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$NODE_PID" 2>/dev/null || true
  fi
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

if [ -z "${NODE_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/node}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-counter-node --example counter_regtest ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-counter-node --example counter_regtest ||
      die "the build failed"
  fi
  NODE_BIN="$TARGET/debug/examples/counter_regtest"
fi
[ -x "$NODE_BIN" ] || die "no example binary at $NODE_BIN"

# ---- helpers for the example ------------------------------------------------

# Start the example on a data directory. Sets EX_PID and EX_LOG; its standard
# input is a fifo that stays open on descriptor 3, so that commands can be
# sent.
start_example() { # tag, data dir, [peer]
  EX_LOG="$BASE/example-$1.log"
  : >"$EX_LOG"
  EX_N=0
  rm -f "$FIFO"
  mkfifo "$FIFO"
  exec 3<>"$FIFO"
  # shellcheck disable=SC2086
  "$NODE_BIN" "$2" "${3:-127.0.0.1:$P2P_PORT}" --regtest-single-peer $EX_FLAGS <&3 >"$EX_LOG" 2>&1 &
  EX_PID=$!
}
# Send one command and wait for its `done`. LAST holds what it printed.
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
    if ! kill -0 "$EX_PID" 2>/dev/null; then
      LAST=$(tail -n +$((before + 1)) "$EX_LOG")
      return 1
    fi
    sleep 0.25
  done
  LAST=$(tail -n +$((before + 1)) "$EX_LOG")
  return 1
}
# Wait for a line of the current log that matches the pattern.
await() { # pattern seconds
  local i
  for i in $(seq 1 $(($2 * 4))); do
    grep -Eq -- "$1" "$EX_LOG" && return 0
    if ! kill -0 "$EX_PID" 2>/dev/null; then break; fi
    sleep 0.25
  done
  grep -Eq -- "$1" "$EX_LOG"
}
# Ask for the state of a sale until it is the wanted one.
await_state() { # index state seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    cmd "judge $1" || return 1
    if grep -q "^sale $1 state=$2 " <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
# Sets STATE to the state of a sale. Not run in a subshell: `cmd` counts.
STATE=
state_of() { # index
  STATE=
  cmd "judge $1" || return 1
  STATE=$(sed -nE "s/^sale $1 state=([A-Za-z]+) .*/\1/p" <<<"$LAST")
}
# Wait until the node's view of the chain is at the height of Core's.
await_tip() { # seconds
  local want i
  want=$(cli getblockcount)
  for i in $(seq 1 $(($1 * 2))); do
    cmd status || return 1
    if grep -q "^status tip=$want .* peers=1\$" <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
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
# How many times a report line was printed, over every run so far.
reports() { # "PosPaid index=0"
  cat "$BASE"/example-*.log 2>/dev/null | grep -Ec -- "^report $1( |\$)" || true
}
# Bytes of one kind of message that Core received from its newest inbound
# peer, which is the node.
core_received() { # message
  cli getpeerinfo | jq --arg m "$1" \
    '[.[] | select(.inbound == true)] | sort_by(.id) | last | (.bytesrecv_per_msg[$m] // 0)'
}
inbound_peers() { cli getpeerinfo | jq '[.[] | select(.inbound == true)] | length'; }

# ---- Core -----------------------------------------------------------------------

core_start() {
  bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" \
    -listen=1 -bind="127.0.0.1:$P2P_PORT" -listenonion=0 -daemonwait \
    -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
    -fallbackfee=0.0001 -dbcache=50 >/dev/null || return 1
  NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
}
# Stop Core for good, as a crash of the machine would not: a clean stop
# that keeps its mempool on disk.
core_stop() {
  cli stop >/dev/null 2>&1 || return 1
  local i
  for i in $(seq 1 60); do
    kill -0 "$NODE_PID" 2>/dev/null || break
    sleep 0.5
  done
  ! kill -0 "$NODE_PID" 2>/dev/null
}
core_start || die "bitcoind did not start"

cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
cli -named createwallet wallet_name=shop disable_private_keys=true blank=true >/dev/null ||
  die "no watch-only wallet"
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 150 "$MINE" >/dev/null || die "could not mine"
[ "$(cli getblockcount)" = 150 ] || die "the node is not at height 150"
say "A node that serves filters, at height 150"

# ---- 1 -------------------------------------------------------------------------

say "1. The node can open a sale (NOT a check of the health rules)"
start_example first "$DATA_DIR"
DESC=
if await "^descriptor " 30; then
  DESC=$(sed -nE 's/^descriptor (.*)$/\1/p' "$EX_LOG" | head -1)
fi
if await "^warning THIS RUN IS NOT A CHECK OF THE HEALTH RULES" 5 && await_tip 90; then
  cmd health
  note "$(grep '^health ' <<<"$LAST")"
  note "the example said: $(grep -c '^warning ' "$EX_LOG") warning lines at start"
  if grep -q "^health used=healthy real=TooFewPeerGroups peers=1\$" <<<"$LAST"; then
    pass 1 "one ready peer, tip 150; answer 'healthy' by the single-peer switch while the real rules say TooFewPeerGroups (WR-50). Not a check of WR-50/WR-51."
  else
    bad 1 "unexpected health line: $(grep '^health ' <<<"$LAST")"
  fi
else
  bad 1 "the node did not reach the peer and the tip within 90 s"
fi
note "descriptor: $DESC"

# ---- 2 -------------------------------------------------------------------------

say "2. The first address is the wallet description's index 0"
ADDR0=
if [ -n "$DESC" ]; then
  res=$(cli -rpcwallet=shop importdescriptors \
    "[{\"desc\":\"$DESC\",\"timestamp\":0,\"active\":true,\"range\":[0,40]}]")
  if [ "$(jq -r 'all(.[]; .success)' <<<"$res")" != true ]; then
    bad 2 "Core did not take the wallet description: $res"
  else
    ADDR0=$(cli deriveaddresses "$DESC" "[0,0]" | jq -r '.[0][0]')
  fi
fi
cmd "open $SALE_AMOUNT $SALE_RATE"
note "$(grep -E '^(opened|refused)' <<<"$LAST")"
OPENED=$(grep '^opened ' <<<"$LAST" || true)
SHOWN0=$(sed -nE 's/.* address=([a-z0-9]+) .*/\1/p' <<<"$OPENED")
SATS0=$(sed -nE 's/.* sats=([0-9]+)$/\1/p' <<<"$OPENED")
if [ -n "${RESULT[2]:-}" ]; then
  :
elif [ -z "$ADDR0" ] || [ -z "$SHOWN0" ]; then
  bad 2 "no address from the example or from Core"
elif [ "$SHOWN0" = "$ADDR0" ] && grep -q ' index=0 ' <<<"$OPENED"; then
  pass 2 "sale 0 got $SHOWN0, Core's deriveaddresses index 0 of the same description"
else
  bad 2 "the example showed $SHOWN0, Core derives $ADDR0"
fi
note "the sale needs $SATS0 sats"

# ---- 3 -------------------------------------------------------------------------

say "3. A payment is seen: Waiting, seen by one peer"
TXID0=
if [ -n "$SHOWN0" ] && [ -n "$SATS0" ]; then
  TXID0=$(cli -rpcwallet=miner sendtoaddress "$SHOWN0" "$(btc "$SATS0")")
fi
if [ -n "$TXID0" ] && await_state 0 Waiting 45; then
  LINE0=$(grep '^sale 0 ' <<<"$LAST")
  note "$LINE0"
  if grep -q " received=$SATS0 needs=$SATS0 mode=None " <<<"$LINE0" &&
    [ "$(reports PosPaid)" = 0 ]; then
    pass 3 "$TXID0 paid exactly $SATS0 sats; Waiting, mode None (not seen by 3 groups), nothing reported"
  else
    bad 3 "state is Waiting but the line is not as expected: $LINE0"
  fi
else
  bad 3 "the sale did not show Waiting (last: $(grep '^sale 0 ' <<<"$LAST" | tail -1))"
fi

# ---- 4 -------------------------------------------------------------------------

say "4. One block: Confirmed, PosPaid once"
cli generatetoaddress 1 "$MINE" >/dev/null
if await_state 0 Confirmed 45; then
  cmd "pump 2"
  note "$(grep '^sale 0 ' <<<"$(cat "$EX_LOG")" | tail -1)"
  n=$(reports "PosPaid index=0")
  if [ "$n" = 1 ]; then
    pass 4 "Confirmed after one block; 'report PosPaid index=0' printed $n time"
  else
    bad 4 "Confirmed, but PosPaid index=0 was reported $n times"
  fi
else
  bad 4 "the sale did not show Confirmed"
fi

# ---- 5 -------------------------------------------------------------------------

say "5. Five more blocks: Final, BooksFinal once"
cli generatetoaddress 5 "$MINE" >/dev/null
if await_state 0 Final 45; then
  cmd "pump 2"
  paid=$(reports "PosPaid index=0")
  final=$(reports "BooksFinal index=0")
  if [ "$final" = 1 ] && [ "$paid" = 1 ]; then
    pass 5 "Final at 6 confirmations; BooksFinal printed $final time, PosPaid still $paid"
  else
    bad 5 "Final, but BooksFinal was reported $final times and PosPaid $paid times"
  fi
else
  bad 5 "the sale did not show Final"
fi

# ---- 6 -------------------------------------------------------------------------

say "6. A reorganization that takes the payment's block away"
cmd "open $SALE_AMOUNT $SALE_RATE"
OPENED=$(grep '^opened ' <<<"$LAST" || true)
note "$OPENED"
SHOWN1=$(sed -nE 's/.* address=([a-z0-9]+) .*/\1/p' <<<"$OPENED")
SATS1=$(sed -nE 's/.* sats=([0-9]+)$/\1/p' <<<"$OPENED")
TXID1=
if [ -n "$SHOWN1" ]; then
  TXID1=$(cli -rpcwallet=miner sendtoaddress "$SHOWN1" "$(btc "$SATS1")")
fi
STEP6=
if [ -n "$TXID1" ] && await_state 1 Waiting 45; then
  cli generatetoaddress 1 "$MINE" >/dev/null
  BLOCK1=$(cli getbestblockhash)
  if await_state 1 Confirmed 45; then
    cli invalidateblock "$BLOCK1"
    OTHER=$(cli -rpcwallet=miner getnewaddress)
    cli generateblock "$OTHER" '[]' >/dev/null
    cli generateblock "$OTHER" '[]' >/dev/null
    if cli getrawmempool | jq -e --arg t "$TXID1" 'index($t) != null' >/dev/null; then
      note "the payment is back in Core's mempool and was not mined again"
    else
      note "the payment is not in Core's mempool"
    fi
    if await_state 1 DoNotHandOver 60; then
      cmd "pump 2"
      note "$(grep '^sale 1 ' <<<"$(cat "$EX_LOG")" | tail -1)"
      unpaid=$(reports "PosUnpaid index=1")
      paid1=$(reports "PosPaid index=1")
      if [ "$unpaid" = 1 ] && [ "$paid1" = 1 ]; then
        STEP6=ok
        pass 6 "Confirmed, then DoNotHandOver after the block left the chain; PosPaid printed $paid1 time, PosUnpaid $unpaid time"
      else
        bad 6 "DoNotHandOver, but PosPaid index=1 was reported $paid1 times and PosUnpaid $unpaid times"
      fi
    else
      bad 6 "the second sale did not show DoNotHandOver after the reorganization"
    fi
  else
    bad 6 "the second sale did not show Confirmed before the reorganization"
  fi
else
  bad 6 "the second sale did not show Waiting"
fi

# ---- 7 -------------------------------------------------------------------------

say "7. Stop, start again on the same directory"
TIP=$(cli getblockcount)
state_of 0
S0=$STATE
state_of 1
S1=$STATE
note "before the stop: sale 0 $S0, sale 1 $S1; Core is at height $TIP"
FIRST_GETDATA=$(core_received getdata)
FIRST_CFILTERS=$(core_received getcfilters)
note "the first run: Core received $FIRST_GETDATA bytes of getdata and $FIRST_CFILTERS bytes of getcfilters from the node"
cmd status
CHECKED_BEFORE=$(sed -nE 's/^status tip=[0-9]+ checked=([0-9]+) .*/\1/p' <<<"$LAST")
REPORTS_BEFORE=$(cat "$BASE"/example-*.log | grep -c '^report ' || true)
if stop_example; then
  start_example second "$DATA_DIR"
  if await "^started " 20 && await_tip 60; then
    cmd "pump 6"
    state_of 0
    T0S=$STATE
    state_of 1
    T1S=$STATE
    note "$(grep '^started ' "$EX_LOG")"
    note "after the restart: sale 0 $T0S, sale 1 $T1S"
    SECOND_GETDATA=$(core_received getdata)
    SECOND_CFILTERS=$(core_received getcfilters)
    note "the second run: Core received $SECOND_GETDATA bytes of getdata and $SECOND_CFILTERS bytes of getcfilters from the node"
    REPORTS_SECOND=$(grep -c '^report ' "$EX_LOG" || true)
    REPORTS_FIRST=$(grep -c '^report ' "$BASE/example-first.log" || true)
    note "reports printed: $REPORTS_FIRST in the first run, $REPORTS_SECOND in the second"
    # A fresh start has to check every block of the chain, and asks for each
    # of them with a getdata of 61 bytes (24 of header, 1 of count, 36 of
    # one item). The restart passes if it asks for at most five things
    # (blocks or transactions) and for no filter.
    LIMIT=$((5 * 61))
    ok=1
    [ "$T0S" = "$S0" ] && [ "$T1S" = "$S1" ] || { ok=0; note "the states differ"; }
    [ "$REPORTS_SECOND" = 0 ] || { ok=0; note "the second run printed reports"; }
    [ "$SECOND_GETDATA" -le "$LIMIT" ] || { ok=0; note "getdata $SECOND_GETDATA is above $LIMIT"; }
    [ "$SECOND_CFILTERS" = 0 ] || { ok=0; note "getcfilters were asked"; }
    [ "$(inbound_peers)" = 1 ] || { ok=0; note "Core sees $(inbound_peers) inbound peers"; }
    if [ "$ok" = 1 ]; then
      pass 7 "same states (sale 0 $T0S, sale 1 $T1S), no report in the second run, checked height $CHECKED_BEFORE kept, getdata $SECOND_GETDATA bytes against $FIRST_GETDATA in the first run, getcfilters $SECOND_CFILTERS"
    else
      bad 7 "restart: states $T0S/$T1S against $S0/$S1, reports $REPORTS_SECOND, getdata $SECOND_GETDATA, getcfilters $SECOND_CFILTERS"
    fi
    stop_example || note "the second run did not stop cleanly"

    # What a start sends when it does not know how far it has checked: a
    # copy of the directory with the checked height deleted. The addresses
    # that were shown are watched from the first block.
    cp -r "$DATA_DIR" "$LOST_DIR"
    rm -f "$LOST_DIR/checked.bin"
    start_example lost "$LOST_DIR"
    if await "^started " 20 && await_tip 90; then
      cmd "pump 6"
      LOST_GETDATA=$(core_received getdata)
      LOST_CFILTERS=$(core_received getcfilters)
      note "$(grep '^started ' "$EX_LOG")"
      note "no checked height, same chain (height $TIP): Core received $LOST_GETDATA bytes of getdata and $LOST_CFILTERS bytes of getcfilters"
      note "that is about $((LOST_GETDATA / 61)) things asked for; the restart asked for about $((SECOND_GETDATA / 61)) and the first run for about $((FIRST_GETDATA / 61))"
      note "reports printed in that run: $(grep -c '^report ' "$EX_LOG" || true)"
    else
      note "the run without a checked height did not reach the tip"
    fi
    stop_example || note "that run did not stop cleanly"

    # An empty directory watches nothing at first, so it asks for no block.
    start_example fresh "$FRESH_DIR"
    if await "^started " 20 && await_tip 90; then
      cmd "pump 4"
      note "an empty directory, same chain: Core received $(core_received getdata) bytes of getdata and $(core_received getcfilters) bytes of getcfilters"
    fi
    stop_example || note "the fresh run did not stop cleanly"
  else
    bad 7 "the second run did not start or reach the tip"
    stop_example || true
  fi
else
  bad 7 "the first run did not stop"
fi

# ---- 9 -------------------------------------------------------------------------

say "9. The node is killed right after a confirmation"
start_example third "$DATA_DIR"
STEP9=
if await "^started " 20 && await_tip 60; then
  cmd "open $SALE_AMOUNT $SALE_RATE"
  OPENED=$(grep '^opened ' <<<"$LAST" || true)
  note "$OPENED"
  SHOWN2=$(sed -nE 's/.* address=([a-z0-9]+) .*/\1/p' <<<"$OPENED")
  SATS2=$(sed -nE 's/.* sats=([0-9]+)$/\1/p' <<<"$OPENED")
  TXID2=
  if [ -n "$SHOWN2" ]; then
    TXID2=$(cli -rpcwallet=miner sendtoaddress "$SHOWN2" "$(btc "$SATS2")")
  fi
  if [ -n "$TXID2" ] && await_state 2 Waiting 45; then
    cli generatetoaddress 1 "$MINE" >/dev/null
    if await_state 2 Confirmed 45; then
      # No clean stop: nothing is saved on the way out.
      kill -9 "$EX_PID"
      wait "$EX_PID" 2>/dev/null
      EX_PID=
      exec 3>&-
      # First with a peer address nothing listens on, so that the answer
      # comes from what was on disk at the kill and from nothing else.
      start_example alone "$DATA_DIR" "127.0.0.1:$DEAD_PORT"
      AT_ONCE=
      if await "^started " 20; then
        cmd "pump 2"
        state_of 2
        AT_ONCE=$STATE
        note "$(grep '^sale 2 ' <<<"$LAST")"
      fi
      stop_example || note "the run without a peer did not stop cleanly"
      start_example fourth "$DATA_DIR"
      if await "^started " 20; then
        await_tip 60
        cmd "pump 4"
        state_of 2
        LATER=$STATE
        paid2=$(reports "PosPaid index=2")
        unpaid2=$(reports "PosUnpaid index=2")
        note "after the kill: sale 2 $AT_ONCE with no peer, $LATER with the peer back; PosPaid printed $paid2 time, PosUnpaid $unpaid2 times"
        if [ "$AT_ONCE" = Confirmed ] && [ "$LATER" = Confirmed ] &&
          [ "$paid2" = 1 ] && [ "$unpaid2" = 0 ]; then
          STEP9=ok
          pass 9 "killed with no clean stop right after the confirmation; a start with no peer shows Confirmed from the disk alone, and so does a start with the peer back; PosPaid still $paid2, PosUnpaid $unpaid2"
        fi
      fi
      stop_example || note "the run after the kill did not stop cleanly"
    fi
  fi
fi
[ -n "$STEP9" ] || bad 9 "a kill right after a confirmation did not leave the sale Confirmed with one PosPaid and no PosUnpaid"

# ---- 10, 11, 12 ------------------------------------------------------------------

# Open a sale of some cents and pay it from Core. Sets OPEN_INDEX, OPEN_TXID.
OPEN_INDEX=
OPEN_TXID=
open_and_pay() { # cents
  local o addr sats
  OPEN_INDEX=
  OPEN_TXID=
  cmd "open $1 $SALE_RATE" || return 1
  o=$(grep '^opened ' <<<"$LAST" || true)
  note "$o"
  OPEN_INDEX=$(sed -nE 's/^opened index=([0-9]+) .*/\1/p' <<<"$o")
  addr=$(sed -nE 's/.* address=([a-z0-9]+) .*/\1/p' <<<"$o")
  sats=$(sed -nE 's/.* sats=([0-9]+)$/\1/p' <<<"$o")
  [ -n "$OPEN_INDEX" ] && [ -n "$addr" ] && [ -n "$sats" ] || return 1
  OPEN_TXID=$(cli -rpcwallet=miner sendtoaddress "$addr" "$(btc "$sats")") || return 1
}
# The own node says it has a transaction, but only one that Core really has.
own_has() { # txid
  cli getmempoolentry "$1" >/dev/null 2>&1 || return 1
  cmd "own has $1"
}
# One judged line of a sale, in LAST and in SALE_LINE.
SALE_LINE=
sale_line() { # index
  cmd "judge $1" || return 1
  SALE_LINE=$(grep "^sale $1 " <<<"$LAST" | tail -1)
}

say "10. Own node and light check: the tier that was checked is the one named"
EX_FLAGS="--regtest-own-node --customers-off-wifi"
start_example own "$DATA_DIR"
S10= S11= S12=
IDX_A= IDX_B= IDX_C= TX_A= TX_B= TX_C=
if await "^started " 20 && await_tip 60; then
  cmd limits
  LIMITS=$(grep '^limits ' <<<"$LAST")
  note "$LIMITS"
  cmd "heartbeat self"
  note "$(grep '^heartbeat ' <<<"$LAST")"
  if open_and_pay 2000; then IDX_A=$OPEN_INDEX TX_A=$OPEN_TXID; fi
  if open_and_pay 3000; then IDX_B=$OPEN_INDEX TX_B=$OPEN_TXID; fi
  if open_and_pay 2000; then IDX_C=$OPEN_INDEX TX_C=$OPEN_TXID; fi
  if [ -n "$TX_A" ] && [ -n "$TX_B" ] && [ -n "$TX_C" ] &&
    await_state "$IDX_A" Waiting 45 && await_state "$IDX_B" Waiting 45 && await_state "$IDX_C" Waiting 45; then
    # The own node has said nothing yet: the light check, over its $10.
    sale_line "$IDX_A"
    note "before the own node says anything: $SALE_LINE"
    LIGHT_FIRST=0
    grep -q " mode=Some(Light) check=Light own_saw=false own_counted=false " <<<"$SALE_LINE" && LIGHT_FIRST=1
    cmd "own up"
    own_has "$TX_A"
    own_has "$TX_B"
    ok=1
    await_state "$IDX_A" PaidInstant 30 || ok=0
    sale_line "$IDX_A"
    note "$SALE_LINE"
    grep -q " mode=Some(NodeAlsoSeenByLight) " <<<"$SALE_LINE" || { ok=0; note "sale A is not on the own node's check"; }
    grep -q "Paid (your node checked it)" <<<"$SALE_LINE" || { ok=0; note "sale A is not labelled as the own node's"; }
    sale_line "$IDX_B"
    note "$SALE_LINE"
    grep -q "^sale $IDX_B state=Waiting .* check=YourNodeAndLight own_saw=true own_counted=true " <<<"$SALE_LINE" ||
      { ok=0; note "sale B (over both limits) is not waiting on the node and the light check"; }
    grep -q '^check Your node and light check: ' <<<"$LAST" || { ok=0; note "no words for the check of sale B"; }
    cmd "own status"
    OWN_STATUS=$(grep '^own ' <<<"$LAST")
    note "$OWN_STATUS"
    grep -Eq "^own usable=true .* asked=[1-9]" <<<"$OWN_STATUS" || { ok=0; note "the node did not ask the own node about the payments, or the own node is not usable"; }
    grep -q "^limits light_per_sale_usd=10 node_per_sale_usd=25\$" <<<"$LIMITS" || { ok=0; note "the limits are not \$10 and \$25"; }
    [ "$LIGHT_FIRST" = 1 ] || { ok=0; note "sale A did not first show the light check"; }
    if [ "$ok" = 1 ]; then
      S10=ok
      pass 10 "a \$20 payment waited on the light check (limit \$10), was accepted once the own node had it too (limit \$25) as \"Paid (your node checked it)\", mode NodeAlsoSeenByLight; a \$30 payment waited, with \"Your node and light check\"; the node asked the own node"
    fi
  fi
fi
[ -n "$S10" ] || bad 10 "the own node's check was not shown as expected (see the notes above)"

say "11. Core stopped: the own node is down, the light check is named"
if [ -n "$S10" ] && core_stop; then
  cmd "own down"
  ok=1
  sale_line "$IDX_B"
  note "$SALE_LINE"
  grep -q "^sale $IDX_B state=Waiting .* check=LightOwnNodeUnusable own_saw=true own_counted=false " <<<"$SALE_LINE" ||
    { ok=0; note "sale B does not say the own node could not be used"; }
  grep -q '^check Light check, not your node: ' <<<"$LAST" || { ok=0; note "no words for the fallback on sale B"; }
  sale_line "$IDX_C"
  note "$SALE_LINE"
  grep -q "^sale $IDX_C state=Waiting .* mode=Some(Light) check=Light own_saw=false own_counted=false " <<<"$SALE_LINE" ||
    { ok=0; note "sale C is not waiting on the light check"; }
  cmd "own status"
  OWN_STATUS=$(grep '^own ' <<<"$LAST")
  note "$OWN_STATUS"
  grep -q "^own usable=false heading=" <<<"$OWN_STATUS" || { ok=0; note "the own node's line does not say it is down"; }
  if [ "$ok" = 1 ]; then
    S11=ok
    pass 11 "with Core stopped the own node's line says it is down; sale B says \"Light check, not your node\", sale C waits on the light check"
  fi
fi
[ -n "$S11" ] || bad 11 "the fallback to the light check was not shown as expected (see the notes above)"

say "12. Core started again: back on the own node's check"
if [ -n "$S11" ] && core_start; then
  # The wallets load in the background after a start; load them if they are not.
  for w in miner shop; do
    for _ in $(seq 1 40); do
      cli listwallets 2>/dev/null | jq -e --arg w "$w" 'index($w) != null' >/dev/null && break
      cli loadwallet "$w" >/dev/null 2>&1
      sleep 0.5
    done
  done
  cmd "own up"
  ok=1
  own_has "$TX_C" || { ok=0; note "Core does not hold the payment of sale C after the restart"; }
  await_state "$IDX_C" PaidInstant 30 || ok=0
  sale_line "$IDX_C"
  note "$SALE_LINE"
  grep -q " mode=Some(NodeAlsoSeenByLight) " <<<"$SALE_LINE" && grep -q "Paid (your node checked it)" <<<"$SALE_LINE" ||
    { ok=0; note "sale C is not accepted on the own node's check"; }
  sale_line "$IDX_B"
  note "$SALE_LINE"
  grep -q " check=YourNodeAndLight own_saw=true own_counted=true " <<<"$SALE_LINE" ||
    { ok=0; note "sale B is not back on the own node's check"; }
  if [ "$ok" = 1 ]; then
    S12=ok
    pass 12 "Core back: sale C accepted as \"Paid (your node checked it)\", sale B is on the node and the light check again"
  fi
fi
[ -n "$S12" ] || bad 12 "the return to the own node's check was not shown as expected (see the notes above)"
stop_example || note "the own node run did not stop cleanly"
EX_FLAGS=

# ---- 8 -------------------------------------------------------------------------

say "8. The watch-only wallet sees the first payment"
if [ -n "$SHOWN0" ] && [ -n "$SATS0" ]; then
  coin=$(cli -rpcwallet=shop listunspent | jq -c --arg a "$SHOWN0" --arg t "$TXID0" \
    '[.[] | select(.address == $a and .txid == $t)] | first // empty')
  note "listunspent on the watch-only wallet: ${coin:-nothing}"
  if [ -n "$coin" ] && [ "$(jq -r '.amount * 100000000 | round' <<<"$coin")" = "$SATS0" ]; then
    pass 8 "the coin $TXID0 of $SATS0 sats at $SHOWN0 is in the watch-only wallet of the 2-of-3 description"
  else
    bad 8 "the watch-only wallet does not show the first payment"
  fi
else
  bad 8 "there was no first payment"
fi

# ---- result -------------------------------------------------------------------

say "Result"
failed=0
for n in 1 2 3 4 5 6 7 8 9 10 11 12; do
  printf '%s  step %s\n' "${RESULT[$n]:-FAIL}" "$n"
  [ "${RESULT[$n]:-FAIL}" = PASS ] || failed=1
done
if [ "$failed" = 0 ]; then
  echo "ALL PASS (step 1 is not a check of the health rules; steps 10 to 12 check the counter's wiring of the own node, not the own node's connection)"
else
  echo "SOME STEPS FAILED"
  exit 1
fi
