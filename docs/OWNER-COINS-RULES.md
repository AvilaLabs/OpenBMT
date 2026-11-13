# Owner coin view rules: which coins the wallet holds

Status: specification for `crates/openbmt-owner-node/src/coins.rs`,
`crates/openbmt-owner-node/src/receipts.rs` and
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
- **OC-53. A reorganization while the node was stopped.** The view keeps
  the hash of the block it last checked. At start, before the view or its
  facts are used, that block is looked up in the chain the node holds. If
  it is no longer there, the blocks of the view's undo records that are not
  in that chain are the blocks that left, and they are undone as OC-05
  says, in the view and in the view of every former wallet; when no undo
  record reaches the fork, it is the deep reorganization of OC-06. The
  background watch keeps headers and no coins, so this is also what
  follows a reorganization it saw. A sale's facts, a refund's facts and a
  settlement are never taken from a view that has not passed this check.
- **OC-54. A payment finished elsewhere is undone too.** When a payment
  given to a hardware signer is settled from a block (OC-52), the
  transaction, taken from that block, is kept in the view's undo record as
  the owner's own confirmed transactions are. If that block leaves the
  chain, the paid rows it settled return to sent and the transaction is
  followed again as OC-05 says: the node rebroadcasts the same bytes and
  signs nothing. Its paid rows cannot be archived while the transaction is
  in the undo horizon.

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
  receive and change scripts the live watch holds (OC-22), including the
  gap, must fit the network manager's 100,000-script limit (NR-41). Receive
  scripts that are set aside (OC-23) are not counted. The node checks this
  before deriving scripts or saving raised marks. At startup, a larger set
  stops startup; at runtime, it halts watch updates and reports the limit. It
  does not silently drop scripts or dispatch a newly committed transaction
  while halted. Stop issuing new addresses and update the device or report
  the limit to the OpenBMT project for help.

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
  the highest value it has seen. Each range also keeps the owner's clock, in
  seconds, at the moment the batch was signed (OC-23).
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
  drops the receipts with the coins and the marker of pruned indices
  (OC-29); the rescan from the birthday finds them again. A block seen
  twice records a receipt once. A receipt may leave the log only as OC-28
  says.
- **OC-13.** At most 262,144 receipts are kept. When pruning (OC-28) can free
  nothing and the cap is reached, the view takes the named state "receipt log
  full": it records no more receipts, gives no sale's facts (OC-14), and says
  so in words. Coins are still tracked and the owner can still spend. Every
  kept receipt is then of an unspent coin or of a block younger than the keep
  time, so the state means the wallet holds more than 262,144 such payments;
  a reset and rescan does not end it, because the rescan finds the same
  payments again, and the words tell the owner to report the limit to the
  OpenBMT project. Receipts are stored in the view's own form (`OBMe`,
  OC-08), strictly: sorted by index and outpoint, no outpoint twice, and,
  unless the log is full, every unspent receive coin has its receipt, then
  the pruned index ranges (OC-29). At 300 payments a day, 400 days of
  receipts are about 120,000, well under the cap, so a shop that spends its
  coins does not reach the state.
- **OC-14.** The facts of a sale come from `verified_facts`. The view must be
  ready, the receipt log must not be full, and the view must have checked the
  chain up to the tip given; otherwise nothing is said and the caller waits
  ("view not ready"). The sale's index must lie in a range the owner issued to
  the sale's counter; otherwise the request is refused ("not this counter's
  address"). The amount received is the sum of the receipts at the index
  that are in a block at or below the tip. The sale has reached the refund
  depth when something was received and every one of those receipts has at
  least the refund depth of confirmations (tip minus height, plus one). A
  receipt that was spent still counts. An index that lost a receipt to
  pruning (OC-29) is refused with its own named refusal, "too old for
  facts", after the check of the counter and before any sum: the wallet
  cannot tell what was paid, and a sum of the receipts that are left would be
  a wrong zero or a wrong low figure. The refusal is final, not a wait. Its
  words say "This sale is too old for the wallet to check what was paid.",
  name the owner, and the next step is that the owner decides by hand.
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

## 6. The line about receipts, and the offer to merge them

