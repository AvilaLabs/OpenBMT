#!/usr/bin/env bash
# The spend path of a wallet with one key against Bitcoin Core, on regtest
# (SP-87 of docs/SPEND-RULES.md).
#
# Question: does a file that `openbmt-spend` writes for a wallet with one key
# get signed and finished by Bitcoin Core, does `finalize` make from Core's
# signature the very transaction Core finishes, and does Core's
# `testmempoolaccept` take it?
#
# The key is the same on both sides, under different prefixes. The library
# holds the mainnet description of the account key (`xpub`), as the owner
# phone does; Core's regtest wallet holds the same account key with its
# private part under the test network's prefix (`tprv`). The scripts do not
# depend on the network, so Core's addresses are the regtest addresses of the
# library's scripts. The library's file carries the mainnet extended public
# key as its global extended public key (SP-82); Core reads it as it is.
#
# What it does (the steps are numbered as in the printed table):
#   1. Starts a private regtest node, imports the private receive and change
#      descriptions into a Core wallet, and funds four receive addresses with
#      different amounts, one block after each. Core's own derivation of the
#      addresses is compared with the library's.
#   2. The example reads the coins (Core's `listunspent` and
#      `getrawtransaction`), plans a payment at a rate with the tip's height
#      as the lock time, and writes the file. Core's `decodepsbt` and
#      `analyzepsbt` agree with the plan: fee, inputs, outputs, one key-path
#      hint per input, the one global extended public key, and the next role
#      is "signer".
#   3. Core signs (`walletprocesspsbt`, not finished). The library takes the
#      signatures, combines and finalizes; Core's `finalizepsbt` on the same
#      file gives the same transaction bytes; `testmempoolaccept` allows it;
#      the rate Core computes is at least the asked rate; it is sent and
#      confirmed and the change is where the plan put it.
#   4. A second payment, with the change placed first by a seed: Core signs
#      and finishes in one call (`walletprocesspsbt` with finalize). The
#      library reads the returned file as the finished transaction it holds
#      and checks it against the file it sent (SP-86); the bytes equal Core's
#      own extraction. It confirms.
#   5. A send of every coin to the wallet's own change script (a
#      consolidation): the output carries its key-path hint in Core's
#      reading, Core signs, the library finishes, Core takes it.
#   6. Negative: a transaction in which input 0 carries a signature made for
#      input 1 is refused by `finalize`, and when forced out by hand,
#      `testmempoolaccept` refuses it too.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (the
# directory is put first on PATH; set CORE_BIN to use another; the bitcoind in
# /usr/local/bin is never used), jq, base64, and cargo unless SPEND_SINGLE_BIN
# names a prebuilt `spend_single_regtest` example binary. A build goes
# through ~/.cache/openbmt/guard.sh when that file exists.
#
# Environment:
#   CORE_BIN          directory with bitcoind and bitcoin-cli
#   DRILL_DIR         scratch directory (default:
#                     ~/.cache/openbmt/tmp/spend-single-core, made new)
#   DRILL_RPC_PORT    RPC port for the regtest node (default 18995)
#   DRILL_KEEP=1      keep the scratch directory afterwards
#   SPEND_SINGLE_BIN  the `spend_single_regtest` example binary
#
# Everything here is regtest on 127.0.0.1. The key comes from fixed test
# bytes: TEST ONLY, NEVER USE. No real keys or funds are involved. The node
# and the scratch directory are removed on every exit. The default Bitcoin
# data directory is never touched.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_RPC_PORT:-18995}
mkdir -p "$HOME/.cache/openbmt/tmp"
BASE=${DRILL_DIR:-$HOME/.cache/openbmt/tmp/spend-single-core}
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

# A fresh scratch directory: whatever an earlier run left is removed first.
if [ -z "${DRILL_DIR:-}" ]; then rm -rf -- "$BASE"; fi
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

