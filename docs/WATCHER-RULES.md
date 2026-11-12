# Watcher rules: what the counter believes about a payment

Status: specification for `crates/openbmt-watch`, which implements it.
Each rule has an ID so that a test can name the rule it proves. The rules
implement `docs/DESIGN.md` §5.1 ("Checks payments itself") and the inputs
of §6. Where this file is more exact than the design, this file is what the
code follows.

The watcher is the counter's own judgment of what has arrived. Everything it
is told comes from somewhere that can lie: Bitcoin peers, the shop's Box, the
network the counter is on. These rules say what each kind of evidence is
allowed to prove, and nothing is believed beyond that.

The library has no network, no clock and no storage. The peer connections
(`docs/NETWORK-RULES.md`) and the Box connection live around it. The caller
passes in what was received, from whom, and the time. The library decides
what it proves. What to do about a payment that is seen but not confirmed is
decided by `openbmt_core::policy::evaluate`; this library produces the
facts that decision reads.

## 1. The header chain

The counter keeps its own chain of block headers in every mode (§5.1), and
this library's `HeaderStore` is what checks it. The peer connections
only fetch headers. Nothing a peer says about the chain is believed:
every header goes through the rules below first.

- **WR-04.** The header store starts from one block that it trusts, and
  counts work from there. Mainnet has a checkpoint; signet and regtest
  start at their genesis blocks. The mainnet checkpoint is block 967,680,
  a retarget block, compiled into the library. Testnet and Testnet4 are
  refused.
- **WR-05.** A header is held only if all of these hold, checked in this
  order: its parent is held; its bits are exactly what the difficulty rules
  require on its own branch; its hash meets the target its bits state; its
  time is later than the median time of the previous 11 headers on its own
  branch (as many as exist, back to the start block). The required bits are
  worked out as Bitcoin Core does: inside a period of 2,016 blocks, the
  parent's bits; at a period's first block, the parent's target scaled by
  the time the last period took, which is limited to between a quarter and
  four times 14 days, capped at the network's limit and rounded through the
  compact form. Regtest bits never change. The store does not apply the
  testnet minimum-difficulty exception or BIP94, and refuses those
  networks (WR-04).
- **WR-06.** A header whose time is more than 2 hours ahead of the caller's
  clock is refused for now. Nothing is stored and nothing is marked bad:
  the same header is checked afresh when it is offered again. The clock is
  asked after the checks of WR-05, so that a bad header is reported as bad
  whatever its time.
- **WR-07.** The best chain is the one with the most total work, counted
  from the start block. A header with equal work does not replace the
  current best chain. Every header of the best chain is kept. A header
  beside it is kept only while its height is within 2,016 of the tip's, and
  at most 4,032 are kept; a new one past that is refused. A header whose
  parent is more than 2,016 blocks below the tip is refused as well, since
  it could not be kept.
- **WR-08.** The store has a stored form that the caller keeps: the magic
  `OBMW`, version 1, one byte for the network, the hash of the start block,
  a 4-byte little-endian count (at most 2,000,000), then that many 80-byte
  headers of the best chain after the start block, and nothing else. Reading
  it checks the network and the start block against the library's, then
  puts every header through the checks of WR-05, in order, each extending
  the tip. WR-06 is not asked, since the headers are old. Any failure
  refuses the whole file. Headers beside the best chain are not stored. The
  best chain cannot grow past 2,000,000 headers after its start block: a
  better header past that limit is refused before it is stored or the best
  chain is changed. The store keeps all best-chain headers; it does not
  prune old headers or replace them with a checkpoint, because that would
  remove proof needed to check the chain. If the limit is reached, update
  the counter or contact the OpenBMT project for help before restarting.

Why work counted from the checkpoint is enough: under WR-05 the
difficulty can fall by at most a factor of four per period, and each
period is 2,016 blocks that must be mined at the difficulty then in
force. An attacker cannot hand over a cheap chain from the checkpoint;
every header costs real work at close to the network's difficulty.

Stored headers are not protected from being rolled back to an older copy.
An older copy is a valid chain with an old tip, and WR-51 reports it.

The rest of the library sees the chain through three questions:

- the height of a block hash on the best chain, if it is there;
- the height of the tip;
- the time in the tip's header.

- **WR-01.** A block counts only if its hash is on the best chain when the
  question is asked. A block that has left the best chain proves nothing,
  and a payment confirmed only in such a block is no longer confirmed
  (WR-33).
- **WR-02.** The confirmations of a payment in a block at height `h` are
  the tip height minus `h`, plus one.
- **WR-03.** Whatever header is handed in with a proof or a block is also
  checked on its own: its hash meets the target its own bits field states.
  This does not replace WR-01. It stops a header that was never mined from
  being looked up at all.

