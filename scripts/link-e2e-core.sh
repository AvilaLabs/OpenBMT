#!/usr/bin/env bash
# The owner and the counter talking through a relay, next to Bitcoin Core on
# regtest.
#
# Question: do the owner's heartbeat and the counter's check-ins and records
# travel through the relay of `openbmt-link` (docs/LINK-RULES.md section 6),
# does the owner end up with a verified copy of the counter's chain that has
# the same length and head, does a relay that is gone and comes back leave
# the shop working on-chain and the link catching up, and does the relay turn
# a device that is not a member away?
#
# One private regtest node, A, serving compact filters. The counter example
# (`counter_regtest`, `openbmt-counter-node`) and the owner example
# (`owner_regtest`, `openbmt-owner-node`) are both peers of A, so their chains
# agree. The relay is the `relay` example of `openbmt-link` with a fixed test
# identity and a `members` file holding the two example keys. The script
# carries nothing between the devices: the heartbeat, the check-ins, the
# records and the receipts all go through the relay's TCP port.
#
# THE HEALTH RULES ARE NOT CHECKED HERE. Both examples are started with
# `--regtest-single-peer`, which replaces the health answer with "healthy"
# (every peer is on loopback, one network group, WR-41; see
# `scripts/instant-e2e-core.sh`). Instant acceptance is not exercised: the
# sales here are confirmed by a block.
#
# The steps (printed as a table at the end):
#   1. A heartbeat sent by the owner through the relay is taken by the
#      counter, and `hbstate` shows it fresh.
#   2. A sale goes to Confirmed, and the owner's copy of the counter's chain
#      has the same length and head as the counter's.
#   3. A check-in arrived at the owner, the copy holds it at its place, and no
#      alert is raised.
#   4. The relay is stopped. The heartbeat goes stale on the counter (age
#      above the signed limit of 60 seconds), and a sale still goes to
#      Confirmed on-chain.
#   5. The relay is started again on the same directory. The heartbeat is
#      fresh again and the owner's copy catches up to the same length and
#      head.
#   6. A device whose key is not in the members file can neither make a
#      mailbox, nor put a message, nor fetch.
#   7. A counter started the way the counter app starts it, through the
#      `start` call of `openbmt-counter-ffi`, from a pairing record made by
#      the whole pairing ceremony out of a bundle that carries a Box as its
#      relay (CR-130, CR-131), talks to its owner through that Box
#      (`openbmt-box`, paired as `scripts/box-e2e.sh` pairs it): the owner's
#      heartbeat is taken by the counter and the counter's records and check-in
#      reach the owner, each through the Box (CR-134 to CR-136). The example
#      `counter_box_link` of `openbmt-counter-ffi` does the counter and the
#      owner; this script starts and pairs the Box.
#   8. Settings reach the counter through that Box (CR-160 to CR-163, LK-64):
#      a tightening the owner signed alone, then a loosening co-signed by a
#      test approver, each sent by the owner through the Box. The counter
#      applies both, and the owner's copy of the counter's records holds the
#      `Setting` record of each version.
#   9. Settings that remove the counter reach it the same way (CR-165): it
#      applies them, says on Home that it was removed, and refuses to open a
#      new sale.
#  10. A batch of addresses that the relay loses is sent again by the owner's
#      link until the counter's records show it held (LK-70, LK-71, CR-167):
#      with the counter stopped the owner hands the link a batch, and the relay
#      is restarted with its mailboxes removed, so the first message is gone.
#      The counter, started again, holds the batch after the owner's resend
#      (the owner example sends again every 5 seconds here), the owner's copy
#      shows it and the batch stops waiting. The owner then hands over the
#      same batch once more, and the counter's records still show it once
#      (CR-168).
#  11. The owner names a watch key in the settings (CR-185): signed by the
#      owner and a test approver, as a loosening must be. The counter takes
#      them, and names the watch key.
#  12. The owner stops. Its heartbeat goes stale on the counter.
#  13. A watch-only owner (`watch_regtest`, `WatchNode`, LK-75: no owner
#      secret, no wallet, no key 1) starts on the same data directory and
#      sends heartbeats signed by the watch key through the relay. The counter
#      takes them, the heartbeat is verified and fresh, and so the owner's
#      word that instant sales may go on (CR-50 to CR-53) is there with the
#      owner's own phone stopped (CR-186). The relay lists the watch key as a
#      member, as the person who runs it does (LK-74).
#  14. Settings that remove the watch key (a tightening, signed by the owner
#      alone) are taken. The heartbeats the watcher still sends through the
#      relay are no longer taken, and a heartbeat signed by the watch key,
#      handed to the counter by hand, is refused.
#  Steps 11 and 14 hand the settings to the counter by its `settings` command;
#  the delivery through the link is step 8, and the Box's part is BX-22.
#  What step 13 shows of "instant is allowed" is the heartbeat: the sale
#  itself is not run as an instant one, because the health rules and Tor are
#  not exercised here (see above).
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (put first on
# PATH; set CORE_BIN to use another), jq, and cargo unless the four binaries
# below are named. A build goes through ~/.cache/openbmt/guard.sh when that
# file exists.
#
# Environment:
#   CORE_BIN       directory with bitcoind and bitcoin-cli
#   DRILL_DIR      scratch directory (default: a new one under ~/.cache/openbmt)
#   DRILL_PORTS    first of four free ports on 127.0.0.1 (default 18976): RPC
#                  and peer port of A, the relay's port and the Box's port
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   COUNTER_BIN    the `counter_regtest` example binary
#   OWNER_BIN      the `owner_regtest` example binary
#   RELAY_BIN      the `relay` example binary of `openbmt-link`
#   PROBE_BIN      the `link_probe` example binary of `openbmt-courier`
#   WATCH_BIN      the `watch_regtest` example binary of `openbmt-owner-node`
#                  (steps 10 to 13)
#   BOX_BIN        the `openbmt-box` binary (step 7)
#   PAIR_BIN       the `box_pair` example binary of `openbmt-box` (step 7)
#   BOXLINK_BIN    the `counter_box_link` example binary of
#                  `openbmt-counter-ffi` (step 7)
#
# Everything here is regtest on 127.0.0.1, with keys made from fixed test
# bytes. No real keys or funds are involved. The processes and the scratch
# directory are removed on every exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
P0=${DRILL_PORTS:-18976}
A_RPC=$P0
A_P2P=$((P0 + 1))
RELAY_PORT=$((P0 + 2))
BOX_PORT=$((P0 + 3))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/link-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
A_DIR="$BASE/core-a"
C_DATA="$BASE/counter"
O_DATA="$BASE/owner"
R_DATA="$BASE/relay"
SALE_AMOUNT=500       # $5.00 in cents, 5,000 sats at the rate below
SALE_RATE=10000000    # $100,000 per bitcoin, in cents
MAX_AGE=60            # the signed heartbeat age limit, at its minimum
HB_EVERY=3            # seconds between the owner's heartbeats
BATCH_RESEND=5        # seconds between the owner's sends of a batch that waits
FRESH=20              # a heartbeat younger than this is fresh, in seconds

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
notrun() { RESULT[$1]="NOT RUN"; MESSAGE[$1]=$2; printf 'NOT RUN  step %s: %s\n' "$1" "$2"; }

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

mkdir -p "$A_DIR" "$C_DATA" "$O_DATA" "$R_DATA"
A_PID=
RELAY_PID=
BOX_PID=
declare -A PID LOG FDN COUNT
PID[c]=; PID[o]=; PID[w]=
FDN[c]=3; FDN[o]=4; FDN[w]=5
COUNT[c]=0; COUNT[o]=0; COUNT[w]=0
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
  exec 3>&- 4>&- 5>&- 2>/dev/null || true
  stop_pid "${PID[c]:-}"
  stop_pid "${PID[o]:-}"
  stop_pid "${PID[w]:-}"
  stop_pid "$RELAY_PID"
  stop_pid "$BOX_PID"
  stop_core
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/link-e2e}
build() { # package example
  local guard=$HOME/.cache/openbmt/guard.sh
  if [ -x "$guard" ]; then
    "$guard" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p "$1" --example "$2" || die "the build of $2 failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p "$1" --example "$2" || die "the build of $2 failed"
  fi
}
if [ -z "${COUNTER_BIN:-}" ] || [ -z "${OWNER_BIN:-}" ] || [ -z "${RELAY_BIN:-}" ] || [ -z "${PROBE_BIN:-}" ] || [ -z "${WATCH_BIN:-}" ]; then
  say "Build the examples"
  [ -n "${COUNTER_BIN:-}" ] || { build openbmt-counter-node counter_regtest; COUNTER_BIN="$TARGET/debug/examples/counter_regtest"; }
  [ -n "${OWNER_BIN:-}" ] || { build openbmt-owner-node owner_regtest; OWNER_BIN="$TARGET/debug/examples/owner_regtest"; }
  [ -n "${RELAY_BIN:-}" ] || { build openbmt-link relay; RELAY_BIN="$TARGET/debug/examples/relay"; }
  [ -n "${PROBE_BIN:-}" ] || { build openbmt-courier link_probe; PROBE_BIN="$TARGET/debug/examples/link_probe"; }
  [ -n "${WATCH_BIN:-}" ] || { build openbmt-owner-node watch_regtest; WATCH_BIN="$TARGET/debug/examples/watch_regtest"; }
