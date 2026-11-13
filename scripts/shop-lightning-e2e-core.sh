#!/usr/bin/env bash
# Shop Lightning on regtest, against Bitcoin Core 32.1, an LSPS2 provider and a
# payer (docs/SHOP-LIGHTNING-RULES.md, SL-50).
#
# Question: does the node of `crates/openbmt-lightning-node` do what SL-10,
# SL-20 to SL-26, SL-28 to SL-31, SL-36, SL-40 and SL-41 say when a real payer
# pays it through a real provider?
#
# What it does:
#   1. Starts a private regtest bitcoind, LND (the payer), the provider (a
#      test tool on ldk-node, `tools/shop-lightning-lsp`, outside the
#      workspace, with `client_trusts_lsp` false) and the second device
#      (`shop-lightning-device`, a directory behind TCP). LND opens a private
#      channel to the provider.
#   2. Starts the shop's node (`examples/shop_regtest.rs` of
#      `openbmt-lightning-node`, made from the BIP-85 text's master key).
#   3. Checks, one `PASS` line each:
#        - the node id is the one the BIP-85 child gives (SL-10)
#        - the first payment opens a channel at the fee shown (SL-28, SL-29)
#          and the channel is unannounced (SL-31)
#        - the second payment uses the same channel
#        - an amount above the room is not offered as an invoice, and one
#          whose fee is above the owner's limit or above the provider's
#          maximum is on-chain only (SL-29, SL-40)
#        - with the second device away a payment is held, nothing panics, the
#          counter shows on-chain only, and the payment completes when the
#          device returns (SL-21, SL-23, SL-24, SL-41)
#        - a counter that was wiped is restored from the second device's newer
#          copy; an older counter copy is brought to the newer one and a start
#          from it is refused; an older second device is refused as the start
#          and brought to the counter's newer copy (SL-25)
#        - the node offline for 150 blocks and back is fine
#        - a close pays the shop's wallet (SL-36)
#        - an invoice made before a restart is paid after it, through the
#          provider's opening of a channel, with the fee that was recorded
#          (SL-20, SL-29)
#        - a node killed between the claim and the acknowledgement gives the
#          paid event again with the same payment hash, and not after the
#          acknowledgement (SL-21)
#        - a channel force-closed by the provider: the node's sweeper spends
#          the shop's output, after the shop's wait, to the address of the
#          index the caller last gave, and Core sees it confirmed at that
#          address of the pinned wallet (SL-24, SL-33)
#        - the addresses of the sweep, the splice out and the close are the
#          pinned wallet's, derived by the node from the wallet description
#          (a Core descriptor wallet is imported from the same description);
#          a description that does not read is refused (SL-33)
#        - a payment that needs a new channel, with a channel already open
#          and its room too small, is paid with the provider's skim at or
#          under the recorded maximum; one whose skim is above the recorded
#          maximum is failed back (SL-21, SL-29)
#   The script ends `ALL PASS` only if every check printed PASS.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin, LND in
# ~/.local/opt/lnd-0.21.8, jq, and cargo (through ~/.cache/openbmt/guard.sh).
#
# Environment:
#   CORE_BIN       directory with bitcoind and bitcoin-cli
#   LND_DIR        directory with lnd and lncli
#   DRILL_PORTS    first of eleven free ports on 127.0.0.1 (default 19700)
#   DRILL_DIR      scratch directory (default a new one under ~/.cache/openbmt)
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   SHOP_BIN       the `shop_regtest` example binary
#   DEVICE_BIN     the `shop-lightning-device` binary
#   LSP_BIN        the `shop-lightning-lsp` binary
#
# Everything here is regtest on 127.0.0.1 with fresh keys, except the shop's,
# which are made from the BIP-85 text's published master key. No real keys or
# funds are involved, and nothing of ~/.bitcoin or ~/.lnd is used. Every
# process is stopped by its PID, and the scratch directory removed, on every
# exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
LND_DIR=${LND_DIR:-$HOME/.local/opt/lnd-0.21.8}
export PATH="$CORE_BIN:$LND_DIR:$PATH"
ROOT=$(cd "$(dirname "$0")/.." && pwd)
P0=${DRILL_PORTS:-19700}
BTC_RPC=$P0
BTC_P2P=$((P0 + 1))
ZMQ_BLK=$((P0 + 2))
ZMQ_TX=$((P0 + 3))
LSP_P2P=$((P0 + 4))
LSP_CTL=$((P0 + 5))
SHOP_CTL=$((P0 + 6))
DEV_PORT=$((P0 + 7))
LND_P2P=$((P0 + 8))
LND_RPC=$((P0 + 9))
LND_REST=$((P0 + 10))
RPC_USER=shopln
RPC_PASS=shopln_pw
# The node id the BIP-85 child at index 0 of the text's master key gives
# (prototype of 2026-11-12, and `sl_10_...` in the crate's tests).
EXPECTED_ID=039fe56192d9a44a06a725f975e5d7ccf307e023e2eab9a6bb5114ee8f69b6a9e9

mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/shop-ln-e2e-XXXXXX")}
BTC_DIR="$BASE/bitcoind"
LND_HOME="$BASE/lnd"
LSP_DIR="$BASE/lsp"
SHOP_DIR="$BASE/shop"
DEV_DIR="$BASE/device"
mkdir -p "$BTC_DIR" "$LND_HOME" "$LSP_DIR" "$SHOP_DIR" "$DEV_DIR"
PAID_LOG="$BASE/paid.log"
: >"$PAID_LOG"

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
# One command to a control port; the reply ends when the connection closes.
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
  stop_pid shop
  stop_pid device
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

TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/shop-lightning}
LSP_TARGET=${LSP_TARGET_DIR:-$HOME/.cache/openbmt/target/shop-lightning-lsp}
guarded() {
  local guard=$HOME/.cache/openbmt/guard.sh
  if [ -x "$guard" ]; then "$guard" env CARGO_BUILD_JOBS=2 "$@"; else env CARGO_BUILD_JOBS=2 "$@"; fi
}
if [ -z "${SHOP_BIN:-}" ] || [ -z "${DEVICE_BIN:-}" ]; then
  say "Build the node's example and the second device"
  guarded CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
    -p openbmt-lightning-node --bins --example shop_regtest || die "the build of the node failed"
  SHOP_BIN=${SHOP_BIN:-$TARGET/debug/examples/shop_regtest}
  DEVICE_BIN=${DEVICE_BIN:-$TARGET/debug/shop-lightning-device}
fi
if [ -z "${LSP_BIN:-}" ]; then
  say "Build the test provider"
  guarded CARGO_TARGET_DIR="$LSP_TARGET" cargo build --quiet --locked \
    --manifest-path "$ROOT/tools/shop-lightning-lsp/Cargo.toml" || die "the build of the provider failed"
  LSP_BIN=$LSP_TARGET/debug/shop-lightning-lsp
fi
for bin in SHOP_BIN DEVICE_BIN LSP_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done

# ---- the network ---------------------------------------------------------------

say "Start Bitcoin Core, the payer, the provider and the second device"
bitcoind -regtest -datadir="$BTC_DIR" -port="$BTC_P2P" -rpcport="$BTC_RPC" -rpcuser="$RPC_USER" \
  -rpcpassword="$RPC_PASS" -fallbackfee=0.0002 -txindex=1 -listen=0 -dnsseed=0 \
  -zmqpubrawblock="tcp://127.0.0.1:$ZMQ_BLK" -zmqpubrawtx="tcp://127.0.0.1:$ZMQ_TX" \
  -pid="$BASE/bitcoind.pid" >/dev/null 2>&1 &
PIDS[bitcoind]=$!
for _ in $(seq 1 60); do bcli getblockcount >/dev/null 2>&1 && break; sleep 1; done
bcli getblockcount >/dev/null 2>&1 || die "bitcoind did not start"
bcli createwallet miner >/dev/null
bcli createwallet shop >/dev/null
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

start_device() {
  "$DEVICE_BIN" "$DEV_PORT" "$DEV_DIR" >>"$BASE/device.log" 2>&1 &
  PIDS[device]=$!
  wait_port "$DEV_PORT" 20
}
start_device || die "the second device did not start"

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

# ---- the shop's node -----------------------------------------------------------