if [ -z "${SPEND_SINGLE_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/spend-single}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-spend --example spend_single_regtest ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-spend --example spend_single_regtest ||
      die "the build failed"
  fi
  SPEND_SINGLE_BIN="$TARGET/debug/examples/spend_single_regtest"
fi
[ -x "$SPEND_SINGLE_BIN" ] || die "no example binary at $SPEND_SINGLE_BIN"
ex() { "$SPEND_SINGLE_BIN" "$@"; }

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" -listen=0 -daemonwait \
  -fallbackfee=0.0001 -txindex=1 -dbcache=50 >/dev/null || die "bitcoind did not start"
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
[ -n "$NODE_PID" ] || die "no node pid"

b64() { base64 -w0 "$1"; }
unb64() { base64 -d <<<"$1" >"$2"; }
# The value in satoshis of the first output paying $2 in the decoded
# transaction $1.
tx_paying() { jq -r --arg a "$2" '[.vout[] | select(.scriptPubKey.address == $a)] | first | .value * 100000000 | round' <<<"$1"; }

# ---- 1 -------------------------------------------------------------------------

say "1. The key in a Core wallet, four funded coins"
KEYS="$WORK/keys.txt"
ex keys 0 1 2 3 4 5 >"$KEYS" || die "the example could not make the key"
PRIV_RECV=$(sed -n 's/^private-receive //p' "$KEYS")
PRIV_CHNG=$(sed -n 's/^private-change //p' "$KEYS")
[ -n "$PRIV_RECV" ] && [ -n "$PRIV_CHNG" ] || die "no private descriptor from the example"
note "the library's wallet: $(sed -n 's/^descriptor //p' "$KEYS" | cut -c1-60)..."

# The same account key, private, under the test prefix; Core adds checksums.
PRIV_RECV="$PRIV_RECV#$(cli getdescriptorinfo "$PRIV_RECV" | jq -r '.checksum')"
PRIV_CHNG="$PRIV_CHNG#$(cli getdescriptorinfo "$PRIV_CHNG" | jq -r '.checksum')"
cli -named createwallet wallet_name=shop blank=true >/dev/null || die "no shop wallet"
res=$(cli -rpcwallet=shop importdescriptors "[{\"desc\":\"$PRIV_RECV\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,99]},{\"desc\":\"$PRIV_CHNG\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,99]}]")
chk 1 "Core imported the private receive and change descriptions" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]

# Core derives the same addresses (the same scripts) on both chains.
core_recv=$(cli deriveaddresses "$PRIV_RECV" "[0,5]" | jq -r '.[]')
ours_recv=$(sed -n 's/^receive [0-9]* //p' "$KEYS")
chk 1 "Core and the library derive the same six receive addresses" [ "$core_recv" = "$ours_recv" ]
core_chng=$(cli deriveaddresses "$PRIV_CHNG" "[0,5]" | jq -r '.[]')
ours_chng=$(sed -n 's/^change [0-9]* //p' "$KEYS")
chk 1 "Core and the library derive the same six change addresses" [ "$core_chng" = "$ours_chng" ]
mapfile -t RADDR <<<"$ours_recv"
mapfile -t CADDR <<<"$ours_chng"

cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 101 "$MINE" >/dev/null || die "no blocks"
AMOUNTS=(40000 55000 70000 100000)
for i in 0 1 2 3; do
  cli -rpcwallet=miner sendtoaddress "${RADDR[$i]}" "$(btc "${AMOUNTS[$i]}")" >/dev/null || die "funding $i failed"
  cli generatetoaddress 1 "$MINE" >/dev/null
done
have=$(cli -rpcwallet=shop listunspent 1 | jq -c '[.[] | .amount * 100000000 | round] | sort')
chk 1 "the Core wallet holds the four coins ($have)" [ "$have" = "[40000,55000,70000,100000]" ]
done_step 1 "the key is in Core, four coins of 40000..100000 sat at depths 4..1, addresses agree"

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

# ---- 2 -------------------------------------------------------------------------

say "2. Plan a payment of 120,000 sat at $RATE sat per 1000 vB, locked to the tip"
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
chk 2 "three inputs, oldest first (depths 4, 3, 2; 40000, 55000, 70000)" \
  [ "$(sed -n 's/^input [0-9a-f]*:[0-9]* \([0-9]*\) \([0-9]*\) .*/\1@\2/p' "$WORK/plan1.txt" | tr '\n' ' ')" = "40000@4 55000@3 70000@2 " ]