fi
for bin in COUNTER_BIN OWNER_BIN RELAY_BIN PROBE_BIN WATCH_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done
if [ -z "${BOX_BIN:-}" ] || [ -z "${PAIR_BIN:-}" ] || [ -z "${BOXLINK_BIN:-}" ]; then
  say "Build the Box and the counter's link example"
  if [ -z "${BOX_BIN:-}" ] || [ -z "${PAIR_BIN:-}" ]; then
    guard=$HOME/.cache/openbmt/guard.sh
    if [ -x "$guard" ]; then
      "$guard" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
        --manifest-path "$ROOT/Cargo.toml" -p openbmt-box --bins --example box_pair ||
        die "the build of the Box failed"
    else
      CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
        --manifest-path "$ROOT/Cargo.toml" -p openbmt-box --bins --example box_pair ||
        die "the build of the Box failed"
    fi
    [ -n "${BOX_BIN:-}" ] || BOX_BIN="$TARGET/debug/openbmt-box"
    [ -n "${PAIR_BIN:-}" ] || PAIR_BIN="$TARGET/debug/examples/box_pair"
  fi
  if [ -z "${BOXLINK_BIN:-}" ]; then
    build openbmt-counter-ffi counter_box_link
    BOXLINK_BIN="$TARGET/debug/examples/counter_box_link"
  fi
