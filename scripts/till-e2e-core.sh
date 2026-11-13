#!/usr/bin/env bash
# A sale from the till, through a real Box, to a real counter, next to
# Bitcoin Core on regtest (docs/COUNTER-RULES.md section 27, CR-266 to
# CR-271; docs/BOX-RULES.md section 5c, BX-70 to BX-79).
#
# Question: does a sale that a till asks the Box for reach the counter, wait
# there for the cashier, open as an ordinary sale on Start, get paid on the
# chain, and come back to the till as `paid`; and what do the counter and the
# till see when the till cancels, before Start and after it?
#
# One private regtest node, A, serving compact filters. The Box is the real
# `openbmt-box` binary with `--till-listen`, paired by the `box_pair` example
# (the Key app and the owner phone), with the counter of `counter_regtest`
# listed in its first settings (`welcome ... with-rc`). The counter is the
# `counter_regtest` example with `--box-key`: the Box is its relay, a Box of
# its settings and a Box in its courier's table. The till is played by curl,
# with the till key from `openbmt-box till-key`, sent as the bearer token and
# nonce of BX-71 and BX-80, each answer checked by its reply mac. The script
# carries nothing between the devices: the sale request, the notices and the
# cancels all go through the Box's own port.
#
# THE HEALTH RULES ARE NOT CHECKED HERE. The counter is started with
# `--regtest-single-peer`, as in `scripts/link-e2e-core.sh`. Sales are
# confirmed by a block.
#
# The steps (printed as a table at the end):
#   1. The till asks for a sale; the counter holds it with its amount,
#      reference and minutes left; Not now hides it and Start still opens it.
#   2. Start opens the sale as a typed sale is opened, the Box reads `opened`,
#      the customer pays, and the till reads `paid`.
#   3. A sale the till cancels before Start leaves the counter's list, and
#      the till reads `cancelled`.
#   4. A sale the till cancels after Start is marked on the counter and goes
#      on; the payment that arrives anyway is paid, and the till reads `paid`.
#
# Environment:
#   CORE_BIN       the folder of Bitcoin Core's binaries
#                  (default $HOME/.local/opt/bitcoin-core-32.1/bin)
#   DRILL_PORTS    the first of five ports (default 18986)
#   DRILL_DIR      a scratch directory (default a new one; removed on exit
#                  unless DRILL_KEEP=1)
#   COUNTER_BIN, BOX_BIN, PAIR_BIN   binaries; built if not given
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
TILL_PORT=$((P0 + 4))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/till-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
A_DIR="$BASE/core-a"
C_DATA="$BASE/counter"
B_DATA="$BASE/box"
K_DATA="$BASE/keyapp"
SALE_RATE=10000000    # $100,000 per bitcoin, in cents

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

for tool in bitcoind bitcoin-cli jq curl; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH"
done
[ "$(command -v bitcoind)" = "$CORE_BIN/bitcoind" ] || die "bitcoind is not the one in $CORE_BIN"
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) die "this check is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

mkdir -p "$A_DIR" "$C_DATA" "$B_DATA" "$K_DATA"
A_PID=
BOX_PID=
C_PID=
LOG_C="$BASE/counter.log"
LOG_BOX="$BASE/box.log"
CNT=0

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
  exec 3>&- 2>/dev/null || true
  stop_pid "$C_PID"
  stop_pid "$BOX_PID"
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
if [ -z "${COUNTER_BIN:-}" ] || [ -z "${BOX_BIN:-}" ] || [ -z "${PAIR_BIN:-}" ]; then
  say "Build the counter example, the Box and the pairing example"
  build -p openbmt-counter-node --example counter_regtest
  build -p openbmt-box --bins --example box_pair
  COUNTER_BIN=${COUNTER_BIN:-$TARGET/debug/examples/counter_regtest}
  BOX_BIN=${BOX_BIN:-$TARGET/debug/openbmt-box}
  PAIR_BIN=${PAIR_BIN:-$TARGET/debug/examples/box_pair}
fi
for bin in COUNTER_BIN BOX_BIN PAIR_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done

