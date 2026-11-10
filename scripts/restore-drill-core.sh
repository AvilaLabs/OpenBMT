#!/usr/bin/env bash
# Restore drill against Bitcoin Core, on regtest.
#
# Question: can an OpenBMT wallet (2-of-3 P2WSH sortedmulti, BIP48 keys, one
# multipath descriptor) be found and spent with Bitcoin Core alone, with no
# OpenBMT software, including payments at very high address indexes?
#
# What it does:
#   1. Starts a private regtest node in a scratch directory.
#   2. Makes three fresh keys and builds the wallet descriptor.
#   3. Compares openbmt-core's descriptor checksum, single-path forms and
#      addresses with Bitcoin Core's.
#   4. Pays addresses at low and high indexes.
#   5. Restores watch-only four ways and times each: a default import (which
#      misses the high indexes), the multipath descriptor with an explicit
#      range (slow), the two single-path descriptors with the receive one
#      imported as not active (fast), and a scan of the unspent output set
#      with no wallet at all.
#   6. Spends with each pair of keys through PSBTs, shows that one key alone
#      cannot spend, and checks whether a signer wallet needs the range too.
#
# Needs: bitcoind and bitcoin-cli (Bitcoin Core 29.0 or newer) and jq on PATH.
# The comparison in step 3 needs cargo; set SKIP_CRATE_CHECK=1 to leave it out,
# or set DESCRIBE to a prebuilt `describe` example binary.
#
# Environment:
#   DRILL_DIR         scratch directory (default: a new temporary directory)
#   DRILL_HIGH        highest address index paid and restored (default 5000)
#   DRILL_SLOW=0      skip the slow restore in step 5b. Its time grows with
#                     DRILL_HIGH (tens of milliseconds per index); see
#                     docs/research/restore-drill-core-2026-11-09.md
#   DRILL_RPC_PORT    RPC port for the regtest node (default 18943)
#   DRILL_KEEP=1      keep the scratch directory afterwards
#   SKIP_CRATE_CHECK  set to 1 to skip step 3
#   DESCRIBE          command that runs the `describe` example
#
# Everything here is regtest. No real keys or funds are involved.

set -euo pipefail

HIGH=${DRILL_HIGH:-5000}
MARGIN=1000
TOP=$((HIGH + MARGIN))
INDEXES=(0 999 1000 4321 "$HIGH")
RPC_PORT=${DRILL_RPC_PORT:-18943}
DIR=${DRILL_DIR:-$(mktemp -d)}
ROOT=$(cd "$(dirname "$0")/.." && pwd)

say() { printf '\n== %s\n' "$*"; }
ok() { printf 'PASS  %s\n' "$*"; }
fail() {
  printf 'FAIL  %s\n' "$*" >&2
  exit 1
}
# A wide import can run for many minutes, so the client never times out.
cli() { bitcoin-cli -regtest -datadir="$DIR" -rpcport="$RPC_PORT" -rpcclienttimeout=0 "$@"; }
sats() { jq -n --arg v "$1" '$v | tonumber * 100000000 | round'; }

[[ "$HIGH" =~ ^[0-9]+$ ]] && [ "$HIGH" -ge 5000 ] && [ "$HIGH" -le 900000 ] ||
  fail "DRILL_HIGH must be a whole number from 5000 to 900000"
for tool in bitcoind bitcoin-cli jq; do
  command -v "$tool" >/dev/null || fail "$tool is not on PATH"
done
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) fail "this drill is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

mkdir -p "$DIR"
NODE_PID=
cleanup() {
  if [ -n "$NODE_PID" ]; then
    bitcoin-cli -regtest -datadir="$DIR" -rpcport="$RPC_PORT" stop >/dev/null 2>&1 || true
    # A node in the middle of a wide import does not stop until it is done.
    for _ in $(seq 1 20); do
      kill -0 "$NODE_PID" 2>/dev/null || break
      sleep 1
    done
    kill -9 "$NODE_PID" 2>/dev/null || true
  fi
  if [ "${DRILL_KEEP:-0}" != 1 ]; then rm -rf -- "$DIR"; fi
}
trap cleanup EXIT

bitcoind -regtest -datadir="$DIR" -rpcport="$RPC_PORT" -listen=0 -daemonwait \
  -fallbackfee=0.0001 -blockfilterindex=1 -dbcache=50 >/dev/null
NODE_PID=$(cat "$DIR/regtest/bitcoind.pid")

