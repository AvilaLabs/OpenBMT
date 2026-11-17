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
#        - each fresh JIT channel's first payment fails back without a
#          preimage; after that exact provider funding confirms, retrying the
#          same invoice pays at the fee shown (SL-27 to SL-31)
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
#        - with LSPS1=1 and a provider that serves LSPS1: after a sweep leaves
#          the room under half the cap, the node orders a channel, pays the
#          provider's invoice from its own channel within the owner's most,
#          the funding confirms, the room comes back to about the cap, a
#          payment then goes through a channel with a confirmed funding, and
#          the fee counts in the fees paid (SL-28, SL-29, SL-35, SL-40, SL-45)
#        - an unconfirmed JIT payment whose funding Core is told never to mine
#          is failed without releasing a preimage or recording PaymentClaimed;
#          after Core mines that exact funding, retrying the invoice succeeds
#          (SL-27)
#   The script ends `ALL PASS` only if every check printed PASS.
#
#   With SHOP_CHAIN=filters the run is another one, on the same network: the
#   node's chain data comes from the compact-filter watcher on the counter's
#   peer layer (SL-26, SL-27), not from RPC (Core is started to serve filters
#   and given no RPC login by the node). It checks:
#        - the node reaches Core's tip with no RPC
#        - an unconfirmed first JIT payment is failed without a preimage; after
#          its funding is seen confirmed through a filter-matched block, retry
#          of the same invoice succeeds and a second payment uses the channel
#        - a reorganization that takes the funding transaction out of the best
#          chain: the node unconfirms it (the library then closes the ready
#          channel, as it does for any such reorganization), the closing
#          transaction goes out through the peers, the funding transaction
#          mined again is given again, and a payment opens a new channel
#        - the node stopped for blocks and started again catches up
#        - a close pays the pinned wallet
#        - with Tor on (SL-32) and nothing listening at the proxy, the node
#          does not start and nothing reaches the provider; with something at
#          the proxy that is not a SOCKS proxy, the node starts, never reaches
#          the provider, and no connection to the provider is made at all
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin, LND in
# ~/.local/opt/lnd-0.21.8, jq, and cargo (through ~/.cache/openbmt/guard.sh).
#
# Environment:
#   CORE_BIN       directory with bitcoind and bitcoin-cli
#   LND_DIR        directory with lnd and lncli
#   SHOP_CHAIN     `filters` for the run of the filter watcher (default: Core's
#                  RPC, the run above)
#   DRILL_PORTS    first of fourteen free ports on 127.0.0.1 (default 19700)
#   DRILL_DIR      scratch directory (default a new one under ~/.cache/openbmt)
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   SHOP_BIN       the `shop_regtest` example binary
#   MONITOR_AUDIT_BIN the test-only `shop_monitor_audit` example binary
#   DEVICE_BIN     the `shop-lightning-device` binary
#   LSP_BIN        the `shop-lightning-lsp` binary
#   LSPS1          `1` turns LSPS1 on at the provider (`lsps1=on`) and runs the
#                  check of room bought ahead after the sweep (default off: the
#                  provider serves LSPS2 alone, and the node must find that out
#                  and order nothing)
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
# The Tor run of the filter watcher's run: nothing listens at the first, a
# program that is no proxy listens at the second, and a second device of its own.
TOR_FREE=$((P0 + 11))
TOR_FAKE=$((P0 + 12))
TOR_DEV=$((P0 + 13))
SHOP_CHAIN=${SHOP_CHAIN:-core}
LSPS1=${LSPS1:-0}
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
  stop_pid fakeproxy
  stop_pid tordevice
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
  say "Build the node examples and the second device"
  guarded CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
    -p openbmt-lightning-node --features tcp-device --bins --example shop_regtest --example shop_monitor_audit || die "the build of the node failed"
  SHOP_BIN=${SHOP_BIN:-$TARGET/debug/examples/shop_regtest}
  DEVICE_BIN=${DEVICE_BIN:-$TARGET/debug/shop-lightning-device}
fi
MONITOR_AUDIT_BIN=${MONITOR_AUDIT_BIN:-$TARGET/debug/examples/shop_monitor_audit}
if [ ! -x "$MONITOR_AUDIT_BIN" ]; then
  say "Build the test-only monitor audit example"
  guarded CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
    -p openbmt-lightning-node --example shop_monitor_audit || die "the monitor audit build failed"
fi
if [ -z "${LSP_BIN:-}" ]; then
  say "Build the test provider"
  guarded CARGO_TARGET_DIR="$LSP_TARGET" cargo build --quiet --locked \
    --manifest-path "$ROOT/tools/shop-lightning-lsp/Cargo.toml" || die "the build of the provider failed"
  LSP_BIN=$LSP_TARGET/debug/shop-lightning-lsp
fi
for bin in SHOP_BIN DEVICE_BIN LSP_BIN MONITOR_AUDIT_BIN; do
  [ -x "${!bin}" ] || die "no binary at ${!bin} ($bin)"
done

# ---- the network ---------------------------------------------------------------

say "Start Bitcoin Core, the payer, the provider and the second device"
# The filter watcher's run needs Core to serve filters to a peer on this machine.
if [ "$SHOP_CHAIN" = filters ]; then
  CORE_NET=(-listen=1 -bind="127.0.0.1:$BTC_P2P" -blockfilterindex=1 -peerblockfilters=1)
  CHAIN_ARGS=(chain=filters p2p="$BTC_P2P")
