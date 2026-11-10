#!/usr/bin/env bash
# The owner's signed heartbeat and instant acceptance, against Bitcoin Core
# on regtest.
#
# Question: does the counter node take the owner watcher's signed heartbeat
# (CR-50 to CR-56), refuse the wrong ones, age it, and judge its tip against
# the counter's own header chain, with two real nodes on the two sides?
#
# Two private regtest nodes, A and B, both serving compact filters. The
# counter example (`counter_regtest`) is the peer of A; the owner example
# (`owner_regtest`, `openbmt-owner-node`) is the peer of B. A and B are joined
# for the first steps and parted for the last. The script carries the owner's
# heartbeat bytes to the counter as hex; the transport between the two
# devices is not part of what is checked.
#
# THE HEALTH RULES ARE NOT CHECKED HERE, AND INSTANT ACCEPTANCE IS NOT
# REACHED. On one machine every peer is on a loopback address, and
# `openbmt_watch::network_group` puts every 127.x.x.x address (and every
# private one) into the one "local" group (WR-41). The rules ask for peers
# in 3 groups (WR-50), and the watcher counts a payment as seen only when 3
# groups reported it (WR-44); the manager would also refuse a second peer in
# the same group (NR-20). Nodes on 127.0.0.1, 127.1.0.1 and 127.2.0.1 are
# therefore still one group. The grouping rule is not changed here. Both
# examples are started with `--regtest-single-peer`, which replaces the
# health answer with "healthy" in the examples only, and prints what the real
# rules say. With the switch, WR-50 to WR-53 are all not exercised. A payment
# is seen from one group, so the instant decision is never asked and the
# sale-level steps (2b, 3b, 4, 6b) cannot pass; they are marked FAIL with the
# exact reason, and the same decisions are checked with three groups in the
# node's unit tests (crates/openbmt-counter-node/src/tests/heartbeat.rs).
#
# The steps (printed as a table at the end):
#   1.  Both examples healthy by the real rules. FAIL here: one group.
#   1b. Both examples ready under the switch, on the same chain.
#   2a. No heartbeat has been taken: the counter says so.
#   2b. A paid sale shows Waiting with the "no independent watcher" reason.
#   3a. A fresh heartbeat from the owner watcher is taken and verified.
#   3b. A new paid sale shows PaidInstant.
#   4.  PosPaid reported once, exposure on the ledger, released at the first
#       confirmation.
#   5.  A replayed heartbeat, one signed by the approver key and one naming
#       another counter are all refused, each for its own reason.
#   6a. With the signed age limit at its minimum of 60 seconds, the taken
#       heartbeat really ages past it (the script waits).
#   6b. A new paid sale shows Waiting again with the no-watcher reason.
#   7.  The owner watcher sits on a node that was parted from A. Its tip is
#       not held by the counter: "not verified" and not an alarm. After A
#       mines a block of its own, the heartbeat names another block at a
#       height the counter holds: "not verified" and logged as a
#       disagreement.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (put first on
# PATH; set CORE_BIN to use another), jq, and cargo unless COUNTER_BIN and
# OWNER_BIN name prebuilt example binaries. A build goes through
# ~/.cache/openbmt/guard.sh when that file exists.
#
# Environment:
#   CORE_BIN       directory with bitcoind and bitcoin-cli
#   DRILL_DIR      scratch directory (default: a new one under ~/.cache/openbmt)
#   DRILL_PORTS    first of four free ports on 127.0.0.1 (default 18986): RPC
#                  and peer port of A, RPC and peer port of B
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   COUNTER_BIN    the `counter_regtest` example binary
#   OWNER_BIN      the `owner_regtest` example binary
#
# Everything here is regtest on 127.0.0.1, with keys made from fixed test
# bytes. No real keys or funds are involved. The nodes and the scratch
# directory are removed on every exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
P0=${DRILL_PORTS:-18986}
A_RPC=$P0
A_P2P=$((P0 + 1))
B_RPC=$((P0 + 2))
B_P2P=$((P0 + 3))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/instant-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
A_DIR="$BASE/core-a"
B_DIR="$BASE/core-b"
C_DATA="$BASE/counter"
O_DATA="$BASE/owner"
SALE_AMOUNT=500       # $5.00 in cents, 5,000 sats at the rate below
SALE_RATE=10000000    # $100,000 per bitcoin, in cents
MAX_AGE=60            # the signed heartbeat age limit, at its minimum

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
cli_a() { bitcoin-cli -regtest -datadir="$A_DIR" -rpcport="$A_RPC" "$@"; }
cli_b() { bitcoin-cli -regtest -datadir="$B_DIR" -rpcport="$B_RPC" "$@"; }
btc() { printf '%d.%08d' $(($1 / 100000000)) $(($1 % 100000000)); }

