#!/usr/bin/env bash
# The encrypted transport (BIP324) against Bitcoin Core, on regtest.
#
# Question: does the counter's peer session and network driver speak the
# encrypted transport with Bitcoin Core, derive the same session ID, and
# fall back to the old transport once, and only once, against a node that
# does not speak it?
#
# What it does: runs the two interop checks twice each, against a node with
# -v2transport=1 and against a node with -v2transport=0.
#
#   scripts/net-interop-core.sh     the session (steps 1 to 9; step 2 checks
#                                   the transport and the NR-81 fallback,
#                                   step 2b compares the session ID)
#   scripts/netio-interop-core.sh   the driver (steps 1 to 8; step 8 checks
#                                   that the connection after a drop goes
#                                   straight to the transport that worked)
#
# Needs: Bitcoin Core 32.1 in ~/.local/opt/bitcoin-core-32.1/bin (see the
# two scripts), jq, and cargo unless PEER and NODE_BIN name the prebuilt
# `regtest_peer` and `regtest_node` example binaries.
#
# Environment: the variables of the two scripts. The ports are fixed here,
# so that the four runs follow each other without a clash.
#
# Everything here is regtest on 127.0.0.1. No real keys or funds are
# involved.

set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)

for V2T in 1 0; do
  printf '\n######## net-interop-core.sh with -v2transport=%s\n' "$V2T"
  DRILL_V2TRANSPORT=$V2T DRILL_RPC_PORT=18946 DRILL_P2P_PORT=18947 \
    bash "$HERE/net-interop-core.sh"
  printf '\n######## netio-interop-core.sh with -v2transport=%s\n' "$V2T"
  DRILL_V2TRANSPORT=$V2T DRILL_RPC_PORT=18956 DRILL_P2P_PORT=18957 \
    bash "$HERE/netio-interop-core.sh"
done

printf '\nALL PASS (v2transport=1 and v2transport=0)\n'
