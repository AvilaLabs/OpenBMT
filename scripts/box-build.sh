#!/usr/bin/env bash
# The Box binary, built the way packaging/box/Dockerfile builds it, with the
# directories of the build mapped to fixed names so that the binary does not
# depend on where the checkout or the cargo registry is (it embeds the paths of
# its dependencies' sources otherwise). Two builds of one commit give the same
# binary; scripts/repro-check.sh checks that.
#
# Usage: scripts/box-build.sh        (CARGO_TARGET_DIR picks the target directory)
#   The binary is at $CARGO_TARGET_DIR/release/openbmt-box (default: target/).

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
target="${CARGO_TARGET_DIR:-$root/target}"
# The later mapping wins, so the narrower paths come last.
RUSTFLAGS="--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$root=/openbmt --remap-path-prefix=$target=/target" \
    exec cargo build --locked --release -p openbmt-box
