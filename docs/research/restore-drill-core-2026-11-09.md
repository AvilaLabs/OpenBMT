# Restore drill against Bitcoin Core (2026-11-09)

Experiment E9 of `docs/DESIGN.md`, the Bitcoin Core part. The Sparrow part and
a hardware signer have not been run.

**Question.** Can an OpenBMT wallet be found and spent with Bitcoin Core
alone, from the wallet description in the recovery kit, with no OpenBMT
software? Does that still work when the shop has used a very large number of
addresses?

**Answer.** Yes, with three conditions that the kit has to state: the import
must name its range, the receive descriptor must be imported as not active,
and the keys sign outside Core's watch-only wallet. A restore that follows
Core's defaults finds only part of the money, and one that names the range
but keeps the other defaults can take hours for a busy shop.

## Method

`scripts/restore-drill-core.sh` runs the whole drill and checks every step.
It uses Bitcoin Core 32.1 on regtest, in a scratch directory, with a block
filter index. No real keys or funds are involved.

1. Three fresh keys are made and the wallet descriptor is built: 2-of-3 P2WSH
   `sortedmulti`, BIP48 key paths, one multipath `<0;1>/*` descriptor.
2. `openbmt-core` and Bitcoin Core are compared: the descriptor checksum, the
   two single-path forms and the first addresses on both keychains.
3. Addresses at receive indexes 0, 999, 1,000, 4,321 and one high index are
   paid and confirmed.
4. The wallet is restored watch-only in a new Core wallet, four ways, and
   each is timed.
5. The wallet is spent through PSBTs with each pair of keys, then swept.

The high index was 5,000 in three runs, 20,000 in one and 200,000 in one.
All runs were on one laptop that was doing other work at the same time, so
the times are rough. The same step differed between runs by a factor of up
to 1.5, and the slow import by more (see the side experiment below). The
ratios inside one run are the result, not the seconds.

## Results

### The descriptor

Bitcoin Core accepts the multipath descriptor and expands it into the receive
and change descriptors. `openbmt-core` gives the same checksum, the same two
single-path descriptors and the same addresses.

### A default import misses payments

Imported without a range, the descriptor found the payments at indexes 0, 999
and 1,000 and nothing at 4,321 or above: 0.06 of 0.15 BTC. Core derives 1,000
addresses ahead by default. Nothing in the result says that payments are
missing.

The import range must cover the highest index the shop has issued. The kit
therefore names the range, and records the highest index issued
(`docs/DESIGN.md` §5.5).

### With the range, an ordinary import finds everything but is slow

| Highest index paid | Import range | Whole import | Of which rescan |
|---|---|---|---|
| 5,000 | 0 to 6,000 | 126 s, 175 s | 119 s, 163 s |
| 20,000 | 0 to 21,000 | 702 s | 671 s |

The chain in these runs is little more than 100 blocks, so the rescan itself
has almost nothing to read. Core's log for the 20,000 run shows where the
time went.
When the rescan finds a payment at an index, Core marks every address up to
that index as used, one at a time:

| Addresses marked | Time | Per address |
|---|---|---|
| 0 to 999 | 16 s | 16 ms |
| 1,000 to 4,321 | 82 s | 25 ms |
| 4,322 to 20,000 | 572 s | 36 ms |

The cost per address rose with the index. An import with a range of 151,000
was stopped after 15 minutes without finishing. At the rates above it would
have needed more than an hour, and that is an estimate, not a measurement.

### The cause is the address book

A side experiment imported the same receive descriptor three ways, with a
payment at index 3,000 and a range of 0 to 4,000:

| Import | Time | Address-book entries afterwards |
|---|---|---|
| active, not internal (the usual way) | 207 s | 3,001 |
| not active | 6 s | 0 |
| active, internal | 6 s | 0 |

For a descriptor that is active and not internal, Core writes an address-book
entry for every receive address it marks as used. That write is the slow
step. A descriptor that is not active, or is internal, gets no entries.

This run was slower per address (about 69 ms) than the runs above. The
machine was busier, and the comparison that matters is between the three
rows.

### Not active: the fast restore

The receive descriptor is imported in its single-path form with
`"active": false`. The change descriptor is imported as active and internal
with a small range, so the wallet has somewhere to send change when it builds
a transaction.

| Highest index paid | Receive range | Whole import | Of which rescan |
|---|---|---|---|
| 5,000 | 0 to 6,000 | 8 s, 10 s, 12 s | 3 s, 4 s, 5 s |
| 200,000 | 0 to 201,000 | 296 s | 157 s |

Every payment was found in every run. At index 5,000 this is about 15 times
faster than the ordinary import in the same run. The time still grows with
the range, at about 1.5 ms per index here. Core derives three public keys
for each index, and about half of the time at index 200,000 was the rescan,
although the chain is short. Why the rescan grows with the range was not
looked into.

One side effect: a wallet restored this way has no active receive descriptor,
so it cannot hand out a new receive address. A restore only has to find the
coins and move them, so this costs nothing.

### A balance check with no wallet

`scantxoutset` with the two descriptors and the same ranges lists every
unspent payment and names the index of each. It creates no wallet and does no
rescan.

| Highest index paid | Range | Time |
|---|---|---|
| 5,000 | 0 to 6,000 | 9 s, 10 s, 12 s |
| 200,000 | 0 to 201,000 | 295 s |

It reports unspent outputs only. A payment that has been spent since does not
appear.

### Spending

The drill spends through PSBTs. The watch-only wallet builds the transaction,
two signer wallets each sign their own copy, and the node combines the two,
finalizes and broadcasts.

- Each pair of keys spent: 1 and 2, 1 and 3, 2 and 3.
- One key alone never completed a transaction.
- A final transaction swept every coin, including the change from the
  earlier spends, and the wallet's balance was zero afterwards.

Two things about the signers:

- **Core does not read seed words.** It has no BIP39 import. In the drill
  each signer wallet is given its account private key directly. A real
  restore signs in software or hardware that reads the words, and Core's
  part is to find the coins and build the transaction.
- **A Core wallet used as a signer needs the range too.** A signer wallet
  imported with a range of 0 to 1,000 added no signature for the coin at
  index 5,000, and gave no error. With the full range it signed. Importing
  the three signer wallets took 26 s to 32 s at index 5,000 and 680 s
  at index 200,000.

### Other observations

- A node in the middle of a wide import does not act on `stop` or on a
  termination signal until the import is done.
- An import with `"timestamp": "now"` still rescans the last two hours of
  blocks.
- Core refuses an import range wider than 1,000,000.
- `getrawtransaction` does not find a confirmed transaction without a
  transaction index. The wallet's own `gettransaction` does.

## What the design takes from this

`docs/DESIGN.md` §5.4 (the recovery kit) now says:

- the kit gives the two single-path descriptors, each with its range: receive
  up to the highest index issued plus a margin, change with a small range;
- the receive descriptor is imported as not active;
- a signer wallet in Core gets the same receive range;
- `scantxoutset` with the same ranges is the quick balance check;
- Core does not read seed words.

## Limits of this drill

- Regtest only, with a chain of about 130 blocks. On mainnet the rescan
  reads the real chain from the wallet's birthday. A block filter index makes
  that faster, but it was not measured here.
- One laptop under other load, and one run for most figures.
- The keys were plain account keys inside Core wallets. No hardware signer
  and no second wallet program took part.
- The Sparrow restore, which is the path the kit names first, needs a person
  at a screen and has not been run.
- The kit's restore steps are not final until that part is done.