## 2. A confirmation the Box reports (merkle proof)

The Box can hold information back. It must not be able to make up a
confirmation. A confirmation from the Box is believed only with a proof
made of four parts: the block header, a partial merkle tree (the format of
BIP37 and of Bitcoin Core's `gettxoutproof`), the block's coinbase
transaction in full, and the payment transaction in full.

- **WR-10.** The proof is at most 64 KiB **[to tune]**, and each of the two
  transactions at most 400,000 bytes. The bytes are a header followed by a
  partial merkle tree and nothing else.
- **WR-11.** The header passes WR-03 and its block is on the chain (WR-01).
- **WR-12.** The partial merkle tree is well formed: the transaction count
  is at least 2 and at most 16,666; there are no more hashes than
  transactions; every hash and every flag bit is used; and no node has two
  equal children (CVE-2012-2459). The root it gives equals the header's
  merkle root.
- **WR-13.** The tree proves exactly two transactions: one at position 0
  and one at a later position. A proof that covers more or fewer is
  refused.
- **WR-14.** The coinbase transaction handed in has the ID proven at
  position 0, and it is a coinbase: one input, whose previous output is the
  null outpoint.
- **WR-15.** The payment transaction handed in has the ID proven at the
  later position. It is not a coinbase.
- **WR-16.** Neither transaction is 64 bytes long without its witness.

Why the coinbase is part of the proof: a merkle tree does not mark which of
its nodes are transactions. An inner node is the hash of 64 bytes, and so
is a 64-byte transaction, so a proof can stop one level short or go one
level too deep. In the BIP37 format every proven transaction sits at the
same depth, which follows from the transaction count the proof states. The
coinbase is the one transaction whose form the verifier knows and whose
place is fixed. When a real coinbase sits at position 0 at that depth, the
depth is the true one, and the payment beside it at the same depth is a
transaction of the block and not an inner node. WR-16 closes the remaining
case, where a transaction is itself read as an inner node.

## 3. A confirmation the light client finds (full block)

The light client matches compact block filters against the shop's
addresses and downloads the blocks that match. A filter match alone proves
nothing: filters have false positives, and a peer can serve a false filter.

- **WR-20.** The block is at most 4,000,000 bytes, its header passes WR-03
  and its block is on the chain (WR-01).
- **WR-21.** The merkle root computed from the block's transactions equals
  the header's merkle root, the first transaction is a coinbase and no
  other transaction is.
- **WR-22.** A payment found in the block is not 64 bytes long without its
  witness, and is not the coinbase.
- **WR-23.** A missing payment is not proof of no payment. A peer that
  serves a false filter can hide a block. The library never reports "not
  paid"; it only reports what has been proven so far.

## 4. What a transaction pays

- **WR-30.** What a transaction pays a sale is the sum of all its outputs
  whose script is exactly the sale's address script. Nothing else in the
  transaction counts.
- **WR-31.** A sale's received amount is the sum over the transactions that
  pay it and are still standing: seen or confirmed, and not displaced by a
  conflict (WR-42). A transaction counts once, however many times and from
  however many sources it is reported. Standing transactions that conflict
  with each other count once between them, at the smallest amount any of
  them pays: only one of them can confirm, and the sale must not show more
  than the worst case.
  The library also names the transactions that make up that amount, as
  the last status found them: the standing transactions that pay the sale,
  and what each pays, ascending by transaction ID. Of two or more that
  conflict, the one that pays the least is named (the smaller ID if they
  pay the same). What they pay adds up to the received amount. The earliest
  first-seen time of these (WR-46) is given as well; for a payment that was
  never seen before a block held it, it is the time the caller gave when
  it told the library of the block, and when the caller gave none the
  payment has no time.
- **WR-32.** A sale is confirmed at depth `d` when transactions with at
  least `d` confirmations each (WR-02) pay it at least the sale amount less
  the shop's tolerance. The tolerance is the settings' value in basis
  points of the sale amount, rounded down.
- **WR-33.** Confirmation is worked out again whenever the chain is asked
  about. If a block leaves the best chain, the payments confirmed in it go
  back to "seen", and the sale's status goes back with them. The library
  says so in its answer; it does not hide that a sale which was confirmed
  no longer is.
- **WR-34.** Each of a sale's transactions is kept in full, at most 400,000
  bytes each and at most 20 per sale **[to tune]**. Past that, new
  transactions for the sale are refused and the sale is marked "needs a
  person".

## 5. Unconfirmed payments: who saw it, and conflicts

A light client sees an unconfirmed transaction only as peers relay it. It
cannot see the amounts of the inputs, cannot tell whether the inputs are
confirmed and cannot work out the fee (§6). These rules claim none of
that.

- **WR-40.** A sighting is a transaction in full, together with where it
  came from: a Bitcoin peer (with its address) or the Box. The transaction
  must pay the sale something (WR-30), or spend a coin that a transaction
  already known for the sale spends (WR-42). Anything else is ignored.
- **WR-41.** Peers are counted by network group, not by address. Two peers
  in one group count once. The groups: an IPv4 address by its first 16
  bits; an IPv6 address by its first 32 bits; an IPv4 address carried
  inside IPv6 (mapped, 6to4, Teredo) by the IPv4 rule. Every peer reached
  by a name that anyone can make without cost (Tor, I2P, CJDNS) is in one
  single group together, because an attacker can be all of them. Local
  and private addresses are one single group as well. The Box is not a
  peer and is never counted as one.
- **WR-42.** Two different transactions conflict when they spend at least
  one coin in common. When a transaction arrives that conflicts with one
  the sale already knows:
  - if it is a fee bump in the sense of
    `openbmt_core::policy::classify_replacement` (it pays the sale's script
    at least as much), it takes the earlier one's place. The sale's
    "first seen" time starts again from this sighting, and the peer count
    starts again from the peers that reported the new one. The sale
    remembers the ID of the one replaced (up to 64) and ignores later
    unconfirmed sightings of it: a peer that is slow to hear of the bump
    still relays the old one, and that is not a new conflict. If the old
    one turns up in a block on the chain it is taken as a confirmation
    like any other, and a conflict is set;
  - otherwise the sale has a conflict. The conflict flag is set and it
    stays set for the life of the sale, even if the conflicting
    transaction is never seen again. Both transactions are kept.

  A conflict is also set when a block on the chain holds a transaction that
  spends one of the payment's coins and is not the payment.
- **WR-43.** Once one of two conflicting transactions is confirmed
  (WR-32 at depth 1), the other is displaced and no longer counts toward
  the received amount (WR-31). If the block leaves the chain, it counts as
  standing again (WR-33) and the conflict flag stays set.
- **WR-44.** "Seen by the light client" means that peers from at least 3
  network groups **[to tune]** reported the same transaction. Fewer groups
  is "reported" and gives no instant label.
- **WR-45.** The check mode handed to the instant decision follows from the
  evidence, never from what a source says about itself:
  - reported by the Box and seen by the light client:
    `NodeAlsoSeenByLight`;
  - seen by the light client and not reported by the Box: `Light`;
  - reported by the Box and not seen by the light client:
    `BoxReportsOnly`;
  - anything less: no mode, and the sale waits for a confirmation.
- **WR-46.** The time since first seen, for the conflict watch of §6, is
  counted from the first sighting that made the transaction "seen" under
  WR-44 or reported by the Box, on the caller's clock. If the clock
  passed in is earlier than a time already recorded, the time since first
  seen is zero.

## 6. Health

Instant acceptance needs a check that is working. The library answers
"healthy" only when all of these hold, and names the first that does not.

- **WR-50.** The light client is connected to peers in at least 3 network
  groups (WR-41), and at least 2 of those groups have a peer that serves
  compact block filters **[to tune, E3]**.
- **WR-51.** The tip's header time is at most 2 hours behind the clock
  **[to tune]**. Blocks are sometimes an hour apart; a longer silence more
  often means the counter has been cut off.
- **WR-52.** No connected peer has announced a best height more than 2
  blocks above the counter's tip for longer than 10 minutes **[to tune]**.
  The library keeps no state: the caller holds the time since when a peer
  has been ahead and passes it back with each check. A caller that drops
  it never fails this rule.
- **WR-53.** The clock is not more than 2 hours behind the tip's header
  time. A header's time can run up to 2 hours ahead of real time and no
  further, so a clock further behind than that is wrong. The library then
  reports "clock mismatch": the counter's price lock and sale expiry must
  not trust the local clock (§5.1). A clock that is far ahead looks the
  same as a tip that is old, and is reported under WR-51.
- **WR-54.** The counter has not known a header above the height it has
  checked in full for more than 15 seconds **[to tune]**. Headers alone do
  not show that the blocks behind them arrived: a peer that holds back a
  block body leaves the headers fresh. The checked height is the one of
  `docs/NETWORK-RULES.md` (NR-40 to NR-45). The library keeps no state:
  the caller holds the time since when the tip has been above the checked
  height and passes it back with each check. A caller that drops it never
  fails this rule. The problem is named after the rules WR-50 to WR-53, so
  it is reported only when none of them fails. It turns instant acceptance
  off and nothing else: a sale still opens (CR-01) and waits for a
  confirmation.

## 7. The merchant's own node

The merchant may set one own node: a Bitcoin Core node that the counter's
driver reaches over the peer protocol, at a Tor v3 address that the owner set
(`docs/NETWORK-RULES.md`, NR-98 to NR-103). It is a second check, not a
replacement for the light client. This library never sees an address. The
driver tells it whether the node is connected, how far it has got, when it
was last heard and which transactions it holds in its memory pool, and the
library keeps that apart from what the random peers report.

- **WR-60.** The own node is a source of its own kind and never a peer. What
  it reports is not a sighting: it adds no network group (WR-41), starts no
  clock (WR-46), changes no amount (WR-31) and has no part in WR-44. It is
  not in the peers of the health check (WR-50 to WR-52), so its state never
  turns the light client's health on or off. The library does not check who
  the node is: the driver names it, and only a connection made to the
  configured Tor v3 address is named (NR-98, NR-99).
- **WR-61.** A report from the own node names a transaction it holds. If the
  sale knows that transaction the report is recorded on it, and
  `SaleStatus::own_node_saw` is true while the weakest standing unconfirmed
  payment (WR-45) has been reported. If the sale does not know the
  transaction nothing is recorded and the call says so; the caller asks the
  node again when the light client has sighted it (WR-65).
- **WR-62.** The own-node check mode is `NodeAlsoSeenByLight` through the
  own node only when all three hold: the own node has reported the
  transaction (WR-61), the light client has seen it (WR-44), and the own node
  can be used now (WR-63). The own node alone gives no mode, however many
  times it reports. The Box's rules (WR-45) are as before; the own node adds
  a second way to the same mode and never lowers one the Box gave.
  `SaleStatus::own_node_counted` is true exactly when the own node is what
  makes the mode.
- **WR-63.** The own node can be used when it is connected after its
  handshake, has shown a best height and has been heard from, and none of
  these fails, checked in this order. **[to tune]**
  1. The node broke a rule of the peer protocol: unusable until the app asks
     the driver to try it again (NR-100).
  2. There is no connection to it. Until a first connection since the
     start, the last retry (NR-100) or the new setting, and for less than 120
     seconds after that moment, the line says the counter is connecting
     (Tor takes time to build the way to the node), not that the node is not
     reachable; the counter checks with its light check meanwhile. After 120
     seconds with no connection, and at once after a connection that was
     dropped, it says not reachable.
  3. It has been silent for more than 10 minutes.
  4. The counter's tip is 2 or more blocks above its best height, or 1
     block above it for more than 5 minutes without a break. A new block
     reaches the counter's peers before it reaches the own node, so one
     block of lag is normal for a moment; without the 5 minutes the shown
     check would fall to the light check and back at every block. Catching
     up starts the count over.

  A node that is ahead of the counter is not a problem. The health line for
  the own node says which rule failed, the node's best height, the counter's
  tip and when the node was last heard.
- **WR-64.** A fallback is never silent. When the own node reported a payment
  and cannot be used, the status has `own_node_saw` true and `own_node_counted`
  false, its mode is the light check's (WR-45), and the own node's health
  line has words that say the counter is checking with its light check and
  why: connecting for the first two minutes, not reachable since a time, behind by blocks or by one block for
  too long, silent since a
  time, dropped for breaking a rule, or not yet known. Every state of the
  line has words (SC-01) and the rule number is in the small print.
- **WR-65.** Reports are not stored. A restored sale starts with none, and
  the sale's stored form is the same bytes with or without them. The counter
  asks the own node again for every standing unconfirmed payment when it has
  restored the sale, when the own node connects, and when the light client
  sights a transaction the own node reported earlier than the sale knew it.
  Until the node answers, the light check's mode applies.

## 8. What these rules do not do

- **They do not prove that a payment will confirm.** An unconfirmed
  payment can be replaced or never mined, whatever was seen. The limits of
  §6 are the protection; the labels say what was checked.
- **They do not check blocks in full.** Section 1 checks the work in
  every header and the difficulty rules. It does not run the scripts or
  check the coins of each transaction; no light client can. A payment in a
  block is as good as the work on top of it, which is why the shop sets a
  confirmation depth.
- **They do not check the signet block signature.** On signet the headers
  are checked for work only, and signet work is cheap. Test networks prove
  nothing about money.
- **They do not see what is hidden.** Peers and the Box can stay silent
  about a payment or a conflict. Counting network groups makes that harder
  and does not make it impossible.
- **They do not judge fees or inputs.** A light client cannot (§6).
- **They do not decide.** Whether to hand over goods is the instant
  decision's answer (`openbmt_core::policy`) or a confirmation at the
  shop's depth.
