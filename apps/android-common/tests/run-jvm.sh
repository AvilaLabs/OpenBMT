#!/usr/bin/env bash
# The tests of the code both apps share (apps/android-common/src/model), on
# the desktop JVM. No Android class is used.
#
# Needs JAVA_HOME (JDK 21) and KOTLIN_HOME (the Kotlin 2.2.20 compiler
# directory). The scratch directory is the first argument, or a new one under
# ~/.cache/openbmt/tmp.

set -euo pipefail

: "${JAVA_HOME:?Set JAVA_HOME to a JDK 21 installation}"
: "${KOTLIN_HOME:?Set KOTLIN_HOME to the Kotlin 2.2.20 compiler directory}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
common="$(dirname "$here")"
lib="$KOTLIN_HOME/lib"
scratch="${1:-$(mktemp -d "${TMPDIR:-$HOME/.cache/openbmt/tmp}/common-jvm.XXXXXX")}"
mkdir -p "$scratch"
trap 'rm -rf "$scratch/common-classes"' EXIT

"$JAVA_HOME/bin/java" -Xmx2g -cp "$lib/*" \
    org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
    -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
    -classpath "$lib/kotlin-stdlib.jar:$lib/annotations-13.0.jar" \
    -d "$scratch/common-classes" \
    "$common"/src/model/*.kt "$here/CommonModelTest.kt" "$here/PaletteTest.kt" "$here/FeedbackModelTest.kt"

"$JAVA_HOME/bin/java" -Xmx2g -cp "$scratch/common-classes:$lib/kotlin-stdlib.jar" \
    org.openbmt.app.CommonModelTest

# The colours: the contrast of every pair, and the theme resources made from them
# (OPENBMT_WRITE_COLOURS=1 writes res/values/colors.xml and res/values-night/colors.xml first).
"$JAVA_HOME/bin/java" -Xmx1g -cp "$scratch/common-classes:$lib/kotlin-stdlib.jar" \
    org.openbmt.app.PaletteTest "$common"

# The sounds, haptics and motion: levels, the outcome's choice, the shake, the hold and the logo's snap.
"$JAVA_HOME/bin/java" -Xmx1g -cp "$scratch/common-classes:$lib/kotlin-stdlib.jar" \
    org.openbmt.app.FeedbackModelTest
