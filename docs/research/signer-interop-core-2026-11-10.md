# The signer against Bitcoin Core (2026-11-10)

**Question.** Does `openbmt-signer` read the transaction files Bitcoin Core
builds, and does Bitcoin Core accept the files and signatures the signer
returns?

**Answer.** Yes, for PSBT version 0. Bitcoin Core 32 writes PSBT version 2
unless it is asked for version 0, and the signer refuses version 2
(SR-01). A caller that builds files with Core has to ask for version 0.

## Method

`scripts/signer-interop-core.sh` runs the whole check on regtest with
Bitcoin Core 32.1 and fails at the first step that does not hold. No real
keys or funds are involved.

The wallet is the usual 2-of-3. Core holds a watch-only copy that builds
the transactions, and keys 1 and 3 as two separate Core wallets. Key 2 signs
through the signer, run by the example
`crates/openbmt-signer/examples/regtest_sign.rs`, with the first ceilings
(SR-38) and its state kept in a file between runs.

## Results

One run, all steps passed.

| Step | What was checked |
|---|---|
| Core builds, the signer signs first | The signer and Core agree on the fee. The payment is shown as an other destination and Core's change output is accepted as change (SR-20). Core sees one signature per input in the signer's file and cannot finalize it. Core's key 1 adds the second signature, and the transaction is accepted by the node. |
| Fee bump | Core's `psbtbumpfee` builds a replacement that spends the same coin. The signer logs the two as one entry, at the payment plus the larger fee (SR-33). Core's key 3 signs, the replacement confirms and the first transaction does not. |
| Core signs first | The signer checks key 3's signature from Core and adds its own (SR-14). The transaction confirms. |
| The same file again | The signer counts it once and returns the same bytes (SR-33, SR-53). |
| Above the ceiling | 200,000 sat to another destination is refused under SR-35. |
| PSBT version 2 | The file Core writes by default is refused under SR-01. |

## PSBT version 2

In Bitcoin Core 32.1, `walletcreatefundedpsbt`, `createpsbt`,
`converttopsbt` and `psbtbumpfee` take a `psbt_version` option whose
default is 2. The first run of this check failed at its first step for that
reason: the signer refused Core's file under SR-01.

The signer stays at version 0. The Key app signs files that the OpenBMT
owner phone builds, and that software writes version 0. Reading version 2
as well would add a second parser to the part of the signer that faces
hostile input.

What the design takes from this: the recovery kit's Core steps ask for
`psbt_version=0` (`docs/DESIGN.md` §5.4). A restore signs outside Core,
and whether Sparrow and the hardware signers read version 2 has not been
tested here.

## Limits of this check

- Regtest only, one run, one version of Bitcoin Core.
- Transactions with one input. Many inputs, inputs on both keychains and
  large transactions are covered by the crate's own tests, not by Core
  here.
- The other direction of key order, key 1 and key 3 without the signer, is
  in the restore drill (`docs/research/restore-drill-core-2026-11-09.md`).
- No other wallet program and no hardware signer took part.
