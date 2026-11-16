#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR=$(cd "$(dirname "$0")" && pwd)
: "${SHOP_BIN:?set path to shop_regtest}"
: "${DEVICE_BIN:?set path to shop-lightning-device}"
: "${LSP_BIN:?set path to the regtest-only shop-lightning-lsp tool}"
[ "${CONFIRM_REGTEST_PROVIDER:-}" = 1 ] || { echo 'INCONCLUSIVE: set CONFIRM_REGTEST_PROVIDER=1 only for the reviewed regtest-only provider binary' >&2; exit 2; }
PROBE_MODE=${PROBE_MODE:-withhold}
case "$PROBE_MODE" in honest|withhold) ;; *) echo 'INCONCLUSIVE: PROBE_MODE must be honest or withhold' >&2; exit 2 ;; esac
CORE_BIN=${CORE_BIN:-}
LND_DIR=${LND_DIR:-$HOME/.local/opt/lnd}
if [ -n "$CORE_BIN" ]; then PATH="$CORE_BIN:$PATH"; fi
PATH="$LND_DIR:$PATH"
BASE=${PROBE_RUN_DIR:?set PROBE_RUN_DIR to a new, empty caller-owned directory}
if [ -e "$BASE" ]; then
  [ -d "$BASE" ] && [ -z "$(find "$BASE" -mindepth 1 -maxdepth 1 -print -quit)" ] || { echo "INCONCLUSIVE: $BASE must be an empty directory; choose a new run directory" >&2; exit 2; }
else
  mkdir -p "$BASE"
fi
BTC="$BASE/bitcoind"; LND="$BASE/lnd"; LSP="$BASE/lsp"; SHOP="$BASE/shop"; DEV="$BASE/device"
mkdir -p "$BTC" "$LND" "$LSP" "$SHOP" "$DEV"
RPC=19700; GATE_RPC=19714; BTC_P2P=19701; ZMQ_BLK=19702; ZMQ_TX=19703
LSP_P2P=19704; LSP_CTL=19705; SHOP_CTL=19706; DEV_PORT=19707; LND_P2P=19708; LND_RPC=19709; LND_REST=19710
probe_rpc_user=shopln; probe_rpc_pass=shopln_pw
GATE="$BASE/suppress-next.flag"; MARKER="$BASE/suppressed-txid.txt"
PIDS=()
CORE_REGTEST_VERIFIED=0
LND_STARTED=0
LSP_STARTED=0
SHOP_STARTED=0
python3 - <<'PYPORTS'
import socket
sockets = []
try:
    for port in range(19700, 19715):
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            sock.bind(("127.0.0.1", port))
        except OSError as exc:
            raise SystemExit(f"INCONCLUSIVE: loopback port {port} is occupied: {exc}")
        sockets.append(sock)
finally:
    for sock in sockets:
        sock.close()
PYPORTS
ctl(){ local port=$1; shift; local command="$*"; timeout "${CTL_TIMEOUT:-60}" bash -c 'exec 3<>/dev/tcp/127.0.0.1/$0; echo "$1" >&3; cat <&3' "$port" "$command" 2>/dev/null; }
bcli(){ bitcoin-cli -regtest -datadir="$BTC" -rpcport="$RPC" -rpcuser="$probe_rpc_user" -rpcpassword="$probe_rpc_pass" "$@"; }
bw(){ local name=$1; shift; bcli -rpcwallet="$name" "$@"; }
lnc(){ lncli --lnddir="$LND" --tlscertpath="$LND/tls.cert" --network=regtest --rpcserver="127.0.0.1:$LND_RPC" --no-macaroons "$@"; }
cleanup(){
  set +e
  CTL_TIMEOUT=2
  if [ "$SHOP_STARTED" = 1 ]; then ctl "$SHOP_CTL" stop >/dev/null 2>&1; fi
  if [ "$LSP_STARTED" = 1 ]; then ctl "$LSP_CTL" stop >/dev/null 2>&1; fi
  if [ "$LND_STARTED" = 1 ]; then lnc stop >/dev/null 2>&1; fi
  if [ "$CORE_REGTEST_VERIFIED" = 1 ]; then bcli stop >/dev/null 2>&1; fi
  for pid in "${PIDS[@]}"; do
    for _ in $(seq 1 40); do jobs -pr | grep -Fxq "$pid" || break; sleep 0.25; done
    if jobs -pr | grep -Fxq "$pid"; then
      kill "$pid" 2>/dev/null || true
      for _ in $(seq 1 20); do jobs -pr | grep -Fxq "$pid" || break; sleep 0.1; done
      if jobs -pr | grep -Fxq "$pid"; then kill -9 "$pid" 2>/dev/null || true; fi
    fi
    wait "$pid" 2>/dev/null || true
  done
  echo "Evidence retained in $BASE"
}
trap cleanup EXIT
for bin in "$(command -v bitcoind || true)" "$(command -v bitcoin-cli || true)" "$(command -v lnd || true)" "$(command -v lncli || true)" "$SHOP_BIN" "$DEVICE_BIN" "$LSP_BIN"; do
  [ -x "$bin" ] || { echo "INCONCLUSIVE: missing executable $bin" >&2; exit 2; }
