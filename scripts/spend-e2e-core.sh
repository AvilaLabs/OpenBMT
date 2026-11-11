#!/usr/bin/env bash
# The spend path against Bitcoin Core, on regtest.
#
# Question: does a transaction that `openbmt-spend` plans, the signer signs,
# `combine` joins and `finalize` finishes get accepted by Bitcoin Core, and do
# Core and the library agree about what the transaction is?
#
# What it does (the steps are numbered as in the printed table):
#   1. Starts a private regtest node, imports the 2-of-3 wallet description
#      into a blank watch-only Core wallet, and funds five receive addresses
#      with different amounts, one block after each, so that the coins have
#      different depths. Core's own derivation of the addresses is compared
#      with the library's.
#   2. The example reads the coins (Core's `listunspent` and
#      `getrawtransaction`), plans a refund of a fixed amount to a Core
#      address at a given rate and with the chain tip's height as the lock
#      time, and prints the plan: oldest coins first, the change at the given
#      index.
#   3. Core's `decodepsbt` and `analyzepsbt` agree with the plan: the fee,
#      the inputs, the outputs, and the next role is "signer".
#   4. Two signers, the real `openbmt-signer` as key 1 and as key 2, in
#      separate runs of the example, each sign. `combine` joins them and
#      `finalize` finishes the transaction.
#   5. Core's `testmempoolaccept` allows it with the lock time of the tip's
#      height, its vsize and fee give a rate at
#      or above the asked rate, `sendrawtransaction` takes it, a block
#      confirms it, the refund address has the coin, and the watch-only
#      wallet shows the change at the expected change index.
#   6. A second refund, signed by Core as one of the two signers (a Core
#      wallet that holds key 3's private descriptor, `walletprocesspsbt`) and
#      by the signer as key 1: mixed signers join, finish and confirm. The
#      change is placed first among the outputs by a seed, and Core sees it
#      there. Core's `combinepsbt` and `finalizepsbt` on the signer's two
#      files (key 1 and key 2) give the same transaction bytes as `finalize`.
#   7. `send_all`: everything left goes to one Core address with no change
#      and the watch-only wallet is empty after a block.
#   8. Negative: two fresh coins are planned; a transaction in which one of
#      the two signatures on input 0 was made for input 1 is refused by
#      `finalize`, and when it is forced out by hand in the example, Core's
#      `testmempoolaccept` refuses it too.
#   9. A lock height above the tip: a transaction planned, signed, combined
#      and finished with a lock time ten blocks ahead is refused by Core as
#      non-final, which is the expected result and counts as a pass. After ten
#      more blocks Core takes the same transaction.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another; the bitcoind in
# /usr/local/bin is never used), jq, base64, and cargo unless SPEND_BIN names a
# prebuilt `spend_regtest` example binary. A build goes through
# ~/.cache/openbmt/guard.sh when that file exists.
#
# Environment:
#   CORE_BIN        directory with bitcoind and bitcoin-cli
#   DRILL_DIR       scratch directory (default: a new directory under
#                   ~/.cache/openbmt)
#   DRILL_RPC_PORT  RPC port for the regtest node (default 18974)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#   SPEND_BIN       the `spend_regtest` example binary
#
# Everything here is regtest on 127.0.0.1. The keys come from fixed test
# bytes: TEST ONLY, NEVER USE. No real keys or funds are involved. The node
# and the scratch directory are removed on every exit. The default Bitcoin
# data directory is never touched.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_RPC_PORT:-18974}
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/spend-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/core"
WORK="$BASE/work"
RATE=2500 # satoshis per 1000 virtual bytes

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
cli() { bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" "$@"; }
btc() { printf '%d.%08d' $(($1 / 100000000)) $(($1 % 100000000)); }
sats() { jq -rn --arg v "$1" '$v | tonumber * 100000000 | round'; }

declare -a RESULT MESSAGE
declare -a BAD
pass() { RESULT[$1]=PASS; MESSAGE[$1]=$2; printf 'PASS  step %s: %s\n' "$1" "$2"; }
bad() { RESULT[$1]=FAIL; MESSAGE[$1]=$2; printf 'FAIL  step %s: %s\n' "$1" "$2"; }
# chk STEP "what" command...: records a failed check for the step.
chk() {
  local n=$1 what=$2
  shift 2
  if "$@"; then note "ok:     $what"; else note "NOT ok: $what"; BAD[$n]=1; fi
}
# done_step STEP "what passed" : PASS if no check of the step failed.
done_step() {
  if [ -z "${BAD[$1]:-}" ]; then pass "$1" "$2"; else bad "$1" "a check failed: $2"; fi
}

for tool in bitcoind bitcoin-cli jq base64; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH"
done
[ "$(command -v bitcoind)" = "$CORE_BIN/bitcoind" ] || die "bitcoind is not the one in $CORE_BIN"
[ "$(command -v bitcoin-cli)" = "$CORE_BIN/bitcoin-cli" ] || die "bitcoin-cli is not the one in $CORE_BIN"
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) die "this check is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