The owner is shown one line about what the wallet's payments would cost to
spend, and now and then an offer to merge them into one. Both rest on the
owner's own checked view and on the fee samples of blocks the device has
checked (`docs/FEE-RULES.md`, FE-20), and on nothing a service says. Screens
say "payments" and "merge", never "coin", "output" or "consolidate". The
code is in `crates/openbmt-owner-node/src/receipts.rs`.

- **OC-16.** The line counts every unspent coin of the wallet, receive and
  change alike: each costs one input when it is spent. The count is the
  coins that the view offers to a spend (OC-01 to OC-04, and the owner's own
  pending change at 0 confirmations), so it follows a receipt, a spend and a
  reorganization as the view does.
- **OC-17.** The cost is what spending all of them to one output would cost
  at the rate of `FeeHistory::recent_rate` (FE-13), not a number of coins
  times a constant. It is worked out with the spend crate's own size model
  for a consolidation of that many inputs and one output (`docs/SPEND-RULES.md`,
  SP-38), so it is the fee the planner gives that spend, to the satoshi: no
  signing and no coin is needed. More coins than one transaction takes
  (SP-39) are counted as several consolidations. The rate is at least 1
  satoshi per virtual byte (1,000 per 1,000), the lowest that nodes relay by
  default: a block whose fees average less than that does not make a
  cheaper spend. The amount is in sats with separators (SC-09). The owner
  device has no exchange rate at hand, so the shop's currency is not used.