done
bitcoind -datadir="$BTC" -nosettings -version | head -1 | grep -q 'version v32.1' || { echo 'INCONCLUSIVE: expected Core 32.1' >&2; exit 2; }
# Private regtest. Core, LND payer, and merchant talk to Core directly. Only provider RPC is proxied.
bitcoind -nosettings -regtest -datadir="$BTC" -port="$BTC_P2P" -rpcport="$RPC" -rpcuser="$probe_rpc_user" -rpcpassword="$probe_rpc_pass" \
  -fallbackfee=0.0002 -txindex=1 -listen=0 -dnsseed=0 -zmqpubrawblock="tcp://127.0.0.1:$ZMQ_BLK" -zmqpubrawtx="tcp://127.0.0.1:$ZMQ_TX" \
  -pid="$BASE/bitcoind.pid" >"$BASE/bitcoind.log" 2>&1 & PIDS+=("$!")
for _ in $(seq 1 60); do bcli getblockcount >/dev/null 2>&1 && break; sleep 1; done
bcli getblockcount >/dev/null || { echo 'INCONCLUSIVE: Core did not start' >&2; exit 2; }
CORE_CHAIN=$(bcli getblockchaininfo | jq -r .chain)
[ "$CORE_CHAIN" = regtest ] || { echo "INCONCLUSIVE: expected Core chain regtest, got $CORE_CHAIN" >&2; exit 2; }
CORE_REGTEST_VERIFIED=1
bcli createwallet miner >/dev/null
bcli createwallet shop >/dev/null
MINER_ADDR=$(bw miner getnewaddress)
bw miner generatetoaddress 101 "$MINER_ADDR" >/dev/null
CORE_RPC_UPSTREAM="$RPC" CORE_RPC_GATE_PORT="$GATE_RPC" CORE_RPC_USER="$probe_rpc_user" CORE_RPC_PASS="$probe_rpc_pass" \
  CORE_RPC_GATE_FILE="$GATE" CORE_RPC_MARKER_FILE="$MARKER" \
  python3 "$SCRIPT_DIR/core_rpc_gate.py" >"$BASE/proxy.log" 2>&1 & PIDS+=("$!")
for _ in $(seq 1 30); do (exec 3<>/dev/tcp/127.0.0.1/"$GATE_RPC") 2>/dev/null && break; sleep .2; done
"$LND_DIR/lnd" --lnddir="$LND" --bitcoin.regtest --bitcoin.node=bitcoind --bitcoind.rpchost="127.0.0.1:$RPC" \
  --bitcoind.rpcuser="$probe_rpc_user" --bitcoind.rpcpass="$probe_rpc_pass" --bitcoind.zmqpubrawblock="tcp://127.0.0.1:$ZMQ_BLK" \
  --bitcoind.zmqpubrawtx="tcp://127.0.0.1:$ZMQ_TX" --listen="127.0.0.1:$LND_P2P" \
  --rpclisten="127.0.0.1:$LND_RPC" --restlisten="127.0.0.1:$LND_REST" --noseedbackup --no-macaroons --alias=payer >"$BASE/lnd.log" 2>&1 & PIDS+=("$!"); LND_STARTED=1
