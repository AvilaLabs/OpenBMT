#!/usr/bin/env bash
# Builds the OpenBMT Counter app's APK with the Android SDK's own tools and no
# Gradle: the Rust library for two ABIs (scripts/counter-ffi-build.sh), resources
# (aapt2), Kotlin (kotlinc), dex (d8), assembly (zip), alignment (zipalign
# with 16 KiB pages) and a signature (apksigner) with a local test key from
# keytool. The test key is made once under ~/.cache/openbmt/tmp/counterapk/ and is
# never committed; it is not a release key.
#
# The built-in Tor's native library, libtor.so, is taken from the pinned
# tor-android AAR (docs/DEPENDENCIES.md) by apps/android-common/tor/tor-library.sh,
# which the owner app's build calls too: the AAR's SHA-256 is checked first and
# a mismatch stops the build. Nothing else of that AAR is used.
#
# Every step stops the script with a message when it fails. Nothing is
# downloaded. Heavy steps run under the memory guard.
#
# Usage: apps/counter-android/build.sh
#   JAVA_HOME         a JDK 21           (default: ~/Android/jdk21)
#   ANDROID_SDK       the SDK            (default: ~/Android/Sdk)
#   ANDROID_NDK_HOME  the NDK            (default: $ANDROID_SDK/ndk/26.3.11579264)
#   KOTLIN_HOME       Kotlin 2.2.20      (default: ~/.cache/openbmt/tools/kotlinc)
#   OPENBMT_GUARD     the memory guard   (default: ~/.cache/openbmt/guard.sh)
#   COUNTERAPK_OUT        build directory    (default: ~/.cache/openbmt/target/counterapk/apk)
#   TORANDROID_AAR    the tor-android AAR (default: ~/.cache/openbmt/deps/tor-android/tor-android-0.4.9.14.aar)
#   COUNTERAPK_SKIP_RUST  set to 1 to reuse the .so files of an earlier run
#   COUNTERAPK_CARGO_TARGET  the cargo target directory (default: ~/.cache/openbmt/target/counterapk)

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
cache="$HOME/.cache/openbmt"
guard="${OPENBMT_GUARD:-$cache/guard.sh}"
export JAVA_HOME="${JAVA_HOME:-$HOME/Android/jdk21}"
sdk="${ANDROID_SDK:-$HOME/Android/Sdk}"
ndk="${ANDROID_NDK_HOME:-$sdk/ndk/26.3.11579264}"
export KOTLIN_HOME="${KOTLIN_HOME:-$cache/tools/kotlinc}"
out="${COUNTERAPK_OUT:-$cache/target/counterapk/apk}"
cargo_target="${COUNTERAPK_CARGO_TARGET:-$cache/target/counterapk}"
keydir="$cache/tmp/counterapk"
bt="$sdk/build-tools/36.0.0"
platform="$sdk/platforms/android-36/android.jar"
stdlib="$KOTLIN_HOME/lib/kotlin-stdlib.jar"
tor_aar="${TORANDROID_AAR:-$cache/deps/tor-android/tor-android-0.4.9.14.aar}"
kotlin_version="2.2.20"
stdlib_sha256="8836ccffd3585fadda9901244b20d42901d2f3cd581058d8434e2ffabcf3a3e7"
wrapper="$repo/crates/openbmt-counter-ffi/bindings/kotlin/org/openbmt/counter"
common="$repo/apps/android-common"
tor_part="$common/tor"
min_sdk=29
target_sdk=36
abis=("arm64-v8a:aarch64-linux-android" "x86_64:x86_64-linux-android")

die() { printf 'build.sh: %s\n' "$1" >&2; exit 1; }
step() { printf '\n== %s\n' "$1"; }
heavy() { "$guard" "$@"; }

# ---- the tools ------------------------------------------------------------------

