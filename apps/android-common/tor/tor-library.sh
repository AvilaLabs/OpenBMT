#!/usr/bin/env bash
# The built-in Tor's native library, libtor.so, for the owner app's and the
# counter app's builds (never the Key app's, which makes no network
# connection). It is taken from the pinned tor-android AAR
# (docs/DEPENDENCIES.md): the AAR's SHA-256 is checked first and a mismatch
# stops the build. Nothing else of that AAR is used.
#
# Usage:
#   apps/android-common/tor/tor-library.sh check AAR
#   apps/android-common/tor/tor-library.sh unpack AAR ABI DIR LLVM_STRIP LLVM_READELF
#
# `check` stops unless AAR is the pinned file. `unpack` checks it again,
# takes `jni/ABI/libtor.so` out of it into DIR/lib/ABI, strips it and says its
# size and the alignment of its segments. Nothing is downloaded.

set -euo pipefail

aar_sha256="28ff2bb6d4d42eabe57183dedd7a97fce77e8ce4b6f5af6fa03993b3ca054edd"

die() { printf 'tor-library.sh: %s\n' "$1" >&2; exit 1; }

check() {
    local aar="$1"
    [ -f "$aar" ] || die "no tor-android AAR at $aar (set TORANDROID_AAR; see docs/DEPENDENCIES.md)"
    [ "$(sha256sum "$aar" | cut -d' ' -f1)" = "$aar_sha256" ] || die "$aar is not the pinned tor-android 0.4.9.14 AAR (SHA-256 differs)"
}

case "${1:-}" in
check)
    [ $# -eq 2 ] || die "usage: tor-library.sh check AAR"
    check "$2"
    echo "tor-android 0.4.9.14 AAR, sha256 checked"
    ;;
unpack)
    [ $# -eq 6 ] || die "usage: tor-library.sh unpack AAR ABI DIR LLVM_STRIP LLVM_READELF"
    tor_aar="$2" abi="$3" dir="$4" strip="$5" readelf="$6"
    check "$tor_aar"
    command -v unzip >/dev/null || die "unzip is needed"
    mkdir -p "$dir/lib/$abi"
    # The built-in Tor's library, from the checked AAR and nothing else of it.
    unzip -q -o -j "$tor_aar" "jni/$abi/libtor.so" -d "$dir/lib/$abi" || die "the tor-android AAR has no libtor.so for $abi"
    "$strip" --strip-unneeded "$dir/lib/$abi/libtor.so"
    align="$("$readelf" -l "$dir/lib/$abi/libtor.so" | awk '$1 == "LOAD" {print $NF}' | sort -u | tr '\n' ' ')"
    echo "$abi libtor.so: $(stat -c %s "$dir/lib/$abi/libtor.so") bytes stripped, LOAD alignment: $align"
    case "$align" in *0x4000*) ;; *) echo "note: $abi libtor.so segments are not aligned to 16 KiB" ;; esac
    ;;
*)
    die "usage: tor-library.sh check AAR | unpack AAR ABI DIR LLVM_STRIP LLVM_READELF"
    ;;
esac
