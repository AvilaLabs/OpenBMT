# The counter node against Bitcoin Core (2026-11-10)

**Question.** Do the pieces of the counter's receive path work together? A
sale is opened on an address from a signed batch of a real 2-of-3 wallet
description. A payment from Core's wallet is seen, mined, buried and
reorganized away. A restart shows the same states, repeats no report and
does not ask for what it already checked. A kill right after a
confirmation leaves the sale confirmed.

**Answer.** Yes, after one fix: all nine steps pass against one Core node
on regtest, in repeated full runs of the script. Step 1 is not a check of
the health rules; see below.

**One defect was found and fixed.** Step 9 failed when it was first run.
The driver saved its headers at most once a minute, and the counter
stored a confirmation as soon as the block came. A node killed in between
started with a sale that named a block its stored headers did not hold,
took the block for one that had left the chain, showed `DoNotHandOver`,
and reported the sale to the POS as unpaid and then as paid a second time
once the peer was back. The driver now saves the headers before it hands
a block to the app (NR-73). A second gap, found by reading and not by a
run, is closed too: after a reorganization the stored checked height
could stay above the fork (see the end of this note).

## Setup

`crates/openbmt-counter-node` joins the counter's judgment of a sale
(`openbmt-counter`), the network driver (`openbmt-net-io`) and a directory
of files (`DirStore`). `scripts/counter-e2e-core.sh` drives the example
`crates/openbmt-counter-node/examples/counter_regtest.rs`: the real `Node`
with the real `Driver`, the real sockets, the system clock and
`/dev/urandom`, one fixed peer (the Core node, with `allow_local` because it
is on this machine), and the real counter on a directory. The example reads
one command per line on its standard input and prints one line for each
result and each report. The script reads those lines and drives Core with
`bitcoin-cli`.

The identities (owner, approver, counter) and the three wallet keys are made
from fixed bytes marked "test only, never use" in the example. The wallet
description is `wsh(sortedmulti(2,...))` on the BIP48 account path, built by
`WalletDescription`; the batch of 20 addresses is derived by `BatchBuilder`
and signed by the owner and approver identities. No script is derived by
hand. The settings are `Settings::defaults` with the counter given a full
share; confirmations are the defaults, 1 to confirm and 6 for final.

Versions: Bitcoin Core `v32.1.0` (the binaries in
`~/.local/opt/bitcoin-core-32.1/bin` only), rustc 1.98.1. Core ran on regtest
in a scratch directory with `-blockfilterindex=1 -peerblockfilters=1
-listen=1 -bind=127.0.0.1:18967 -txindex=0`, and was stopped at the end. The
node and the scratch directory (Core's data, the counter's directory and
the logs) were removed on every exit. A wallet created for the test mined
150 blocks to itself and paid the sales. A second wallet, blank and
watch-only, took the wallet description by `importdescriptors`.

## The single-peer switch (step 1 is not a check of the health rules)

The health rules (WR-50, WR-51) need peers in three network groups and two
groups with filter peers. One local node is one peer, so the real rules
cannot say "healthy" here, and the library was not weakened. The example has
a switch, `--regtest-single-peer`, that replaces the health answer with
"healthy" in the example alone. It prints three `warning` lines when it
starts. The `health` command prints the answer in use and what the real
rules say for the same peers:

```text
health used=healthy real=TooFewPeerGroups peers=1
```

Step 1 therefore shows that the node reaches the peer and the tip, takes the
`Peers` event, and can open a sale once the answer is "healthy". It does
not show that the rules work with real peers. The rules have their own
tests in `openbmt-watch`, and the node's tests check that `open` refuses
before the first `Peers` event and while the real rules say a problem
(`opening_needs_the_health_rules_to_hold`).

## Steps

The lines are from the last run. Hashes and addresses change from run to run
only where the clock enters (the keys are fixed, so the addresses repeat).