say "1. Three fresh keys"
declare -a TPRV KEY
for i in 1 2 3; do
  cli -named createwallet wallet_name="key$i" >/dev/null
  TPRV[i]=$(cli -rpcwallet="key$i" listdescriptors true |
    jq -r '.descriptors[0].desc' | grep -o 'tprv[A-Za-z0-9]*' | head -1)
  # Import a descriptor on the BIP48 account path; when Core lists it back
  # without private keys it gives the origin and the account public key.
  probe="pkh(${TPRV[i]}/48h/1h/0h/2h/0/*)"
  probe="$probe#$(cli getdescriptorinfo "$probe" | jq -r '.checksum')"
  cli -rpcwallet="key$i" importdescriptors \
    "[{\"desc\":\"$probe\",\"timestamp\":\"now\",\"range\":[0,0]}]" >/dev/null
  KEY[i]=$(cli -rpcwallet="key$i" listdescriptors | jq -r '.descriptors[].desc' |
    grep -F '/48h/1h/0h/2h]' | sed -E 's|^pkh\((.*)/0/\*\)#.*$|\1|')
  [[ "${KEY[i]}" =~ ^\[[0-9a-f]{8}/48h/1h/0h/2h\]tpub[A-Za-z0-9]+$ ]] || fail "could not read account key $i"
  echo "key $i: ${KEY[i]}"
  cli unloadwallet "key$i" >/dev/null
done

say "2. Wallet descriptor"
BODY="wsh(sortedmulti(2,${KEY[1]}/<0;1>/*,${KEY[2]}/<0;1>/*,${KEY[3]}/<0;1>/*))"
INFO=$(cli getdescriptorinfo "$BODY")
DESC="$BODY#$(jq -r '.checksum' <<<"$INFO")"
echo "$DESC"
[ "$(jq -r '.issolvable' <<<"$INFO")" = true ] || fail "Core says the descriptor is not solvable"
[ "$(jq -r '.multipath_expansion | length' <<<"$INFO")" = 2 ] || fail "Core did not expand <0;1> into two descriptors"
RECV=$(cli getdescriptorinfo "$(jq -r '.multipath_expansion[0]' <<<"$INFO" | sed 's/#.*//')" | jq -r '.descriptor')
CHNG=$(cli getdescriptorinfo "$(jq -r '.multipath_expansion[1]' <<<"$INFO" | sed 's/#.*//')" | jq -r '.descriptor')
ok "Core accepts the multipath descriptor and expands it into receive and change"

core_lines() {
  echo "descriptor $DESC"
  echo "receive-descriptor $RECV"
  echo "change-descriptor $CHNG"
  local i both
  for i in "${INDEXES[@]}"; do
    both=$(cli deriveaddresses "$DESC" "[$i,$i]")
    echo "receive $i $(jq -r '.[0][0]' <<<"$both")"
    echo "change $i $(jq -r '.[1][0]' <<<"$both")"
  done
}

say "3. openbmt-core against Bitcoin Core"
core_lines >"$DIR/core.txt"
if [ "${SKIP_CRATE_CHECK:-0}" = 1 ]; then
  echo "skipped (SKIP_CRATE_CHECK=1)"
else
  DESCRIBE=${DESCRIBE:-cargo run --quiet --locked --manifest-path "$ROOT/Cargo.toml" -p openbmt-core --example describe --}
  $DESCRIBE regtest "$DESC" "${INDEXES[@]}" >"$DIR/crate.txt"
  diff "$DIR/core.txt" "$DIR/crate.txt" || fail "openbmt-core and Bitcoin Core disagree (see the diff above)"
  ok "same checksum, same single-path descriptors, same $((2 * ${#INDEXES[@]})) addresses"
fi

say "4. Pay addresses at indexes ${INDEXES[*]}"
cli -named createwallet wallet_name=miner >/dev/null
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 101 "$MINE" >/dev/null
TOTAL=0
n=0
declare -A ADDR_INDEX
for i in "${INDEXES[@]}"; do
  n=$((n + 1))
  addr=$(grep "^receive $i " "$DIR/core.txt" | cut -d' ' -f3)
  ADDR_INDEX[$addr]=$i
  cli -rpcwallet=miner sendtoaddress "$addr" "0.0$n" >/dev/null
  TOTAL=$((TOTAL + n * 1000000))
  echo "index $i  0.0$n BTC  $addr"
done
cli generatetoaddress 6 "$MINE" >/dev/null
echo "total paid: $TOTAL sat"

