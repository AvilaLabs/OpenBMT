#!/usr/bin/env bash
# The web demo: the demo crate built for WebAssembly (release, size-optimised,
# with link-time optimisation), then the wasm, the page's files and the words
# the apps write copied into dist/demo/. The folder is static files only:
# serve it from any web server, or open index.html.
#
# The words of the apps' own states are read from demo/words/*.json, which the
# JVM tests of the two apps write (OPENBMT_WRITE_SCREENS=1); this script does
# not make them. The library's words are in the wasm.
#
# The C code of the secp256k1 package needs a clang that can target wasm32.
# CC_wasm32_unknown_unknown and AR_wasm32_unknown_unknown name one; with none
# set, the Android NDK's clang and llvm-ar are used. Nothing is downloaded.
#
# The counter's own library, without its network side, is built the same way
# (crates/openbmt-demo-counter) into dist/demo-counter/: counter.wasm and
# counter-host.mjs, the page's side of the counter's one native call.
#
# The owner's own library, without its network side, is built the same way
# (crates/openbmt-demo-owner) into dist/demo-owner/: owner.wasm and
# owner-host.mjs, the page's side of the owner's one native call.
#
# The cargo calls run under the memory guard.
#
# Usage: scripts/demo-build.sh
#   OPENBMT_GUARD      the memory guard (default: ~/.cache/openbmt/guard.sh)
#   OPENBMT_DEMO_TARGET  the cargo target directory
#                      (default: ~/.cache/openbmt/target/demo)
#   ANDROID_NDK_HOME   the NDK (default: ~/Android/Sdk/ndk/26.3.11579264)

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
guard="${OPENBMT_GUARD:-$HOME/.cache/openbmt/guard.sh}"
target_dir="${OPENBMT_DEMO_TARGET:-$HOME/.cache/openbmt/target/demo}"
ndk="${ANDROID_NDK_HOME:-$HOME/Android/Sdk/ndk/26.3.11579264}"
bin="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"
out="$root/dist/demo"
counter_out="$root/dist/demo-counter"
owner_out="$root/dist/demo-owner"
cd "$root"

if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
    echo "NOT RUN  the wasm32-unknown-unknown target is not installed (rustup target add wasm32-unknown-unknown)"
    exit 2
fi
cc="${CC_wasm32_unknown_unknown:-$bin/clang}"
ar="${AR_wasm32_unknown_unknown:-$bin/llvm-ar}"
if [ ! -x "$cc" ] || [ ! -x "$ar" ]; then
    echo "NOT RUN  no clang and llvm-ar for wasm32 (set CC_wasm32_unknown_unknown and AR_wasm32_unknown_unknown)"
    exit 2
fi
for words in counter-app owner-app; do
    if [ ! -s "demo/words/$words.json" ]; then
        echo "NOT RUN  demo/words/$words.json is missing (run the app's JVM tests with OPENBMT_WRITE_SCREENS=1)"
        exit 2
    fi
done

# Two calls, not one: built together, the demo crate's exports would be
# switched on in the counter's module too (one set of features for a build).
build() {
    "$guard" env CARGO_TARGET_DIR="$target_dir" CARGO_BUILD_JOBS=2 \
        "CC_wasm32_unknown_unknown=$cc" "AR_wasm32_unknown_unknown=$ar" \
        CARGO_PROFILE_RELEASE_OPT_LEVEL=s CARGO_PROFILE_RELEASE_LTO=true \
        CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_PANIC=abort \
        CARGO_PROFILE_RELEASE_STRIP=true \
        cargo build --release --locked --target wasm32-unknown-unknown "$@"
}
build -p openbmt-demo
build -p openbmt-demo-counter
build -p openbmt-demo-owner

rm -rf "$out"
mkdir -p "$out/words"
cp "$target_dir/wasm32-unknown-unknown/release/openbmt_demo.wasm" "$out/demo.wasm"
cp demo/web/screens.css demo/web/screens.js demo/web/demo.js demo/web/favicon.svg demo/web/avila-mark.png "$out/"
if [ -d demo/web/fonts ]; then cp -r demo/web/fonts "$out/"; fi
# The shell page is written apart from these; without it, the development page stands in.
for f in index.html page.css page.js; do
    if [ -f "demo/web/$f" ]; then cp "demo/web/$f" "$out/"; fi
done
if [ ! -f demo/web/index.html ]; then cp demo/web/dev.html "$out/index.html"; fi
cp demo/words/counter-app.json demo/words/owner-app.json "$out/words/"
rm -rf "$counter_out"
mkdir -p "$counter_out"
cp "$target_dir/wasm32-unknown-unknown/release/openbmt_demo_counter.wasm" "$counter_out/counter.wasm"
cp demo/web2/counter-host.mjs "$counter_out/"
rm -rf "$owner_out"
mkdir -p "$owner_out"
cp "$target_dir/wasm32-unknown-unknown/release/openbmt_demo_owner.wasm" "$owner_out/owner.wasm"
cp demo/web2/owner-host.mjs "$owner_out/"
echo "PASS  dist/demo-owner ($(wc -c <"$owner_out/owner.wasm") bytes of wasm)"
echo "PASS  dist/demo-counter ($(wc -c <"$counter_out/counter.wasm") bytes of wasm)"
echo "PASS  dist/demo ($(wc -c <"$out/demo.wasm") bytes of wasm)"