fi
for bin in BOX_BIN PAIR_BIN BOXLINK_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done

# ---- helpers for the examples ---------------------------------------------------

# Start an example. `c` is the counter, `o` the owner, `w` the watch-only owner. Its standard input is
# a fifo that stays open on descriptor 3 (counter) or 4 (owner), so that
# commands can be sent.
start_ex() { # who data-dir peer-port relay-port relay-key
  local who=$1 fifo="$BASE/stdin-$1"
  LOG[$who]="$BASE/example-$who.log"
  : >"${LOG[$who]}"
  COUNT[$who]=0
  rm -f "$fifo"
  mkfifo "$fifo"
  if [ "$who" = w ]; then
    exec 5<>"$fifo"
    "$WATCH_BIN" "$2" "127.0.0.1:$3" --regtest-single-peer --relay "127.0.0.1:$4" \
      --relay-key "$5" --heartbeat-every "$HB_EVERY" --heartbeat-max-age "$MAX_AGE" \
      <&5 >"${LOG[w]}" 2>&1 &
  elif [ "$who" = c ]; then
    exec 3<>"$fifo"
    "$COUNTER_BIN" "$2" "127.0.0.1:$3" --regtest-single-peer --customers-off-wifi \
      --heartbeat-max-age "$MAX_AGE" --relay "127.0.0.1:$4" --relay-key "$5" \
      <&3 >"${LOG[c]}" 2>&1 &
  else
    exec 4<>"$fifo"
    "$OWNER_BIN" "$2" "127.0.0.1:$3" --regtest-single-peer --relay "127.0.0.1:$4" \
      --relay-key "$5" --heartbeat-every "$HB_EVERY" --batch-resend "$BATCH_RESEND" <&4 >"${LOG[o]}" 2>&1 &
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
stop_ex() { # who
  local who=$1
  echo quit >&"${FDN[$who]}"
  xawait "$who" "^stopped\$" 30 || return 1
  wait "${PID[$who]}"
  local rc=$?
  PID[$who]=
  case "$who" in c) exec 3>&- ;; o) exec 4>&- ;; w) exec 5>&- ;; esac
  return $rc
}
# The value of `key=` in a line.
kv() { # line key
  sed -nE "s/^(.* )?$2=([^ ]*).*/\2/p" <<<"$1" | head -1
}
# Sets CHAIN_N and CHAIN_HEAD from the counter, COPY_N and COPY_HEAD from the
# owner's copy, and the owner's check-in fields.
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
# Wait until the copy and the chain have the same length and head; the chain
# can grow by a check-in between the two reads, so the reads are repeated.
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
# The age of the counter's heartbeat as the instant decision sees it, and
# whether it was verified: sets HB_AGE and HB_VERIFIED.
HB_AGE=; HB_VERIFIED=
read_hb() {
  local line
  xcmd c hbstate || return 1
  line=$(grep '^hbstate ' <<<"$LAST")
  HB_AGE=$(kv "$line" policy_age)
  HB_VERIFIED=$(kv "$line" policy_verified)
}
TXID=
pay_sale() { # address sats
  TXID=$(cli_a -rpcwallet=miner sendtoaddress "$1" "$(btc "$2")")
}
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
HEIGHT=150
# Open a sale, pay it, mine a block and wait for Confirmed.
confirmed_sale() {
  open_sale || return 1
  local s=$SALE
  pay_sale "$SADDR" "$SSATS"
  cli_a generatetoaddress 1 "$MINE" >/dev/null
  HEIGHT=$((HEIGHT + 1))
  await_tip c "$HEIGHT" 60 || return 1
  await_tip o "$HEIGHT" 60 || return 1
  await_state "$s" Confirmed 60
}

# ---- Core, the relay ---------------------------------------------------------------

bitcoind -regtest -datadir="$A_DIR" -rpcport="$A_RPC" \
  -listen=1 -bind="127.0.0.1:$A_P2P" -listenonion=0 -daemonwait -connect=0 \
  -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
  -fallbackfee=0.0001 -dbcache=50 >/dev/null || die "node A did not start"
A_PID=$(cat "$A_DIR/regtest/bitcoind.pid")
cli_a -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli_a -rpcwallet=miner getnewaddress)
cli_a generatetoaddress 150 "$MINE" >/dev/null || die "could not mine"
say "A node that serves filters, at height 150"

