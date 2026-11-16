# On-chain batch probe

This is a standalone, stdlib-only Python probe for one narrow question: can a merchant hot wallet consolidate a backlog of ordinary confirmed customer payments into one transaction to a watch-only savings wallet, without a payment provider? Both merchant wallets start empty. Settlement fees come from receipts, so this flow requires no separate merchant Bitcoin deposit; it still has an economic cost.

It uses Bitcoin Core 32.1 in a fresh regtest datadir, with no production service or application code. It does not use Lightning or Payjoin. Each of 100 synthetic 10,000-sat payments is a separate `sendtoaddress` transaction to a distinct merchant hot-wallet P2WPKH address. The probe mines every ten payments, then checks there are 100 confirmed merchant-wallet UTXOs totaling 1,000,000 sats.

It imports the supplied public P2TR savings address into a blank descriptor wallet with private keys disabled. The `synthetic-public-fixture` label is only a label; this does not prove hardware custody or the merchant's actual cold-storage setup. The probe constructs and signs three alternative PSBTs spending exactly those 100 inputs to one savings output at 1, 10, and 100 sat/vB. It records each transaction's actual vsize, fee, output value, and `testmempoolaccept` result. These are alternative fee plans, not three payments. It broadcasts only the 1 sat/vB transaction, mines it, and checks that the hot wallet has no unspent outputs while savings has the confirmed output. Customer receipts remain recorded at their 1,000,000-sat / 1,000,000,000-msat gross; the batch fee is paid once by reducing the savings output.

A separate 1,000 sat/vB funding attempt is expected to fail from insufficient funds because the resulting fee exceeds the 1,000,000-sat output value. The failure is recorded without depending on Core's exact error text. No transaction from that attempt is broadcast.

## Run

```sh
python3 run_probe.py
```

The runner defaults to `~/.local/opt/bitcoin-core-32.1/bin/bitcoind`. Set `BITCOIND` to the Core 32.1 executable path to override it. RPC and P2P default to loopback ports 19730 and 19731; `PROBE_RPC_PORT` and `PROBE_P2P_PORT` can override them. It refuses occupied ports before starting. The runner creates a random 0700 directory under `/tmp`, puts a random `rpcauth` credential in a 0600 config, starts Core against only that directory, and stops only the process it started. It never connects to or uses an existing datadir. The run directory and `result.json` are retained as evidence.

## What this does not prove

The 100 ordinary payments initially create 100 outputs. The later merchant
transaction consumes them; the probe does not prevent their initial creation.
An online-balance cap is an operational target, not a cryptographic spending
restriction or lifetime loss limit. Late and unsolicited payments to issued
addresses cannot be hard-blocked. Using a receiving working wallet would be a
new explicit product policy; the existing refund-only float is unchanged.

Batching assumes the online merchant signing key is available when the merchant chooses to sweep. The receiving cold/watch-only wallet does not need to sign. A fee ceiling can leave hot-key funds exposed while fees are high; it is not a lifetime fee guarantee or forecast of future fees. This does not prove zero-confirmation retail acceptance, production OpenBMT controller behavior, provider independence in an integrated service, hardware signing/custody, or compatibility with every wallet. The fee plans only establish local regtest behavior for this Core version and wallet configuration.