else
  CORE_NET=(-listen=0)
  CHAIN_ARGS=(rpc="$BTC_RPC" user="$RPC_USER" pass="$RPC_PASS")
fi
bitcoind -regtest -datadir="$BTC_DIR" -port="$BTC_P2P" -rpcport="$BTC_RPC" -rpcuser="$RPC_USER" \
  -rpcpassword="$RPC_PASS" -fallbackfee=0.0002 -txindex=1 "${CORE_NET[@]}" -dnsseed=0 \
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

LSP_ARGS=()
[ "$LSPS1" = 1 ] && LSP_ARGS+=(lsps1=on lsps1_fee_sat=1500)
"$LSP_BIN" dir="$LSP_DIR" ctl="$LSP_CTL" rpc="$BTC_RPC" user="$RPC_USER" pass="$RPC_PASS" p2p="$LSP_P2P" \
  ${LSP_ARGS[@]+"${LSP_ARGS[@]}"} >"$LSP_DIR/out.log" 2>&1 &
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
  "$SHOP_BIN" ctl="$SHOP_CTL" data="$SHOP_DIR" "${CHAIN_ARGS[@]}" \
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
  lnc listpayments --include_incomplete | jq -r --arg h "$1" '.payments[]|select(.payment_hash==$h)|.status' | tail -1
}
payment_preimage_absent() { # hash -> evidence file; true only for empty or nonmatching all-zero default
  local hash=$1 payment_json raw candidate_hash classification absent=0
  payment_json=$(lnc listpayments --include_incomplete | jq -c --arg h "$hash" '[.payments[]|select(.payment_hash==$h)]|last // {}')
  raw=$(jq -r '.payment_preimage // empty' <<<"$payment_json")
  candidate_hash=
  if [[ "$raw" =~ ^[0-9a-fA-F]{64}$ ]]; then
    candidate_hash=$(python3 - "$raw" <<'PYHASH'
import hashlib, sys
print(hashlib.sha256(bytes.fromhex(sys.argv[1])).hexdigest())
PYHASH
)
  fi
  if [ -z "$raw" ]; then
    classification=empty
    absent=1
  elif [[ "$raw" =~ ^0{64}$ ]] && [ "$candidate_hash" != "$hash" ]; then
    classification=zero_default_nonmatching
    absent=1
  elif [[ "$raw" =~ ^0{64}$ ]]; then
    classification=zero_default_hash_matches_invoice
  else
    classification=unexpected_nonzero
  fi
  printf 'payment_hash=%s\npayment_status=%s\npayment_preimage_raw=%s\npayment_preimage_class=%s\npayment_preimage_absent=%s\npayment_preimage_hash=%s\n' \
    "$hash" "$(jq -r '.status // empty' <<<"$payment_json")" "${raw:-empty}" "$classification" "$absent" "${candidate_hash:-none}" >"$BASE/unconfirmed-payer-evidence-$hash.txt"
  [ "$absent" = 1 ]
}
assert_unconfirmed_claim_refused() { # hash
  local hash=$1 rc
  if pay "$INVOICE" 90s; then rc=0; else rc=$?; fi
  [ "$rc" -ne 0 ] || return 1
  [ "$(pay_status "$hash")" = FAILED ] || return 1
  ! grep -qi 'preimage:' "$BASE/pay.out" || return 1
  payment_preimage_absent "$hash" || return 1
  ctl "$SHOP_CTL" paid >"$BASE/unconfirmed-paid-$hash.txt"
  ! grep -Fq "paid $hash " "$BASE/unconfirmed-paid-$hash.txt"
}
funding_txid_for_shop() {
  ctl "$LSP_CTL" channels | grep "cp=$SHOP_ID" | sed -n 's/.*funding_txo=Some(OutPoint { txid: \([0-9a-f]*\), vout: [0-9]* }).*/\1/p' | head -1
}
await_funding_confirmation() { # exact txid, expected shop channel count
  local txid=$1 expected_channels=$2 core_conf=0 conf some_count=0 positive_count=0
  for _ in $(seq 1 30); do bcli getmempoolentry "$txid" >/dev/null 2>&1 && break; sleep 1; done
  bcli getmempoolentry "$txid" >/dev/null 2>&1 || return 1
  # Mine only after identifying the exact funding output of the just-failed JIT attempt.
  mine 3
  for _ in $(seq 1 120); do
    core_conf=$(bcli getrawtransaction "$txid" true 2>/dev/null | jq -r '.confirmations // 0' || echo 0)
    conf=$(ctl "$SHOP_CTL" confirmations || true)
    some_count=$(grep -o 'Some(' <<<"$conf" | wc -l)
    positive_count=$(grep -oE 'Some\([1-9][0-9]*\)' <<<"$conf" | wc -l)
    if [ "$core_conf" -ge 1 ] && [ "$some_count" -eq "$expected_channels" ] \
        && [ "$positive_count" -eq "$expected_channels" ] && ! grep -q None <<<"$conf"; then
      FUNDING_CONFIRMATIONS=$conf
      return 0
    fi
    sleep 1
  done
  return 1
}
shop_invoice() { # sats -> sets INVOICE and the reply
  REPLY=$(ctl "$SHOP_CTL" invoice "$1")
  INVOICE=$(sed -n 's/^invoice \([^ ]*\) .*/\1/p' <<<"$REPLY")
}
lsp_channels_with_shop() { ctl "$LSP_CTL" channels | grep -c "cp=$SHOP_ID"; }

