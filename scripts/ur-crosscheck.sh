#!/usr/bin/env bash
# Cross-check of crates/openbmt-ur against Hummingbird, Sparrow's BC-UR
# library, an independent implementation (a test tool: never shipped, not
# on a list in deps/).
#
# The example `crosscheck gen` prints a few hundred messages from a fixed
# seed (types `bytes` and `crypto-psbt`, 1 byte to 20 KB, some PSBT-shaped)
# with the parts openbmt-ur writes for each. The Java harness
# crates/openbmt-ur/crosscheck/UrCross.java
#
#   - decodes those parts with Hummingbird, in order and with 30% lost,
#     repeated and shuffled;
#   - encodes the same messages with Hummingbird and compares the first
#     2 * seqLen parts character for character;
#   - writes Hummingbird's parts, in order and with losses, which
#     `crosscheck read` decodes with openbmt-ur.
#
# Then the cosigner keys (UR-23 to UR-30): the harness has Hummingbird's
# CryptoAccount and CryptoHDKey encode shared-wallet keys in the shapes the
# signers write (with and without cosigner, with other outputs around, with
# and without the coin info) and writes, for each, the key text it must give,
# worked out by the harness itself; `crosscheck keys` reads each with
# openbmt-ur and compares. A key that Hummingbird marks private, a second
# account, two keys and two different fingerprints must be refused.
#
#   scripts/ur-crosscheck.sh
#
# Needs a JDK 21 (javac and java: $JAVA_HOME, else ~/Android/jdk21, else
# the PATH) and cargo. The two jars are fetched once into
# ~/.cache/openbmt/tools/hummingbird/ and must have the pinned SHA-256.
set -euo pipefail
cd "$(dirname "$0")/.."

# com.sparrowwallet:hummingbird:1.7.6 and its one runtime dependency,
# co.nstant.in:cbor:0.9, from Maven Central. The SHA-1 is the one Maven
# Central publishes beside each; the SHA-256 is pinned here.
HB_URL=https://repo1.maven.org/maven2/com/sparrowwallet/hummingbird/1.7.6/hummingbird-1.7.6.jar
HB_SHA256=b236ede4c0ee1c348fa034d59c0af229a1760a967f36f5513f8c9569b1ec5b71
CBOR_URL=https://repo1.maven.org/maven2/co/nstant/in/cbor/0.9/cbor-0.9.jar
CBOR_SHA256=5ef465c47e34c6b13f93ece2ff49e94b933d69cf9f172cc4997418303884da73

tools="${OPENBMT_TOOLS:-$HOME/.cache/openbmt/tools}/hummingbird"
mkdir -p "$tools"

# fetch URL FILE SHA256
fetch() {
    local url="$1" jar="$2" want="$3"
    if [ ! -f "$jar" ]; then
        curl --fail --silent --show-error --location --retry 3 \
            --output "$jar.part" "$url"
        curl --fail --silent --show-error --location --retry 3 \
            --output "$jar.sha1" "$url.sha1"
        local published actual
        published="$(tr -d ' \n' <"$jar.sha1" | cut -c1-40)"
        actual="$(sha1sum "$jar.part" | cut -d' ' -f1)"
        if [ "$published" != "$actual" ]; then
            echo "FAIL  $jar's SHA-1 is not the one Maven Central publishes" >&2
            rm -f "$jar.part"
            exit 1
        fi
        mv "$jar.part" "$jar"
    fi
    local got
    got="$(sha256sum "$jar" | cut -d' ' -f1)"
    if [ "$got" != "$want" ]; then
        echo "FAIL  $jar has SHA-256 $got, not the pinned $want" >&2
        exit 1
    fi
    echo "PASS  $(basename "$jar"), SHA-256 $want"
}

hb="$tools/hummingbird-1.7.6.jar"
cbor="$tools/cbor-0.9.jar"
fetch "$HB_URL" "$hb" "$HB_SHA256"
fetch "$CBOR_URL" "$cbor" "$CBOR_SHA256"

if [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/javac" ]; then
    jdk="$JAVA_HOME"
elif [ -x "$HOME/Android/jdk21/bin/javac" ]; then
    jdk="$HOME/Android/jdk21"
elif command -v javac >/dev/null 2>&1; then
    jdk="$(dirname "$(dirname "$(readlink -f "$(command -v javac)")")")"
else
    echo "FAIL  no JDK found (set JAVA_HOME)" >&2
    exit 1
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/ur-crosscheck.XXXXXX")"
trap 'rm -rf "$work"' EXIT
"$jdk/bin/javac" -Xlint:all -cp "$hb:$cbor" -d "$work" \
    crates/openbmt-ur/crosscheck/UrCross.java

cargo run --release --locked --quiet -p openbmt-ur --example crosscheck -- gen \
    >"$work/cases.txt"
echo "cases: $(grep -c '^case ' "$work/cases.txt") ($(wc -c <"$work/cases.txt") bytes)"
"$jdk/bin/java" -cp "$work:$hb:$cbor" UrCross "$work/cases.txt" "$work/theirs.txt"
cargo run --release --locked --quiet -p openbmt-ur --example crosscheck -- \
    read "$work/cases.txt" "$work/theirs.txt"

# The cosigner keys.
"$jdk/bin/java" -cp "$work:$hb:$cbor" UrCross keys "$work/keys.txt"
cargo run --release --locked --quiet -p openbmt-ur --example crosscheck -- \
    keys "$work/keys.txt"
