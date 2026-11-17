#!/usr/bin/env bash
# Pure JVM checkout model tests. Outputs go to a caller-provided scratch directory or /tmp.
set -euo pipefail

: "${JAVA_HOME:?Set JAVA_HOME to JDK 21}"
: "${KOTLIN_HOME:?Set KOTLIN_HOME to Kotlin 2.2.20}"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
app="$(dirname "$here")"
lib="$KOTLIN_HOME/lib"
scratch="${1:-$(mktemp -d "${TMPDIR:-/tmp}/checkout-model.XXXXXX")}"
mkdir -p "$scratch"
rm -rf "$scratch/classes"
runner="$scratch/CheckoutTestRunner.kt"
cat > "$runner" <<'KOTLIN'
package org.openbmt.checkout

object CheckoutTestRunner {
    @JvmStatic
    fun main(args: Array<String>) {
        CheckoutLedgerTest.main(args)
        CheckoutEventsTest.run()
        CheckoutHistoryTest.run()
        CheckoutReceiptTest.run()
        CheckoutPricingTest.run()
        CheckoutCatalogTest.run()
        CheckoutInventoryTest.run()
        CheckoutCashSessionTest.run()
        CheckoutReportTest.run()
        CheckoutAuthorityTest.run()
        CheckoutAuthorityAuditJournalTest.run()
        CheckoutAuthorityControllerTest.run()
    }
}
KOTLIN

"$JAVA_HOME/bin/java" -cp "$lib/*" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
  -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
  -classpath "$lib/kotlin-stdlib.jar:$lib/annotations-13.0.jar" \
  -d "$scratch/classes" \
  "$app"/src/model/*.kt "$here"/*.kt "$runner"

"$JAVA_HOME/bin/java" -cp "$scratch/classes:$lib/kotlin-stdlib.jar" \
  org.openbmt.checkout.CheckoutTestRunner
