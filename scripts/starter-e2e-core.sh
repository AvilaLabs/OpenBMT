#!/usr/bin/env bash
# The starter wallet against Bitcoin Core, on regtest
# (docs/START-SIMPLE-RULES.md, SS-04, SS-05, SS-20, SS-30, SS-42, SS-43).
#
# Question: do the 12 words that the starter library makes from the
# kernel's bytes open the wallet that Bitcoin Core derives from the same
# bytes by its own code; does a refund signed from the sealed seed get
# accepted by Core and confirm; and does the upgrade move, which derives the
# new wallet's receive address from its description, pay that address from
# every coin and leave the starter wallet empty?
#
# Bitcoin Core plays the chain and the shop's new wallet. The starter wallet
# is the real library, through the example `starter`. The words are never
# printed by the library: this script works them out itself, with Python's
# hashlib alone (the mix of SS-03, the word list, PBKDF2 and the BIP32
# master key), and Core derives every address from that master key. The
# example types the check-back on the library's keypad with those words,
# which passes only if they are the library's own, and then seals the seed.
# The key is the same on both sides, under different prefixes: the library
# holds the mainnet description of the account key (`xpub`), as the owner
# phone does; Core holds the master key under the test network's prefix
# (`tprv`). The scripts do not depend on the network, so Core's addresses
# are the regtest addresses of the library's scripts.
#
# What it does:
#   1. Works out the words and the master key independently, makes the
#      starter wallet in the library from the same fixed kernel bytes, and
#      compares Core's own derivation of m/84h/0h/0h/0/0..7 with the
#      library's receive addresses (SS-04, SS-05).
#   2. Funds three receive addresses of the starter wallet, one block after.
#   3. Pays a refund from the sealed seed (SS-30): Core accepts it at no less
#      than the asked rate, it spends only the starter's coins and pays only
#      the refund and the starter's change, it confirms, the change is a coin
#      of the change chain. A wrong data key signs nothing (SS-20).
#   4. The upgrade (SS-42): with a coin still unconfirmed the library refuses
#      to move; then once every coin is confirmed it pays the new wallet's
#      receive address 5 from every coin, in one transaction with no change,
#      Core accepts it, it confirms, the starter wallet is empty and the new
#      wallet holds the coin at 0/5.
#   5. The seed may be deleted once the move has one confirmation (SS-43).
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (put first
# on PATH; set CORE_BIN to use another; the bitcoind in /usr/local/bin is
# never used), jq, python3, and cargo unless STARTER_BIN names a built
# example.
#
# Environment:
#   CORE_BIN     directory with bitcoind and bitcoin-cli
#   DRILL_DIR    scratch directory (default: made new under ~/.cache/openbmt)
#   DRILL_PORTS  the RPC port of the regtest node (default 20100)
#   DRILL_KEEP=1 keep the scratch directory afterwards
#   STARTER_BIN  the `starter` example binary
#
# Everything here is regtest on 127.0.0.1. The keys come from fixed test
# bytes: TEST ONLY, NEVER USE. The node and the scratch directory are
# removed on every exit. The default Bitcoin data directory is never
# touched.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
export PATH="$CORE_BIN:$PATH"
RPC_PORT=${DRILL_PORTS:-20100}
mkdir -p "$HOME/.cache/openbmt/tmp"
BASE=${DRILL_DIR:-$(mktemp -d "$HOME/.cache/openbmt/tmp/starter-e2e-XXXXXX")}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
NODE_DIR="$BASE/core"
WORK="$BASE/work"
RATE=2000 # satoshis per 1000 virtual bytes
# The kernel's first read, fixed for the test: 1, 2, ... 32.
KERNEL=0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20
ROLLS=""

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

for tool in bitcoind bitcoin-cli jq python3; do
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

if [ -z "${STARTER_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/start-simple}
  GUARD=$HOME/.cache/openbmt/guard.sh
  if [ -x "$GUARD" ]; then
    "$GUARD" env CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner --example starter ||
      die "the build failed"
  else
    CARGO_TARGET_DIR="$TARGET" cargo build --quiet --locked \
      --manifest-path "$ROOT/Cargo.toml" -p openbmt-owner --example starter ||
      die "the build failed"
  fi
  STARTER_BIN="$TARGET/debug/examples/starter"
fi
[ -x "$STARTER_BIN" ] || die "no example binary at $STARTER_BIN"
ex() { "$STARTER_BIN" "$@"; }

bitcoind -regtest -datadir="$NODE_DIR" -rpcport="$RPC_PORT" -listen=0 -daemonwait \
  -fallbackfee=0.0001 -txindex=1 -dbcache=50 >/dev/null || die "bitcoind did not start"
NODE_PID=$(cat "$NODE_DIR/regtest/bitcoind.pid")
[ -n "$NODE_PID" ] || die "no node pid"

cli -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINE=$(cli -rpcwallet=miner getnewaddress)
cli generatetoaddress 101 "$MINE" >/dev/null || die "no blocks"

# coins FILE WALLET MINCONF: one line for each coin of the wallet with at
# least MINCONF confirmations (unconfirmed payments from others included at
# 0): txid vout confirmations chain index transaction-hex.
make_coins() {
  : >"$1"
  # The wallets learn of a payment through the node's validation queue.
  cli syncwithvalidationinterfacequeue >/dev/null
  cli -rpcwallet="$2" listunspent "${3:-1}" 9999999 '[]' true |
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

# ---- the words, worked out apart from the library ----------------------------------
# Python, hashlib only: SS-03's mix with the starter tag, the BIP39 word
# list (the list is read from the example; its SHA-256 is checked against
# the published English list), the BIP39 seed (PBKDF2), the BIP32 master
# key, and its serialization under the test network's prefix.
ex wordlist >"$WORK/wordlist.txt" || die "no word list"
INDEP=$(python3 -I - "$WORK/wordlist.txt" "$KERNEL" "$ROLLS" <<'PY'
import hashlib, hmac, sys
wl_path, kernel_hex, rolls = sys.argv[1], sys.argv[2], sys.argv[3]
words = open(wl_path).read().split()
assert len(words) == 2048
# The published BIP39 English list (bitcoin/bips, bip-0039/english.txt).
assert hashlib.sha256(("\n".join(words) + "\n").encode()).hexdigest() == \
    "2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda", "not the BIP39 list"
digits = "".join(c for c in rolls)
ent = hashlib.sha256(b"OpenBMT/starter/v1" + b"\0" + bytes.fromhex(kernel_hex) + digits.encode()).digest()[:16]
bits = bin(int.from_bytes(ent, "big"))[2:].zfill(128)
bits += bin(hashlib.sha256(ent).digest()[0])[2:].zfill(8)[:4]
mnemonic = [words[int(bits[i:i + 11], 2)] for i in range(0, 132, 11)]
seed = hashlib.pbkdf2_hmac("sha512", " ".join(mnemonic).encode(), b"mnemonic", 2048)
I = hmac.new(b"Bitcoin seed", seed, hashlib.sha512).digest()
raw = bytes.fromhex("04358394") + b"\0" + b"\0\0\0\0" + b"\0\0\0\0" + I[32:] + b"\0" + I[:32]
chk = hashlib.sha256(hashlib.sha256(raw).digest()).digest()[:4]
A = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
n = int.from_bytes(raw + chk, "big")
out = ""
while n:
    n, r = divmod(n, 58)
    out = A[r] + out
print(" ".join(mnemonic))
print(out)
PY
) || die "the independent computation failed"
INDEP_WORDS=$(sed -n 1p <<<"$INDEP")
INDEP_TPRV=$(sed -n 2p <<<"$INDEP")
[ "$(wc -w <<<"$INDEP_WORDS")" = 12 ] || die "the independent words are not 12"

say "1. The starter wallet from the kernel's bytes, in the library and in Core"
mark=$FAILS
out=$(ex make --kernel "$KERNEL" ${ROLLS:+--rolls "$ROLLS"} --words "$INDEP_WORDS" \
  --sealed "$WORK/sealed.hex" --key "$WORK/key.hex")
code=$?
sed 's/^/      /' <<<"$out" | grep -v "^      \(receive\|change\|descriptor\)"
chk "the library made the wallet (exit 0)" [ "$code" = 0 ]
chk "the check-back on the library's keypad passed with the independently worked words" \
  grep -q "^check-back passed" <<<"$out"
chk "the sealed seed and the data key were written (45 and 32 bytes)" \
  bash -c '[ "$(wc -c <"$1")" = 90 ] && [ "$(wc -c <"$2")" = 64 ]' _ "$WORK/sealed.hex" "$WORK/key.hex"
chk "the words are in no output of the library" bash -c '! grep -qiwE "$1" <<<"$2"' _ "$(tr ' ' '|' <<<"$INDEP_WORDS")" "$out"
mapfile -t SADDR < <(sed -n 's/^receive [0-9]* //p' <<<"$out")
mapfile -t SCHANGE < <(sed -n 's/^change [0-9]* //p' <<<"$out")
cli -named createwallet wallet_name=starter blank=true >/dev/null || die "no starter wallet"
for pair in "0 false" "1 true"; do
  set -- $pair
  d="wpkh($INDEP_TPRV/84h/0h/0h/$1/*)"
  d="$d#$(cli getdescriptorinfo "$d" | jq -r '.checksum')"
  res=$(cli -rpcwallet=starter importdescriptors "[{\"desc\":\"$d\",\"timestamp\":\"now\",\"active\":true,\"internal\":$2,\"range\":[0,99]}]")
  chk "Core imported the private descriptor of chain $1 from the independent master key" [ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ]
  if [ "$1" = 0 ]; then
    CORE_ADDR=$(cli deriveaddresses "$d" "[0,7]" | jq -r 'join(" ")')
  else
    CORE_CHANGE=$(cli deriveaddresses "$d" "[0,3]" | jq -r 'join(" ")')
  fi
done
chk "Core derives the receive addresses 0 to 7 of m/84h/0h/0h from the master key as the library does" \
  [ "$CORE_ADDR" = "${SADDR[*]}" ]
chk "Core derives the change addresses 0 to 3 as the library does" [ "$CORE_CHANGE" = "${SCHANGE[*]}" ]
rule SS-05 "$mark" "the words made from the kernel's bytes are the words worked out apart from the library, and Core derives the same addresses"
mark=$FAILS
chk "the library's wallet is BIP84: native SegWit, account 0, no passphrase (Core's addresses are bcrt1q)" \
  [ "${SADDR[0]:0:6}" = bcrt1q ]
rule SS-04 "$mark" "12 words, no passphrase, native SegWit single key at m/84h/0h/0h"

# ---- the shop's new wallet ----------------------------------------------------------
ex shop-keys >"$WORK/shop-keys.txt" || die "no shop keys from the example"
r=$(sed -n 's/^private-receive //p' "$WORK/shop-keys.txt")
c=$(sed -n 's/^private-change //p' "$WORK/shop-keys.txt")
r="$r#$(cli getdescriptorinfo "$r" | jq -r '.checksum')"
c="$c#$(cli getdescriptorinfo "$c" | jq -r '.checksum')"
cli -named createwallet wallet_name=newshop blank=true >/dev/null || die "no new wallet"
res=$(cli -rpcwallet=newshop importdescriptors "[{\"desc\":\"$r\",\"timestamp\":\"now\",\"active\":true,\"range\":[0,99]},{\"desc\":\"$c\",\"timestamp\":\"now\",\"active\":true,\"internal\":true,\"range\":[0,99]}]")
[ "$(jq -r 'all(.[]; .success)' <<<"$res")" = true ] || die "Core did not import the new wallet"
NEW5=$(cli deriveaddresses "$r" "[5,5]" | jq -r '.[0]')

# ---- funding ---------------------------------------------------------------------------
say "2. The starter wallet is funded: 100000, 80000 and 60000 sat"
for pair in "0 100000" "1 80000" "2 60000"; do
  set -- $pair
  cli -rpcwallet=miner sendtoaddress "${SADDR[$1]}" "$(btc "$2")" >/dev/null || die "funding $1 failed"
done
cli generatetoaddress 1 "$MINE" >/dev/null
chk "the starter wallet holds 240000 sat in three coins" \
  [ "$(cli -rpcwallet=starter listunspent 1 | jq -c '[.[] | .amount * 100000000 | round] | sort')" = "[60000,80000,100000]" ]

# ---- a refund --------------------------------------------------------------------------
say "3. A refund of 70000 sat from the sealed seed"
mark=$FAILS
DEST=$(cli -rpcwallet=miner getnewaddress)
make_coins "$WORK/coins1.txt" starter
BEFORE=$(cli -rpcwallet=starter listunspent 1 | jq -c '[.[] | "\(.txid):\(.vout)"] | sort')
TIP=$(cli getblockcount)
out=$(ex pay --sealed "$WORK/sealed.hex" --key "$WORK/key.hex" --coins "$WORK/coins1.txt" --addr "$DEST" \
  --sats 70000 --rate "$RATE" --change-index 0 --lock-height "$TIP" --out "$WORK/refund1.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library signed the refund (exit 0)" [ "$code" = 0 ]
TXID=$(fact txid "$out")
HEX=$(cat "$WORK/refund1.hex")
chk "Core decodes the transaction to the ID the library gave" [ "$(cli decoderawtransaction "$HEX" | jq -r '.txid')" = "$TXID" ]
rule SS-30 "$mark" "a queued refund is signed from the sealed seed and the data key"
mark=$FAILS
DEC=$(cli decoderawtransaction "$HEX")
SPENT=$(jq -c '[.vin[] | "\(.txid):\(.vout)"] | sort' <<<"$DEC")
chk "every input is a coin the starter wallet held before" \
  [ "$(jq -n --argjson s "$SPENT" --argjson b "$BEFORE" '$s | all(. as $x | $b | index($x) != null)')" = true ]
chk "the outputs are the refund's address and the starter's change address 0, and nothing else" \
  [ "$(jq -c --arg d "$DEST" --arg c "${SCHANGE[0]}" '[.vout[].scriptPubKey.address] | sort == ([$d, $c] | sort)' <<<"$DEC")" = true ]
chk "the refund output is 70000 sat" \
  [ "$(jq -r --arg d "$DEST" '.vout[] | select(.scriptPubKey.address == $d) | .value * 100000000 | round' <<<"$DEC")" = 70000 ]
rule SS-30 "$mark" "the transaction spends only the starter's coins and pays only the refund and the starter's change"
mark=$FAILS
accept_send "$HEX" "$TXID" "$RATE"
rule SS-30 "$mark" "Core accepts the refund at no less than the asked rate, and it confirms"
mark=$FAILS
CH=$(fact change "$out")
set -- $CH
CH_SATS=$2
chk "the refund's destination holds 70000 sat" \
  [ "$(cli -rpcwallet=miner getreceivedbyaddress "$DEST" | jq -r '. * 100000000 | round')" = 70000 ]
chk "the starter wallet holds the change ($CH_SATS sat) as a coin of its change chain at index 0" \
  [ "$(cli -rpcwallet=starter listunspent 1 | jq -r --arg t "$TXID" --argjson s "$CH_SATS" \
    '[.[] | select(.txid == $t) | select((.amount * 100000000 | round) == $s) | .desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)"] | tostring')" = '["1/0"]' ]
chk "the starter wallet's balance is its 240000 less the refund and the fee" \
  [ "$(cli -rpcwallet=starter getbalances | jq -r '.mine.trusted * 100000000 | round')" = $((240000 - 70000 - $(fact fee "$out"))) ]
rule SS-30 "$mark" "the change is back in the starter wallet: its balance is what it held less the refund and the fee"

# ---- the data key --------------------------------------------------------------------------
say "4. A data key that is not the seal's signs nothing"
mark=$FAILS
printf '%064d' 7 >"$WORK/wrong-key.hex"
make_coins "$WORK/coins2.txt" starter
MEMPOOL=$(cli getmempoolinfo | jq -r '.size')
out=$(ex pay --sealed "$WORK/sealed.hex" --key "$WORK/wrong-key.hex" --coins "$WORK/coins2.txt" --addr "$DEST" \
  --sats 20000 --rate "$RATE" --change-index 1 --lock-height "$TIP" --out "$WORK/refund-wrong.hex")
code=$?
note "$out"
chk "the library refused it (exit 2) with the rule SS-20" bash -c '[ "$1" = 2 ] && grep -q "^refused SS-20" <<<"$2"' _ "$code" "$out"
chk "no transaction was written and the mempool is as it was" bash -c '[ ! -e "$1" ] && [ "$(bitcoin-cli -regtest -datadir="$2" -rpcport="$3" getmempoolinfo | jq -r .size)" = "$4" ]' _ "$WORK/refund-wrong.hex" "$NODE_DIR" "$RPC_PORT" "$MEMPOOL"
rule SS-20 "$mark" "the sealed seed opens only with its own data key"

# ---- the upgrade -------------------------------------------------------------------------
say "5. The upgrade: one move of every coin to the new wallet's receive address 5"
mark=$FAILS
# A coin that is not confirmed yet: the move waits.
cli -rpcwallet=miner sendtoaddress "${SADDR[3]}" "$(btc 25000)" >/dev/null || die "funding 3 failed"
make_coins "$WORK/coins3.txt" starter 0
chk "the coins listed include one with no confirmation" grep -q '^[0-9a-f]* [0-9]* 0 ' "$WORK/coins3.txt"
TIP=$(cli getblockcount)
out=$(ex move --sealed "$WORK/sealed.hex" --key "$WORK/key.hex" --coins "$WORK/coins3.txt" --new-index 5 \
  --rate "$RATE" --lock-height "$TIP" --min-conf 1 --out "$WORK/move-early.hex")
code=$?
note "$out"
chk "with a coin unconfirmed the library refuses to move (exit 2, SS-42)" bash -c '[ "$1" = 2 ] && grep -q "^refused SS-42" <<<"$2"' _ "$code" "$out"
chk "nothing was written" [ ! -e "$WORK/move-early.hex" ]
cli generatetoaddress 1 "$MINE" >/dev/null
make_coins "$WORK/coins4.txt" starter 1
N=$(grep -c . "$WORK/coins4.txt")
HELD=$(cli -rpcwallet=starter getbalances | jq -r '.mine.trusted * 100000000 | round')
TIP=$(cli getblockcount)
out=$(ex move --sealed "$WORK/sealed.hex" --key "$WORK/key.hex" --coins "$WORK/coins4.txt" --new-index 5 \
  --rate "$RATE" --lock-height "$TIP" --min-conf 1 --out "$WORK/move.hex")
code=$?
sed 's/^/      /' <<<"$out"
chk "the library made the move (exit 0)" [ "$code" = 0 ]
chk "the destination is the address Core derives for the new wallet's receive index 5, from its description" \
  [ "$(fact to "$out")" = "$NEW5" ]
chk "every coin of the starter wallet is spent ($N)" [ "$(fact inputs "$out")" = "$N" ]
chk "there is no change" [ "$(fact change "$out")" = none ]
MOVE_TXID=$(fact txid "$out")
MOVE_DEC=$(cli decoderawtransaction "$(cat "$WORK/move.hex")")
chk "the one output pays the new wallet's address" \
  [ "$(jq -c --arg a "$NEW5" '[.vout[].scriptPubKey.address] == [$a]' <<<"$MOVE_DEC")" = true ]
chk "the output and the fee are everything the starter wallet held ($HELD sat)" \
  [ $(($(fact paid "$out") + $(fact fee "$out"))) = "$HELD" ]
accept_send "$(cat "$WORK/move.hex")" "$MOVE_TXID" "$RATE"
chk "the starter wallet holds nothing" [ "$(cli -rpcwallet=starter listunspent 0 | jq -r 'length')" = 0 ]
chk "the new wallet holds the coin at receive index 5" \
  [ "$(cli -rpcwallet=newshop listunspent 1 | jq -r --arg t "$MOVE_TXID" '[.[] | select(.txid == $t) | .desc | capture("/(?<c>[01])/(?<i>[0-9]+)\\]") | "\(.c)/\(.i)"] | tostring')" = '["0/5"]' ]
rule SS-42 "$mark" "the upgrade move pays the new wallet's derived address from every coin in one payment, and empties the starter wallet"

# ---- the key may go ----------------------------------------------------------------------
say "6. The key may be deleted once the move has one confirmation"
mark=$FAILS
CONF=$(cli getrawtransaction "$MOVE_TXID" true | jq -r '.confirmations')
chk "the move has $CONF confirmation(s)" [ "$CONF" -ge 1 ]
chk "the library says the seed may go at $CONF" [ "$(fact may-go "$(ex may-go --confirmations "$CONF")")" = yes ]
chk "the library says it may not at 0" [ "$(fact may-go "$(ex may-go --confirmations 0)")" = no ]
rule SS-43 "$mark" "the seed may be deleted once the move has one confirmation"

echo
if [ "$FAILS" = 0 ]; then
  echo "ALL PASS"
else
  echo "FAILED: $FAILS check(s)"
  exit 1
fi
