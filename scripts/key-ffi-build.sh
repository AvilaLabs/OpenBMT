#!/usr/bin/env bash
# The Key app's FFI library: Kotlin bindings and Android libraries.
#
# Step 2: the Kotlin bindings, generated twice by the bindgen binary of
# crates/openbmt-key-ffi. They are written to
# crates/openbmt-key-ffi/bindings/kotlin/ only if both runs give the same
# bytes.
# Step 3: the shared library for aarch64 and x86_64 Android, API level 26,
# linked with the NDK's clang. The .so files stay in the target directory
# and are not committed. The UniFFI symbols are listed with the NDK's own
# llvm-readelf.
#
# Every cargo call runs under the memory guard. Prints one PASS, FAIL or
# NOT RUN line for each step. Nothing is downloaded except the Rust
# standard library of the Android targets (rustup) and crates.io crates.
#
# Usage: scripts/key-ffi-build.sh
#   ANDROID_NDK_HOME  the NDK (default: ~/Android/Sdk/ndk/26.3.11579264)
#   OPENBMT_GUARD     the memory guard (default: ~/.cache/openbmt/guard.sh)
#   OPENBMT_KEY_TARGET  the cargo target directory
#                     (default: ~/.cache/openbmt/target/keyffi)

set -u

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
guard="${OPENBMT_GUARD:-$HOME/.cache/openbmt/guard.sh}"
target_dir="${OPENBMT_KEY_TARGET:-$HOME/.cache/openbmt/target/keyffi}"
scratch="$HOME/.cache/openbmt/keyffi"
ndk="${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/26.3.11579264}"
bin="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
api=26
out="$root/crates/openbmt-key-ffi/bindings/kotlin"
lib="$target_dir/debug/libopenbmt_key_ffi.so"

mkdir -p "$scratch"
cd "$root" || exit 2
status=0

report() { # PASS|FAIL|"NOT RUN" step detail
    printf '%s  %s  %s\n' "$1" "$2" "$3"
    [ "$1" = FAIL ] && status=1
}

cargo_guarded() {
    "$guard" env CARGO_TARGET_DIR="$target_dir" CARGO_BUILD_JOBS=2 "$@"
}

# ---- step 2: Kotlin bindings -----------------------------------------------

if cargo_guarded cargo build -p openbmt-key-ffi --features bindgen --bin uniffi-bindgen \
    >"$scratch/host-build.log" 2>&1 &&
    cargo_guarded cargo build -p openbmt-key-ffi >>"$scratch/host-build.log" 2>&1; then
    rm -rf "$scratch/run1" "$scratch/run2"
    gen() {
        "$target_dir/debug/uniffi-bindgen" generate --library "$lib" \
            --config "$root/crates/openbmt-key-ffi/uniffi.toml" \
            --language kotlin --no-format --out-dir "$1" >"$1.log" 2>&1
    }
    if gen "$scratch/run1" && gen "$scratch/run2"; then
        f1="$(find "$scratch/run1" -name '*.kt' | sort)"
        f2="$(find "$scratch/run2" -name '*.kt' | sort)"
        if [ -n "$f1" ] && [ "$(cd "$scratch/run1" && sha256sum $(find . -name '*.kt' | sort))" = \
            "$(cd "$scratch/run2" && sha256sum $(find . -name '*.kt' | sort))" ]; then
            rm -rf "$out"
            mkdir -p "$out"
            cp -r "$scratch/run1/." "$out/"
            report PASS "bindings" "two runs identical; written to crates/openbmt-key-ffi/bindings/kotlin/"
            (cd "$out" && sha256sum $(find . -name '*.kt' | sort))
        else
            report FAIL "bindings" "the two runs differ; not written (see $scratch/run1 and run2)"
        fi
    else
        report FAIL "bindings" "bindgen failed; see $scratch/run1.log"
    fi
else
    report FAIL "bindings" "the host build failed; see $scratch/host-build.log"
fi

# ---- step 3: Android libraries -----------------------------------------------

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
        cargo build --release -p openbmt-key-ffi --target "$triple" \
        >"$scratch/android-$triple.log" 2>&1; then
        report FAIL "android $triple" "build failed; see $scratch/android-$triple.log: $(grep -m1 '^error' "$scratch/android-$triple.log")"
        return
    fi
    local so="$target_dir/$triple/release/libopenbmt_key_ffi.so"
    local size symbols
    size="$(stat -c %s "$so")"
    symbols="$("$bin/llvm-readelf" --dyn-syms "$so" | grep -c ' uniffi_openbmt_key_ffi_')"
    if [ "$symbols" -gt 0 ]; then
        report PASS "android $triple" "$so, $size bytes, $symbols exported uniffi_openbmt_key_ffi_ symbols"
        "$bin/llvm-readelf" --dyn-syms "$so" | grep -E ' (uniffi_openbmt_key_ffi_(fn_(constructor|method)_keyapp_(open|state|open_session)|fn_method_sessionhandle_(lines|decide|finish))|ffi_openbmt_key_ffi_uniffi_contract_version)'
    else
        report FAIL "android $triple" "$so, $size bytes, no exported uniffi symbols"
    fi
}

android aarch64-linux-android aarch64-linux-android
android x86_64-linux-android x86_64-linux-android

exit "$status"
