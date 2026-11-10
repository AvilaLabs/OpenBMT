#!/usr/bin/env bash
# The signer against Bitcoin Core, on regtest.
#
# Question: does `openbmt-signer` read the transaction files Bitcoin Core
# builds, and does Bitcoin Core accept the files and signatures the signer
# returns?
#
# What it does:
#   1. Starts a private regtest node in a scratch directory, makes three
#      keys and the wallet descriptor, and pays the wallet.
#   2. Core builds a payment. The signer signs first with key 2, Core signs
#      second with key 1, Core finalizes and broadcasts.
#   3. Core builds a fee bump of that payment. The signer counts the two as
#      one (SR-33). Core signs with key 3 and the replacement confirms.
#   4. Core signs first with key 3. The signer checks that signature and
#      adds its own. The same file signed again gives the same bytes.
#   5. A payment above the per-transaction ceiling is refused (SR-35).
#   6. A PSBT version 2 file, which Core 32 writes by default, is refused
#      (SR-01).
#
# Needs: bitcoind and bitcoin-cli (Bitcoin Core 32.0 or newer), jq and
# base64 on PATH, and cargo unless SIGNER names a prebuilt `regtest_sign`
# example binary.
#
# Environment:
#   DRILL_DIR       scratch directory (default: a new temporary directory)
#   DRILL_RPC_PORT  RPC port for the regtest node (default 18944)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#   SIGNER          command that runs the `regtest_sign` example
#
# Everything here is regtest. No real keys or funds are involved.

set -euo pipefail

RPC_PORT=${DRILL_RPC_PORT:-18944}
DIR=${DRILL_DIR:-$(mktemp -d)}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SIGNER=${SIGNER:-cargo run --quiet --locked --manifest-path "$ROOT/Cargo.toml" -p openbmt-signer --example regtest_sign --}

say() { printf '\n== %s\n' "$*"; }
ok() { printf 'PASS  %s\n' "$*"; }
fail() {
  printf 'FAIL  %s\n' "$*" >&2
  exit 1
}
cli() { bitcoin-cli -regtest -datadir="$DIR" -rpcport="$RPC_PORT" "$@"; }
sats() { jq -n --arg v "$1" '$v | tonumber * 100000000 | round'; }
field() { awk -v k="$1" -v n="$2" '$1 == k { print $n; exit }'; }

for tool in bitcoind bitcoin-cli jq base64; do
  command -v "$tool" >/dev/null || fail "$tool is not on PATH"
done
VERSION=$(bitcoind --version | head -1)
case "$VERSION" in
  "Bitcoin Core daemon version"*) ;;
  *) fail "this check is for Bitcoin Core; found: $VERSION" ;;
esac
say "$VERSION"