step "tools"
[ -x "$JAVA_HOME/bin/java" ] || die "no JDK at $JAVA_HOME (set JAVA_HOME to a JDK 21)"
export PATH="$JAVA_HOME/bin:$PATH"
jv="$(java -version 2>&1 | head -n 1)"
case "$jv" in *'"21.'*) ;; *) die "JDK 21 is needed, found: $jv" ;; esac
for t in javac keytool jar; do [ -x "$JAVA_HOME/bin/$t" ] || die "the JDK has no $t"; done
[ -f "$KOTLIN_HOME/lib/kotlin-compiler.jar" ] || die "no Kotlin compiler at $KOTLIN_HOME (set KOTLIN_HOME)"
grep -q "^$kotlin_version" "$KOTLIN_HOME/build.txt" || die "Kotlin $kotlin_version is needed, found: $(cat "$KOTLIN_HOME/build.txt")"
[ "$(sha256sum "$stdlib" | cut -d' ' -f1)" = "$stdlib_sha256" ] || die "kotlin-stdlib.jar is not the checked $kotlin_version file"
[ -f "$platform" ] || die "no android.jar at $platform (platform android-36 is needed)"
for t in aapt2 d8 zipalign apksigner; do [ -x "$bt/$t" ] || die "build-tools 36.0.0 has no $t at $bt"; done
[ -d "$ndk" ] || die "no NDK at $ndk (set ANDROID_NDK_HOME)"
[ -x "$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip" ] || die "the NDK has no llvm-strip"
[ -x "$guard" ] || die "no memory guard at $guard (set OPENBMT_GUARD)"
command -v zip >/dev/null || die "zip is needed"
command -v unzip >/dev/null || die "unzip is needed"
echo "$jv"
"$tor_part/tor-library.sh" check "$tor_aar"
echo "kotlin $kotlin_version ($(cat "$KOTLIN_HOME/build.txt")), stdlib sha256 checked"
echo "aapt2: $("$bt/aapt2" version 2>&1 | head -n 1)"
echo "d8: $("$bt/d8" --version 2>&1 | head -n 1)"
echo "apksigner: $("$bt/apksigner" --version 2>&1 | head -n 1)"
echo "platform: android-36, minSdk $min_sdk, targetSdk $target_sdk"

rm -rf "$out"
mkdir -p "$out"/{res,classes,dex,stage} "$keydir"

# ---- the tests of the pure parts -----------------------------------------------------

step "JVM tests"
heavy "$here/tests/run-jvm.sh" "$out/jvm"
# The camera scan's native reader, built for the host and run on frames made by the encoder
# (it guards its own cargo and kotlinc calls).
QRJNI_HOST_TARGET="$cargo_target" "$common/tests/qr-jni-test.sh" "$out/qrjni-jvm"

# ---- the Rust library ------------------------------------------------------------------

step "Rust library"
if [ "${COUNTERAPK_SKIP_RUST:-0}" != 1 ]; then
    # Page size 16 KiB for the segments, so the library loads on 16 KiB devices.
    export CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"
    export CARGO_TARGET_X86_64_LINUX_ANDROID_RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384"
    ANDROID_NDK_HOME="$ndk" OPENBMT_COUNTER_TARGET="$cargo_target" "$repo/scripts/counter-ffi-build.sh" | tee "$out/ffi-build.log"
    grep -q -E '^(FAIL|NOT RUN)' "$out/ffi-build.log" && die "the Rust library did not build"
    # The QR reader both apps use for the camera scan (the same target directory, the same page size).
    ANDROID_NDK_HOME="$ndk" OPENBMT_QRJNI_TARGET="$cargo_target" "$repo/scripts/qr-jni-build.sh" | tee "$out/qr-jni-build.log"
    grep -q -E '^(FAIL|NOT RUN)' "$out/qr-jni-build.log" && die "the QR reader library did not build"
fi
strip="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip"
readelf="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf"
for pair in "${abis[@]}"; do
    abi="${pair%%:*}"
    triple="${pair##*:}"
    mkdir -p "$out/stage/lib/$abi"
    for lib in libopenbmt_counter_ffi.so libopenbmt_qr_jni.so; do
        so="$cargo_target/$triple/release/$lib"
        [ -f "$so" ] || die "no library $lib for $abi at $so"
        cp "$so" "$out/stage/lib/$abi/"
        "$strip" --strip-unneeded "$out/stage/lib/$abi/$lib"
        align="$("$readelf" -l "$out/stage/lib/$abi/$lib" | awk '$1 == "LOAD" {print $NF}' | sort -u | tr '\n' ' ')"
        echo "$abi $lib: $(stat -c %s "$out/stage/lib/$abi/$lib") bytes stripped, LOAD alignment: $align"
        case "$align" in *0x4000*) ;; *) echo "note: $abi $lib segments are not aligned to 16 KiB" ;; esac
    done
    # The built-in Tor's library, from the checked AAR and nothing else of it.
    "$tor_part/tor-library.sh" unpack "$tor_aar" "$abi" "$out/stage" "$strip" "$readelf" || die "libtor.so for $abi was not made"
done

# ---- resources --------------------------------------------------------------------------