say "The shop's pinned wallet: a description that does not read is refused"
SWEEP0=5
"$SHOP_BIN" ctl="$SHOP_CTL" data="$SHOP_DIR" "${CHAIN_ARGS[@]}" \
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


# ---- the filter watcher's run --------------------------------------------------

if [ "$SHOP_CHAIN" = filters ]; then
  say "Filter watcher: the node's chain data comes from a peer's compact filters"
  # The node saves what it needs from the driver at most once a minute, so
  # the first reach of the tip can take that long.
  TIP=$(bcli getblockcount)
  await_status "chain_ok=true height=$TIP " 150
  check "SL-26: the node reaches Core's tip, $TIP, with no RPC login given to it" $?

  shop_invoice 50000
  H1=$(hash_of "$INVOICE")
  [ -n "$INVOICE" ] && [ "$(field fee_msat "$REPLY")" = 1000000 ]
  check "SL-29: the provider's fee is named before the invoice is made" $?
  assert_unconfirmed_claim_refused "$H1"
  check "SL-27: the first unconfirmed JIT attempt fails back with no preimage or paid record" $?
  FTX=$(funding_txid_for_shop)
  [ -n "$FTX" ]
  check "the provider channel's funding transaction is $FTX" $?
  for _ in $(seq 1 30); do bcli getmempoolentry "$FTX" >/dev/null 2>&1 && break; sleep 1; done
  bcli getmempoolentry "$FTX" >/dev/null 2>&1
  check "the funding-first provider published the channel transaction" $?
  mine 3
  CONF=
  for _ in $(seq 1 150); do
    CONF=$(ctl "$SHOP_CTL" confirmations)
    case "$CONF" in *"Some("[1-9]*) break ;; esac
    sleep 1
  done
  note "confirmations: $CONF"
  case "$CONF" in *"Some("[1-9]*) true ;; *) false ;; esac
  check "SL-26, SL-27: the funding transaction is seen confirmed through a block the filter matched" $?
  pay "$INVOICE" 90s && await_paid "$H1" 30
  check "SL-21, SL-27: the same invoice succeeds after the funding reaches the shop's confirmation depth" $?
  grep -q "paid $H1 amount_msat=49000000 .* funding=confirmed" "$PAID_LOG"
  check "the paid event records the 49,000-sat receipt on confirmed funding" $?
  FBLOCK=$(bcli getrawtransaction "$FTX" true | jq -r .blockhash)
  FHEIGHT=$(bcli getblockheader "$FBLOCK" | jq -r .height)
  note "funding in block $FBLOCK at height $FHEIGHT"

  shop_invoice 10000
  H2=$(hash_of "$INVOICE")
  pay "$INVOICE" 60s && await_paid "$H2" 30
  check "the second payment uses the channel" $?

  say "A reorganization takes the funding transaction out of the best chain"
  TIP_BEFORE=$(bcli getblockcount)
  bcli invalidateblock "$FBLOCK"
  # The other branch has no funding transaction: blocks with a coinbase only,
  # one more than the branch that left.
  for _ in $(seq 1 $((TIP_BEFORE - FHEIGHT + 2))); do
    bw miner generateblock "$(cat "$BASE/miner.addr")" '[]' >/dev/null
  done
  TIP=$(bcli getblockcount)
  [ "$(bcli getrawtransaction "$FTX" true 2>/dev/null | jq -r '.blockhash // "none"')" = none ]
  check "Core's best chain holds no funding transaction now (height $TIP)" $?
  for _ in $(seq 1 150); do grep -q "unconfirmed $FTX" "$SHOP_DIR/node.log" && break; sleep 1; done
  grep -q "unconfirmed $FTX" "$SHOP_DIR/node.log"
  check "SL-26: the node gave the funding transaction to transaction_unconfirmed" $?
  await_status "chain_ok=true height=$TIP " 150
  check "SL-26: the node follows the other branch to its tip, $TIP" $?
  grep -q "Funding transaction was un-confirmed" "$SHOP_DIR/node.log"
  check "SL-26: the library closes a channel that was ready when its funding left the chain (its own rule), so the node was told" $?
  CLOSE_TX=
  for _ in $(seq 1 30); do
    CLOSE_TX=$(sed -n 's/.*INFO  broadcast \([0-9a-f]\{64\}\) to peers.*/\1/p' "$SHOP_DIR/node.log" | head -1)
    [ -n "$CLOSE_TX" ] && break
    sleep 1
  done
  # Core asks for it over the peer connection (it may then refuse it: the
  # provider closes at the same moment and the two spend the same output).
  FETCHED=1
  for _ in $(seq 1 60); do
    grep -Eq "relayed $CLOSE_TX: asked [1-9][0-9]*, fetched [1-9]" "$SHOP_DIR/node.log" && { FETCHED=0; break; }
    sleep 1
  done
  [ -n "$CLOSE_TX" ] && [ "$FETCHED" = 0 ]
  check "SL-26: the closing transaction $CLOSE_TX was handed to the peers and Core fetched it from the node over the peer connection, with no RPC call" $?
  mine 2
  [ "$(bcli getrawtransaction "$FTX" true 2>/dev/null | jq -r '.blockhash // "none"')" != none ]
  check "the funding transaction is mined again (Core)" $?
  for _ in $(seq 1 150); do
    [ "$(grep -c "given $FTX at" "$SHOP_DIR/node.log")" -ge 2 ] && break
    sleep 1
  done
  [ "$(grep -c "given $FTX at" "$SHOP_DIR/node.log")" -ge 2 ]
  check "SL-26: the funding transaction, mined again, is given to transactions_confirmed again" $?
  mine 3
  TIP=$(bcli getblockcount)
  await_status "chain_ok=true height=$TIP .*channels=0" 150
  check "SL-26: the node follows to the tip, $TIP, with the channel closed" $?
  # A new payment opens a new channel through the provider.
  shop_invoice 50000
  H3=$(hash_of "$INVOICE")
  assert_unconfirmed_claim_refused "$H3"
  check "after reorganization, an unconfirmed new JIT channel cannot settle the first attempt" $?
  FTX_NEW=$(funding_txid_for_shop)
  [ -n "$FTX_NEW" ] && [ "$FTX_NEW" != "$FTX" ]
  check "the provider opened a replacement channel with new funding $FTX_NEW" $?
  for _ in $(seq 1 30); do bcli getmempoolentry "$FTX_NEW" >/dev/null 2>&1 && break; sleep 1; done
  bcli getmempoolentry "$FTX_NEW" >/dev/null 2>&1
  check "the replacement funding transaction is published" $?
  mine 3
  pay "$INVOICE" 90s && await_paid "$H3" 30
  check "the same invoice succeeds after replacement funding confirms" $?
  mine 3
  TIP=$(bcli getblockcount)
  await_status "chain_ok=true height=$TIP .*channels=1 usable=1" 150
  check "the new channel is usable and the node is at the tip" $?

  say "Stopped for blocks, started again"
  stop_shop
  mine 12
  start_shop max_fee_msat=3000000
  check "the node starts again" $?
  TIP=$(bcli getblockcount)
  await_status "chain_ok=true height=$TIP " 150
  check "SL-26: the node catches up to Core's tip, $TIP, from where the library had reached" $?
  await_status "provider=true channels=1 usable=1" 60
  shop_invoice 2000
  H4=$(hash_of "$INVOICE")
  pay "$INVOICE" 60s && await_paid "$H4" 30
  check "a payment after the catch-up is paid" $?

  say "Close to the shop's wallet, on the filter watcher"
  SHOP_ADDR=$(addr_at 11)
  BEFORE=$(bw pinned getbalance)
  for _ in $(seq 1 30); do
    R=$(ctl "$SHOP_CTL" close 11)
    [ "$R" = "Ok(1)" ] && break
    sleep 1
  done
  [ "$R" = "Ok(1)" ]
  check "SL-36: the close of the channel was begun" $?
  for _ in $(seq 1 30); do
    mine 1
    sleep 1
    [ "$(bw pinned getbalance)" != "$BEFORE" ] && break
  done
  mine 3
  AFTER=$(bw pinned getbalance)
  GOT=$(bw pinned getreceivedbyaddress "$SHOP_ADDR" 1)
  awk -v g="$GOT" 'BEGIN { exit !(g > 0) }' && is_pinned "$SHOP_ADDR"
  check "SL-36: the close paid the pinned wallet's address of index 11 ($BEFORE -> $AFTER)" $?
  TIP=$(bcli getblockcount)
  await_status "chain_ok=true height=$TIP .*channels=0" 150
  check "SL-26: the node saw the close confirmed (the channel is gone from the library)" $?
  grep -q "to peers" "$SHOP_DIR/node.log"
  if [ $? = 0 ]; then note "the node sent a transaction through the peers"; else note "no transaction of the node's went through the peers in this run (the provider sent the close)"; fi
  stop_shop

  # ---- Tor ----------------------------------------------------------------------

  say "Tor on (SL-32): nothing listens at the proxy"
  TOR_SHOP=$BASE/shop-tor
  TOR_DEVDIR=$BASE/device-tor
  mkdir -p "$TOR_DEVDIR"
  "$DEVICE_BIN" "$TOR_DEV" "$TOR_DEVDIR" >>"$BASE/device-tor.log" 2>&1 &
  PIDS[tordevice]=$!
  wait_port "$TOR_DEV" 20 || die "the second device of the Tor run did not start"
  tor_conns() { # connections of the node's process to the provider's port
    ss -tnp 2>/dev/null | grep "pid=${PIDS[shop]}," | grep -c ":$LSP_P2P " 
  }
  "$SHOP_BIN" ctl="$SHOP_CTL" data="$TOR_SHOP" "${CHAIN_ARGS[@]}" \
    provider_id="$LSP_ID" provider_addr="127.0.0.1:$LSP_P2P" device="$TOR_DEV" sweep_index=0 \
    tor="127.0.0.1:$TOR_FREE" >"$BASE/tor-refused.out" 2>&1
  RC=$?
  [ "$RC" = 3 ] && grep -q "TorUnreachable" "$BASE/tor-refused.out" && [ ! -e "$TOR_SHOP" ]
  check "SL-32: with Tor on and no proxy, the node does not start, and nothing is made on disk" $?

  say "Tor on (SL-32): something listens at the proxy that is not a proxy"
  "$DEVICE_BIN" "$TOR_FAKE" "$BASE/fakeproxy-dir" >>"$BASE/fakeproxy.log" 2>&1 &
  PIDS[fakeproxy]=$!
  wait_port "$TOR_FAKE" 20 || die "the stand-in for the proxy did not start"
  SHOP_DIR_MAIN=$SHOP_DIR
  DEV_PORT_MAIN=$DEV_PORT
  SHOP_DIR=$TOR_SHOP
  DEV_PORT=$TOR_DEV
  : >"$BASE/shop.out"
  "$SHOP_BIN" ctl="$SHOP_CTL" data="$SHOP_DIR" "${CHAIN_ARGS[@]}" \
    provider_id="$LSP_ID" provider_addr="127.0.0.1:$LSP_P2P" device="$DEV_PORT" sweep_index=0 \
    tor="127.0.0.1:$TOR_FAKE" >"$BASE/shop.out" 2>"$BASE/shop.err" &
  PIDS[shop]=$!
  wait_port "$SHOP_CTL" 90
  check "with Tor on and something at the proxy's address, the node starts" $?
  sleep 25
  S=$(shop_status)
  [ "$(field provider "$S")" = false ]
  check "SL-32: the provider is not reached while the proxy does not carry the connection: $(field provider "$S")" $?
  [ "$(tor_conns)" = 0 ]
  check "SL-32: and the node made no connection to the provider's port itself, not even once" $?
  grep -q "tor: the provider not reached" "$SHOP_DIR/node.log"
  check "SL-32: the node's log says the proxy failed, and the node went on waiting" $?
  stop_shop
  SHOP_DIR=$SHOP_DIR_MAIN
  DEV_PORT=$DEV_PORT_MAIN

  echo
  if [ "$FAILED" = 0 ]; then echo "ALL PASS"; else echo "FAILED"; exit 1; fi
  exit 0
