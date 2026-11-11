#!/usr/bin/env bash
# The tests of the owner app's pure parts, on the desktop JVM: its words (and
# docs/screens/owner-app.md), the setup text, the config, the text form of a
# request and a response, the Tor gate, the screen choice, the lines and
# forms of the screens, and the rules of docs/OWNER-APP-RULES.md (every rule
# has a test named for it or a reason); then the tests of the shared code
# (apps/android-common/tests). No Android class is used and nothing is
# fetched.
#
# OPENBMT_WRITE_SCREENS=1 writes docs/screens/owner-app.md first.
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
wrapper="$repo/crates/openbmt-owner-ffi/bindings/kotlin/org/openbmt/owner"
lib="$KOTLIN_HOME/lib"
scratch="${1:-$(mktemp -d "${TMPDIR:-$HOME/.cache/openbmt/tmp}/ownerapk-jvm.XXXXXX")}"
mkdir -p "$scratch"
trap 'rm -rf "$scratch/jvm-classes"' EXIT

"$common/tests/run-jvm.sh" "$scratch/common"

"$JAVA_HOME/bin/java" -Xmx2g -cp "$lib/*" \
    org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
    -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
    -classpath "$lib/kotlin-stdlib.jar:$lib/annotations-13.0.jar" \
    -d "$scratch/jvm-classes" \
    "$wrapper/Native.kt" "$wrapper/OwnerNode.kt" \
    "$common"/src/model/*.kt "$app"/src/model/*.kt "$app/tests/AppModelTest.kt"

"$JAVA_HOME/bin/java" -Xmx2g -cp "$scratch/jvm-classes:$lib/kotlin-stdlib.jar" \
    org.openbmt.owner.app.AppModelTest "$repo/docs/screens/owner-app.md" "$repo/docs/OWNER-APP-RULES.md"
