# Synthetic reverse swap batch claim probe

This isolated regtest crate demonstrates two unilateral Taproot script-path
claims in one transaction. It uses the repository's SW-25 tree
(`SwapTree` / `KeyOrder::RefundFirst`) and pinned `bitcoin` and `miniscript`
versions. It does not contact a swap service, exercise merchant application
integration, or prove crash recovery. Every key and preimage is a fixed,
public synthetic fixture. Never use these values for real funds.

## Build and inspect fixtures

From this directory:

```sh
cargo build --release
./target/release/batch-claim-probe prepare
```

For an automated fresh-chain run, build in the default debug target directory
and run `python3 run_probe.py`. Set `BITCOIND` to the Bitcoin Core binary when
it is not installed at `~/.local/opt/bitcoin-core-32.1/bin/bitcoind`. Set
`PROBE_BINARY` when the probe uses another target directory. The runner
preflights localhost ports 19720 and 19721 (overridable with `PROBE_RPC_PORT`
and `PROBE_P2P_PORT`), creates a fresh temporary datadir, checks the chain is
regtest, uses fixed test funding fees, and stops only its own Core process.
It keeps `result.json` and logs in the printed temporary directory. It refuses
occupied or invalid ports before starting Core. No existing datadir is reused.

`prepare` prints two swap funding addresses and their scripts, claim leaves,
control blocks and timeouts, plus a cold input address and merchant cold
destination. Claim keys and preimages differ between the swaps. Provider
refund public keys are distinct fixed valid points; provider private keys are
not included or needed. Both deadlines match, avoiding mixed-deadline claims.

## Regtest setup

Start Bitcoin Core manually with a fresh datadir and unique RPC/P2P ports.
Check both ports and the chosen directory before startup; do not overwrite or
reuse an existing datadir. Mine enough blocks for coinbase maturity. Fund the
two swap addresses independently with exactly 100,000 sats each. Confirm each
funding transaction and inspect its selected output in Core before signing.

The basic merchant-only claim command is:

```sh
./target/release/batch-claim-probe spend \
  '<raw-swap-funding-tx-hex-1>' '<vout-1>' \
  '<raw-swap-funding-tx-hex-2>' '<vout-2>' '<fee-sats>' > claim.hex
```

This pays a single fixed merchant cold destination. It builds and verifies
both claim signatures locally before emitting raw transaction hex.

## Optional bounded cold wallet coupon

This separate mode demonstrates a single cold wallet contribution that is
preauthorized before the swap outpoints are known. It is not hardware wallet
isolation: all fixed fixture secrets are in this program. Fund the printed
`coldInputAddress` with exactly 300,000 sats in a separate transaction, then
sign the coupon:

```sh
./target/release/batch-claim-probe coupon '<cold-funding-tx-hex>' '<vout>' > coupon.json
```

The coupon has one Taproot key-path input and one output of 490,000 sats to
the fixed merchant cold destination. Its signature is
`SIGHASH_SINGLE|SIGHASH_ANYONECANPAY`; the local negative checks confirm that
the same signature rejects a reduced output amount and a redirected output.
After verifying the three funding transactions in Core, append the two
100,000-sat claim inputs:

```sh
./target/release/batch-claim-probe assemble \
  '<signedCouponHex-from-coupon.json>' \
  '<cold-funding-tx-hex>' '<cold-vout>' \
  '<raw-swap-funding-tx-hex-1>' '<vout-1>' \
  '<raw-swap-funding-tx-hex-2>' '<vout-2>' > claim.hex
```

The coupon assembler verifies its known cold outpoint, value, output pair and
signature using the supplied public prevout data. Its code does not use the
cold input signing key; only coupon generation uses that key. It appends the
two unilateral claim inputs, preserves and rechecks the coupon signature,
signs each claim with BIP-341 `SIGHASH_DEFAULT` over the full ordered
`Prevouts::All`, and evaluates both claim scripts locally. The resulting
example totals 500,000 sats in and 490,000 sats out, for a 10,000-sat fee.
This coupon is bounded to its single known cold outpoint and fixed output. It
cannot roll forward to an unknown replacement outpoint, and additional cold
inputs require additional signatures.

Keep `claim.hex` private until broadcast: claim preimages are in its witness.
Submit it to the same regtest Core node with `sendrawtransaction`, mine it,
and verify both swap prevouts and the cold output. The program checks raw
funding transaction bytes, txids, vouts, fixture scripts and amounts, but
cannot query Core for confirmation, best-chain status, or unspentness. Perform
those checks before calling it.

This synthetic probe does not implement production eligibility rules, fee
policy, mixed-deadline handling, timeout races, persistence, or crash
recovery. Do not use mainnet or real funds.