start_shop() { # [extra arguments]; returns 0 once the control port answers
  : >"$BASE/shop.out"
  "$SHOP_BIN" ctl="$SHOP_CTL" data="$SHOP_DIR" rpc="$BTC_RPC" user="$RPC_USER" pass="$RPC_PASS" \
    provider_id="$LSP_ID" provider_addr="127.0.0.1:$LSP_P2P" device="$DEV_PORT" sweep_index="$SWEEP0" "$@" \
    >"$BASE/shop.out" 2>"$BASE/shop.err" &
  PIDS[shop]=$!
  local i
  for i in $(seq 1 90); do
    (exec 3<>"/dev/tcp/127.0.0.1/$SHOP_CTL") 2>/dev/null && return 0
    kill -0 "${PIDS[shop]}" 2>/dev/null || { wait "${PIDS[shop]}" 2>/dev/null; return 1; }
    sleep 1
  done
  return 1
}
stop_shop() {
  ctl "$SHOP_CTL" stop >/dev/null
  local i
  for i in $(seq 1 60); do kill -0 "${PIDS[shop]}" 2>/dev/null || break; sleep 0.5; done
  stop_pid shop
}
shop_status() { ctl "$SHOP_CTL" status; }
field() { # name from a status line
  sed -n "s/.*\b$1=\([^ ]*\).*/\1/p" <<<"$2"
}
shop_alive() { kill -0 "${PIDS[shop]}" 2>/dev/null; }
await_status() { # pattern seconds
  local i s
  for i in $(seq 1 "$2"); do
    s=$(shop_status)
    grep -Eq -- "$1" <<<"$s" && return 0
    sleep 1
  done
  note "last status: $s"
  return 1
}
# Waits for the sale with this payment hash to be reported paid, and
# acknowledges it (the node keeps a paid event until it is acknowledged).
await_paid_keep() { # hash seconds
  local i
  for i in $(seq 1 "$2"); do
    ctl "$SHOP_CTL" paid >>"$PAID_LOG"
    grep -q "paid $1" "$PAID_LOG" && return 0
    sleep 1
  done
  return 1
}
await_paid() { # hash seconds
  await_paid_keep "$1" "$2" || return 1
  ctl "$SHOP_CTL" ack "$1" >/dev/null
}
hash_of() { lnc decodepayreq "$1" | jq -r .payment_hash; }
pay() { # invoice [timeout]; returns lncli's status
  timeout 100 lncli --lnddir="$LND_HOME" --tlscertpath="$LND_HOME/tls.cert" --network=regtest \
    --rpcserver="127.0.0.1:$LND_RPC" --no-macaroons payinvoice --pay_req="$1" --force --timeout="${2:-60s}" >"$BASE/pay.out" 2>&1
}
pay_status() { # hash
  lnc listpayments --include_incomplete | jq -r --arg h "$1" '.payments[]|select(.payment_hash==$h)|.status' | head -1
}
shop_invoice() { # sats -> sets INVOICE and the reply
  REPLY=$(ctl "$SHOP_CTL" invoice "$1")
  INVOICE=$(sed -n 's/^invoice \([^ ]*\) .*/\1/p' <<<"$REPLY")
}
lsp_channels_with_shop() { ctl "$LSP_CTL" channels | grep -c "cp=$SHOP_ID"; }

say "The shop's pinned wallet: a description that does not read is refused"
SWEEP0=5
"$SHOP_BIN" ctl="$SHOP_CTL" data="$SHOP_DIR" rpc="$BTC_RPC" user="$RPC_USER" pass="$RPC_PASS" \
  provider_id="$LSP_ID" provider_addr="127.0.0.1:$LSP_P2P" device="$DEV_PORT" sweep_index="$SWEEP0" \
  wallet="not-a-wallet" >"$BASE/refused.out" 2>&1
RC=$?
[ "$RC" = 3 ] && grep -q "WalletRefused" "$BASE/refused.out" && [ ! -e "$SHOP_DIR/state" ]
check "SL-33: a wallet description that does not read is refused, and nothing is made on disk" $?

say "Start the shop's node"
start_shop max_fee_msat=3000000 || die "the node did not start: $(cat "$BASE/shop.out" "$BASE/shop.err" | tail -3)"
SHOP_ID=$(ctl "$SHOP_CTL" nodeid)
[ "$SHOP_ID" = "$EXPECTED_ID" ]
check "SL-10: the node id is the one the BIP-85 child of the text's master key gives" $?
await_status "provider=true" 60 || die "the node did not connect to the provider"
note "node $SHOP_ID"