chk 2 "the change goes to change index 0, which is Core's change address 0" [ "${P_CHANGE%% *}" = "${CADDR[0]}" ]
chk 2 "the fee is ceil(vsize * rate / 1000)" [ "$P_FEE" = "$(((P_VSIZE * RATE + 999) / 1000))" ]
chk 2 "the change is the coins minus 120000 minus the fee" [ "${P_CHANGE##* }" = "$((165000 - 120000 - P_FEE))" ]
B1=$(b64 "$WORK/base1.psbt")
DEC=$(cli decodepsbt "$B1")
ANA=$(cli analyzepsbt "$B1")
chk 2 "Core's fee equals the plan's fee ($P_FEE)" [ "$(jq -r '.fee * 100000000 | round' <<<"$DEC")" = "$P_FEE" ]
chk 2 "Core's txid of the file equals the plan's" [ "$(jq -r '.tx.txid' <<<"$DEC")" = "$P_TXID" ]
chk 2 "Core sees three inputs and two outputs" [ "$(jq -r '[(.tx.vin | length), (.tx.vout | length)] | tostring' <<<"$DEC")" = "[3,2]" ]
chk 2 "the payment output is 120000 sat to the payment address" [ "$(tx_paying "$(jq -c '.tx' <<<"$DEC")" "$DEST")" = 120000 ]
chk 2 "the change output is the plan's change" [ "$(tx_paying "$(jq -c '.tx' <<<"$DEC")" "${P_CHANGE%% *}")" = "${P_CHANGE##* }" ]
chk 2 "Core reads the lock time as the tip's height ($TIP)" [ "$(jq -r '.tx.locktime' <<<"$DEC")" = "$TIP" ]
chk 2 "Core reads one key-path hint on every input" \
  [ "$(jq -r '[.inputs[] | (.bip32_derivs | length)] | unique | tostring' <<<"$DEC")" = "[1]" ]
chk 2 "every input has the hash type ALL and no witness script" \
  [ "$(jq -r '[.inputs[] | select(.sighash == "ALL" and (.witness_script == null))] | length' <<<"$DEC")" = 3 ]
note "Core's global_xpubs: $(jq -c '.global_xpubs' <<<"$DEC")"
chk 2 "Core reads the one global extended public key with the account path" \
  [ "$(jq -r '[.global_xpubs[] | .path] | tostring' <<<"$DEC")" = '["m/84h/0h/0h"]' ]
chk 2 "the change output carries one key-path hint, the payment none" \
  [ "$(jq -r '[.outputs[] | (.bip32_derivs // [] | length)] | sort | tostring' <<<"$DEC")" = "[0,1]" ]
chk 2 "analyzepsbt: the next role is signer, every input has its utxo and none is final" \
  [ "$(jq -r '[.next, ([.inputs[] | select(.has_utxo and (.is_final | not))] | length)] | tostring' <<<"$ANA")" = '["signer",3]' ]
done_step 2 "planned: oldest first, change at index 0, fee $P_FEE sat over $P_VSIZE vB; Core reads the file as the rule says"

# ---- 3 -------------------------------------------------------------------------

say "3. Core signs; the library combines and finalizes; Core's own finalize is the same"
C1=$(cli -rpcwallet=shop walletprocesspsbt "$B1" true ALL true false)
C1P=$(jq -r '.psbt' <<<"$C1")
chk 3 "Core's wallet added one signature to each input" \
  [ "$(cli decodepsbt "$C1P" | jq -c '[.inputs[] | (.partial_signatures // {} | length)]')" = "[1,1,1]" ]
unb64 "$C1P" "$WORK/core_signed1.psbt"
ex plan "${PLAN1_ARGS[@]}" --signed "$WORK/core_signed1.psbt" --hex-out "$WORK/tx1.hex" "$DEST:120000" >"$WORK/fin1.txt"
code=$?
sed -n '/^signatures/,$p' "$WORK/fin1.txt" | sed 's/^/      /'
chk 3 "the library took the signatures, combined and finalized" [ "$code" = 0 ]
chk 3 "one signature on each of the three inputs" [ "$(sed -n 's/^signatures //p' "$WORK/fin1.txt")" = "1,1,1" ]
chk 3 "the finished transaction has the planned transaction ID" [ "$(sed -n 's/^final-txid //p' "$WORK/fin1.txt")" = "$P_TXID" ]
HEX1=$(cat "$WORK/tx1.hex" 2>/dev/null)
CORE_FIN=$(cli finalizepsbt "$C1P")
chk 3 "Core's finalizepsbt completes the signed file" [ "$(jq -r '.complete' <<<"$CORE_FIN")" = true ]
if [ "$(jq -r '.hex' <<<"$CORE_FIN")" = "$HEX1" ]; then
  note "Core's finalizepsbt and the library's finalize give the same transaction bytes"
  EQUAL=1
else
  note "the transaction bytes DIFFER between Core's finalize and the library's:"
  note "core: $(jq -r '.hex' <<<"$CORE_FIN")"
  note "ours: $HEX1"
  EQUAL=