for _ in $(seq 1 120); do [ "$(lnc getinfo 2>/dev/null | jq -r .synced_to_chain || true)" = true ] && break; sleep 1; done
[ "$(lnc getinfo 2>/dev/null | jq -r .synced_to_chain || true)" = true ] || { echo 'INCONCLUSIVE: LND did not sync' >&2; exit 2; }
"$LSP_BIN" dir="$LSP" ctl="$LSP_CTL" rpc="$GATE_RPC" user="$probe_rpc_user" pass="$probe_rpc_pass" p2p="$LSP_P2P" >"$BASE/lsp.log" 2>&1 & PIDS+=("$!"); LSP_STARTED=1
for _ in $(seq 1 120); do (exec 3<>/dev/tcp/127.0.0.1/"$LSP_CTL") 2>/dev/null && break; sleep 1; done
LSP_ID=$(ctl "$LSP_CTL" nodeid || true)
[ -n "$LSP_ID" ] || { echo 'INCONCLUSIVE: provider did not start through RPC proxy' >&2; exit 2; }
bw miner sendtoaddress "$(ctl "$LSP_CTL" addr)" 1 >/dev/null
LND_ADDR=$(lnc newaddress p2wkh | jq -r .address)
[ -n "$LND_ADDR" ] && [ "$LND_ADDR" != null ] || { echo 'INCONCLUSIVE: LND wallet is not initialized/unlocked' >&2; exit 2; }
bw miner sendtoaddress "$LND_ADDR" 1 >/dev/null
bw miner generatetoaddress 6 "$MINER_ADDR" >/dev/null
for _ in $(seq 1 60); do [ "$(lnc walletbalance | jq -r .confirmed_balance 2>/dev/null || echo 0)" -gt 0 ] && break; sleep 1; done
lnc connect "$LSP_ID@127.0.0.1:$LSP_P2P" >/dev/null
for _ in $(seq 1 30); do lnc openchannel --node_key="$LSP_ID" --local_amt=5000000 --private >/dev/null 2>&1 && break; sleep 2; done
[ "$(lnc pendingchannels | jq '.pending_open_channels|length')" = 1 ] || { echo 'INCONCLUSIVE: payer could not open provider channel' >&2; exit 2; }
bw miner generatetoaddress 6 "$MINER_ADDR" >/dev/null
for _ in $(seq 1 90); do [ "$(lnc listchannels | jq '[.channels[]|select(.active)]|length')" = 1 ] && break; sleep 1; done
[ "$(lnc listchannels | jq '[.channels[]|select(.active)]|length')" = 1 ] || { echo 'INCONCLUSIVE: payer-provider channel not active' >&2; exit 2; }
"$DEVICE_BIN" "$DEV_PORT" "$DEV" >"$BASE/device.log" 2>&1 & PIDS+=("$!")
for _ in $(seq 1 30); do (exec 3<>/dev/tcp/127.0.0.1/"$DEV_PORT") 2>/dev/null && break; sleep 1; done
"$SHOP_BIN" ctl="$SHOP_CTL" data="$SHOP" rpc="$RPC" user="$probe_rpc_user" pass="$probe_rpc_pass" provider_id="$LSP_ID" \
  provider_addr="127.0.0.1:$LSP_P2P" device="$DEV_PORT" sweep_index=5 max_fee_msat=3000000 >"$BASE/shop.out" 2>"$BASE/shop.err" & PIDS+=("$!"); SHOP_STARTED=1
for _ in $(seq 1 120); do (exec 3<>/dev/tcp/127.0.0.1/"$SHOP_CTL") 2>/dev/null && break; sleep 1; done
SHOP_ID=$(ctl "$SHOP_CTL" nodeid || true)
[ -n "$SHOP_ID" ] || { echo 'INCONCLUSIVE: shop did not start' >&2; exit 2; }
for _ in $(seq 1 60); do ctl "$SHOP_CTL" status | grep -q 'provider=true' && break; sleep 1; done
INVOICE_REPLY=$(ctl "$SHOP_CTL" invoice 50000)
INVOICE=$(sed -n 's/^invoice \([^ ]*\) .*/\1/p' <<<"$INVOICE_REPLY")
[ -n "$INVOICE" ] || { echo "INCONCLUSIVE: invoice not created: $INVOICE_REPLY" >&2; exit 2; }
printf '%s\n' "$INVOICE_REPLY" >"$BASE/invoice.txt"
# Gate only after invoice negotiation. The honest control leaves this absent.
if [ "$PROBE_MODE" = withhold ]; then touch "$GATE"; fi
set +e
 timeout 110 "$LND_DIR/lncli" --lnddir="$LND" --tlscertpath="$LND/tls.cert" --network=regtest --rpcserver="127.0.0.1:$LND_RPC" --no-macaroons \
   payinvoice --pay_req="$INVOICE" --force --timeout=90s >"$BASE/pay.log" 2>&1
