# openbmt-core

The rules every OpenBMT device must agree on, as plain Rust with no network,
no clock, no files and no async code. Owner mode, Counter mode and the Key app
will all use this crate, so they cannot drift apart on what a valid wallet,
batch, record or setting is.

## What it does

- **Wallet description** (`wallet`). The 2-of-3 P2WSH `sortedmulti` wallet with
  BIP48 `2'` paths, kept in one pinned text form with a BIP380 checksum. It
  rejects wrong paths, the wrong network, repeated keys and repeated
  fingerprints. It gives the four checksum words, the two single-path
  fallback descriptors, and receive and change scripts and addresses.
- **Words** (`words`). Four BIP39 words carry 44 bits. Screens show them after
  a slow stretch (about a million SHA-256 rounds), so finding a second input
  with the same words costs about 2^65 hash compressions. A party that picks
  both compared values can still find a pair in about 2^22 tries, so the words
  are only a human cross-check. Software always compares the full 32-byte
  hashes.
- **Device identity** (`identity`). A separate Schnorr (BIP340) key for each
  device, and tagged signatures, so a signature for a batch can never pass as
  a record or a setting. It also gives four words for showing a pinned root.
  Signing and verifying always go through a tag.
- **Address batches** (`batch`). A counter's contiguous range of receive
  addresses, valid only with the owner and Key app signatures. It includes the
  Key app's check and the counter's `next_script` state, which never hands out
  a script twice and returns nothing when the range is empty. The Key app
  runs its checks in this order and stops at the first failure: right wallet,
  batch shape, the owner's signature (the owner root must differ from the Key
  app's own, and must have signed this batch's hash), known counter, range in
  bounds and exactly at the high-water mark (no gap, no overlap), re-derive
  and compare the hash, store the new mark, and only then co-sign. Because the
  owner's signature comes before any state is touched, only the owner can make
  the Key app use up address indexes.
- **Records** (`records`). Signed records, each carrying the hash of the one
  before. It finds edited, deleted, inserted and reordered records, oversized
  bodies and unknown authors. Checking a period needs every device that was on
  the device list at any time in it. A check-in carries the author's head, so
  a cut-off tail shows up when the chain is compared with a later check-in.
- **Settings** (`settings`). The shop's limits and switches. Tightening needs
  the owner signature. Loosening, or anything mixed, needs the Key app's
  too. Versions only go up, and a store can start from a version floor so an
  old copy cannot be replayed into an empty store. Each check mode has one
  shop-wide budget for outstanding instant payments, and each counter holds a
  share of it in basis points (10,000 = the whole budget). Its slice is
  `budget * share / 10,000`, rounded down; a counter with no share has no
  slice. A new counter or a bigger share is loosening. Settings are validated
  when verified: version at least 1, at most 256 devices, tolerance at most
  500 basis points, at least one confirmation, heartbeat age between 60 and
  3,600 seconds, every counter on the device list, and shares summing to at
  most 10,000. `counter_set` gives the counters as the set the Key app passes
  as its approved counters; a counter may hold a share of 0, which gets
  addresses but no instant acceptance.
- **Instant acceptance** (`policy`). Given one payment and its surroundings,
  the answer is accept now, wait for a confirmation, or instant is off, with a
  reason the screen can put into words. The limits come from the counter's own
  slice. On Wi-Fi that customers can join without Tor, the light client's own
  sighting is not independent, so a node check there is judged with the Box
  limits and label. It also holds the exposure total, the pause after a
  cancel, and the fee-bump-or-cancel test. A replacement is a fee bump only if
  the original paid our script something and the replacement pays our script
  at least as much in total; anything else is a cancel.

- **Wire encodings** (`wire`). Signed batches, records and signed settings
  have one byte form each, and so do the four state snapshots below. Every
  message is a 4-byte ASCII magic, a version byte (currently 1) and the
  payload: `OBMB` signed batch, `OBMR` record, `OBMS` signed settings, `OBMH`
  high-water mark, `OBMC` counter batches, `OBME` exposure, `OBMP` pause
  tracker. The payload of a signed batch, record or signed settings starts
  with the exact bytes that are hashed and signed, followed by the signatures.
  Decoding is strict: wrong magic, an unknown version, missing or trailing
  bytes, a flag other than 0 or 1, a list that is not in strictly ascending
  order, a script that is not 34 bytes, an invalid public key, an unknown
  record kind and any count or length above its limit are all refused, and
  limits are checked before anything is allocated. For all bytes that decode,
  encoding the result gives the same bytes back. **Decoding does not verify.**
  It does not check signatures and does not run `verify` or `validate`; the
  caller must still do that before trusting a value.
- **State snapshots.** `HighWater`, `CounterBatches`, `Exposure` and
  `PauseTracker` can be written out and read back. A restored
  `CounterBatches` hands out the same scripts, in the same order, as the
  state it was saved from. Things the caller always supplies again at
  start-up (the pinned roots, the batch policy, the counter's own id and the
  wallet checksum) are not read from the snapshot: `CounterBatches::from_bytes`
  takes them as arguments, so stored bytes cannot change who is trusted. The
  snapshot does carry the counter and wallet it was made for, and a snapshot
  for another counter or wallet is refused. Invariants the types keep are
  checked again on reading (issued ranges do not overlap, positions are inside
  their batch, exposure ids are ascending and the total fits in 64 bits).
- **Persist before release.** A crash between "state advanced" and "state
  saved" must never let an effect out twice, so the calls that release
  something take a `persist` callback and run it first. `check_and_cosign`
  computes the new high-water mark on a copy, hands it to `persist`, and
  signs only if that succeeds (a mark lost in a crash would let the same range
  be signed for a second counter). `CounterBatches::next_script`, `add` and
  `retire_unused`, and `Exposure::accept_instant` and `add_override`, hand
  `persist` the snapshot bytes of the state as it will be, and change state
  only if `persist` returns `Ok` (an address shown twice would be paid by two
  customers). On a persist error the state is unchanged and nothing is
  released; the error comes back to the caller as `Persist`, apart from a
  refusal. `PauseTracker::record_cancel` also takes `persist`, with the
  opposite rule on failure: a cancel only tightens, so the pause holds in
  memory even when the store fails, and the caller stores the state again
  before it accepts another instant payment.

## What it deliberately does not do

- No networking, peers, light client or node access. It never sees a payment
  arrive; callers tell it what they saw.
- No clock. Callers pass ages and times in seconds.
- No private keys for money. It handles public keys only. Identity keys sign
  messages, and they must never be derived from a wallet key. The caller
  supplies the 32 secret bytes of an identity from a secure random source.
- No storage, no screens, no exchange rates. Dollar amounts come in as a
  sats-per-dollar number from the caller.
- No transaction building or signing, and no PSBT checks (the Key app's
  signing rules live elsewhere).

## Tests

`cargo test -p openbmt-core`. Test keys come from BIP39 test-vector mnemonics
on testnet and regtest. Receive and change scripts from miniscript are checked
against a second, hand-built derivation for the first 50 indexes on both
chains. For every wire type there are round-trip tests, tests that every
truncation fails and that flipping any single bit never panics and never
decodes to something that encodes differently. Fuzz targets for the decoders
are in `fuzz/` at the repository root.