| # | Step | Result |
|---|---|---|
| 1 | The node reaches the peer and the tip and can open a sale (NOT a check of the health rules) | PASS: one ready peer, tip 150, `used=healthy real=TooFewPeerGroups` |
| 2 | The first address is index 0 of the description, as Core derives it | PASS: `bcrt1qq0lmg2md704mfuc4wctpzz3h22ejpsyqzcmegclfak30ucwwnu7qtqejlc` from both |
| 3 | Core pays exactly what the sale needs: Waiting, seen by one peer, no instant | PASS: 100000 sats; `Waiting`, `mode=None`, no report |
| 4 | One block: Confirmed, PosPaid once | PASS: `Confirmed (1 confirmations)`, `report PosPaid index=0` once |
| 5 | Five more blocks: Final, BooksFinal once | PASS: `Final`, `report BooksFinal index=0` once, PosPaid still once |
| 6 | The payment's block leaves the chain: DoNotHandOver, PosUnpaid once | PASS: `A block that held the payment is no longer on the chain.`, `report PosUnpaid index=1` once |
| 7 | Restart on the same directory: same states, no repeated report, no old blocks asked for | PASS: sale 0 Final and sale 1 DoNotHandOver both times, 0 reports in the second run, 0 bytes of `getdata` |
| 8 | The watch-only wallet sees the payment as its own coin | PASS: `listunspent` shows the txid, the address and 0.001 BTC |
| 9 | Killed with no clean stop right after a confirmation: still Confirmed, nothing reported again | PASS after the fix: `Confirmed` with no peer and with the peer back, PosPaid once, PosUnpaid never. Before the fix: `DoNotHandOver`, PosUnpaid once, PosPaid twice |

What each step compared, beyond the PASS:

- **Step 2.** `importdescriptors` took the canonical description with
  `<0;1>/*`, and `deriveaddresses "$DESC" "[0,0]"` gave the receive address
  at index 0. The example showed the address of the first sale, which is the
  first script of the signed batch. They are the same string.
- **Step 3.** The payment was made with `sendtoaddress` with the exact amount
  the sale printed. The state came from a `Sighting` event from one peer
  address, so the watcher had one network group and no check mode. That is
  `Waiting` with the text "Not yet seen by enough of the network." The count
  of `PosPaid` reports was zero.
- **Step 6.** The second sale was paid and confirmed once. Its block was
  invalidated and two empty blocks were mined to another address with
  `generateblock <address> '[]'`. The payment went back into Core's mempool
  and was not mined again (the script checks `getrawmempool`).
- **Step 7.** Core's `getpeerinfo` was read for the node's inbound
  connection, once per run, before the node was stopped and again in the new
  run after it reached the tip and ran six more seconds. See the next
  section.
- **Step 9.** A third sale was paid and confirmed once, and the example
  was killed with signal 9 as soon as it showed `Confirmed`. It was then
  started with a peer address that nothing listens on, so that its answer
  came from the disk alone, and then once more with the real peer.

## What the restart asked for

`bytesrecv_per_msg` on Core's side is what Core received from the node.
All runs below are against the same chain, height 158.

| Run | `getdata` received | `getcfilters` received |
|---|---|---|
| First run (the node follows the chain from height 150, with two sales) | 671 bytes | 0 |
| Restart on the same directory (checked height 158 kept) | 0 bytes | 0 |
| The same directory with `checked.bin` deleted, so the node checks from the first block | 9638 bytes | 0 |
| An empty directory | 0 bytes | 0 |

A `getdata` for one block is 61 bytes (24 of header, 1 of count, 36 for one
item). 9638 bytes is 158 of them: one for each block of the chain. The
restart asked for none. The script's pass condition is at most five things
(305 bytes), no filter, the same states, no report, and one inbound peer.

No run asks for a filter. With one peer the manager has fewer than two filter
peers, so every block it checks is fetched in full (NR-42). That is why
`getdata` is the message that tells a restart from a fresh start here, and
`getcfilters` stays at zero in every row. The filter path was not used and
is not covered.

An empty directory fetches nothing, because a counter that has shown no
address watches no script, and the manager ends a block's work at once when
there is nothing to look for. That is the reason for "absent means the tip of
the stored headers" in the start rule; it is not a measure of a start that
has to catch up.

## What is NOT covered

- **The health rules (WR-50 to WR-53)** with real peers. The single-peer
  switch hides them in this run (see above).
- **Instant acceptance.** No heartbeat from the owner's watcher was taken
  in this run, so the heartbeat was always "not verified" and a sale could
  not be accepted at once. (The node takes the heartbeat since this note was
  written; see `instant-e2e-core-2026-11-10.md`.) No step reached
  `PaidInstant`, and none was meant to. The unit test
  `announcements_add_groups_only_for_cached_transactions` shows a payment
  seen by three groups staying `Waiting` with "no independent watcher".
  Sightings from three groups were not made against Core.
- **More than one peer.** One fixed local peer, one network group. Filters
  from two peers, a peer that lies, a peer that drops out, and the
  `AlsoAnnounced` path against Core (only in unit tests, with a hand-made
  driver).
