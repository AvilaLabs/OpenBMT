# Owner coin view rules: which coins the wallet holds

Status: specification for `crates/openbmt-owner-node/src/coins.rs`. Each
rule has an ID so that a test can name the rule it proves (`oc_NN_*`).

The owner's device builds a spend from a list of coins
(`docs/SPEND-RULES.md`). This view is where the list comes from. It learns
the coins from blocks that passed the watcher's checks
(`docs/WATCHER-RULES.md`, WR-20 to WR-22), and from nothing else. It does
not ask a server what the wallet holds.

## 1. What is a coin

- **OC-01.** A coin is an output of a transaction in a checked block, paid
  to a script in the watch set. Confirmations are the tip height minus the
  block height, plus one. A block that fails its check, or does not extend
  the header chain, changes nothing. A block seen twice is counted once. If
  the file cannot be written, the view stays as it was.
- **OC-02.** An unconfirmed transaction from anyone else is never a coin.
  Peers show such transactions, and they can be thrown away or replaced.
- **OC-03.** The owner's own finished transaction is held as pending. Its
  change outputs are listed at 0 confirmations. At most 64 are held. If a
  checked block spends an input of a pending transaction in another
  transaction, the pending one is reported replaced and is dropped from
  the view.
- **OC-04.** A coin that a checked block spends is removed. A coin made
  and spent in one block is never held.

## 2. Reorganizations

- **OC-05.** A reorganization undoes the blocks that left: the coins they
  made are removed, the coins they spent are put back, and a pending
  transaction they confirmed is pending again. The last 100 blocks keep
  what this needs.
- **OC-06.** A reorganization of more than 100 blocks, or of a block that
  holds coins and has no record kept, cannot be undone. The view takes the
  named state "deep reorg: rescan", offers no coins, and stays so until it
  is reset and rescanned from the birthday height. Exactly 100 blocks can
  still be undone.

## 3. The watch set

- **OC-07.** The watch set is the receive scripts from index 0 to the
  highest index a batch handed out plus 20, and the change scripts from 0
  to the next change index plus 20. It grows when the marks grow. A script
  that was added later may have been paid before: the view is rescanned
  from the birthday height, and a spend of a coin that the new script
  needs is found by the same scan. Marks never go down.

## 4. Storage

- **OC-08.** The view is stored in a framed form with its own magic
  (`OBMe`) and a version, in the node's atomic file writes. It is stored
  before the chain position advances, so a restart at any write point
  gives the same view and fetches nothing again. Reading is strict: a cut,
  a trailing byte, a wrong magic or version, a changed byte that breaks an
  invariant, or bytes that do not re-encode to themselves are refused.
- **OC-09.** The tip block counts as one confirmation. A tip below a
  coin's block gives zero.

## What these rules do not say

- **They do not prove the wallet holds the coins on the network.** A
  checked block shows the payment is in a block the counter's peers agreed
  on. A deeper reorganization than 100 blocks is a named state, not a
  repair.
- **They do not choose which coins to spend.** That is `openbmt-spend`.
- **They do not say a broadcast transaction arrived.** That is
  `docs/NETWORK-RULES.md`, NR-86 to NR-91.
