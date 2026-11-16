# LSPS2 funding publication probe

This same-host Bitcoin Core regtest harness compares ordinary provider behavior
with a deliberate RPC proxy that suppresses the exact provider funding txid and
retries. It checks the payer's preimage hash, the merchant's paid/funding record,
and live Core results for that exact transaction. The `withhold` mode passes only
when Core returns exact `-5` not-found results for both mempool and raw transaction
queries; the `honest` control requires the transaction to appear in both queries.
The proxy does not suppress honest mode. The checks are not weakened by the
packaging changes.

## Requirements and run

Build the repository binaries and the reviewed regtest-only provider variant
separately. Supply explicit `SHOP_BIN`, `DEVICE_BIN`, and `LSP_BIN` executable
paths. `CORE_BIN` may name a directory containing Bitcoin Core 32.1 binaries; if
omitted, `bitcoind` and `bitcoin-cli` must be on `PATH`. `LND_DIR` may name the
LND installation directory; if omitted, `$HOME/.local/opt/lnd` is used.

The captured default-mode runs use the unmodified `tools/shop-lightning-lsp`
source (`client_trusts_lsp: false`). From the repository root, build both into one
caller-owned target directory. The first build includes substantial test-only dependencies.

```sh
investigation_target=$(mktemp -d)
cargo build --locked -p openbmt-lightning-node --features tcp-device \
  --example shop_regtest --bin shop-lightning-device --target-dir "$investigation_target" -j 1 \
  --config 'profile.dev.package.lightning.opt-level=0' \
  --config 'profile.dev.package.lightning.debug=0'
cargo build --locked --manifest-path tools/shop-lightning-lsp/Cargo.toml \
  --target-dir "$investigation_target" -j 1 \
  --config 'profile.dev.package."*".opt-level=2' \
  --config 'profile.dev.package.lightning.opt-level=0' \
  --config 'profile.dev.package.lightning.debug=0'
```

Then use `$investigation_target/debug/examples/shop_regtest`,
`$investigation_target/debug/shop-lightning-device`, and
`$investigation_target/debug/shop-lightning-lsp` for the corresponding variables.
The tested payer version is LND 0.21.8. Bash, Python 3, jq, GNU timeout and the
specified Bitcoin Core 32.1 binaries are also required.

For each run, choose a new, empty directory and pass it as `PROBE_RUN_DIR`. The
probe refuses nonempty directories and retains logs and state there. For example:

```sh
RUN_DIR=$(mktemp -d /tmp/openbmt-merchant-honest.XXXXXX)
CORE_BIN=/path/to/core-bin \
SHOP_BIN=/path/to/shop_regtest \
DEVICE_BIN=/path/to/shop-lightning-device \
LSP_BIN=/path/to/regtest-shop-lightning-lsp \
PROBE_RUN_DIR="$RUN_DIR" CONFIRM_REGTEST_PROVIDER=1 PROBE_MODE=honest \
  ./run_probe.sh
```

Run the withhold case separately with another new directory and
`PROBE_MODE=withhold`. Set `CONFIRM_REGTEST_PROVIDER=1` only after verifying that
`LSP_BIN` is the reviewed regtest-only provider binary. The script starts an
isolated private regtest chain but uses fixed ports 19700–19714; ensure they are
free and do not run a conflicting regtest harness at the same time.

## Limits

This is a same-host regtest fixture, including a local TCP device fixture. It uses
no real funds and does not test physical hardware or an actual Box service. A
payer preimage and merchant `paid` event do not mean the cashier fulfilled the
sale. Results do not establish VSS, native SDK behavior, production deployment
behavior, or ordinary provider behavior beyond the observed control run. Inspect
retained evidence before drawing conclusions.

Captured results are in [the execution summary](../evidence/funding-publication-2026-11-16.json).
