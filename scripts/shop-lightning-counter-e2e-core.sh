#!/usr/bin/env bash
# Shop Lightning at the counter, on regtest, against Bitcoin Core 32.1, an
# LSPS2 provider, a payer and a real Box (docs/SHOP-LIGHTNING-RULES.md SL-70
# to SL-78, docs/LINK-RULES.md LK-82, docs/BOX-RULES.md BX-90 to BX-94).
#
# Question: does the counter that the settings name run the node, keep its
# second copy on the Box, offer an invoice only when the rules allow it, mark a
# sale paid only after both copies are stored, and wind down when the settings
# stop naming it?
#
# What it runs. Bitcoin Core serving compact filters; LND (the payer); the
# provider (`tools/shop-lightning-lsp`); the real `openbmt-box`, paired by the
# `box_pair` example with first settings that name the counter of
# `counter_regtest` as the Lightning counter and the Box as its second device;
# and the counter, the `counter_regtest` example with `--shop-ln`, paired with
# a fixed single-key wallet. The counter's node takes its chain data from the
# counter's own filter driver (SL-26) and reaches the Box over the relay
# connection (LK-82). The seed record is the file `shop-seed.bin`, which the
# app would keep sealed.
#
# The checks (one `PASS` line each; the script ends `ALL PASS` only if every
# check passed):
#   (a) a seed of another node ID is refused with its rule and nothing is
#       stored (SL-72); the seed of the pinned ID is taken
#   (b) a first payment opens a channel through the provider, and the Box
#       holds the channel monitor before the sale is marked paid (SL-21)
#   (c) with the Box stopped the counter offers on-chain only, and Lightning
#       comes back when the Box does (SL-41)
#   (d) a counter that the settings do not name offers on-chain only and
#       makes nothing on disk (SL-70)
#   (e) settings without `shop_lightning` stop the invoices, close the channel
#       (SL-36) and then delete the seed (SL-77)
#
# Not part of any gate. Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin,
# LND in ~/.local/opt/lnd-0.21.8, jq, and cargo (through ~/.cache/openbmt/guard.sh).
#
# Environment:
#   CORE_BIN       directory with bitcoind and bitcoin-cli
#   LND_DIR        directory with lnd and lncli
#   DRILL_PORTS    first of ten free ports on 127.0.0.1 (default 19780)
#   DRILL_DIR      scratch directory (default a new one under ~/.cache/openbmt)
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   COUNTER_BIN, BOX_BIN, PAIR_BIN, LSP_BIN   binaries; built if not given
#
# Everything here is regtest on 127.0.0.1 with keys made from fixed test
# bytes. No real keys or funds are involved, and nothing of ~/.bitcoin or
# ~/.lnd is used. Every process is stopped by its PID, and the scratch
# directory removed, on every exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
LND_DIR=${LND_DIR:-$HOME/.local/opt/lnd-0.21.8}
export PATH="$CORE_BIN:$LND_DIR:$PATH"
ROOT=$(cd "$(dirname "$0")/.." && pwd)
P0=${DRILL_PORTS:-19780}
BTC_RPC=$P0
BTC_P2P=$((P0 + 1))
ZMQ_BLK=$((P0 + 2))
ZMQ_TX=$((P0 + 3))
LSP_P2P=$((P0 + 4))
LSP_CTL=$((P0 + 5))
LND_P2P=$((P0 + 6))
LND_RPC=$((P0 + 7))
LND_REST=$((P0 + 8))
BOX_PORT=$((P0 + 9))
RPC_USER=shopln
RPC_PASS=shopln_pw
SALE_RATE=10000000   # $100,000 per bitcoin, in cents: $50.00 is 50,000 sats
SALE_CENTS=5000
CAP_SATS=100000
MAX_FEE_SATS=3000

# Seeds: the stick's 64 bytes as 128 hex. The right one is pinned in the
# settings; the other has another node ID.
SEED_OK=$(printf '%02x' $(seq 1 64) | tr -d ' ')
SEED_BAD=$(printf '%02x' $(seq 101 164) | tr -d ' ')

mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/shop-ln-counter-e2e-XXXXXX")}
BTC_DIR="$BASE/bitcoind"
LND_HOME="$BASE/lnd"
LSP_DIR="$BASE/lsp"
K_DATA="$BASE/keyapp"
B_DATA="$BASE/box"
C_DATA="$BASE/counter"
D_DATA="$BASE/counter-other"
mkdir -p "$BTC_DIR" "$LND_HOME" "$LSP_DIR" "$K_DATA" "$B_DATA" "$C_DATA" "$D_DATA"

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
FAILED=0
pass() { printf 'PASS  %s\n' "$*"; }
fail() { printf 'FAIL  %s\n' "$*"; FAILED=1; }
check() { # description condition-exit-status
  if [ "$2" = 0 ]; then pass "$1"; else fail "$1"; fi
}

for tool in bitcoind bitcoin-cli lnd lncli jq; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH"
done
[ "$(command -v bitcoind)" = "$CORE_BIN/bitcoind" ] || die "bitcoind is not the one in $CORE_BIN"
case "$(bitcoind --version | head -1)" in
  "Bitcoin Core daemon version v32.1"*) ;;
  *) die "this check is for Bitcoin Core 32.1; found: $(bitcoind --version | head -1)" ;;
esac
say "$(bitcoind --version | head -1); $(lnd --version)"

bcli() { bitcoin-cli -regtest -datadir="$BTC_DIR" -rpcport="$BTC_RPC" -rpcuser="$RPC_USER" -rpcpassword="$RPC_PASS" "$@"; }
bw() { local w=$1; shift; bcli -rpcwallet="$w" "$@"; }
lnc() {
  lncli --lnddir="$LND_HOME" --tlscertpath="$LND_HOME/tls.cert" --network=regtest \
    --rpcserver="127.0.0.1:$LND_RPC" --no-macaroons "$@"
}
mine() { bw miner generatetoaddress "$1" "$(cat "$BASE/miner.addr")" >/dev/null; }
ctl() { # port command...
  local port=$1
  shift
  timeout "${CTL_TIMEOUT:-60}" bash -c 'exec 3<>/dev/tcp/127.0.0.1/$0; echo "$1" >&3; cat <&3' "$port" "$*" 2>/dev/null
}
wait_port() { # port seconds
  local i
  for i in $(seq 1 "${2:-60}"); do
    (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null && return 0
    sleep 1
  done
  return 1
}

declare -A PIDS
stop_pid() { # name
  local pid=${PIDS[$1]:-} i
  [ -n "$pid" ] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null || true
    for i in $(seq 1 40); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.25
    done
    kill -9 "$pid" 2>/dev/null || true
  fi
  PIDS[$1]=
}
cleanup() {
  exec 3>&- 4>&- 2>/dev/null || true
  stop_pid counter
  stop_pid other
  stop_pid box
  stop_pid lsp
  stop_pid lnd
  if [ -n "${PIDS[bitcoind]:-}" ]; then
    bcli stop >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do kill -0 "${PIDS[bitcoind]}" 2>/dev/null || break; sleep 1; done
    stop_pid bitcoind
  fi
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; else note "kept: $BASE"; fi
}
trap cleanup EXIT

# ---- build ---------------------------------------------------------------------

TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/shop-lightning-counter}
LSP_TARGET=${LSP_TARGET_DIR:-$HOME/.cache/openbmt/target/shop-lightning-lsp}
guarded() {
  local guard=$HOME/.cache/openbmt/guard.sh
  if [ -x "$guard" ]; then "$guard" env CARGO_BUILD_JOBS=2 "$@"; else env CARGO_BUILD_JOBS=2 "$@"; fi
}
if [ -z "${COUNTER_BIN:-}" ] || [ -z "${BOX_BIN:-}" ] || [ -z "${PAIR_BIN:-}" ]; then
  say "Build the counter example, the Box and the pairing example"
  guarded CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
    -p openbmt-counter-node --example counter_regtest || die "the build of the counter failed"
  guarded CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
    -p openbmt-box --bins --example box_pair || die "the build of the Box failed"
  COUNTER_BIN=${COUNTER_BIN:-$TARGET/debug/examples/counter_regtest}
  BOX_BIN=${BOX_BIN:-$TARGET/debug/openbmt-box}
  PAIR_BIN=${PAIR_BIN:-$TARGET/debug/examples/box_pair}