fi

# ---- the first payment opens a channel -----------------------------------------

say "First payment"
shop_invoice 50000
note "$(sed 's/^\(invoice [a-z0-9]\{20\}\)[^ ]*/\1.../' <<<"$REPLY")"
H1=$(hash_of "$INVOICE")
[ -n "$INVOICE" ] && [ "$(field fee_msat "$REPLY")" = 1000000 ]
check "SL-29: the provider's fee is named before the invoice is made: 1,000 sat for 50,000 sat" $?
assert_unconfirmed_claim_refused "$H1"
check "SL-27: the first unconfirmed JIT attempt fails back without a preimage or paid record" $?
FUNDING_1=$(ctl "$LSP_CTL" channels | grep "cp=$SHOP_ID" | sed -n 's/.*funding_txo=Some(OutPoint { txid: \([0-9a-f]*\), vout: \([0-9]*\) }).*/\1:\2/p')
FUNDING_TXID=${FUNDING_1%%:*}
[ -n "$FUNDING_TXID" ] && [ "$(lsp_channels_with_shop)" = 1 ]
check "SL-28: the first attempt opens one provider channel, with funding $FUNDING_1" $?
for _ in $(seq 1 30); do bcli getmempoolentry "$FUNDING_TXID" >/dev/null 2>&1 && break; sleep 1; done
bcli getmempoolentry "$FUNDING_TXID" >/dev/null 2>&1
check "the funding-first provider published the channel transaction" $?
mine 3
CONFIRMED=0
for _ in $(seq 1 120); do
  CONF=$(ctl "$SHOP_CTL" confirmations)
  case "$CONF" in *"Some("[1-9]*) CONFIRMED=1; break ;; esac
  sleep 1
