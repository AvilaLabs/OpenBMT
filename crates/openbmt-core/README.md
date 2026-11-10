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
- **Device identity** (`identity`). A separate Schnorr (BIP340) key for each
  device, and tagged signatures, so a signature for a batch can never pass as
  a record or a setting. It also gives four words for showing a pinned root.
- **Address batches** (`batch`). A counter's contiguous range of receive
  addresses, valid only with the owner and Key app signatures. It includes the
  Key app's check (re-derive, compare, no gap or overlap, co-sign) and the
  counter's `next_address` state, which never hands out a script twice and
  returns nothing when the range is empty.
- **Records** (`records`). Signed records, each carrying the hash of the one
  before. It finds edited, deleted, inserted and reordered records and unknown
  authors. A check-in carries the author's head, so a cut-off tail shows up
  when the chain is compared with a later check-in.
- **Settings** (`settings`). The shop's limits and switches. Tightening needs
  the owner signature. Loosening, or anything mixed, needs the Key app's
  too. Versions only go up.
- **Instant acceptance** (`policy`). Given one payment and its surroundings,
  the answer is accept now, wait for a confirmation, or instant is off, with a
  reason the screen can put into words. It also holds the exposure total, the
  pause after a cancel, and the fee-bump-or-cancel test.

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
- No verification of the owner's signature inside the Key app's batch check;
  the counter does that when it takes the batch.

## Tests

`cargo test -p openbmt-core`. Test keys come from BIP39 test-vector mnemonics
on testnet and regtest. Receive and change scripts from miniscript are checked
against a second, hand-built derivation for the first 50 indexes on both
chains.