step "resources"
"$bt/aapt2" compile --dir "$here/res" -o "$out/res.zip"
# The theme and the colours the three apps share (colors.xml is made from Palette.kt).
"$bt/aapt2" compile --dir "$common/res" -o "$out/common-res.zip"
"$bt/aapt2" link -o "$out/base.apk" -I "$platform" --manifest "$here/AndroidManifest.xml" \
    --min-sdk-version "$min_sdk" --target-sdk-version "$target_sdk" \
    --version-code 1 --version-name 0.1.0 "$out/res.zip" "$out/common-res.zip"

# ---- Kotlin -----------------------------------------------------------------------------

step "Kotlin"
heavy java -Xmx2g -cp "$KOTLIN_HOME/lib/*" org.jetbrains.kotlin.cli.jvm.K2JVMCompiler \
    -no-stdlib -no-reflect -Werror -jvm-target 1.8 \
    -classpath "$platform:$stdlib" \
    -d "$out/classes" \
    "$wrapper/Native.kt" "$wrapper/CounterNode.kt" "$common"/src/model/*.kt "$common"/src/android/*.kt \
    "$tor_part"/src/model/*.kt "$tor_part"/src/android/*.kt \
    "$here"/src/model/*.kt "$here"/src/android/*.kt
jar cf "$out/classes.jar" -C "$out/classes" .

# ---- dex --------------------------------------------------------------------------------

step "dex"
heavy "$bt/d8" --release --min-api "$min_sdk" --lib "$platform" --output "$out/dex" "$out/classes.jar" "$stdlib"
[ -f "$out/dex/classes.dex" ] || die "d8 made no classes.dex"
ls -l "$out"/dex/*.dex

# ---- assemble, align, sign -----------------------------------------------------------------

step "assemble"
cp "$out/base.apk" "$out/unaligned.apk"
cp "$out"/dex/*.dex "$out/stage/"
(
    cd "$out/stage"
    # Libraries are stored, not compressed, so that they can be mapped from the APK
    # at 16 KiB alignment (extractNativeLibs is false in the manifest).
    zip -q -X -0 ../unaligned.apk lib/*/*.so
    zip -q -X ../unaligned.apk ./*.dex
)
"$bt/zipalign" -P 16 -f 4 "$out/unaligned.apk" "$out/aligned.apk"
"$bt/zipalign" -c -P 16 4 "$out/aligned.apk" || die "the APK is not aligned"
echo "aligned: 4 bytes for everything, 16 KiB for the stored libraries"

step "signature"
ks="$keydir/test-key.p12"
pass_file="$keydir/test-key.pass"
if [ ! -f "$ks" ]; then
    umask 077
    head -c 18 /dev/urandom | base64 | tr -d '/+=\n' > "$pass_file"
    keytool -genkeypair -keystore "$ks" -storetype PKCS12 -storepass:file "$pass_file" -keypass:file "$pass_file" \
        -alias counterapk-test -keyalg EC -groupname secp256r1 -validity 3650 \
        -dname "CN=OpenBMT Counter local test key, O=test only" >/dev/null 2>&1 || die "keytool could not make the test key"
    echo "made a local test key at $ks (not a release key; never committed)"
fi
"$bt/apksigner" sign --ks "$ks" --ks-pass "file:$pass_file" --ks-key-alias counterapk-test \
    --min-sdk-version "$min_sdk" --out "$out/openbmt-counter.apk" "$out/aligned.apk"
"$bt/apksigner" verify --print-certs -v --min-sdk-version "$min_sdk" "$out/openbmt-counter.apk" | sed -e 's/^/  /'
"$bt/zipalign" -c -P 16 4 "$out/openbmt-counter.apk" || die "the signed APK is not aligned"

# ---- checks on the result --------------------------------------------------------------------

step "aapt2 dump badging"
"$bt/aapt2" dump badging "$out/openbmt-counter.apk" | tee "$out/badging.txt"

step "manifest checks"
"$here/tests/check-manifest.sh" "$out/openbmt-counter.apk" "$bt/aapt2" | tee "$out/manifest-checks.txt"

step "result"
apk="$out/openbmt-counter.apk"
echo "APK: $apk"
echo "size: $(stat -c %s "$apk") bytes"
echo "native libraries in the APK (stored, uncompressed):"
unzip -lv "$apk" | awk '/lib\/.*\.so$/ {print "  " $NF ": " $1 " bytes"}'
echo "classes.dex: $(unzip -lv "$apk" | awk '/classes.dex$/ {print $1}') bytes"
echo "sha256: $(sha256sum "$apk" | cut -d' ' -f1)"