mkdir -p "$NODE_DIR" "$WORK"
NODE_PID=

cleanup() {
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

if [ -z "${SPEND_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/spend}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-spend --example spend_regtest ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-spend --example spend_regtest ||
      die "the build failed"
  fi
  SPEND_BIN="$TARGET/debug/examples/spend_regtest"
fi
[ -x "$SPEND_BIN" ] || die "no example binary at $SPEND_BIN"
ex() { "$SPEND_BIN" "$@"; }

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" -listen=0 -daemonwait \
  -fallbackfee=0.0001 -txindex=1 -dbcache=50 >/dev/null || die "bitcoind did not start"
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
[ -n "$NODE_PID" ] || die "no node pid"

b64() { base64 -w0 "$1"; }
unb64() { base64 -d <<<"$1" >"$2"; }
# The balance of an output in satoshis: value of the first output paying $2
# in the decoded transaction $1.
tx_paying() { jq -r --arg a "$2" '[.vout[] | select(.scriptPubKey.address == $a)] | first | .value * 100000000 | round' <<<"$1"; }

# ---- 1 -------------------------------------------------------------------------

say "1. The wallet in a watch-only Core wallet, five funded coins"
KEYS="$WORK/keys.txt"
ex keys 0 1 2 3 4 5 6 >"$KEYS" || die "the example could not make the keys"
DESC=$(sed -n 's/^descriptor //p' "$KEYS")
RECV_DESC=$(sed -n 's/^receive-descriptor //p' "$KEYS")
CHNG_DESC=$(sed -n 's/^change-descriptor //p' "$KEYS")
[ -n "$DESC" ] && [ -n "$RECV_DESC" ] && [ -n "$CHNG_DESC" ] || die "no descriptor from the example"
note "the wallet: ${DESC:0:60}..."

cli -named createwallet wallet_name=shop disable_private_keys=true blank=true >/dev/null || die "no shop wallet"
res=$(cli -rpcwallet=shop importdescriptors "[{\"desc\":\"$DESC\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,99]}]")
chk 1 "Core imported the multipath description" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]

# Core derives the addresses of the receive and change chains itself.
core_recv=$(cli deriveaddresses "$RECV_DESC" "[0,6]" | jq -r '.[]')
ours_recv=$(sed -n 's/^receive [0-9]* //p' "$KEYS")
chk 1 "Core and the library derive the same seven receive addresses" [ "$core_recv" = "$ours_recv" ]
core_chng=$(cli deriveaddresses "$CHNG_DESC" "[0,6]" | jq -r '.[]')
ours_chng=$(sed -n 's/^change [0-9]* //p' "$KEYS")
chk 1 "Core and the library derive the same seven change addresses" [ "$core_chng" = "$ours_chng" ]
mapfile -t RADDR <<<"$ours_recv"
mapfile -t CADDR <<<"$ours_chng"

# Core wallet holding key 3: its private receive and change descriptors.
cli -named createwallet wallet_name=core3 blank=true >/dev/null || die "no core3 wallet"
priv_import() { # key number
  local r c
  r=$(sed -n "s/^private-receive $1 //p" "$KEYS")
  c=$(sed -n "s/^private-change $1 //p" "$KEYS")
  r="$r#$(cli getdescriptorinfo "$r" | jq -r '.checksum')"
  c="$c#$(cli getdescriptorinfo "$c" | jq -r '.checksum')"
  cli -rpcwallet="core$1" importdescriptors "[{\"desc\":\"$r\",\"timestamp\":\"now\",\"active\":false,\"range\":[0,99]},{\"desc\":\"$c\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,99]}]"
}
res=$(priv_import 3)
chk 1 "the Core wallet for key 3 holds its private descriptors" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]

cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 101 "$MINE" >/dev/null || die "no blocks"
AMOUNTS=(40000 55000 70000 85000 100000)
for i in 0 1 2 3 4; do
  cli -rpcwallet=miner sendtoaddress "${RADDR[$i]}" "$(btc "${AMOUNTS[$i]}")" >/dev/null || die "funding $i failed"
  cli generatetoaddress 1 "$MINE" >/dev/null
done
have=$(cli -rpcwallet=shop listunspent 1 | jq -c '[.[] | .amount * 100000000 | round] | sort')
chk 1 "the watch-only wallet holds the five coins ($have)" [ "$have" = "[40000,55000,70000,85000,100000]" ]
done_step 1 "the wallet is in Core, five coins of 40000..100000 sat at depths 5..1, derivations agree"

# The coins, as the example reads them: one line each.
make_coins() { # file
  : >"$1"
  cli -rpcwallet=shop listunspent 1 |
    jq -r '.[] | .desc as $d | ($d | capture("/(?<c>[01])/(?<i>[0-9]+)\\]")) as $m
           | "\(.txid) \(.vout) \(.confirmations) \($m.c) \($m.i)"' |
    while read -r txid vout conf chain idx; do
      echo "$txid $vout $conf $chain $idx $(cli getrawtransaction "$txid")" >>"$1"
    done
}
now() { date +%s; }

# ---- 2 -------------------------------------------------------------------------

say "2. Plan a refund of 120,000 sat at $RATE sat per 1000 vB, locked to the tip"
DEST=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins1.txt"
TIP=$(cli getblockcount)
PLAN1_ARGS=(--coins "$WORK/coins1.txt" --rate "$RATE" --change-index 0 --min-conf 1 --lock-height "$TIP" --out "$WORK/base1.psbt")
ex plan "${PLAN1_ARGS[@]}" "$DEST:120000" >"$WORK/plan1.txt"
code=$?
sed 's/^/      /' "$WORK/plan1.txt"
chk 2 "the example planned it" [ "$code" = 0 ]
P_FEE=$(sed -n 's/^fee //p' "$WORK/plan1.txt")
P_VSIZE=$(sed -n 's/^vsize //p' "$WORK/plan1.txt")
P_TXID=$(sed -n 's/^txid //p' "$WORK/plan1.txt")
P_CHANGE=$(sed -n 's/^change 0 \([a-z0-9]*\) \([0-9]*\)$/\1 \2/p' "$WORK/plan1.txt")
chk 2 "three inputs, oldest first (depths 5, 4, 3; 40000, 55000, 70000)" \
  [ "$(sed -n 's/^input [0-9a-f]*:[0-9]* \([0-9]*\) \([0-9]*\) .*/\1@\2/p' "$WORK/plan1.txt" | tr '\n' ' ')" = "40000@5 55000@4 70000@3 " ]
chk 2 "the change goes to change index 0, which is Core's change address 0" [ "${P_CHANGE%% *}" = "${CADDR[0]}" ]
chk 2 "the plan's lock time is the tip's height ($TIP)" [ "$(sed -n 's/^lock-height //p' "$WORK/plan1.txt")" = "$TIP" ]
chk 2 "the fee is ceil(vsize * rate / 1000)" [ "$P_FEE" = "$(((P_VSIZE * RATE + 999) / 1000))" ]
chk 2 "the change is the coins minus 120000 minus the fee" [ "${P_CHANGE##* }" = "$((165000 - 120000 - P_FEE))" ]
done_step 2 "the example read Core's coins and planned: oldest first, change at index 0, fee $P_FEE sat over $P_VSIZE vB"

# ---- 3 -------------------------------------------------------------------------

say "3. Core reads the plan's file"
B1=$(b64 "$WORK/base1.psbt")
DEC=$(cli decodepsbt "$B1")
ANA=$(cli analyzepsbt "$B1")
chk 3 "Core's fee equals the plan's fee ($P_FEE)" [ "$(jq -r '.fee * 100000000 | round' <<<"$DEC")" = "$P_FEE" ]
chk 3 "Core sees three inputs" [ "$(jq -r '.tx.vin | length' <<<"$DEC")" = 3 ]
chk 3 "Core sees two outputs: the refund and the change" [ "$(jq -r '.tx.vout | length' <<<"$DEC")" = 2 ]
chk 3 "the refund output is 120000 sat to the refund address" [ "$(tx_paying "$(jq -c '.tx' <<<"$DEC")" "$DEST")" = 120000 ]
chk 3 "the change output is the plan's change" [ "$(tx_paying "$(jq -c '.tx' <<<"$DEC")" "${P_CHANGE%% *}")" = "${P_CHANGE##* }" ]
chk 3 "Core reads the file's lock time as the tip's height ($TIP)" [ "$(jq -r '.tx.locktime' <<<"$DEC")" = "$TIP" ]
chk 3 "Core's txid of the file equals the plan's" [ "$(jq -r '.tx.txid' <<<"$DEC")" = "$P_TXID" ]
chk 3 "analyzepsbt: the next role is signer" [ "$(jq -r '.next' <<<"$ANA")" = signer ]
chk 3 "analyzepsbt: every input has its utxo and none is final" \
  [ "$(jq -r '[.inputs[] | select(.has_utxo and (.is_final | not))] | length' <<<"$ANA")" = 3 ]
