# Key maker rules: coin flips to a wallet, offline

Status: implemented in `tools/keymaker/src/core.js` (the core only; the page,
its words and the QR code come later). Each rule has an ID so that a test can
name the rule it proves (`km_NN_...`, in `tools/keymaker/test/`, run with
`npm test` inside `tools/keymaker/`). The rules serve the key maker design:
the owner makes the wallet's words from their own coin flips, offline, and
Electrum must show the same wallet before the shop goes live.

The core turns flips into words and words into the public description of a
wallet. It has no screen, no storage and no network, and it has no source of
chance: the only bits that enter are the flips the owner passes in.

**First build. No independent security audit has been done. Not for real
funds.**

## 1. What is fixed

- **KM-01. The fixed choices.** There are 128 flips, 12 BIP39 English words,
  no passphrase, native SegWit with a single key (BIP84), the account
  `m/84'/0'/0'` and real bitcoin only. Nothing chooses between options.
- **KM-02. The order of the flips.** A flip is 1 for heads and 0 for tails.
  The first flip is the most significant bit of the first byte, and the
  flips fill 16 bytes of entropy in that order.
- **KM-03. No source of chance.** Nothing in `src/` asks for random bytes,
  reads the clock or calls the platform's crypto object. The wallet is a
  function of the flips alone.
- **KM-04. Wrong flips are refused.** Anything other than exactly 128 values,
  each the number 1 or the number 0, is refused with `KM-04`, as is entropy
  that is not 16 bytes. No value is converted or padded.
- **KM-05. No network.** Nothing in `src/` fetches, opens a socket, loads a
  module from a URL or by a computed name, or names a web address.

## 2. What comes out

- **KM-06. The outputs.** From the 12 words the core gives the master
  fingerprint (8 hex characters), the account key at `m/84'/0'/0'` as a plain
  `xpub`, the first receive address at `m/84'/0'/0'/0/0` as a `bc1q` address,
  and three descriptors: `wpkh([fp/84h/0h/0h]xpub/<0;1>/*)` and the two
  single-path forms ending `/0/*` and `/1/*`, each with its checksum.
- **KM-07. The working.** For "Check it by hand" the core gives eleven groups
  of 11 flips with each group's number and word, then the last 7 flips with
  the 4 check bits (the first 4 bits of the SHA-256 of the entropy), their
  number and their word. The twelve words so found equal the wallet's words,
  or the working is refused with `KM-07`.
- **KM-08. The checksum on every descriptor.** Each descriptor ends in `#` and
  the 8-character BIP380 checksum of the text before it, and passes the
  BIP380 check.
- **KM-09. Characters outside the descriptor set.** The checksum is written
  from the BIP380 text. A character outside its input set is refused with
  `KM-09`.

## 3. What it is checked against

- **KM-10. BIP380.** The checksum gives `89f8spxm` for `raw(deadbeef)`, and
  the published bad cases (no checksum, an empty, a longer or a shorter one,
  an error in the payload or in the checksum, a character outside the set)
  all fail the check.
- **KM-11. BIP39.** Every English vector of the trezor `vectors.json`
  (24 of them) gives its words from its entropy, its seed with the passphrase
  `TREZOR`, and its root key.
- **KM-12. BIP32.** Test vectors 1 and 2 give every published key, public and
  private, along their chains.
- **KM-13. BIP84.** The BIP84 mnemonic gives the published account key (as a
  plain `xpub`), root key and first receive address.
- **KM-14. Electrum.** For 20 fixed flip strings (`test/electrum/flips.txt`),
  the words, fingerprint, `xpub` and first receive address equal what the
  library of Electrum 4.8.4 prints (`test/electrum/expected.txt`).
  `test/electrum/crosscheck.sh` runs that Electrum again, without a network,
  and compares. The test seed (the flips `10` repeated 64 times) gives
  `primary fetch primary fetch primary fetch primary fetch primary fetch
  primary fever`, fingerprint `818bc9b6` and the first address
  `bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj`.

## 4. Libraries

The libraries are `@noble/hashes`, `@noble/curves`, `@scure/base`,
`@scure/bip39` and `@scure/bip32`, each pinned to an exact version in
`tools/keymaker/package.json`, with the integrity hashes in
`package-lock.json`. The code of the core outside those libraries is the
checksum, the flip packing, the address encoding and the working.
