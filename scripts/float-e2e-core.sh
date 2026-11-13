#!/usr/bin/env bash
# The refund float against Bitcoin Core, on regtest (docs/FLOAT-RULES.md,
# FL-02, FL-06, FL-11, FL-20 to FL-23, FL-35).
#
# Question: does a refund that the `float` module of `openbmt-owner` builds
# from the float's own coins, signs and finishes get accepted by Bitcoin
# Core, does it confirm with its change back to the float, does a refund
# larger than the float get refused, and does turning the float off pay the
# shop's own address from every coin?
#
# Bitcoin Core plays the chain and the shop's wallet. The float is the real
# library, through the example `float`. The float's key is the same on both
# sides, under different prefixes: the library holds the mainnet description
# of the account key (`xpub`), as the owner phone does; Core's regtest wallet
# holds the same account key with its private part under the test network's
# prefix (`tprv`). The scripts do not depend on the network, so Core's
# addresses are the regtest addresses of the library's scripts.
#
# What it does:
#   1. Starts a private regtest node, imports the float's private
#      descriptors into one Core wallet and the shop's into another, and
#      compares Core's own derivation of the float's addresses with the
#      library's (FL-02).
#   2. Funds three receive addresses of the float, one block after.
#   3. Pays a refund from the float (FL-20): Core accepts the finished
#      transaction at no less than the asked rate (FL-22), it spends only the
#      float's coins and pays only the refund and the float's change (FL-21),
#      it confirms, and the change is a coin of the float's change chain.
#   4. A refund larger than the float is refused and nothing is sent (FL-23).
#   5. A second refund, from the first one's change among the coins.
#   6. The refill amount at Core's own balance of the float (FL-11).
#   7. Turning the float off (FL-35): every coin goes to the shop's receive
#      address 3; the float is empty and the shop's wallet holds the coin.
#   8. The sealed paths (FL-03): the float's Compact SeedQR is sealed as the
#      owner phone's setup seals it, and a refund signed from the sealed seed
#      is the same transaction as the one signed from the words, and Core
#      takes it.
#   9. The key brought back (FL-06): the same scan again is taken and sealed
#      anew under a new data key, and the new seal signs a refund Core takes;
#      the words of the next index and of another shop are refused with the
#      rule FL-06 and leave the sealed seed and the data key byte for byte as
#      they were.
#  10. Turning the float off from the sealed seed (FL-35, FL-03): the move
#      pays the shop's receive address 4 from every coin, and Core takes it.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (put first on
# PATH; set CORE_BIN to use another; the bitcoind in /usr/local/bin is never
# used), jq, and cargo unless FLOAT_BIN names a built example.
#
# Environment:
#   CORE_BIN     directory with bitcoind and bitcoin-cli
#   DRILL_DIR    scratch directory (default: made new under ~/.cache/openbmt)
#   DRILL_PORTS  the RPC port of the regtest node (default 19900)
#   DRILL_KEEP=1 keep the scratch directory afterwards
#   FLOAT_BIN    the `float` example binary
#
# Everything here is regtest on 127.0.0.1. The keys come from published or
# fixed test words: TEST ONLY, NEVER USE. The node and the scratch directory
# are removed on every exit. The default Bitcoin data directory is never
# touched.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_PORTS:-19900}
mkdir -p "$HOME/.cache/openbmt/tmp"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/tmp/float-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/core"
WORK="$BASE/work"
RATE=2000 # satoshis per 1000 virtual bytes

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}
cli() { bitcoin-cli -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" "$@"; }
btc() { printf '%d.%08d' $(($1 / 100000000)) $(($1 % 100000000)); }

FAILS=0
chk() {
  local what=$1
  shift
  if "$@"; then note "ok:     $what"; else note "NOT ok: $what"; FAILS=$((FAILS + 1)); fi
}
# rule RULE MARK "what": PASS if no check since MARK failed.
rule() {
  if [ "$FAILS" = "$2" ]; then printf 'PASS %s %s\n' "$1" "$3"; else printf 'FAIL %s %s\n' "$1" "$3"; fi
}

for tool in bitcoind bitcoin-cli jq; do
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

if [ -z "${FLOAT_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/float}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner --example float ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner --example float ||
      die "the build failed"
  fi
  FLOAT_BIN="$TARGET/debug/examples/float"
fi
[ -x "$FLOAT_BIN" ] || die "no example binary at $FLOAT_BIN"
ex() { "$FLOAT_BIN" "$@"; }

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" -listen=0 -daemonwait \
  -fallbackfee=0.0001 -txindex=1 -dbcache=50 >/dev/null || die "bitcoind did not start"
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
[ -n "$NODE_PID" ] || die "no node pid"

cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 101 "$MINE" >/dev/null || die "no blocks"

# coins FILE WALLET: one line for each confirmed coin of the wallet: txid
# vout confirmations chain index transaction-hex.
make_coins() {
  : >"$1"
  cli -rpcwallet="$2" listunspent 1 |
    jq -r '.[] | .desc as $d | ($d | capture("/(?<c>[01])/(?<i>[0-9]+)\\]")) as $m
           | "\(.txid) \(.vout) \(.confirmations) \($m.c) \($m.i)"' |
    while read -r txid vout conf chain idx; do
      echo "$txid $vout $conf $chain $idx $(cli getrawtransaction "$txid")" >>"$1"
    done
}

# accept_send TXHEX TXID RATE: the finished transaction goes through
# testmempoolaccept; then it is sent and confirmed.
accept_send() {
  local hexs=$1 txid=$2 rate=$3 tma vs fee
  tma=$(cli testmempoolaccept "[\"$hexs\"]")
  vs=$(jq -r '.[0].vsize' <<<"$tma")
  fee=$(jq -r '.[0].fees.base * 100000000 | round' <<<"$tma")
  chk "testmempoolaccept allows $txid" [ "$(jq -r '.[0].allowed' <<<"$tma")" = true ]
  chk "its rate ($fee sat over $vs vB) is at least the asked $rate per 1000 vB" [ $((fee * 1000)) -ge $((rate * vs)) ]
  chk "Core took it" [ "$(cli sendrawtransaction "$hexs")" = "$txid" ]
  cli generatetoaddress 1 "$MINE" >/dev/null
  chk "a block confirmed it" [ "$(cli getrawtransaction "$txid" true | jq -r '.confirmations')" -ge 1 ]
}

fact() { sed -n "s/^$1 //p" <<<"$2" | tail -1; }

# ---- the wallets ----------------------------------------------------------------
say "1. The float's key and the shop's, in Core"
mark=$FAILS
ex keys --who float >"$WORK/float-keys.txt" || die "no float keys from the example"
ex keys --who shop >"$WORK/shop-keys.txt" || die "no shop keys from the example"
import_wallet() {
  local name=$1 keys=$2 r c res
  r=$(sed -n 's/^private-receive //p' "$keys")
  c=$(sed -n 's/^private-change //p' "$keys")
  r="$r#$(cli getdescriptorinfo "$r" | jq -r '.checksum')"
  c="$c#$(cli getdescriptorinfo "$c" | jq -r '.checksum')"
  cli -named createwallet wallet_name="$name" blank=true >/dev/null || die "no $name wallet"
  res=$(cli -rpcwallet="$name" importdescriptors "[{\"desc\":\"$r\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,99]},{\"desc\":\"$c\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,99]}]")
  chk "$name: Core imported the private descriptors" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]
  echo "$r"
}
FLOAT_R=$(import_wallet float "$WORK/float-keys.txt" | tail -1)
import_wallet shop "$WORK/shop-keys.txt" >/dev/null
mapfile -t FADDR < <(sed -n 's/^receive [0-9]* //p' "$WORK/float-keys.txt")
mapfile -t FCHANGE < <(sed -n 's/^change [0-9]* //p' "$WORK/float-keys.txt")
mapfile -t SADDR < <(sed -n 's/^receive [0-9]* //p' "$WORK/shop-keys.txt")
CORE_ADDR=$(cli deriveaddresses "$FLOAT_R" "[0,7]" | jq -r 'join(" ")')
chk "Core derives the float's receive addresses 0 to 7 as the library does" [ "$CORE_ADDR" = "${FADDR[*]}" ]
rule FL-02 "$mark" "the float is the plain account-0 native SegWit wallet of its 12 words: Core derives the same addresses"

# ---- funding ----------------------------------------------------------------------
say "2. The float is funded: 100000, 80000 and 60000 sat"
for pair in "0 100000" "1 80000" "2 60000"; do
  set -- $pair
  cli -rpcwallet=miner sendtoaddress "${FADDR[$1]}" "$(btc "$2")" >/dev/null || die "funding $1 failed"
done
cli generatetoaddress 1 "$MINE" >/dev/null
chk "the float holds 240000 sat in three coins" \
  [ "$(cli -rpcwallet=float listunspent 1 | jq -c '[.[] | .amount * 100000000 | round] | sort')" = "[60000,80000,100000]" ]

# ---- a refund --------------------------------------------------------------------
say "3. A refund of 70000 sat from the float's own coins"
mark=$FAILS
DEST=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins1.txt" float
BEFORE=$(cli -rpcwallet=float listunspent 1 | jq -c '[.[] | "\(.txid):\(.vout)"] | sort')
TIP=$(cli getblockcount)
out=$(ex pay --coins "$WORK/coins1.txt" --addr "$DEST" --sats 70000 --rate "$RATE" \
  --change-index 0 --lock-height "$TIP" --out "$WORK/refund1.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library paid the refund" [ "$code" = 0 ]
TXID=$(fact txid "$out")
HEX=$(cat "$WORK/refund1.hex")
chk "Core decodes the transaction to the ID the library gave" [ "$(cli decoderawtransaction "$HEX" | jq -r '.txid')" = "$TXID" ]
rule FL-20 "$mark" "a refund is paid from the float's coins, signed and finished by the library"
mark=$FAILS
DEC=$(cli decoderawtransaction "$HEX")
SPENT=$(jq -c '[.vin[] | "\(.txid):\(.vout)"] | sort' <<<"$DEC")
chk "every input is a coin the float held before" \
  [ "$(jq -n --argjson s "$SPENT" --argjson b "$BEFORE" '$s | all(. as $x | $b | index($x) != null)')" = true ]
chk "the outputs are the refund's address and the float's change address 0, and nothing else" \
  [ "$(jq -c --arg d "$DEST" --arg c "${FCHANGE[0]}" '[.vout[].scriptPubKey.address] | sort == ([$d, $c] | sort)' <<<"$DEC")" = true ]
chk "the refund output is 70000 sat" \
  [ "$(jq -r --arg d "$DEST" '.vout[] | select(.scriptPubKey.address == $d) | .value * 100000000 | round' <<<"$DEC")" = 70000 ]
rule FL-21 "$mark" "the transaction spends only the float's coins and pays only the refund and the float's change"
mark=$FAILS
accept_send "$HEX" "$TXID" "$RATE"
rule FL-22 "$mark" "Core accepts it at no less than the asked rate, and it confirms"
mark=$FAILS
CH=$(fact change "$out")
set -- $CH
CH_SATS=$2
chk "the refund's destination holds 70000 sat" \
  [ "$(cli -rpcwallet=miner getreceivedbyaddress "$DEST" | jq -r '. * 100000000 | round')" = 70000 ]
chk "the float's wallet holds the change ($CH_SATS sat) as a coin of its change chain at index 0" \
  [ "$(cli -rpcwallet=float listunspent 1 | jq -r --arg t "$TXID" --argjson s "$CH_SATS" \
    '[.[] | select(.txid == $t) | select((.amount * 100000000 | round) == $s) | .desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)"] | tostring')" = '["1/0"]' ]
chk "the float's balance is its 240000 less the refund and the fee" \
  [ "$(cli -rpcwallet=float getbalances | jq -r '.mine.trusted * 100000000 | round')" = $((240000 - 70000 - $(fact fee "$out"))) ]
rule FL-20 "$mark" "the change is back in the float: its balance is what it held less the refund and the fee"

# ---- too large ---------------------------------------------------------------------
say "4. A refund larger than the float is refused and nothing is sent"
mark=$FAILS
make_coins "$WORK/coins2.txt" float
MEMPOOL=$(cli getmempoolinfo | jq -r '.size')
out=$(ex pay --coins "$WORK/coins2.txt" --addr "$DEST" --sats 400000 --rate "$RATE" \
  --change-index 1 --lock-height "$TIP" --out "$WORK/refund-big.hex")
code=$?
note "$out"
chk "the library refused it (exit 2) with the rule FL-23" bash -c '[ "$1" = 2 ] && grep -q "^refused FL-23" <<<"$2"' _ "$code" "$out"
chk "no transaction was written and the mempool is as it was" bash -c '[ ! -e "$1" ] && [ "$(bitcoin-cli -regtest -datadir="$2" -rpcport="$3" getmempoolinfo | jq -r .size)" = "$4" ]' _ "$WORK/refund-big.hex" "$NODE_DIR" "$RPC_PORT" "$MEMPOOL"
rule FL-23 "$mark" "a refund larger than the float holds is refused, never split"

# ---- a second refund -------------------------------------------------------------
say "5. A second refund, with the first one's change among the coins"
mark=$FAILS
DEST2=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins3.txt" float
chk "the coins of the float include a coin of the change chain" grep -q ' 1 0 ' "$WORK/coins3.txt"
TIP=$(cli getblockcount)
out=$(ex pay --coins "$WORK/coins3.txt" --addr "$DEST2" --sats 20000 --rate "$RATE" \
  --change-index 1 --lock-height "$TIP" --out "$WORK/refund2.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library paid the second refund" [ "$code" = 0 ]
accept_send "$(cat "$WORK/refund2.hex")" "$(fact txid "$out")" "$RATE"
chk "the second refund's destination holds 20000 sat" \
  [ "$(cli -rpcwallet=miner getreceivedbyaddress "$DEST2" | jq -r '. * 100000000 | round')" = 20000 ]
rule FL-20 "$mark" "a second refund is paid from the float, change to the next change address"

# ---- the cap ------------------------------------------------------------------------
say "6. The refill amount at Core's own balance of the float"
mark=$FAILS
HELD=$(cli -rpcwallet=float getbalances | jq -r '.mine.trusted * 100000000 | round')
note "the float holds $HELD sat"
out=$(ex refill --cap 300000 --confirmed "$HELD" --unconfirmed 0)
chk "the refill is the cap less what the float holds: $out" [ "$out" = "refill $((300000 - HELD))" ]
out=$(ex refill --cap "$HELD" --confirmed "$HELD" --unconfirmed 0)
code=$?
chk "at the cap there is nothing to refill: $out" bash -c '[ "$1" = 2 ] && grep -q "^refused FL-11" <<<"$2"' _ "$code" "$out"
rule FL-11 "$mark" "the refill is the cap less the balance Core shows, and none at the cap"

# ---- turning it off -----------------------------------------------------------------
say "7. Turning the float off: every coin to the shop's receive address 3"
mark=$FAILS
make_coins "$WORK/coins4.txt" float
N=$(grep -c . "$WORK/coins4.txt")
TIP=$(cli getblockcount)
out=$(ex sweep --coins "$WORK/coins4.txt" --shop-index 3 --rate "$RATE" --lock-height "$TIP" --out "$WORK/sweep.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library swept the float" [ "$code" = 0 ]
chk "the destination is the shop's receive address 3" [ "$(fact to "$out")" = "${SADDR[3]}" ]
chk "every coin of the float is spent ($N)" [ "$(fact inputs "$out")" = "$N" ]
chk "there is no change" [ "$(fact change "$out")" = none ]
SWEEP_TXID=$(fact txid "$out")
SWEEP_DEC=$(cli decoderawtransaction "$(cat "$WORK/sweep.hex")")
chk "the one output pays the shop's address" \
  [ "$(jq -c --arg a "${SADDR[3]}" '[.vout[].scriptPubKey.address] == [$a]' <<<"$SWEEP_DEC")" = true ]
accept_send "$(cat "$WORK/sweep.hex")" "$SWEEP_TXID" "$RATE"
chk "the float holds nothing" [ "$(cli -rpcwallet=float listunspent 0 | jq -r 'length')" = 0 ]
chk "the shop's wallet holds the coin at receive index 3" \
  [ "$(cli -rpcwallet=shop listunspent 1 | jq -r --arg t "$SWEEP_TXID" '[.[] | select(.txid == $t) | .desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)"] | tostring')" = '["0/3"]' ]
rule FL-35 "$mark" "turning the float off pays the shop's own address from every coin, and the float is empty"

# ---- the sealed paths ----------------------------------------------------------------
say "8. The float's scan is sealed, and a refund is signed from the sealed seed"
mark=$FAILS
DESCRIPTION=$(sed -n 's/^descriptor //p' "$WORK/float-keys.txt")
SCAN=$(fact scan "$(ex scan --which float)")
out=$(ex seal --scan "$SCAN" --sealed "$WORK/sealed.hex" --key "$WORK/key.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library sealed the float's scan" [ "$code" = 0 ]
chk "the sealed float is the same wallet: its description is the float's" [ "$(fact description "$out")" = "$DESCRIPTION" ]
chk "its first address is the float's receive address 0" [ "$(fact address0 "$out")" = "${FADDR[0]}" ]
chk "the data key is 32 bytes and the sealed seed is 45 bytes" bash -c '[ "$(wc -c <"$1")" = 64 ] && [ "$(wc -c <"$2")" = 90 ]' _ "$WORK/key.hex" "$WORK/sealed.hex"
for pair in "4 90000" "5 50000"; do
  set -- $pair
  cli -rpcwallet=miner sendtoaddress "${FADDR[$1]}" "$(btc "$2")" >/dev/null || die "funding $1 failed"
done
cli generatetoaddress 1 "$MINE" >/dev/null
make_coins "$WORK/coins5.txt" float
chk "the float holds 140000 sat in two coins again" \
  [ "$(cli -rpcwallet=float listunspent 1 | jq -c '[.[] | .amount * 100000000 | round] | sort')" = "[50000,90000]" ]
BEFORE=$(cli -rpcwallet=float listunspent 1 | jq -c '[.[] | "\(.txid):\(.vout)"] | sort')
DEST3=$(cli -rpcwallet=miner getnewaddress)
TIP=$(cli getblockcount)
ex_pay_args=(--coins "$WORK/coins5.txt" --addr "$DEST3" --sats 30000 --rate "$RATE" --change-index 2 --lock-height "$TIP")
out=$(ex pay-sealed --sealed "$WORK/sealed.hex" --key "$WORK/key.hex" "${ex_pay_args[@]}" --out "$WORK/refund3.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library paid the refund from the sealed seed" [ "$code" = 0 ]
direct=$(ex pay "${ex_pay_args[@]}" --out "$WORK/refund3-direct.hex")
chk "it is the transaction that the words themselves sign" [ "$(fact txid "$direct")" = "$(fact txid "$out")" ]
HEX=$(cat "$WORK/refund3.hex")
DEC=$(cli decoderawtransaction "$HEX")
chk "every input is a coin the float held, and the outputs are the refund and the float's change address 2" \
  [ "$(jq -n --argjson d "$DEC" --argjson b "$BEFORE" --arg a "$DEST3" --arg c "${FCHANGE[2]}" \
    '([$d.vin[] | "\(.txid):\(.vout)"] | all(. as $x | $b | index($x) != null)) and ([$d.vout[].scriptPubKey.address] | sort == ([$a, $c] | sort))')" = true ]
accept_send "$HEX" "$(fact txid "$out")" "$RATE"
chk "the refund's destination holds 30000 sat" \
  [ "$(cli -rpcwallet=miner getreceivedbyaddress "$DEST3" | jq -r '. * 100000000 | round')" = 30000 ]
rule FL-03 "$mark" "a refund signed from the sealed seed is the words' own transaction, and Core takes it"

say "9. The key brought back: the same words again are taken, others are refused"
mark=$FAILS
sha() { sha256sum "$1" | cut -d' ' -f1; }
SEALED_BEFORE=$(sha "$WORK/sealed.hex")
KEY_BEFORE=$(sha "$WORK/key.hex")
out=$(ex restore --scan "$SCAN" --description "$DESCRIPTION" --sealed "$WORK/sealed2.hex" --key "$WORK/key2.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library took the float's words scanned again" [ "$code" = 0 ]
chk "the restored wallet is the recorded float" [ "$(fact description "$out")" = "$DESCRIPTION" ]
chk "the new data key is not the old one, and the new sealed seed is not the old" \
  bash -c '[ "$1" != "$2" ] && [ "$3" != "$4" ]' _ "$(sha "$WORK/key2.hex")" "$KEY_BEFORE" "$(sha "$WORK/sealed2.hex")" "$SEALED_BEFORE"
chk "the old data key does not open the new sealed seed" bash -c '
  out=$("$1" pay-sealed --sealed "$2" --key "$3" --coins "$4" --addr "$5" --sats 1000 --rate "$6" --change-index 3 --lock-height 0 --out "$7")
  [ "$?" = 2 ] && grep -q "^refused SS-" <<<"$out"' _ "$FLOAT_BIN" "$WORK/sealed2.hex" "$WORK/key.hex" "$WORK/coins5.txt" "$DEST3" "$RATE" "$WORK/nope.hex"
for which in index1 shop; do
  wrong=$(fact scan "$(ex scan --which "$which")")
  out=$(ex restore --scan "$wrong" --description "$DESCRIPTION" --sealed "$WORK/sealed.hex" --key "$WORK/key.hex")
  code=$?
  note "$which: $out"
  chk "the words of $which are refused with the rule FL-06 (exit 2)" bash -c '[ "$1" = 2 ] && grep -q "^refused FL-06" <<<"$2"' _ "$code" "$out"
  chk "the sealed seed and the data key are byte for byte as they were" \
    [ "$(sha "$WORK/sealed.hex")" = "$SEALED_BEFORE" -a "$(sha "$WORK/key.hex")" = "$KEY_BEFORE" ]
done
out=$(ex restore --scan "$(fact scan "$(ex scan --which index1)")" --description "$DESCRIPTION" --sealed "$WORK/sealed3.hex" --key "$WORK/key3.hex")
chk "a refusal writes no file" [ ! -e "$WORK/sealed3.hex" -a ! -e "$WORK/key3.hex" ]
# The new seal signs a refund of the float's coins.
make_coins "$WORK/coins6.txt" float
DEST4=$(cli -rpcwallet=miner getnewaddress)
TIP=$(cli getblockcount)
out=$(ex pay-sealed --sealed "$WORK/sealed2.hex" --key "$WORK/key2.hex" --coins "$WORK/coins6.txt" --addr "$DEST4" --sats 25000 \
  --rate "$RATE" --change-index 3 --lock-height "$TIP" --out "$WORK/refund4.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the restored seal signs a refund" [ "$code" = 0 ]
accept_send "$(cat "$WORK/refund4.hex")" "$(fact txid "$out")" "$RATE"
chk "the refund's destination holds 25000 sat" \
  [ "$(cli -rpcwallet=miner getreceivedbyaddress "$DEST4" | jq -r '. * 100000000 | round')" = 25000 ]
rule FL-06 "$mark" "the float's words scanned again are taken and sealed anew, other words are refused and change nothing, and the new seal signs"

say "10. Turning the float off from the restored seal: every coin to the shop's receive address 4"
mark=$FAILS
make_coins "$WORK/coins7.txt" float
N=$(grep -c . "$WORK/coins7.txt")
TIP=$(cli getblockcount)
out=$(ex sweep-sealed --sealed "$WORK/sealed2.hex" --key "$WORK/key2.hex" --coins "$WORK/coins7.txt" --shop-index 4 \
  --rate "$RATE" --lock-height "$TIP" --out "$WORK/sweep2.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library swept the float from the sealed seed" [ "$code" = 0 ]
chk "the destination is the shop's receive address 4" [ "$(fact to "$out")" = "${SADDR[4]}" ]
chk "every coin of the float is spent ($N)" [ "$(fact inputs "$out")" = "$N" ]
chk "there is no change" [ "$(fact change "$out")" = none ]
SWEEP2_TXID=$(fact txid "$out")
accept_send "$(cat "$WORK/sweep2.hex")" "$SWEEP2_TXID" "$RATE"
chk "the float holds nothing" [ "$(cli -rpcwallet=float listunspent 0 | jq -r 'length')" = 0 ]
chk "the shop's wallet holds the coin at receive index 4" \
  [ "$(cli -rpcwallet=shop listunspent 1 | jq -r --arg t "$SWEEP2_TXID" '[.[] | select(.txid == $t) | .desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)"] | tostring')" = '["0/4"]' ]
rule FL-35 "$mark" "turning the float off from the sealed seed pays the shop's own address from every coin"

echo
if [ "$FAILS" = 0 ]; then
  echo "ALL PASS"
else
  echo "FAILED: $FAILS check(s)"
  exit 1
fi