done
note "funding confirmations before retry: $CONF"
[ "$CONFIRMED" = 1 ]
check "SL-27: the node sees the funding at the shop's configured confirmation depth" $?
pay "$INVOICE" 90s && await_paid "$H1" 30
check "retry of the same invoice succeeds after funding confirmation" $?
grep -q "paid $H1 amount_msat=49000000 .* funding=confirmed" "$PAID_LOG"
check "SL-21, SL-29: one claim is recorded for 49,000 sat after the 1,000-sat fee" $?
S=$(shop_status)
[ "$(field usable "$S")" = 1 ] && [ "$(lsp_channels_with_shop)" = 1 ]
check "SL-28: the confirmed channel is usable" $?
ctl "$LSP_CTL" channels | grep "cp=$SHOP_ID" | grep -q 'announced=false'
check "SL-31: the channel is unannounced" $?
note "channel $FUNDING_1, room $(field room_msat "$S") msat"

# SL-27: the successful retry follows the funding confirmation gate.
PAID_FUNDING=$(sed -n "s/^paid $H1 .* funding=//p" "$PAID_LOG" | head -1)
[ "$PAID_FUNDING" = confirmed ]
check "SL-27: the first paid event only follows confirmed funding" $?

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
"$MONITOR_AUDIT_BIN" "$SHOP_DIR/state" "$DEV_DIR" 0
check "SL-24: exact plaintext channel-monitor and monitor-update state matches on the quiesced local/device copies" $?
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
# A close is refused while a monitor update is still on its way, and while
# the splice is not locked: blocks are mined until it is, then it is tried
# again.
for _ in $(seq 1 60); do
  R=$(ctl "$SHOP_CTL" close 11)
  [ "$R" = "Ok(1)" ] && break
  case "$R" in *"not locked"*) mine 1 ;; esac
  sleep 1