fi
if [ -z "${LSP_BIN:-}" ]; then
  say "Build the test provider"
  guarded CARGO_TARGET_DIR="$LSP_TARGET" cargo build --quiet --locked \
    --manifest-path "$ROOT/tools/shop-lightning-lsp/Cargo.toml" || die "the build of the provider failed"
  LSP_BIN=$LSP_TARGET/debug/shop-lightning-lsp
fi
for bin in COUNTER_BIN BOX_BIN PAIR_BIN LSP_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done

# ---- the network ---------------------------------------------------------------

say "Start Bitcoin Core (serving filters), the payer and the provider"
bitcoind -regtest -datadir="$BTC_DIR" -port="$BTC_P2P" -rpcport="$BTC_RPC" -rpcuser="$RPC_USER" \
  -rpcpassword="$RPC_PASS" -fallbackfee=0.0002 -txindex=1 -listen=1 -bind="127.0.0.1:$BTC_P2P" \
  -blockfilterindex=1 -peerblockfilters=1 -dnsseed=0 \
  -zmqpubrawblock="tcp://127.0.0.1:$ZMQ_BLK" -zmqpubrawtx="tcp://127.0.0.1:$ZMQ_TX" \
  -pid="$BASE/bitcoind.pid" >/dev/null 2>&1 &
PIDS[bitcoind]=$!
for _ in $(seq 1 60); do bcli getblockcount >/dev/null 2>&1 && break; sleep 1; done
bcli getblockcount >/dev/null 2>&1 || die "bitcoind did not start"
bcli createwallet miner >/dev/null
bw miner getnewaddress >"$BASE/miner.addr"
mine 101
note "height $(bcli getblockcount)"

lnd --lnddir="$LND_HOME" --bitcoin.regtest --bitcoin.node=bitcoind \
  --bitcoind.rpchost="127.0.0.1:$BTC_RPC" --bitcoind.rpcuser="$RPC_USER" --bitcoind.rpcpass="$RPC_PASS" \
  --bitcoind.zmqpubrawblock="tcp://127.0.0.1:$ZMQ_BLK" --bitcoind.zmqpubrawtx="tcp://127.0.0.1:$ZMQ_TX" \
  --listen="127.0.0.1:$LND_P2P" --rpclisten="127.0.0.1:$LND_RPC" --restlisten="127.0.0.1:$LND_REST" \
  --noseedbackup --no-macaroons --alias=payer >"$LND_HOME/stdout.log" 2>&1 &
PIDS[lnd]=$!
lnd_synced() { [ "$(lnc getinfo 2>/dev/null | jq -r .synced_to_chain)" = true ]; }
for _ in $(seq 1 90); do lnd_synced && break; sleep 1; done
lnd_synced || die "lnd did not start"

"$LSP_BIN" dir="$LSP_DIR" ctl="$LSP_CTL" rpc="$BTC_RPC" user="$RPC_USER" pass="$RPC_PASS" p2p="$LSP_P2P" \
  >"$LSP_DIR/out.log" 2>&1 &
PIDS[lsp]=$!
wait_port "$LSP_CTL" 120 || die "the provider did not start"
LSP_ID=$(ctl "$LSP_CTL" nodeid)

bw miner sendtoaddress "$(ctl "$LSP_CTL" addr)" 1 >/dev/null
bw miner sendtoaddress "$(lnc newaddress p2wkh | jq -r .address)" 1 >/dev/null
mine 6
for _ in $(seq 1 60); do
  [ "$(lnc walletbalance | jq -r .confirmed_balance)" -gt 0 ] 2>/dev/null &&
    ctl "$LSP_CTL" balances | grep -q 'total_onchain_balance_sats: [1-9]' && break
  sleep 1