- **OC-18.** The line has four states, each with its own words (SC-01,
  SC-07): the view is not ready (it is not ready, or has not checked every
  block up to the tip, as OC-14 says for a sale's facts); the wallet holds
  no coin; the wallet holds coins and fewer than 3 blocks of the last 144
  give a fee yet (FE-13); and the line itself, the count and the cost.
- **OC-19.** The offer to merge is shown with the line when all three
  hold: `FeeHistory::is_low` says the recent fee is low (FE-14, which needs
  its own minimum of history); the wallet holds at least `MERGE_MIN_COINS`
  coins that a spend may use, 10 **[to tune]** (a coin with fewer
  confirmations than the owner's queue asks for is not one of them); and
  merging now costs less than the same merge at `FeeHistory::usual_rate`
  (FE-17), so that the saving is above zero. The words give the cost now and
  the cost at the usual fee, and say plainly that a merge shows on the
  blockchain that these payments belong to one wallet (`docs/DESIGN.md` §11).
  The offer is a suggestion and is never taken by itself.
- **OC-20.** Approving the offer queues one cash-out of every coin the merge
  takes in (the amount "all") to the wallet's next change address, and
  builds a session for the Key app from it at the rate that the offer
  stated, through the same path any spend takes (`OwnerNode::build_request`
  with the queue's cash-out of AS-57). It is then an ordinary two-key
  session: the Key app shows it as a move inside the wallet, and key 1 signs
  it when the owner takes the response (`OwnerNode::take`). Nothing is
  signed or sent by the approval itself, and no new signing path exists. The
  merge is refused, and the queue is as it was but for the number the entry
  used, when the offer does not hold at that moment, when a refund or a
  cash-out is queued or in a session (the merge would take the same coins
  and the change address would no longer be the one in the entry), or when
  the session cannot take it (the session that came out without it is given
  up and the entry is cancelled). A merge entry is queued and built in one
  call, so it does not wait in the queue for a change address that has moved.
- **OC-21.** One merge takes in at most `MAX_CONSOLIDATION_INPUTS` (SP-39)
  coins, and at most 4 MiB of the previous transactions that every input
  carries (half of the signer's 8 MiB file, `docs/SIGNER-RULES.md`), oldest
  first (SP-11); the first coin is always taken. A wallet that holds more
  gets the rest merged by another offer once this merge has been taken: the
  new coin and the coins left make the next count. The words of the offer
  then say "400 of your 1,000 payments", and its cost is that of the merge
  it builds.

## 7. The live watch and the deep check

The network manager watches at most 100,000 scripts (NR-41). The owner's
wallet issues receive addresses for good (OC-10), so a shop with a few
hundred sales a day would pass that in about a year. Before these rules the
node stopped there: it refused to start, or, when it was running, halted
and refused every change of the queue (OC-07). The rules below keep the
wallet watching every address it has issued, some of them all the time and
the rest by a check once a month. The code is in
`crates/openbmt-owner-node/src/live.rs` and `deep.rs`.

- **OC-22.** The live watch is the set of scripts the node has the driver
  watch for the wallet: every change script from index 0 to the change mark
  plus the gap, and every receive script from index 0 to the receive mark
  plus the gap, except those set aside (OC-23). Its bound is
  `OWNER_LIVE_WATCH`, 90,000 scripts **[to tune]**, which leaves room under
  the driver's limit for change and for growth. Nothing is set aside while
  the live watch is at the bound or below it. The node tells the driver to
  stop watching what is set aside (NR-97). The set aside is stored as runs of
  indices in the coin view, in the same write as the marks (OC-08), and is
  never larger than what was issued.
- **OC-23.** When the live watch would pass the bound, receive scripts are
  set aside, oldest issued first (by the time of the batch, then by index),
  and only as many as bring it to the bound. A receive script may be set
  aside only when it is quiet: its batch was signed at least `QUIET_SECS`
  ago by the owner's clock, 60 days **[to tune]** (the time is kept with the
  issued range, OC-10, and read back strictly), and the view holds no
  unspent coin at it and no held transaction pays it. A script with an
  unspent coin always stays, because the spend of the coin must be seen. A
  change script never leaves. A receive index that no issued range holds is
  never set aside. If too few scripts are quiet, the live watch stays over
  the bound; the limit that stops the node is the driver's (OC-07). The node
  looks for scripts to set aside when the marks grow, and at least every 10
  minutes, and not while a deep check runs (OC-24).
- **OC-24.** The scripts set aside are checked by a deep check every
  `DEEP_CHECK_SECS`, 30 days **[to tune]**, counted from the end of the last
  one (from the first time scripts were set aside, if none has run), and at
  once when the owner asks (`OwnerNode::request_deep_check`). The check reads
  the blocks above `from`, the height above which the view has not checked
  them, up to the tip the node had when the check began, using the driver's
  rescan over compact filters (NR-90) and the view's `apply_block` (OC-01),
  not a scanner of its own. `from` is the view's position when the first
  script was set aside, and goes down to the fork of any reorganization
  (OC-05) and to the birthday when the view is reset (OC-06). It goes up to
  the target of a check that finished. The check runs only when the view is
  ready and has checked up to the tip. The scripts are checked in passes of
  at most `OWNER_LIVE_WATCH` at a time. The check runs on a second driver of
  its own, in `<dir>/deepcheck`, with its own lock and headers, started the
  way the sending driver is (NR-91). It does not touch the live watch, and a
  block it hands over changes coins and receipts only; the view's position
  and state stay as they were while it runs. A block that the node's own
  chain does not hold is passed over. A change of the chain stops the check
  without calling it a failure, and it starts again when the view has caught
  up. A payment found to a script set aside is applied like any other: it is
  a receipt (OC-12) and a coin (OC-01), spendable at once. At the end of
  each pass the scripts that were paid go back in the live watch, and the
  main driver reads the blocks above the target again for them, so that
  nothing between the target and now is missed. When every pass is done, the
  view stores the time, the next due time and the new `from`.
- **OC-25.** While any script is set aside, the owner is told in one line:
  how many old addresses are checked once a month instead of all the time,
  that a payment to one shows up at the next check, and when the last check
  was (in whole days) and when the next is (a time the app shows on the
  owner's own clock). Before the first check the line says none has run. The
  words are in `crate::say` and under SC-01 to SC-12; there is no line while
  nothing is set aside.
- **OC-26.** A deep check that is running has its own words, and so has one
  that failed. A check fails when its driver cannot start or stops, when it
  hands over a block that does not pass the checks, when it has made no
  progress for `DEEP_STALL_SECS` (1 hour **[to tune]**), or when the view
  cannot be stored. A failure sets aside nothing and loses nothing: the
  scripts stay as they were, the failure is stored with the time of the next
  try, `DEEP_RETRY_SECS` later (1 hour **[to tune]**), and the words give
  that time. The next successful check clears it.
- **OC-27.** The scripts set aside and the schedule (last check, next check,
  failure, `from`) are stored in the coin view, strictly (OC-08): runs
  ascending, not empty, not touching and not past the last index; no
  schedule when nothing is set aside, and a due time when something is. The
  progress of a check in progress is not stored. A restart in the middle
  starts the check again from the stored `from`, which loses nothing: a
  payment already found is a stored receipt and a block read twice is
  applied once. If a restart finds a script set aside that holds a coin (a
  stop between a find and its return to the live watch), the script goes
  back in the live watch at start.

- **OC-28.** Receipts of spent coins leave the log when it fills. A receipt
  may leave only when both hold: its coin is not held (a receipt of a held
  coin, one that a pending transaction spends included, always stays), and
  its block is more than `RECEIPT_KEEP_BLOCKS` below the tip, 57,600 blocks
  (400 days at 144 a day) **[to tune]**, longer than any refund window a
  shop is likely to honour. The age is counted in blocks because the view
  keeps no block time per receipt, and the header chain gives only the tip's.
  Pruning runs when more than 90% of the cap (`PRUNE_START`) is kept: when
  the node starts, with the position the view reflects as the tip, and
  before and after the receipts of each applied block are recorded. It lets
  go of the oldest blocks first, frees down to 80% (`PRUNE_TO`), and is
  stored with the block that caused it, in one write. When a coin that was
  spent is put back by a reorganization, its receipt is kept again from the
  undo record; if the log is at the cap by then, the view takes the named
  state of OC-13. A reorganization deeper than the keep time cannot happen
  in practice, and this is what the view does if it does: it stays whole,
  keeps the marker (OC-29), and names its state, and it does not panic.
  A rescan that re-reads old blocks records their receipts again and prunes
  them again as it reads the blocks that spend them.
- **OC-29.** The receive indices that lost a receipt to pruning are kept as
  a list of ranges (start and end, inclusive), sorted and apart, at most
  `MAX_PRUNED_RANGES` (1,024) of them, stored after the receipts (OC-13)
  and read strictly: start not above end, end below 2^31, each range
  starting more than one above the end of the one before. Ranges that touch
  join. Past the bound, the two ranges with the least space between them
  join, so the list cannot grow without bound; an index in that space is
  then refused as too old as well, which is wrong in the safe direction (a
  sale is never answered with a wrong figure). Indices are handed out in
  order and old blocks are pruned first, so in practice the list is a few
  ranges. A payment that arrives late at a pruned index is still refused for
  that index. A reset drops the list with the receipts.

## 8. Matching sales on the owner's phone

The owner node runs the till matcher of `docs/POS-RULES.md` itself. The
orders come from the vendor, through the phone's own read-only token; the
sale facts come from the counters' records the node already holds. The
node does no network work for it: the app sends the request and hands back
the reply (OA-16 to OA-19).

- **OC-30.** The sale facts are folded from the node's verified copies of
  each counter's records, with the device list the link holds (PS-06,
  PS-09). If any counter's copy does not fold, no facts are given, the
  poll fails with a named refusal, and nothing is matched on part of the
  counters. The facts are asked for only when the last page of a read has
  been taken.
- **OC-31.** The setup (vendor, one location or merchant id, the names
  that mean Bitcoin, the currencies the owner sells in) is the owner's own
  and is not part of the shop's signed settings, which carry none of it.
  It is kept in `pos-setup.bin`, in a strict frame with the magic `OBMr`:
  read back with no byte left over, every bound checked, and refused if
  writing it again would give other bytes. One location serves all the
  counters the link holds. A vendor or location that differs from the
  stored one starts an empty ledger; the ledger is written before the setup
  that names it.
- **OC-32.** The ledger is kept in `pos-ledger.bin` by the durable write
  the node uses for its other files. A page is stored before it takes
  effect; a page that cannot be stored takes no effect. A setup or ledger
  file that does not read back does not stop the node from starting: every
  till call but removal then refuses with a named state, until the owner
  removes the till.
- **OC-33.** A read is a request (Square: a POST of the search body; Clover:
  a GET of the orders path) and a reply (a status and the body bytes). The
  node builds the request from the cursor it holds and parses the reply
  with `openbmt-pos-readers`. The request carries no sign-in text. Only a
  200 is read. A 401 or 403 is "sign-in refused", a 429 is "asked to wait",
  and any other status is "vendor unavailable"; each has its own words. A
  reply that does not parse is a named refusal and changes nothing. After
  a Square orders reply that has orders, the node hands out Square's
  payments (a GET of `/v2/payments`, as PS-95 says) one request at a time,
  and the page takes effect only when its payments are all read, through
  `parse_pages` (PS-96). A page with no orders asks for no payments.
- **OC-34.** A read starts at most once in `POS_MIN_POLL_SECS` (60). The
  next page of a read already begun is not held back, and nor is a request
  for payments, which is not a page. A read is at most 64 pages
  (`MAX_PAGES`). A page that says more is coming but does not move the
  cursor ends the read with a named refusal. The payments of one page of
  orders are at most 64 pages of 100 (`POS_MAX_PAYMENT_PAGES`); a page that
  repeats its marker ends them, and more than that refuses the whole read
  with its own words, so nothing is cut or guessed.
- **OC-35.** The pages are matched as PS-10 to PS-40 say, with the facts of
  OC-30, when the last page has been taken or the page limit is reached. The
  reply tells the app which matches and alerts are new, which cleared and
  which were retired, and whether another page is wanted.
- **OC-36.** An alert is acknowledged in the phone's own ledger and stays
  there. No signed command is sent and none is needed; the Box and the
  counters do not learn of it.
- **OC-37.** Every refusal of the till has words that pass the checks of
  `docs/SCREEN-RULES.md`: what happened, why, and what to do next, with no
  rule numbers.

## 9. Setting up the owner's phone

The owner's phone makes key 1, reads the Key app's public answer, and builds
the 2-of-3 wallet from the three public keys (`docs/DESIGN.md` §2.1 steps 4
and 5). It hands the Key app the wallet setup to check, and starts its node
from the same wallet once the owner has compared the checksum words and
fingerprints. The Key app's side is SR-64 to SR-69 in
`docs/SIGNER-RULES.md`; the code is `openbmt-owner` (`ceremony`) and ops 80
to 85 of `openbmt-owner-ffi`. OC-38 and OC-39 are not used.

- **OC-40.** Key 1 is made from 64 random bytes the caller passes, on the
  BIP 48 path `m/48'/coin'/0'/2'` (coin 0 on mainnet, 1 otherwise). The
  owner's root is a separate secret from the same bytes. The twelve
  recovery words are shown only when the owner asks for them. The setup
  between steps is a strict frame `OBMu` (version 1): read back with no
  byte left over, every length checked, and refused if writing it again
  would give other bytes. It holds secrets: the app seals it, and the debug
  form of the setup shows none of them.
- **OC-41.** The wallet is `wsh(sortedmulti(2,...))` with key 1, key 2 and
  key 3 in their slots. Key 2 comes from the Key app's answer, or from
  text, never both. Key 3 is the one in the answer. The wallet is refused
  if the network of the answer differs from key 1's, if the Key app's root
  is the owner's, if two of the keys are the same, or if any key is not at
  the account path of its slot.
- **OC-42.** The setup for the Key app is a strict frame `OBMw` (version 1)
  with the network, the canonical wallet description, the owner's root and
  the batch minimum. The description must be the canonical form.
- **OC-43.** The owner is shown the checksum words, the fingerprints of the
  three keys, and each pair of keys with who holds each. The Key app shows
  the same words and fingerprints; the owner compares them.
- **OC-44.** The record reads back at every stage and nothing else does. A
  record whose parts do not belong together (a key not at its path, a
  description that does not hold the keys, a stage that does not fit the
  fields) is refused.
- **OC-45.** The setup finishes only from a built wallet, with a refund
  depth of at most 100 confirmations and a birthday. The finished record
  gives the values the node starts from: the descriptor, the network, the
  roots, the batch minimum, the refund depth and the birthday.
- **OC-46.** Every stage and every refusal has words that pass the checks
  of `docs/SCREEN-RULES.md`, with no rule numbers.
- **OC-47.** The facts of a queued refund follow the chain, whoever queued
  it. When blocks leave the chain, in the same step that changes the coin
  view, the facts of every sale with a refund in the queue, manual or from
  intake, are worked out again from the view as it is (OC-14, at the height
  the view has checked), built, stored, and swapped in only when the store
  succeeds. A sale the view cannot give facts for keeps its amount and is
  no longer at the refund depth. A refund whose facts no longer fit (AS-52,
  AS-53) stays queued and waits for depth, as it already does, and is in no
  session until they fit again. The same refresh runs again when a session
  is built, on the view at the tip, so a refund is never ready on facts
  older than the view, and one whose receipt confirmed again is ready
  again. No new words: the refund shows the state it already has.

## 10. What the owner paid out

The owner's queue forgets a refund or a cash-out once its transaction is paid
and past the undo horizon (`Owner::archive_paid`, AS-54). The books
(`docs/BOOKS-RULES.md`, BK-12) need each one with its time and its value in
the shop's currency, so the node keeps a log of its own.

- **OC-48.** When the node sends a transaction of the queue's outbox, it
  appends one entry for each refund and for the cash-out the transaction
  pays: the kind, the number the entry had in the queue, the transaction,
  the satoshis that went out (the value of the transaction's output to the
  entry's destination, the first one not taken by another entry, and the
  queue's amount when no output has the destination), the time by the node's
  clock, and for a refund the sale (the counter's key and the sale index) and
  the number of the counter's request when it came from one. The entry is
  stored before the queue is told the transaction was sent. When it cannot be
  stored, the entry stays signed with a note, and the next start sends again
  and tries again. An entry for the same kind, number and transaction is
  written once, however often the transaction is sent again; a transaction
  that confirms before it was ever sent by this node is logged when it
  reaches the refund depth, before the queue is told it is paid.
- **OC-49.** An entry carries its value in the shop's currency, in minor units
  with the currency's code (`SAT`, whose minor unit is the satoshi, for
  Bitcoin Standard), or no value. A refund's value is the one its request
  stated; without one, the satoshis paid out at the sale's last quote (the
  last quote the counter's records give, as a cash refund is valued, CR-71),
  rounded down; without a quote, none. A cash-out's value is the satoshis
  paid out at the price the owner confirmed when the cash-out was queued
  (OA-102); without one, none. A value that is none is never made up. The
  satoshis of a cash-out of everything are those its transaction paid out,
  read from the transaction itself, also when it was finished on a
  hardware signer and settled from a block (OC-52); they are never logged
  as zero.
- **OC-50.** The log is `paid-log.bin`: `OBM&`, version 1, and then entries of
  118 bytes, each ending in the first 8 bytes of the SHA-256 of the 110 bytes
  before it. The file is only added to: a new file is built from the old
  entries and the new ones, stored through the watcher's atomic write, and
  swapped in only when the store succeeds. `archive_paid` does not touch it.
  It holds at most 100,000 entries; past that nothing more is appended and
  the node notes it. It is read strictly: a length that is not a whole number
  of entries, a check that does not match, a flag that is not 0 or 1, a field
  that a kind does not use and is not zero, an entry that repeats an earlier
  one, a currency that is not three capital letters and a kind that is not
  known refuse the whole file. A node whose log does not read back does not
  start: the fault is the one of any other file of the node that does not read
  back (`NodeError::Files`).
- **OC-51.** The queue keeps what the log needs until the entry is logged: a
  cash-out keeps the price the owner confirmed for it (a currency code and
  minor units per bitcoin, never zero), and a refund that came from a
  counter's request keeps the request's number and the sale's last quote. The
  stored queue is version 3 from the first such entry (AS-65, the codec's
  module docs): after the Box claims, a list of the cash-outs' prices and a
  list of the refunds' notes, each ascending by number and each naming a
  queued entry. A queue with neither stays version 1 or 2, byte for byte as
  before.
- **OC-52.** A block that pays or beats a transaction given to a hardware
  signer is written down as owed before the wallet's coin view takes the
  block, and the entries are settled from that record, again after every
  block, after key 1 recovery and at start, until each is settled or its
  block has left the chain. A settlement retried is not logged twice.

## What these rules do not say

- **They do not watch an address set aside all the time.** A payment to one
  is seen at the next deep check, up to `DEEP_CHECK_SECS` later (OC-24),
  and a spend of it, when it is found, from then on. The owner is told
  (OC-25). A shop that cannot wait that long for a late payment should not
  issue more addresses than the live watch holds.
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
- **They do not keep a given-up merge from coming back.** If the owner gives
  up the session of a merge (AS-66), its entry returns to the queue with the
  change address it was made for. That address is still the wallet's own, but
  the Key app then shows it as a payment to an address, not as a move
  inside the wallet. The owner cancels such an entry (AS-54).
- **They do not give the fee a transaction pays.** The line is a guide. The
  owner sets the rate of a spend when the session is built, and the fee of a
  merge is the one the offer stated only at the rate it names.
