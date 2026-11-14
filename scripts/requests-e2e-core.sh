#!/usr/bin/env bash
# The owner's requests against Bitcoin Core, on regtest (docs/REQUEST-RULES.md,
# RQ-03, RQ-10, RQ-15 to RQ-18, RQ-26 to RQ-31, RQ-37).
#
# Question: does a request made by the owner node of a shop with its own
# wallet get the payments Core sends to its address, count them only at the
# confirmation depth, take a payment back when its block leaves the chain and
# count it again when it is mined again, queue a return on the owner's queue
# without paying it, and keep all of it across a restart?
#
# One Bitcoin Core node serves compact filters on 127.0.0.1, and the example
# `requests_regtest` is the owner node with it as its only peer. The shop's
# wallet is a single key of fixed test words on the main network's path, so
# the addresses the example prints are the regtest form of the same scripts
# (the example converts in both directions). The confirmation depth is 2.
# The example is in the owner library's crate, and its `read` command prints
# a request as the library's `request_read` (op 234) gives it to the app.
#
# What it does (the steps are numbered as in the printed table):
#   1. The example starts and syncs. A request for 100000 sat is made: its
#      address is a valid regtest address and the book holds the request.
#   2. Core pays 40000 sat to the address. At one confirmation the request
#      is still waiting and the payment is held; at two it is part paid, with
#      60000 sat still owed.
#   3. Core pays the rest. At two confirmations the request is paid.
#   4. The block with the second payment is invalidated and a longer chain
#      without it is mined: the credit is taken back, the request is part
#      paid again. The payment is mined again and the request is paid again.
#      The library's read of the request holds both payments (txid, amount,
#      confirmed, credit), the request's address and its unit.
#   5. A return of 20000 sat to an address of the customer is accepted: it is
#      queued on the owner's queue and nothing is paid, and Core's mempool
#      stays empty. The read holds the return (state, amount, address).
#   6. The example is stopped and started again on the same directory: the
#      request, its credits and the queued return are as they were, in the
#      read as well.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (put first on
# PATH; set CORE_BIN to use another), jq, and cargo unless REQUESTS_BIN names a
# built `requests_regtest` example. A build goes through
# ~/.cache/openbmt/guard.sh when that file exists.
#
# Environment:
#   CORE_BIN       directory with bitcoind and bitcoin-cli
#   DRILL_DIR      scratch directory (default: a new directory under
#                  ~/.cache/openbmt)
#   DRILL_PORTS    the RPC port; the peer port is the next one (default 21300)
#   DRILL_KEEP=1   keep the scratch directory afterwards
#   REQUESTS_BIN   the `requests_regtest` example binary
#
# Everything here is regtest on 127.0.0.1, with keys from fixed test words:
# TEST ONLY, NEVER USE. The node and the scratch directory are removed on
# every exit. The default Bitcoin data directory is never touched.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_PORTS:-21300}
P2P_PORT=$((RPC_PORT + 1))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/requests-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/core"
DATA_DIR="$BASE/owner"
FIFO="$BASE/stdin"

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