declare -A RESULT MESSAGE
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

mkdir -p "$A_DIR" "$B_DIR" "$C_DATA" "$O_DATA"
A_PID=
B_PID=
declare -A PID LOG FDN COUNT
PID[c]=; PID[o]=
FDN[c]=3; FDN[o]=4
COUNT[c]=0; COUNT[o]=0
LAST=

stop_core() { # cli-function pid
  local pid=$2
  [ -n "$pid" ] || return 0
  "$1" stop >/dev/null 2>&1 || true
  for _ in $(seq 1 30); do
    kill -0 "$pid" 2>/dev/null || break
    sleep 1
  done
  kill -9 "$pid" 2>/dev/null || true
}

cleanup() {
  exec 3>&- 4>&- 2>/dev/null || true
  for who in c o; do
    local pid=${PID[$who]:-}
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      for _ in $(seq 1 20); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.25
      done
      kill -9 "$pid" 2>/dev/null || true
    fi
  done
  stop_core cli_a "$A_PID"
  stop_core cli_b "$B_PID"
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

build() { # package example
  local target=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/instant}
  local guard=$HOME/.cache/openbmt/guard.sh
  if [ -x "$guard" ]; then
    "$guard" env CARGO_TARGET_DIR="$target" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p "$1" --example "$2" || die "the build of $2 failed"
  else
    CARGO_TARGET_DIR="$target" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p "$1" --example "$2" || die "the build of $2 failed"
  fi
}
if [ -z "${COUNTER_BIN:-}" ] || [ -z "${OWNER_BIN:-}" ]; then
  say "Build the examples"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/instant}
  [ -n "${COUNTER_BIN:-}" ] || { build openbmt-counter-node counter_regtest; COUNTER_BIN="$TARGET/debug/examples/counter_regtest"; }
  [ -n "${OWNER_BIN:-}" ] || { build openbmt-owner-node owner_regtest; OWNER_BIN="$TARGET/debug/examples/owner_regtest"; }
fi
[ -x "$COUNTER_BIN" ] || die "no counter example binary at $COUNTER_BIN"
[ -x "$OWNER_BIN" ] || die "no owner example binary at $OWNER_BIN"

# ---- helpers for the examples ---------------------------------------------------

