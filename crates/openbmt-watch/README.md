# openbmt-watch

What the OpenBMT counter believes about a payment, as a Rust library with no
network, no clock and no storage. The caller passes in what was received,
from whom, and the time in seconds, and answers three questions about its
header chain. The library decides what the evidence proves and nothing more.
It produces the facts that `openbmt_core::policy::Inputs` reads (`mode`,
`received_sats`, `conflict_seen`, `secs_since_first_seen`, `check_healthy`);
it does not call `evaluate`.

The specification is [docs/WATCHER-RULES.md](../../docs/WATCHER-RULES.md).
Each rule has an ID (WR-01 and on). Every refusal names its rule
(`Refusal::rule()`, and its text starts with the ID), and every test is named
for the rule it proves.

**This crate has had internal review only. No independent security audit has
been done yet.** Do not use it with real funds.

## The calls

| Call | What it does | Rules |
|---|---|---|
| `HeaderChain` (trait) | The caller's view of the best header chain: height of a hash, tip height, tip time. | WR-01 |
| `confirmations` | Tip height minus block height plus one; `None` if the block is not on the chain. | WR-01, WR-02 |
| `check_header` | The header meets the target its own bits state. | WR-03 |
| `HeaderStore::new` | A header store for mainnet (from a checkpoint), signet or regtest (from genesis). Testnet is refused. | WR-04 |
| `HeaderStore::accept` | Check a header (parent, bits, work, time, clock) and hold it. Says whether it extended the chain, sits beside it or changed it. | WR-05 to WR-07 |
| `HeaderStore::to_bytes`, `from_bytes` | The stored form of the best chain, and a read that checks every header again. | WR-08 |
| `HeaderStore::tip`, `header_at`, `locator` | The tip, the header at a height, and the block locator for peers. | WR-07 |
| `verify_box_proof` | Header, partial merkle tree, coinbase and payment in full, all checked. Returns a `ProvenPayment`. | WR-10 to WR-16, WR-01 |
| `payments_in_block` | A full block, checked, and every payment in it to one script. | WR-20 to WR-23, WR-30, WR-01 |
| `spends_in_block` | A full block, checked, and which of its transactions spend given coins. | WR-20, WR-21, WR-42 |
| `WellFormedBlock` | The block checks that need no chain, kept so that `payments` and `spends` can both be asked of one block. | WR-20, WR-21 |
| `network_group` | The network group of a peer. | WR-41 |
| `SaleWatch::new` | The evidence for one sale: script, amount, tolerance in basis points. | WR-30 to WR-34 |
| `SaleWatch::sighting` | An unconfirmed transaction in full, from a peer or the Box, with the time. | WR-40 to WR-46 |
| `SaleWatch::confirmed` | A `ProvenPayment` from the proof or block calls. | WR-30 to WR-34, WR-42 |
| `SaleWatch::watched_outpoints`, `spent_elsewhere` | Look for spends of the payment's coins in blocks; record one. | WR-42, WR-43 |
| `SaleWatch::status` | Everything the rules say about the sale now, worked out again from the chain. | WR-31 to WR-34, WR-42 to WR-46 |
| `check_health` | Whether the check is working, and the first rule that fails. | WR-50 to WR-53 |

A refusal means nothing was believed and nothing was recorded. The one
exception is WR-34: a transaction the sale cannot keep marks the sale "needs
a person".

## How the rules map to the code

| Rules | Where |
|---|---|
| WR-01 to WR-03 | `src/chain.rs` |
| WR-04 to WR-08 | `src/headers.rs` |
| WR-10 to WR-16 | `src/proof.rs`: `verify_box_proof`, `extract_tree` |
| WR-20 to WR-23 | `src/block.rs` |
| WR-30 to WR-34, WR-40, WR-42 to WR-46 | `src/sale.rs` |
| WR-41 | `src/netgroup.rs` |
| WR-50 to WR-53 | `src/health.rs` |
| Refusals and their rule IDs | `src/refusal.rs` |
| Thresholds | `pub const`s in `src/lib.rs`, named for their rule |

Some choices worth knowing:

- The proof is read with `bitcoin::MerkleBlock` through
  `consensus::deserialize`, which refuses bytes left over. The size checks
  come first.
- `PartialMerkleTree::extract_matches` does most of WR-12: it limits the
  transaction count to 16,666, refuses more hashes than transactions, runs
  out of bits or hashes, leaves a hash or a flag byte unused, and refuses a
  node with two equal children. This crate adds a count of at least 2, spare
  bits after the last flag bit that must be zero (so a proof has one
  encoding), and the comparison of the root with the header's.
- A proof must prove exactly two transactions, the first at position 0
  (WR-13), and the coinbase must be a real coinbase with the proven ID
  (WR-14). With WR-16 this closes the depth attack: a tree read one level
  too deep or too short shows inner nodes as transactions, and an inner node
  is the hash of 64 bytes.
- The chain is asked last, so that the refusal for a bad proof or block
  never depends on the chain's state.
