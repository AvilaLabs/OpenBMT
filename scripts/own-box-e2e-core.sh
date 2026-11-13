#!/usr/bin/env bash
# A shop with its own wallet and no Key app, a real Box and a real counter,
# next to Bitcoin Core on regtest (docs/BOX-RULES.md BX-16 to BX-18,
# docs/COUNTER-RULES.md CR-130, CR-132).
#
# Question: does a Box that an own-wallet shop pairs with its owner root alone
# (no approver) take that shop's settings with the owner's signature, and does
# it then serve as the relay between the counter and the owner: the owner's
# heartbeat reaches the counter, the counter's records and refund request reach
# the owner, and the owner's copy of the chain equals the counter's?
#
# One private regtest node, A, serving compact filters. The Box is the real
# `openbmt-box` binary. The own shop is the `own_box` example of
# `openbmt-owner-node`: the owner library's `OwnShop` pairs the counter of
# `counter_regtest` by its offer and request, adds the Box with
# `OwnShop::add_box` (settings signed by the owner root alone) and sends the
# own-wallet welcome (`Welcome::make_own`). `box_pair` makes the invitation and
# keeps the claim (BX-10, BX-13), as for the Key app's Box. After the welcome
# the owner is `owner_regtest` and the counter is `counter_regtest`, both
# linked to the Box as their relay; the script carries nothing between them.
#
# THE HEALTH RULES ARE NOT CHECKED HERE. Both examples are started with
# `--regtest-single-peer`, as in `scripts/link-e2e-core.sh`. Sales are
# confirmed by blocks.
#
# In steps 1 to 5 the counter is paired before the Box, so that the first
# settings list it. Steps 6 and 7 run the other order against a second real
# Box (docs/APPROVAL-RULES.md AP-12, docs/OWNER-APP-RULES.md OA-119 to OA-122).
#
# The steps (printed as a table at the end):
#   1. The Box is paired under the own-wallet kind: its stored roots name the
#      owner root and no approver, the welcome came with settings that carry
#      no approver signature, and a welcome of the Key app's kind is refused
#      by the paired Box.
#   2. The owner's heartbeat reaches the counter through the Box.
#   3. A sale reaches Final; the counter's records reach the owner through the
#      Box, and the owner's copy of the chain equals the counter's.
#   4. The counter's check-in reaches the owner, and the owner's copy holds it
#      at its place.
#   5. The cashier's refund request reaches the owner's queue through the Box.
#   6. A Box added with no counter, then a first counter paired: the Box's
#      relay refuses the counter before and carries it after, and the Box
#      took the settings with the owner's signature alone (AP-12).
#   7. A second counter paired after: the change waits (the owner phone's
#      settings stay as they were), the Box neither takes it nor relays the
#      counter; the first counter's confirmation is joined, the settings go
#      to the Box as `OBM<`, and then the Box relays the second counter and
#      holds the new version.
#
# Steps 6 and 7 are driven by `own_box flow`, with the owner library's real
# `OwnShop` and the real Box. The first counter's confirmation there is signed
# with the core's `CounterConfirmation::sign`: `counter_regtest` has no counter
# paired with an own wallet and a Box, so the counter's own CR-241 record is
# not part of this run (it is covered by the counter node's tests).
#
# Environment:
#   CORE_BIN       the folder of Bitcoin Core's binaries
#                  (default $HOME/.local/opt/bitcoin-core-32.1/bin)
#   DRILL_PORTS    the first of four ports (default 18986)
#   DRILL_DIR      a scratch directory (default a new one; removed on exit
#                  unless DRILL_KEEP=1)
#   COUNTER_BIN, OWNER_BIN, BOX_BIN, PAIR_BIN, OWNBOX_BIN   binaries; built if
#                  not given
#
# Everything here is regtest on 127.0.0.1, with keys made from fixed test
# bytes. No real keys or funds are involved.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
P0=${DRILL_PORTS:-18986}
A_RPC=$P0
A_P2P=$((P0 + 1))
BOX_PORT=$((P0 + 3))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/own-box-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
A_DIR="$BASE/core-a"
C_DATA="$BASE/counter"
O_DATA="$BASE/owner"
B_DATA="$BASE/box"
K_DATA="$BASE/shop"
B2_DATA="$BASE/box2"
K2_DATA="$BASE/shop2"
BOX2_PORT=$((P0 + 2))
SALE_AMOUNT=500       # $5.00 in cents
SALE_RATE=10000000    # $100,000 per bitcoin, in cents
HB_EVERY=3            # seconds between the owner's heartbeats
FRESH=20              # a heartbeat younger than this is fresh, in seconds
REFUND_SATS=2000      # the part the cashier asks for

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
cli_a() { bitcoin-cli -regtest -datadir="$A_DIR" -rpcport="$A_RPC" "$@"; }
btc() { printf '%d.%08d' $(($1 / 100000000)) $(($1 % 100000000)); }

