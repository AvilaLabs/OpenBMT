#!/usr/bin/env bash
# The apps' own screen model (plain Kotlin) compiled for the browser with the
# Kotlin/JS compiler already in the tool cache: an ES module in dist/kotlin-js/.
# Nothing is downloaded and no Gradle is used. The compiler runs under the
# memory guard.
#
# Usage: scripts/demo-kotlin-js.sh [counter|owner]   (default: counter)
#   KOTLIN_HOME   the Kotlin 2.2.20 compiler directory
#                 (default: ~/.cache/openbmt/tools/kotlinc)
#   JAVA_HOME     a JDK 21 (default: ~/Android/jdk21)
#   OPENBMT_GUARD the memory guard (default: ~/.cache/openbmt/guard.sh)
#
# The source list is the one the app's JVM tests use, without the tests, plus
# the files in demo/kotlin-js/<app>/ (the web twins of the JVM-only parts).

set -euo pipefail

app_name="${1:-counter}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
guard="${OPENBMT_GUARD:-$HOME/.cache/openbmt/guard.sh}"
kotlin_home="${KOTLIN_HOME:-$HOME/.cache/openbmt/tools/kotlinc}"
java_home="${JAVA_HOME:-$HOME/Android/jdk21}"
lib="$kotlin_home/lib"
common="$root/apps/android-common"
twins="$root/demo/kotlin-js/$app_name"
out="$root/dist/kotlin-js"
mkdir -p "$out"

case "$app_name" in
counter)
    app="$root/apps/counter-android"
    wrapper="$root/crates/openbmt-counter-ffi/bindings/kotlin/org/openbmt/counter"
    extra=("$common"/tor/src/model/*.kt "$wrapper"/*.kt) ;;
owner)
    app="$root/apps/owner-android"
    wrapper="$root/crates/openbmt-owner-ffi/bindings/kotlin/org/openbmt/owner"
    extra=("$wrapper"/*.kt) ;;
*) echo "usage: $0 [counter|owner]" >&2; exit 2 ;;
esac

# The files that are replaced by a twin in demo/kotlin-js/<app>/ (the JVM's
# files, sockets, TLS and native libraries have no meaning in a page).
skip=" DurableFile.kt LightningTransport.kt SwapTransport.kt TorControl.kt ShareImages.kt QrNative.kt UrNative.kt Native.kt Socks5Probe.kt Pbkdf2Hasher.kt "
sources=()
for f in "$common"/src/model/*.kt "${extra[@]}" "$app"/src/model/*.kt; do
    case "$skip" in *" $(basename "$f") "*) ;; *) sources+=("$f") ;; esac
done
sources+=("$root"/demo/kotlin-js/common/*.kt)
if [ -d "$twins" ]; then
    sources+=("$twins"/*.kt)
fi

compile() {
    OPENBMT_NO_SCCACHE=1 "$guard" env JAVA_HOME="$java_home" \
        "$java_home/bin/java" -Xmx2g -cp "$lib/*" \
        org.jetbrains.kotlin.cli.js.K2JSCompiler -libraries "$lib/kotlin-stdlib-js.klib" \
        -ir-output-name "$app_name-model" "$@"
}

# Step 1 compiles the sources to a library (all the compile errors come here);
# step 2 turns the library and the standard library into an ES module.
rm -rf "$out/$app_name"
compile -Xallow-kotlin-package -Xir-produce-klib-dir -ir-output-dir "$out/$app_name-klib" "${sources[@]}"
compile -Xir-produce-js -Xinclude="$out/$app_name-klib" -module-kind es -target es2015 \
    -ir-output-dir "$out/$app_name"
ls -l "$out/$app_name"
