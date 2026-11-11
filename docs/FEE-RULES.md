# Fee rules: a fee rate the device checks by itself

Status: specification for `crates/openbmt-watch/src/fees.rs`, which
implements it. Each rule has an ID so that a test can name the rule it
proves. The rules serve the line about receipts in `docs/DESIGN.md` §9
("What the owner is shown about receipts"): the fee of the last blocks
comes from blocks the owner's own device has checked, never from a service.
The owner device takes the samples as FE-20 says (`crates/openbmt-owner-node`,
`receipts.rs`), and the line and the offer that use them are in
`docs/OWNER-COINS-RULES.md`, OC-16 to OC-21.

The idea. The transactions of a block do not give their fees, because an
input's value is in an earlier transaction that the device does not hold.
The coinbase transaction, though, claims the block's subsidy plus every
fee in the block, and the subsidy is fixed by the height. So for a block
whose transactions were checked against its header, the fees of the block
are what its coinbase claims above the subsidy, and the size of the block
is known. That gives the average fee rate of the block with no outside
help.

The library has no network, no clock and no storage. Everything is counted
in blocks, by height, never in time. The caller passes in blocks and keeps
the stored form.

## 1. One sample

- **FE-01.** A sample is made from one block, its height and its network.
  The caller passes a block that has already passed the watcher's full
  check (`WR-20` to `WR-22`, which also checks the header against the
  chain). The sample function does not check the header and does not
  know the chain; it trusts the height it is given. It holds the
  height, the block's hash, the fees, the block's size in virtual bytes,
  whether the coinbase claimed less than the subsidy, and the average
  fee rate when there is one.
- **FE-02.** The sample function checks again, on its own, two things
  that the fee sum depends on: the transactions hash to the header's
  merkle root, and the first transaction is a coinbase. A block that
  fails either is refused and no sample is made.
- **FE-03.** The subsidy is 50 bitcoin in satoshis, shifted right once for
  each completed halving interval, and zero after 64 halvings. The
  interval is 210,000 blocks on mainnet, testnet, testnet4 and signet,
  and 150 blocks on regtest.
- **FE-04.** The fees are the sum of the coinbase's outputs minus the
  subsidy. A sum that does not fit in 64 bits is refused. A coinbase that
  claims less than the subsidy is allowed, since a miner may leave
  money unclaimed: the fees are then 0 and the sample says so
  (`claimed_less`).
- **FE-05.** The size is the block's weight divided by 4, rounded up. The
  rate is the fees in thousandths of a satoshi per virtual byte
  (`rate_milli`): fees times 4,000, divided by the summed weight of the
  block's transactions other than the coinbase, rounded down. The
  coinbase, the header and the transaction count are left out because
  they pay no fee. This is the measure of Bitcoin Core's `avgfeerate`
  (`getblockstats`), in thousandths.
- **FE-06.** A block with only a coinbase has no rate: it holds no
  information about fees. It is still a sample, with fees and size.

## 2. A history

- **FE-10.** A history holds at most 512 samples, in ascending order of
  height, at most one for each height. When it is full and a sample is
  added above the lowest, the lowest is dropped. A sample below the lowest
  of a full history is not kept.
- **FE-11.** A second sample for a height held already is kept only if its
  block hash differs from the first, which means the chain was
  reorganized and the old block is gone. It then replaces the first. The
  same block again changes nothing.
- **FE-12.** The stored form is the magic `OBMj`, version 1, a 4-byte
  little-endian count (at most 512), then that many samples of: height
  (u32), block hash (32 bytes), fees (u64), size (u32, at most 1,000,000),
  a flag for `claimed_less` (0 or 1), then a flag and the rate (u64) when
  the flag is 1. All numbers are little-endian. Reading is strict, as in
  `openbmt_core::wire`: exact length, heights strictly ascending, and the
  bytes read must be what writing the value would give. A sample that
  claims less than the subsidy must hold fees 0.
- **FE-13.** `recent_rate(tip)` is the median `rate_milli` of the samples
  whose height is in the last 144 blocks, from `tip - 143` to `tip`. With
  an even count the lower of the two middle values is taken. It is none
  when fewer than 3 such samples have a rate.
