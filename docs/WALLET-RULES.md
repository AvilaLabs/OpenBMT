# Wallet rules

The wallet descriptions OpenBMT reads: the single-key account of an owner
who already holds their own keys, and the check on every text the owner
scans or opens before the app goes on. The 2-of-3 description of
`crates/openbmt-core/src/wallet.rs` is read as it always was. The code is
`crates/openbmt-core/src/wallet_single.rs`; the words are in
`crates/openbmt-core/src/say/import.rs`. Tests are named after the rule
(`wl_01_...`). This file is the first slice of "Use my own wallet"
(`docs/DESIGN.md` §2.1): the types and the checks. No screen, spend or
counter uses them yet.

## 1. The single-key description

- **WL-01. One form.** The description is exactly
  `wpkh([fp/84h/0h/Nh]xpub/<0;1>/*)#checksum` on mainnet. Its canonical
  string writes the hardened marker as `h` and the fingerprint in lower
  case; `'` is accepted on input and written back as `h`. `checksum32` is
  the SHA-256 of the canonical string, `#checksum` included, as for the
  2-of-3 wallet.
- **WL-02. The origin and the key.** The origin is required: eight hex
  digits, then `84h`, `0h` and an account `Nh` with N from 0 to 2^31-1 and
  no sign or leading zero. The key is an `xpub` (a `zpub`, `ypub` or
  `tpub` is refused) of depth 3 whose child number is N, hardened.
  Anything else is refused as not accepted.
- **WL-03. The branches.** The key is followed by `/<0;1>/*`. The two
  single-path descriptions (`/0/*` and `/1/*`) are also accepted when
  given together, separated by white space and in either order, and must
  name the same key and origin; they read as the one canonical multipath
  description. One of them alone, two of the same branch, or two that
  name different keys are refused.
- **WL-04. The checksum.** The BIP380 checksum is required and checked
  (the check of the 2-of-3 wallet is reused). Text with no checksum, a
  wrong one, or a character no description has is refused.
- **WL-05. Scripts and addresses.** For an index below 2^31 the wallet
  gives the receive and change script (P2WPKH, 22 bytes) and address, the
  first receive address, and the two single-path strings with their own
  checksums. They are the same as those a descriptor library derives from
  the canonical string and from the single-path strings, for every index
  tried. A hardened index is refused.
- **WL-06. Known answers.** The Key Maker's test seed (fingerprint
  `818bc9b6`) gives the first address
  `bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj`, and each of the 20 seeds
  whose first address was computed by Electrum
  (`testdata/keymaker-electrum-expected.txt`) gives the address Electrum
  gave, from its fingerprint and account key.

## 2. What the owner brings in

- **WL-07. One entry point.** `ImportedWallet::read(text)` returns the
  single-key wallet or the 2-of-3 wallet (read by its own strict parser,
  unchanged, on mainnet), or a refusal. It never panics.
- **WL-08. A private key.** Text that holds an extended private key
  (`xprv`, `tprv` and the other versions), a key in wallet import format,
  or 12 to 24 English seed words (or a count a seed has of lower-case
  words of 3 to 8 letters) is refused as holding a private key. It is
  refused whatever else is wrong with it.
- **WL-09. The text is not repeated.** A refusal, in `Display`, in
  `Debug` and in its words, holds nothing of the text.
- **WL-10. Damaged.** A checksum that is missing or wrong, on any of the
  descriptions given, is refused as damaged.
- **WL-11. A test network.** A `tpub`, `upub` or `vpub`, an address that
  starts `tb1` or `bcrt1`, or a coin type of `1h` in an origin is refused
  as a test network.
- **WL-12. Another type.** Everything else is refused as a type that is
  not supported: taproot, `sh(wpkh(`, a key with no origin, a `zpub`, a
  threshold or key count not built, a miniscript, one branch alone.
- **WL-13. The order.** The checks run in this order: a private key, a
  test network, the checksum, the type. The first that applies decides.
- **WL-14. The size.** Text of more than 4096 bytes is refused as not
  supported before any of it is read.
- **WL-15. The words.** Each refusal has the words of
  `docs/screens/core.md`, under SC-01 to SC-12, for the owner: this holds
  a private key, this description is damaged, this is a test-network
  wallet, this wallet type is not supported.
