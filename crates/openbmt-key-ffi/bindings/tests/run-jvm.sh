#!/usr/bin/env bash
set -euo pipefail

: "${JAVA_HOME:?Set JAVA_HOME to a JDK 21 installation}"
: "${KOTLIN_COMPILER_CP:?Set KOTLIN_COMPILER_CP to Kotlin 2.2.20 compiler and runtime jars}"
: "${KOTLIN_STDLIB_JAR:?Set KOTLIN_STDLIB_JAR to kotlin-stdlib 2.2.20}"
: "${KOTLIN_ANNOTATIONS_JAR:?Set KOTLIN_ANNOTATIONS_JAR to JetBrains annotations}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

"$JAVA_HOME/bin/java" -cp "$KOTLIN_COMPILER_CP" \
  org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
  -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
  -classpath "$KOTLIN_STDLIB_JAR:$KOTLIN_ANNOTATIONS_JAR" \
  -d "$scratch" \
  "$repo_root/crates/openbmt-key-ffi/bindings/kotlin/org/openbmt/key/Native.kt" \
  "$repo_root/crates/openbmt-key-ffi/bindings/kotlin/org/openbmt/key/KeyApp.kt" \
  "$repo_root/crates/openbmt-key-ffi/bindings/tests/ProtocolDecoderTest.kt"

"$JAVA_HOME/bin/java" -cp "$scratch:$KOTLIN_STDLIB_JAR" \
  org.openbmt.key.ProtocolDecoderTest
