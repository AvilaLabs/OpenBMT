# openbmt-spend

The spend path around the signer, as a Rust library with no network, no
clock, no storage and no randomness: build the transaction to be signed, join
two signers' signatures, and finish them into a transaction the network
takes.

The specification is [docs/SPEND-RULES.md](../../docs/SPEND-RULES.md). Each
rule has an ID (SP-01 and on), every error says which rule it comes from, and
every test is named for the rule it proves.

**This crate has had internal review only. No independent security audit has
been done yet.** Do not use it with real funds.

## The calls

| Call | What it does | Rules |
|---|---|---|
| `Coin::check` | The previous transaction hashes to the outpoint and the output is the wallet's script at the coin's keychain and index. | SP-01, SP-02 |
| `plan` | Chooses coins (oldest first, no randomness), works out the outputs, the change and the fee, and returns a `Plan` or a `PlanError`. | SP-03, SP-10 to SP-36 |
| `Plan::to_psbt` | The plan as a PSBT version 0 that the signer reviews with no refusal. | SP-40 to SP-42 |
| `combine` | Joins the signatures of the signed copies into the base file, verifying each one against the BIP 143 hash first. | SP-50 to SP-54 |
| `finalize`, `finalize_plan` | Two signatures per input make the witness; the finished transaction is then checked again from scratch, and with `finalize_plan` also against the plan. | SP-60 to SP-64 |

The fee is `ceil(vsize * rate / 1000)`, with the rate in satoshis per 1000
virtual bytes and every signature counted at its longest (72 bytes), as the
signer counts it. A finished transaction therefore pays at least the rate
that was asked, and a little more.

## What it is not

It estimates no fee, broadcasts nothing, tracks no coins and has no
consensus library. The check at the end of `finalize` is the library's own,
made with the `bitcoin` crate's signature hash and secp256k1;
`scripts/spend-e2e-core.sh` asks Bitcoin Core's `testmempoolaccept` as the
independent check. The full list is in section 9 of the rules.

## Running the check against Bitcoin Core

```
scripts/spend-e2e-core.sh
```

Needs Bitcoin Core 32.1 in `~/.local/opt/bitcoin-core-32.1/bin`. It runs on
regtest with keys from fixed test bytes (the example
`examples/spend_regtest.rs`, test only, never use).