fi
chk 3 "byte-equal transactions from Core's finalize and the library's" [ -n "$EQUAL" ]
TMA=$(cli testmempoolaccept "[\"$HEX1\"]")
note "testmempoolaccept: $(jq -c '.[0] | {allowed, vsize, fees}' <<<"$TMA")"
CVS=$(jq -r '.[0].vsize' <<<"$TMA")
CFEE=$(jq -r '.[0].fees.base * 100000000 | round' <<<"$TMA")
chk 3 "testmempoolaccept allows it" [ "$(jq -r '.[0].allowed' <<<"$TMA")" = true ]
chk 3 "the transaction carries the lock time of the tip's height ($TIP)" \
  [ "$(cli decoderawtransaction "$HEX1" | jq -r '.locktime')" = "$TIP" ]
chk 3 "Core's fee is the plan's fee" [ "$CFEE" = "$P_FEE" ]
chk 3 "the rate Core computes ($CFEE sat over $CVS vB) is at least the asked $RATE per 1000 vB" [ $((CFEE * 1000)) -ge $((RATE * CVS)) ]
within() { [ "$1" -le "$2" ] && [ $(($2 - $1)) -le "$3" ]; }
chk 3 "Core's vsize is at most the plan's, and no more than 2 vB per input below it" within "$CVS" "$P_VSIZE" 7
SENT=$(cli sendrawtransaction "$HEX1")
chk 3 "sendrawtransaction took it and the txid is the plan's" [ "$SENT" = "$P_TXID" ]
cli generatetoaddress 1 "$MINE" >/dev/null
got=$(cli -rpcwallet=miner listunspent 1 9999999 "[\"$DEST\"]" | jq -r '[.[].amount * 100000000 | round] | tostring')
chk 3 "the payment address holds the coin of 120000 sat" [ "$got" = "[120000]" ]
ch=$(cli -rpcwallet=shop listunspent 1 | jq -c --arg t "$P_TXID" \
  '[.[] | select(.txid == $t)] | map({sats: (.amount * 100000000 | round), address, desc: (.desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") )})')