found() { # wallet -> the funded indexes the wallet can see, sorted
  cli -rpcwallet="$1" listunspent | jq -r '.[].address' |
    while read -r a; do echo "${ADDR_INDEX[$a]:-?}"; done | sort -n | xargs
}
balance() { sats "$(cli -rpcwallet="$1" getbalance)"; }
import_into() { # wallet, JSON import request
  local res
  res=$(cli -rpcwallet="$1" importdescriptors "$2")
  [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ] || fail "import into $1 failed: $res"
}
watch_wallet() { # name, JSON import request
  cli -named createwallet wallet_name="$1" disable_private_keys=true blank=true >/dev/null
  import_into "$1" "$2"
}
# One import with "timestamp" 0 derives the keys and then rescans the chain.
timed_restore() { # name, JSON import request
  SECONDS=0
  watch_wallet "$1" "$2"
  T_TOTAL=$SECONDS
  T_RESCAN=$(grep -F "[$1] Rescan completed in" "$DIR/regtest/debug.log" | tail -1 |
    sed -E 's/.* in +([0-9]+)ms.*/\1/')
  echo "restore ${T_TOTAL}s in all, of which the rescan took $((T_RESCAN / 1000))s"
}

say "5a. Restore with Core's default range"
watch_wallet restore_default "[{\"desc\":\"$DESC\",\"timestamp\":0,\"active\":true}]"
echo "sees indexes: $(found restore_default)   balance: $(balance restore_default) of $TOTAL sat"
if [ "$(balance restore_default)" -lt "$TOTAL" ]; then
  echo "NOTE  a default import does not find the high indexes; the range must be given"
else
  echo "NOTE  this version of Core found every payment with its default range"
fi
cli unloadwallet restore_default >/dev/null

say "5b. Multipath descriptor, active, explicit range [0, $TOP]"
if [ "${DRILL_SLOW:-1}" = 0 ]; then
  echo "skipped (DRILL_SLOW=0)"
else
  timed_restore restore_range "[{\"desc\":\"$DESC\",\"timestamp\":0,\"active\":true,\"range\":[0,$TOP]}]"
  [ "$(balance restore_range)" = "$TOTAL" ] || fail "explicit range found $(balance restore_range) of $TOTAL sat"
  ok "finds every payment: indexes $(found restore_range)"
  echo "NOTE  slow: Core writes an address-book entry for each of the"
  echo "      $(cli -rpcwallet=restore_range getaddressesbylabel "" | jq length) receive addresses up to the highest one used"
  cli unloadwallet restore_range >/dev/null
fi

# The restored wallet only has to find and spend coins. It never hands out a
# new receive address, so the receive descriptor does not need to be active.
# Core then skips the address-book writes that make 5b slow.
say "5c. Single-path descriptors: receive [0, $TOP] not active, change [0, $MARGIN] active"
timed_restore restore_split "[{\"desc\":\"$RECV\",\"timestamp\":0,\"active\":false,\"range\":[0,$TOP]},{\"desc\":\"$CHNG\",\"timestamp\":0,\"active\":true,\"internal\":true,\"range\":[0,$MARGIN]}]"
[ "$(balance restore_split)" = "$TOTAL" ] || fail "single-path restore found $(balance restore_split) of $TOTAL sat"
ok "finds every payment: indexes $(found restore_split)"

say "5d. Scan the unspent outputs with no wallet"
SECONDS=0
SCAN=$(cli scantxoutset start "[{\"desc\":\"$RECV\",\"range\":[0,$TOP]},{\"desc\":\"$CHNG\",\"range\":[0,$MARGIN]}]")
echo "scan ${SECONDS}s"
[ "$(sats "$(jq -r '.total_amount' <<<"$SCAN")")" = "$TOTAL" ] || fail "scantxoutset found $(jq -r '.total_amount' <<<"$SCAN") BTC"
SCAN_INDEXES=$(jq -r '.unspents[].desc' <<<"$SCAN" | sed -E 's|^[^]]*/0/([0-9]+)\].*$|\1|' | sort -n | xargs)
[ "$SCAN_INDEXES" = "${INDEXES[*]}" ] || fail "scantxoutset named indexes $SCAN_INDEXES"
ok "finds every unspent payment and names its index: $SCAN_INDEXES"