chk 3 "Core reads the three key-path hints on every input" \
  [ "$(jq -r '[.inputs[] | (.bip32_derivs | length)] | unique | tostring' <<<"$DEC")" = "[3]" ]
note "Core estimates $(jq -r '.estimated_vsize' <<<"$ANA") vB at $(jq -r '.estimated_feerate' <<<"$ANA") BTC/kvB; the plan says $P_VSIZE vB (not compared: Core's estimate of a multisig input is its own)"
done_step 3 "decodepsbt and analyzepsbt agree on fee, inputs, outputs and txid; next role: signer"

# ---- 4 -------------------------------------------------------------------------

say "4. Two signers, combine, finalize"
ex sign --who 1 --state "$WORK/key1.state" --now "$(now)" --in "$WORK/base1.psbt" --out "$WORK/s1_1.psbt" >"$WORK/sign1.txt"
code1=$?
ex sign --who 2 --state "$WORK/key2.state" --now "$(now)" --in "$WORK/base1.psbt" --out "$WORK/s2_1.psbt" >"$WORK/sign2.txt"
code2=$?
sed 's/^/      key1: /' "$WORK/sign1.txt"
sed 's/^/      key2: /' "$WORK/sign2.txt"
chk 4 "the real signer signed as key 1 with no refusal" [ "$code1" = 0 ]
chk 4 "the real signer signed as key 2 with no refusal" [ "$code2" = 0 ]
chk 4 "the signer's fee is the plan's fee" [ "$(sed -n 's/^fee //p' "$WORK/sign1.txt")" = "$P_FEE" ]
chk 4 "the signer's size is the plan's size" [ "$(sed -n 's/^vsize //p' "$WORK/sign1.txt")" = "$P_VSIZE" ]
chk 4 "the signer shows the change as change at index 0" [ "$(grep -c 'Change { index: 0 }' "$WORK/sign1.txt")" = 1 ]
ex plan "${PLAN1_ARGS[@]}" --signed "$WORK/s1_1.psbt" --signed "$WORK/s2_1.psbt" \
  --combined-out "$WORK/c1.psbt" --hex-out "$WORK/tx1.hex" "$DEST:120000" >"$WORK/fin1.txt"
code=$?
sed -n '/^signatures/,$p' "$WORK/fin1.txt" | sed 's/^/      /'
chk 4 "combine and finalize succeeded" [ "$code" = 0 ]
chk 4 "two signatures on each of the three inputs" [ "$(sed -n 's/^signatures //p' "$WORK/fin1.txt")" = "2,2,2" ]
chk 4 "the finished transaction has the planned transaction ID" [ "$(sed -n 's/^final-txid //p' "$WORK/fin1.txt")" = "$P_TXID" ]
done_step 4 "the signer signed twice (keys 1 and 2, separate runs); combine and finalize gave $P_TXID"

# ---- 5 -------------------------------------------------------------------------

say "5. Core takes it"
HEX1=$(cat "$WORK/tx1.hex" 2>/dev/null)
TMA=$(cli testmempoolaccept "[\"$HEX1\"]")
note "testmempoolaccept: $(jq -c '.[0] | {allowed, vsize, fees}' <<<"$TMA")"
CVS=$(jq -r '.[0].vsize' <<<"$TMA")
CFEE=$(jq -r '.[0].fees.base * 100000000 | round' <<<"$TMA")
chk 5 "testmempoolaccept allows it" [ "$(jq -r '.[0].allowed' <<<"$TMA")" = true ]
chk 5 "the transaction carries the lock time of the tip's height ($TIP), and Core takes it" \
  [ "$(cli decoderawtransaction "$HEX1" | jq -r '.locktime')" = "$TIP" ]