# Start an example. `c` is the counter, `o` the owner watcher. Its standard
# input is a fifo that stays open on descriptor 3 (counter) or 4 (owner), so
# that commands can be sent.
start_ex() { # who data-dir peer-port
  local who=$1 fifo="$BASE/stdin-$1"
  LOG[$who]="$BASE/example-$who.log"
  : >"${LOG[$who]}"
  COUNT[$who]=0
  rm -f "$fifo"
  mkfifo "$fifo"
  if [ "$who" = c ]; then
    exec 3<>"$fifo"
    "$COUNTER_BIN" "$2" "127.0.0.1:$3" --regtest-single-peer --customers-off-wifi \
      --heartbeat-max-age "$MAX_AGE" <&3 >"${LOG[c]}" 2>&1 &
  else
    exec 4<>"$fifo"
    "$OWNER_BIN" "$2" "127.0.0.1:$3" --regtest-single-peer <&4 >"${LOG[o]}" 2>&1 &
  fi
  PID[$who]=$!
}
# Send one command and wait for its `done`. LAST holds what it printed.
xcmd() { # who command [seconds]
  local who=$1 before i n fd=${FDN[$1]}
  COUNT[$who]=$((COUNT[$who] + 1))
  n=${COUNT[$who]}
  before=$(wc -l <"${LOG[$who]}")
  echo "$2" >&"$fd"
  for i in $(seq 1 $((${3:-30} * 4))); do
    if grep -q "^done $n\$" "${LOG[$who]}"; then
      LAST=$(tail -n +$((before + 1)) "${LOG[$who]}")
      return 0
    fi
    if ! kill -0 "${PID[$who]}" 2>/dev/null; then
      LAST=$(tail -n +$((before + 1)) "${LOG[$who]}")
      return 1
    fi
    sleep 0.25
  done
  LAST=$(tail -n +$((before + 1)) "${LOG[$who]}")
  return 1
}
# Wait for a line of the log that matches the pattern.
xawait() { # who pattern seconds
  local i
  for i in $(seq 1 $(($3 * 4))); do
    grep -Eq -- "$2" "${LOG[$1]}" && return 0
    if ! kill -0 "${PID[$1]}" 2>/dev/null; then break; fi
    sleep 0.25
  done
  grep -Eq -- "$2" "${LOG[$1]}"
}
# Wait until the example's view of the chain is at this height.
await_tip() { # who height seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    xcmd "$1" status || return 1
    if grep -q "^status tip=$2 " <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
# Ask for the state of a sale until it is the wanted one.
await_state() { # index state seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    xcmd c "judge $1" || return 1
    if grep -q "^sale $1 state=$2 " <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
stop_ex() { # who
  local who=$1
  echo quit >&"${FDN[$who]}"
  xawait "$who" "^stopped\$" 30 || return 1
  wait "${PID[$who]}"
  local rc=$?
  PID[$who]=
  if [ "$who" = c ]; then exec 3>&-; else exec 4>&-; fi
  return $rc
}
# How many times a report line was printed by the counter.
reports() { # "PosPaid index=0"
  grep -Ec -- "^report $1( |\$)" "${LOG[c]}" 2>/dev/null || true
}
# A heartbeat from the owner watcher: sets HB to its hex.
HB=
make_hb() { # [owner|approver|other-counter]
  HB=
  xcmd o "heartbeat ${1:-owner}" || return 1
  HB=$(sed -nE 's/^heartbeat ([0-9a-f]+)$/\1/p' <<<"$LAST" | head -1)
  [ -n "$HB" ]
}
# Pay the sale's address from the miner wallet at A; sets TXID.
TXID=
pay_sale() { # address sats
  TXID=$(cli_a -rpcwallet=miner sendtoaddress "$1" "$(btc "$2")")
}
# Open a sale; sets SALE (index), SADDR and SSATS.
SALE=
SADDR=
SSATS=
open_sale() {
  xcmd c "open $SALE_AMOUNT $SALE_RATE" || return 1
  local line
  line=$(grep '^opened ' <<<"$LAST" || true)
  SALE=$(sed -nE 's/^opened index=([0-9]+) .*/\1/p' <<<"$line")
  SADDR=$(sed -nE 's/.* address=([a-z0-9]+) .*/\1/p' <<<"$line")
  SSATS=$(sed -nE 's/.* sats=([0-9]+)$/\1/p' <<<"$line")
  [ -n "$SALE" ] && [ -n "$SADDR" ] && [ -n "$SSATS" ]
}
# Sets WHY to the `why=` text of a `judge` of the sale. Not run in a
# subshell: `xcmd` counts.
WHY=
why_of() { # index
  WHY=
  xcmd c "judge $1" || return 1
  WHY=$(sed -nE "s/^sale $1 .* why=(.*)\$/\1/p" <<<"$LAST")
}

# ---- Cores ----------------------------------------------------------------------

start_core() { # datadir rpc p2p
  bitcoind -regtest -datadir="$1" -rpcport="$2" \
    -listen=1 -bind="127.0.0.1:$3" -listenonion=0 -daemonwait -connect=0 \
    -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
    -fallbackfee=0.0001 -dbcache=50 >/dev/null
}
start_core "$A_DIR" "$A_RPC" "$A_P2P" || die "node A did not start"
A_PID=$(cat "$A_DIR/regtest/bitcoind.pid")
start_core "$B_DIR" "$B_RPC" "$B_P2P" || die "node B did not start"
B_PID=$(cat "$B_DIR/regtest/bitcoind.pid")

cli_a -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli_a -rpcwallet=miner getnewaddress)
cli_a generatetoaddress 150 "$MINE" >/dev/null || die "could not mine"
cli_b addnode "127.0.0.1:$A_P2P" onetry
for _ in $(seq 1 60); do
  [ "$(cli_b getblockcount)" = 150 ] && break
  sleep 1
done
[ "$(cli_b getblockcount)" = 150 ] || die "node B did not reach height 150"
say "Two nodes that serve filters, joined, at height 150"

# ---- 1 and 1b --------------------------------------------------------------------

say "1. Counter and owner watcher healthy by the real rules"
start_ex c "$C_DATA" "$A_P2P"
start_ex o "$O_DATA" "$B_P2P"
xawait c "^started " 30 || die "the counter example did not start"
xawait o "^started " 30 || die "the owner example did not start"
READY=
if await_tip c 150 90 && await_tip o 150 90; then READY=yes; fi
xcmd c health
CH=$(grep '^health ' <<<"$LAST")
xcmd c groups
CG=$(grep '^groups ' <<<"$LAST")
xcmd o health
OH=$(grep '^health ' <<<"$LAST")
xcmd o groups
OG=$(grep '^groups ' <<<"$LAST")
note "counter: $CH"
note "counter: $CG"
note "owner:   $OH"
note "owner:   $OG"
if [[ "$CH" == *"real=ok"* && "$OH" == *"real=ok"* ]]; then
  pass 1 "both are healthy by the real rules"
else
  bad 1 "the real rules say: counter ${CH#health }; owner ${OH#health }. The peers are on loopback, which is one network group (WR-41), and WR-50 asks for 3. Not changed; the run goes on with the single-peer switch, so WR-50 to WR-53 are not exercised."
fi
xcmd c status
note "counter: $(grep '^status ' <<<"$LAST")"
xcmd o status
note "owner:   $(grep '^status ' <<<"$LAST")"
if [ -n "$READY" ] && grep -q "used=healthy" <<<"$CH" && grep -q "used=healthy" <<<"$OH"; then
  pass 1b "both examples reached their node and the tip, height 150, and answer healthy under the switch"
else
  bad 1b "an example did not reach its node and the tip within 90 s"
fi

# ---- 2 ---------------------------------------------------------------------------

say "2. No heartbeat yet: a paid sale waits, and the counter says why"
xcmd c hbstate
note "$(grep '^hbstate ' <<<"$LAST")"
if grep -q "^hbstate view=none policy_verified=false policy_age=0 last_seq=0\$" <<<"$LAST"; then
  pass 2a "the counter has taken no heartbeat: view none, not verified, last number 0"
else
  bad 2a "unexpected hbstate: $(grep '^hbstate ' <<<"$LAST")"
fi
ROUTE2=
if open_sale; then
  S2=$SALE
  pay_sale "$SADDR" "$SSATS"
  if await_state "$S2" Waiting 45; then
    sleep 9
    why_of "$S2"
    note "sale $S2 why: $WHY"
    ROUTE2=$WHY
  fi
fi
if [[ "$ROUTE2" == *"no independent watcher"* ]]; then
  pass 2b "sale $S2 waits and says: $ROUTE2"
else
  bad 2b "sale ${S2:-?} waits, but the reason is not the missing watcher: \"$ROUTE2\". The payment is seen from one network group (all peers are on loopback), the watcher counts it as seen only from 3 (WR-44), so the instant decision is not asked and the heartbeat is never read."
fi

# ---- 3 ---------------------------------------------------------------------------

say "3. A fresh heartbeat is taken; a new paid sale"
make_hb owner || die "the owner watcher made no heartbeat"
HB1=$HB
note "heartbeat of ${#HB1} hex characters"
xcmd c "heartbeat $HB1"
TAKEN=$(grep '^heartbeat ' <<<"$LAST")
note "$TAKEN"
xcmd c hbstate
note "$(grep '^hbstate ' <<<"$LAST")"
if grep -q "^heartbeat taken view=Verified sent_at=" <<<"$TAKEN" &&
  grep -q "^hbstate view=Verified policy_verified=true policy_age=[0-9]* last_seq=[0-9]*\$" <<<"$LAST"; then
  pass 3a "the heartbeat of the owner watcher is taken and verified; its age is the counter's time minus its own"
else
  bad 3a "the heartbeat was not taken as verified: $TAKEN"
fi
INSTANT=
if open_sale; then
  S3=$SALE
  pay_sale "$SADDR" "$SSATS"
  TX3=$TXID
  for _ in $(seq 1 24); do
    xcmd c "judge $S3"
    if grep -q "^sale $S3 state=PaidInstant " <<<"$LAST"; then INSTANT=yes; break; fi
    sleep 0.5
  done
  note "$(grep "^sale $S3 " <<<"$LAST")"
fi
if [ -n "$INSTANT" ]; then
  pass 3b "sale $S3 shows PaidInstant"
else
  bad 3b "sale $S3 does not show PaidInstant (the line was: $(grep "^sale $S3 " <<<"$LAST")). One network group saw the payment, 3 are needed (WR-44); the mode is None. The settings are met (customers off the Wi-Fi, \$5 under the \$10 limit, 8 s watch, heartbeat verified)."
fi

# ---- 4 ---------------------------------------------------------------------------

say "4. PosPaid once, exposure on the ledger, released at the first confirmation"
xcmd c pump 1
PAID_BEFORE=$(reports "PosPaid index=$S3")
note "PosPaid for sale $S3 before a block: $PAID_BEFORE (no instant acceptance, so none is expected)"
cli_a generatetoaddress 1 "$MINE" >/dev/null
await_tip c 151 60 && await_tip o 151 60
state_ok=
if await_state "$S3" Confirmed 45; then state_ok=yes; fi
xcmd c pump 1
PAID_AFTER=$(reports "PosPaid index=$S3")
note "after one block: sale $S3 $(grep -q . <<<"$state_ok" && echo Confirmed || echo "not Confirmed"); PosPaid printed $PAID_AFTER time(s)"
if [ -n "$INSTANT" ] && [ -n "$state_ok" ] && [ "$PAID_BEFORE" = 1 ] && [ "$PAID_AFTER" = 1 ]; then
  pass 4 "PosPaid once at acceptance, once only after the block; Confirmed; exposure released"
else
  bad 4 "not reached: there was no instant acceptance (see 3b), so nothing went on the exposure ledger and PosPaid came only with the confirmation (before the block $PAID_BEFORE, after $PAID_AFTER; the sale is $(grep -q . <<<"$state_ok" && echo Confirmed || echo "not Confirmed")). The ledger and its release are checked in the counter's unit tests (CR-13, CR-15) and in the node's heartbeat tests."
fi

# ---- 5 ---------------------------------------------------------------------------

say "5. A replayed heartbeat, one from the approver key and one for another counter"
xcmd c "heartbeat $HB1"
REPLAY=$(grep '^heartbeat ' <<<"$LAST")
note "replay: $REPLAY"
make_hb approver || die "no approver heartbeat"
xcmd c "heartbeat $HB"
BYAPP=$(grep '^heartbeat ' <<<"$LAST")
note "approver key: $BYAPP"
make_hb other-counter || die "no other-counter heartbeat"
xcmd c "heartbeat $HB"
OTHERC=$(grep '^heartbeat ' <<<"$LAST")
note "other counter: $OTHERC"
if [[ "$REPLAY" == "heartbeat refused CR-50: heartbeat number "* ]] &&
  [[ "$BYAPP" == "heartbeat refused CR-50: heartbeat is not signed by the owner" ]] &&
  [[ "$OTHERC" == "heartbeat refused CR-50: heartbeat is for another counter" ]]; then
  pass 5 "the replay is refused as not above the last number, the approver's signature as not the owner's, and the other counter's as for another counter"
else
  bad 5 "the answers were not the three expected refusals"
fi

# ---- 6 ---------------------------------------------------------------------------

say "6. The heartbeat ages past the signed limit of $MAX_AGE seconds"
# A fresh heartbeat, so that the age is counted from now.
make_hb owner && xcmd c "heartbeat $HB" >/dev/null
xcmd c hbstate
note "just taken: $(grep '^hbstate ' <<<"$LAST")"
note "waiting $((MAX_AGE + 5)) seconds"
sleep $((MAX_AGE + 5))
xcmd c hbstate
AGED=$(grep '^hbstate ' <<<"$LAST")
note "$AGED"
AGE=$(sed -nE 's/.* policy_age=([0-9]+) .*/\1/p' <<<"$AGED")
if grep -q "policy_verified=true" <<<"$AGED" && [ -n "$AGE" ] && [ "$AGE" -gt "$MAX_AGE" ]; then
  pass 6a "the heartbeat is still verified but $AGE seconds old, above the signed limit of $MAX_AGE; the instant decision refuses it as too old"
else
  bad 6a "the heartbeat did not age past $MAX_AGE seconds: $AGED"
fi
if open_sale; then
  S6=$SALE
  pay_sale "$SADDR" "$SSATS"
  await_state "$S6" Waiting 45
  sleep 9
  why_of "$S6"
  WHY6=$WHY
  note "sale $S6 why: $WHY6"
fi
if [[ "${WHY6:-}" == *"no independent watcher"* ]]; then
  pass 6b "sale $S6 waits and says: $WHY6"
else
  bad 6b "sale ${S6:-?} waits, but the reason is not the missing watcher: \"${WHY6:-}\" (one network group; see 2b)"
fi

# ---- 7 ---------------------------------------------------------------------------

say "7. The owner watcher's tip is not the counter's"
# Part B from A: B is the outbound side, so it drops A.
cli_b disconnectnode "127.0.0.1:$A_P2P" >/dev/null 2>&1
for _ in $(seq 1 20); do
  [ "$(cli_b getpeerinfo | jq '[.[] | select(.inbound == false)] | length')" = 0 ] && break
  sleep 0.5
done
BH=$(cli_b generateblock "$MINE" '[]' | jq -r .hash)
AH_BEFORE=$(cli_a getblockcount)
await_tip o 152 30
xcmd o status
note "owner: $(grep '^status ' <<<"$LAST")"
note "node A at height $AH_BEFORE, node B at $(cli_b getblockcount), block of B: $BH"
make_hb owner || die "no heartbeat from the owner watcher"
xcmd c "heartbeat $HB"
FIRST7=$(grep '^heartbeat ' <<<"$LAST")
note "$FIRST7"
# A mines its own block at the same height.
cli_a generateblock "$MINE" '[]' >/dev/null
await_tip c 152 30
make_hb owner || die "no heartbeat from the owner watcher"
xcmd c "heartbeat $HB"
SECOND7=$(grep '^heartbeat ' <<<"$LAST")
note "$SECOND7"
xcmd c "pump 1"
NOTES7=$(grep '^note heartbeat' "${LOG[c]}" || true)
note "log: ${NOTES7:-nothing}"
if [[ "$FIRST7" == *"view=NotVerified rule=CR-53 disagreement=false"* ]] &&
  [[ "$SECOND7" == *"view=NotVerified rule=CR-53 disagreement=true"* ]] &&
  grep -q "^note heartbeat disagreement" <<<"$NOTES7"; then
  pass 7 "a tip the counter did not hold yet was 'not verified' and no alarm; another block at a height the counter holds was 'not verified' and logged as a disagreement"
else
  bad 7 "the two answers were not as expected"
fi

stop_ex c || note "the counter example did not stop cleanly"
stop_ex o || note "the owner example did not stop cleanly"

# ---- result ----------------------------------------------------------------------

say "Result"
failed=0
for n in 1 1b 2a 2b 3a 3b 4 5 6a 6b 7; do
  printf '%s  step %s\n' "${RESULT[$n]:-FAIL}" "$n"
  [ "${RESULT[$n]:-FAIL}" = PASS ] || failed=1
done
if [ "$failed" = 0 ]; then
  echo "ALL PASS"
else
  echo "SOME STEPS FAILED (see the notes above; 1, 2b, 3b, 4 and 6b are the loopback limit, not the heartbeat)"
  exit 1
fi