OWNER_KEY=$("$PROBE_BIN" key 11) || die "no owner key"
COUNTER_KEY=$("$PROBE_BIN" key 13) || die "no counter key"
THIRD_KEY=$("$PROBE_BIN" key 15) || die "no third key"
[ ${#OWNER_KEY} = 64 ] && [ ${#COUNTER_KEY} = 64 ] && [ ${#THIRD_KEY} = 64 ] || die "the keys are not 64 characters"
WATCH_KEY=$("$PROBE_BIN" key 5a) || die "no watch key"
[ ${#WATCH_KEY} = 64 ] || die "the watch key is not 64 characters"
printf '%s\n%s\n%s\n' "$OWNER_KEY" "$COUNTER_KEY" "$WATCH_KEY" >"$R_DATA/members"
note "members: the owner, the counter and the watch key (LK-74); the third key ($THIRD_KEY) is not listed"

RELAY_LOG="$BASE/relay.log"
start_relay() {
  : >"$RELAY_LOG"
  "$RELAY_BIN" "$R_DATA" "127.0.0.1:$RELAY_PORT" --test-identity >"$RELAY_LOG" 2>&1 &
  RELAY_PID=$!
  for _ in $(seq 1 40); do
    [ "$(wc -l <"$RELAY_LOG")" -ge 3 ] && break
    kill -0 "$RELAY_PID" 2>/dev/null || return 1
    sleep 0.25
  done
  [ "$(wc -l <"$RELAY_LOG")" -ge 3 ]
}
start_relay || die "the relay did not start: $(cat "$RELAY_LOG")"
RELAY_KEY=$(sed -n 1p "$RELAY_LOG")
note "relay key $RELAY_KEY on 127.0.0.1:$RELAY_PORT, $(sed -n 3p "$RELAY_LOG")"

start_ex c "$C_DATA" "$A_P2P" "$RELAY_PORT" "$RELAY_KEY"
start_ex o "$O_DATA" "$A_P2P" "$RELAY_PORT" "$RELAY_KEY"
xawait c "^started " 30 || die "the counter example did not start: $(tail -3 "${LOG[c]}")"
xawait o "^started " 30 || die "the owner example did not start: $(tail -3 "${LOG[o]}")"
READY=
if await_tip c 150 90 && await_tip o 150 90; then READY=yes; fi
[ -n "$READY" ] || die "an example did not reach the tip, height 150, within 90 s"
say "Counter and owner at height 150, both linked to the relay"

# ---- 1 ---------------------------------------------------------------------------

say "1. The owner's heartbeat arrives through the relay"
FOUND=
for _ in $(seq 1 45); do
  read_hb
  if [ "$HB_VERIFIED" = true ] && [ -n "$HB_AGE" ] && [ "$HB_AGE" -le "$FRESH" ]; then FOUND=yes; break; fi
  sleep 1
done
xcmd c linkstate
LS1=$(grep '^linkstate ' <<<"$LAST")
note "hbstate: verified=$HB_VERIFIED age=$HB_AGE"
note "$LS1"
TAKEN=$(kv "$LS1" heartbeats_taken)
if [ -n "$FOUND" ] && [ "${TAKEN:-0}" -ge 1 ] && [ "$(kv "$LS1" reachable)" = true ]; then
  pass 1 "the counter took $TAKEN heartbeat(s) sent by the owner through the relay; the last is verified and $HB_AGE s old (fresh is under $FRESH)"
else
  bad 1 "no fresh verified heartbeat through the relay within 45 s (verified=$HB_VERIFIED, age=$HB_AGE, taken=${TAKEN:-?})"
fi

# ---- 2 ---------------------------------------------------------------------------

say "2. A sale is confirmed; the owner's copy of the chain equals the counter's"
if confirmed_sale; then
  SALE_OK=yes
  note "sale $SALE is Confirmed at height $HEIGHT"
else
  SALE_OK=
fi
if await_same_chain 90; then SAME=yes; else SAME=; fi
read_states
note "counter: $CHAIN_N records, head $CHAIN_HEAD"
note "owner:   $COPY_N records, head $COPY_HEAD"
if [ -n "$SALE_OK" ] && [ -n "$SAME" ] && [ "${CHAIN_N:-0}" -ge 3 ]; then
  pass 2 "sale $SALE reached Confirmed and the owner's copy has the counter's $CHAIN_N records and the same head"
else
  bad 2 "sale confirmed: ${SALE_OK:-no}; chains equal: ${SAME:-no} (counter $CHAIN_N, owner copy $COPY_N)"
fi

# ---- 3 ---------------------------------------------------------------------------

say "3. A check-in arrived and matches"
# The check-in comes as a record and as a message of kind Check-in (LK-57);
# the owner may hold the record before its next poll takes the message.
for _ in $(seq 1 60); do
  read_states && [ -n "$CI_SEQ" ] && [ "$CI_SEQ" != none ] && break
  sleep 0.5
done
xcmd c linkstate
LS3=$(grep '^linkstate ' <<<"$LAST")
xcmd o linkstate
OS3=$(grep '^linkstate ' <<<"$LAST")
note "counter: $LS3"
note "owner:   $OS3"
read_states
note "owner copy: check_in_seq=$CI_SEQ held=$CI_HELD last_check_in_at=$CI_AT"
if [ "$CI_SEQ" != none ] && [ -n "$CI_SEQ" ] && [ "$CI_HELD" = true ] && [ "$CI_AT" != none ] &&
  [ "$(kv "$LS3" last_check_in_at)" != none ] && [ "$(kv "$OS3" alerts)" = 0 ] && [ "$(kv "$OS3" refused)" = 0 ]; then
  pass 3 "the owner holds the counter's check-in at record $CI_SEQ, its own copy has that record at that place, and no alert is raised"
else
  bad 3 "no matching check-in at the owner (seq=$CI_SEQ held=$CI_HELD), or an alert/refusal: $OS3"
fi

# ---- 4 ---------------------------------------------------------------------------

say "4. The relay is stopped"
RELAY_UP=yes
stop_pid "$RELAY_PID"
RELAY_PID=
RELAY_UP=
for _ in $(seq 1 3); do sleep 1; done
STALE=
for _ in $(seq 1 $((MAX_AGE + 30))); do
  read_hb
  if [ -n "$HB_AGE" ] && [ "$HB_AGE" -gt "$MAX_AGE" ]; then STALE=yes; break; fi
  sleep 1
done
note "heartbeat: verified=$HB_VERIFIED age=$HB_AGE (limit $MAX_AGE)"
xcmd c linkstate
note "$(grep '^linkstate ' <<<"$LAST")"
UNREACHED=$(kv "$(grep '^linkstate ' <<<"$LAST")" reachable)
if confirmed_sale; then SALE4=yes; note "sale $SALE is Confirmed at height $HEIGHT with the relay down"; else SALE4=; fi
read_states
note "counter: $CHAIN_N records; owner copy: $COPY_N records (the copy cannot move while the relay is down)"
if [ -n "$STALE" ] && [ -n "$SALE4" ] && [ "$UNREACHED" = false ]; then
  pass 4 "with the relay down the heartbeat aged to $HB_AGE s, above the signed limit of $MAX_AGE, the link says the relay is not reachable, and sale $SALE still reached Confirmed on-chain"
else
  bad 4 "heartbeat stale: ${STALE:-no} (age $HB_AGE); sale confirmed: ${SALE4:-no}; reachable=$UNREACHED"
fi

# ---- 5 ---------------------------------------------------------------------------

say "5. The relay is started again"
if start_relay; then
  RELAY_UP=yes
  note "relay up again, key $(sed -n 1p "$RELAY_LOG")"
  if [ "$(sed -n 1p "$RELAY_LOG")" != "$RELAY_KEY" ]; then
    bad 5 "the relay came back with another key"
  else
    FRESH_AGAIN=
    for _ in $(seq 1 60); do
      read_hb
      if [ "$HB_VERIFIED" = true ] && [ -n "$HB_AGE" ] && [ "$HB_AGE" -le "$FRESH" ]; then FRESH_AGAIN=yes; break; fi
      sleep 1
    done
    note "heartbeat: verified=$HB_VERIFIED age=$HB_AGE"
    if await_same_chain 90; then CAUGHT=yes; else CAUGHT=; fi
    read_states
    note "counter: $CHAIN_N records, head $CHAIN_HEAD"
    note "owner:   $COPY_N records, head $COPY_HEAD"
    if [ -n "$FRESH_AGAIN" ] && [ -n "$CAUGHT" ]; then
      pass 5 "the heartbeat is fresh again ($HB_AGE s) and the owner's copy caught up to the counter's $CHAIN_N records and head"
    else
      bad 5 "heartbeat fresh: ${FRESH_AGAIN:-no} (age $HB_AGE); copy caught up: ${CAUGHT:-no} (counter $CHAIN_N, copy $COPY_N)"
    fi
  fi
else
  bad 5 "the relay did not start again: $(cat "$RELAY_LOG")"
fi

# ---- 6 ---------------------------------------------------------------------------

say "6. A device that is not a member"
if [ -n "$RELAY_UP" ]; then
  PROBE=$("$PROBE_BIN" stranger "127.0.0.1:$RELAY_PORT" "$RELAY_KEY" "$OWNER_KEY" 2>&1)
  note "$(sed 's/^/probe: /' <<<"$PROBE")"
  if grep -q '^make refused ' <<<"$PROBE" && grep -q '^put refused ' <<<"$PROBE" &&
    grep -q '^fetch refused ' <<<"$PROBE" && ! grep -q ' accepted' <<<"$PROBE"; then
    pass 6 "a device whose key is not in the members file was refused when it tried to make a mailbox, to put a message and to fetch"
  else
    bad 6 "the relay did not refuse every attempt: $PROBE"
  fi
else
  notrun 6 "the relay is not running, so there was nothing to probe"
fi

# ---- 7 ---------------------------------------------------------------------------

say "7. A counter started like the app starts it talks to its owner through a Box"
B7="$BASE/box7"
K7="$BASE/keyapp7"
L7="$BASE/counterlink7"
BOX_LOG="$BASE/box7.log"
mkdir -p "$B7" "$K7" "$L7"
INV7=$("$PAIR_BIN" invite "$K7" 2>&1)
printf '%s\n' "$INV7" >"$BASE/invitation7.txt"
: >"$BOX_LOG"
"$BOX_BIN" "$B7" "127.0.0.1:$BOX_PORT" --no-console --invitation-file "$BASE/invitation7.txt" \
  </dev/null >"$BOX_LOG" 2>&1 &
BOX_PID=$!
BOX_UP=
for _ in $(seq 1 40); do
  if grep -q '^state: ' "$BOX_LOG"; then BOX_UP=yes; break; fi
  kill -0 "$BOX_PID" 2>/dev/null || break
  sleep 0.25
done
if [ -n "$BOX_UP" ]; then
  CLAIM7=$("$PAIR_BIN" claim "$K7" "127.0.0.1:$BOX_PORT" 2>&1)
  WELCOME7=$("$PAIR_BIN" welcome "$K7" "127.0.0.1:$BOX_PORT" 2>&1)
  BOX_KEY=$(sed -n 's/^key: //p' "$BOX_LOG" | head -1)
  note "claim: $(head -1 <<<"$CLAIM7"); welcome: $WELCOME7; Box key $BOX_KEY"
  if [ "$WELCOME7" = welcomed ] && [ ${#BOX_KEY} = 64 ]; then
    OUT7=$("$BOXLINK_BIN" "$L7" "127.0.0.1:$BOX_PORT" "$BOX_KEY" "127.0.0.1:$A_P2P" 2>&1)
    RC7=$?
    note "$(sed 's/^/counter: /' <<<"$OUT7")"
    if grep -q '^bundle: version 2' <<<"$OUT7" &&
      grep -q '^paired: the record holds the relay' <<<"$OUT7" &&
      grep -q '^owner: heartbeat .* sent through the Box' <<<"$OUT7" &&
      grep -q '^counter link: ok' <<<"$OUT7"; then
      pass 7 "a counter paired from a bundle with the Box as its relay, and started through the library's start call, took the owner's heartbeat and sent its records and a check-in to the owner, all through the Box"
    else
      bad 7 "the counter's link through the Box did not complete (exit $RC7): $(tail -3 <<<"$OUT7")"
    fi
    if grep -q '^owner: settings version 2 sent through the Box' <<<"$OUT7" &&
      grep -q '^owner: settings version 3 sent through the Box' <<<"$OUT7" &&
      grep -q '^counter: settings version 3 applied' <<<"$OUT7" &&
      grep -q '^owner: Setting records for versions {1, 2, 3} received through the Box' <<<"$OUT7" &&
      grep -q '^counter settings: ok' <<<"$OUT7"; then
      pass 8 "a tightening and a loosening co-signed by the approver went from the owner through the Box to the counter, which applied both, and the owner's copy holds the Setting records for versions 1, 2 and 3"
    else
      bad 8 "the settings did not reach the counter through the Box (exit $RC7): $(tail -3 <<<"$OUT7")"
    fi
    if [ "$RC7" = 0 ] && grep -q '^counter: Home says ' <<<"$OUT7" &&
      grep -q '^counter: a new sale is refused, the counter was removed from the shop (CR-165)' <<<"$OUT7" &&
      grep -q '^counter removal: ok' <<<"$OUT7"; then
      pass 9 "settings that remove the counter reached it through the Box, it applied them, said so on Home and refused to open a new sale"
    else
      bad 9 "the counter removed by the owner's settings did not refuse a new sale (exit $RC7): $(tail -3 <<<"$OUT7")"
    fi
  else
    bad 7 "the Box was not paired: welcome '$WELCOME7', key '$BOX_KEY'"
    notrun 8 "the Box was not paired"
    notrun 9 "the Box was not paired"
  fi
else
  bad 7 "the Box did not start: $(cat "$BOX_LOG")"
  notrun 8 "the Box did not start"
  notrun 9 "the Box did not start"
fi
stop_pid "$BOX_PID"
BOX_PID=

# ---- 10 --------------------------------------------------------------------------

say "10. A batch the relay loses is sent again, and the counter holds it once"
if [ -z "$RELAY_UP" ]; then
  notrun 10 "the relay is not running"
else
  xcmd c batches
  B0=$(grep '^batches ' <<<"$LAST")
  REM0=$(kv "$B0" remaining)
  note "counter before: $B0"
  if [ "$(kv "$B0" records)" != 0 ] || [ -z "$REM0" ]; then
    bad 10 "the counter already shows a batch, or the example printed no batches line: $B0"
  elif ! stop_ex c; then
    bad 10 "the counter example did not stop"
  else
    xcmd o "sendbatch 20"
    SENT8=$(grep '^sendbatch ' <<<"$LAST")
    note "owner: $SENT8"
    xcmd o batches
    note "owner: $(grep '^batches ' <<<"$LAST")"
    # The relay has the message. Take it away: stop the relay, remove its
    # mailboxes (the members file stays), start it again.
    stop_pid "$RELAY_PID"
    RELAY_PID=
    rm -rf -- "$R_DATA/mailboxes"
    if ! start_relay || [ "$(sed -n 1p "$RELAY_LOG")" != "$RELAY_KEY" ]; then
      bad 10 "the relay did not start again with the same key: $(cat "$RELAY_LOG")"
    else
      start_ex c "$C_DATA" "$A_P2P" "$RELAY_PORT" "$RELAY_KEY"
      if ! xawait c "^started " 30 || ! await_tip c "$HEIGHT" 60; then
        bad 10 "the counter example did not start again: $(tail -3 "${LOG[c]}")"
      else
        HELD8=
        for _ in $(seq 1 90); do
          xcmd c batches || break
          B8=$(grep '^batches ' <<<"$LAST")
          if [ "$(kv "$B8" records)" = 1 ] && [ "$(kv "$B8" ranges)" = 20-40 ]; then HELD8=yes; break; fi
          sleep 1
        done
        note "counter after: ${B8:-none}"
        # The owner's copy has the counter's record, and the batch stops waiting.
        GONE8=
        for _ in $(seq 1 60); do
          xcmd o batches
          O8=$(grep '^batches ' <<<"$LAST")
          if [ "$(kv "$O8" waiting)" = 0 ]; then GONE8=yes; break; fi
          sleep 1
        done
        note "owner after: ${O8:-none}"
        # The same batch handed over again changes nothing at the counter.
        xcmd o "sendbatch 20"
        sleep $((BATCH_RESEND * 3))
        xcmd c batches
        B8B=$(grep '^batches ' <<<"$LAST")
        note "counter after a second hand-over: $B8B"
        REM8=$(kv "$B8B" remaining)
        if [ -n "$HELD8" ] && [ -n "$GONE8" ] && [ "$(kv "$B8B" records)" = 1 ] &&
          [ "$REM8" = $((REM0 + 20)) ] && await_same_chain 60; then
          pass 10 "the first message was lost with the relay's mailboxes; the counter held the batch 20-40 after the owner's resend ($REM0 addresses left before, $REM8 after), the owner's copy showed it and the batch stopped waiting, and a second hand-over left one record"
        else
          bad 10 "held: ${HELD8:-no}; stopped waiting: ${GONE8:-no}; records after the second hand-over: $(kv "$B8B" records); addresses left $REM0 -> ${REM8:-?}"
        fi
      fi
    fi
  fi
fi

# ---- 11 --------------------------------------------------------------------------

say "11. The owner names a watch key, with the approver's co-signature"
SET_NAME=$("$WATCH_BIN" --settings 2 name --heartbeat-max-age "$MAX_AGE" | sed -n 's/^settings //p')
SET_REMOVE=$("$WATCH_BIN" --settings 3 remove --heartbeat-max-age "$MAX_AGE" | sed -n 's/^settings //p')
if [ -z "$SET_NAME" ] || [ -z "$SET_REMOVE" ]; then
  bad 11 "the settings could not be made"
else
  xcmd c "settings $SET_NAME"
  note "$(grep '^settings ' <<<"$LAST")"
  if grep -q "^settings taken version=2 change=Loosening watch_key=$WATCH_KEY\$" <<<"$LAST"; then
    pass 11 "the settings that name the watch key, signed by the owner and the approver, were taken by the counter as a loosening, and it names $WATCH_KEY"
  else
    bad 11 "the counter did not take the settings that name the watch key: $(grep '^settings ' <<<"$LAST")"
  fi
fi

# ---- 12 --------------------------------------------------------------------------

say "12. The owner stops"
if stop_ex o; then OWNER_STOPPED=yes; else OWNER_STOPPED=; fi
GONE=
for _ in $(seq 1 $((FRESH + 40))); do
  read_hb
  if [ -n "$HB_AGE" ] && [ "$HB_AGE" -gt "$FRESH" ]; then GONE=yes; break; fi
  sleep 1
done
note "heartbeat: verified=$HB_VERIFIED age=$HB_AGE (fresh is under $FRESH)"
if [ -n "$OWNER_STOPPED" ] && [ -n "$GONE" ]; then
  pass 12 "the owner example stopped cleanly and the counter's heartbeat aged to $HB_AGE s, past the fresh limit of $FRESH: without the owner, instant sales would stop"
else
  bad 12 "owner stopped: ${OWNER_STOPPED:-no}; heartbeat aged past $FRESH s: ${GONE:-no} (age $HB_AGE)"
fi

# ---- 13 --------------------------------------------------------------------------

say "13. A watch-only owner sends heartbeats; the counter takes them"
xcmd c linkstate
TAKEN_BEFORE=$(kv "$(grep '^linkstate ' <<<"$LAST")" heartbeats_taken)
start_ex w "$O_DATA" "$A_P2P" "$RELAY_PORT" "$RELAY_KEY"
if xawait w "^started " 60; then
  FOUND=
  for _ in $(seq 1 60); do
    read_hb
    if [ "$HB_VERIFIED" = true ] && [ -n "$HB_AGE" ] && [ "$HB_AGE" -le "$FRESH" ]; then FOUND=yes; break; fi
    sleep 1
  done
  xcmd c linkstate
  LS12=$(grep '^linkstate ' <<<"$LAST")
  TAKEN_AFTER=$(kv "$LS12" heartbeats_taken)
  xcmd w state
  WS12=$(grep '^state ' <<<"$LAST")
  note "hbstate: verified=$HB_VERIFIED age=$HB_AGE"
  note "counter: heartbeats taken $TAKEN_BEFORE before, $TAKEN_AFTER after"
  note "watcher: $WS12"
  if [ -n "$FOUND" ] && [ "${TAKEN_AFTER:-0}" -gt "${TAKEN_BEFORE:-0}" ] &&
    [ "$(kv "$WS12" reached)" = true ] && [ "$(kv "$WS12" relay_refused)" = false ] &&
    [ "$(kv "$WS12" sent)" -ge 1 ]; then
    pass 13 "with the owner example stopped, the watch-only owner sent heartbeats signed by the watch key through the relay; the counter took $((TAKEN_AFTER - TAKEN_BEFORE)) of them, and its heartbeat is verified and $HB_AGE s old, so the owner's word for instant sales is there"
  else
    bad 13 "heartbeat fresh and verified: ${FOUND:-no} (age $HB_AGE); taken $TAKEN_BEFORE to $TAKEN_AFTER; watcher: $WS12"
  fi
else
  bad 13 "the watch-only owner did not start: $(tail -3 "${LOG[w]}")"
fi

# ---- 14 --------------------------------------------------------------------------

say "14. The watch key is removed; its heartbeats are refused"
if [ -n "${SET_REMOVE:-}" ] && [ -n "${PID[w]}" ]; then
  xcmd c "linkstate"
  SEQ_BEFORE=$(kv "$(grep '^linkstate ' <<<"$LAST")" heartbeats_taken)
  xcmd c "settings $SET_REMOVE"
  note "$(grep '^settings ' <<<"$LAST")"
  REMOVED=
  grep -q "^settings taken version=3 change=Tightening watch_key=none\$" <<<"$LAST" && REMOVED=yes
  # The watcher goes on sending; the counter must stop taking, and the
  # heartbeat ages out.
  xcmd w heartbeat
  HB_HEX=$(sed -n 's/^heartbeat //p' <<<"$LAST")
  xcmd c linkstate
  TAKEN13=$(kv "$(grep '^linkstate ' <<<"$LAST")" heartbeats_taken)
  STALE13=
  for _ in $(seq 1 $((FRESH + 40))); do
    read_hb
    if [ -n "$HB_AGE" ] && [ "$HB_AGE" -gt "$FRESH" ]; then STALE13=yes; break; fi
    sleep 1
  done
  xcmd c linkstate
  LS13=$(grep '^linkstate ' <<<"$LAST")
  note "heartbeats taken at removal: $TAKEN13; later: $(kv "$LS13" heartbeats_taken); refused by the courier: $(kv "$LS13" refused)"
  xcmd w state
  note "watcher: $(grep '^state ' <<<"$LAST") (it goes on sending)"
  xcmd c "heartbeat $HB_HEX"
  HAND13=$(grep '^heartbeat ' <<<"$LAST")
  note "by hand: $HAND13"
  if [ -n "$REMOVED" ] && [ -n "$STALE13" ] && [ -n "$HB_HEX" ] &&
    [ "$(kv "$LS13" heartbeats_taken)" = "$TAKEN13" ] && grep -q '^heartbeat refused' <<<"$HAND13"; then
    pass 14 "the settings that remove the watch key were taken as a tightening; the watcher kept sending, but the counter took no more of its heartbeats through the relay (taken stayed at $TAKEN13, the heartbeat aged to $HB_AGE s), and the same key's heartbeat handed over by hand was refused"
  else
    bad 14 "removed: ${REMOVED:-no}; heartbeat aged: ${STALE13:-no}; taken $TAKEN13 to $(kv "$LS13" heartbeats_taken); by hand: $HAND13"
  fi
else
  bad 14 "the watcher did not run, so there was nothing to remove"
fi
stop_ex w || note "the watch-only owner did not stop cleanly"

stop_ex c || note "the counter example did not stop cleanly"
[ -z "${PID[o]}" ] || stop_ex o || note "the owner example did not stop cleanly"

# ---- result ----------------------------------------------------------------------

say "Result"
failed=0
notrun=0
for n in 1 2 3 4 5 6 7 8 9 10 11 12 13 14; do
  printf '%s  step %s\n' "${RESULT[$n]:-FAIL}" "$n"
  case "${RESULT[$n]:-FAIL}" in
    PASS) ;;
    "NOT RUN") notrun=$((notrun + 1)) ;;
    *) failed=1 ;;
  esac
done
if [ "$failed" != 0 ]; then
  echo "SOME STEPS FAILED"
  exit 1
elif [ "$notrun" != 0 ]; then
  echo "PASS for the steps that ran; $notrun steps NOT RUN"
else
  echo "ALL PASS"
fi