done
note "close: $R"
[ "$R" = "Ok(1)" ]
check "SL-36: the close of the channel was begun" $?
# The close is negotiated with the provider before it is sent: on a slow
# machine that takes more than half a minute.
for _ in $(seq 1 120); do
  mine 1
  sleep 1
  [ "$(bw pinned getbalance)" != "$BEFORE" ] && break
done
mine 3
AFTER=$(bw pinned getbalance)
note "pinned wallet: $BEFORE -> $AFTER"
awk -v a="$AFTER" -v b="$BEFORE" 'BEGIN { exit !(a > b) }'
CLOSE_OK=$?
check "SL-36: the pinned wallet was paid by the close" $CLOSE_OK
if [ "$CLOSE_OK" != 0 ]; then
  # Enough of what each side saw to tell where the close stopped.
  note "shop node, close lines:"
  grep -Ei "close|shutdown|broadcast|sweep|spend" "$SHOP_DIR/node.log" | tail -40 | sed 's/^/        /'
  note "provider, last lines:"
  tail -40 "$LSP_DIR/out.log" | sed 's/^/        /'
  note "mempool: $(bcli getrawmempool | jq -c .)"
fi
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
assert_unconfirmed_claim_refused "$H10"
check "SL-27: after restart the invoice's first unconfirmed JIT attempt fails without a preimage or paid record" $?
FUNDING_10=$(funding_txid_for_shop)
[ -n "$FUNDING_10" ]
check "the provider opened the fresh channel with funding $FUNDING_10" $?
await_funding_confirmation "$FUNDING_10" 1
check "SL-27: Core and the shop confirm that exact funding before retry" $?
note "funding confirmations before retry: $FUNDING_CONFIRMATIONS"
pay "$INVOICE_10" 90s
check "SL-20: the same invoice made before restart pays after the provider funding confirms" $?
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
assert_unconfirmed_claim_refused "$H12"
check "SL-27: a fresh JIT channel cannot settle its first unconfirmed attempt" $?
FUNDING_12=$(funding_txid_for_shop)
[ -n "$FUNDING_12" ]
check "the provider opened the new channel with funding $FUNDING_12" $?
await_funding_confirmation "$FUNDING_12" 1
check "the exact new channel funding reaches the shop's confirmation depth" $?
pay "$INVOICE" 90s && await_paid "$H12" 30
check "retry of the same invoice succeeds on the confirmed new channel" $?
stop_shop
start_shop max_fee_msat=20000000 || die "the node did not start"
await_status "provider=true channels=1 usable=1" 60 || die "no channel after the restart"
S=$(shop_status)
ROOM=$(field room_msat "$S")
MAX_FEE_13=20000000
# Even after the largest opening skim this invoice cannot fit in the already
# confirmed channel. It therefore needs the additional JIT channel whose
# funding must pass the claim-time confirmation gate.
AMT=$(((ROOM + MAX_FEE_13) / 1000 + 1))
Q=$(ctl "$SHOP_CTL" quote "$AMT")
note "room $ROOM msat; $AMT sat quotes: $Q (net amount exceeds old room after up to $MAX_FEE_13 msat skim)"
CH_BEFORE=$(lsp_channels_with_shop)
FUNDING_BEFORE_13=$(funding_txid_for_shop)
grep -q '^newchannel' <<<"$Q"
check "SL-40: an amount above the room of the open channel needs a new one: $Q" $?
shop_invoice "$AMT"
H13=$(hash_of "$INVOICE")
FEE13=$(field fee_msat "$REPLY")
note "fee shown: $FEE13 msat"
assert_unconfirmed_claim_refused "$H13"
check "SL-27: the payment needing a fresh channel fails before its funding confirms" $?
FUNDING_13=$(ctl "$LSP_CTL" channels | grep "cp=$SHOP_ID" | sed -n 's/.*funding_txo=Some(OutPoint { txid: \([0-9a-f]*\), vout: [0-9]* }).*/\1/p' | grep -Fvx "$FUNDING_BEFORE_13" | head -1)
[ -n "$FUNDING_13" ] && [ "$(lsp_channels_with_shop)" = $((CH_BEFORE + 1)) ]
check "the provider opened an additional channel with funding $FUNDING_13" $?
await_funding_confirmation "$FUNDING_13" "$((CH_BEFORE + 1))"
check "the exact additional channel funding reaches the shop's confirmation depth" $?
pay "$INVOICE" 90s && await_paid "$H13" 30
check "SL-21, SL-29: retry uses the confirmed additional channel and is paid" $?
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

# ---- room bought ahead (LSPS1) -------------------------------------------------