done
for _ in $(seq 1 60); do lnd_synced && break; sleep 1; done
lnc connect "$LSP_ID@127.0.0.1:$LSP_P2P" >/dev/null
for _ in $(seq 1 30); do
  lnc openchannel --node_key="$LSP_ID" --local_amt=5000000 --private >/dev/null 2>&1 && break
  sleep 2
done
[ "$(lnc pendingchannels | jq '.pending_open_channels|length')" = 1 ] || die "LND could not open its channel"
mine 6
for _ in $(seq 1 60); do
  [ "$(lnc listchannels | jq '[.channels[]|select(.active)]|length')" = 1 ] && break
  sleep 1
done
[ "$(lnc listchannels | jq '[.channels[]|select(.active)]|length')" = 1 ] || die "the payer's channel is not active"
note "provider $LSP_ID"

# ---- the Box and the counter ---------------------------------------------------

NODE_ID=$("$COUNTER_BIN" --shop-ln-id "$SEED_OK" 2>&1 | tail -1)
BAD_ID=$("$COUNTER_BIN" --shop-ln-id "$SEED_BAD" 2>&1 | tail -1)
[ ${#NODE_ID} = 66 ] && [ ${#BAD_ID} = 66 ] && [ "$NODE_ID" != "$BAD_ID" ] || die "no node IDs: '$NODE_ID' '$BAD_ID'"
note "pinned node $NODE_ID"

"$PAIR_BIN" shop-ln "$K_DATA" "$NODE_ID" "$LSP_ID" "127.0.0.1:$LSP_P2P" "$CAP_SATS" "$MAX_FEE_SATS" >/dev/null ||
  die "no Shop Lightning block"
"$PAIR_BIN" invite "$K_DATA" >"$BASE/invitation.txt" 2>&1 || die "no invitation"
LOG_BOX="$BASE/box.log"
start_box() {
  : >"$LOG_BOX"
  "$BOX_BIN" "$B_DATA" "127.0.0.1:$BOX_PORT" --no-console --invitation-file "$BASE/invitation.txt" \
    </dev/null >>"$LOG_BOX" 2>&1 &
  PIDS[box]=$!
  local i
  for i in $(seq 1 60); do
    grep -q '^state: ' "$LOG_BOX" && return 0
    kill -0 "${PIDS[box]}" 2>/dev/null || return 1
    sleep 0.25
  done
  return 1
}
start_box || die "the Box did not start: $(cat "$LOG_BOX")"
"$PAIR_BIN" claim "$K_DATA" "127.0.0.1:$BOX_PORT" >/dev/null 2>&1 || die "the claim failed"
[ "$("$PAIR_BIN" welcome "$K_DATA" "127.0.0.1:$BOX_PORT" with-rc 2>&1)" = welcomed ] || die "the Box was not welcomed"
"$PAIR_BIN" mailbox "$K_DATA" "127.0.0.1:$BOX_PORT" owner >/dev/null 2>&1 || die "no owner mailbox"
BOX_KEY=$(sed -n 's/^key: //p' "$LOG_BOX" | head -1)
[ ${#BOX_KEY} = 64 ] || die "no Box key"
RC_KEY=$("$PAIR_BIN" key "$K_DATA" rc) || die "no counter key"
BOX_LN_DIR="$B_DATA/lightning/$RC_KEY"
say "A Box paired with the counter ${RC_KEY:0:8}, which the settings name as the Lightning counter"

# The counter: the example with its stdin on a FIFO, one command at a time.
LAST=
start_counter() { # name data-dir log extra args...
  local name=$1 data=$2 log=$3
  shift 3
  local fifo="$BASE/stdin-$name"
  rm -f "$fifo"
  mkfifo "$fifo"
  if [ "$name" = counter ]; then exec 3<>"$fifo"; else exec 4<>"$fifo"; fi
  : >"$log"
  if [ "$name" = counter ]; then
    "$COUNTER_BIN" "$data" "127.0.0.1:$BTC_P2P" --regtest-single-peer --own-wallet test "$@" <&3 >"$log" 2>&1 &
  else
    "$COUNTER_BIN" "$data" "127.0.0.1:$BTC_P2P" --regtest-single-peer --own-wallet test "$@" <&4 >"$log" 2>&1 &
  fi
  PIDS[$name]=$!
  local i
  for i in $(seq 1 240); do
    grep -q '^started ' "$log" && return 0
    kill -0 "${PIDS[$name]}" 2>/dev/null || return 1
    sleep 0.25
  done
  return 1
}
LOG_C="$BASE/counter.log"
LOG_D="$BASE/other.log"
SHOP_ARGS=(--shop-ln "$NODE_ID" "$LSP_ID" "127.0.0.1:$LSP_P2P" "$CAP_SATS" "$MAX_FEE_SATS")
COUNTER_ARGS=(--relay "127.0.0.1:$BOX_PORT" --relay-key "$BOX_KEY" --box-key "$BOX_KEY" "${SHOP_ARGS[@]}")
start_c() { start_counter counter "$C_DATA" "$LOG_C" "${COUNTER_ARGS[@]}"; }
stop_c() {
  echo quit >&3
  local i
  for i in $(seq 1 60); do kill -0 "${PIDS[counter]}" 2>/dev/null || break; sleep 0.5; done
  stop_pid counter
  exec 3>&- 2>/dev/null || true
}
xcmd() { # command [seconds] ; the counter
  local before i n
  # The commands of a process are numbered by the example from 1, so the
  # next number is one more than the `done` lines so far (this works in a
  # subshell too).
  n=$(($(grep -c '^done ' "$LOG_C") + 1))
  before=$(wc -l <"$LOG_C")
  echo "$1" >&3
  for i in $(seq 1 $((${2:-30} * 4))); do
    if grep -q "^done $n\$" "$LOG_C"; then
      LAST=$(tail -n +$((before + 1)) "$LOG_C")
      return 0
    fi
    kill -0 "${PIDS[counter]}" 2>/dev/null || break
    sleep 0.25
  done
  LAST=$(tail -n +$((before + 1)) "$LOG_C")
  return 1
}
kv() { # line key
  sed -nE "s/^(.* )?$2=([^ ]*).*/\2/p" <<<"$1" | head -1
}
# The counter has the chain's tip and a healthy check.
await_tip() {
  local tip i
  tip=$(bcli getblockcount)
  for i in $(seq 1 240); do
    xcmd status >/dev/null && grep -q "^status tip=$tip " <<<"$LAST" && return 0
    sleep 0.5
  done
  return 1
}
start_c || die "the counter did not start: $(tail -3 "$LOG_C")"
await_tip || die "the counter did not reach the tip: $LAST"
say "The counter is at height $(bcli getblockcount), linked to the Box"

# Every file under a folder, as text, for a search for a secret.
# The record file the example keeps for the app (`shop-seed.bin`) is left out:
# the app seals it, and it is the one place the seed may be.
seed_in() { # folder hex -> 0 when some file holds the hex or its bytes
  local f bytes
  bytes=$(printf '%s' "$2" | xxd -r -p | xxd -p | tr -d '\n')
  while IFS= read -r -d '' f; do
    if grep -qF -- "$2" "$f" 2>/dev/null || [ "$(xxd -p "$f" | tr -d '\n' | grep -cF -- "$bytes")" != 0 ]; then
      return 0
    fi
  done < <(find "$1" -type f -not -name shop-seed.bin -print0 2>/dev/null)
  return 1
}

# ---- (a) the seed ----------------------------------------------------------------

say "(a) The seed: another node ID is refused and nothing is stored"
xcmd shop-status
grep -q "needs_seed=true" <<<"$LAST"
check "SL-76: the Lightning counter with no seed asks for it ($(grep '^shop ' <<<"$LAST"))" $?
xcmd "shop-seed $SEED_BAD"
LINE=$(grep '^shop-seed ' <<<"$LAST")
note "$LINE"
grep -q "^shop-seed refused SL-72" <<<"$LAST" && [ ! -e "$C_DATA/shop-seed.bin" ] &&
  ! seed_in "$C_DATA" "$SEED_BAD" && ! seed_in "$BOX_LN_DIR" "$SEED_BAD" 2>/dev/null
check "SL-72: a seed of another node ID is refused with its rule, and nothing is stored on the counter or the Box" $?
xcmd "shop-seed not-a-seed"
grep -q "^shop-seed refused" <<<"$LAST" && [ ! -e "$C_DATA/shop-seed.bin" ]
check "SL-76: text that is not a seed is refused and nothing is stored" $?
xcmd "shop-seed $SEED_OK"
grep -q "^shop-seed kept" <<<"$LAST" && [ -s "$C_DATA/shop-seed.bin" ]
check "SL-72: the seed of the pinned ID is taken and its record is given to the app" $?
! seed_in "$C_DATA" "$SEED_OK" && ! seed_in "$B_DATA" "$SEED_OK"
check "SL-76: the seed is in no plain file of the counter's folder or the Box's (the app seals the record)" $?

# The app starts the node again with the record it kept.
stop_c
start_c || die "the counter did not start with its seed: $(tail -3 "$LOG_C")"
await_tip || die "the counter did not reach the tip: $LAST"

# ---- (b) a first payment ---------------------------------------------------------

open_sale() { # -> IDX, SADDR, SSATS
  xcmd "open $SALE_CENTS $SALE_RATE" || return 1
  local line
  line=$(grep '^opened ' <<<"$LAST" || true)
  IDX=$(kv "$line" index)
  SADDR=$(kv "$line" address)
  SSATS=$(kv "$line" sats)
  [ -n "$IDX" ] && [ -n "$SADDR" ] && [ -n "$SSATS" ]
}
# The state of a sale as the counter judges it.
sale_state() { # index
  xcmd "judge $1" 30 || return 1
  kv "$(grep "^sale $1 " <<<"$LAST")" state
}
# Ask for the invoice until it is offered or the seconds run out; sets INVOICE and HASH.
INVOICE=
HASH=
LNLINE=
await_offer() { # index seconds
  local i
  INVOICE=
  for i in $(seq 1 "$2"); do
    xcmd "lightning $1" 30 || return 1
    LNLINE=$(grep '^lightning ' <<<"$LAST" | head -1)
    case "$LNLINE" in
      "lightning invoice "*)
        INVOICE=$(kv "$LNLINE" bolt11)
        HASH=$(kv "$LNLINE" hash)
        return 0
        ;;
    esac
    sleep 1
  done
  return 1
}
lsp_channels_with_shop() { ctl "$LSP_CTL" channels | grep -c "cp=$NODE_ID"; }
pay() { # invoice [timeout]
  timeout 100 lncli --lnddir="$LND_HOME" --tlscertpath="$LND_HOME/tls.cert" --network=regtest \
    --rpcserver="127.0.0.1:$LND_RPC" --no-macaroons payinvoice --pay_req="$1" --force --timeout="${2:-60s}" >"$BASE/pay.out" 2>&1
}
# The names the Box keeps are hex of the node's key names; a channel monitor's
# are `monitors~...` and `monitor_updates~...`.
box_has_monitor() {
  local f name found=
  while IFS= read -r f; do
    name=$(basename "$f" | xxd -r -p 2>/dev/null | tr -d '\0')
    case "$name" in monitors~*) found=1 ;; esac
  done < <(find "$BOX_LN_DIR" -type f 2>/dev/null)
  [ -n "$found" ]
}

say "(b) A first payment opens a channel; the Box holds the monitor before the sale is paid"
open_sale || die "no sale: $LAST"
S1=$IDX
note "sale $S1 for $SSATS sats at $SADDR"
if await_offer "$S1" 240; then
  note "invoice for $(kv "$LNLINE" msat) msat, hash $HASH"
  OFFERED=0
else
  note "last line: $LNLINE; status: $(xcmd shop-status >/dev/null; grep '^shop ' <<<"$LAST")"
  OFFERED=1
fi
check "SL-40, SL-42: the Lightning counter offers an invoice for the first sale" "$OFFERED"
[ "$OFFERED" = 0 ] || die "no invoice, so nothing further can be checked"
STATE_BEFORE=$(sale_state "$S1")
[ "$STATE_BEFORE" = NotPaidYet ]
check "the sale is not paid before the payer pays ($STATE_BEFORE)" $?
pay "$INVOICE" 90s &
PAYER=$!
PAIDSEEN=1
MONITOR_AT_PAID=1
for _ in $(seq 1 240); do
  S=$(sale_state "$S1")
  if [ "$S" = PaidLightning ]; then
    # The moment the counter says paid, what the Box holds is looked at.
    box_has_monitor && MONITOR_AT_PAID=0
    PAIDSEEN=0
    break
  fi
  sleep 0.5
done
wait "$PAYER"
PAYRC=$?
check "SL-21: the first payment opens a channel through the provider and the sale is paid by Lightning (payer rc $PAYRC)" "$PAIDSEEN"
check "SL-21, LK-82: when the sale was first reported paid the Box already held the channel monitor ($(find "$BOX_LN_DIR" -type f 2>/dev/null | wc -l) files)" "$MONITOR_AT_PAID"
[ "$(lsp_channels_with_shop)" = 1 ]
check "SL-28: the provider holds one channel with the shop's node ($NODE_ID)" $?
xcmd shop-report
note "$(grep '^shop-report' <<<"$LAST")"
[ "$(kv "$(grep '^shop-report' <<<"$LAST")" stored_at)" != 0 ]
check "SL-78: the Lightning report says when the Box last stored the state" $?
# The sale is given once: the same payment is not given again after a restart.
stop_c
start_c || die "the counter did not start again: $(tail -3 "$LOG_C")"
await_tip || die "the counter did not reach the tip: $LAST"
sleep 5
[ "$(sale_state "$S1")" = PaidLightning ]
check "SL-21: after a restart the paid sale is still paid once (acknowledged, not given again)" $?

# ---- (c) the Box away ------------------------------------------------------------

say "(c) With the Box stopped the counter offers on-chain only, and Lightning returns with the Box"
stop_pid box
sleep 2
open_sale || die "no sale: $LAST"
S2=$IDX
if await_offer "$S2" 45; then GOT=1; else GOT=0; fi
note "with the Box away: $LNLINE"
[ "$GOT" = 0 ] && grep -q "^lightning unavailable" <<<"$LNLINE" && [ -n "$SADDR" ]
check "SL-41: with the Box stopped the sale has its on-chain address and no invoice ($LNLINE)" $?
# The Box comes back, from the same folder and port.
LOG_BOX="$BASE/box2.log"
: >"$LOG_BOX"
"$BOX_BIN" "$B_DATA" "127.0.0.1:$BOX_PORT" --no-console </dev/null >>"$LOG_BOX" 2>&1 &
PIDS[box]=$!
open_sale || die "no sale: $LAST"
S3=$IDX
if await_offer "$S3" 180; then BACK=0; else BACK=1; fi
note "with the Box back: $LNLINE"
check "SL-41: when the Box returns, Lightning is offered again" "$BACK"
if [ "$BACK" = 0 ]; then
  pay "$INVOICE" 60s
  PAIDAGAIN=1
  for _ in $(seq 1 120); do
    [ "$(sale_state "$S3")" = PaidLightning ] && { PAIDAGAIN=0; break; }
    sleep 0.5
  done
  check "the payment after the return is paid on the channel that is open" "$PAIDAGAIN"
fi

# ---- (d) a counter that is not named ---------------------------------------------

say "(d) A counter the settings do not name offers on-chain only"
OTHER_ARGS=(--box-key "$BOX_KEY" "${SHOP_ARGS[@]}" --shop-ln-other)
start_counter other "$D_DATA" "$LOG_D" "${OTHER_ARGS[@]}" || die "the other counter did not start: $(tail -3 "$LOG_D")"
xother() { # command
  local before i
  before=$(wc -l <"$LOG_D")
  echo "$1" >&4
  for i in $(seq 1 120); do
    if [ "$(grep -c '^done ' "$LOG_D")" -gt "${DONE_D:-0}" ]; then
      DONE_D=$(grep -c '^done ' "$LOG_D")
      LAST=$(tail -n +$((before + 1)) "$LOG_D")
      return 0
    fi
    sleep 0.25
  done
  LAST=$(tail -n +$((before + 1)) "$LOG_D")
  return 1
}
DONE_D=0
for _ in $(seq 1 240); do
  xother status && grep -q "^status tip=$(bcli getblockcount) " <<<"$LAST" && break
  sleep 0.5
done
xother "open $SALE_CENTS $SALE_RATE"
DLINE=$(grep '^opened ' <<<"$LAST")
xother "lightning $(kv "$DLINE" index)"
LN_D=$(grep '^lightning ' <<<"$LAST")
xother shop-status
SH_D=$(grep '^shop ' <<<"$LAST")
note "$DLINE"
note "$LN_D / $SH_D"
[ -n "$(kv "$DLINE" address)" ] && [ "$LN_D" = "lightning off" ] && grep -q "needs_seed=false" <<<"$SH_D" &&
  grep -q "why=none" <<<"$SH_D"
check "SL-70: the counter the settings do not name has an on-chain code, no Lightning line, no call for a seed" $?
echo quit >&4
for _ in $(seq 1 40); do kill -0 "${PIDS[other]}" 2>/dev/null || break; sleep 0.5; done
stop_pid other
exec 4>&- 2>/dev/null || true
[ ! -e "$D_DATA/shop-seed.bin" ] && [ -z "$(find "$D_DATA" -ipath '*shop-lightning*' -type f 2>/dev/null)" ]
check "SL-70: that counter made no Shop Lightning folder and no seed record" $?

# ---- (e) settings without shop_lightning -----------------------------------------

say "(e) Settings without shop_lightning close the channel and then delete the seed"
CHANNELS_BEFORE=$(lsp_channels_with_shop)
xcmd "shop-settings off 3"
note "$(grep '^settings ' <<<"$LAST")"
grep -q "^settings taken version=3" <<<"$LAST"
check "SL-77: the settings without shop_lightning are taken" $?
open_sale || die "no sale: $LAST"
S4=$IDX
sleep 3
xcmd "lightning $S4"
LN4=$(grep '^lightning ' <<<"$LAST")
note "after the change: $LN4"
case "$LN4" in "lightning invoice"*) false ;; *) true ;; esac
check "SL-77: after the change no invoice is offered" $?
# The channel closes cooperatively; the close is mined; the seed goes when
# nothing is left to do.
DELETED=1
CLOSED=1
for _ in $(seq 1 120); do
  mine 1
  xcmd shop-status 30
  if grep -q "^shop-seed deleted" <<<"$LAST"; then DELETED=0; break; fi
  sleep 2
done
[ "$(lsp_channels_with_shop)" = 0 ] && CLOSED=0
note "$(grep '^shop ' <<<"$LAST")"
check "SL-36: the channel was closed cooperatively (the provider holds $(lsp_channels_with_shop) of $CHANNELS_BEFORE channels with the shop's node)" "$CLOSED"
check "SL-77: the seed record is deleted once no channel is open and nothing is left to sweep" "$DELETED"
[ ! -e "$C_DATA/shop-seed.bin" ]
check "SL-77: the seed record is gone from the counter's folder" $?
LASTSTORE=$(xcmd shop-report >/dev/null; grep '^shop-report' <<<"$LAST")
note "$LASTSTORE"
true

say "Done"
if [ "$FAILED" = 0 ]; then
  echo "ALL PASS"
else
  echo "SOME FAILED"
  exit 1
fi
