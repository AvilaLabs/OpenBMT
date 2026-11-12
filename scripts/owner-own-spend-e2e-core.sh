#!/usr/bin/env bash
# The spends of a shop with its own wallet against Bitcoin Core, on regtest
# (docs/SESSION-RULES.md section 6, AS-100 to AS-107).
#
# Question: does the file the owner phone writes for the shop's own wallet
# read, sign and finish in Bitcoin Core, does what comes back (a signed file,
# a finished file, a finished transaction) finish into the transaction Core
# accepts, does a wallet that sends the payment itself get its entries paid
# when the block comes, does a merge of twelve small coins work at a low fee,
# and is a file signed by the wrong key refused?
#
# Bitcoin Core plays the shop's wallet (a descriptor wallet and
# `walletprocesspsbt`) for two wallets, one after the other:
#   single  a wallet with one key (SP-82 file), the key private in Core;
#   multi   a 2-of-3 wallet of which Core holds two keys (SP-45 file, no
#           signature of the owner's: the owner phone holds no key).
# The owner is the real `openbmt-owner` through the example `own_spend`.
#
# For each wallet:
#   1. A private regtest node holds the wallet in Core and 18 funded coins:
#      six of 300,000 sat and twelve of 6,000 sat.
#   2. A refund and a cash-out are queued and built together (AS-101); the
#      files are decoded and analysed by Core (AS-102, the same file again);
#      Core signs without finishing; the owner takes the files back, finishes
#      them (AS-103, AS-104); `testmempoolaccept` allows each; they confirm.
#   3. A cash-out whose file Core signs AND finishes: the owner takes the
#      finished file; and another whose finished transaction (hex) comes back.
#   4. A cash-out whose wallet sends the transaction itself and never
#      returns the file: when the block holds it the owner marks its entries
#      paid (AS-105), the state reads back (AS-107).
#   5. The merge of twelve small coins at a low fee (AS-106): one output, the
#      wallet's own change script; Core signs, the owner finishes, Core
#      allows it at no less than the asked rate.
#   6. A file signed by the wrong key, and a file as it was given (unsigned),
#      are refused and nothing changes; the right signature is taken after.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (put first on
# PATH; set CORE_BIN to use another; the bitcoind in /usr/local/bin is never
# used), jq, base64 and cargo unless OWN_SPEND_BIN names a built example.
#
# Environment:
#   CORE_BIN        directory with bitcoind and bitcoin-cli
#   DRILL_DIR       scratch directory (default: made new under ~/.cache/openbmt)
#   DRILL_RPC_PORT  RPC port for the regtest node (default 18997)
#   DRILL_KEEP=1    keep the scratch directory afterwards
#   OWN_SPEND_BIN   the `own_spend` example binary
#
# Everything here is regtest on 127.0.0.1. The keys come from fixed test
# bytes: TEST ONLY, NEVER USE. The node and the scratch directory are removed
# on every exit. The default Bitcoin data directory is never touched.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_RPC_PORT:-18997}
mkdir -p "$HOME/.cache/openbmt"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/own-spend-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/core"
WORK="$BASE/work"
RATE=2500 # satoshis per 1000 virtual bytes
LOW=1000  # the low rate of the merge

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
refused_with() { [ "$1" = 2 ] && grep -q "^refused $2" "$3"; }

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

