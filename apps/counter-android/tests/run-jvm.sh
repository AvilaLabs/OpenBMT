#!/usr/bin/env bash
# The tests of the counter app's pure parts, on the desktop JVM: its words
# (and docs/screens/counter-app.md), the PIN store with a fake clock, the
# pairing steps, the config, the keypad, the QR layout, the price schedule and
# book with a fake clock and a fake fetcher, the Tor gate and the screen
# choice, and the Lightning transport against a local HTTPS server with a
# self-signed certificate (made here with keytool); then the tests of the shared code (apps/android-common/tests). No
# Android class is used and nothing is fetched.
#
# OPENBMT_WRITE_SCREENS=1 writes docs/screens/counter-app.md first.
#
# Needs JAVA_HOME (JDK 21) and KOTLIN_HOME (the Kotlin 2.2.20 compiler
# directory, with lib/kotlin-compiler.jar and lib/kotlin-stdlib.jar). The
# scratch directory is the first argument, or a new one under
# ~/.cache/openbmt/tmp.

set -euo pipefail

: "${JAVA_HOME:?Set JAVA_HOME to a JDK 21 installation}"
: "${KOTLIN_HOME:?Set KOTLIN_HOME to the Kotlin 2.2.20 compiler directory}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(dirname "$here")"
repo="$(cd "$app/../.." && pwd)"
common="$repo/apps/android-common"
tor_part="$common/tor"
wrapper="$repo/crates/openbmt-counter-ffi/bindings/kotlin/org/openbmt/counter"
lib="$KOTLIN_HOME/lib"
scratch="${1:-$(mktemp -d "${TMPDIR:-$HOME/.cache/openbmt/tmp}/counterapk-jvm.XXXXXX")}"
mkdir -p "$scratch"
trap 'rm -rf "$scratch/jvm-classes"' EXIT

"$common/tests/run-jvm.sh" "$scratch/common"

"$JAVA_HOME/bin/java" -Xmx2g -cp "$lib/*" \
    org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
    -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
    -classpath "$lib/kotlin-stdlib.jar:$lib/annotations-13.0.jar" \
    -d "$scratch/jvm-classes" \
    "$wrapper/Native.kt" "$wrapper/CounterNode.kt" \
    "$common"/src/model/*.kt "$tor_part"/src/model/*.kt "$app"/src/model/*.kt "$app/tests/AppModelTest.kt" "$app/tests/LightningTransportTest.kt"

"$JAVA_HOME/bin/java" -Xmx2g -cp "$scratch/jvm-classes:$lib/kotlin-stdlib.jar" \
    org.openbmt.counter.app.AppModelTest "$repo/docs/screens/counter-app.md"

# The transport of CR-114 against a local server: a certificate made for the run.
"$JAVA_HOME/bin/keytool" -genkeypair -alias t -keyalg EC -groupname secp256r1 -dname "CN=localhost" \
    -validity 2 -storetype PKCS12 -keystore "$scratch/pin.p12" -storepass changeit -keypass changeit 2>/dev/null
"$JAVA_HOME/bin/java" -Xmx1g -cp "$scratch/jvm-classes:$lib/kotlin-stdlib.jar" \
    org.openbmt.counter.app.LightningTransportTest "$scratch/pin.p12"