chk 5 "Core's fee is the plan's fee" [ "$CFEE" = "$P_FEE" ]
chk 5 "the rate Core computes ($CFEE sat over $CVS vB) is at least the asked $RATE per 1000 vB" [ $((CFEE * 1000)) -ge $((RATE * CVS)) ]
chk 5 "Core's vsize is at most the plan's (the plan counts signatures at their longest)" [ "$CVS" -le "$P_VSIZE" ]
chk 5 "the real size is no more than 2 vB per input below the plan's" [ $((P_VSIZE - CVS)) -le 7 ]
SENT=$(cli sendrawtransaction "$HEX1")
chk 5 "sendrawtransaction took it and the txid is the plan's" [ "$SENT" = "$P_TXID" ]
cli generatetoaddress 1 "$MINE" >/dev/null
got=$(cli -rpcwallet=miner listunspent 1 9999999 "[\"$DEST\"]" | jq -r '[.[].amount * 100000000 | round] | tostring')
chk 5 "the refund address holds the coin of 120000 sat" [ "$got" = "[120000]" ]
ch=$(cli -rpcwallet=shop listunspent 1 | jq -c --arg t "$P_TXID" \
  '[.[] | select(.txid == $t)] | map({sats: (.amount * 100000000 | round), address, desc: (.desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") )})')
