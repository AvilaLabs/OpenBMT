#!/usr/bin/env bash
# The owner phone's web orders against Bitcoin Core on regtest and the
# plugin's own REST callbacks (docs/WEB-ORDER-RULES.md, WO-05, WO-10 to WO-12,
# WO-20 to WO-22, WO-30 to WO-32).
#
# Question: does an owner node that pairs with a store give it addresses,
# take its orders as requests, count a payment at the shop's depth, tell the
# store with a signed notice that the plugin accepts and acts on, tell it
# only what a part payment is, send a new range when the store runs low, and
# refuse an order whose address it never gave?
#
# The node is the example `web_orders_regtest` with one Bitcoin Core node,
# serving compact filters on 127.0.0.1, as its only peer. This script plays
# the app: it sends each call the node makes to the plugin, and hands the
# answer back. The store is the plugin's own REST callbacks
# (integrations/woocommerce/openbmt-pay/wc/class-openbmt-pay-rest.php) run
# by the PHP command line, one process for each request, with the few
# WordPress functions they call stood in for by tests_e2e/wp_stubs.php and
# the store's data in a JSON file. No WordPress and no web server run, and
# the HTTPS and Tor of a real visit are not part of this check. The
# confirmation depth is 2.
#
# What it does (the steps are numbered as in the printed table):
#   1. Pairing: the owner node shows a code, the plugin reads it, and the
#      fingerprints are the same. The first visit gives the store 100
#      addresses.
#   2. An order is made in the store (its address is the pool's next one)
#      and the next visit makes a request of it at that address.
#   3. Core pays the whole order: at one confirmation nothing is told; at
#      two the request is paid, the next visit sends a signed notice, and
#      the plugin completes the order. A second visit changes nothing, and
#      the same notice sent again, or bent, is refused.
#   4. An order of 40000 sat is paid 10000 sat: the plugin gets a notice of a
#      part payment, notes it and leaves the order on hold, not paid.
#   5. Orders take the store below 20 addresses: the next visit sends a new
#      range and the store has 100 more.
#   6. An order at an address the owner node never gave (a store that is not
#      the shop's) is refused and shown, and no request is made.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (put first on
# PATH; set CORE_BIN to use another), jq, php 8.2 or later with sodium, and
# cargo unless WEB_BIN names a built `web_orders_regtest` example. A build
# goes through ~/.cache/openbmt/guard.sh when that file exists.
#
# Environment:
#   CORE_BIN       directory with bitcoind and bitcoin-cli
#   DRILL_DIR      scratch directory (default: a new directory under
#                  ~/.cache/openbmt)
#   DRILL_PORTS    the RPC port; the peer port is the next one (default 21320)
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   WEB_BIN        the `web_orders_regtest` example binary
#
# Everything here is regtest on 127.0.0.1, with keys from fixed test words:
# TEST ONLY, NEVER USE. The node and the scratch directory are removed on
# every exit. The default Bitcoin data directory is never touched.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_PORTS:-21320}
P2P_PORT=$((RPC_PORT + 1))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/web-orders-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PLUGIN="$ROOT/integrations/woocommerce/openbmt-pay"
NODE_DIR="$BASE/core"
DATA_DIR="$BASE/owner"
FIFO="$BASE/stdin"
export OBM_E2E_STATE="$BASE/store.json"
STORE_URL="https://shop.example/store"

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
cli() { bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" "$@"; }

declare -a RESULT BAD
pass() { RESULT[$1]=PASS; printf 'PASS  step %s: %s\n' "$1" "$2"; }
bad() { RESULT[$1]=FAIL; printf 'FAIL  step %s: %s\n' "$1" "$2"; }
chk() {
  local n=$1 what=$2
  shift 2
  if "$@"; then note "ok:     $what"; else note "NOT ok: $what"; BAD[$n]=1; fi
}
done_step() {
  if [ -z "${BAD[$1]:-}" ]; then pass "$1" "$2"; else bad "$1" "a check failed: $2"; fi
}

for tool in bitcoind bitcoin-cli jq php; do
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
LAST=

cleanup() {
  exec 3>&- 2>/dev/null || true
  if [ -n "$EX_PID" ] && kill -0 "$EX_PID" 2>/dev/null; then
    kill "$EX_PID" 2>/dev/null || true
    sleep 1
    kill -9 "$EX_PID" 2>/dev/null || true
  fi
  if [ -n "$NODE_PID" ]; then
    cli stop >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$NODE_PID" 2>/dev/null || true
  fi
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

if [ -z "${WEB_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/web-orders}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner-node --example web_orders_regtest ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner-node --example web_orders_regtest ||
      die "the build failed"
  fi
  WEB_BIN="$TARGET/debug/examples/web_orders_regtest"
fi
[ -x "$WEB_BIN" ] || die "no example binary at $WEB_BIN"


# ---- helpers for the example ------------------------------------------------

start_example() { # tag
  EX_LOG="$BASE/example-$1.log"
  : >"$EX_LOG"
  EX_N=0
  rm -f "$FIFO"
  mkfifo "$FIFO"
  exec 3<>"$FIFO"
  "$WEB_BIN" "$DATA_DIR" "127.0.0.1:$P2P_PORT" --depth 2 <&3 >"$EX_LOG" 2>&1 &
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
  wait "$EX_PID"
  EX_PID=
  exec 3>&-
}
# Wait until the node is at Core's tip, has checked up to it and is ready.
await_synced() { # seconds
  local want i
  want=$(cli getblockcount)
  for i in $(seq 1 $(($1 * 2))); do
    cmd tip || return 1
    if grep -q "^status tip=$want checked=$want view=ready\$" <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
# Wait until the book's line for the web order matches the pattern.
await_request() { # order pattern seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    cmd requests || return 1
    if grep -Eq "^request [0-9]+ $2 (.* )?Web order $1\$" <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
# Send the call the node made (in $LAST) to the plugin's own REST callbacks
# and hand the answer back, as the app does with the store's.
send_call() {
  local url route type body headers request answer status data
  url=$(sed -n 's/^url //p' <<<"$LAST")
  route=${url#*/wp-json}
  type=$(sed -n 's/^type //p' <<<"$LAST")
  body=$(sed -n 's/^body //p' <<<"$LAST")
  headers=$(sed -n 's/^header \([^ ]*\) \(.*\)$/\1\t\2/p' <<<"$LAST" |
    jq -R -s -c 'split("\n") | map(select(length > 0) | split("\t") | {(.[0]): .[1]}) | add')
  request=$(jq -n -c --arg route "$route" --arg body "$body" --argjson headers "$headers" \
    '{method: "POST", route: $route, headers: $headers, body: $body}')
  LAST_REQUEST=$request
  answer=$(php -d display_errors=stderr "$PLUGIN/tests_e2e/route.php" <<<"$request") || return 1
  status=$(jq -r '.status' <<<"$answer")
  data=$(jq -c '.data // empty' <<<"$answer")
  CALL_KIND=$(sed -n 's/^call //p' <<<"$LAST")
  CALL_STATUS=$status
  CALL_DATA=$data
  cmd "reply $status $data" || return 1
}
# One visit: the node's steps until it has nothing more to send. The kinds of
# the calls it made are in CALLS, the last answer's status and data in
# CALL_STATUS and CALL_DATA.
visit() {
  local i
  CALLS=
  for i in $(seq 1 30); do
    cmd step || return 1
    if grep -q '^idle$' <<<"$LAST"; then return 0; fi
    grep -q '^call ' <<<"$LAST" || return 1
    send_call || return 1
    CALLS="$CALLS $CALL_KIND"
  done
  return 1
}
# The value of one field of the node's `web` line, in WF. Not in a
# subshell: the commands are counted here.
wf() { # name
  cmd web || return 1
  WF=$(sed -nE "s/^web .*\b$1=([^ ]*).*/\1/p" <<<"$LAST")
}
order_in_store() { # order, a jq filter
  jq -r ".orders[\"$1\"] | $2" "$OBM_E2E_STATE"
}
mine() { # blocks
  cli generatetoaddress "$1" "$MINE" >/dev/null
}

# ---- Core --------------------------------------------------------------------

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" \
  -listen=1 -bind="127.0.0.1:$P2P_PORT" -listenonion=0 -daemonwait \
  -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
  -fallbackfee=0.0001 -dbcache=50 >/dev/null || die "bitcoind did not start"
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
[ -n "$NODE_PID" ] || die "no node pid"
cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli -rpcwallet=miner getnewaddress)
CUSTOMER=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 110 "$MINE" >/dev/null || die "could not mine"
say "One node that serves filters, at height 110"


start_example first
await "^started " 30 || die "the example did not start"
await_synced 90 || die "the example did not sync"
paid_to() { # address sats-as-btc
  cli -rpcwallet=miner sendtoaddress "$1" "$2" >/dev/null
}

# ---- 1 ----------------------------------------------------------------------

say "1. Pairing, and the first range"
cmd "pair $STORE_URL Corner Shop" || die "no answer to pair"
CODE=$(sed -n 's/^code //p' <<<"$LAST")
FP_NODE=$(sed -n 's/^fingerprint //p' <<<"$LAST")
[ -n "$CODE" ] || die "no pairing code"
FP_STORE=$(php "$PLUGIN/tests_e2e/pair.php" "$CODE") || die "the plugin did not take the code"
note "owner node: $FP_NODE   plugin: $FP_STORE"
squash() { tr -d ' -' <<<"$1" | tr 'A-Z' 'a-z'; }
chk 1 "the plugin shows the fingerprint the owner node shows" \
  [ "$(squash "$FP_NODE")" = "$(squash "$FP_STORE")" ]
cmd confirm
visit || die "the first visit did not end"
note "calls:$CALLS"
chk 1 "the visit asked for orders and sent one range" [ "$CALLS" = " Orders Range" ]
chk 1 "the plugin accepted the range and has 100 addresses" [ "$(jq -r '.data.left' <<<"{\"data\":$CALL_DATA}")" = 100 ]
wf left
chk 1 "the owner node says the store has 100 left" [ "$WF" = 100 ]
done_step 1 "the store is paired, the fingerprints match, the plugin holds a range it checked"

# ---- 2 ----------------------------------------------------------------------

say "2. An order becomes a request at its address"
ADDR1=$(php "$PLUGIN/tests_e2e/make_order.php" 1 50000) || die "no order"
note "order 1 at $ADDR1"
visit || die "the visit did not end"
note "calls:$CALLS"
cmd "regtest $ADDR1"
PAY1=$(sed -n 's/^regtest //p' <<<"$LAST")
chk 2 "the order's address is a valid regtest address" \
  [ "$(cli validateaddress "$PAY1" | jq -r '.isvalid')" = true ]
await_request 1 "waiting credited=0" 20
chk 2 "the book holds a request for the order, waiting" [ $? = 0 ]
cmd "order 1"
chk 2 "the order is taken and waits" grep -q '^order 1 waiting$' <<<"$LAST"
chk 2 "the store has the order on hold" [ "$(order_in_store 1 .status)" = on-hold ]
done_step 2 "an order the store reports is a request of the owner node, at the address the store gave"

# ---- 3 ----------------------------------------------------------------------

say "3. The whole order is paid: a signed notice, and the plugin completes the order"
paid_to "$PAY1" 0.0005 || die "no payment"
mine 1
await_synced 60 || BAD[3]=1
visit
wf told
chk 3 "at one confirmation no notice is sent" [ "$WF" = 0 ]
chk 3 "at one confirmation the order is not paid in the store" [ "$(order_in_store 1 .paid)" = false ]
mine 1
await_request 1 "paid credited=50000" 60
chk 3 "at two confirmations the request is paid" [ $? = 0 ]
visit
note "calls:$CALLS  last answer: $CALL_DATA"
chk 3 "the visit sent a notice" bash -c '[[ "$1" == *Notice* ]]' _ "$CALLS"
chk 3 "the plugin took it and completed the order" [ "$CALL_DATA" = '{"action":"complete"}' ]
chk 3 "the order is paid in the store" [ "$(order_in_store 1 .paid)" = true ]
chk 3 "the order is processing" [ "$(order_in_store 1 .status)" = processing ]
chk 3 "the order holds the payment's transaction ID" [ "$(order_in_store 1 '.transaction_id | length')" = 64 ]
chk 3 "the order has a note with the amounts" [ "$(order_in_store 1 '.notes | length')" -ge 2 ]
wf told
chk 3 "the owner node counts the order told" [ "$WF" = 1 ]
NOTES=$(order_in_store 1 '.notes | length')
visit
chk 3 "a second visit sends no notice" bash -c '[[ "$1" != *Notice* ]]' _ "$CALLS"
chk 3 "a second visit changes nothing in the store" [ "$(order_in_store 1 '.notes | length')" = "$NOTES" ]
# The same notice again, and the same notice with its body bent: the plugin
# refuses both before it reads them, and the order does not change.
REPLAYED=$(php "$PLUGIN/tests_e2e/route.php" <<<"$LAST_REQUEST" | jq -r '.status')
chk 3 "the same notice sent again is refused (the number used once)" [ "$REPLAYED" = 403 ]
BENT=$(jq -c '.body |= (.[:-1] + (if .[-1:] == "0" then "1" else "0" end))' <<<"$LAST_REQUEST")
BENT_STATUS=$(php "$PLUGIN/tests_e2e/route.php" <<<"$BENT" | jq -r '.status')
chk 3 "a notice with its body bent is refused (the HMAC)" [ "$BENT_STATUS" = 403 ]
chk 3 "the store's order is as it was" [ "$(order_in_store 1 '.notes | length')" = "$NOTES" ]
done_step 3 "a payment at the shop's depth gives a signed notice, and the plugin pays the order once"

# ---- 4 ----------------------------------------------------------------------

say "4. A part payment does not pay the order"
ADDR2=$(php "$PLUGIN/tests_e2e/make_order.php" 2 40000) || die "no order"
visit
cmd "regtest $ADDR2"
PAY2=$(sed -n 's/^regtest //p' <<<"$LAST")
paid_to "$PAY2" 0.0001 || die "no payment"
mine 2
await_request 2 "partpaid-30000 credited=10000" 60
chk 4 "the request is part paid, with 30000 sat owed" [ $? = 0 ]
visit
note "calls:$CALLS  last answer: $CALL_DATA"
chk 4 "the visit sent a notice" bash -c '[[ "$1" == *Notice* ]]' _ "$CALLS"
chk 4 "the plugin only noted it" [ "$CALL_DATA" = '{"action":"note"}' ]
chk 4 "the order is not paid in the store" [ "$(order_in_store 2 .paid)" = false ]
chk 4 "the order is still on hold" [ "$(order_in_store 2 .status)" = on-hold ]
chk 4 "the plugin noted the part payment" [ "$(order_in_store 2 '.notes | length')" -ge 2 ]
done_step 4 "a part payment is noted by the plugin and the order stays on hold"

# ---- 5 ----------------------------------------------------------------------

say "5. The store runs low: a new range"
for n in $(seq 3 81); do
  php "$PLUGIN/tests_e2e/make_order.php" "$n" 1000 >/dev/null || die "no order $n"
done
chk 5 "the store has 19 addresses left" \
  [ "$(php -r '$d=json_decode(file_get_contents(getenv("OBM_E2E_STATE")),true);echo count($d["options"]["openbmt_pay_pool"]["queue"]);')" = 19 ]
visit
note "calls:$CALLS  last answer: $CALL_DATA"
chk 5 "the visit reported the orders and sent a new range" [ "$CALLS" = " Orders Range" ]
wf left
chk 5 "the plugin accepted it and has 119 addresses" [ "$WF" = 119 ]
wf sent
chk 5 "the owner node has given 200 addresses" [ "$WF" = 200 ]
wf orders
chk 5 "every order is a request" [ "$WF" = 81 ]
done_step 5 "below 20 addresses the owner node sends the next range"

# ---- 6 ----------------------------------------------------------------------

say "6. An order at an address the owner node never gave is refused"
php "$PLUGIN/tests_e2e/make_order.php" 90 5000 "$MINE" >/dev/null || die "no order"
visit
cmd "order 90"
chk 6 "the order is refused" grep -q '^order 90 refused$' <<<"$LAST"
wf refused
chk 6 "the owner node shows one refused order" [ "$WF" = 1 ]
cmd requests
chk 6 "no request was made for it" bash -c '! grep -q "Web order 90" <<<"$1"' _ "$LAST"
done_step 6 "an address that was not in a range the owner node sent is refused and shown"

echo
FAILED=0
for n in 1 2 3 4 5 6; do [ "${RESULT[$n]:-}" = PASS ] || FAILED=$((FAILED + 1)); done
if [ "$FAILED" = 0 ]; then
  echo "ALL PASS"
else
  echo "FAILED: $FAILED step(s)"
  exit 1
fi
