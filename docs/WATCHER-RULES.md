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

The library has no network, no clock and no storage. The light client
(`bip157`) and the Box connection live in the app around it. The caller
passes in what was received, from whom, and the time. The library decides
what it proves. What to do about a payment that is seen but not confirmed is
decided by `openbmt_core::policy::evaluate`; this library produces the
facts that decision reads.

## 1. The header chain

The counter keeps its own chain of block headers in every mode (§5.1). The
light client builds it: it checks each header's proof of work and the
difficulty rules. This library does not repeat that work. It sees the chain
through three questions the caller answers:

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
  however many sources it is reported.
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
    starts again from the peers that reported the new one;
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
- **WR-53.** The clock is not more than 2 hours behind the tip's header
  time. A header's time can run up to 2 hours ahead of real time and no
  further, so a clock further behind than that is wrong. The library then
  reports "clock mismatch": the counter's price lock and sale expiry must
  not trust the local clock (§5.1). A clock that is far ahead looks the
  same as a tip that is old, and is reported under WR-51.

## 7. What these rules do not do

- **They do not prove that a payment will confirm.** An unconfirmed
  payment can be replaced or never mined, whatever was seen. The limits of
  §6 are the protection; the labels say what was checked.
- **They do not check proof of work across the chain.** That is the light
  client's job (section 1). If the header chain is wrong, every
  confirmation that rests on it is wrong.
- **They do not see what is hidden.** Peers and the Box can stay silent
  about a payment or a conflict. Counting network groups makes that harder
  and does not make it impossible.
- **They do not judge fees or inputs.** A light client cannot (§6).
- **They do not decide.** Whether to hand over goods is the instant
  decision's answer (`openbmt_core::policy`) or a confirmation at the
  shop's depth.
