# The fee sample against Bitcoin Core (2026-11-10)

**Question.** Does `block_fee_sample` (`docs/FEE-RULES.md`) get the fees and
the size of a block right from the block's bytes alone, as Bitcoin Core
counts them, with no outside data about the transactions' inputs?

**Answer.** Yes. In the run below every step passes: the fees match
Core's `totalfee` exactly in three blocks, the size matches Core's
weights exactly, the rate in whole sat/vB equals Core's `avgfeerate`,
and the empty block gives fees 0 and no rate. The regtest halving at
block 150 is used correctly.

## Setup

`scripts/fee-sample-core.sh` runs the whole check. Bitcoin Core `v32.1.0`
(the binaries in `~/.local/opt/bitcoin-core-32.1/bin` only) ran on regtest
in a scratch directory and was stopped by its pid at the end. The
`fee_sample` example of `openbmt-watch` reads a block as hex on standard
input (the output of `getblock <hash> 0`) and prints the sample. No keys or
addresses of any value are involved; the wallet is made fresh by the script.

1. 101 blocks are mined. Five transactions are sent at 1, 3, 10, 40 and
   150 sat/vB. One block takes them (height 102).
2. One more block is mined with nothing in the mempool (height 103).
3. The chain is mined to height 155, five more transactions are sent at
   the same rates, and one block takes them (height 156, after the
   regtest halving).

For each block the sample is compared with `getblockstats` and `getblock`.

## Results

| Block | Fees (sample = Core) | Size in vB | Rate (sat/vB) | Core's average |
|---|---|---|---|---|
| 102, five transactions | 115,800 sats | 990 | 150.585 | 150 |
| 103, no transactions | 0 | 221 | none | 0 |
| 156, five transactions, halved subsidy | 29,032 sats | 1,059 | 34.696 | 34 |

The fees of block 102 differ from run to run, because the regtest wallet
picks its own coins and so its transactions differ in size.

- The fees equal `totalfee` in all three blocks.
- The block's weight equals the weight of its transactions without the
  coinbase (`total_weight`), plus the coinbase's weight, plus 324 weight
  units for the header and the count, and the sample's size is that weight
  divided by 4 and rounded up.
- The empty block has fees 0 and no rate, as FE-06 says.
- Block 156 pays a little over 25 coins in all, so the halved subsidy of
  regtest (FE-03) is the one the sample subtracts.

## The rate is Core's measure

The rate divides the fees by the weight of the transactions other than
the coinbase (FE-05), which is the weight Core reports as `total_weight`
and divides by for `avgfeerate`. The script checks that the sample's rate
equals fees times 4,000 over Core's `total_weight`, and that its whole
sat/vB equals Core's `avgfeerate`. A first version divided by the block's
size less the coinbase's, which also counts the 81 bytes of header and
transaction count; in a small block that read about a fifth lower than
Core, so it was changed.

## Limits of this check

- Regtest blocks hold a handful of transactions that the wallet made, all
  paid at feerates the wallet chose. The fee identity (the coinbase claims
  the subsidy plus the fees) is a consensus rule, so the numbers do not
  depend on the traffic, but no block with a miner that claims less, or a
  block from a real network, was looked at.
- The check shows that the sample says what the block says. It does not
  show that an average fee rate is the rate needed to get into the next
  block; `docs/FEE-RULES.md` says it is not.