if [ -z "${OWN_SPEND_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/own-spend}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner --example own_spend ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner --example own_spend ||
      die "the build failed"
  fi
  OWN_SPEND_BIN="$TARGET/debug/examples/own_spend"
fi
[ -x "$OWN_SPEND_BIN" ] || die "no example binary at $OWN_SPEND_BIN"
ex() { "$OWN_SPEND_BIN" "$@"; }

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" -listen=0 -daemonwait \
  -fallbackfee=0.0001 -txindex=1 -dbcache=50 >/dev/null || die "bitcoind did not start"
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
[ -n "$NODE_PID" ] || die "no node pid"

cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 101 "$MINE" >/dev/null || die "no blocks"

# coins FILE WALLET [SATS]: one line for each coin of the wallet (of SATS
# satoshis, when given): txid vout confirmations chain index transaction-hex.
make_coins() {
  : >"$1"
  cli -rpcwallet="$2" listunspent 1 |
    jq -r --argjson s "${3:-0}" '.[] | select($s == 0 or (.amount * 100000000 | round) == $s)
           | .desc as $d | ($d | capture("/(?<c>[01])/(?<i>[0-9]+)\\]")) as $m
           | "\(.txid) \(.vout) \(.confirmations) \($m.c) \($m.i)"' |
    while read -r txid vout conf chain idx; do
      echo "$txid $vout $conf $chain $idx $(cli getrawtransaction "$txid")" >>"$1"
    done
}

# accept_send TXHEX TXID RATE: every finished transaction goes through
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

# item_lines OUT: the item numbers a build printed.
item_numbers() { sed -n 's/^item \([0-9]*\) txid .*/\1/p' <<<"$1"; }
item_txid() { sed -n "s/^item $2 txid \([0-9a-f]*\) .*/\1/p" <<<"$1"; }

run_kind() {
  local KIND=$1 SHOP SIGNER
  say "=== wallet: $KIND ==="
  ex keys --kind "$KIND" >"$WORK/keys-$KIND.txt" || die "no keys from the example"
  local DESC R C
  DESC=$(sed -n 's/^descriptor //p' "$WORK/keys-$KIND.txt")
  R=$(sed -n 's/^private-receive //p' "$WORK/keys-$KIND.txt")
  C=$(sed -n 's/^private-change //p' "$WORK/keys-$KIND.txt")
  R="$R#$(cli getdescriptorinfo "$R" | jq -r '.checksum')"
  C="$C#$(cli getdescriptorinfo "$C" | jq -r '.checksum')"
  mapfile -t RADDR < <(sed -n 's/^receive [0-9]* //p' "$WORK/keys-$KIND.txt")
  mapfile -t CADDR < <(sed -n 's/^change [0-9]* //p' "$WORK/keys-$KIND.txt")
  SIGNER="signer_$KIND"
  cli -named createwallet wallet_name="$SIGNER" blank=true >/dev/null || die "no signer wallet"
  local res
  if [ "$KIND" = single ]; then
    SHOP=$SIGNER
    res=$(cli -rpcwallet="$SIGNER" importdescriptors "[{\"desc\":\"$R\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,99]},{\"desc\":\"$C\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,99]}]")
  else
    SHOP="shop_$KIND"
    cli -named createwallet wallet_name="$SHOP" disable_private_keys=true blank=true >/dev/null || die "no shop wallet"
    local w
    w=$(cli -rpcwallet="$SHOP" importdescriptors "[{\"desc\":\"$DESC\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,99]}]")
    chk "Core imported the multipath description of the 2-of-3 wallet" [ "$(jq -r 'all(.[]; .success)' <<<"$w")" = true ]
    res=$(cli -rpcwallet="$SIGNER" importdescriptors "[{\"desc\":\"$R\",\"timestamp\":\"now\",\"active\":false,\"range\":[0,99]},{\"desc\":\"$C\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,99]}]")
  fi
  chk "the signing wallet holds its private descriptors" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]
  local i
  for i in 0 1 2 3 4 5; do
    cli -rpcwallet=miner sendtoaddress "${RADDR[$i]}" "$(btc 300000)" >/dev/null || die "funding $i failed"
    cli generatetoaddress 1 "$MINE" >/dev/null
  done
  for i in 8 9 10 11 12 13 14 15 16 17 18 19; do
    cli -rpcwallet=miner sendtoaddress "${RADDR[$i]}" "$(btc 6000)" >/dev/null || die "funding $i failed"
  done
  cli generatetoaddress 8 "$MINE" >/dev/null
  chk "the wallet holds six coins of 300000 and twelve of 6000 sat" \
    [ "$(cli -rpcwallet="$SHOP" listunspent 1 | jq -c '[.[] | .amount * 100000000 | round] | sort | [(map(select(. == 300000)) | length), (map(select(. == 6000)) | length)]')" = "[6,12]" ]

  local NOW=1700000000 mark out code dest dir tip b64 dec signed txid hexs n
  # ---- a refund and a cash-out -------------------------------------------------
  say "$KIND 2. A refund and a cash-out, built together, signed by Core, taken back"
  mark=$FAILS
  dir="$WORK/$KIND-a"
  dest=$(cli -rpcwallet=miner getnewaddress)
  local dest2
  dest2=$(cli -rpcwallet=miner getnewaddress)
  ex queue --dir "$dir" --kind "$KIND" --what refund --sale 1 --addr "$dest" --sats 40000 >"$dir.q1" || die "queue refund"
  ex queue --dir "$dir" --kind "$KIND" --what cash --addr "$dest2" --sats 100000 >"$dir.q2" || die "queue cash-out"
  make_coins "$dir/coins.txt" "$SHOP" 300000
  tip=$(cli getblockcount)
  out=$(ex build --dir "$dir" --kind "$KIND" --coins "$dir/coins.txt" --rate "$RATE" \
    --lock-height "$tip" --change-index 0 --now "$NOW" --picks 1,2)
  code=$?
  sed 's/^/      /' <<<"$out"
  chk "the owner froze the spends" [ "$code" = 0 ]
  chk "nothing was skipped" bash -c "! grep -q '^skipped' <<<\"\$1\"" _ "$out"
  chk "the same file is given again unchanged" bash -c "! grep -q 'same-again false' <<<\"\$1\"" _ "$out"
  rule AS-101 "$mark" "$KIND: the queued refund and cash-out are frozen into one session of files"
  mark=$FAILS
  for n in $(item_numbers "$out"); do
    txid=$(item_txid "$out" "$n")
    b64=$(base64 -w0 "$dir/item-$n.psbt")
    dec=$(cli decodepsbt "$b64")
    chk "item $n: Core decodes it and it holds the unsigned transaction $txid" [ "$(jq -r '.tx.txid' <<<"$dec")" = "$txid" ]
    chk "item $n: every input has its previous output and SIGHASH_ALL" \
      [ "$(jq -r 'all(.inputs[]; (has("witness_utxo") or has("non_witness_utxo")) and .sighash == "ALL")' <<<"$dec")" = true ]
    if [ "$KIND" = multi ]; then
      chk "item $n: every input names its witness script and three keys" \
        [ "$(jq -r 'all(.inputs[]; has("witness_script") and (.bip32_derivs | length) == 3)' <<<"$dec")" = true ]
      chk "item $n: no signature yet (the owner phone holds no key)" \
        [ "$(jq -r 'all(.inputs[]; (.partial_signatures // {} | length) == 0)' <<<"$dec")" = true ]
    else
      chk "item $n: one key-path hint on every input" [ "$(jq -r 'all(.inputs[]; (.bip32_derivs | length) == 1)' <<<"$dec")" = true ]
    fi
    chk "item $n: Core's analyzepsbt says a signer is next" [ "$(cli analyzepsbt "$b64" | jq -r '.next')" = signer ]
  done
  rule AS-102 "$mark" "$KIND: the file reads in Core with what a wallet needs, and comes again the same"
  mark=$FAILS
  for n in $(item_numbers "$out"); do
    txid=$(item_txid "$out" "$n")
    b64=$(base64 -w0 "$dir/item-$n.psbt")
    signed=$(cli -rpcwallet="$SIGNER" walletprocesspsbt "$b64" true ALL true false)
    chk "item $n: Core signed and did not finish" [ "$(jq -r '.complete' <<<"$signed")" = false ]
    jq -r '.psbt' <<<"$signed" >"$dir/back-$n.psbt"
    o=$(ex take --dir "$dir" --kind "$KIND" --number "$n" --in "$dir/back-$n.psbt" --now $((NOW + 100)))
    code=$?
    sed 's/^/      /' <<<"$o"
    chk "item $n: the owner took it back and finished it" [ "$code" = 0 ]
    chk "item $n: the finished transaction is the file's" [ "$(sed -n 's/^txid //p' <<<"$o" | tail -1)" = "$txid" ]
    hexs=$(cat "$dir/final-$n.hex")
    accept_send "$hexs" "$txid" "$RATE"
  done
  rule AS-103 "$mark" "$KIND: a signed file is read, judged and finished"
  mark=$FAILS
  chk "the refund's destination holds 40000 sat" \
    [ "$(cli -rpcwallet=miner getreceivedbyaddress "$dest" | jq -r '. * 100000000 | round')" = 40000 ]
  chk "the cash-out's destination holds 100000 sat" \
    [ "$(cli -rpcwallet=miner getreceivedbyaddress "$dest2" | jq -r '. * 100000000 | round')" = 100000 ]
  rule AS-104 "$mark" "$KIND: the finished transactions went out through Core and paid what was queued"

  # ---- finished by the wallet --------------------------------------------------
  say "$KIND 3. A file Core signed AND finished, and a finished transaction"
  mark=$FAILS
  local variant
  for variant in psbt hex; do
    dir="$WORK/$KIND-b-$variant"
    dest=$(cli -rpcwallet=miner getnewaddress)
    ex queue --dir "$dir" --kind "$KIND" --what cash --addr "$dest" --sats 70000 >/dev/null || die "queue"
    make_coins "$dir/coins.txt" "$SHOP" 300000
    tip=$(cli getblockcount)
    out=$(ex build --dir "$dir" --kind "$KIND" --coins "$dir/coins.txt" --rate "$RATE" \
      --lock-height "$tip" --change-index 1 --now "$NOW" --picks 1)
    code=$?
    chk "$variant: the owner froze the spend" [ "$code" = 0 ]
    n=$(item_numbers "$out" | head -1)
    txid=$(item_txid "$out" "$n")
    b64=$(base64 -w0 "$dir/item-$n.psbt")
    signed=$(cli -rpcwallet="$SIGNER" walletprocesspsbt "$b64" true ALL true true)
    chk "$variant: Core signed and finished" [ "$(jq -r '.complete' <<<"$signed")" = true ]
    if [ "$variant" = psbt ]; then
      jq -r '.psbt' <<<"$signed" >"$dir/back"
    else
      jq -r '.hex' <<<"$signed" >"$dir/back"
    fi
    o=$(ex take --dir "$dir" --kind "$KIND" --number "$n" --in "$dir/back" --now $((NOW + 200)))
    code=$?
    sed 's/^/      /' <<<"$o"
    chk "$variant: the owner took the finished file" [ "$code" = 0 ]
    hexs=$(cat "$dir/final-$n.hex")
    chk "$variant: the transaction is Core's own extraction" \
      [ "$(cli finalizepsbt "$(jq -r '.psbt' <<<"$signed")" | jq -r '.hex')" = "$hexs" ]
    accept_send "$hexs" "$txid" "$RATE"
  done
  rule AS-103 "$mark" "$KIND: a finished file and a finished transaction are checked against the file and taken"

  # ---- a wallet that sends itself ----------------------------------------------
  say "$KIND 4. The wallet sends the payment itself and never returns the file"
  mark=$FAILS
  dir="$WORK/$KIND-c"
  dest=$(cli -rpcwallet=miner getnewaddress)
  ex queue --dir "$dir" --kind "$KIND" --what cash --addr "$dest" --sats 50000 >/dev/null || die "queue"
  make_coins "$dir/coins.txt" "$SHOP" 300000
  tip=$(cli getblockcount)
  out=$(ex build --dir "$dir" --kind "$KIND" --coins "$dir/coins.txt" --rate "$RATE" \
    --lock-height "$tip" --change-index 2 --now "$NOW" --picks 1)
  n=$(item_numbers "$out" | head -1)
  txid=$(item_txid "$out" "$n")
  b64=$(base64 -w0 "$dir/item-$n.psbt")
  signed=$(cli -rpcwallet="$SIGNER" walletprocesspsbt "$b64" true ALL true true)
  hexs=$(jq -r '.hex' <<<"$signed")
  chk "the wallet sent it itself ($(cli sendrawtransaction "$hexs"))" [ "$(cli getmempoolentry "$txid" | jq -r '.vsize')" -gt 0 ]
  cli generatetoaddress 1 "$MINE" >/dev/null
  o=$(ex confirmed --dir "$dir" --kind "$KIND" --txid "$txid")
  code=$?
  sed 's/^/      /' <<<"$o"
  chk "the owner marked it" [ "$code" = 0 ]
  chk "the transaction was watched before the block (one filed)" grep -qx 'watched 1' <<<"$o"
  chk "the cash-out is paid with that transaction" grep -q "^cash-out [0-9]* Paid { txid: $txid }" <<<"$o"
  chk "the session ended and nothing is watched" bash -c "grep -qx 'sessions 0' <<<\"\$1\" && grep -qx 'watched-after 0' <<<\"\$1\"" _ "$o"
  rule AS-105 "$mark" "$KIND: a transaction the wallet sent itself is followed and its entry is paid when it confirms"
  mark=$FAILS
  chk "the snapshot with its sessions reads back to the same bytes (version 5)" grep -qx 'snapshot-same true' <<<"$o"
  rule AS-107 "$mark" "$KIND: the state with an own session is written and read back exactly"

  # ---- a merge of twelve small coins -------------------------------------------
  say "$KIND 5. A merge of twelve small coins at a low fee"
  mark=$FAILS
  dir="$WORK/$KIND-m"
  ex queue --dir "$dir" --kind "$KIND" --what merge --change-index 3 --sats 0 >"$dir.q" || die "queue merge"
  make_coins "$dir/coins.txt" "$SHOP" 6000
  chk "twelve small coins" [ "$(grep -c . "$dir/coins.txt")" = 12 ]
  tip=$(cli getblockcount)
  out=$(ex build --dir "$dir" --kind "$KIND" --coins "$dir/coins.txt" --rate "$LOW" \
    --lock-height "$tip" --change-index 4 --now "$NOW" --picks 1)
  code=$?
  sed 's/^/      /' <<<"$out"
  chk "the owner froze the merge" [ "$code" = 0 ]
  n=$(item_numbers "$out" | head -1)
  txid=$(item_txid "$out" "$n")
  b64=$(base64 -w0 "$dir/item-$n.psbt")
  dec=$(cli decodepsbt "$b64")
  chk "twelve inputs and one output, to the wallet's change address 3" \
    [ "$(jq -r --arg a "${CADDR[3]}" '[(.tx.vin | length), (.tx.vout | length), .tx.vout[0].scriptPubKey.address] | tostring' <<<"$dec")" = "[12,1,\"${CADDR[3]}\"]" ]
  signed=$(cli -rpcwallet="$SIGNER" walletprocesspsbt "$b64" true ALL true false)
  jq -r '.psbt' <<<"$signed" >"$dir/back"
  o=$(ex take --dir "$dir" --kind "$KIND" --number "$n" --in "$dir/back" --now $((NOW + 300)))
  code=$?
  sed 's/^/      /' <<<"$o"
  chk "the owner took the merge back and finished it" [ "$code" = 0 ]
  hexs=$(cat "$dir/final-$n.hex")
  accept_send "$hexs" "$txid" "$LOW"
  chk "the wallet holds one coin from it at change index 3" \
    [ "$(cli -rpcwallet="$SHOP" listunspent 1 | jq -r --arg t "$txid" '[.[] | select(.txid == $t) | .desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)"] | tostring')" = '["1/3"]' ]
  rule AS-106 "$mark" "$KIND: twelve small coins merge into one at a low fee, signed in the wallet"

  # ---- the wrong key -------------------------------------------------------------
  say "$KIND 6. A file signed by the wrong key, and a file not signed, are refused"
  mark=$FAILS
  dir="$WORK/$KIND-w"
  dest=$(cli -rpcwallet=miner getnewaddress)
  ex queue --dir "$dir" --kind "$KIND" --what cash --addr "$dest" --sats 30000 >/dev/null || die "queue"
  make_coins "$dir/coins.txt" "$SHOP" 300000
  tip=$(cli getblockcount)
  out=$(ex build --dir "$dir" --kind "$KIND" --coins "$dir/coins.txt" --rate "$RATE" \
    --lock-height "$tip" --change-index 5 --now "$NOW" --picks 1)
  n=$(item_numbers "$out" | head -1)
  txid=$(item_txid "$out" "$n")
  ex forge --dir "$dir" --kind "$KIND" --number "$n" --out "$dir/wrong.psbt" >/dev/null || die "forge"
  before=$(sha256sum "$dir/owner.bin")
  ex take --dir "$dir" --kind "$KIND" --number "$n" --in "$dir/wrong.psbt" --now $((NOW + 400)) >"$dir/r1.txt"
  code=$?
  chk "a file signed by the wrong key is refused: $(cat "$dir/r1.txt")" refused_with "$code" AS-103 "$dir/r1.txt"
  ex take --dir "$dir" --kind "$KIND" --number "$n" --in "$dir/item-$n.psbt" --now $((NOW + 401)) >"$dir/r2.txt"
  code=$?
  chk "the file as it was given is refused: $(cat "$dir/r2.txt")" refused_with "$code" AS-103 "$dir/r2.txt"
  chk "nothing changed" [ "$before" = "$(sha256sum "$dir/owner.bin")" ]
  b64=$(base64 -w0 "$dir/item-$n.psbt")
  signed=$(cli -rpcwallet="$SIGNER" walletprocesspsbt "$b64" true ALL true false)
  jq -r '.psbt' <<<"$signed" >"$dir/back"
  o=$(ex take --dir "$dir" --kind "$KIND" --number "$n" --in "$dir/back" --now $((NOW + 402)))
  chk "the right signature is taken afterwards" [ "$?" = 0 ]
  hexs=$(cat "$dir/final-$n.hex")
  accept_send "$hexs" "$txid" "$RATE"
  rule AS-103 "$mark" "$KIND: a file signed by the wrong key and an unsigned file are refused, and nothing changes"
}

run_kind single
run_kind multi

echo
if [ "$FAILS" = 0 ]; then
  echo "ALL PASS"
  exit 0
fi
echo "FAIL  $FAILS check(s) did not hold"
exit 1
