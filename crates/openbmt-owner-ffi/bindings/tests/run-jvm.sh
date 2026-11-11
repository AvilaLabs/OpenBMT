#!/usr/bin/env bash
set -euo pipefail

: "${JAVA_HOME:?Set JAVA_HOME to a JDK 21 installation}"
: "${KOTLIN_COMPILER_CP:?Set KOTLIN_COMPILER_CP to Kotlin 2.2.20 compiler and runtime jars}"
: "${KOTLIN_STDLIB_JAR:?Set KOTLIN_STDLIB_JAR to kotlin-stdlib 2.2.20}"
: "${KOTLIN_ANNOTATIONS_JAR:?Set KOTLIN_ANNOTATIONS_JAR to JetBrains annotations}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# The key ceremony run of the harness calls the real library: the directory
# that holds libopenbmt_owner_ffi.so (the host build of the crate). When it is
# not given, the crate is built here, into the target directory cargo uses.
lib_dir="${OWNER_FFI_LIB_DIR:-}"
if [ -z "$lib_dir" ]; then
  (cd "$repo_root" && cargo build --locked -p openbmt-owner-ffi)
  lib_dir="${CARGO_TARGET_DIR:-$repo_root/target}/debug"
fi
test -f "$lib_dir/libopenbmt_owner_ffi.so" || {
  echo "no libopenbmt_owner_ffi.so in $lib_dir" >&2
  exit 1
}

"$JAVA_HOME/bin/java" -cp "$KOTLIN_COMPILER_CP" \
  org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
  -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
  -classpath "$KOTLIN_STDLIB_JAR:$KOTLIN_ANNOTATIONS_JAR" \
  -d "$scratch" \
  "$repo_root/crates/openbmt-owner-ffi/bindings/kotlin/org/openbmt/owner/Native.kt" \
  "$repo_root/crates/openbmt-owner-ffi/bindings/kotlin/org/openbmt/owner/OwnerNode.kt" \
  "$repo_root/crates/openbmt-owner-ffi/bindings/tests/ProtocolDecoderTest.kt"

"$JAVA_HOME/bin/java" -Djava.library.path="$lib_dir" \
  -cp "$scratch:$KOTLIN_STDLIB_JAR" \
  org.openbmt.owner.ProtocolDecoderTest