if [ -z "${REQUESTS_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/requests}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner-ffi --example requests_regtest ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner-ffi --example requests_regtest ||
      die "the build failed"
  fi
  REQUESTS_BIN="$TARGET/debug/examples/requests_regtest"
fi
[ -x "$REQUESTS_BIN" ] || die "no example binary at $REQUESTS_BIN"

# ---- helpers for the example ------------------------------------------------

start_example() { # tag
  EX_LOG="$BASE/example-$1.log"
  : >"$EX_LOG"
  EX_N=0
  rm -f "$FIFO"
  mkfifo "$FIFO"
  exec 3<>"$FIFO"
  "$REQUESTS_BIN" "$DATA_DIR" "127.0.0.1:$P2P_PORT" --depth 2 <&3 >"$EX_LOG" 2>&1 &
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
# Wait until the request line matches the pattern.
await_request() { # number pattern seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    cmd "status $1" || return 1
    if grep -Eq "^request $1 $2( |\$)" <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
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

# ---- 1 ----------------------------------------------------------------------

say "1. A request for 100000 sat"
start_example first
await "^started " 30 || die "the example did not start"
await_synced 90 || die "the example did not sync"
cmd "make 100000 Rent for the stall" || die "no answer to make"
note "$(grep '^made' <<<"$LAST")"
ADDR=$(sed -nE 's/^made 1 [0-9]+ (.*)$/\1/p' <<<"$LAST")
[ -n "$ADDR" ] || die "no request from the example"
chk 1 "Core reads the address as a valid regtest address" \
  [ "$(cli validateaddress "$ADDR" | jq -r '.isvalid')" = true ]
chk 1 "the address is a native SegWit address of the test chain" bash -c '[[ "$1" == bcrt1q* ]]' _ "$ADDR"
cmd "share 1" && note "$(grep '^shared' <<<"$LAST")"
chk 1 "the link pays the same address for 100000 sat" \
  grep -q "^shared 1 sats=100000 bitcoin:$ADDR" <<<"$LAST"
await_request 1 waiting 10 || BAD[1]=1
done_step 1 "a request is made, its address is a regtest address of the wallet, and it waits"

# ---- 2 ----------------------------------------------------------------------

say "2. 40000 sat: counted at the second confirmation, not at the first"
TX1=$(cli -rpcwallet=miner sendtoaddress "$ADDR" 0.0004) || die "no payment"
mine 1
await_synced 60 || BAD[2]=1
cmd "status 1"
note "$(grep '^request' <<<"$LAST")"
chk 2 "at one confirmation the request is still waiting, the payment is held" \
  grep -q '^request 1 waiting credited=0 pending=1$' <<<"$LAST"
cmd "read 1"
note "$(tr '\n' ';' <<<"$LAST")"
chk 2 "the read shows the held payment as not confirmed, credited 0, with its txid and 40000 sat" \
  grep -q "^payment $TX1 vout=[0-9]* sats=40000 confirmed=false credit=0\$" <<<"$LAST"
chk 2 "the read says waiting, in sat, at the request's own address (the wallet's main network form as read)" \
  grep -Eq "^read 1 Waiting unit=SAT address=$ADDR raw=bc1q[a-z0-9]+ amount=100000 paid=0 received=0 returned=0 payments=1 returns=0\$" <<<"$LAST"
mine 1
await_request 1 'partpaid-60000 credited=40000' 60
chk 2 "at two confirmations 40000 sat is counted and 60000 sat is still owed" [ $? = 0 ]
done_step 2 "a payment is counted at the confirmation depth and not before"

# ---- 3 ----------------------------------------------------------------------

say "3. The rest, 60000 sat"
TX2=$(cli -rpcwallet=miner sendtoaddress "$ADDR" 0.0006) || die "no second payment"
mine 1
B2=$(cli -rpcwallet=miner gettransaction "$TX2" | jq -r '.blockhash')
[ -n "$B2" ] && [ "$B2" != null ] || die "the second payment is in no block"
H2=$(cli getblock "$B2" | jq -r '.height')
mine 1
await_request 1 'paid credited=100000' 60
chk 3 "at two confirmations the request is paid in full" [ $? = 0 ]
done_step 3 "the rest of the payment makes the request paid"

# ---- 4 ----------------------------------------------------------------------

say "4. The block with the second payment leaves the chain"
cli invalidateblock "$B2" >/dev/null || die "could not invalidate the block"
cli generateblock "$MINE" '[]' >/dev/null
cli generateblock "$MINE" '[]' >/dev/null
cli generateblock "$MINE" '[]' >/dev/null
chk 4 "Core's chain is now longer than the old one, without the payment" \
  [ "$(cli getblockcount)" -gt "$H2" ]
await_synced 90 || BAD[4]=1
await_request 1 'partpaid-60000 credited=40000' 60
chk 4 "the second payment's credit is taken back" [ $? = 0 ]
mine 2
await_request 1 'paid credited=100000' 90
chk 4 "mined again, it is counted again and the request is paid" [ $? = 0 ]
cmd "read 1"
note "$(tr '\n' ';' <<<"$LAST")"
chk 4 "the read says paid: 100000 credited, 100000 received, two payments" \
  grep -Eq "^read 1 Paid unit=SAT address=$ADDR raw=bc1q[a-z0-9]+ amount=100000 paid=100000 received=100000 returned=0 payments=2 returns=0\$" <<<"$LAST"
chk 4 "the read holds the first payment: its txid, 40000 sat, confirmed, credited 40000" \
  grep -q "^payment $TX1 vout=[0-9]* sats=40000 confirmed=true credit=40000\$" <<<"$LAST"
chk 4 "the read holds the second payment: its txid, 60000 sat, confirmed, credited 60000" \
  grep -q "^payment $TX2 vout=[0-9]* sats=60000 confirmed=true credit=60000\$" <<<"$LAST"
done_step 4 "a payment whose block left the chain is taken back and counted again when it is mined again"

# ---- 5 ----------------------------------------------------------------------

say "5. A return of 20000 sat is accepted and queued"
MEMPOOL=$(cli getmempoolinfo | jq -r '.size')
cmd "return 1 20000 $CUSTOMER"
note "$(grep -E '^(returned|refused)' <<<"$LAST")"
chk 5 "the return is accepted" grep -q '^returned id=1 refund=[0-9]* sats=20000$' <<<"$LAST"
cmd queue
note "$(tr '\n' ';' <<<"$LAST")"
chk 5 "the refund waits on the owner's queue" grep -q '^entry [0-9]* Queued$' <<<"$LAST"
chk 5 "the book holds the return as queued" grep -q '^return 1 queued$' <<<"$LAST"
cmd "read 1"
note "$(tr '\n' ';' <<<"$LAST")"
chk 5 "the read holds the return: queued, 20000 sat, to the customer's address" \
  grep -q "^returnview 1 Queued sats=20000 address=$CUSTOMER\$" <<<"$LAST"
chk 5 "the read counts the 20000 sat as returned" \
  grep -q '^read 1 Paid .* received=100000 returned=20000 payments=2 returns=1$' <<<"$LAST"
sleep 2
chk 5 "nothing was paid: Core's mempool is as it was" \
  [ "$(cli getmempoolinfo | jq -r '.size')" = "$MEMPOOL" ]
cmd "return 1 90000 $CUSTOMER"
chk 5 "a return of more than the request received is refused" grep -q '^refused ' <<<"$LAST"
done_step 5 "a return is queued on the owner's queue and nothing is paid"

# ---- 6 ----------------------------------------------------------------------

say "6. The example stops and starts again on the same directory"
stop_example
start_example second
await "^started " 30 || die "the example did not start again"
await_synced 90 || BAD[6]=1
cmd "status 1"
note "$(grep '^request' <<<"$LAST")"
chk 6 "the request and its credits are as they were" \
  grep -q '^request 1 paid credited=100000 pending=0$' <<<"$LAST"
cmd queue
chk 6 "the refund is still queued" grep -q '^entry [0-9]* Queued$' <<<"$LAST"
chk 6 "the return is still in the book" grep -q '^return 1 queued$' <<<"$LAST"
cmd "read 1"
chk 6 "the read still holds both payments and the queued return" \
  bash -c 'grep -q "^payment $1 .*confirmed=true credit=40000\$" <<<"$3" && grep -q "^payment $2 .*confirmed=true credit=60000\$" <<<"$3" && grep -q "^returnview 1 Queued sats=20000 address=$4\$" <<<"$3"' _ "$TX1" "$TX2" "$LAST" "$CUSTOMER"
cmd "make 5000 Second"
chk 6 "a new request takes a new index" grep -q '^made 2 ' <<<"$LAST"
done_step 6 "the book, the credits and the queued return survive a restart"

echo
FAILED=0
for n in 1 2 3 4 5 6; do [ "${RESULT[$n]:-}" = PASS ] || FAILED=$((FAILED + 1)); done
if [ "$FAILED" = 0 ]; then
  echo "ALL PASS"
else
  echo "FAILED: $FAILED step(s)"
  exit 1
fi
