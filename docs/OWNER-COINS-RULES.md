# Owner coin view rules: which coins the wallet holds

Status: specification for `crates/openbmt-owner-node/src/coins.rs` and
`crates/openbmt-owner/src/state.rs`. Each
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
  what this needs, including the exact signed transaction bytes. If a
  transaction had already reached refund depth, its still-linked paid rows
  return to sent and the node rebroadcasts those same signed bytes; it never
  signs a replacement automatically. Paid rows whose transaction remains in
  this undo horizon cannot be archived. Once the confirmation leaves the
  horizon, archiving folds refund amounts into the sale's archived total.
- **OC-06.** A reorganization of more than 100 blocks, or of a block that
  holds coins and has no record kept, cannot be undone. The view takes the
  named state "deep reorg: rescan", offers no coins, and stays so until it
  is reset and rescanned from the birthday height. Exactly 100 blocks can
  still be undone.

## 3. The watch set

- **OC-07.** The watch set is the receive scripts from index 0 to the
  highest index a batch handed out plus 20, and the change scripts from 0
  to the next change index plus 20. It grows when the marks grow. When the
  marks grow at runtime, the view stores the new marks and enters
  `rescanning` at one block below its last checked height. The node installs
  the scripts before asking the driver to recheck blocks above that position.
  The driver's ordered rescan-start event gates old checked-height events, so
  the view cannot become ready from stale progress. This short catch-up scan
  covers work already in flight without rescanning from the birthday. The
  view stays ready when there is no checked block above the birthday to
  revisit, but the node still waits for the ordered marker before offering a
  spend. The
  marks and rewind position are stored together, so a restart starts with the
  new scripts and resumes the catch-up scan. At start, scripts are installed
  before peers connect. The node refreshes marks at start, immediately before
  each block and reorganization is applied, and after a batch is queued or a
  spend is built or taken. A new view, reset, deep reorganization, or an
  explicit owner request scans from the birthday. Marks never go down. The
  total receive and change script ranges, including their gap, must fit the
  network manager's 100,000-script limit. The node checks this before deriving
  scripts or saving raised marks. At startup, a larger range stops startup; at
  runtime, it halts watch updates and reports the limit. It does not silently
  drop scripts or dispatch a newly committed transaction while halted. Stop
  issuing new addresses and update the device or report the limit to the
  OpenBMT project for help.

## 4. Storage

- **OC-08.** The view is stored in a framed form with its own magic
  (`OBMe`) and a version, in the node's atomic file writes. It is stored
  before the chain position advances, so a restart at any write point
  gives the same view and fetches nothing again. Reading is strict: a cut,
  a trailing byte, a wrong magic or version, a changed byte that breaks an
  invariant, or bytes that do not re-encode to themselves are refused.
- **OC-09.** The tip block counts as one confirmation. A tip below a
  coin's block gives zero.

## 5. Where a sale's facts come from

The owner's limit on a refund, and its answer to "has the sale reached the
refund depth", rest on the owner's own checked view of the chain and on the
address batches the owner signed. They never rest on what a counter says it
received. A counter that is taken over can sign a record that claims any
amount.

- **OC-10.** Issued ranges are kept for good. Every address batch the owner
  signs is recorded as a range (start, count, counter) in the owner's stored
  state, in the same write that signs and queues the batch: the new state is
  built, stored, and only then swapped in. A range is never removed, not when
  the batch is delivered, cancelled or archived. `Owner::counter_of(index)`
  names the counter whose range holds the receive index, or nothing.
  `Owner::receive_mark` counts the ranges, so a caller no longer has to keep
  the highest value it has seen.
- **OC-11.** Ranges never overlap: a batch that overlaps a range already
  issued is refused, and the refusal names this rule. At most 65,536 ranges
  are kept. A batch beyond that is refused with the named refusal "issued
  ranges full", and the owner is told to report it to the OpenBMT project.
- **OC-12.** Receipts are kept after they are spent. Every output that pays a
  receive script, in a checked block, is recorded as a receipt (index,
  outpoint, amount, height, block). Change outputs are not receipts. Unlike a
  coin, a receipt stays when it is spent. A reorganization undoes receipts
  exactly as it undoes coins: the receipts of a block that left are removed,
  and the replacement chain records its own. A block that left and holds a
  receipt but has no undo record is a deep reorganization (OC-06). A reset
  drops the receipts with the coins; the rescan from the birthday finds them
  again. A block seen twice records a receipt once.
- **OC-13.** At most 262,144 receipts are kept. Past that, the view takes the
  named state "receipt log full": it records no more receipts, gives no
  sale's facts (OC-14), and says so in words. Coins are still tracked and the
  owner can still spend. The state ends with a reset and rescan. Receipts are
  stored in the view's own form (`OBMe`, OC-08), strictly: sorted by index
  and outpoint, no outpoint twice, and, unless the log is full, every
  unspent receive coin has its receipt.
- **OC-14.** The facts of a sale come from `verified_facts`. The view must be
  ready, the receipt log must not be full, and the view must have checked the
  chain up to the tip given; otherwise nothing is said and the caller waits
  ("view not ready"). The sale's index must lie in a range the owner issued to
  the sale's counter; otherwise the request is refused ("not this counter's
  address"). The amount received is the sum of the receipts at the index
  that are in a block at or below the tip. The sale has reached the refund
  depth when something was received and every one of those receipts has at
  least the refund depth of confirmations (tip minus height, plus one). A
  receipt that was spent still counts.
- **OC-15.** The intake of refund requests gives the owner these facts, both
  when it queues a request and when it refreshes the facts of queued
  refunds. The counter's own figure is read only to compare. When the claim
  is more than the amount verified, the intake adds a line to its list
  ("claimed more than seen", with both amounts) and uses the verified amount;
  the owner's rule AS-52 then decides the request against that amount. A
  claim lower than the verified amount is not noted, because late payments
  and surplus explain it. A request whose facts must wait for the view
  (OC-14) is not dropped and not listed as refused. It stays where it is and
  is tried again on the next run, with no time limit, until the view is ready.
  Requests after it wait with it, so the order of a counter's records holds.

## What these rules do not say

- **They do not prove the wallet holds the coins on the network.** A
  checked block shows the payment is in a block the counter's peers agreed
  on. A deeper reorganization than 100 blocks is a named state, not a
  repair.
- **They do not choose which coins to spend.** That is `openbmt-spend`.
- **They do not decide whether a payment is a sale.** Receipts at an index
  are everything that paid the script, whoever paid it and for whatever
  reason. A payment that arrives after the sale closed raises the limit.
- **They do not say a broadcast transaction arrived.** That is
  `docs/NETWORK-RULES.md`, NR-86 to NR-91.
