#!/usr/bin/env bash
# Cross-check of crates/openbmt-qr against ZXing, an independent QR
# implementation (a test tool: never shipped, not on a list in deps/).
#
# Encoder: the example `crosscheck` prints a deterministic set of cases; the Java
# harness crates/openbmt-qr/crosscheck/QrCross.java encodes each with
# ZXing and compares the matrices, decodes ours with ZXing, and reports
# where the mask choice differs. See that file for the details.
#
# Reader: crosscheck/QrDecodeCross.java makes images of codes encoded by
# ZXing (random content, levels, module sizes, turns, a UTF-8 ECI) for the
# openbmt-qr reader, and decodes images made by this encoder (the example
# `decodecheck`) with ZXing; where both read an image, the data must be the
# same, and every clean image must be read by both.
#
#   scripts/qr-crosscheck.sh
#
# Needs a JDK 21 (javac and java: $JAVA_HOME, else ~/Android/jdk21, else
# the PATH) and cargo. The jar is fetched once into
# ~/.cache/openbmt/tools/zxing/ and must have the pinned SHA-256.
set -euo pipefail
cd "$(dirname "$0")/.."

# com.google.zxing:core:3.5.3 from Maven Central. The SHA-1 is the one
# Maven Central publishes beside it; the SHA-256 is pinned here.
ZXING_URL=https://repo1.maven.org/maven2/com/google/zxing/core/3.5.3/core-3.5.3.jar
ZXING_SHA256=8d8064c1636fdaef7189dd9055c7d59950a8940a12f2293956446ec3c109fd82

tools="${OPENBMT_TOOLS:-$HOME/.cache/openbmt/tools}/zxing"
jar="$tools/core-3.5.3.jar"
mkdir -p "$tools"

if [ ! -f "$jar" ]; then
    curl --fail --silent --show-error --location --retry 3 \
        --output "$jar.part" "$ZXING_URL"
    curl --fail --silent --show-error --location --retry 3 \
        --output "$jar.sha1" "$ZXING_URL.sha1"
    published="$(tr -d ' \n' <"$jar.sha1" | cut -c1-40)"
    actual="$(sha1sum "$jar.part" | cut -d' ' -f1)"
    if [ "$published" != "$actual" ]; then
        echo "FAIL  the jar's SHA-1 is not the one Maven Central publishes" >&2
        rm -f "$jar.part"
        exit 1
    fi
    mv "$jar.part" "$jar"
fi
got="$(sha256sum "$jar" | cut -d' ' -f1)"
if [ "$got" != "$ZXING_SHA256" ]; then
    echo "FAIL  $jar has SHA-256 $got, not the pinned $ZXING_SHA256" >&2
    exit 1
fi
echo "PASS  zxing core 3.5.3 jar, SHA-256 $ZXING_SHA256"

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

work="$(mktemp -d "${TMPDIR:-/tmp}/qr-crosscheck.XXXXXX")"
trap 'rm -rf "$work"' EXIT
"$jdk/bin/javac" -Xlint:all -cp "$jar" -d "$work" \
    crates/openbmt-qr/crosscheck/QrCross.java \
    crates/openbmt-qr/crosscheck/QrDecodeCross.java \
    crates/openbmt-qr/crosscheck/com/google/zxing/qrcode/encoder/PenaltyAccess.java

cargo run --release --locked --quiet -p openbmt-qr --example crosscheck \
    >"$work/cases.txt"
echo "cases: $(grep -c '^case ' "$work/cases.txt") ($(wc -c <"$work/cases.txt") bytes)"
"$jdk/bin/java" -cp "$work:$jar" QrCross <"$work/cases.txt"

# The reader.
images="$work/images"
"$jdk/bin/java" -cp "$work:$jar" QrDecodeCross gen "$images" 300
cargo run --release --locked --quiet -p openbmt-qr --example decodecheck -- \
    gen "$images" 300
cargo run --release --locked --quiet -p openbmt-qr --example decodecheck -- \
    read "$images"
echo "images: $(ls "$images"/*.pgm | wc -l)"
"$jdk/bin/java" -cp "$work:$jar" QrDecodeCross check "$images"
