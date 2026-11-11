#!/usr/bin/env bash
# The QR reader library for Android (both apps): the shared library for aarch64 and
# x86_64, API level 26, linked with the NDK's clang, and a check of what it
# exports.
#
# The .so files stay in the target directory and are not committed. The
# check uses the NDK's own llvm-readelf: the only exported symbol whose name
# starts with Java_ must be the one entry point of the library, and no
# symbol of the former binding generator may remain.
#
# Every cargo call runs under the memory guard. Prints one PASS, FAIL or
# NOT RUN line for each target. Nothing is downloaded except the Rust
# standard library of the Android targets (rustup) and crates.io crates.
#
# Usage: scripts/qr-jni-build.sh
#   ANDROID_NDK_HOME  the NDK (default: ~/Android/Sdk/ndk/26.3.11579264)
#   OPENBMT_GUARD     the memory guard (default: ~/.cache/openbmt/guard.sh)
#   OPENBMT_QRJNI_TARGET  the cargo target directory
#                     (default: ~/.cache/openbmt/target/qrjni)

set -u

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
guard="${OPENBMT_GUARD:-$HOME/.cache/openbmt/guard.sh}"
target_dir="${OPENBMT_QRJNI_TARGET:-$HOME/.cache/openbmt/target/qrjni}"
scratch="$HOME/.cache/openbmt/qrjni"
ndk="${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/26.3.11579264}"
bin="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
api=26
entry="Java_org_openbmt_common_QrNative_decode"

mkdir -p "$scratch"
cd "$root" || exit 2
status=0

report() { # PASS|FAIL|"NOT RUN" step detail
    printf '%s  %s  %s\n' "$1" "$2" "$3"
    [ "$1" = FAIL ] && status=1
}

android() { # rust-triple clang-triple
    local triple="$1" clang_triple="$2"
    local upper lower clang ar
    upper="$(printf '%s' "$triple" | tr 'a-z-' 'A-Z_')"
    lower="$(printf '%s' "$triple" | tr '-' '_')"
    clang="$bin/${clang_triple}${api}-clang"
    ar="$bin/llvm-ar"
    if [ ! -x "$clang" ]; then
        report "NOT RUN" "android $triple" "no linker at $clang"
        return
    fi
    if ! rustup target add "$triple" >"$scratch/rustup-$triple.log" 2>&1; then
        report "NOT RUN" "android $triple" "rustup target add failed: $(tail -n 3 "$scratch/rustup-$triple.log" | tr '\n' ' ')"
        return
    fi
    if ! "$guard" env CARGO_TARGET_DIR="$target_dir" CARGO_BUILD_JOBS=2 \
        "CARGO_TARGET_${upper}_LINKER=$clang" "CC_${lower}=$clang" "AR_${lower}=$ar" \
        cargo build --release --locked -p openbmt-qr-jni --target "$triple" \
        >"$scratch/android-$triple.log" 2>&1; then
        report FAIL "android $triple" "build failed; see $scratch/android-$triple.log: $(grep -m1 '^error' "$scratch/android-$triple.log")"
        return
    fi
    local so="$target_dir/$triple/release/libopenbmt_qr_jni.so"
    local size defined java uniffi
    size="$(stat -c %s "$so")"
    # Defined (not UND) dynamic symbols: the name is the last column.
    defined="$("$bin/llvm-readelf" --dyn-syms "$so" | awk '$7 != "UND" && $8 != "" {print $8}')"
    java="$(printf '%s\n' "$defined" | grep -c '^Java_')"
    uniffi="$(printf '%s\n' "$defined" | grep -ci 'uniffi')"
    if [ "$java" -eq 1 ] && printf '%s\n' "$defined" | grep -qx "$entry" && [ "$uniffi" -eq 0 ]; then
        report PASS "android $triple" "$so, $size bytes, exports $entry only, no uniffi symbol"
        printf '%s\n' "$defined" | sed 's/^/    exported: /'
    else
        report FAIL "android $triple" "$so, $size bytes, $java Java_ symbols, $uniffi uniffi symbols"
        printf '%s\n' "$defined" | sed 's/^/    exported: /'
    fi
}

android aarch64-linux-android aarch64-linux-android
android x86_64-linux-android x86_64-linux-android

exit "$status"