if [ "$LSPS1" = 1 ]; then
  say "Room bought ahead from a provider that offers LSPS1"
  # The node had no cap until now, so nothing was ordered before this point.
  start_shop max_fee_msat=20000000 || die "the node did not start: $(cat "$BASE/shop.out" "$BASE/shop.err" | tail -3)"
  await_status "provider=true channels=2 usable=2" 90 || die "the node does not hold its two channels"
  CH0=$(lsp_channels_with_shop)
  # A sweep, as the cap asks for (SL-33); it does not give the room back (SL-35).
  BEFORE=$(bw pinned getbalance)
  R=$(ctl "$SHOP_CTL" sweep 12 5000)
  note "sweep: $R"
  [ "$R" = "Ok(())" ]
  check "SL-33: a sweep of 5,000 sat was begun" $?
  for _ in $(seq 1 40); do
    mine 1
    sleep 1
    [ "$(bw pinned getbalance)" != "$BEFORE" ] && break
  done
  mine 6
  await_status "provider=true channels=2 usable=2" 90 || die "the channels are not usable after the splice"
  S=$(shop_status)
  S0_ROOM_SAT=$(( $(field room_msat "$S") / 1000 ))
  note "room after the sweep: $S0_ROOM_SAT sat"
  # With the room above half the cap nothing is ordered, and the node finds
  # out that the provider offers LSPS1.
  stop_shop
  start_shop max_fee_msat=20000000 cap_sat=$((S0_ROOM_SAT * 2 - 1000)) || die "the node did not start"
  await_status "provider=true channels=2 usable=2" 90 || die "no channels after the restart"
  sleep 15
  L=$(ctl "$SHOP_CTL" lsps1)
  note "room above half the cap: $L"
  [ "$(field offer "$L")" = Offered ] && [ "$(field order "$L")" = none ]
  check "SL-28: the provider answered its LSPS1 options, and with the room above half the cap nothing is ordered" $?
  # The room is under half the cap now.
  CAP_SAT=$((S0_ROOM_SAT * 2 + 100000))
  stop_shop
  start_shop max_fee_msat=20000000 cap_sat="$CAP_SAT" || die "the node did not start"
  ORDERED=1
  for _ in $(seq 1 90); do
    L=$(ctl "$SHOP_CTL" lsps1)
    case "$(field order "$L")" in Paid*|Done*) ORDERED=0; break ;; esac
    sleep 1
  done
  note "after the order: $L"
  [ "$ORDERED" = 0 ] && [ "$(field fees_paid_sat "$L")" = 1500 ]
  check "SL-28, SL-29: with the room under half the cap ($S0_ROOM_SAT sat of $CAP_SAT) the node ordered a channel and paid the provider's 1,500 sat from its own channel" $?
  # The provider opens it once the invoice is paid.
  for _ in $(seq 1 60); do
    [ "$(lsp_channels_with_shop)" = $((CH0 + 1)) ] && break
    sleep 1
  done
  [ "$(lsp_channels_with_shop)" = $((CH0 + 1))  ]
  check "SL-28: the provider opened the ordered channel" $?
  ! ctl "$LSP_CTL" channels | grep "cp=$SHOP_ID" | grep -q 'announced=true'
  check "SL-31: no channel is announced, the ordered one included" $?
  # The ordered channel is used after one confirmation (SL-28).
  mine 6
  ROOM_BACK=1
  for _ in $(seq 1 120); do
    S=$(shop_status)
    ROOM_SAT=$(( $(field room_msat "$S") / 1000 ))
    [ "$ROOM_SAT" -ge $((CAP_SAT * 9 / 10)) ] && [ "$ROOM_SAT" -le $((CAP_SAT * 105 / 100)) ] && { ROOM_BACK=0; break; }
    sleep 1
  done
  note "room $ROOM_SAT sat for a cap of $CAP_SAT sat: $S"
  [ "$ROOM_BACK" = 0 ]
  check "SL-35: the room to receive is back to about the cap ($ROOM_SAT sat of $CAP_SAT sat)" $?
  CONF=$(ctl "$SHOP_CTL" confirmations)
  ! grep -q None <<<"$CONF"
  check "SL-26: every channel's funding has a confirmation: $CONF" $?
  # An amount the room before the order could not take.
  AMT=$((S0_ROOM_SAT + 50000))
  Q=$(ctl "$SHOP_CTL" quote "$AMT")
  [ "$Q" = "invoice" ]
  check "SL-40: an amount above the room before the order is an invoice now: $Q" $?
  shop_invoice "$AMT"
  H20=$(hash_of "$INVOICE")
  [ "$(field fee_msat "$REPLY")" = 0 ]
  check "that payment needs no opening, so it has no opening fee" $?
  pay "$INVOICE" 60s && await_paid "$H20" 30
  check "SL-28, SL-40: a payment above the room before the order is paid through channels with a confirmed funding" $?
  [ "$(lsp_channels_with_shop)" = $((CH0 + 1))  ]
  check "no further channel was opened for it" $?
  for _ in $(seq 1 60); do
    L=$(ctl "$SHOP_CTL" lsps1)
    case "$(field order "$L")" in Done*) break ;; esac
    sleep 1
  done
  [ "$(field fees_paid_sat "$L")" = 1500 ] && case "$(field order "$L")" in Done*) true ;; *) false ;; esac
  check "SL-45: the order is done, and its fee counts once in the fees paid: $L" $?
  # The restart keeps the order: nothing is ordered a second time.
  stop_shop
  start_shop max_fee_msat=20000000 cap_sat="$((CAP_SAT * 4))" || die "the node did not start"
  await_status "provider=true" 60 || die "the node did not connect"
  sleep 15
  L=$(ctl "$SHOP_CTL" lsps1)
  [ "$(field fees_paid_sat "$L")" = 1500 ] && [ "$(lsp_channels_with_shop)" = $((CH0 + 1)) ]
  check "SL-28: after a restart the order is still there, even with the room under half a larger cap within the hour, and no second one was made: $L" $?
  stop_shop