note "the wallet's coins from it: $ch"
chk 3 "the wallet shows the change at change index 0 with the planned amount" \
  [ "$(jq -r --arg a "${P_CHANGE%% *}" --argjson s "${P_CHANGE##* }" 'length == 1 and .[0].address == $a and .[0].sats == $s and .[0].desc.c == "1" and .[0].desc.i == "0"' <<<"$ch")" = true ]
if [ -n "$EQUAL" ]; then SAME="the same bytes"; else SAME="different bytes"; fi
done_step 3 "Core signed, the library finished with $SAME as Core's finalize; Core allowed, relayed and confirmed it at $CFEE sat over $CVS vB"

# ---- 4 -------------------------------------------------------------------------

say "4. Core signs and finishes in one call; the library reads the finished transaction"
DEST2=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins2.txt"
TIP2=$(cli getblockcount)
# The change goes at a place taken from a seed and the inputs, and the inputs
# are Core's coins, which differ on every run. Seeds are tried until one puts
# the change first among the outputs.
for n in $(seq 0 63); do
  SEED2=$(printf '%064x' "$n")
  PLAN2_ARGS=(--coins "$WORK/coins2.txt" --rate 3000 --change-index 1 --min-conf 1 --lock-height "$TIP2" --change-seed "$SEED2" --out "$WORK/base2.psbt")
  ex plan "${PLAN2_ARGS[@]}" "$DEST2:60000" >"$WORK/plan2.txt"
  code=$?
  [ "$code" = 0 ] && [ "$(sed -n 's/^change-position //p' "$WORK/plan2.txt")" = 0 ] && break
done
chk 4 "the second plan was made" [ "$code" = 0 ]
sed 's/^/      /' "$WORK/plan2.txt"
P2_TXID=$(sed -n 's/^txid //p' "$WORK/plan2.txt")
P2_FEE=$(sed -n 's/^fee //p' "$WORK/plan2.txt")
P2_AT=$(sed -n 's/^change-position //p' "$WORK/plan2.txt")
chk 4 "the seed puts the change first among the outputs (place $P2_AT)" [ "$P2_AT" = 0 ]
B2=$(b64 "$WORK/base2.psbt")
chk 4 "Core reads the change at that place, to change address 1" \
  [ "$(cli decodepsbt "$B2" | jq -r --argjson n "${P2_AT:-9}" '.tx.vout[$n].scriptPubKey.address')" = "${CADDR[1]}" ]
C2=$(cli -rpcwallet=shop walletprocesspsbt "$B2" true ALL true true)
chk 4 "Core signed and finished every input" [ "$(jq -r '.complete' <<<"$C2")" = true ]
unb64 "$(jq -r '.psbt' <<<"$C2")" "$WORK/core_final2.psbt"
ex plan "${PLAN2_ARGS[@]}" --final "$WORK/core_final2.psbt" --hex-out "$WORK/tx2.hex" "$DEST2:60000" >"$WORK/fin2.txt"
code=$?
sed -n '/^checked-fee/,$p' "$WORK/fin2.txt" | sed 's/^/      /'
chk 4 "the library read the returned file as the finished transaction and checked it" [ "$code" = 0 ]
chk 4 "the checked fee is the plan's fee ($P2_FEE)" [ "$(sed -n 's/^checked-fee //p' "$WORK/fin2.txt")" = "$P2_FEE" ]
chk 4 "its transaction ID is the plan's" [ "$(sed -n 's/^final-txid //p' "$WORK/fin2.txt")" = "$P2_TXID" ]
HEX2=$(cat "$WORK/tx2.hex" 2>/dev/null)
chk 4 "the bytes equal Core's own extraction of the final file" \
  [ "$(cli finalizepsbt "$(jq -r '.psbt' <<<"$C2")" | jq -r '.hex')" = "$HEX2" ]
TMA2=$(cli testmempoolaccept "[\"$HEX2\"]")
chk 4 "testmempoolaccept allows it" [ "$(jq -r '.[0].allowed' <<<"$TMA2")" = true ]
chk 4 "the rate is at least the asked 3000 per 1000 vB" \
  [ $(($(jq -r '.[0].fees.base * 100000000 | round' <<<"$TMA2") * 1000)) -ge $((3000 * $(jq -r '.[0].vsize' <<<"$TMA2"))) ]
chk 4 "sendrawtransaction took it" [ "$(cli sendrawtransaction "$HEX2")" = "$P2_TXID" ]
cli generatetoaddress 1 "$MINE" >/dev/null
got=$(cli -rpcwallet=miner listunspent 1 9999999 "[\"$DEST2\"]" | jq -r '[.[].amount * 100000000 | round] | tostring')
chk 4 "the second payment address holds 60000 sat" [ "$got" = "[60000]" ]
chk 4 "the change is at change index 1" \
  [ "$(cli -rpcwallet=shop listunspent 1 | jq -r --arg t "$P2_TXID" '[.[] | select(.txid == $t) | .desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)"] | tostring')" = '["1/1"]' ]
done_step 4 "Core signed and finished in one call; the library's check of the finished transaction passed and the bytes are Core's"

# ---- 5 -------------------------------------------------------------------------

say "5. A send of every coin to the wallet's own change script (a consolidation)"
make_coins "$WORK/coins3.txt"
NCOINS=$(grep -c . "$WORK/coins3.txt")
PLAN3_ARGS=(--coins "$WORK/coins3.txt" --rate 2000 --change-index 2 --min-conf 1 --out "$WORK/base3.psbt" --send-all)
ex plan "${PLAN3_ARGS[@]}" "${CADDR[2]}:0" >"$WORK/plan3.txt"
chk 5 "the consolidation plan was made" [ "$?" = 0 ]
sed 's/^/      /' "$WORK/plan3.txt"
P3_TXID=$(sed -n 's/^txid //p' "$WORK/plan3.txt")
P3_FEE=$(sed -n 's/^fee //p' "$WORK/plan3.txt")
P3_IN=$(sed -n 's/^total-in //p' "$WORK/plan3.txt")
chk 5 "every coin of the wallet is spent ($NCOINS)" [ "$(grep -c '^input ' "$WORK/plan3.txt")" = "$NCOINS" ]
one_output_to_change_2() { [ "$(grep -c '^payment ' "$WORK/plan3.txt")" = 1 ] && grep -q "^payment ${CADDR[2]} " "$WORK/plan3.txt"; }
chk 5 "one output, and it is the wallet's change address 2" one_output_to_change_2
B3=$(b64 "$WORK/base3.psbt")
chk 5 "Core reads one key-path hint on the output" \
  [ "$(cli decodepsbt "$B3" | jq -r '[.outputs[] | (.bip32_derivs // [] | length)] | tostring')" = "[1]" ]
C3=$(cli -rpcwallet=shop walletprocesspsbt "$B3" true ALL true false)
unb64 "$(jq -r '.psbt' <<<"$C3")" "$WORK/core_signed3.psbt"
ex plan "${PLAN3_ARGS[@]}" --signed "$WORK/core_signed3.psbt" --hex-out "$WORK/tx3.hex" "${CADDR[2]}:0" >"$WORK/fin3.txt"
chk 5 "the library took Core's signatures and finalized" [ "$?" = 0 ]
HEX3=$(cat "$WORK/tx3.hex" 2>/dev/null)
chk 5 "the bytes equal Core's finalizepsbt" [ "$(cli finalizepsbt "$(jq -r '.psbt' <<<"$C3")" | jq -r '.hex')" = "$HEX3" ]
TMA3=$(cli testmempoolaccept "[\"$HEX3\"]")
chk 5 "testmempoolaccept allows it" [ "$(jq -r '.[0].allowed' <<<"$TMA3")" = true ]
chk 5 "the rate is at least the asked 2000 per 1000 vB" \
  [ $(($(jq -r '.[0].fees.base * 100000000 | round' <<<"$TMA3") * 1000)) -ge $((2000 * $(jq -r '.[0].vsize' <<<"$TMA3"))) ]
chk 5 "sendrawtransaction took it" [ "$(cli sendrawtransaction "$HEX3")" = "$P3_TXID" ]
cli generatetoaddress 1 "$MINE" >/dev/null
chk 5 "the wallet holds one coin, $((P3_IN - P3_FEE)) sat, at change index 2" \
  [ "$(cli -rpcwallet=shop listunspent 1 | jq -c '[.[] | {sats: (.amount * 100000000 | round), at: (.desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)")}]')" = "[{\"sats\":$((P3_IN - P3_FEE)),\"at\":\"1/2\"}]" ]