# A Core descriptor wallet from the same description (the test-network spelling
# of its key), so that Core can say whether an address is the wallet's.
DESC=$(ctl "$SHOP_CTL" descriptor)
bcli -named createwallet wallet_name=pinned blank=true disable_private_keys=true descriptors=true >/dev/null || die "no pinned wallet in Core"
RES=$(bw pinned importdescriptors "[{\"desc\":\"$DESC\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,100]}]")
[ "$(jq -r 'all(.[]; .success)' <<<"$RES")" = true ] || die "Core did not take the description: $RES"
# The address of an index of the pinned wallet, as the node derives it.
addr_at() { ctl "$SHOP_CTL" address "$1"; }
is_pinned() { # address: Core says it is the pinned wallet's
  [ "$(bw pinned getaddressinfo "$1" | jq -r .ismine)" = true ]
}
A0=$(addr_at 0)
A5=$(addr_at 5)
[ "$A0" = "$(bcli deriveaddresses "$DESC" "[0,0]" | jq -r '.[0]')" ] && [ "$A5" = "$(bcli deriveaddresses "$DESC" "[5,5]" | jq -r '.[0]')" ] && is_pinned "$A5"
check "SL-33: the node derives the pinned wallet's addresses itself (index 0 and 5 agree with Core's): $A5" $?
[ "$(ctl "$SHOP_CTL" address 2147483648 | cut -c1-3)" = ERR ]
check "SL-33: an index with no address is refused" $?

# ---- the first payment opens a channel -----------------------------------------

say "First payment"
shop_invoice 50000
note "$(sed 's/^\(invoice [a-z0-9]\{20\}\)[^ ]*/\1.../' <<<"$REPLY")"
H1=$(hash_of "$INVOICE")
[ -n "$INVOICE" ] && [ "$(field fee_msat "$REPLY")" = 1000000 ]
check "SL-29: the provider's fee is named before the invoice is made: 1,000 sat for 50,000 sat" $?
pay "$INVOICE" 90s
check "SL-28: the payment through the provider is paid" $?
await_paid "$H1" 30
check "SL-21: the sale is reported paid on the claim, 49,000 sat after the fee" $?
grep -q "paid $H1 amount_msat=49000000" "$PAID_LOG"
check "SL-29: what arrived is the amount less the fee shown" $?
S=$(shop_status)
[ "$(field usable "$S")" = 1 ] && [ "$(lsp_channels_with_shop)" = 1 ]
check "SL-28: the first payment opened one channel from the provider" $?
ctl "$LSP_CTL" channels | grep "cp=$SHOP_ID" | grep -q 'announced=false'
check "SL-31: the channel is unannounced" $?
FUNDING_1=$(ctl "$LSP_CTL" channels | grep "cp=$SHOP_ID" | sed -n 's/.*funding_txo=Some(OutPoint { txid: \([0-9a-f]*\), vout: \([0-9]*\) }).*/\1:\2/p')
note "channel $FUNDING_1, room $(field room_msat "$S") msat"

# ---- the second payment uses it ------------------------------------------------

say "Second payment"
shop_invoice 10000
[ -n "$INVOICE" ] && [ "$(field fee_msat "$REPLY")" = 0 ]
check "the second invoice is on the open channel, with no opening fee" $?
H2=$(hash_of "$INVOICE")
pay "$INVOICE" 60s && await_paid "$H2" 30
check "the second payment is paid" $?
FUNDING_2=$(ctl "$LSP_CTL" channels | grep "cp=$SHOP_ID" | sed -n 's/.*funding_txo=Some(OutPoint { txid: \([0-9a-f]*\), vout: \([0-9]*\) }).*/\1:\2/p')
[ "$(lsp_channels_with_shop)" = 1 ] && [ "$FUNDING_1" = "$FUNDING_2" ]
check "the second payment used the same channel, no new one was opened" $?

# ---- an amount above the room --------------------------------------------------

say "Room to receive"
S=$(shop_status)
ROOM=$(field room_msat "$S")
ABOVE=$((ROOM / 1000 + 100))
Q=$(ctl "$SHOP_CTL" quote "$ABOVE")
note "room $ROOM msat; $ABOVE sat quotes: $Q"
! grep -q '^invoice' <<<"$Q"
check "SL-40: an amount above the room ($ABOVE sat) is not offered as an invoice on the open channel" $?
Q=$(ctl "$SHOP_CTL" quote 400000)
[ "$Q" = "onchain FeeAboveMax" ]
check "SL-29: an opening whose fee (4,000 sat) is above the owner's limit (3,000 sat) is on-chain only: $Q" $?
Q=$(ctl "$SHOP_CTL" quote 2000000)
[ "$Q" = "onchain NoRoom" ]
check "SL-40: an amount the provider cannot open a channel for is on-chain only: $Q" $?
Q=$(ctl "$SHOP_CTL" quote 5000)
[ "$Q" = "invoice" ]
check "SL-40: an amount below the room is an invoice: $Q" $?
shop_alive
check "the node is still running" $?