mkdir -p "$DIR"
NODE_PID=
cleanup() {
  if [ -n "$NODE_PID" ]; then
    bitcoin-cli -regtest -datadir="$DIR" -rpcport="$RPC_PORT" stop >/dev/null 2>&1 || true
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
  -fallbackfee=0.0001 -dbcache=50 >/dev/null
NODE_PID=$(cat "$DIR/regtest/bitcoind.pid")

say "1. Keys, wallet and coins"
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
  cli unloadwallet "key$i" >/dev/null
done
BODY="wsh(sortedmulti(2,${KEY[1]}/<0;1>/*,${KEY[2]}/<0;1>/*,${KEY[3]}/<0;1>/*))"
INFO=$(cli getdescriptorinfo "$BODY")
DESC="$BODY#$(jq -r '.checksum' <<<"$INFO")"
RECV=$(cli getdescriptorinfo "$(jq -r '.multipath_expansion[0]' <<<"$INFO" | sed 's/#.*//')" | jq -r '.descriptor')
CHNG=$(cli getdescriptorinfo "$(jq -r '.multipath_expansion[1]' <<<"$INFO" | sed 's/#.*//')" | jq -r '.descriptor')
echo "$DESC"

import_into() { # wallet, JSON import request
  local res
  res=$(cli -rpcwallet="$1" importdescriptors "$2")
  [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ] || fail "import into $1 failed: $res"
}
# The shop's wallet in Core: it watches and builds, and holds no key.
cli -named createwallet wallet_name=shop disable_private_keys=true blank=true >/dev/null
import_into shop "[{\"desc\":\"$DESC\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,99]}]"
# Keys 1 and 3 as Core wallets: each holds its own private key and the
# other two public keys.
core_signer() { # key number
  local i=$1 r c
  r=${RECV%#*}
  r=${r/"${KEY[i]}"/"${TPRV[i]}/48h/1h/0h/2h"}
  [ "$r" != "${RECV%#*}" ] || fail "could not place private key $i in the receive descriptor"
  r="$r#$(cli getdescriptorinfo "$r" | jq -r '.checksum')"
  c=${CHNG%#*}
  c=${c/"${KEY[i]}"/"${TPRV[i]}/48h/1h/0h/2h"}
  c="$c#$(cli getdescriptorinfo "$c" | jq -r '.checksum')"
  cli -named createwallet wallet_name="signer$i" blank=true >/dev/null
  import_into "signer$i" "[{\"desc\":\"$r\",\"timestamp\":\"now\",\"active\":false,\"range\":[0,99]},{\"desc\":\"$c\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,99]}]"
}
core_signer 1
core_signer 3

cli -named createwallet wallet_name=miner >/dev/null
MINE=$(cli -rpcwallet=miner getnewaddress)
DEST=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 101 "$MINE" >/dev/null
for _ in 1 2 3; do
  cli -rpcwallet=miner sendtoaddress "$(cli -rpcwallet=shop getnewaddress)" 0.01 >/dev/null
done
cli generatetoaddress 1 "$MINE" >/dev/null
[ "$(sats "$(cli -rpcwallet=shop getbalance)")" = 3000000 ] || fail "the shop wallet does not see its coins"
ok "three coins of 0.01 BTC in the wallet"

STATE="$DIR/key2.state"
N=0
# Run the signer on a base64 PSBT. Sets REPORT (its lines), CODE (its exit
# code) and, when it signed, OUT (the returned PSBT in base64).
key2_sign() {
  N=$((N + 1))
  base64 -d <<<"$1" >"$DIR/in$N.psbt"
  CODE=0
  REPORT=$($SIGNER "$STATE" "$DESC" "${TPRV[2]}" "$DIR/in$N.psbt" "$DIR/out$N.psbt" "$(date +%s)") || CODE=$?
  printf '%s\n' "$REPORT" | sed 's/^/      /'
  OUT=
  if [ "$CODE" = 0 ]; then OUT=$(base64 -w0 "$DIR/out$N.psbt"); fi
}
sig_counts() { cli decodepsbt "$1" | jq -c '[.inputs[] | (.partial_signatures // {} | length)]'; }
# Core 32 writes PSBT version 2 unless it is asked for version 0. The signer
# reads version 0 only (SR-01), so every file here is asked for as version 0.
build() { # amount in BTC, PSBT version (default 0)
  cli -rpcwallet=shop -named walletcreatefundedpsbt outputs="[{\"$DEST\":$1}]" fee_rate=2 \
    psbt_version="${2:-0}" | jq -r '.psbt'
}
finish() { # PSBT with two signatures per input -> txid, broadcast
  local final
  final=$(cli finalizepsbt "$1")
  [ "$(jq -r '.complete' <<<"$final")" = true ] || fail "Core could not finalize: $(cli analyzepsbt "$1")"
  cli sendrawtransaction "$(jq -r '.hex' <<<"$final")"
}

say "2. Core builds, the signer signs first, Core signs second"
PSBT=$(build 0.0005)
CORE_FEE=$(sats "$(cli decodepsbt "$PSBT" | jq -r '.fee')")
key2_sign "$PSBT"
[ "$CODE" = 0 ] || fail "the signer refused a payment Core built"
[ "$(field fee 2 <<<"$REPORT")" = "$CORE_FEE" ] || fail "the signer and Core disagree about the fee"
grep -q "^output [0-9]* 50000 $DEST other\$" <<<"$REPORT" || fail "the payment is not shown as an other destination"
grep -q '^output [0-9]* [0-9]* [a-z0-9]* change [0-9]*$' <<<"$REPORT" || fail "Core's change output was not accepted as change"
FEE1=$(field fee 2 <<<"$REPORT")
[ "$(field totals 2 <<<"$REPORT")" = $((50000 + FEE1)) ] || fail "the day total is not the payment plus the fee"
ok "the signer read Core's file: same fee, payment and change recognized"
[ "$(sig_counts "$OUT" | jq -c 'unique')" = "[1]" ] || fail "Core does not see one signature per input in the signer's file"
if cli finalizepsbt "$OUT" | jq -e '.complete' >/dev/null; then
  fail "a transaction with one signature was finalized"
fi
TWO=$(cli -rpcwallet=signer1 walletprocesspsbt "$OUT" true ALL true false | jq -r '.psbt')
[ "$(sig_counts "$TWO" | jq -c 'unique')" = "[2]" ] || fail "Core's key 1 did not add its signature to the signer's file"
TXID1=$(finish "$TWO")
ok "Core read the signer's file, added key 1, finalized and broadcast: $TXID1"

say "3. A fee bump counts once"
BUMP=$(cli -rpcwallet=shop -named psbtbumpfee txid="$TXID1" fee_rate=10 psbt_version=0 | jq -r '.psbt')
key2_sign "$BUMP"
[ "$CODE" = 0 ] || fail "the signer refused Core's fee bump"
FEE2=$(field fee 2 <<<"$REPORT")
[ "$FEE2" -gt "$FEE1" ] || fail "the bump does not pay more"
[ "$(field log-entries 2 <<<"$REPORT")" = 1 ] || fail "the bump was logged as a second transaction"
[ "$(field totals 2 <<<"$REPORT")" = $((50000 + FEE2)) ] || fail "the day total is not the payment plus the larger fee"
TWO=$(cli -rpcwallet=signer3 walletprocesspsbt "$OUT" true ALL true false | jq -r '.psbt')
TXID2=$(finish "$TWO")
cli generatetoaddress 1 "$MINE" >/dev/null
[ "$(cli -rpcwallet=shop gettransaction "$TXID2" | jq -r '.confirmations')" -ge 1 ] || fail "the replacement did not confirm"
[ "$(cli -rpcwallet=shop gettransaction "$TXID1" | jq -r '.confirmations')" -lt 0 ] || fail "the first payment was not replaced"
ok "payment and bump counted once ($((50000 + FEE2)) sat); keys 2 and 3 replaced it: $TXID2"

say "4. Core signs first, the signer second"
PSBT=$(build 0.0003)
ONE=$(cli -rpcwallet=signer3 walletprocesspsbt "$PSBT" true ALL true false | jq -r '.psbt')
[ "$(sig_counts "$ONE" | jq -c 'unique')" = "[1]" ] || fail "Core's key 3 did not sign"
key2_sign "$ONE"
[ "$CODE" = 0 ] || fail "the signer refused a file that carries Core's signature"
[ "$(sig_counts "$OUT" | jq -c 'unique')" = "[2]" ] || fail "the signer's file does not carry both signatures"
FIRST=$OUT
key2_sign "$ONE"
[ "$CODE" = 0 ] || fail "the signer refused to sign the same file again"
[ "$(field already-counted 2 <<<"$REPORT")" = yes ] || fail "the same transaction was counted twice"
[ "$OUT" = "$FIRST" ] || fail "the same file did not give the same bytes"
TXID3=$(finish "$OUT")
cli generatetoaddress 1 "$MINE" >/dev/null
[ "$(cli -rpcwallet=shop gettransaction "$TXID3" | jq -r '.confirmations')" -ge 1 ] || fail "the payment did not confirm"
ok "the signer checked Core's signature and added its own; signing again gave the same bytes: $TXID3"

say "5. A payment above the ceiling"
key2_sign "$(build 0.002)"
[ "$CODE" = 2 ] || fail "the signer did not refuse 200,000 sat to another destination"
grep -q '^refused SR-35: ' <<<"$REPORT" || fail "the refusal does not name SR-35"
ok "200,000 sat to another destination refused under SR-35"

say "6. A PSBT version 2 file"
key2_sign "$(build 0.0002 2)"
[ "$CODE" = 2 ] || fail "the signer did not refuse a PSBT version 2 file"
grep -q '^refused SR-01: ' <<<"$REPORT" || fail "the refusal does not name SR-01"
ok "the PSBT version 2 file Core writes by default is refused under SR-01"

say "Result"
echo "The signer read every file Bitcoin Core built, and Bitcoin Core read"
echo "every file the signer returned. Transactions signed by key 2 with key 1"
echo "or key 3 confirmed. A fee bump was counted once."