done_step 5 "every coin went to the wallet's own change address 2 in one transaction"

# ---- 6 -------------------------------------------------------------------------

say "6. A signature made for another input"
for i in 4 5; do
  cli -rpcwallet=miner sendtoaddress "${RADDR[$i]}" 0.0006 >/dev/null || die "funding $i failed"
done
cli generatetoaddress 1 "$MINE" >/dev/null
DEST4=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins4.txt"
ex plan --coins "$WORK/coins4.txt" --rate 2000 --change-index 3 --min-conf 1 --out "$WORK/base4.psbt" "$DEST4:90000" >"$WORK/plan4.txt"
chk 6 "a plan over fresh coins was made" [ "$?" = 0 ]
chk 6 "it has at least two inputs" [ "$(grep -c '^input ' "$WORK/plan4.txt")" -ge 2 ]
ex forge --psbt "$WORK/base4.psbt" --hex-out "$WORK/forced.hex" >"$WORK/forge.txt"
code=$?
sed 's/^/      /' "$WORK/forge.txt"
forge_refused() { [ "$code" = 0 ] && grep -q '^refused ' "$WORK/forge.txt"; }
chk 6 "the library's finalize refused the file in which input 0 carries a signature made for input 1" forge_refused
FORCED=$(cat "$WORK/forced.hex" 2>/dev/null)
chk 6 "a forced transaction was written" [ -n "$FORCED" ]
TMAF=$(cli testmempoolaccept "[\"$FORCED\"]")
note "Core on the forced transaction: $(jq -c '.[0] | {allowed, "reject-reason"}' <<<"$TMAF")"
chk 6 "Core's testmempoolaccept refuses it" [ "$(jq -r '.[0].allowed' <<<"$TMAF")" = false ]
chk 6 "for a script failure, not for some other reason" \
  [ "$(jq -r '.[0]["reject-reason"] // ""' <<<"$TMAF" | grep -c 'script-verify-flag')" = 1 ]
done_step 6 "finalize refused the swapped signature, and Core refused the same transaction forced out by hand"

# ---- result -------------------------------------------------------------------

say "Result"
failed=0
for n in 1 2 3 4 5 6; do
  printf '%s  step %s: %s\n' "${RESULT[$n]:-FAIL}" "$n" "${MESSAGE[$n]:-did not run}"
  [ "${RESULT[$n]:-FAIL}" = PASS ] || failed=1
done
if [ "$failed" = 0 ]; then
  echo "ALL PASS"
  echo "spend-single-e2e-core-ok"
else
  echo "SOME STEPS FAILED"
  exit 1
fi