# ---- the second device away ----------------------------------------------------

say "Second device away"
shop_invoice 5000
H3=$(hash_of "$INVOICE")
[ -n "$INVOICE" ]
check "an invoice is made while the second device is there" $?
stop_pid device
note "second device stopped"
pay "$INVOICE" 170s &
PAY_PID=$!
sleep 12
shop_alive
check "SL-23: the node did not panic with the second device away" $?
ctl "$SHOP_CTL" paid >>"$PAID_LOG"
! grep -q "paid $H3" "$PAID_LOG"
check "SL-21: the sale is not reported paid while the second device lacks the state" $?
ST=$(pay_status "$H3")
[ "$ST" = IN_FLIGHT ]
check "SL-21: the payer's payment is held, in flight: $ST" $?
S=$(shop_status)
DV=$(field device "$S")
PD=$(field pending "$S")
[ "$DV" = Away ] && [ "$PD" -ge 1 ]
check "SL-23: the manager can read that the second device is away: device=$DV pending=$PD" $?
Q=$(ctl "$SHOP_CTL" quote 1000)
[ "$Q" = "onchain SecondDeviceAway" ]
check "SL-41: with the second device away the counter shows the on-chain code alone: $Q" $?
start_device
check "the second device is back" $?
await_paid "$H3" 120
check "SL-21: the held payment is reported paid after the second device returns" $?
wait "$PAY_PID"
ST=$(pay_status "$H3")
[ "$ST" = SUCCEEDED ]
check "the payer's payment succeeded: $ST" $?
await_status "device=Reachable pending=0" 30
check "SL-23: the second device reads reachable again with nothing waiting" $?

# ---- restore from a copy: the newer, and the older refused ---------------------

say "Restore and the older copy"
stop_shop
cp -a "$SHOP_DIR/state" "$BASE/old-local"
cp -a "$DEV_DIR" "$BASE/old-device"
note "copies of the state taken (T1)"
start_shop max_fee_msat=3000000 || die "the node did not start: $(cat "$BASE/shop.out" "$BASE/shop.err" | tail -3)"
await_status "provider=true channels=1 usable=1" 60 || die "no channel after the restart"
shop_invoice 2000
H4=$(hash_of "$INVOICE")
pay "$INVOICE" 60s && await_paid "$H4" 30
check "a payment after the restart (T2, newer than the copies)" $?
stop_shop

# The counter wiped: only the second device holds the state.
rm -rf -- "$SHOP_DIR/state"
start_shop max_fee_msat=3000000
check "SL-25: a counter that was wiped starts from the second device's copy" $?
await_status "provider=true channels=1 usable=1" 60
check "SL-25: the restored node has the channel back and usable" $?
[ "$(ctl "$SHOP_CTL" nodeid)" = "$SHOP_ID" ]
check "SL-10: the restored node is the same node" $?
shop_invoice 1500
H5=$(hash_of "$INVOICE")
pay "$INVOICE" 60s && await_paid "$H5" 30
check "SL-25: the restored node takes a payment (T3)" $?
stop_shop

# The counter's copy is the older one (T1), the second device's the newer.
rm -rf -- "$SHOP_DIR/state"
cp -a "$BASE/old-local" "$SHOP_DIR/state"
start_shop start_from=local max_fee_msat=3000000
RC=$?
[ "$RC" != 0 ] && grep -q "OlderCopy(DeviceNewer)" "$BASE/shop.out"
RC2=$?
WHY=$(head -c 80 "$BASE/shop.out")
check "SL-25: a start from the older counter copy is refused: $WHY" $RC2
stop_pid shop
start_shop max_fee_msat=3000000
check "SL-25: started the usual way, the older counter copy is brought to the newer one" $?
await_status "provider=true channels=1 usable=1" 60
check "SL-25: the channel is usable after that" $?
shop_invoice 1200
H6=$(hash_of "$INVOICE")
pay "$INVOICE" 60s && await_paid "$H6" 30
check "SL-25: a payment after bringing the older copy up (T4)" $?
stop_shop

