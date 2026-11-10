#!/usr/bin/env bash
# The tests of the Key app's pure parts, on the desktop JVM: the screen model,
# the words model, the app's own words (against docs/screens/key.md), the line
# text and the saved-state file. No Android class is used.
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
wrapper="$repo/crates/openbmt-key-ffi/bindings/kotlin/org/openbmt/key"
lib="$KOTLIN_HOME/lib"
scratch="${1:-$(mktemp -d "${TMPDIR:-$HOME/.cache/openbmt/tmp}/keyapk-jvm.XXXXXX")}"
mkdir -p "$scratch"
trap 'rm -rf "$scratch/jvm-classes"' EXIT

"$JAVA_HOME/bin/java" -Xmx2g -cp "$lib/*" \
    org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
    -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
    -classpath "$lib/kotlin-stdlib.jar:$lib/annotations-13.0.jar" \
    -d "$scratch/jvm-classes" \
    "$wrapper/Native.kt" "$wrapper/KeyApp.kt" \
    "$app"/src/model/*.kt "$app/tests/AppModelTest.kt"

"$JAVA_HOME/bin/java" -Xmx2g -cp "$scratch/jvm-classes:$lib/kotlin-stdlib.jar" \
    org.openbmt.key.app.AppModelTest "$repo/docs/screens/key.md"