else
  # No LSPS1 at the provider: the node finds that out and orders nothing.
  say "A provider without LSPS1"
  start_shop max_fee_msat=20000000 cap_sat=100000000 || die "the node did not start: $(cat "$BASE/shop.out" "$BASE/shop.err" | tail -3)"
  await_status "provider=true" 60 || die "the node did not connect to the provider"
  CH0=$(lsp_channels_with_shop)
  sleep 45
  L=$(ctl "$SHOP_CTL" lsps1)
  [ "$(field offer "$L")" = NotOffered ] && [ "$(field order "$L")" = none ] && [ "$(lsp_channels_with_shop)" = "$CH0" ]
  check "SL-28: a provider without LSPS1 is found out, nothing is ordered, and room comes only with a payment too big for it: $L" $?
  stop_shop
fi

# ---- a funding that remains unconfirmed, then confirms -------------------------

say "An unconfirmed payment fails; a retry follows its funding confirmation"
# The first HTLC reaches an unconfirmed provider channel, but the claim gate
# fails it back. Core then keeps that exact funding transaction out of blocks
# for 144 blocks. It is mined only after the test removes the negative fee
# delta, after which the same invoice is retried.
FUNDING_LOG="$BASE/funding.log"
: >"$FUNDING_LOG"
start_shop max_fee_msat=20000000 || die "the node did not start: $(cat "$BASE/shop.out" "$BASE/shop.err" | tail -3)"
await_status "provider=true" 60 || die "the node did not connect to the provider"
ctl "$LSP_CTL" forceclose "$SHOP_ID" >/dev/null
for _ in $(seq 1 60); do
  mine 1
  sleep 1
  [ "$(lsp_channels_with_shop)" = 0 ] && await_status "channels=0" 1 && break
done
await_status "provider=true channels=0" 60 || die "the shop still holds a channel"
note "the shop holds no channel"
shop_invoice 50000
HL=$(hash_of "$INVOICE")
[ -n "$INVOICE" ] && grep -q "fee_msat=[1-9]" <<<"$REPLY"
check "with no channel the invoice is one that opens one" $?
assert_unconfirmed_claim_refused "$HL"
check "SL-27: the first unconfirmed attempt fails without a payer preimage or paid record" $?
FUNDING_L=$(funding_txid_for_shop)
[ -n "$FUNDING_L" ] && [ "$(lsp_channels_with_shop)" = 1 ]
check "the attempt opened one provider channel, funded by $FUNDING_L" $?
for _ in $(seq 1 30); do bcli getmempoolentry "$FUNDING_L" >/dev/null 2>&1 && break; sleep 1; done
bcli getmempoolentry "$FUNDING_L" >/dev/null 2>&1
check "the funding transaction is in Core's mempool, in no block" $?
# Core is told not to mine the funding: with a fee delta that large its
# modified fee is under the block minimum, so no block template takes it.
bcli prioritisetransaction "$FUNDING_L" 0 -100000000 >/dev/null
check "Core was told not to mine the funding transaction (test-only: a fee delta of -1 BTC)" $?
mine 144
TIP=$(bcli getblockcount)
[ "$(bcli getrawtransaction "$FUNDING_L" true 2>/dev/null | jq -r '.blockhash // "none"')" = none ] &&
  bcli getmempoolentry "$FUNDING_L" >/dev/null 2>&1
check "after 144 blocks mined in one call (height $TIP) the funding transaction is still in no block" $?
await_status "chain_ok=true height=$TIP " 150
check "SL-26: the node follows to the tip, $TIP" $?
ctl "$SHOP_CTL" funding >"$FUNDING_LOG"
! grep -q "funding $FUNDING_L " "$FUNDING_LOG"
check "SL-27: no claim means there is no paid-funding outcome to report after 144 blocks" $?
! grep -q "paid $HL " "$PAID_LOG"
check "SL-21: the payer's refused first attempt still has no paid ledger record" $?
# Put things back as they were: confirm the exact funding, then retry the same
# invoice whose expectation was retained because no PaymentClaimed event arose.
bcli prioritisetransaction "$FUNDING_L" 0 100000000 >/dev/null
await_funding_confirmation "$FUNDING_L" 1
check "Core and the shop observe the funding confirmed before the retry" $?
pay "$INVOICE" 90s && await_paid_keep "$HL" 30
check "the same invoice succeeds once its funding is confirmed" $?
grep -q "paid $HL .* funding=confirmed" "$PAID_LOG"
check "SL-21, SL-27: the one successful paid record names confirmed funding" $?
stop_shop

echo
if [ "$FAILED" = 0 ]; then echo "ALL PASS"; else echo "FAILED"; exit 1; fi
