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
- **WL-04. The checksum.** The strict single-key parser and the 2-of-3 wallet
  require and check the BIP380 checksum. The import boundary also accepts the
  checksumless BlueWallet account form in WL-16 and the two branch forms in
  WL-17; any checksum present on those forms is checked. A bad checksum or a
  character no description has is refused.
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
- **WL-10. Damaged.** A required checksum that is missing or wrong, or a
  checksum supplied on an otherwise supported checksum-optional import form
  that does not match, is refused as damaged.
- **WL-11. A test network.** A `tpub`, `upub` or `vpub`, an address that
  starts `tb1` or `bcrt1`, or a coin type of `1h` in an origin is refused
  as a test network.
- **WL-12. Another type.** Everything outside the forms in WL-01, WL-03,
  WL-16, WL-17 and WL-18 is refused as a type that is not supported: taproot,
  `sh(wpkh(`, a key with no origin, a `ypub` or `zpub`, a threshold or key
  count not built, a miniscript, one branch alone, or an ambiguous Core export.
- **WL-13. The order.** The checks run in this order: a private key, a
  test network, the checksum, the type. For Core JSON, private-key and
  test-network checks also inspect every decoded JSON string before filtering
  entries. The first applicable check decides.
- **WL-14. The size.** Text of more than 4096 bytes is refused as not
  supported before any of it is read.
- **WL-15. The words.** Each refusal has the words of
  `docs/screens/core.md`, under SC-01 to SC-12, for the owner: this holds
  a private key, this description is damaged, this is a test-network
  wallet, this wallet type is not supported.
- **WL-16. BlueWallet account export.** The import boundary accepts
  `wpkh([fp/84'/0'/0']xpub)` without a checksum or child suffix. It expands
  this account-0 public key to the canonical receive/change multipath form.
  The strict `SingleKeyWallet::parse` parser continues to require the
  checksum and both branches.
- **WL-17. Paired branch export.** The import boundary accepts the receive
  `/0/*` and change `/1/*` descriptors together, in either order and with
  whitespace between them. A checksum may be absent on either descriptor;
  when present it must be valid. Both descriptors must have the same master
  fingerprint, BIP84 account origin and xpub. A lone branch remains unsupported.
- **WL-18. Bitcoin Core export.** The import boundary accepts a JSON object
  shaped like Bitcoin Core 32.1 `listdescriptors` output. It selects exactly
  one active account-0 BIP84 receive descriptor marked `internal:false` and
  one active account-0 BIP84 change descriptor marked `internal:true`, with
  matching origin and xpub. Descriptors have valid checksums. Inactive and
  unsupported descriptors do not become candidates. Missing, mismatched,
  duplicated or ambiguous candidates are refused.
- **WL-19. Strict JSON.** Core export JSON is at most 4096 UTF-8 bytes, has
  valid JSON strings and surrogate pairs, unique object keys, nesting depth
  at most 16, and no trailing non-whitespace data. Private-key and
  test-network checks inspect all decoded strings, including metadata and
  inactive entries, before descriptor selection.

## 3. The wallet a counter pins, and its index ranges

The code is `crates/openbmt-core/src/pinned_wallet.rs`. A counter paired the
new way derives its own receive addresses from the pinned description, for
both kinds of wallet. The owner phone still gives it a range of indexes.
No counter, owner screen or app uses these types yet.

- **WL-20. The counter's own addresses.** `PinnedWallet` gives the receive
  script and address at an index for the single-key wallet and for the
  2-of-3 wallet. They are those of `SingleKeyWallet` and of
  `WalletDescription` for the indexes 0, 1 and 999; the test seed's first
  address is the known one of WL-06. An index of 2^31 or more has no
  address.
- **WL-21. The pinned wallet on the wire.** `OBM)`, version 1, the text
  length, the canonical description text, its SHA-256. A message of another
  magic or version, a length above the import limit, a cut or a longer
  message is refused, and what decodes encodes to the same bytes.
- **WL-22. Nothing the import refuses.** Decoding reads the text through
  `ImportedWallet::read` and refuses a private key, a test network, a
  damaged checksum and another type with the same refusal. The text must be
  the canonical spelling and the hash must be its own.
- **WL-23. The shape of a range.** A range `start..end` of one counter is
  not empty, has `end` below 2^31 and holds at most 1,000,000 indexes. A
  range of another shape is not signed and does not decode.
- **WL-24. The owner's signature.** `SignedRange` carries the owner root's
  signature over the counter, the start and the end, under its own tag.
  `verify` accepts only the pinned owner root, and a changed counter, start
  or end, or a signature under another tag, fails. On the wire: `OBM*`,
  version 1, counter, start, end, signature; the decoder is strict.
- **WL-25. Used indexes.** A `RangeBook` refuses a range that overlaps an
  index it has used, in a range that is used up or in the used part of one
  that is not. The book is unchanged by a refusal.
- **WL-26. A range still in use.** A `RangeBook` refuses a range that
  overlaps a range already taken that still has an unused index, with a
  refusal of its own. A range next to it, or over one that is used up, is
  taken.
- **WL-27. The next index.** `next` hands out the unused indexes of the
  ranges in the order they were taken, each once, and says when none is
  left (or none was taken).
- **WL-28. Another counter.** A `RangeBook` refuses a range for another
  counter, and `accept_signed` checks the signature before it takes a
  range.