- **Tor**, the SOCKS5 path and the one-circuit-per-peer rule.
- **The real network.** Regtest only, on `127.0.0.1`, with `allow_local`.
  No seeds, no address book that learned anything, no mainnet headers and no
  checkpoint.
- **A crash at other moments.** Step 9 kills the node at one moment:
  right after a confirmation. A kill in the middle of a write is covered
  for the directory in unit tests (a cut-short last record, a failed
  write), not against Core. A loss of power, where the disk may lose what
  was not yet flushed, was not tried.
- **Two processes on one directory.** The directory has no lock.

## Where the existing crates did not fit

Each was handled in the node, and each is a place where a later change in
the crates would help.

1. **`Counter` has no way to list its sales.** It has `sale(index)` and
   nothing that walks them. The node needs them to judge what an event
   touched and to decide NR-44. It keeps its own set of indexes, from the
   slot files at start and from `open` after that. A sale left behind by an
   `open` that failed after it stored the sale is not in the set until the
   next start (it expires by itself under CR-07).
2. **NR-44 needs a status the counter does not give without a side effect.**
   `SaleWatch::status` takes `&mut self` because it keeps a memory for
   "confirmed at the last status". The node asks it on a clone, so that its
   own question does not use up the one `unconfirmed_again` that `judge`
   needs. The clone copies every transaction of every live sale on every
   pass that an event touched. A read-only status on `Sale` would remove it.
3. **`watch_from` is fixed when the driver starts.** The rules want "the tip"
   for a counter that never ran, but at that moment the header store may be
   empty, and its tip is its start block (genesis on regtest, the checkpoint
   on mainnet). The node writes the checked height before any address can be
   shown, and a later `Checked` event raises it. On mainnet a first start
   has a `watch_from` of the checkpoint height, not of the real tip. That
   costs nothing while no address is shown: with no script to look for, a
   block is done at once, and the checked height follows the headers up.
   A sale cannot open before the headers have caught up, because the
   health rules refuse it while peers show a higher chain (WR-52).
4. **The checked height and a reorganization.** The driver sends `Checked`
   only when the number rises. After a reorganization the manager checks the
   new branch from the fork, but the height it reports stays above the fork
   until it passes the old number. A node that stopped in that window would
   have started above blocks of the new branch it did not check. The node
   now takes its stored height down to the block below the fork when
   blocks leave the chain, before it does anything else with the event. A
   unit test covers it; no run against Core stops a node in that window.

Smaller things that did not fit exactly:

- `Counter::watched_scripts` returns the scripts that were shown. NR-41
  says the filters are matched against those not yet used in the current
  batch too. A script that was not shown cannot be paid, so nothing is lost,
  but the node calls `watch_script` when `open` returns an address
  rather than relying on the starting list.
- `FileStorage` keeps only `Key::Headers` and `Key::Book`, and its write
  halves are `pub(crate)`. `DirStore` writes a file the same way
  (temporary file, flush, rename, flush the directory) by its own code.
- `Stored.records` is an owned `Chain`, so `Loaded::stored` clones the
  records on each call.
- The `Peers` event carries the tip time but the `Chain` event does not. The
  node's `ChainView` takes the time only when the event's tip height is its
  own tip, so after a reorganization its tip time is the old one until the
  next `Peers` event (at most 10 seconds).
- After a restart, and before the first `Peers` event, the health answer is
  "the check is not working". A sale that is `Waiting` shows
  `DoNotHandOver (cannot check)` for those seconds, because CR-12 rule 7
  asks for a healthy check. Sales that are `Confirmed`, `Final` or
  `DoNotHandOver` are not affected, and the check showed no wrong report.

## Notes

- **A block of equal work does not reorganize.** After `invalidateblock` on
  the tip, one new block has the same work as the block it replaces, so the
  header store keeps the old one as best and nothing leaves the chain. Step
  6 mines two blocks, as the driver's own check does.
- **An empty directory fetches no block.** A first run asks for none until
  the first address is shown. The numbers for a start that has to catch up
  come from the copy with the checked height deleted.
- **What a PASS rests on.** Each one rests on a counted line or a compared
  value (the report counts over all logs, the two derived addresses, the
  bytes in `getpeerinfo`), not on the absence of a failure.
- **Order of events.** A `Block` event came after the `Chain` event that
  puts it on the best chain in every run. If it had not, `Counter::block`
  would have refused it as "not on the chain" and the sale would not have
  confirmed. The node does not depend on it beyond logging and holding the
  checked height below such a block.