say "6. Spend with two of three keys"
signer_request() { # key number, receive range end -> JSON import request
  local i=$1 top=$2 r c
  # Signer i holds its own private key and the other two public keys.
  r=${RECV%#*}
  # The pattern is quoted: the key starts with "[", which would otherwise
  # be read as a set of characters.
  r=${r/"${KEY[i]}"/"${TPRV[i]}/48h/1h/0h/2h"}
  [ "$r" != "${RECV%#*}" ] || fail "could not place private key $i in the receive descriptor"
  r="$r#$(cli getdescriptorinfo "$r" | jq -r '.checksum')"
  c=${CHNG%#*}
  c=${c/"${KEY[i]}"/"${TPRV[i]}/48h/1h/0h/2h"}
  c="$c#$(cli getdescriptorinfo "$c" | jq -r '.checksum')"
  echo "[{\"desc\":\"$r\",\"timestamp\":\"now\",\"active\":false,\"range\":[0,$top]},{\"desc\":\"$c\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,$MARGIN]}]"
}
SECONDS=0
for i in 1 2 3; do
  cli -named createwallet wallet_name="signer$i" blank=true >/dev/null
  import_into "signer$i" "$(signer_request "$i" "$TOP")"
done
echo "three signer wallets imported in ${SECONDS}s"

DEST=$(cli -rpcwallet=miner getnewaddress)

# Does a signer need the range? Ask a signer that only knows indexes 0 to
# $MARGIN to sign for the coin at index $HIGH.
cli -named createwallet wallet_name=signer1_narrow blank=true >/dev/null
import_into signer1_narrow "$(signer_request 1 "$MARGIN")"
HIGH_ADDR=$(grep "^receive $HIGH " "$DIR/core.txt" | cut -d' ' -f3)
HIGH_COIN=$(cli -rpcwallet=restore_split listunspent 1 9999999 "[\"$HIGH_ADDR\"]" | jq -c '[.[0] | {txid, vout}]')
probe=$(cli -rpcwallet=restore_split -named walletcreatefundedpsbt inputs="$HIGH_COIN" \
  outputs="[{\"$DEST\":0.001}]" add_inputs=false fee_rate=2 | jq -r '.psbt')
sig_count() { cli decodepsbt "$(cli -rpcwallet="$1" walletprocesspsbt "$probe" | jq -r '.psbt')" |
  jq -r '.inputs[0].partial_signatures // {} | length'; }
[ "$(sig_count signer1)" = 1 ] || fail "signer 1 did not sign for index $HIGH"
if [ "$(sig_count signer1_narrow)" = 0 ]; then
  echo "NOTE  a signer wallet imported with range [0, $MARGIN] adds no signature for index $HIGH;"
  echo "      signer wallets need the same receive range as the watch-only wallet"
else
  echo "NOTE  this version of Core signs for index $HIGH from a wallet imported with range [0, $MARGIN]"
fi
cli unloadwallet signer1_narrow >/dev/null

spend() { # signer a, signer b, amount in BTC or "all"
  local a=$1 b=$2 amount=$3 psbt one two combined final txid
  if [ "$amount" = all ]; then
    psbt=$(cli -rpcwallet=restore_split -named sendall recipients="[\"$DEST\"]" fee_rate=2 psbt=true | jq -r '.psbt')
  else
    psbt=$(cli -rpcwallet=restore_split -named walletcreatefundedpsbt outputs="[{\"$DEST\":$amount}]" fee_rate=2 | jq -r '.psbt')
  fi
  one=$(cli -rpcwallet="signer$a" walletprocesspsbt "$psbt")
  [ "$(jq -r '.complete' <<<"$one")" = false ] || fail "one signature completed the transaction"
  if cli finalizepsbt "$(jq -r '.psbt' <<<"$one")" | jq -e '.complete' >/dev/null; then
    fail "a transaction with one signature was finalized"
  fi
  # The two signers work separately on the same unsigned PSBT.
  two=$(cli -rpcwallet="signer$b" walletprocesspsbt "$psbt")
  combined=$(cli combinepsbt "[\"$(jq -r '.psbt' <<<"$one")\",\"$(jq -r '.psbt' <<<"$two")\"]")
  final=$(cli finalizepsbt "$combined")
  [ "$(jq -r '.complete' <<<"$final")" = true ] || fail "keys $a and $b could not complete the transaction"
  txid=$(cli sendrawtransaction "$(jq -r '.hex' <<<"$final")")
  cli generatetoaddress 1 "$MINE" >/dev/null
  [ "$(cli -rpcwallet=restore_split gettransaction "$txid" | jq -r '.confirmations')" -ge 1 ] ||
    fail "spend $txid did not confirm"
  ok "keys $a and $b spent $amount (one key alone could not): $txid"
}
spend 1 2 0.005
spend 1 3 0.005
spend 2 3 0.005
spend 3 1 all
[ "$(balance restore_split)" = 0 ] || fail "wallet not empty after the sweep"
ok "wallet emptied; the change from the first three spends was found and spent"

say "Result"
echo "Bitcoin Core restored and spent the wallet from the descriptor alone,"
echo "with payments up to index $HIGH."
echo "The import needs an explicit range that covers the highest index used,"
echo "and is fast when the receive descriptor is imported as not active."
