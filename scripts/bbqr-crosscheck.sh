#!/usr/bin/env bash
# Cross-check of crates/openbmt-bbqr against the reference implementation of
# BBQr in Coinkite's repository (python/bbqr, public domain), an independent
# implementation (a test tool: never shipped, not on a list in deps/).
#
# The files of the reference are fetched once, at one pinned commit, into
# ~/.cache/openbmt/tools/bbqr/ and must have the pinned SHA-256 values: its
# readers and writers (utils.py, join.py), the specification (BBQr.md) and
# its ten test files (PSBTs and transactions of 376 bytes to 100 KB).
#
#   - crosscheck/bbqr_cross.py writes, for the test files and some made-up
#     inputs, the parts the reference writes in the encodings H, 2 and Z at
#     five part sizes; `crosscheck read` joins each set with openbmt-bbqr, in
#     order and reversed, and the bytes and the type must be the same (this
#     is also the test of the inflate against zlib);
#   - `crosscheck write` writes openbmt-bbqr's parts for the same inputs at
#     the same sizes, and the reference's reader must give the same bytes.
#
#   scripts/bbqr-crosscheck.sh
#
# Needs python3 (standard library only) and cargo.
set -euo pipefail
cd "$(dirname "$0")/.."

COMMIT=8dc7ef07d0d520763cc0001a885ca8d29ac8719a
BASE="https://raw.githubusercontent.com/coinkite/BBQr/$COMMIT"
tools="${OPENBMT_TOOLS:-$HOME/.cache/openbmt/tools}/bbqr/$COMMIT"
mkdir -p "$tools/test_data"

# fetch PATH SHA256: PATH is the file's path in the reference repository.
fetch() {
    local path="$1" want="$2" dest="$tools/$1"
    mkdir -p "$(dirname "$dest")"
    if [ ! -f "$dest" ]; then
        curl --fail --silent --show-error --location --retry 6 --retry-max-time 300 \
            --output "$dest.part" "$BASE/$path"
        mv "$dest.part" "$dest"
    fi
    local got
    got="$(sha256sum "$dest" | cut -d' ' -f1)"
    if [ "$got" != "$want" ]; then
        echo "FAIL  $path has SHA-256 $got, not the pinned $want" >&2
        exit 1
    fi
}

fetch python/bbqr/consts.py 600147b178c25ff8a8a133d05c9324770fd6f4c7de4ec43c16743df2c27b1f46
fetch python/bbqr/utils.py 98ee24c30254f1e87080984a73a105b893b2aa6b1587a979d812c49cf34881e9
fetch python/bbqr/join.py 6a9c98770633ee4b52b31e26fca38bf286ba3f0e12b71157fc4a4916c1670a1d
fetch BBQr.md eda18acb6922350a2d407121332fdbf2af91843c4599c27425b55b66959f93cb
fetch test_data/1in1000out.psbt 7e064fd32aad5cf0bc4ac79fe80a8a27fb832506c2fcdc04b2591c4a87a7ca26
fetch test_data/1in100out.psbt c38b8d114150bf8908bef19d1cbb94430a9ea05bfcab2a629051820ba91245df
fetch test_data/1in10out.psbt 045b95a5c8275a3f8562840d7369cc9eb7d7d81c162a881c48b683902bdfc698
fetch test_data/1in20out.psbt 645e7bf99b52045d9a683b19d342cf9cc55c9482c50a88c1b1eb23297273aba1
fetch test_data/1in2out.psbt 2929866d35ee77eeca2f7addc3d17d81fdf777ac3208374a76f8f06ce9abf222
fetch test_data/devils-txn.txn 6de209567aabaa3f18276be099199eaf1e9c16100507228a92b12c67b197dd6f
fetch test_data/finalized-by-ckcc.txn ea0807625402361eb6168e6577bdac837793773c7bbf5bb9050fca4d93a865f7
fetch test_data/last.txn 3b4f221677384aa22a7bba33711a44c5788e604bf7287d8a4ece1b2dee77b3e5
fetch test_data/nfc-result.txn 226018f757e5c5d85924b5fbfa1951c26f1d8f197d8eae99f1136137a762fc13
fetch test_data/signed.txn a8964f7ab05ad4d23620ee762b97eac9a8db1c084fb1214a2d8f65786e0da1d7
echo "PASS  the reference files at $COMMIT have the pinned SHA-256 values"

work="$(mktemp -d "${TMPDIR:-/tmp}/bbqr-crosscheck.XXXXXX")"
trap 'rm -rf "$work"' EXIT
# The reference's modules stand in a package of their own, in the work folder.
mkdir -p "$work/ref/bbqr"
cp "$tools/python/bbqr/consts.py" "$tools/python/bbqr/utils.py" "$tools/python/bbqr/join.py" \
    "$work/ref/bbqr/"

here=crates/openbmt-bbqr
python3 -I "$here/crosscheck/bbqr_cross.py" make "$work/ref" "$tools/test_data" "$work/cases.txt"
cargo run --release --locked --quiet -p openbmt-bbqr --example crosscheck -- \
    read "$work/cases.txt"
cargo run --release --locked --quiet -p openbmt-bbqr --example crosscheck -- \
    write "$work/cases.txt" "$work/ours.txt"
python3 -I "$here/crosscheck/bbqr_cross.py" verify "$work/ref" "$work/cases.txt" "$work/ours.txt"
