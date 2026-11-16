#!/usr/bin/env bash
# Pure JVM ledger tests. Outputs go to a caller-provided scratch directory or /tmp.
set -euo pipefail

: "${JAVA_HOME:?Set JAVA_HOME to JDK 21}"
: "${KOTLIN_HOME:?Set KOTLIN_HOME to Kotlin 2.2.20}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(dirname "$here")"
lib="$KOTLIN_HOME/lib"
scratch="${1:-$(mktemp -d "${TMPDIR:-/tmp}/checkout-ledger.XXXXXX")}"
mkdir -p "$scratch"
rm -rf "$scratch/classes"

"$JAVA_HOME/bin/java" -cp "$lib/*" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
  -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
  -classpath "$lib/kotlin-stdlib.jar:$lib/annotations-13.0.jar" \
  -d "$scratch/classes" \
  "$app/src/model/CheckoutLedger.kt" "$app/tests/CheckoutLedgerTest.kt"

"$JAVA_HOME/bin/java" -cp "$scratch/classes:$lib/kotlin-stdlib.jar" \
  org.openbmt.checkout.CheckoutLedgerTest
