#!/usr/bin/env bash
# The switch from the starter wallet against Bitcoin Core, on regtest
# (docs/COUNTER-RULES.md section 26, CR-256 to CR-263; docs/START-SIMPLE-RULES.md
# SS-41).
#
# Question: does a counter paired with a starter wallet take the owner's
# switch to a new wallet, show the new wallet's first address as Bitcoin Core
# derives it, sell to the new wallet only after the manager's confirmation,
# keep watching the old wallet's handed-out addresses, and take no second
# switch?
#
# Bitcoin Core plays the chain and both wallets: the starter wallet and the
# new wallet are imported as descriptors into two blank watch-only wallets
# (`old` and `new`), and every address is compared with Core's own
# `deriveaddresses`. The counter is the real node, through the example
# `counter_regtest --own-wallet`. The owner phone and the app are played by the
# example with the fixed test owner key and a file (see its header): the real
# owner phone and counter app are checked in the crates' tests.
#
# What it does (the steps are numbered as in the printed table):
#   1. Pairs a counter with an offer of a starter wallet: the offer is
#      version 2 with the starter flag (CR-256), and the counter's first
#      address is Core's index 0 of the starter wallet.
#   2. Opens a sale at the starter wallet (Core's derivation of the same
#      index) and takes a payment from Core's wallet to it: Confirmed. A
#      second sale is opened and left unpaid.
#   3. The owner's switch to the new wallet is taken (CR-257, CR-258): the
#      counter shows the new wallet's first address, which is Core's index 0
#      of the new description, and its kind. A sale opened meanwhile still
#      pays the starter wallet (CR-259).
#   4. "Same address" (after the manager's PIN, which is the app's): the
#      book is written, the app keeps the new record, and the counter says
#      so (CR-261). The next sale is at the new wallet, at an index above
#      every index of the old one, and the address is Core's derivation of
#      that index of the new description. A payment to it from Core's wallet
#      confirms.
#   5. Core sees both payments at the right wallets: the first in the
#      starter wallet's watch-only wallet only, the second in the new
#      wallet's only.
#   6. The sale left unpaid at step 2 is paid after the switch: the old
#      wallet's addresses stay watched (CR-261) and it confirms.
#   7. The example is stopped and started again with the record the app
#      kept: the next sale is still at the new wallet, and a second switch is
#      refused (CR-263).
#   8. A second counter on its own folder is given the switch and the manager
#      says "Different" (CR-260): nothing is switched, its next sale is at the
#      starter wallet and the switch is gone.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another), jq, and
# cargo unless NODE_BIN names a prebuilt `counter_regtest` example binary.
# A build goes through ~/.cache/openbmt/guard.sh when that file exists.
#
# Environment:
#   CORE_BIN     directory with bitcoind and bitcoin-cli
#   DRILL_DIR    scratch directory (default: a new directory under
#                ~/.cache/openbmt)
#   DRILL_PORTS  the RPC port of the regtest node (default 20300); the peer
#                port is the next one
#   DRILL_KEEP=1 keep the scratch directory afterwards
#   NODE_BIN     the `counter_regtest` example binary
#
# Everything here is regtest on 127.0.0.1, with keys made from fixed test
# bytes. No real keys or funds are involved. The node and the scratch
# directory are removed on every exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_PORTS:-20300}
P2P_PORT=$((RPC_PORT + 1))
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/switch-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/core"
DATA_DIR="$BASE/counter"
OTHER_DIR="$BASE/counter-other"
FIFO="$BASE/stdin"
SALE_AMOUNT=10000     # $100.00 in cents
SALE_RATE=10000000    # $100,000 per bitcoin, in cents

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
cli() { bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" "$@"; }
btc() { printf '%d.%08d' $(($1 / 100000000)) $(($1 % 100000000)); }

declare -a RESULT MESSAGE
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

mkdir -p "$NODE_DIR" "$DATA_DIR"
NODE_PID=
EX_PID=
EX_LOG=
EX_N=0
EX_FLAGS=
LAST=

cleanup() {
  exec 3>&- 2>/dev/null || true
  if [ -n "$EX_PID" ] && kill -0 "$EX_PID" 2>/dev/null; then
    kill "$EX_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$EX_PID" 2>/dev/null || break
      sleep 0.25
    done
    kill -9 "$EX_PID" 2>/dev/null || true
  fi
  if [ -n "$NODE_PID" ]; then
    bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" stop >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$NODE_PID" 2>/dev/null || true
  fi
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$BASE"; fi
}
trap cleanup EXIT

if [ -z "${NODE_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/node}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-counter-node --example counter_regtest ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-counter-node --example counter_regtest ||
      die "the build failed"
  fi
  NODE_BIN="$TARGET/debug/examples/counter_regtest"
fi
[ -x "$NODE_BIN" ] || die "no example binary at $NODE_BIN"

# ---- helpers for the example ------------------------------------------------

# Start the example on a data directory. Sets EX_PID and EX_LOG; its standard
# input is a fifo that stays open on descriptor 3, so that commands can be
# sent.
start_example() { # tag, data dir, [peer]
  EX_LOG="$BASE/example-$1.log"
  : >"$EX_LOG"
  EX_N=0
  rm -f "$FIFO"
  mkfifo "$FIFO"
  exec 3<>"$FIFO"
  # shellcheck disable=SC2086
  "$NODE_BIN" "$2" "${3:-127.0.0.1:$P2P_PORT}" --regtest-single-peer --own-wallet "$OLD" <&3 >"$EX_LOG" 2>&1 &
  EX_PID=$!
}
# Send one command and wait for its `done`. LAST holds what it printed.
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
    if ! kill -0 "$EX_PID" 2>/dev/null; then
      LAST=$(tail -n +$((before + 1)) "$EX_LOG")
      return 1
    fi
    sleep 0.25
  done
  LAST=$(tail -n +$((before + 1)) "$EX_LOG")
  return 1
}
# Wait for a line of the current log that matches the pattern.
await() { # pattern seconds
  local i
  for i in $(seq 1 $(($2 * 4))); do
    grep -Eq -- "$1" "$EX_LOG" && return 0
    if ! kill -0 "$EX_PID" 2>/dev/null; then break; fi
    sleep 0.25
  done
  grep -Eq -- "$1" "$EX_LOG"
}
# Ask for the state of a sale until it is the wanted one.
await_state() { # index state seconds
  local i
  for i in $(seq 1 $(($3 * 2))); do
    cmd "judge $1" || return 1
    if grep -q "^sale $1 state=$2 " <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
# Sets STATE to the state of a sale. Not run in a subshell: `cmd` counts.
STATE=
state_of() { # index
  STATE=
  cmd "judge $1" || return 1
  STATE=$(sed -nE "s/^sale $1 state=([A-Za-z]+) .*/\1/p" <<<"$LAST")
}
# Wait until the node's view of the chain is at the height of Core's.
await_tip() { # seconds
  local want i
  want=$(cli getblockcount)
  for i in $(seq 1 $(($1 * 2))); do
    cmd status || return 1
    if grep -q "^status tip=$want .* peers=1\$" <<<"$LAST"; then return 0; fi
    sleep 0.5
  done
  return 1
}
stop_example() {
  echo quit >&3
  await "^stopped\$" 30 || return 1
  wait "$EX_PID"
  local rc=$?
  EX_PID=
  exec 3>&-
  return $rc
}
# How many times a report line was printed, over every run so far.
reports() { # "PosPaid index=0"
  cat "$BASE"/example-*.log 2>/dev/null | grep -Ec -- "^report $1( |\$)" || true
}
# Bytes of one kind of message that Core received from its newest inbound
# peer, which is the node.
core_received() { # message
  cli getpeerinfo | jq --arg m "$1" \
    '[.[] | select(.inbound == true)] | sort_by(.id) | last | (.bytesrecv_per_msg[$m] // 0)'
}
inbound_peers() { cli getpeerinfo | jq '[.[] | select(.inbound == true)] | length'; }

# ---- Core -----------------------------------------------------------------------

core_start() {
  bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" \
    -listen=1 -bind="127.0.0.1:$P2P_PORT" -listenonion=0 -daemonwait \
    -blockfilterindex=1 -peerblockfilters=1 -txindex=0 \
    -fallbackfee=0.0001 -dbcache=50 >/dev/null || return 1
  NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
}
# Stop Core for good, as a crash of the machine would not: a clean stop
# that keeps its mempool on disk.
core_stop() {
  cli stop >/dev/null 2>&1 || return 1
  local i
  for i in $(seq 1 60); do
    kill -0 "$NODE_PID" 2>/dev/null || break
    sleep 0.5
  done
  ! kill -0 "$NODE_PID" 2>/dev/null
}
core_start || die "bitcoind did not start"

# The starter wallet and the new wallet, as descriptions with their checksums.
OLD='wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx'
NEW='wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt'


cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
for w in old new; do
  cli -named createwallet wallet_name=$w disable_private_keys=true blank=true >/dev/null ||
    die "no watch-only wallet $w"
done
# Core on regtest takes test-network keys: the same keys, with the version
# bytes of a tpub, and the checksum Core works out for the new text.
testnet_desc() { # description
  local bare
  bare=$(python3 - "${1%%#*}" <<'PY'
import hashlib, re, sys
A = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
def dec(s):
    n = 0
    for c in s:
        n = n * 58 + A.index(c)
    return n.to_bytes(82, "big")[-82:]
def enc(b):
    n = int.from_bytes(b, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = A[r] + out
    return out
def tpub(m):
    raw = dec(m.group(0))[-82:]
    body = bytes.fromhex("043587cf") + raw[4:78]
    chk = hashlib.sha256(hashlib.sha256(body).digest()).digest()[:4]
    return enc(body + chk)
print(re.sub(r"xpub[1-9A-HJ-NP-Za-km-z]{100,112}", tpub, sys.argv[1]))
PY
)
  cli getdescriptorinfo "$bare" | jq -r '.descriptor'
}
OLD_T=$(testnet_desc "$OLD")
NEW_T=$(testnet_desc "$NEW")
[ -n "$OLD_T" ] && [ -n "$NEW_T" ] || die "could not make the test-network descriptions"
for pair in "old:$OLD_T" "new:$NEW_T"; do
  res=$(cli -rpcwallet="${pair%:*}" importdescriptors \
    "[{\"desc\":\"${pair#*:}\",\"timestamp\":0,\"active\":true,\"range\":[0,3000]}]")
  [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ] || die "Core did not take a description: $res"
done
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 150 "$MINE" >/dev/null || die "could not mine"
[ "$(cli getblockcount)" = 150 ] || die "the node is not at height 150"
say "A node that serves filters, at height 150"

# Core's own derivation of an index of a description.
derive() { # description index
  local d=$NEW_T
  [ "$1" = "$OLD" ] && d=$OLD_T
  cli deriveaddresses "$d" "[$2,$2]" | jq -r '.[0] | if type == "array" then .[0] else . end'
}
# What the last `opened` line said.
opened_field() { # field
  sed -nE "s/^opened (.* )?$1=([a-z0-9]+).*/\\2/p" <<<"$LAST" | head -1
}
sale_open() { # sets IDX ADDR SATS from a new sale
  cmd "open $SALE_AMOUNT $SALE_RATE" || return 1
  IDX=$(opened_field index)
  ADDR=$(opened_field address)
  SATS=$(sed -nE 's/^opened (.* )?sats=([0-9]+)$/\2/p' <<<"$LAST" | head -1)
  [ -n "$IDX" ] && [ -n "$ADDR" ] && [ -n "$SATS" ]
}
# Whether a wallet of Core holds a coin of a transaction at an address.
holds() { # wallet txid address
  [ "$(cli -rpcwallet="$1" listunspent 0 | jq --arg t "$2" --arg a "$3" \
    '[.[] | select(.txid == $t and .address == $a)] | length')" = 1 ]
}
holds_any() { # wallet txid
  [ "$(cli -rpcwallet="$1" listunspent 0 | jq --arg t "$2" '[.[] | select(.txid == $t)] | length')" != 0 ]
}

# ---- 1 -------------------------------------------------------------------------

say "1. A counter is paired with the offer of a starter wallet"
start_example first "$DATA_DIR"
if await "^started " 60 && await_tip 90; then
  OFFER=$(grep '^offer ' "$EX_LOG" | head -1)
  note "$OFFER"
  if grep -q '^offer version=2 starter=true ' <<<"$OFFER" && [ -f "$DATA_DIR/own-record.bin" ]; then
    pass 1 "the offer is version 2 with the starter flag, the counter paired and started, the tip is reached"
  else
    bad 1 "the offer was not a version 2 offer with the starter flag: $OFFER"
  fi
else
  bad 1 "the node did not start or reach the tip within 90 s"
fi

# ---- 2 -------------------------------------------------------------------------

say "2. A sale at the starter wallet, paid and confirmed"
S2=
if sale_open; then
  note "sale $IDX at $ADDR needs $SATS sats"
  IDX0=$IDX; ADDR0=$ADDR
  TX0=$(cli -rpcwallet=miner sendtoaddress "$ADDR" "$(btc "$SATS")")
  cli generatetoaddress 1 "$MINE" >/dev/null
  # A second sale, left unpaid until after the switch.
  sale_open && IDX1=$IDX && ADDR1=$ADDR && SATS1=$SATS
  if [ -n "$TX0" ] && await_state "$IDX0" Confirmed 60; then
    if [ "$ADDR0" = "$(derive "$OLD" "$IDX0")" ] && [ "$ADDR1" = "$(derive "$OLD" "$IDX1")" ]; then
      pass 2 "sale $IDX0 at $ADDR0 and sale $IDX1 at $ADDR1 are Core's derivation of the starter wallet; sale $IDX0 is Confirmed"
      S2=ok
    else
      bad 2 "an address is not Core's derivation of the starter wallet (got $ADDR0 and $ADDR1)"
    fi
  else
    bad 2 "the first sale did not show Confirmed"
  fi
else
  bad 2 "no sale was opened"
fi

# ---- 3 -------------------------------------------------------------------------

say "3. The owner's switch is taken; the counter shows the new wallet's first address"
cmd "switch-take $NEW 1000 1000"
note "$(grep '^switch ' <<<"$LAST")"
TAKEN=$(grep -c '^switch taken$' <<<"$LAST" || true)
cmd "switch-state"
PENDING=$(grep '^switch ' <<<"$LAST")
note "$PENDING"
NEW0=$(derive "$NEW" 0)
# A sale opened meanwhile still pays the starter wallet.
sale_open
DURING_IDX=$IDX; DURING_ADDR=$ADDR
note "a sale opened meanwhile: $DURING_IDX at $DURING_ADDR"
SHOWN_NEW0=$(sed -nE 's/^switch pending address=([a-z0-9]+) .*/\1/p' <<<"$PENDING")
if [ "$TAKEN" = 1 ] && grep -q ' kind=TwoOfThree$' <<<"$PENDING" && [ "$DURING_ADDR" = "$(derive "$OLD" "$DURING_IDX")" ]; then
  # The counter prints the address of the pinned wallet's network (mainnet):
  # it has the same witness program as Core's regtest address of the index.
  if [ "$(cli -rpcwallet=miner getaddressinfo "$NEW0" | jq -r '.witness_program')" = "$(python3 - "$SHOWN_NEW0" <<'PY'
import sys
CH="qpzry9x8gf2tvdw0s3jn54khce6mua7l"
a=sys.argv[1]
d=[CH.index(c) for c in a[a.rfind('1')+1:]][:-6]
bits=0;acc=0;out=[]
for v in d[1:]:
    acc=(acc<<5)|v;bits+=5
    while bits>=8:
        bits-=8;out.append((acc>>bits)&0xff)
print(bytes(out).hex())
PY
)" ]; then
    pass 3 "the switch was taken; the counter shows the new wallet's first address (Core's index 0 of the new description, 2-of-3); a sale meanwhile paid the starter wallet"
  else
    bad 3 "the address the counter shows is not Core's index 0 of the new description ($SHOWN_NEW0 against $NEW0)"
  fi
else
  bad 3 "the switch was not taken as expected: taken=$TAKEN pending=$PENDING"
fi

# ---- 4 -------------------------------------------------------------------------

say "4. Same address: the next sale pays the new wallet"
cmd "switch-same"
SAME=$(grep '^switch ' <<<"$LAST")
note "$SAME"
cmd "switch-kept"
KEPT=$(grep '^switch ' <<<"$LAST")
note "$KEPT"
if sale_open; then
  note "next sale $IDX at $ADDR"
  IDXN=$IDX; ADDRN=$ADDR
  if grep -q '^switch same record_bytes=' <<<"$SAME" && grep -q '^switch kept$' <<<"$KEPT" &&
    [ "$IDXN" -ge 1000 ] && [ "$ADDRN" = "$(derive "$NEW" "$IDXN")" ]; then
    TXN=$(cli -rpcwallet=miner sendtoaddress "$ADDRN" "$(btc "$SATS")")
    cli generatetoaddress 1 "$MINE" >/dev/null
    if await_state "$IDXN" Confirmed 60; then
      pass 4 "sale $IDXN at $ADDRN is Core's derivation of the new description at an index above every old one; it is paid and Confirmed"
    else
      bad 4 "the payment to the new wallet did not show Confirmed"
    fi
  else
    bad 4 "the sale after the switch is not at the new wallet (index $IDXN, address $ADDRN, new would be $(derive "$NEW" "$IDXN"))"
  fi
else
  bad 4 "no sale was opened after the switch"
fi

# ---- 5 -------------------------------------------------------------------------

say "5. Core sees both payments at the right wallets"
if [ -n "${TX0:-}" ] && [ -n "${TXN:-}" ] &&
  holds old "$TX0" "$ADDR0" && ! holds_any new "$TX0" &&
  holds new "$TXN" "$ADDRN" && ! holds_any old "$TXN"; then
  pass 5 "$TX0 is a coin of the starter wallet only; $TXN is a coin of the new wallet only"
else
  bad 5 "the coins are not where they should be"
fi

# ---- 6 -------------------------------------------------------------------------

say "6. An address of the starter wallet handed out before the switch is still watched"
if [ -n "${IDX1:-}" ]; then
  TX1=$(cli -rpcwallet=miner sendtoaddress "$ADDR1" "$(btc "$SATS1")")
  cli generatetoaddress 1 "$MINE" >/dev/null
  if [ -n "$TX1" ] && await_state "$IDX1" Confirmed 60 && holds old "$TX1" "$ADDR1"; then
    pass 6 "sale $IDX1 (opened before the switch, at the starter wallet) was paid after it and is Confirmed"
  else
    bad 6 "the payment to the old address was not seen"
  fi
else
  bad 6 "there was no sale left open"
fi

# ---- 7 -------------------------------------------------------------------------

say "7. After a restart the counter pays the new wallet and takes no second switch"
stop_example || note "the first run did not stop cleanly"
start_example second "$DATA_DIR"
if await "^started " 60 && await_tip 90 && sale_open; then
  note "sale $IDX at $ADDR"
  IDXR=$IDX; ADDRR=$ADDR
  cmd "switch-take $OLD 5000 1000"
  SECOND=$(grep '^switch ' <<<"$LAST")
  note "$SECOND"
  state_of "$IDXN"
  if [ "$IDXR" -gt "$IDXN" ] && [ "$ADDRR" = "$(derive "$NEW" "$IDXR")" ] &&
    grep -q '^switch refused .*CR-263' <<<"$SECOND" && [ "$STATE" = Confirmed ]; then
    pass 7 "the restart read the record the app kept: sale $IDXR is at the new wallet, sale $IDXN is still Confirmed, and a second switch is refused (CR-263)"
  else
    bad 7 "after the restart: index $IDXR at $ADDRR, second switch: $SECOND, state $STATE"
  fi
else
  bad 7 "the counter did not start again or open a sale"
fi
stop_example || note "the second run did not stop cleanly"

# ---- 8 -------------------------------------------------------------------------

say "8. Different: nothing is switched and the next sale pays the starter wallet"
mkdir -p "$OTHER_DIR"
start_example other "$OTHER_DIR"
if await "^started " 60 && await_tip 90; then
  cmd "switch-take $NEW 1000 1000"
  cmd "switch-different"
  DIFF=$(grep '^switch ' <<<"$LAST")
  note "$DIFF"
  cmd "switch-state"
  GONE=$(grep '^switch ' <<<"$LAST")
  if sale_open && [ "$ADDR" = "$(derive "$OLD" "$IDX")" ] && grep -q '^switch different$' <<<"$DIFF" &&
    grep -q '^switch none$' <<<"$GONE"; then
    pass 8 "Different dropped the switch; sale $IDX is at $ADDR, Core's derivation of the starter wallet"
  else
    bad 8 "after Different: $DIFF, $GONE, sale $IDX at $ADDR"
  fi
else
  bad 8 "the second counter did not start"
fi
stop_example || note "the second counter did not stop cleanly"

# ---- result -------------------------------------------------------------------

say "Result"
failed=0
for n in 1 2 3 4 5 6 7 8; do
  printf '%s  step %s\n' "${RESULT[$n]:-FAIL}" "$n"
  [ "${RESULT[$n]:-FAIL}" = PASS ] || failed=1
done
if [ "$failed" = 0 ]; then
  echo "ALL PASS (the owner phone and the app are played by the example; the real ones are checked in the crates' tests)"
else
  echo "SOME STEPS FAILED"
  exit 1
fi
