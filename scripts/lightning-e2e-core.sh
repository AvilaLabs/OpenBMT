#!/usr/bin/env bash
# The own-node Lightning code of `openbmt-lightning` against real nodes, on
# regtest: LND and Core Lightning, each over real HTTPS.
#
# Question: do the rules of docs/LIGHTNING-RULES.md (LN-05 to LN-09, LN-12,
# LN-14, LN-15, LN-17, LN-18) hold against a running LND and a running Core
# Lightning, and does the crate's code do what they say?
#
# What it does:
#   1. Starts a private regtest bitcoind, LND (bitcoind backend, RPC and ZMQ,
#      REST on) and Core Lightning (clnrest), each in its own directory under
#      the run directory, on non-default ports.
#   2. Funds both nodes and opens one channel in each direction (LND to Core
#      Lightning and back), so each node can pay the other's invoice.
#   3. Makes the credentials a merchant would make: the admin macaroon, the
#      stock invoice.macaroon, a baked `invoices:read invoices:write`
#      macaroon; an unrestricted rune, a rune for `invoice` and
#      `listinvoices`, and a rune with one method more.
#   4. Runs the example `lightning_regtest` (crates/openbmt-lightning), which
#      drives the crate's real request builders and readers against the two
#      nodes over HTTPS and prints `PASS <rule> <what>` or
#      `FAIL <rule> <what>: <evidence>` for each check. The HTTPS is `curl`
#      after the certificate the node shows has been taken with `openssl
#      s_client` and has passed `CertPin::accepts`.
#   5. Builds and runs the JVM test of the counter app's `LightningTransport`
#      (Kotlin, `HttpsURLConnection` with the same kind of pin) against both
#      nodes: it sends the crate's invoice requests and the crate reads the
#      replies.
#   The script ends `ALL PASS` only if every check printed PASS.
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin, LND in
# ~/.local/opt/lnd-0.21.8, Core Lightning in ~/.local/opt/cln-26.06.10 (with
# libpq for the library path), curl, openssl, jq, and cargo (through
# ~/.cache/openbmt/guard.sh). The JVM part needs JAVA_HOME and KOTLIN_HOME as
# for apps/counter-android/tests/run-jvm.sh and is skipped with a notice
# when they are not set (set LNREAL_NO_JVM=1 to skip it on purpose).
#
# Environment:
#   CORE_BIN      directory with bitcoind and bitcoin-cli
#   LND_DIR       directory with lnd and lncli
#   CLN_DIR       Core Lightning install directory (usr/bin/lightningd)
#   CLN_LIBS      library path for Core Lightning's libpq
#   RUN_DIR       run directory (default ~/.cache/openbmt/lnreal/run)
#   NODE_BIN      a built `lightning_regtest` example binary
#   LNREAL_KEEP=1 keep the run directory afterwards
#
# Everything here is regtest on 127.0.0.1 with fresh keys. Nothing of
# ~/.bitcoin, ~/.lnd or ~/.lightning is used. Every daemon is stopped, and
# the run directory removed, on every exit.

set -uo pipefail

CORE_BIN=${CORE_BIN:-$HOME/.local/opt/bitcoin-core-32.1/bin}
LND_DIR=${LND_DIR:-$HOME/.local/opt/lnd-0.21.8}
CLN_DIR=${CLN_DIR:-$HOME/.local/opt/cln-26.06.10}
CLN_LIBS=${CLN_LIBS:-$CLN_DIR/libpq/usr/lib/x86_64-linux-gnu}
export PATH="$CORE_BIN:$LND_DIR:$CLN_DIR/usr/bin:$PATH"
export LD_LIBRARY_PATH="$CLN_LIBS${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
ROOT=$(cd "$(dirname "$0")/.." && pwd)
RUN=${RUN_DIR:-$HOME/.cache/openbmt/lnreal/run}