declare -A RESULT MESSAGE
pass() { RESULT[$1]=PASS; MESSAGE[$1]=$2; printf 'PASS  step %s: %s\n' "$1" "$2"; }
bad() { RESULT[$1]=FAIL; MESSAGE[$1]=$2; printf 'FAIL  step %s: %s\n' "$1" "$2"; }

for tool in bitcoind bitcoin-cli jq xxd; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH"
done
[ "$(command -v bitcoind)" = "$CORE_BIN/bitcoind" ] || die "bitcoind is not the one in $CORE_BIN"
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) die "this check is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

mkdir -p "$A_DIR" "$C_DATA" "$O_DATA" "$B_DATA" "$K_DATA" "$B2_DATA" "$K2_DATA"
A_PID=
BOX_PID=
BOX2_PID=
declare -A PID LOG FDN COUNT
PID[c]=; PID[o]=
FDN[c]=3; FDN[o]=4
COUNT[c]=0; COUNT[o]=0
LAST=

stop_pid() { # pid
  local pid=$1
  [ -n "$pid" ] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.25
    done
    kill -9 "$pid" 2>/dev/null || true
  fi
}
stop_core() {
  [ -n "$A_PID" ] || return 0
  cli_a stop >/dev/null 2>&1 || true
  for _ in $(seq 1 30); do
    kill -0 "$A_PID" 2>/dev/null || break
    sleep 1
  done
  kill -9 "$A_PID" 2>/dev/null || true
}
cleanup() {
  exec 3>&- 4>&- 2>/dev/null || true
  stop_pid "${PID[c]:-}"
  stop_pid "${PID[o]:-}"
  stop_pid "$BOX_PID"
  stop_pid "$BOX2_PID"
  stop_core
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

guard=$HOME/.cache/openbmt/guard.sh
TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target}
build() { # cargo args
  if [ -x "$guard" ]; then
    "$guard" cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" "$@" || die "the build failed: $*"
  else
    cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" "$@" || die "the build failed: $*"
  fi
}
if [ -z "${COUNTER_BIN:-}" ] || [ -z "${OWNER_BIN:-}" ] || [ -z "${BOX_BIN:-}" ] || [ -z "${PAIR_BIN:-}" ] || [ -z "${OWNBOX_BIN:-}" ]; then
  say "Build the counter, the owner, the Box and the pairing examples"
  build -p openbmt-counter-node --example counter_regtest
  build -p openbmt-owner-node --example owner_regtest --example own_box
  build -p openbmt-box --bins --example box_pair
  COUNTER_BIN=${COUNTER_BIN:-$TARGET/debug/examples/counter_regtest}
  OWNER_BIN=${OWNER_BIN:-$TARGET/debug/examples/owner_regtest}
  OWNBOX_BIN=${OWNBOX_BIN:-$TARGET/debug/examples/own_box}
  BOX_BIN=${BOX_BIN:-$TARGET/debug/openbmt-box}
  PAIR_BIN=${PAIR_BIN:-$TARGET/debug/examples/box_pair}
fi
for bin in COUNTER_BIN OWNER_BIN BOX_BIN PAIR_BIN OWNBOX_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done

# ---- helpers ---------------------------------------------------------------------