# ---- helpers ---------------------------------------------------------------------

# One command to the counter, and what it printed up to its `done`.
LAST=
xcmd() { # command [seconds]
  local before i n
  CNT=$((CNT + 1))
  n=$CNT
  before=$(wc -l <"$LOG_C")
  echo "$1" >&3
  for i in $(seq 1 $((${2:-30} * 4))); do
    if grep -q "^done $n\$" "$LOG_C"; then
      LAST=$(tail -n +$((before + 1)) "$LOG_C")
      return 0
    fi
    kill -0 "$C_PID" 2>/dev/null || break
    sleep 0.25
  done
  LAST=$(tail -n +$((before + 1)) "$LOG_C")
  return 1
}
await_tip() { # height seconds
  local i
  for i in $(seq 1 $(($2 * 2))); do
    xcmd status || return 1
    grep -q "^status tip=$1 " <<<"$LAST" && return 0
    sleep 0.5
  done
  return 1
}
kv() { # line key
  sed -nE "s/^(.* )?$2=([^ ]*).*/\2/p" <<<"$1" | head -1
}
# A tagged hash as BIP340 makes it (BX-80): SHA-256 of the SHA-256 of the
# tag twice, then the message; the message is given as hex.
tagged() { # tag hex-message -> 64 hex
  local t
  t=$(printf '%s' "$1" | sha256sum | cut -d' ' -f1)
  { printf '%s%s%s' "$t" "$t" "$2" | xxd -r -p; } | sha256sum | cut -d' ' -f1
}
# The till holds the key; it sends the bearer token made from it, a fresh
# nonce, and takes an answer only when its X-OBM-Reply is the mac of this
# request, the SHA-256 of its body included (BX-71, BX-80). An answer that does not check is printed as
# {"error":"replyNotChecked"}.
till() { # method path [json]
  local nonce hdr body status mac want sent
  nonce=$(head -c 16 /dev/urandom | xxd -p)
  hdr=$(mktemp "$BASE/till-hdr-XXXXXX")
  body=$(mktemp "$BASE/till-body-XXXXXX")
  local args=(-s --max-time 10 -X "$1" -D "$hdr" -o "$body"
    -H "Authorization: Bearer $TILL_TOKEN" -H "X-OBM-Nonce: $nonce")
  sent=$(printf '' | sha256sum | cut -d' ' -f1)
  if [ $# -ge 3 ]; then
    sent=$(printf '%s' "$3" | sha256sum | cut -d' ' -f1)
    args+=(-H 'Content-Type: application/json' --data "$3")
  fi
  curl "${args[@]}" "http://127.0.0.1:$TILL_PORT$2"
  status=$(sed -nE '1s/^HTTP\/[0-9.]+ ([0-9]{3}).*/\1/p' "$hdr" | head -1)
  mac=$(tr -d '\r' <"$hdr" | sed -nE 's/^[Xx]-[Oo][Bb][Mm]-[Rr]eply: *([0-9a-f]{64})$/\1/p' | head -1)
  want=$({ printf 'OBM-TILL-REPLY-2\n%s\n%s\n%s\n%s\n%s\n' "$1" "$2" "$nonce" "$sent" "$status"; cat "$body"; } |
    openssl dgst -sha256 -mac HMAC -macopt "hexkey:$TILL_REPLY" -r | cut -d' ' -f1)
  if [ -n "$mac" ] && [ "$mac" = "$want" ]; then
    cat "$body"
  else
    printf '{"error":"replyNotChecked"}'
  fi
  rm -f "$hdr" "$body"
}
till_state() { # sale
  till GET "/till/v1/sales/$1" | jq -r '.state // .error'
}
# Wait for the Box to read `state` for `sale`.
await_till() { # sale state seconds
  local i s
  for i in $(seq 1 $(($3 * 2))); do
    s=$(till_state "$1")
    [ "$s" = "$2" ] && return 0
    sleep 0.5
  done
  TILL_SEEN=$s
  return 1
}
TILL_SEEN=
# Wait until the counter lists `n` requests.
await_waiting() { # n seconds
  local i
  for i in $(seq 1 $(($2 * 2))); do
    xcmd till-waiting || return 1
    grep -q "^till waiting=$1\$" <<<"$LAST" && return 0
    sleep 0.5
  done
  return 1
}
# Ask for a sale of $5.00 as the till does; sets SALE (32 hex).
ask() { # request-id reference
  local out
  out=$(till POST /till/v1/sales \
    "{\"counter\":\"$CTR\",\"amount\":\"5.00\",\"currency\":\"USD\",\"request\":\"$1\",\"reference\":\"$2\"}")
  ASKED=$out
  SALE=$(jq -r '.sale // empty' <<<"$out")
  [ ${#SALE} = 32 ] && [ "$(jq -r .state <<<"$out")" = waiting ]
}
start_sale() { # sale hex -> sets IDX, SADDR, SSATS
  xcmd "till-start $1 $SALE_RATE" || return 1
  local line
  line=$(grep '^opened ' <<<"$LAST" || true)
  IDX=$(kv "$line" index)
  SADDR=$(kv "$line" address)
  SSATS=$(kv "$line" sats)
  [ -n "$IDX" ] && [ -n "$SADDR" ] && [ -n "$SSATS" ]
}
HEIGHT=150
pay_and_confirm() { # address sats
  cli_a -rpcwallet=miner sendtoaddress "$1" "$(btc "$2")" >/dev/null || return 1
  cli_a generatetoaddress 1 "$MINE" >/dev/null || return 1
  HEIGHT=$((HEIGHT + 1))
  await_tip "$HEIGHT" 60
}
IDX=; SADDR=; SSATS=; SALE=; ASKED=

# ---- Core, the Box, the counter -------------------------------------------------

bitcoind -regtest -datadir="$A_DIR" -rpcport="$A_RPC" \
  -listen=1 -bind="127.0.0.1:$A_P2P" -listenonion=0 -daemonwait -connect=0 \
  -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
  -fallbackfee=0.0001 -dbcache=50 >/dev/null || die "node A did not start"
A_PID=$(cat "$A_DIR/regtest/bitcoind.pid")
cli_a -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli_a -rpcwallet=miner getnewaddress)
cli_a generatetoaddress 150 "$MINE" >/dev/null || die "could not mine"
say "A node that serves filters, at height 150"

"$PAIR_BIN" invite "$K_DATA" >"$BASE/invitation.txt" 2>&1 || die "no invitation"
"$BOX_BIN" "$B_DATA" "127.0.0.1:$BOX_PORT" --no-console --invitation-file "$BASE/invitation.txt" \
  --till-listen "127.0.0.1:$TILL_PORT" </dev/null >"$LOG_BOX" 2>&1 &
BOX_PID=$!
for _ in $(seq 1 40); do
  grep -q '^state: ' "$LOG_BOX" && grep -q '^till listener: ' "$LOG_BOX" && break
  kill -0 "$BOX_PID" 2>/dev/null || break
  sleep 0.25
done
grep -q '^till listener: ' "$LOG_BOX" || die "the Box did not start: $(cat "$LOG_BOX")"
"$PAIR_BIN" claim "$K_DATA" "127.0.0.1:$BOX_PORT" >/dev/null 2>&1 || die "the claim failed"
[ "$("$PAIR_BIN" welcome "$K_DATA" "127.0.0.1:$BOX_PORT" with-rc 2>&1)" = welcomed ] ||
  die "the Box was not welcomed"
# The owner's mailbox, which the counter's records go to (LK-20).
"$PAIR_BIN" mailbox "$K_DATA" "127.0.0.1:$BOX_PORT" owner >/dev/null 2>&1 || die "no owner mailbox"
BOX_KEY=$(sed -n 's/^key: //p' "$LOG_BOX" | head -1)
[ ${#BOX_KEY} = 64 ] || die "no Box key"
TILL_KEY=$("$BOX_BIN" till-key "$B_DATA" 2>&1 | grep -Eo '[0-9a-f]{64}' | head -1)
[ ${#TILL_KEY} = 64 ] || die "no till key"
TILL_TOKEN=$(tagged OpenBMT/till-bearer/v1 "$TILL_KEY")
TILL_REPLY=$(tagged OpenBMT/till-reply/v1 "$TILL_KEY")
RC_KEY=$("$PAIR_BIN" key "$K_DATA" rc) || die "no counter key"
CTR=${RC_KEY:0:8}
say "A Box paired with the counter $CTR, the till listener on 127.0.0.1:$TILL_PORT"

FIFO="$BASE/stdin-c"
mkfifo "$FIFO"
exec 3<>"$FIFO"
: >"$LOG_C"
"$COUNTER_BIN" "$C_DATA" "127.0.0.1:$A_P2P" --regtest-single-peer --customers-off-wifi \
  --relay "127.0.0.1:$BOX_PORT" --relay-key "$BOX_KEY" --box-key "$BOX_KEY" \
  <&3 >"$LOG_C" 2>&1 &
C_PID=$!
for _ in $(seq 1 120); do
  grep -q '^started ' "$LOG_C" && break
  kill -0 "$C_PID" 2>/dev/null || break
  sleep 0.25
done
grep -q '^started ' "$LOG_C" || die "the counter did not start: $(tail -3 "$LOG_C")"
await_tip 150 90 || die "the counter did not reach height 150"
say "The counter is at height 150, linked to the Box"

# ---- 1 ---------------------------------------------------------------------------

say "1. A request waits on the counter; Not now hides it; Start opens it"
if ask req-one "ORDER-17" && await_waiting 1 40; then
  S1=$SALE
  LINE=$(grep '^till sale=' <<<"$LAST" | head -1)
  note "$LINE"
  OK1=
  # The answer names the amount (with the currency's two decimals), the
  # currency, the request and the reference the Box took (BX-72).
  ECHO1=$(jq -r '[.amount, .currency, .request, .reference] | join("|")' <<<"$ASKED")
  [ "$ECHO1" = "5.00|USD|req-one|ORDER-17" ] || note "the create answer echoes '$ECHO1'"
  [ "$(kv "$LINE" sale)" = "$S1" ] && [ "$(kv "$LINE" amount)" = 500 ] &&
    [ "$(kv "$LINE" reference)" = "ORDER-17" ] && [ "$(kv "$LINE" minutes_left)" -ge 4 ] &&
    [ "$ECHO1" = "5.00|USD|req-one|ORDER-17" ] && OK1=yes
  # The same request again is the same sale and sends nothing new.
  AGAIN=$(till POST /till/v1/sales \
    "{\"counter\":\"$CTR\",\"amount\":\"5.00\",\"currency\":\"USD\",\"request\":\"req-one\",\"reference\":\"ORDER-17\"}" |
    jq -r .sale)
  xcmd "till-not-now $S1"
  HID=$(grep -c '^till not-now ok' <<<"$LAST")
  xcmd till-waiting
  SHOWN=$(grep '^till waiting=' <<<"$LAST")
  if [ -n "$OK1" ] && [ "$AGAIN" = "$S1" ] && [ "$HID" = 1 ] && [ "$SHOWN" = "till waiting=0" ] &&
    [ "$(till_state "$S1")" = waiting ]; then
    pass 1 "the till's request waited on the counter with its amount, reference and minutes left, the answer named the amount, currency, request and reference the Box took, Not now hid it and sent nothing, the Box still read waiting"
  else
    bad 1 "the request did not wait as it should: line '$LINE', again '$AGAIN', not-now '$HID', shown '$SHOWN'"
  fi
else
  bad 1 "the request did not reach the counter's list: $(tail -3 "$LOG_C")"
  S1=
fi

# ---- 2 ---------------------------------------------------------------------------

say "2. Start, payment, paid at the till"
if [ -n "$S1" ] && start_sale "$S1"; then
  I2=$IDX
  note "opened index=$I2 address=$SADDR sats=$SSATS"
  xcmd "till-of $I2"
  OF2=$LAST
  if await_till "$S1" opened 40; then
    OPENED2=yes
  else
    OPENED2=
  fi
  if pay_and_confirm "$SADDR" "$SSATS" && await_till "$S1" paid 60; then
    ANS=$(till GET "/till/v1/sales/$S1")
    note "the till reads: $ANS"
    if [ -n "$OPENED2" ] && grep -q "sale=$S1 cancelled=false" <<<"$OF2" &&
      [ "$(jq -r .words.what <<<"$ANS")" = "Paid in bitcoin." ]; then
      pass 2 "Start opened sale $I2 naming the till's sale, the Box read opened, the payment was confirmed on the chain, and the till read paid"
    else
      bad 2 "the sale was paid but not as it should be: opened '$OPENED2', of '$OF2', till '$ANS'"
    fi
  else
    bad 2 "the till did not read paid (it reads '$TILL_SEEN'); opened at the Box: '$OPENED2'"
  fi
else
  bad 2 "Start did not open a sale: $(tail -3 "$LOG_C")"
fi

# ---- 3 ---------------------------------------------------------------------------

say "3. A cancel before Start"
if ask req-two "ORDER-18" && await_waiting 1 40; then
  S3=$SALE
  CANCEL=$(till POST "/till/v1/sales/$S3/cancel")
  if await_waiting 0 40 && await_till "$S3" cancelled 20; then
    xcmd "till-start $S3 $SALE_RATE"
    if grep -q '^refused ' <<<"$LAST"; then
      pass 3 "the till's cancel took the request off the counter's list, a later Start found nothing to start, and the till read cancelled"
    else
      bad 3 "a cancelled request could still be started: $LAST"
    fi
  else
    bad 3 "the cancel did not clear the counter's list: cancel '$CANCEL', till '$TILL_SEEN'"
  fi
else
  bad 3 "the second request did not reach the counter: $(tail -3 "$LOG_C")"
fi

# ---- 4 ---------------------------------------------------------------------------

say "4. A cancel after Start, and a payment that arrives anyway"
if ask req-three "ORDER-19" && await_waiting 1 40; then
  S4=$SALE
  if start_sale "$S4"; then
    I4=$IDX
    await_till "$S4" opened 40
    till POST "/till/v1/sales/$S4/cancel" >/dev/null
    MARKED=
    for _ in $(seq 1 80); do
      xcmd "till-of $I4"
      if grep -q "till-of index=$I4 sale=$S4 cancelled=true" <<<"$LAST"; then MARKED=yes; break; fi
      sleep 0.5
    done
    xcmd "judge $I4"
    STATE4=$(sed -nE 's/^sale [0-9]+ state=([A-Za-z]+) .*/\1/p' <<<"$LAST" | head -1)
    note "the sale after the cancel: state=$STATE4"
    if [ -n "$MARKED" ] && [ "$(till_state "$S4")" = cancelled ] &&
      pay_and_confirm "$SADDR" "$SSATS" && await_till "$S4" paid 60; then
      pass 4 "the till's cancel marked the started sale on the counter and it went on; the payment that arrived anyway was taken and the till read paid"
    else
      bad 4 "the late payment did not end as paid: marked '$MARKED', till '$(till_state "$S4")', state '$STATE4'"
    fi
  else
    bad 4 "Start did not open the third sale: $(tail -3 "$LOG_C")"
  fi
else
  bad 4 "the third request did not reach the counter: $(tail -3 "$LOG_C")"
fi

# ---- the table -----------------------------------------------------------------------

say "Summary"
FAILS=0
for s in 1 2 3 4; do
  printf '%-4s step %s: %s\n' "${RESULT[$s]:-NOT RUN}" "$s" "${MESSAGE[$s]:-}"
  [ "${RESULT[$s]:-}" = PASS ] || FAILS=$((FAILS + 1))
done
echo quit >&3
for _ in $(seq 1 40); do
  grep -q '^stopped$' "$LOG_C" && break
  sleep 0.25
done
[ "$FAILS" = 0 ] && echo "ALL PASS" || echo "$FAILS step(s) did not pass"
[ "$FAILS" = 0 ]