BTC_RPC=18976
BTC_P2P=18977
ZMQ_BLOCK=18978
ZMQ_TX=18979
LND_P2P=19735
LND_RPC=10109
LND_REST=8089
CLN_P2P=19846
CLN_REST=3011

BTC_DIR="$RUN/bitcoind"
LND_HOME="$RUN/lnd"
CLN_HOME="$RUN/cln"
OUT="$RUN/out"

say() { printf '\n== %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }
die() {
  printf 'ABORT %s\n' "$*" >&2
  exit 2
}

for tool in bitcoind bitcoin-cli lnd lncli lightningd lightning-cli curl openssl jq; do
  command -v "$tool" >/dev/null || die "$tool is not on PATH"
done
[ "$(command -v bitcoind)" = "$CORE_BIN/bitcoind" ] || die "bitcoind is not the one in $CORE_BIN"
case "$(bitcoind --version | head -1)" in
  "Bitcoin Core daemon version"*) ;;
  *) die "this check is for Bitcoin Core" ;;
esac
say "$(bitcoind --version | head -1); $(lnd --version); Core Lightning $(lightningd --version)"

# Refuse to run over a run directory that is in use.
if [ -e "$RUN" ]; then
  for pidfile in "$RUN"/*.pid; do
    [ -e "$pidfile" ] || continue
    if kill -0 "$(cat "$pidfile")" 2>/dev/null; then die "a node of an earlier run is still up ($pidfile)"; fi
  done
  rm -rf -- "$RUN"
fi
mkdir -p "$BTC_DIR" "$LND_HOME" "$CLN_HOME" "$OUT"

btc() { bitcoin-cli -regtest -datadir="$BTC_DIR" -rpcport="$BTC_RPC" "$@"; }
lnd_cli() {
  lncli --lnddir="$LND_HOME" --network=regtest --rpcserver="127.0.0.1:$LND_RPC" \
    --tlscertpath="$LND_HOME/tls.cert" --macaroonpath="$LND_HOME/data/chain/bitcoin/regtest/admin.macaroon" "$@"
}
cln_cli() { lightning-cli --lightning-dir="$CLN_HOME" --network=regtest "$@"; }

stop_daemons() {
  local name pid i
  for name in cln lnd; do
    [ -f "$RUN/$name.pid" ] || continue
    pid=$(cat "$RUN/$name.pid")
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      for i in $(seq 1 60); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.5
      done
      kill -9 "$pid" 2>/dev/null || true
    fi
  done
  if [ -f "$BTC_DIR/regtest/bitcoind.pid" ]; then
    pid=$(cat "$BTC_DIR/regtest/bitcoind.pid")
    btc stop >/dev/null 2>&1 || true
    for i in $(seq 1 60); do
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.5
    done
    kill -9 "$pid" 2>/dev/null || true
  fi
}
SERVER_PID=
cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null
  stop_daemons
  if [ "${LNREAL_KEEP:-0}" != 1 ]; then rm -rf -- "$RUN"; fi
}
trap cleanup EXIT

wait_for() { # seconds description command...
  local secs=$1 what=$2 i
  shift 2
  for i in $(seq 1 $((secs * 2))); do
    "$@" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  die "timed out waiting for $what"
}

# ---- the example ----------------------------------------------------------------

if [ -z "${NODE_BIN:-}" ]; then
  say "Build the example"
  TARGET=${CARGO_TARGET_DIR:-$HOME/.cache/openbmt/target/lnreal}
  OPENBMT_NO_SCCACHE=1 CARGO_BUILD_JOBS=3 "$HOME/.cache/openbmt/guard.sh" env CARGO_TARGET_DIR="$TARGET" \
    cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" -p openbmt-lightning \
    --example lightning_regtest || die "the build failed"
  NODE_BIN="$TARGET/debug/examples/lightning_regtest"
fi
[ -x "$NODE_BIN" ] || die "no example binary at $NODE_BIN"

# ---- bitcoind -------------------------------------------------------------------

say "bitcoind"
bitcoind -regtest -datadir="$BTC_DIR" -rpcport="$BTC_RPC" -rpcuser=lnreal -rpcpassword=lnreal \
  -listen=1 -bind="127.0.0.1:$BTC_P2P" -listenonion=0 -daemonwait -txindex=1 \
  -zmqpubrawblock="tcp://127.0.0.1:$ZMQ_BLOCK" -zmqpubrawtx="tcp://127.0.0.1:$ZMQ_TX" \
  -fallbackfee=0.0002 -dbcache=50 >/dev/null || die "bitcoind did not start"
btc() { bitcoin-cli -regtest -datadir="$BTC_DIR" -rpcport="$BTC_RPC" -rpcuser=lnreal -rpcpassword=lnreal "$@"; }
btc -named createwallet wallet_name=miner >/dev/null || die "no miner wallet"
MINER=$(btc -rpcwallet=miner getnewaddress)
btc generatetoaddress 110 "$MINER" >/dev/null || die "could not mine"
mine() { btc generatetoaddress "$1" "$MINER" >/dev/null; }

# ---- LND ------------------------------------------------------------------------

say "LND"
lnd --lnddir="$LND_HOME" --bitcoin.regtest --bitcoin.node=bitcoind \
  --bitcoind.rpchost="127.0.0.1:$BTC_RPC" --bitcoind.rpcuser=lnreal --bitcoind.rpcpass=lnreal \
  --bitcoind.zmqpubrawblock="tcp://127.0.0.1:$ZMQ_BLOCK" --bitcoind.zmqpubrawtx="tcp://127.0.0.1:$ZMQ_TX" \
  --listen="127.0.0.1:$LND_P2P" --rpclisten="127.0.0.1:$LND_RPC" --restlisten="127.0.0.1:$LND_REST" \
  --tlsextraip=127.0.0.1 --noseedbackup --debuglevel=warn --alias=lnreal-lnd \
  >"$RUN/lnd.out" 2>&1 &
echo $! >"$RUN/lnd.pid"
LND_MAC="$LND_HOME/data/chain/bitcoin/regtest"
wait_for 90 "LND to be ready" lnd_cli getinfo
LND_KEY=$(lnd_cli getinfo | jq -r .identity_pubkey)
note "LND node key $LND_KEY"

# ---- Core Lightning -------------------------------------------------------------

say "Core Lightning"
lightningd --lightning-dir="$CLN_HOME" --network=regtest --daemon --pid-file="$RUN/cln.pid" \
  --log-file="$RUN/cln.log" --addr="127.0.0.1:$CLN_P2P" --alias=lnreal-cln \
  --bitcoin-rpcconnect=127.0.0.1 --bitcoin-rpcport="$BTC_RPC" --bitcoin-rpcuser=lnreal --bitcoin-rpcpassword=lnreal \
  --clnrest-host=127.0.0.1 --clnrest-port="$CLN_REST" \
  >"$RUN/cln.out" 2>&1 || { tail -20 "$RUN/cln.out" "$RUN/cln.log" 2>/dev/null; die "lightningd did not start"; }
wait_for 90 "Core Lightning to be ready" cln_cli getinfo
CLN_KEY=$(cln_cli getinfo | jq -r .id)
note "CLN node key $CLN_KEY"
ls "$CLN_HOME/regtest" "$CLN_HOME/regtest/clnrest" 2>&1 | head -20

# ---- funds and channels ----------------------------------------------------------

say "Funds and one channel in each direction"
synced() { lnd_cli getinfo | jq -e '.synced_to_chain == true'; }
wait_for 90 "LND to be synced" synced
LND_ADDR=$(lnd_cli newaddress p2wkh | jq -r .address)
CLN_ADDR=$(cln_cli newaddr bech32 | jq -r .bech32)
btc -rpcwallet=miner sendtoaddress "$LND_ADDR" 5 >/dev/null || die "could not fund LND"
btc -rpcwallet=miner sendtoaddress "$CLN_ADDR" 5 >/dev/null || die "could not fund Core Lightning"
mine 6
lnd_funded() { [ "$(lnd_cli walletbalance | jq -r .confirmed_balance)" -gt 0 ]; }
cln_funded() { [ "$(cln_cli listfunds | jq '[.outputs[] | select(.status == "confirmed")] | length')" -gt 0 ]; }
wait_for 60 "LND funds" lnd_funded
wait_for 60 "Core Lightning funds" cln_funded
lnd_cli connect "$CLN_KEY@127.0.0.1:$CLN_P2P" >/dev/null || die "LND could not connect to Core Lightning"
open_lnd_channel() { lnd_cli openchannel --node_key "$CLN_KEY" --local_amt 1000000 --sat_per_vbyte 5; }
wait_for 90 "LND to open its channel" open_lnd_channel
mine 6
cln_channels() { cln_cli listpeerchannels | jq -e "[.channels[] | select(.state == \"CHANNELD_NORMAL\")] | length == $1"; }
cln_one() { cln_channels 1; }
cln_two() { cln_channels 2; }
wait_for 60 "Core Lightning to see the channel from LND" cln_one
fund_cln_channel() { cln_cli fundchannel id="$LND_KEY" amount=1000000 feerate=5000perkw; }
wait_for 90 "Core Lightning to open its channel" fund_cln_channel
mine 6
wait_for 90 "both channels to be usable" cln_two
two_active() { lnd_cli listchannels | jq -e '[.channels[] | select(.active)] | length == 2'; }
wait_for 90 "LND to see both channels active" two_active
mine 1
sleep 3
note "channels: LND $(lnd_cli listchannels | jq -c '[.channels[] | {local_balance, remote_balance, active}]')"

# ---- helpers the example calls, and the credentials ----------------------------------

cat >"$RUN/cln-pays.sh" <<EOS
#!/usr/bin/env bash
# Core Lightning pays the invoice in \$1.
export LD_LIBRARY_PATH="$LD_LIBRARY_PATH"
exec "$CLN_DIR/usr/bin/lightning-cli" --lightning-dir="$CLN_HOME" --network=regtest pay "\$1"
EOS
cat >"$RUN/lnd-pays.sh" <<EOS
#!/usr/bin/env bash
# LND pays the invoice in \$1.
exec "$LND_DIR/lncli" --lnddir="$LND_HOME" --network=regtest --rpcserver="127.0.0.1:$LND_RPC" \\
  --tlscertpath="$LND_HOME/tls.cert" --macaroonpath="$LND_MAC/admin.macaroon" payinvoice --pay_req "\$1" --force
EOS
chmod +x "$RUN/cln-pays.sh" "$RUN/lnd-pays.sh"

b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }
cp "$LND_MAC/admin.macaroon" "$OUT/lnd-admin.mac"
cp "$LND_MAC/invoice.macaroon" "$OUT/lnd-stock.mac"
lnd_cli bakemacaroon invoices:read invoices:write --save_to="$OUT/lnd-baked.mac" >/dev/null || die "could not bake the macaroon"
cp "$LND_HOME/tls.cert" "$OUT/lnd.pem"
openssl x509 -in "$OUT/lnd.pem" -outform DER -out "$OUT/lnd.der"
printf '%s' "$LND_KEY" >"$OUT/lnd.key"
printf 'lndconnect://127.0.0.1:%s?cert=%s&macaroon=%s' "$LND_REST" "$(b64url <"$OUT/lnd.der")" "$(b64url <"$OUT/lnd-baked.mac")" >"$OUT/lnd.connect"
LND_FP=$(openssl x509 -in "$OUT/lnd.pem" -noout -fingerprint -sha256 | cut -d= -f2)

cp "$CLN_HOME/regtest/server.pem" "$OUT/cln.pem"
openssl x509 -in "$OUT/cln.pem" -outform DER -out "$OUT/cln.der"
printf '%s' "$CLN_KEY" >"$OUT/cln.key"
CLN_FP=$(openssl x509 -in "$OUT/cln.pem" -noout -fingerprint -sha256 | cut -d= -f2)
cln_cli createrune | jq -r .rune >"$OUT/cln-rune-open"
cln_cli createrune restrictions='[["method=invoice","method=listinvoices"]]' | jq -r .rune >"$OUT/cln-rune-good"
cln_cli createrune restrictions='[["method=invoice","method=listinvoices","method=getinfo"]]' | jq -r .rune >"$OUT/cln-rune-wide"
# A rune of another node (one changed character of its hash), a revoked one, and one with
# base64 padding: the unique id of the node's runes is 0, 1, 2 so far; the tenth has two digits.
r=$(cat "$OUT/cln-rune-good")
c=${r:5:1}
n=A
[ "$c" = A ] && n=B
printf '%s' "${r:0:5}$n${r:6}" >"$OUT/cln-rune-forged"
REVOKED=$(cln_cli createrune restrictions='[["method=invoice","method=listinvoices"]]')
echo "$REVOKED" | jq -r .rune >"$OUT/cln-rune-revoked"
cln_cli blacklistrune start="$(echo "$REVOKED" | jq -r .unique_id)" >/dev/null || die "could not revoke a rune"
for _ in 1 2 3 4 5 6; do cln_cli createrune >/dev/null; done
cln_cli createrune restrictions='[["method=invoice","method=listinvoices"]]' | jq -r .rune >"$OUT/cln-rune-padded"
[ -s "$OUT/cln-rune-good" ] && [ -s "$OUT/cln-rune-padded" ] || die "could not make the runes"
case "$(cat "$OUT/cln-rune-padded")" in *=) ;; *) die "the padded rune has no padding: $(cat "$OUT/cln-rune-padded")" ;; esac
# An invoice that takes any amount, which the crate never asks for.
cln_cli invoice amount_msat=any label=lnreal-any description="any amount" | jq -r .payment_hash >"$OUT/cln-any.hash"

# A server that shows another certificate, for the pin check.
OTHER_PORT=18990
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 2 -subj /CN=localhost \
  -keyout "$OUT/other.key" -out "$OUT/other.pem" 2>/dev/null || die "could not make another certificate"
openssl x509 -in "$OUT/other.pem" -outform DER -out "$OUT/other.der"
openssl s_server -accept "$OTHER_PORT" -cert "$OUT/other.pem" -key "$OUT/other.key" -www >/dev/null 2>&1 &
SERVER_PID=$!
sleep 1

ADDRESS=$(btc -rpcwallet=miner getnewaddress)
JVM="$OUT/jvm"
mkdir -p "$JVM"
ARGS=(
  --address "$ADDRESS" --jvm-dir "$JVM"
  --lnd-host "127.0.0.1:$LND_REST" --lnd-cert "$OUT/lnd.pem" --lnd-key "$OUT/lnd.key" --lnd-der "$OUT/lnd.der"
  --lnd-admin-mac "$OUT/lnd-admin.mac" --lnd-stock-mac "$OUT/lnd-stock.mac" --lnd-baked-mac "$OUT/lnd-baked.mac"
  --lnd-connect "$OUT/lnd.connect" --lnd-fingerprint "$LND_FP" --lnd-pays "$RUN/lnd-pays.sh"
  --cln-host "127.0.0.1:$CLN_REST" --cln-cert "$OUT/cln.pem" --cln-key "$OUT/cln.key" --cln-der "$OUT/cln.der"
  --cln-rune-open "$OUT/cln-rune-open" --cln-rune-good "$OUT/cln-rune-good" --cln-rune-wide "$OUT/cln-rune-wide"
  --cln-fingerprint "$CLN_FP" --cln-pays "$RUN/cln-pays.sh" --cln-any-hash "$OUT/cln-any.hash"
  --cln-rune-forged "$OUT/cln-rune-forged" --cln-rune-revoked "$OUT/cln-rune-revoked" --cln-rune-padded "$OUT/cln-rune-padded"
  --other-host "127.0.0.1:$OTHER_PORT" --other-cert "$OUT/other.pem" --other-der "$OUT/other.der"
)

# ---- the checks ------------------------------------------------------------------

say "The crate against the nodes, over HTTPS"
"$NODE_BIN" run "${ARGS[@]}" 2>&1 | tee "$OUT/run.log"
RUN_STATUS=${PIPESTATUS[0]}

# ---- the counter app's transport on the JVM ----------------------------------------

GUARD=()
[ -x "$HOME/.cache/openbmt/guard.sh" ] && GUARD=("$HOME/.cache/openbmt/guard.sh")
JVM_STATUS=skipped
JVMCHECK_STATUS=skipped
if [ "${LNREAL_NO_JVM:-0}" = 1 ]; then
  say "JVM transport: skipped on request"
else
  JAVA_HOME=${JAVA_HOME:-$HOME/Android/jdk21}
  KOTLIN_HOME=${KOTLIN_HOME:-$HOME/.cache/openbmt/tools/kotlinc}
  if [ -x "$JAVA_HOME/bin/java" ] && [ -f "$KOTLIN_HOME/lib/kotlin-compiler.jar" ]; then
    say "JVM transport of the counter app against both nodes"
    KLIB="$KOTLIN_HOME/lib"
    WRAPPER="$ROOT/crates/openbmt-counter-ffi/bindings/kotlin/org/openbmt/counter"
    APP="$ROOT/apps/counter-android"
    CLASSES="$OUT/jvm-classes"
    "${GUARD[@]}" "$JAVA_HOME/bin/java" -Xmx2g -cp "$KLIB/*" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
      -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
      -classpath "$KLIB/kotlin-stdlib.jar:$KLIB/annotations-13.0.jar" -d "$CLASSES" \
      "$WRAPPER/Native.kt" "$WRAPPER/CounterNode.kt" "$APP/src/model/HostPort.kt" \
      "$APP/src/model/LightningTransport.kt" "$APP/tests/LightningRealNodeTest.kt" >"$OUT/kotlinc.log" 2>&1 ||
      { tail -20 "$OUT/kotlinc.log"; die "the Kotlin test did not compile"; }
    "${GUARD[@]}" "$JAVA_HOME/bin/java" -Xmx1g -cp "$CLASSES:$KLIB/kotlin-stdlib.jar" \
      org.openbmt.counter.app.LightningRealNodeTest "$JVM"
    JVM_STATUS=$?
    "$NODE_BIN" jvm-check "${ARGS[@]}" 2>&1 | tee "$OUT/jvm-check.log"
    JVMCHECK_STATUS=${PIPESTATUS[0]}
  else
    say "JVM transport: skipped, JAVA_HOME or KOTLIN_HOME is not usable"
  fi
fi

# ---- the verdict -------------------------------------------------------------------

say "Result"
FAILS=$(cat "$OUT/run.log" "$OUT/jvm-check.log" 2>/dev/null | grep -c '^FAIL' || true)
PASSES=$(cat "$OUT/run.log" "$OUT/jvm-check.log" 2>/dev/null | grep -c '^PASS' || true)
echo "$PASSES checks passed, $FAILS failed; example exit $RUN_STATUS, JVM send $JVM_STATUS, JVM check $JVMCHECK_STATUS"
JVM_OK=0
if [ "$JVM_STATUS" = 0 ] && [ "$JVMCHECK_STATUS" = 0 ]; then JVM_OK=1; fi
if [ "${LNREAL_NO_JVM:-0}" = 1 ] && [ "$JVM_STATUS" = skipped ]; then JVM_OK=1; fi
if [ "$RUN_STATUS" = 0 ] && [ "$FAILS" = 0 ] && [ "$JVM_OK" = 1 ]; then
  echo "ALL PASS"
  exit 0
fi
exit 1