# Start an example. `c` is the counter, `o` the owner. Its standard input is a
# fifo that stays open on descriptor 3 (counter) or 4 (owner).
start_ex() { # who key
  local who=$1 fifo="$BASE/stdin-$1"
  LOG[$who]="$BASE/example-$who.log"
  : >"${LOG[$who]}"
  rm -f "$fifo"
  mkfifo "$fifo"
  if [ "$who" = c ]; then
    exec 3<>"$fifo"
    "$COUNTER_BIN" "$C_DATA" "127.0.0.1:$A_P2P" --regtest-single-peer --customers-off-wifi \
      --relay "127.0.0.1:$BOX_PORT" --relay-key "$2" --box-key "$2" --box-owner-fixed <&3 >"${LOG[c]}" 2>&1 &
  else
    exec 4<>"$fifo"
    "$OWNER_BIN" "$O_DATA" "127.0.0.1:$A_P2P" --regtest-single-peer --relay "127.0.0.1:$BOX_PORT" \
      --relay-key "$2" --heartbeat-every "$HB_EVERY" <&4 >"${LOG[o]}" 2>&1 &
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
xawait() { # who pattern seconds
  local i
  for i in $(seq 1 $(($3 * 4))); do
    grep -Eq -- "$2" "${LOG[$1]}" && return 0
    if ! kill -0 "${PID[$1]}" 2>/dev/null; then break; fi
    sleep 0.25
  done
  grep -Eq -- "$2" "${LOG[$1]}"
}
await_tip() { # who height seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    xcmd "$1" status || return 1
    if grep -q "^status tip=$2 " <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
await_state() { # index state seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    xcmd c "judge $1" || return 1
    if grep -q "^sale $1 state=$2 " <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
kv() { # line key
  sed -nE "s/^(.* )?$2=([^ ]*).*/\2/p" <<<"$1" | head -1
}
CHAIN_N=; CHAIN_HEAD=; COPY_N=; COPY_HEAD=; CI_SEQ=; CI_HELD=; CI_AT=
read_states() {
  local line
  xcmd c linkstate || return 1
  line=$(grep '^linkstate ' <<<"$LAST")
  CHAIN_N=$(kv "$line" chain_records)
  CHAIN_HEAD=$(kv "$line" chain_head)
  xcmd o copies || return 1
  line=$(grep '^copy ' <<<"$LAST" | head -1)
  COPY_N=$(kv "$line" records)
  COPY_HEAD=$(kv "$line" head)
  CI_SEQ=$(kv "$line" check_in_seq)
  CI_HELD=$(kv "$line" check_in_held)
  CI_AT=$(kv "$line" last_check_in_at)
}
await_same_chain() { # seconds
  local i
  for i in $(seq 1 $(($1 * 2))); do
    read_states || return 1
    if [ -n "$CHAIN_N" ] && [ "$CHAIN_N" = "$COPY_N" ] && [ "$CHAIN_HEAD" = "$COPY_HEAD" ]; then
      return 0
    fi
    sleep 0.5
  done
  return 1
}
HB_AGE=; HB_VERIFIED=
read_hb() {
  local line
  xcmd c hbstate || return 1
  line=$(grep '^hbstate ' <<<"$LAST")
  HB_AGE=$(kv "$line" policy_age)
  HB_VERIFIED=$(kv "$line" policy_verified)
}
SALE=; SADDR=; SSATS=
open_sale() {
  xcmd c "open $SALE_AMOUNT $SALE_RATE" || return 1
  local line
  line=$(grep '^opened ' <<<"$LAST" || true)
  SALE=$(sed -nE 's/^opened index=([0-9]+) .*/\1/p' <<<"$line")
  SADDR=$(sed -nE 's/.* address=([a-z0-9]+) .*/\1/p' <<<"$line")
  SSATS=$(sed -nE 's/.* sats=([0-9]+)$/\1/p' <<<"$line")
  [ -n "$SALE" ] && [ -n "$SADDR" ] && [ -n "$SSATS" ]
}
HEIGHT=150

# ---- Core, the Box, the own shop ----------------------------------------------------

bitcoind -regtest -datadir="$A_DIR" -rpcport="$A_RPC" \
  -listen=1 -bind="127.0.0.1:$A_P2P" -listenonion=0 -daemonwait -connect=0 \
  -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
  -fallbackfee=0.0001 -dbcache=50 >/dev/null || die "node A did not start"
A_PID=$(cat "$A_DIR/regtest/bitcoind.pid")
cli_a -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli_a -rpcwallet=miner getnewaddress)
cli_a generatetoaddress 150 "$MINE" >/dev/null || die "could not mine"
say "A node that serves filters, at height 150"

LOG_BOX="$BASE/box.log"
"$PAIR_BIN" invite "$K_DATA" >"$BASE/invitation.txt" 2>&1 || die "no invitation"
"$BOX_BIN" "$B_DATA" "127.0.0.1:$BOX_PORT" --no-console --invitation-file "$BASE/invitation.txt" \
  </dev/null >"$LOG_BOX" 2>&1 &
BOX_PID=$!
for _ in $(seq 1 40); do
  grep -q '^state: ' "$LOG_BOX" && break
  kill -0 "$BOX_PID" 2>/dev/null || break
  sleep 0.25
done
grep -q '^state: ' "$LOG_BOX" || die "the Box did not start: $(cat "$LOG_BOX")"
"$PAIR_BIN" claim "$K_DATA" "127.0.0.1:$BOX_PORT" >/dev/null 2>&1 || die "the claim failed"
WELCOME=$("$OWNBOX_BIN" welcome "$K_DATA" "127.0.0.1:$BOX_PORT" 2>&1)
note "own shop: $WELCOME"
BOX_KEY=$(sed -n 's/^key: //p' "$LOG_BOX" | head -1)
[ ${#BOX_KEY} = 64 ] || die "no Box key"
OWNER_KEY=$("$OWNBOX_BIN" key) || die "no owner key"
"$OWNBOX_BIN" mailbox "127.0.0.1:$BOX_PORT" >/dev/null 2>&1 || die "no owner mailbox"
say "A Box paired by an own-wallet shop, key $BOX_KEY"

# ---- 1 ---------------------------------------------------------------------------

say "1. The Box is paired under the own-wallet kind"
ROOTS_HEAD=$(xxd -p -l 6 "$B_DATA/roots" 2>/dev/null | tr -d '\n')
ROOTS_OWNER=$(xxd -p -s 6 -l 32 "$B_DATA/roots" 2>/dev/null | tr -d '\n')
ROOTS_SIZE=$(wc -c <"$B_DATA/roots" 2>/dev/null)
AGAIN=$("$PAIR_BIN" welcome "$K_DATA" "127.0.0.1:$BOX_PORT" 2>&1)
note "roots: $ROOTS_HEAD $ROOTS_OWNER ($ROOTS_SIZE bytes)"
note "a welcome of the Key app's kind to the paired Box: $AGAIN"
if grep -q '^welcomed settings version [0-9]* counters 1 approver none$' <<<"$WELCOME" &&
  [ "$ROOTS_HEAD" = 4f424d680201 ] && [ "$ROOTS_SIZE" = 38 ] && [ "$ROOTS_OWNER" = "$OWNER_KEY" ] &&
  grep -q '^refused' <<<"$AGAIN"; then
  pass 1 "the Box holds the owner root and no approver (kind 1, 38 bytes), the settings came signed by the owner alone with the counter listed, and a welcome of the Key app's kind was refused"
else
  bad 1 "the Box's roots are not an own wallet's: head '$ROOTS_HEAD', size '$ROOTS_SIZE', welcome '$WELCOME', again '$AGAIN'"
fi

# ---- counter and owner, linked to the Box --------------------------------------------

start_ex c "$BOX_KEY"
start_ex o "$BOX_KEY"
xawait c "^started " 30 || die "the counter example did not start: $(tail -3 "${LOG[c]}")"
xawait o "^started " 30 || die "the owner example did not start: $(tail -3 "${LOG[o]}")"
READY=
if await_tip c 150 90 && await_tip o 150 90; then READY=yes; fi
[ -n "$READY" ] || die "an example did not reach the tip, height 150, within 90 s"
say "Counter and owner at height 150, both linked to the Box"

# ---- 2 ---------------------------------------------------------------------------

say "2. The owner's heartbeat reaches the counter through the Box"
FOUND=
for _ in $(seq 1 45); do
  read_hb
  if [ "$HB_VERIFIED" = true ] && [ -n "$HB_AGE" ] && [ "$HB_AGE" -le "$FRESH" ]; then FOUND=yes; break; fi
  sleep 1
done
xcmd c linkstate
LS2=$(grep '^linkstate ' <<<"$LAST")
note "hbstate: verified=$HB_VERIFIED age=$HB_AGE"
note "$LS2"
TAKEN=$(kv "$LS2" heartbeats_taken)
if [ -n "$FOUND" ] && [ "${TAKEN:-0}" -ge 1 ] && [ "$(kv "$LS2" reachable)" = true ]; then
  pass 2 "the counter took $TAKEN heartbeat(s) the owner sent through the Box; the last is verified and $HB_AGE s old (fresh is under $FRESH)"
else
  bad 2 "no fresh verified heartbeat through the Box within 45 s (verified=$HB_VERIFIED, age=$HB_AGE, taken=${TAKEN:-?})"
fi

# ---- 3 ---------------------------------------------------------------------------

say "3. A sale reaches Final; the owner's copy of the chain equals the counter's"
SALE_OK=
if open_sale; then
  cli_a -rpcwallet=miner sendtoaddress "$SADDR" "$(btc "$SSATS")" >/dev/null
  cli_a generatetoaddress 6 "$MINE" >/dev/null
  HEIGHT=$((HEIGHT + 6))
  if await_tip c "$HEIGHT" 90 && await_tip o "$HEIGHT" 90 && await_state "$SALE" Final 90; then SALE_OK=yes; fi
fi
if await_same_chain 90; then SAME=yes; else SAME=; fi
read_states
note "counter: $CHAIN_N records, head $CHAIN_HEAD"
note "owner:   $COPY_N records, head $COPY_HEAD"
if [ -n "$SALE_OK" ] && [ -n "$SAME" ] && [ "${CHAIN_N:-0}" -ge 3 ]; then
  pass 3 "sale $SALE reached Final and the owner's copy, taken through the Box, has the counter's $CHAIN_N records and the same head"
else
  bad 3 "sale final: ${SALE_OK:-no}; chains equal: ${SAME:-no} (counter $CHAIN_N, owner copy $COPY_N)"
fi

# ---- 4 ---------------------------------------------------------------------------

say "4. The check-in reaches the owner"
for _ in $(seq 1 60); do
  read_states && [ -n "$CI_SEQ" ] && [ "$CI_SEQ" != none ] && break
  sleep 0.5
done
xcmd o linkstate
OS4=$(grep '^linkstate ' <<<"$LAST")
note "owner: $OS4"
note "owner copy: check_in_seq=$CI_SEQ held=$CI_HELD last_check_in_at=$CI_AT"
if [ "$CI_SEQ" != none ] && [ -n "$CI_SEQ" ] && [ "$CI_HELD" = true ] && [ "$CI_AT" != none ] &&
  [ "$(kv "$OS4" alerts)" = 0 ] && [ "$(kv "$OS4" refused)" = 0 ]; then
  pass 4 "the owner holds the counter's check-in at record $CI_SEQ, its copy has that record at that place, and no alert is raised"
else
  bad 4 "no matching check-in at the owner (seq=$CI_SEQ held=$CI_HELD), or an alert or refusal: $OS4"
fi

# ---- 5 ---------------------------------------------------------------------------

say "5. The cashier's refund request reaches the owner's queue through the Box"
DEST=$(cli_a -rpcwallet=miner getnewaddress)
xcmd c "refund $SALE $REFUND_SATS $DEST"
note "$(grep '^refund ' <<<"$LAST")"
QUEUED=
for _ in $(seq 1 60); do
  xcmd o refunds || break
  if grep -q '^refunds queued=1 ' <<<"$LAST"; then QUEUED=yes; break; fi
  sleep 1
done
RLINE=$(grep '^refund sale=' <<<"$LAST" || true)
note "$RLINE"
if [ -n "$QUEUED" ] && [ "$(kv "$RLINE" sale)" = "$SALE" ] && [ "$(kv "$RLINE" sats)" = "$REFUND_SATS" ] &&
  [ "$(kv "$RLINE" address)" = "$DEST" ] && [ "$(kv "$RLINE" stage)" = Queued ]; then
  pass 5 "the owner's queue holds the refund: sale $SALE, $REFUND_SATS sats, to $DEST, queued; it came from the counter's records through the Box"
else
  bad 5 "the owner's queue is not as expected: ${RLINE:-no refund line}"
fi

# ---- 6 and 7 ------------------------------------------------------------------------

say "6. A Box added before any counter, then a first counter"
"$PAIR_BIN" invite "$K2_DATA" >"$BASE/invitation2.txt" 2>&1 || die "no second invitation"
"$BOX_BIN" "$B2_DATA" "127.0.0.1:$BOX2_PORT" --no-console --invitation-file "$BASE/invitation2.txt" \
  </dev/null >"$BASE/box2.log" 2>&1 &
BOX2_PID=$!
for _ in $(seq 1 40); do
  grep -q '^state: ' "$BASE/box2.log" && break
  kill -0 "$BOX2_PID" 2>/dev/null || break
  sleep 0.25
done
grep -q '^state: ' "$BASE/box2.log" || die "the second Box did not start: $(cat "$BASE/box2.log")"
"$PAIR_BIN" claim "$K2_DATA" "127.0.0.1:$BOX2_PORT" >/dev/null 2>&1 || die "the second claim failed"
FLOW=$("$OWNBOX_BIN" flow "$K2_DATA" "127.0.0.1:$BOX2_PORT" "$B2_DATA" 2>&1)
printf '%s\n' "$FLOW" | while IFS= read -r l; do note "$l"; done
L_WELCOME=$(grep '^welcomed ' <<<"$FLOW")
L_FIRST=$(grep '^first ' <<<"$FLOW")
L_SECOND=$(grep '^second ' <<<"$FLOW")
L_CONFIRMED=$(grep '^confirmed ' <<<"$FLOW")
if grep -q '^welcomed version [0-9]* counters 0$' <<<"$L_WELCOME" &&
  grep -q '^relay first=refused$' <<<"$FLOW" &&
  [ "$(kv "$L_FIRST" waiting)" = false ] && [ -n "$(kv "$L_FIRST" version)" ] &&
  [ "$(kv "$L_FIRST" box_version)" = "$(kv "$L_FIRST" version)" ] &&
  grep -q 'relay first=ok$' <<<"$L_FIRST"; then
  pass 6 "the Box was added with no counter, which refused the first counter's relay; the first counter was a Free change, taken by the Box at version $(kv "$L_FIRST" box_version) with the owner's signature alone, and its relay then passed"
else
  bad 6 "the first counter did not pass the Box as expected: ${L_WELCOME:-no welcome} / ${L_FIRST:-no first line}"
fi

say "7. A second counter waits for the first counter's confirmation"
if [ "$(kv "$L_SECOND" waiting)" = true ] &&
  [ "$(kv "$L_SECOND" in_force)" = "$(kv "$L_FIRST" version)" ] &&
  [ "$(kv "$L_SECOND" box_version)" = "$(kv "$L_FIRST" version)" ] &&
  grep -q 'relay second=refused$' <<<"$L_SECOND" &&
  [ "$(kv "$L_CONFIRMED" in_force)" = "$(kv "$L_SECOND" held_version)" ] &&
  [ "$(kv "$L_CONFIRMED" waiting)" = false ] &&
  [ "$(kv "$L_CONFIRMED" box_version)" = "$(kv "$L_SECOND" held_version)" ] &&
  grep -q 'relay second=ok$' <<<"$L_CONFIRMED"; then
  pass 7 "the second counter's change (version $(kv "$L_SECOND" held_version)) waited: the owner's settings and the Box's stayed at version $(kv "$L_FIRST" version) and the Box refused the counter; after the first counter's confirmation the owner and the Box hold version $(kv "$L_CONFIRMED" box_version), and the second counter's relay passes"
else
  bad 7 "the second counter's change did not go as expected: ${L_SECOND:-no second line} / ${L_CONFIRMED:-no confirmed line}"
fi

# ---- the table -----------------------------------------------------------------------

say "Summary"
FAILS=0
for s in 1 2 3 4 5 6 7; do
  printf '%-4s step %s: %s\n' "${RESULT[$s]:-NOT RUN}" "$s" "${MESSAGE[$s]:-}"
  [ "${RESULT[$s]:-}" = PASS ] || FAILS=$((FAILS + 1))
done
echo quit >&3
echo quit >&4
for _ in $(seq 1 40); do
  grep -q '^stopped$' "${LOG[c]}" && break
  sleep 0.25
done
[ "$FAILS" = 0 ] && echo "ALL PASS" || echo "$FAILS step(s) did not pass"
[ "$FAILS" = 0 ]
