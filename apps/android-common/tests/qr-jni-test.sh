#!/usr/bin/env bash
# The JVM harness test of the camera scan's native reader: builds
# libopenbmt_qr_jni.so for the host, makes frames with the repository's QR
# encoder (an example of crates/openbmt-qr-jni, written to the scratch
# folder, not committed), and reads them through the same Kotlin calls the
# scan makes (apps/android-common/tests/QrNativeHarness.kt).
#
# Needs JAVA_HOME (JDK 21) and KOTLIN_HOME (the Kotlin 2.2.20 compiler
# directory). The cargo build and the Kotlin compile run under the memory
# guard. The scratch directory is the first argument, or a new one under
# ~/.cache/openbmt/tmp.
#
#   OPENBMT_GUARD         the memory guard (default: ~/.cache/openbmt/guard.sh)
#   QRJNI_HOST_TARGET     the cargo target directory
#                         (default: ~/.cache/openbmt/target/qrjni-host)

set -euo pipefail

: "${JAVA_HOME:?Set JAVA_HOME to a JDK 21 installation}"
: "${KOTLIN_HOME:?Set KOTLIN_HOME to the Kotlin 2.2.20 compiler directory}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
common="$(dirname "$here")"
repo="$(cd "$common/../.." && pwd)"
cache="$HOME/.cache/openbmt"
guard="${OPENBMT_GUARD:-$cache/guard.sh}"
target="${QRJNI_HOST_TARGET:-$cache/target/qrjni-host}"
lib="$KOTLIN_HOME/lib"
scratch="${1:-$(mktemp -d "${TMPDIR:-$cache/tmp}/qrjni-jvm.XXXXXX")}"
mkdir -p "$scratch"
trap 'rm -rf "$scratch/harness-classes"' EXIT

cd "$repo"
"$guard" env CARGO_TARGET_DIR="$target" CARGO_BUILD_JOBS=2 \
    cargo build --locked -p openbmt-qr-jni --lib --example make_frame
so="$target/debug/libopenbmt_qr_jni.so"
[ -f "$so" ] || { echo "qr-jni-test.sh: no library at $so" >&2; exit 1; }

make_frame() { "$target/debug/examples/make_frame" "$scratch/$1.bin" "$2" "$3"; }
make_frame plain "OPENBMT QR 12345" 0
make_frame padded "BITCOIN:BC1QAR0SRRR7XFKVY5L643LYDNW9RE59GTZZWF5MDQ" 64
make_frame bytes "openbmt-counter-pairing 1 / ünïcode / with a longer text so that the version is larger than one: $(printf 'x%.0s' $(seq 1 120))" 16

"$guard" "$JAVA_HOME/bin/java" -Xmx2g -cp "$lib/*" \
    org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
    -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
    -classpath "$lib/kotlin-stdlib.jar:$lib/annotations-13.0.jar" \
    -d "$scratch/harness-classes" \
    "$common/src/model/QrModel.kt" "$common/src/model/QrNative.kt" "$common/src/model/Format.kt" \
    "$common/src/model/WordsModel.kt" "$here/QrNativeHarness.kt"

"$JAVA_HOME/bin/java" -Xmx1g -cp "$scratch/harness-classes:$lib/kotlin-stdlib.jar" \
    org.openbmt.app.QrNativeHarness "$so" \
    "$scratch/plain.bin" "OPENBMT QR 12345" \
    "$scratch/padded.bin" "BITCOIN:BC1QAR0SRRR7XFKVY5L643LYDNW9RE59GTZZWF5MDQ" \
    "$scratch/bytes.bin" "openbmt-counter-pairing 1 / ünïcode / with a longer text so that the version is larger than one: $(printf 'x%.0s' $(seq 1 120))"