- A block that repeats its last transactions has the merkle root of the
  block without the repeat (CVE-2012-2459). `WellFormedBlock::parse` refuses
  any block with two transactions of one ID (under WR-21).
- `HeaderStore` asks `bitcoin::CompactTarget::from_next_work_required` for
  the retarget. A test checks it against the two retargets in real mainnet
  headers. It applies no testnet minimum-difficulty exception and no BIP94.
- Headers beside the best chain are held in the same map as the best chain.
  After a switch of chains the old blocks become side headers, so the count
  of side headers can pass 4,032 for a while; new side headers are refused
  until pruning brings it down.
- A payment found in a block is never the coinbase, and a transaction that
  pays the script but is 64 bytes without witness is refused (WR-22).

## Choices where the rules leave a corner open

- **Several transactions.** A sale can hold several standing transactions
  (a customer who pays in two parts). `received_sats` sums them. The weakest
  standing unconfirmed transaction that pays the sale decides `peer_groups`,
  `mode` and `secs_since_first_seen`: the one with the fewest groups, and
  among those the latest first-seen time. If every standing transaction is
  confirmed, `mode` is `None` and the time is 0.
- **`status` takes `&mut self`.** `unconfirmed_again` compares with the last
  answer, so the watch remembers it. It is true for one status after a
  transaction that was confirmed at the last status is no longer confirmed
  by a block on the chain, and false for the next.
- **Depth.** The sale is confirmed at depth `d` when the standing
  transactions with at least `d` confirmations pay at least the sale amount
  less `floor(sale * bps / 10,000)` (`u128` product; `bps` above 10,000 is
  10,000) and something was paid. `min_confirmations` is the largest such
  `d`, or 0.
- **Displacement (WR-43).** While a transaction of the sale has at least
  one confirmation, every other known transaction that conflicts with it is
  displaced. The same holds for a transaction of the sale whose coin is
  spent by a transaction in a block that is on the chain and was given to
  `spent_elsewhere`. All of it is worked out again at every status.
- **Conflicts that are kept.** A transaction that conflicts with a known one
  and is not a fee bump is stored, counts toward the limit of 20, and sets
  the conflict flag for good. A fee bump has to be a fee bump for every
  transaction it conflicts with, and none of them may ever have been
  confirmed, or it is a conflict and not a replacement.
- **Sources.** The same transaction from the same network group changes
  nothing; from a new group it adds the group. A Box sighting sets the
  "reported by the Box" mark and is never a group. A transaction is known by
  its `compute_txid()`, so the same transaction with other witness bytes is
  the same transaction.
- **Time (WR-46).** The time recorded for a sighting is never earlier than a
  time recorded before. A clock passed to `status` that is earlier than the
  first-seen time gives 0 seconds.
- **Health.** The library keeps no memory, so WR-52 has a hand-off.
  `check_health` returns `behind` (some peer announces a height more than 2
  above the tip). The caller stores the time `behind` first became true and
  passes it back as `behind_since`. WR-52 fails when `behind` is true and
  more than 600 seconds have passed since then. With no stored time the
  clock has not started.
- **Network groups.** Beyond the rule: `0.0.0.0/8`, multicast and reserved
  addresses (`224.0.0.0/3`), IPv6 multicast and site-local addresses are in
  the single local group, and an IPv4 address carried in IPv6 is checked for
  "local" too.

## What it deliberately does not do

These are the limits from section 7 of the rules.

- It does not prove that a payment will confirm. An unconfirmed payment can
  be replaced or never mined, whatever was seen.
- It checks headers, not blocks. `HeaderStore` checks proof of work,
  difficulty and time, and trusts its start block. If the header chain it is
  given is wrong, every confirmation that rests on it is wrong.
- It does not see what is hidden. Peers and the Box can stay silent about a
  payment or a conflict. It never reports "not paid" (WR-23).
- It does not judge fees or inputs. A light client cannot.
- It does not decide. Whether to hand over goods is the answer of
  `openbmt_core::policy::evaluate`, or a confirmation at the shop's depth.

The app around the library does the rest: the light client and its peers,
the Box connection, the clock, keeping a `SaleWatch` for each open sale, and
storing the time `behind` began.

## Tests

`cargo test -p openbmt-watch`. There is at least one test for every rule ID
(a test checks this against the rules file), named `wr_NN_...`, and every
refusal and health problem is checked to name a rule of the file. Blocks are
real: the tests mine headers on regtest difficulty, build blocks with their
merkle roots and make proofs with `PartialMerkleTree`, or write a proof byte
by byte when the library would never build it. The attacks on the proof have
a test each: a proof without the coinbase, a proof for three transactions, a
wrong coinbase, a coinbase that is not one, a payment that is a coinbase,
64-byte transactions, a duplicated last pair, a wrong root, a header that was
never mined, a header not on the chain, a trailing byte, an oversize proof,
and the depth attack. A header chain that the tests can change simulates
reorgs. The header store is tested on 4,043 real mainnet headers that cross
two retargets (`src/tests/data`), and on mined regtest headers. Fuzz targets `watch_box_proof`, `watch_block` and `watch_header_store` are in `fuzz/` at
the repository root.
