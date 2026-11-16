#!/usr/bin/env bash
# Builds the experimental, Regtest-only checkout APK with the repository's
# pinned Android toolchain and native libraries. Nothing is downloaded here.
# Resource-heavy build execution is owned by the coordinator.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
cache="$HOME/.cache/openbmt"
guard="${OPENBMT_GUARD:-$cache/guard.sh}"
export JAVA_HOME="${JAVA_HOME:-$HOME/Android/jdk21}"
sdk="${ANDROID_SDK:-$HOME/Android/Sdk}"
ndk="${ANDROID_NDK_HOME:-$sdk/ndk/26.3.11579264}"
export KOTLIN_HOME="${KOTLIN_HOME:-$cache/tools/kotlinc}"
out="${CHECKOUTAPK_OUT:-$cache/target/checkoutapk/apk}"
cargo_target="${CHECKOUTAPK_CARGO_TARGET:-$cache/target/checkoutapk}"
keydir="${CHECKOUTAPK_KEYDIR:-$cache/tmp/checkoutapk}"
bt="$sdk/build-tools/36.0.0"
platform="$sdk/platforms/android-36/android.jar"
stdlib="$KOTLIN_HOME/lib/kotlin-stdlib.jar"
tor_aar="${TORANDROID_AAR:-$cache/deps/tor-android/tor-android-0.4.9.14.aar}"
wrapper="$repo/crates/openbmt-counter-ffi/bindings/kotlin/org/openbmt/counter"
common="$repo/apps/android-common"
tor_part="$common/tor"
min_sdk=29
target_sdk=36
abis=("arm64-v8a:aarch64-linux-android" "x86_64:x86_64-linux-android")

die() { printf 'checkout build: %s\n' "$1" >&2; exit 1; }
step() { printf '\n== %s\n' "$1"; }
heavy() { "$guard" "$@"; }

step "tools"
[ -x "$JAVA_HOME/bin/java" ] || die "no JDK at $JAVA_HOME (set JAVA_HOME to a JDK 21)"
export PATH="$JAVA_HOME/bin:$PATH"
jv="$(java -version 2>&1 | head -n 1)"
case "$jv" in *'"21.'*) ;; *) die "JDK 21 is needed, found: $jv" ;; esac
[ -f "$KOTLIN_HOME/lib/kotlin-compiler.jar" ] || die "no Kotlin compiler at $KOTLIN_HOME"
grep -q '^2.2.20' "$KOTLIN_HOME/build.txt" || die "Kotlin 2.2.20 is needed"
[ "$(sha256sum "$stdlib" | cut -d' ' -f1)" = "8836ccffd3585fadda9901244b20d42901d2f3cd581058d8434e2ffabcf3a3e7" ] || die "kotlin-stdlib.jar is not the checked 2.2.20 file"
[ -f "$platform" ] || die "no android.jar at $platform"
for t in aapt2 d8 zipalign apksigner; do [ -x "$bt/$t" ] || die "build-tools 36.0.0 has no $t"; done
[ -d "$ndk" ] || die "no NDK at $ndk"
[ -x "$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip" ] || die "the NDK has no llvm-strip"
[ -x "$guard" ] || die "no memory guard at $guard"
command -v zip >/dev/null || die "zip is needed"
command -v unzip >/dev/null || die "unzip is needed"
"$tor_part/tor-library.sh" check "$tor_aar"

rm -rf "$out"
mkdir -p "$out"/{res,classes,dex,stage} "$keydir"

step "Android native libraries"
remap="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$repo=/openbmt --remap-path-prefix=$cargo_target=/target"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384 $remap"
export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384 $remap"
if [ "${CHECKOUTAPK_SKIP_RUST:-0}" != 1 ]; then
    bin="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
    ar="$bin/llvm-ar"
    for pair in "aarch64-linux-android:aarch64-linux-android" "x86_64-linux-android:x86_64-linux-android"; do
        triple="${pair%%:*}"
        clang_triple="${pair##*:}"
        upper="$(printf '%s' "$triple" | tr 'a-z-' 'A-Z_')"
        lower="$(printf '%s' "$triple" | tr '-' '_')"
        rustup target list --installed | grep -Fxq "$triple" || die "Rust target $triple is not installed; this script will not download toolchains"
        clang="$bin/${clang_triple}26-clang"
        [ -x "$clang" ] || die "no Android linker at $clang"
        heavy env CARGO_TARGET_DIR="$cargo_target" CARGO_BUILD_JOBS=2 \
            "CARGO_TARGET_${upper}_LINKER=$clang" "CC_${lower}=$clang" "AR_${lower}=$ar" \
            cargo build --release --locked --offline -p openbmt-counter-ffi --features checkout-regtest --target "$triple"
        heavy env CARGO_TARGET_DIR="$cargo_target" CARGO_BUILD_JOBS=2 \
            "CARGO_TARGET_${upper}_LINKER=$clang" "CC_${lower}=$clang" "AR_${lower}=$ar" \
            cargo build --release --locked --offline -p openbmt-qr-jni --target "$triple"
    done