note "the watch-only wallet's coins from it: $ch"
chk 5 "the watch-only wallet shows the change at change index 0 with the planned amount" \
  [ "$(jq -r --arg a "${P_CHANGE%% *}" --argjson s "${P_CHANGE##* }" 'length == 1 and .[0].address == $a and .[0].sats == $s and .[0].desc.c == "1" and .[0].desc.i == "0"' <<<"$ch")" = true ]
done_step 5 "Core allowed, relayed and confirmed it at $CFEE sat over $CVS vB; the refund and the change are where they should be"

# ---- 6 -------------------------------------------------------------------------

say "6. Mixed signers, and Core's own combine and finalize"
DEST2=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins2.txt"
TIP2=$(cli getblockcount)
# The change goes at a place taken from a seed and the inputs, and the inputs
# are Core's coins, which differ on every run. Seeds are tried until one puts
# the change first among the outputs.
for n in $(seq 0 63); do
  SEED2=$(printf '%064x' "$n")
  PLAN2_ARGS=(--coins "$WORK/coins2.txt" --rate 3000 --change-index 1 --min-conf 1 --lock-height "$TIP2" --change-seed "$SEED2" --out "$WORK/base2.psbt")
  ex plan "${PLAN2_ARGS[@]}" "$DEST2:150000" >"$WORK/plan2.txt"
  code=$?
  [ "$code" = 0 ] && [ "$(sed -n 's/^change-position //p' "$WORK/plan2.txt")" = 0 ] && break
done
chk 6 "the second plan was made" [ "$code" = 0 ]
sed 's/^/      /' "$WORK/plan2.txt"
P2_TXID=$(sed -n 's/^txid //p' "$WORK/plan2.txt")
P2_FEE=$(sed -n 's/^fee //p' "$WORK/plan2.txt")
chk 6 "it spends two coins (the two oldest: 85000 and 100000)" [ "$(grep -c '^input ' "$WORK/plan2.txt")" = 2 ]
P2_AT=$(sed -n 's/^change-position //p' "$WORK/plan2.txt")
chk 6 "the seed puts the change first among the outputs (place $P2_AT)" [ "$P2_AT" = 0 ]
chk 6 "Core reads the change at that place, to change address 1" \
  [ "$(cli decodepsbt "$(b64 "$WORK/base2.psbt")" | jq -r --argjson n "${P2_AT:-9}" '.tx.vout[$n].scriptPubKey.address')" = "${CADDR[1]}" ]
B2=$(b64 "$WORK/base2.psbt")
# Core signs as key 3.
C3=$(cli -rpcwallet=core3 walletprocesspsbt "$B2" true ALL true false | jq -r '.psbt')
unb64 "$C3" "$WORK/core3_2.psbt"
chk 6 "Core's key-3 wallet added one signature to each input" \
  [ "$(cli decodepsbt "$C3" | jq -c '[.inputs[] | (.partial_signatures // {} | length)]')" = "[1,1]" ]
# The signer signs as key 1 and key 2.
ex sign --who 1 --state "$WORK/key1.state" --now "$(now)" --in "$WORK/base2.psbt" --out "$WORK/s1_2.psbt" >"$WORK/sign1b.txt"
chk 6 "the signer signed as key 1" [ "$?" = 0 ]
ex sign --who 2 --state "$WORK/key2.state" --now "$(now)" --in "$WORK/base2.psbt" --out "$WORK/s2_2.psbt" >"$WORK/sign2b.txt"
chk 6 "the signer signed as key 2" [ "$?" = 0 ]
# OpenBMT finalize on keys 1 and 2, and Core's combine and finalize on the same.
ex plan "${PLAN2_ARGS[@]}" --signed "$WORK/s1_2.psbt" --signed "$WORK/s2_2.psbt" \
  --hex-out "$WORK/tx2_ours.hex" "$DEST2:150000" >"$WORK/fin2a.txt"
chk 6 "keys 1 and 2: combine and finalize succeeded" [ "$?" = 0 ]
S1B=$(b64 "$WORK/s1_2.psbt")
S2B=$(b64 "$WORK/s2_2.psbt")
CORE_MERGED=$(cli combinepsbt "[\"$S1B\",\"$S2B\"]")
CORE_FIN=$(cli finalizepsbt "$CORE_MERGED")
chk 6 "Core's finalizepsbt completes the two signed files" [ "$(jq -r '.complete' <<<"$CORE_FIN")" = true ]
if [ "$(jq -r '.hex' <<<"$CORE_FIN")" = "$(cat "$WORK/tx2_ours.hex")" ]; then
  note "Core's combinepsbt + finalizepsbt and our finalize give the same transaction bytes (keys 1 and 2)"
  EQUAL=1
else
  note "the transaction bytes DIFFER between Core's finalize and ours (keys 1 and 2):"
  note "core: $(jq -r '.hex' <<<"$CORE_FIN")"
  note "ours: $(cat "$WORK/tx2_ours.hex")"
  EQUAL=
fi
chk 6 "byte-equal transactions from Core's finalize and ours (keys 1 and 2)" [ -n "$EQUAL" ]
# Mixed: Core's key 3 and the signer's key 1.
ex plan "${PLAN2_ARGS[@]}" --signed "$WORK/core3_2.psbt" --signed "$WORK/s1_2.psbt" \
  --hex-out "$WORK/tx2_mixed.hex" "$DEST2:150000" >"$WORK/fin2b.txt"
chk 6 "Core's signature and the signer's: combine and finalize succeeded" [ "$?" = 0 ]
sed 's/^/      mixed: /' "$WORK/fin2b.txt"
MIXED_CORE=$(cli finalizepsbt "$(cli combinepsbt "[\"$C3\",\"$S1B\"]")" | jq -r '.hex')
chk 6 "the mixed transaction equals Core's own finalize of the same two files" [ "$MIXED_CORE" = "$(cat "$WORK/tx2_mixed.hex")" ]
HEX2=$(cat "$WORK/tx2_mixed.hex" 2>/dev/null)
TMA2=$(cli testmempoolaccept "[\"$HEX2\"]")
chk 6 "testmempoolaccept allows the mixed transaction" [ "$(jq -r '.[0].allowed' <<<"$TMA2")" = true ]
chk 6 "testmempoolaccept allows the keys 1 and 2 transaction too" \
  [ "$(cli testmempoolaccept "[\"$(cat "$WORK/tx2_ours.hex")\"]" | jq -r '.[0].allowed')" = true ]
chk 6 "the rate is at least the asked 3000 per 1000 vB" \
  [ $(($(jq -r '.[0].fees.base * 100000000 | round' <<<"$TMA2") * 1000)) -ge $((3000 * $(jq -r '.[0].vsize' <<<"$TMA2"))) ]
chk 6 "sendrawtransaction took it" [ "$(cli sendrawtransaction "$HEX2")" = "$P2_TXID" ]
cli generatetoaddress 1 "$MINE" >/dev/null
got=$(cli -rpcwallet=miner listunspent 1 9999999 "[\"$DEST2\"]" | jq -r '[.[].amount * 100000000 | round] | tostring')
chk 6 "the second refund address holds 150000 sat" [ "$got" = "[150000]" ]
chk 6 "the change is at change index 1" \
  [ "$(cli -rpcwallet=shop listunspent 1 | jq -r --arg t "$P2_TXID" '[.[] | select(.txid == $t) | .desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)"] | tostring')" = '["1/1"]' ]
if [ -n "$EQUAL" ]; then SAME="the same bytes"; else SAME="different bytes"; fi
done_step 6 "Core's key 3 and the signer's key 1 joined and confirmed; Core's combine and finalize on keys 1 and 2 gave $SAME as ours"

# ---- 7 -------------------------------------------------------------------------

say "7. send_all"
DEST3=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins3.txt"
PLAN3_ARGS=(--coins "$WORK/coins3.txt" --rate 2000 --change-index 2 --min-conf 1 --out "$WORK/base3.psbt" --send-all)
ex plan "${PLAN3_ARGS[@]}" "$DEST3:0" >"$WORK/plan3.txt"
chk 7 "the send_all plan was made" [ "$?" = 0 ]
sed 's/^/      /' "$WORK/plan3.txt"
P3_TXID=$(sed -n 's/^txid //p' "$WORK/plan3.txt")
P3_FEE=$(sed -n 's/^fee //p' "$WORK/plan3.txt")
P3_IN=$(sed -n 's/^total-in //p' "$WORK/plan3.txt")
chk 7 "no change output" [ "$(grep -c '^change ' "$WORK/plan3.txt")" = 0 ]
chk 7 "every coin of the wallet is spent" [ "$(grep -c '^input ' "$WORK/plan3.txt")" = "$(cli -rpcwallet=shop listunspent 1 | jq 'length')" ]
ex sign --who 1 --state "$WORK/key1.state" --now "$(now)" --in "$WORK/base3.psbt" --out "$WORK/s1_3.psbt" >"$WORK/sign1c.txt"
chk 7 "the signer signed as key 1" [ "$?" = 0 ]
ex sign --who 2 --state "$WORK/key2.state" --now "$(now)" --in "$WORK/base3.psbt" --out "$WORK/s2_3.psbt" >"$WORK/sign2c.txt"
chk 7 "the signer signed as key 2" [ "$?" = 0 ]
ex plan "${PLAN3_ARGS[@]}" --signed "$WORK/s1_3.psbt" --signed "$WORK/s2_3.psbt" \
  --hex-out "$WORK/tx3.hex" "$DEST3:0" >"$WORK/fin3.txt"
chk 7 "combine and finalize succeeded" [ "$?" = 0 ]
HEX3=$(cat "$WORK/tx3.hex" 2>/dev/null)
TMA3=$(cli testmempoolaccept "[\"$HEX3\"]")
chk 7 "testmempoolaccept allows it" [ "$(jq -r '.[0].allowed' <<<"$TMA3")" = true ]
chk 7 "the rate is at least the asked 2000 per 1000 vB" \
  [ $(($(jq -r '.[0].fees.base * 100000000 | round' <<<"$TMA3") * 1000)) -ge $((2000 * $(jq -r '.[0].vsize' <<<"$TMA3"))) ]
chk 7 "sendrawtransaction took it" [ "$(cli sendrawtransaction "$HEX3")" = "$P3_TXID" ]
cli generatetoaddress 1 "$MINE" >/dev/null
got=$(cli -rpcwallet=miner listunspent 1 9999999 "[\"$DEST3\"]" | jq -r '[.[].amount * 100000000 | round] | tostring')
chk 7 "the address holds everything: $((P3_IN - P3_FEE)) sat" [ "$got" = "[$((P3_IN - P3_FEE))]" ]
chk 7 "the watch-only wallet has no coin left" [ "$(cli -rpcwallet=shop listunspent 0 | jq 'length')" = 0 ]
chk 7 "and its balance is 0" [ "$(cli -rpcwallet=shop getbalances | jq -r '.mine | (.trusted + .untrusted_pending + .immature) * 100000000 | round')" = 0 ]
done_step 7 "everything went to one address in one transaction with no change; the watch-only wallet is empty"

# ---- 8 -------------------------------------------------------------------------

say "8. A signature made for another input"
for i in 5 6; do
  cli -rpcwallet=miner sendtoaddress "${RADDR[$i]}" 0.0006 >/dev/null || die "funding $i failed"
done
cli generatetoaddress 1 "$MINE" >/dev/null
DEST4=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins4.txt"
ex plan --coins "$WORK/coins4.txt" --rate 2000 --change-index 3 --min-conf 1 --out "$WORK/base4.psbt" "$DEST4:90000" >"$WORK/plan4.txt"
chk 8 "a plan over two fresh coins was made" [ "$?" = 0 ]
chk 8 "it has two inputs" [ "$(grep -c '^input ' "$WORK/plan4.txt")" = 2 ]
ex forge --psbt "$WORK/base4.psbt" --hex-out "$WORK/forced.hex" >"$WORK/forge.txt"
code=$?
sed 's/^/      /' "$WORK/forge.txt"
forge_refused() { [ "$code" = 0 ] && grep -q '^refused ' "$WORK/forge.txt"; }
chk 8 "the library's finalize refused the file in which input 0 carries a signature made for input 1" forge_refused
FORCED=$(cat "$WORK/forced.hex" 2>/dev/null)
chk 8 "a forced transaction was written" [ -n "$FORCED" ]
TMAF=$(cli testmempoolaccept "[\"$FORCED\"]")
note "Core on the forced transaction: $(jq -c '.[0] | {allowed, "reject-reason"}' <<<"$TMAF")"
chk 8 "Core's testmempoolaccept refuses it" [ "$(jq -r '.[0].allowed' <<<"$TMAF")" = false ]
chk 8 "for a script failure, not for some other reason" \
  [ "$(jq -r '.[0]["reject-reason"] // ""' <<<"$TMAF" | grep -c 'script-verify-flag')" = 1 ]
done_step 8 "finalize refused the swapped signature, and Core refused the same transaction forced out by hand"

# ---- 9 -------------------------------------------------------------------------

say "9. A lock height above the tip is refused as non-final"
make_coins "$WORK/coins5.txt"
TIP9=$(cli getblockcount)
AHEAD=$((TIP9 + 10))
DEST5=$(cli -rpcwallet=miner getnewaddress)
PLAN5_ARGS=(--coins "$WORK/coins5.txt" --rate 2000 --change-index 4 --min-conf 1 --lock-height "$AHEAD" --out "$WORK/base5.psbt")
ex plan "${PLAN5_ARGS[@]}" "$DEST5:90000" >"$WORK/plan5.txt"
chk 9 "a plan with the lock height $AHEAD (tip $TIP9 plus 10) was made" [ "$?" = 0 ]
chk 9 "the plan carries that lock height" [ "$(sed -n 's/^lock-height //p' "$WORK/plan5.txt")" = "$AHEAD" ]
ex sign --who 1 --state "$WORK/key1.state" --now "$(now)" --in "$WORK/base5.psbt" --out "$WORK/s1_5.psbt" >"$WORK/sign1e.txt"
chk 9 "the signer signed as key 1 with a lock time ahead of the tip" [ "$?" = 0 ]
ex sign --who 2 --state "$WORK/key2.state" --now "$(now)" --in "$WORK/base5.psbt" --out "$WORK/s2_5.psbt" >"$WORK/sign2e.txt"
chk 9 "the signer signed as key 2" [ "$?" = 0 ]
ex plan "${PLAN5_ARGS[@]}" --signed "$WORK/s1_5.psbt" --signed "$WORK/s2_5.psbt" \
  --hex-out "$WORK/tx5.hex" "$DEST5:90000" >"$WORK/fin5.txt"
chk 9 "combine and finalize succeeded" [ "$?" = 0 ]
HEX5=$(cat "$WORK/tx5.hex" 2>/dev/null)
TMA5=$(cli testmempoolaccept "[\"$HEX5\"]")
note "Core on the transaction locked ahead of the tip: $(jq -c '.[0] | {allowed, "reject-reason"}' <<<"$TMA5")"
chk 9 "Core's testmempoolaccept refuses it (expected)" [ "$(jq -r '.[0].allowed' <<<"$TMA5")" = false ]
chk 9 "for being non-final, not for some other reason" \
  [ "$(jq -r '.[0]["reject-reason"] // ""' <<<"$TMA5" | grep -c 'non-final')" = 1 ]
cli generatetoaddress 10 "$MINE" >/dev/null
TMA5B=$(cli testmempoolaccept "[\"$HEX5\"]")
chk 9 "once the tip has reached $AHEAD, Core takes the same transaction" [ "$(jq -r '.[0].allowed' <<<"$TMA5B")" = true ]
done_step 9 "a lock height ten blocks ahead of the tip was refused by Core as non-final (the expected result), and accepted once the chain reached it"

# ---- result -------------------------------------------------------------------

say "Result"
failed=0
for n in 1 2 3 4 5 6 7 8 9; do
  printf '%s  step %s: %s\n' "${RESULT[$n]:-FAIL}" "$n" "${MESSAGE[$n]:-did not run}"
  [ "${RESULT[$n]:-FAIL}" = PASS ] || failed=1
done
if [ "$failed" = 0 ]; then
  echo "ALL PASS"
else
  echo "SOME STEPS FAILED"
  exit 1
fi