PAY_RC=$?
set -e
sleep 2
HASH=$(lnc decodepayreq "$INVOICE" | jq -r .payment_hash)
PAYMENT_JSON=$(lnc listpayments --include_incomplete | jq -c --arg h "$HASH" '.payments[]|select(.payment_hash==$h)' | head -1)
[ -n "$PAYMENT_JSON" ] || PAYMENT_JSON='{}'
PAY_STATUS=$(jq -r '.status // empty' <<<"$PAYMENT_JSON")
PREIMAGE=$(jq -r '.payment_preimage // empty' <<<"$PAYMENT_JSON")
printf 'payment_hash=%s\npayment_status=%s\npayment_preimage=%s\n' "$HASH" "${PAY_STATUS:-missing}" "${PREIMAGE:-missing}" >"$BASE/payer-evidence.txt"
PREIMAGE_HASH=
if [[ "$PREIMAGE" =~ ^[0-9a-fA-F]{64}$ ]]; then PREIMAGE_HASH=$(python3 - "$PREIMAGE" <<'PYHASH'
import hashlib, sys
print(hashlib.sha256(bytes.fromhex(sys.argv[1])).hexdigest())
PYHASH
); fi
PAID=$(ctl "$SHOP_CTL" paid || true)
printf '%s\n' "$PAID" >"$BASE/paid.txt"
SHOP_LINE=$(grep -F "paid $HASH " "$BASE/paid.txt" | head -1 || true)
SHOP_FUNDING=${SHOP_LINE##* funding=}
PROXY_TXID=$(cat "$MARKER" 2>/dev/null || true)
if [ "$PROBE_MODE" = withhold ]; then TXID=$PROXY_TXID; else TXID=$SHOP_FUNDING; fi
CHANNELS=$(ctl "$LSP_CTL" channels || true)
printf '%s\n' "$CHANNELS" >"$BASE/provider-channels.txt"
BLOCKCHAIN=$(bcli getblockchaininfo 2>"$BASE/core-live.err" || true)
CORE_LIVE=0
if jq -e '.chain == "regtest"' >/dev/null 2>&1 <<<"$BLOCKCHAIN"; then CORE_LIVE=1; fi
MEMPOOL_FOUND=0; RAWTX_FOUND=0; MEMPOOL_ABSENT=0; RAWTX_ABSENT=0
if [ -n "$TXID" ] && [ "$CORE_LIVE" = 1 ]; then
  for _ in $(seq 1 15); do
    if bcli getmempoolentry "$TXID" >"$BASE/mempool.result" 2>&1; then MEMPOOL_FOUND=1; break; fi
    grep -Fq 'error code: -5' "$BASE/mempool.result" || break
    [ "$PROBE_MODE" = honest ] || break
    sleep 1
  done
  if [ "$MEMPOOL_FOUND" = 0 ] && grep -Fq 'error code: -5' "$BASE/mempool.result"; then MEMPOOL_ABSENT=1; fi
  if bcli getrawtransaction "$TXID" true >"$BASE/rawtx.result" 2>&1; then RAWTX_FOUND=1
  elif grep -Fq 'error code: -5' "$BASE/rawtx.result"; then RAWTX_ABSENT=1; fi
fi
printf 'RESULT mode=%s payer_rc=%s payer_status=%s preimage_hash=%s invoice_hash=%s shop_line=%s shop_funding=%s proxy_suppressed_txid=%s evidence_txid=%s core_live_regtest=%s mempool_found=%s mempool_absent_error_minus5=%s rawtx_found=%s rawtx_absent_error_minus5=%s\n' \
 "$PROBE_MODE" "$PAY_RC" "${PAY_STATUS:-missing}" "${PREIMAGE_HASH:-missing}" "$HASH" "${SHOP_LINE:-missing}" "${SHOP_FUNDING:-missing}" "${PROXY_TXID:-none}" "${TXID:-missing}" "$CORE_LIVE" "$MEMPOOL_FOUND" "$MEMPOOL_ABSENT" "$RAWTX_FOUND" "$RAWTX_ABSENT"
if [ "$PAY_RC" = 0 ] && [ "$PAY_STATUS" = SUCCEEDED ] && [ "$PREIMAGE_HASH" = "$HASH" ] && [ "$SHOP_FUNDING" = "$TXID" ] && [ "$CORE_LIVE" = 1 ]; then
  if [ "$PROBE_MODE" = withhold ] && [ "$TXID" = "$PROXY_TXID" ] && [ "$MEMPOOL_ABSENT" = 1 ] && [ "$RAWTX_ABSENT" = 1 ]; then
    echo 'DECISIVE: preimage hash matches and Shop/payment settle while Core returns exact -5 not-found for both queries.'
    exit 0
  elif [ "$PROBE_MODE" = honest ] && [ -z "$PROXY_TXID" ] && [ "$MEMPOOL_FOUND" = 1 ] && [ "$RAWTX_FOUND" = 1 ]; then
    echo 'CONTROL observed: honest provider broadcast appears in live Core mempool and transaction lookup (unconfirmed).'
    exit 0
  fi
fi
echo 'INCONCLUSIVE: required evidence did not align; inspect retained logs and state, do not count as pass.'
exit 3