else
    echo "using coordinator-provided feature-verified checkout-regtest native libraries"
fi
strip="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
readelf="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf"
for pair in "${abis[@]}"; do
    abi="${pair%%:*}"
    triple="${pair##*:}"
    mkdir -p "$out/stage/lib/$abi"
    for lib in libopenbmt_counter_ffi.so libopenbmt_qr_jni.so; do
        so="$cargo_target/$triple/release/$lib"
        [ -f "$so" ] || die "no $lib for $abi at $so"
        cp "$so" "$out/stage/lib/$abi/"
        "$strip" --strip-unneeded "$out/stage/lib/$abi/$lib"
        align="$("$readelf" -l "$out/stage/lib/$abi/$lib" | awk '$1 == "LOAD" {print $NF}' | sort -u | tr '\n' ' ')"
        case "$align" in *0x4000*) ;; *) die "$abi $lib is not aligned for 16 KiB pages" ;; esac
        if [ "$lib" = libopenbmt_qr_jni.so ]; then
            java_symbols="$("$readelf" --dyn-syms "$out/stage/lib/$abi/$lib" | awk '$7 != "UND" && $8 ~ /^Java_/ {print $8}' | sort)"
            expected_symbols="$(printf '%s\n' Java_org_openbmt_common_QrNative_decode Java_org_openbmt_common_UrNative_call | sort)"
            [ "$java_symbols" = "$expected_symbols" ] || die "$abi QR JNI does not export exactly the two expected Java entry points"
        fi
    done
    "$tor_part/tor-library.sh" unpack "$tor_aar" "$abi" "$out/stage" "$strip" "$readelf" || die "libtor.so for $abi was not made"
done

step "resources"
"$bt/aapt2" compile --dir "$here/res" -o "$out/res.zip"
"$bt/aapt2" compile --dir "$common/res" -o "$out/common-res.zip"
"$bt/aapt2" link -o "$out/base.apk" -I "$platform" --manifest "$here/AndroidManifest.xml" \
    --min-sdk-version "$min_sdk" --target-sdk-version "$target_sdk" \
    --version-code 1 --version-name 0.1.0 "$out/res.zip" "$out/common-res.zip"

step "Kotlin"
heavy java -Xmx2g -cp "$KOTLIN_HOME/lib/*" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
    -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
    -classpath "$platform:$stdlib" -d "$out/classes" \
    "$wrapper/Native.kt" "$wrapper/CounterNode.kt" \
    "$common"/src/model/*.kt "$common"/src/android/*.kt \
    "$tor_part"/src/model/*.kt "$tor_part"/src/android/*.kt \
    "$here"/src/model/*.kt "$here"/src/android/*.kt
jar cf "$out/classes.jar" -C "$out/classes" .

step "dex"
heavy "$bt/d8" --release --min-api "$min_sdk" --lib "$platform" --output "$out/dex" "$out/classes.jar" "$stdlib"
[ -f "$out/dex/classes.dex" ] || die "d8 made no classes.dex"

step "assemble and align"
cp "$out/base.apk" "$out/unaligned.apk"
cp "$out"/dex/*.dex "$out/stage/"
epoch="${SOURCE_DATE_EPOCH:-$(git -C "$repo" log -1 --format=%ct 2>/dev/null || true)}"
[ -n "$epoch" ] || die "no commit time and no SOURCE_DATE_EPOCH"
find "$out/stage" -type f -exec chmod 644 {} +
find "$out/stage" -exec touch -h -d "@$epoch" {} +
export TZ=UTC LC_ALL=C
(
    cd "$out/stage"
    zip -q -X -0 ../unaligned.apk lib/*/*.so
    zip -q -X ../unaligned.apk ./*.dex
)
"$bt/zipalign" -P 16 -f 4 "$out/unaligned.apk" "$out/aligned.apk"
"$bt/zipalign" -c -P 16 4 "$out/aligned.apk" || die "the APK is not aligned"

step "test-only signature"
ks="$keydir/test-key.p12"
pass_file="$keydir/test-key.pass"
if [ ! -f "$ks" ]; then
    umask 077
    head -c 18 /dev/urandom | base64 | tr -d '/+=\n' > "$pass_file"
    keytool -genkeypair -keystore "$ks" -storetype PKCS12 -storepass:file "$pass_file" -keypass:file "$pass_file" \
        -alias checkout-test -keyalg RSA -keysize 3072 -validity 3650 \
        -dname "CN=OpenBMT Checkout local test key, O=test only" >/dev/null 2>&1 || die "keytool could not make the test key"
fi
"$bt/apksigner" sign --ks "$ks" --ks-pass "file:$pass_file" --ks-key-alias checkout-test \
    --min-sdk-version "$min_sdk" --out "$out/openbmt-checkout-test.apk" "$out/aligned.apk"
"$bt/apksigner" verify "$out/openbmt-checkout-test.apk" || die "signature verification failed"
echo "TEST ONLY APK: $out/openbmt-checkout-test.apk"
echo "sha256: $(sha256sum "$out/openbmt-checkout-test.apk" | cut -d' ' -f1)"