- **FE-14.** `is_low(tip)` says whether `recent_rate(tip)` is at or below
  the 25th percentile of the rates of the samples in the last 4,032
  blocks (from `tip - 4031` to `tip`). The percentile is by nearest rank:
  with n rates in order, the one at place ceil(n / 4). It is none, which
  means no offer is made, when fewer than 20 samples in that window have
  a rate or when `recent_rate` is none.
- **FE-15.** `forget_from(height)` drops every sample at or above the
  height. The caller uses it when the chain is reorganized past that
  height.
- **FE-16.** A history is changed only by these methods, and a decoded
  history is checked as a whole before it replaces anything.
- **FE-17.** `usual_rate(tip)` is the median `rate_milli` of the samples
  whose height is in the last 4,032 blocks, the window of FE-14, with the
  lower of the two middle values for an even count. It is none when fewer
  than 20 such samples have a rate. It is the "usual fee" that the owner's
  offer to merge compares with (OC-19).

## 3. Taking samples

- **FE-20.** The owner's device takes a sample from every block it
  already downloads in full. When it has downloaded none in the last 24
  blocks, it fetches one more full block. At about 1.5 MB a block that is
  about 9 MB a day. With one sample every 24 blocks, the windows of FE-13
  and FE-14 hold about 6 and 168 samples. **Built** in
  `crates/openbmt-owner-node/src/receipts.rs`, as follows.
  - A sample is taken from a block that the driver hands over as one the
    coin view reads and that the view has applied (so the watcher's checks,
    WR-20 to WR-22, have passed and the block is on the best chain), and
    from a block that the driver hands over as asked for by hash
    (`docs/NETWORK-RULES.md`, NR-47), which the node checks on its own the
    same way. The height is the block's place on the best chain. A block
    that gives no sample is told as a note (FE-30), and stops nothing.
  - The history is kept in `fee-history.bin` in the node's directory, in the
    stored form of FE-12. It is written whole to a temporary file, flushed
    and renamed, and the node's copy changes only after that has worked:
    when the file cannot be written the history is as it was, and a note
    says so. It is written only when a sample changed it.
  - At the start the file is read strictly (FE-12). A file that is missing
    gives an empty history. A file that is cut, changed, too large (over
    64 KiB) or unreadable gives an empty history and a note, and is not a
    stop; the next sample replaces it. Samples of blocks that the stored
    chain no longer holds at their height are dropped at the start, with
    a note.
  - After a checked block, when the newest checked block is within the
    4,032 blocks that FE-14 reads, no one is outstanding, and the node holds
    no sample in the last 24 checked blocks (an empty history holds none),
    the node asks the driver for that block by hash. At most one request
    is outstanding. It ends when the block arrives, when the block leaves
    the chain, or when 24 more blocks have been checked without it, and
    then the newest checked block is asked for in its place. A block more
    than 4,032 below the tip is never asked for: no window reads it.
  - A reorganization calls `forget_from` with the height above the fork, and
    the history is stored again.

## 4. What the owner is told

- **FE-30.** Every refusal of the sample function has words under
  `docs/SCREEN-RULES.md` (`FeeError::words`), listed in `all_words`. The
  refusals are meant for the owner's notes. They do not stop a payment.

## 5. What these rules do not say

- **They do not give the fee needed for the next block.** An average fee
  rate is not the rate at which a transaction gets into the next block.
  It overstates that rate in a calm block that holds a few transactions
  with high fees, and it lags when fees rise fast.
- **They do not stop a miner from bending the number.** A miner can put
  its own transactions in a block with a high fee, which only moves its
  own money, or claim less than it could. One block proves little; the
  median over many blocks and the share rule of FE-14 limit the harm, and
  do not remove it.
- **They do not check the block in full.** The scripts and the coins of
  the transactions are not checked, as in `docs/WATCHER-RULES.md` §7. The
  fees are what the coinbase says, and the proof of work on the header is
  what makes a false one expensive.
- **They do not set a fee.** Nothing here chooses the fee of a transaction
  the owner builds. The screens that use these numbers say "about", and
  offer to merge receipts only when `is_low` says so.
- **They do not know the clock.** The windows are counted in blocks. A
  slow stretch of the chain spans more days than a fast one.
- **They do not fetch anything.** The functions of this library fetch
  nothing. Which blocks are downloaded, and when, is FE-20, built in the
  owner node.