# The second device is the older one (T1), the counter's copy the newer.
stop_pid device
rm -rf -- "$DEV_DIR"
cp -a "$BASE/old-device" "$DEV_DIR"
start_device
start_shop start_from=device max_fee_msat=3000000
RC=$?
[ "$RC" != 0 ] && grep -q "OlderCopy(LocalNewer)" "$BASE/shop.out"
RC2=$?
WHY=$(head -c 80 "$BASE/shop.out")
check "SL-25: a start from the older second device is refused: $WHY" $RC2
stop_pid shop
start_shop max_fee_msat=3000000
check "SL-25: started the usual way, the older second device is brought to the counter's copy" $?
await_status "provider=true channels=1 usable=1" 60
shop_invoice 1100
H7=$(hash_of "$INVOICE")
pay "$INVOICE" 60s && await_paid "$H7" 30
check "SL-25: a payment after bringing the older second device up (T5)" $?
stop_shop
# And a start from the device alone, after that, finds the same state.
rm -rf -- "$SHOP_DIR/state"
start_shop max_fee_msat=3000000 && await_status "provider=true channels=1 usable=1" 60
check "SL-25: the second device, brought up to date, can restore a wiped counter" $?
stop_shop

# ---- offline for 150 blocks ----------------------------------------------------

say "Offline for 150 blocks"
mine 150
start_shop max_fee_msat=3000000
check "the node starts after 150 blocks it did not see" $?
TIP=$(bcli getblockcount)
await_status "height=$TIP " 90
check "SL-26: the node reaches Core's tip, $TIP" $?
await_status "provider=true channels=1 usable=1" 60
check "the channel is usable again" $?
shop_invoice 2500
H8=$(hash_of "$INVOICE")
pay "$INVOICE" 60s && await_paid "$H8" 30
check "a payment after the 150 blocks is paid" $?

# ---- the sweep -----------------------------------------------------------------

say "Sweep to the shop's wallet"
SWEEP_ADDR=$(addr_at 10)
BEFORE=$(bw pinned getbalance)
R=$(ctl "$SHOP_CTL" sweep 10 20000)
note "sweep: $R"
[ "$R" = "Ok(())" ]
check "SL-33: the splice out of 20,000 sat to the address the node derives at index 10 was begun" $?
for _ in $(seq 1 40); do
  mine 1
  sleep 1
  [ "$(bw pinned getbalance)" != "$BEFORE" ] && break
done
mine 6
AFTER=$(bw pinned getbalance)
note "shop wallet: $BEFORE -> $AFTER"
[ "$AFTER" = "$(awk -v b="$BEFORE" 'BEGIN { printf "%.8f", b + 0.0002 }')" ]
check "SL-33: the pinned wallet received 20,000 sat by one transaction" $?
[ "$(bw pinned getreceivedbyaddress "$SWEEP_ADDR" 1)" = 0.00020000 ] && is_pinned "$SWEEP_ADDR"
check "SL-33: and it arrived at the address of index 10, which Core says is the pinned wallet's" $?
await_status "provider=true channels=1 usable=1" 60
check "SL-33: the channel is usable after the splice" $?
shop_invoice 1000
H9=$(hash_of "$INVOICE")
pay "$INVOICE" 60s && await_paid "$H9" 30
check "SL-33: a payment after the splice is paid" $?

# ---- the close -----------------------------------------------------------------

say "Close to the shop's wallet"
SHOP_ADDR=$(addr_at 11)
BEFORE=$(bw pinned getbalance)
# A close is refused while a monitor update is still on its way: try again.
for _ in $(seq 1 30); do
  R=$(ctl "$SHOP_CTL" close 11)
  [ "$R" = "Ok(1)" ] && break
  sleep 1
done
note "close: $R"
[ "$R" = "Ok(1)" ]
check "SL-36: the close of the channel was begun" $?
for _ in $(seq 1 30); do
  mine 1
  sleep 1
  [ "$(bw pinned getbalance)" != "$BEFORE" ] && break
done
mine 3
AFTER=$(bw pinned getbalance)
note "pinned wallet: $BEFORE -> $AFTER"
awk -v a="$AFTER" -v b="$BEFORE" 'BEGIN { exit !(a > b) }'
check "SL-36: the pinned wallet was paid by the close" $?
GOT=$(bw pinned getreceivedbyaddress "$SHOP_ADDR" 1)
awk -v g="$GOT" 'BEGIN { exit !(g > 0) }' && is_pinned "$SHOP_ADDR"
check "SL-36: the close paid the address of index 11 that the node derives ($GOT BTC)" $?

# ---- an invoice made before a restart ------------------------------------------

say "Invoice made before a restart, paid after it, through the provider's opening"
for _ in $(seq 1 20); do
  mine 1
  sleep 1
  await_status "channels=0" 1 && break
