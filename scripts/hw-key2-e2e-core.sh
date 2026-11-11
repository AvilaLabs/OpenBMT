#!/usr/bin/env bash
# Key 2 on a hardware signer, against Bitcoin Core on regtest.
#
# Question: does the file the owner phone writes for a hardware signer carry
# everything a signer reads, and does what comes back by each path finish,
# check, broadcast and confirm?
#
# Key 2 is played by a Core descriptor wallet that holds key 2's private
# descriptors (`walletprocesspsbt` is the signer). The owner is the real
# `openbmt-owner` through the example `hw_key2`: it queues a cash-out, builds
# the session for a hardware signer (the spend apart from the Key app's
# request), signs with key 1 and writes the file for the signer.
#
#   1. Starts a private regtest node, imports the 2-of-3 wallet into a
#      blank watch-only Core wallet and key 2's private descriptors into
#      another, and funds six receive addresses.
#   2. Four cash-outs, one for each way back. Each time: the file the owner
#      wrote is decoded by Core (`decodepsbt`: witness and previous
#      transaction, witness script, three key derivations on every input, the
#      three global extended keys, SIGHASH_ALL) and analysed
#      (`analyzepsbt`), the Key app's request holds no spend, and Core signs
#      as key 2:
#        - file:  the signed PSBT as a binary file;
#        - bbqr:  the signed PSBT through BBQr parts, read in reverse order;
#        - ur:    the signed PSBT through animated crypto-psbt parts;
#        - txn:   Core finalizes (key 1 and key 2 signatures) and returns the
#                 transaction, which the owner checks and takes.
#      The owner finishes, broadcasts through Core and a block confirms it.
#   3. Refusals: the file as it was given (only key 1 signed it) is refused
#      as "this signer did not sign", and a transaction of another cash-out
#      is refused as another payment; nothing changes either time.
#
# Everything here is regtest on 127.0.0.1. The keys come from fixed test
# bytes: TEST ONLY, NEVER USE. The node and the scratch directory are removed
# on every exit. The default Bitcoin data directory is never touched.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_RPC_PORT:-18981}
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/hw-key2-e2e-XXXXXX")}
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

FAILS=0
refused_with() { [ "$1" = 2 ] && grep -q "^refused $2" "$3"; }
chk() {
  local what=$1
  shift
  if "$@"; then note "ok:     $what"; else note "NOT ok: $what"; FAILS=$((FAILS + 1)); fi
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

if [ -z "${HW_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/hwkey2}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner --example hw_key2 ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner --example hw_key2 ||
      die "the build failed"
  fi
  HW_BIN="$TARGET/debug/examples/hw_key2"
fi
[ -x "$HW_BIN" ] || die "no example binary at $HW_BIN"
ex() { "$HW_BIN" "$@"; }

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" -listen=0 -daemonwait \
  -fallbackfee=0.0001 -txindex=1 -dbcache=50 >/dev/null || die "bitcoind did not start"
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
[ -n "$NODE_PID" ] || die "no node pid"

# ---- 1 -------------------------------------------------------------------------

say "1. The wallet in Core: watch-only, and key 2's private descriptors; six coins"
KEYS="$WORK/keys.txt"
ex keys >"$KEYS" || die "the example could not make the keys"
DESC=$(sed -n 's/^descriptor //p' "$KEYS")
[ -n "$DESC" ] || die "no descriptor from the example"
mapfile -t RADDR < <(sed -n 's/^receive [0-9]* //p' "$KEYS")

cli -named createwallet wallet_name=shop disable_private_keys=true blank=true >/dev/null || die "no shop wallet"
res=$(cli -rpcwallet=shop importdescriptors "[{\"desc\":\"$DESC\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,99]}]")
chk "Core imported the multipath description" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]

cli -named createwallet wallet_name=core2 blank=true >/dev/null || die "no core2 wallet"
r=$(sed -n 's/^private-receive 2 //p' "$KEYS")
c=$(sed -n 's/^private-change 2 //p' "$KEYS")
r="$r#$(cli getdescriptorinfo "$r" | jq -r '.checksum')"
c="$c#$(cli getdescriptorinfo "$c" | jq -r '.checksum')"
res=$(cli -rpcwallet=core2 importdescriptors "[{\"desc\":\"$r\",\"timestamp\":\"now\",\"active\":false,\"range\":[0,99]},{\"desc\":\"$c\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,99]}]")
chk "the Core wallet for key 2 holds its private descriptors" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]

cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 101 "$MINE" >/dev/null || die "no blocks"
for i in 0 1 2 3 4 5; do
  cli -rpcwallet=miner sendtoaddress "${RADDR[$i]}" "$(btc 300000)" >/dev/null || die "funding $i failed"
  cli generatetoaddress 1 "$MINE" >/dev/null
done
cli generatetoaddress 6 "$MINE" >/dev/null
have=$(cli -rpcwallet=shop listunspent 1 | jq -c '[.[] | .amount * 100000000 | round] | sort')
chk "the watch-only wallet holds six coins of 300000 sat ($have)" \
  [ "$have" = "[300000,300000,300000,300000,300000,300000]" ]

make_coins() { # file
  : >"$1"
  cli -rpcwallet=shop listunspent 1 |
    jq -r '.[] | .desc as $d | ($d | capture("/(?<c>[01])/(?<i>[0-9]+)\\]")) as $m
           | "\(.txid) \(.vout) \(.confirmations) \($m.c) \($m.i)"' |
    while read -r txid vout conf chain idx; do
      echo "$txid $vout $conf $chain $idx $(cli getrawtransaction "$txid")" >>"$1"
    done
}

# phase NAME INDEX: one cash-out, its file, Core as the signer, the way back.
phase() {
  local name=$1 index=$2 dir="$WORK/$1" dest tip out
  say "2. $name: a cash-out of 100,000 sat, the file for the signer, and the way back by $name"
  mkdir -p "$dir"
  dest=$(cli -rpcwallet=miner getnewaddress)
  make_coins "$dir/coins.txt"
  tip=$(cli getblockcount)
  out=$(ex export --dir "$dir" --coins "$dir/coins.txt" --dest "$dest" --sats 100000 \
    --rate "$RATE" --lock-height "$tip" --change-index "$index" --now "$((1700000000 + index))")
  local code=$?
  sed 's/^/      /' <<<"$out"
  chk "the owner wrote the file for the signer" [ "$code" = 0 ]
  local txid
  txid=$(sed -n 's/^txid //p' <<<"$out")
  chk "the Key app's request holds no spend (AS-90)" grep -qx 'key-app-spends 0' <<<"$out"
  chk "the stored file is given again unchanged" grep -qx 'same-again true' <<<"$out"

  # What Core reads in the file.
  local b64 dec
  b64=$(base64 -w0 "$dir/file.psbt")
  dec=$(cli decodepsbt "$b64")
  chk "Core decodes it, and it holds the unsigned transaction $txid" \
    [ "$(jq -r '.tx.txid' <<<"$dec")" = "$txid" ]
  chk "every input has the witness output, the previous transaction and the witness script" \
    [ "$(jq -r 'all(.inputs[]; has("witness_utxo") and has("non_witness_utxo") and has("witness_script"))' <<<"$dec")" = true ]
  chk "every input names the three keys (BIP 32 derivations)" \
    [ "$(jq -r 'all(.inputs[]; (.bip32_derivs | length) == 3)' <<<"$dec")" = true ]
  chk "every input has key 1's signature and hash type ALL" \
    [ "$(jq -r 'all(.inputs[]; (.partial_signatures | length) == 1 and .sighash == "ALL")' <<<"$dec")" = true ]
  chk "the file names the three keys globally" [ "$(jq -r '.global_xpubs | length' <<<"$dec")" = 3 ]
  chk "the change output names its script and three keys" \
    [ "$(jq -r '[.outputs[] | select(has("witness_script"))] | all(.[]; (.bip32_derivs | length) == 3)' <<<"$dec")" = true ]
  chk "the PSBT is version 0" [ "$(jq -r '.psbt_version' <<<"$dec")" = 0 ]
  chk "Core's analyzepsbt says a signer is next" \
    [ "$(cli analyzepsbt "$b64" | jq -r '.next')" = signer ]

  # Core as key 2.
  local signed fin
  signed=$(cli -rpcwallet=core2 walletprocesspsbt "$b64" true ALL true false)
  chk "Core signs as key 2" [ "$(jq -r '.psbt' <<<"$signed" | wc -c)" -gt 100 ]
  local signed_b64
  signed_b64=$(jq -r '.psbt' <<<"$signed")

  local input="$dir/back.psbt" kind=$name
  case "$name" in
    file | bbqr | ur) base64 -d <<<"$signed_b64" >"$input" ;;
    txn)
      fin=$(cli -rpcwallet=core2 walletprocesspsbt "$b64" true ALL true true)
      chk "Core finalizes with key 1's and key 2's signatures" [ "$(jq -r '.complete' <<<"$fin")" = true ]
      jq -r '.hex' <<<"$fin" >"$input"
      ;;
  esac

  # The refusals come first: what was given back unsigned, nothing changes.
  if [ "$name" = file ]; then
    ex take --dir "$dir" --kind file --in "$dir/file.psbt" --now 1700001000 >"$dir/refusal.txt"
    code=$?
    chk "the file as it was given is refused (this signer did not sign): $(cat "$dir/refusal.txt")" \
      refused_with "$code" AS-95 "$dir/refusal.txt"
  fi

  out=$(ex take --dir "$dir" --kind "$kind" --in "$input" --now 1700001001)
  code=$?
  sed 's/^/      /' <<<"$out"
  chk "the owner took it back and finished it" [ "$code" = 0 ]
  chk "the finished transaction has the ID of the file" [ "$(sed -n 's/^txid //p' <<<"$out" | tail -1)" = "$txid" ]
  chk "the session ended" grep -qx 'session-closed true' <<<"$out"

  local final
  final=$(cat "$dir/final.hex")
  chk "Core accepts the transaction" \
    [ "$(cli testmempoolaccept "[\"$final\"]" | jq -r '.[0].allowed')" = true ]
  local sent
  sent=$(cli sendrawtransaction "$final")
  chk "Core took it ($sent)" [ "$sent" = "$txid" ]
  cli generatetoaddress 6 "$MINE" >/dev/null
  chk "a block confirmed it" [ "$(cli getrawtransaction "$txid" true | jq -r '.confirmations')" -ge 6 ]
  chk "the destination has the coin" \
    [ "$(cli -rpcwallet=miner getreceivedbyaddress "$dest" | jq -r '. * 100000000 | round')" = 100000 ]
}

phase file 0
phase bbqr 1
phase ur 2
phase txn 3

# ---- 3 -------------------------------------------------------------------------

say "3. Another payment's transaction is refused"
dir="$WORK/other"
mkdir -p "$dir"
dest=$(cli -rpcwallet=miner getnewaddress)
make_coins "$dir/coins.txt"
tip=$(cli getblockcount)
ex export --dir "$dir" --coins "$dir/coins.txt" --dest "$dest" --sats 70000 --rate "$RATE" \
  --lock-height "$tip" --change-index 4 --now 1700000100 >"$dir/export.txt" || die "export failed"
# The signed PSBT of the first phase's file belongs to a transaction that is spent already.
ex take --dir "$dir" --kind file --in "$WORK/file/back.psbt" --now 1700001100 >"$dir/refusal.txt"
code=$?
chk "a signed PSBT of another cash-out is refused as another payment: $(cat "$dir/refusal.txt")" \
  refused_with "$code" AS-94 "$dir/refusal.txt"

echo
if [ "$FAILS" = 0 ]; then
  echo "PASS  the file for a hardware signer reads, signs and finishes by every path"
  exit 0
fi
echo "FAIL  $FAILS check(s) did not hold"
exit 1
