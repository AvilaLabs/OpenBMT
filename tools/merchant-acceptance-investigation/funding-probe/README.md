# LSPS2 funding publication probe

This same-host Bitcoin Core regtest harness checks the strict claim-time funding
gate. Before funding reaches the shop's configured depth, the first payment must
fail back with no payer preimage and no merchant `PaymentClaimed` record. In
`withhold` mode, a proxy suppresses the exact provider funding transaction; Core
must return exact `-5` not-found results for both mempool and raw transaction
queries. In `honest` mode, the funding transaction is visible but unconfirmed, so
the first payment must still fail. The harness mines that same transaction, then
retries the same still-live invoice and requires success with exactly one paid
record. The shop regtest example defaults to a one-confirmation claim depth.

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

Run both modes separately, each in a new, empty directory. The probe refuses
nonempty directories and retains logs and state there. For example:

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

In the honest case, a failed first payment leaves its invoice expectation in the
shop's book: the node forgets the expectation only after `PaymentClaimed`. The
probe therefore retries that invoice after mining the funding transaction. It
checks that the retry preimage matches the invoice hash, the paid record reports
`funding=confirmed`, and a second drain returns no duplicate record. This does not
establish that providers using the LSPS2 secret-first trust model can complete a
first JIT payment under the gate; a provider that waits for the claim before
broadcasting cannot satisfy the confirmation requirement.

## Limits

This is a same-host regtest fixture, including a local TCP device fixture. It uses
no real funds and does not test physical hardware or an actual Box service. A
node `PaymentClaimed` record is separate from cashier fulfillment. The result
only tests this receiver and the bundled funding-first provider on regtest; it
does not establish VSS, native SDK behavior, production deployment behavior, or
compatibility with arbitrary LSPS2 providers. Inspect retained evidence before
drawing conclusions.

The captured results in [the earlier execution summary](../evidence/funding-publication-2026-11-16.json)
predate the strict claim gate; they document the original claim-before-publication
finding and are not results of this updated probe.