done
await_status "provider=true channels=0" 60
shop_invoice 20000
note "$(sed 's/^\(invoice [a-z0-9]\{20\}\)[^ ]*/\1.../' <<<"$REPLY")"
H10=$(hash_of "$INVOICE")
INVOICE_10=$INVOICE
[ -n "$INVOICE" ] && [ "$(field fee_msat "$REPLY")" -gt 0 ] 2>/dev/null
check "SL-29: with no channel the invoice is one that opens one, with the provider's fee named ($(field fee_msat "$REPLY") msat)" $?
stop_shop
start_shop max_fee_msat=3000000
check "the node starts again with that invoice unpaid" $?
await_status "provider=true channels=0" 60
pay "$INVOICE_10" 90s
check "SL-20: the invoice made before the restart is paid through the provider's opening after it" $?
await_paid "$H10" 30
check "SL-20: and reported paid: the recorded expectation survived the restart (paid $H10)" $?
await_status "channels=1 usable=1" 30
check "SL-29: the payment opened a channel" $?

# ---- the paid event after a crash ----------------------------------------------

say "Paid event after a crash"
shop_invoice 3000
H11=$(hash_of "$INVOICE")
pay "$INVOICE" 60s && await_paid_keep "$H11" 30
check "a sale is paid and reported, not yet acknowledged" $?
sleep 3
kill -9 "${PIDS[shop]}" 2>/dev/null
wait "${PIDS[shop]}" 2>/dev/null
PIDS[shop]=
note "node killed between the claim and the acknowledgement"
start_shop max_fee_msat=3000000
check "the node starts after the kill" $?
await_status "provider=true channels=1" 60
: >"$BASE/paid.again"
for _ in $(seq 1 10); do
  ctl "$SHOP_CTL" paid >>"$BASE/paid.again"
  grep -q "paid $H11" "$BASE/paid.again" && break
  sleep 1
done
grep -q "paid $H11 amount_msat=3000000" "$BASE/paid.again"
check "SL-21: the paid event comes again after the kill, with the same payment hash and amount" $?
ctl "$SHOP_CTL" ack "$H11" >/dev/null
ctl "$SHOP_CTL" ack "$H10" >/dev/null
stop_shop
start_shop max_fee_msat=3000000
: >"$BASE/paid.again"
sleep 3
ctl "$SHOP_CTL" paid >>"$BASE/paid.again"
! grep -q "paid $H11" "$BASE/paid.again"
check "SL-21: after the acknowledgement it does not come again" $?
await_status "provider=true channels=1 usable=1" 60

# ---- the sweeper ---------------------------------------------------------------

say "Force close by the provider, and the sweeper"
SWEEP1_INDEX=20
SWEEP2_INDEX=21
SWEEP1=$(addr_at $SWEEP1_INDEX)
SWEEP2=$(addr_at $SWEEP2_INDEX)
[ "$(ctl "$SHOP_CTL" sweep_index $SWEEP1_INDEX)" = "Ok(())" ]
check "SL-33: the caller sets an index for the sweeper, the node derives its address" $?
stop_shop
start_shop max_fee_msat=3000000
await_status "provider=true channels=1" 60
[ "$(ctl "$SHOP_CTL" sweep_index $SWEEP2_INDEX)" = "Ok(())" ]
check "SL-33: and changes it again after a restart" $?
R=$(ctl "$SHOP_CTL" sweep_index 2147483648)
[ "$R" = "Err(\"That address is not one of the shop's wallet, so Lightning money is not sent there.\")" ] && [ "$(ctl "$SHOP_CTL" sweep_index $SWEEP2_INDEX)" = "Ok(())" ]
check "SL-33: an index with no address is refused with the rule's words, and the last index stays: $R" $?
R=$(ctl "$LSP_CTL" forceclose "$SHOP_ID")
note "provider force-close: $R"
[ "$R" = "Ok(())" ]
check "the provider force-closed the channel" $?
mine 8
for _ in $(seq 1 30); do
  [ "$(ctl "$SHOP_CTL" tracked)" -ge 1 ] 2>/dev/null && break
  mine 1
  sleep 1
done
[ "$(ctl "$SHOP_CTL" tracked)" -ge 1 ] 2>/dev/null
check "SL-33: the shop's output of the closed channel is tracked by the sweeper: tracked=$(ctl "$SHOP_CTL" tracked)" $?
# Past the shop's to_self_delay (2016 blocks), in steps so that the node
# follows the chain.
for _ in 1 2 3 4; do mine 520; sleep 2; done
note "height $(bcli getblockcount)"
GOT=0
for _ in $(seq 1 60); do
  mine 1
  sleep 2
  GOT=$(bw pinned getreceivedbyaddress "$SWEEP2" 1)
  awk -v g="$GOT" 'BEGIN { exit !(g > 0) }' && break
