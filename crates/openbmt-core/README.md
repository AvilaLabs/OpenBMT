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
  Key app's check (right wallet, shape, the owner's signature, known counter,
  no gap or overlap, re-derive and compare, store the mark, then co-sign) and the
  counter's `next_address` state, which never hands out a script twice and
  returns nothing when the range is empty.
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
  when verified: tolerance at most 500 basis points, at least one
  confirmation, heartbeat age between 60 and 3,600 seconds, every counter on
  the device list, and shares summing to at most 10,000.
- **Instant acceptance** (`policy`). Given one payment and its surroundings,
  the answer is accept now, wait for a confirmation, or instant is off, with a
  reason the screen can put into words. The limits come from the counter's own
  slice. On Wi-Fi that customers can join without Tor, the light client's own
  sighting is not independent, so a node check there is judged with the Box
  limits and label. It also holds the exposure total, the pause after a
  cancel, and the fee-bump-or-cancel test. A replacement is a fee bump only if
  the original paid our script something and the replacement pays our script
  at least as much in total; anything else is a cancel.

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
chains.