done
note "received at the destination: $GOT"
awk -v g="$GOT" 'BEGIN { exit !(g > 0) }'
check "SL-33: the sweeper spent the shop's output to the address of the index the caller gave last, and Core sees it confirmed at that address of the pinned wallet ($GOT BTC)" $?
is_pinned "$SWEEP2"
check "SL-33: that address is the pinned wallet's by Core's own reading" $?
[ "$(bw pinned getreceivedbyaddress "$SWEEP1" 0)" = 0.00000000 ]
check "SL-33: nothing went to the destination that was replaced" $?
stop_shop

# ---- a payment that needs a new channel, on top of one that is open -----------

say "A new channel on top of an open one"
# A limit high enough for the larger amounts here (20,000 sat).
start_shop max_fee_msat=20000000 || die "the node did not start: $(cat "$BASE/shop.out" "$BASE/shop.err" | tail -3)"
await_status "provider=true channels=0" 60 || die "the node still holds a channel"
shop_invoice 20000
H12=$(hash_of "$INVOICE")
pay "$INVOICE" 90s && await_paid "$H12" 30
check "a payment with no channel opens the one that is open from here on" $?
stop_shop
start_shop max_fee_msat=20000000 || die "the node did not start"
await_status "provider=true channels=1 usable=1" 60 || die "no channel after the restart"
S=$(shop_status)
ROOM=$(field room_msat "$S")
AMT=$((ROOM / 1000 + 100))
Q=$(ctl "$SHOP_CTL" quote "$AMT")
note "room $ROOM msat; $AMT sat quotes: $Q"
CH_BEFORE=$(lsp_channels_with_shop)
grep -q '^newchannel' <<<"$Q"
check "SL-40: an amount above the room of the open channel needs a new one: $Q" $?
shop_invoice "$AMT"
H13=$(hash_of "$INVOICE")
FEE13=$(field fee_msat "$REPLY")
note "fee shown: $FEE13 msat"
pay "$INVOICE" 90s && await_paid "$H13" 30
check "SL-21, SL-29: with a channel already open, the payment that needs a new one is paid" $?
GOT_MSAT=$(sed -n "s/^paid $H13 amount_msat=\([0-9]*\).*/\1/p" "$PAID_LOG" | head -1)
SKIM=$((AMT * 1000 - ${GOT_MSAT:-0}))
[ -n "$GOT_MSAT" ] && [ "$SKIM" -gt 0 ] && [ "$SKIM" -le "$FEE13" ]
check "SL-29: the skim, $SKIM msat, is at or under the recorded maximum, $FEE13 msat" $?
[ "$(lsp_channels_with_shop)" = $((CH_BEFORE + 1)) ]
check "SL-28: the provider opened a new channel besides the open one" $?

# A recorded maximum below what the provider takes: the payment is failed back.
S=$(shop_status)
ROOM=$(field room_msat "$S")
AMT2=$((ROOM / 1000 + 100))
Q=$(ctl "$SHOP_CTL" quote "$AMT2")
note "room $ROOM msat; $AMT2 sat quotes: $Q"
grep -q '^newchannel' <<<"$Q" || die "the second amount does not need a new channel: $Q"
REPLY=$(ctl "$SHOP_CTL" invoice_capped "$AMT2" 1)
INVOICE=$(sed -n 's/^invoice \([^ ]*\) .*/\1/p' <<<"$REPLY")
H14=$(hash_of "$INVOICE")
[ -n "$INVOICE" ]
check "an invoice is made that records a maximum of 1 msat for the skim" $?
pay "$INVOICE" 60s
[ "$(pay_status "$H14")" = FAILED ]
check "SL-29: a payment whose skim is above the recorded maximum is failed back: $(pay_status "$H14")" $?
ctl "$SHOP_CTL" paid >>"$PAID_LOG"
! grep -q "paid $H14" "$PAID_LOG"
check "SL-21: and it is not reported paid" $?
grep -q "SkimAboveMax" "$SHOP_DIR/node.log"
check "SL-29: the node's log says the refusal was for the skim" $?
stop_shop

echo
if [ "$FAILED" = 0 ]; then echo "ALL PASS"; else echo "FAILED"; exit 1; fi
