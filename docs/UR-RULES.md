# UR rules: a PSBT as an animated sequence of QR codes

Status: implemented in `crates/openbmt-ur`. Each rule has an ID so that a
test can name the rule it proves (`ur_NN_...`). The rules implement the
BC-UR transport of `docs/DESIGN.md` §2.3 (the owner phone shows one
animated QR) and §9 (a PSBT too big for one code), in the format Sparrow
and the hardware wallets read and write.

A PSBT is often larger than one QR code holds. BC-UR cuts the message into
fragments and sends them as an endless sequence of parts: the first parts
are the fragments one by one and the later ones are exclusive-or mixes of
several, so that a reader that missed some parts still completes after
about as many parts as the message has fragments, in any order.

The text of a part is `ur:<type>/<body>` for a message that fits one part
and `ur:<type>/<seqNum>-<seqLen>/<body>` for the others. The body is the
bytes in minimal Bytewords (BCR-2020-012: the first and the last letter of
each byte's word, then the CRC-32 of the bytes as four more bytes). For a
multi-part body the bytes are the CBOR array `[seqNum, seqLen, messageLen,
checksum, data]` (BCR-2020-005). The apps write the text in upper case for
the QR alphanumeric mode; both cases are read.

The library has no network, no clock, no storage and no screen. It takes
and gives bytes and text. The app draws the codes and words the progress.

## 1. Bounds

- **UR-01.** A type is 1 to 64 characters of `a-z`, `0-9` and `-`. The
  encoder refuses another type; the decoder refuses a text whose type is
  another.
- **UR-02.** A message is 1 byte to 1 MiB (1,048,576 bytes). The encoder
  refuses an empty or a larger one; the decoder refuses a part that says
  its message has another length, and a whole UR of another length.
- **UR-03.** A sequence length is 1 to 10,000. The encoder refuses a
  message that would need more fragments; the decoder refuses a part that
  says more.
- **UR-04.** A fragment is 10 to 2000 bytes. The encoder's largest and
  smallest fragment lengths are in that range, with the smallest not over
  the largest, and a message that no length between them fits is refused.
  The decoder refuses a multi-part part whose data is outside it.
- **UR-16.** A text is at most 3 + 64 + 1 + 2 × (1 MiB + 20) + 32
  characters, the longest UR within the bounds above. Longer text is
  refused before it is read.

## 2. The text

- **UR-05.** The body is minimal Bytewords: an even number of letters, at
  least five bytes' worth, each pair of letters the first and last letter
  of one word of the 256-word list, in upper or lower case. Anything else,
  including any character that is not an ASCII letter, is refused.
- **UR-06.** The last four bytes of the decoded Bytewords are the CRC-32
  of the bytes before them, big-endian. A body where they differ is
  refused.
- **UR-07.** The text starts with `ur:`, then the type, then either the
  body, or `n-m` and the body, with nothing after. `n` and `m` are plain
  decimal digits with no sign and no leading zero, at most ten of them.
- **UR-08.** A multi-part body is exactly a CBOR array of five items: four
  unsigned integers, in the shortest form, and a byte string of the
  shortest length form whose length is the rest of the body. Indefinite
  lengths, tags, other types, a longer-than-needed head and bytes after the
  last item are refused.
- **UR-09.** The fields of a part must belong to one possible message: the
  sequence number and length in the text equal those in the body, the
  sequence number is at least 1, every integer fits 32 bits, and the
  fragments cover the message without a last fragment that is empty (the
  sequence length times the fragment length is at least the message length,
  and one less is under it).
- **UR-13.** A CBOR byte string (the payload of `bytes`, `crypto-psbt` and
  `psbt`) is a head of major type 2 in the shortest form, then exactly that
  many bytes and nothing more. Anything else is refused.

## 3. Reading a sequence

- **UR-10.** The first part accepted fixes the type, the sequence length,
  the message length, the checksum and the fragment length. A part that
  differs in any of them is refused, and so is a whole UR among multi-part
  ones. When a message is complete, a part of another type is refused.
- **UR-11.** The decoder holds at most twice the sequence length of mixed
  parts that it cannot yet reduce. A mixed part with the same fragments as
  one held is not held again. A part that would be held beyond the bound is
  refused.
- **UR-12.** When every fragment is known the message is cut to its
  length. The bytes cut off must all be zeros and the CRC-32 of the message
  must equal the checksum in the parts. If not, the part is refused and the
  decoder starts again from nothing, as more parts of the same stream
  cannot mend it.
- **UR-18.** A part that is refused changes nothing: the decoder holds what
  it held, and later parts are read as if the refused one had not come.
- **UR-19.** Parts are read in any order, with repeats and with losses.
  A repeat of a sequence number is not counted twice. A whole UR decodes
  at once. A sequence of the first sequence-length parts completes, and so
  does a sequence with 30% of the parts dropped, shuffled and repeated,
  given about twice as many as the message has fragments.
- **UR-17.** A decoder never panics, whatever the text: every length is
  checked before it is used, no index is taken that could be out of range,
  and the work and memory for one text are bounded by the rules above. The
  decoder is fuzzed (`fuzz/fuzz_targets/ur_decode.rs`).

## 4. The fountain code

- **UR-15.** Parts follow BCR-2020-005. The message is cut into fragments
  of one length, `ceil(len / n)` for the smallest `n` that fits the largest
  fragment length (and is at most `len / smallest`), the last padded with
  zeros. Part `k` for `k` up to the sequence length is fragment `k - 1`.
  A later part is the exclusive or of the fragments chosen by Xoshiro256**
  seeded with the SHA-256 of `k` and the checksum (four bytes each,
  big-endian): the degree from the weights `1/d` by the alias method, then
  that many indexes without repeats, each drawn as `floor(double * left)`
  from those left. The checksum is the CRC-32 of the message. A message
  that fits the largest fragment length is one part with no sequence.

## 5. The registry types

- **UR-14.** `crypto-psbt` and `psbt` carry a PSBT as a bare CBOR byte
  string. The encoder writes `crypto-psbt`; the reader takes both. The
  bytes must start with `psbt` and `0xff`, on the way out and on the way in.
  Other types are refused by `ur_to_psbt`.

## 6. The call the apps make

The Android apps reach the library through one framed call
(`crates/openbmt-qr-jni`, `UrNative.call`): draw a text as a QR code, cut a
message into the parts of a UR, read the parts of a UR back, and read a
cosigner key out of a finished UR (section 7).

- **UR-20.** The apps send and read two types. `ur:bytes` carries OpenBMT's
  own framed objects (a request to the Key app, its answer, a pairing
  text). `ur:crypto-psbt` carries a plain PSBT to or from another wallet,
  and nothing else. The call encodes these two types and `ur:crypto-output`
  (the descriptor of a wallet, UR-32, whose CBOR the owner crate makes),
  and gives back the message of a finished UR only when its type is one of
  the first two, `psbt`, or one that UR-31 reads a key from; any other
  type is refused.
- **UR-21.** The call takes one framed request: the four bytes `OBMo`, the
  version byte 1, an operation byte (1 to 7 as above, and 8 to 12 for the
  BBQr sets of `docs/BBQR-RULES.md`: write the parts of a payload, open a
  reader of sets of type P or T, give it a part, take its result, close
  it; a reader's refusal is the rule code BQ-01 to BQ-12 of the part's
  fault, the handles are those of UR-22 and at most four readers are open),
  the fields of the operation and nothing after them, with every length
  exactly the bytes that follow. A
  request of any other shape (another magic, version or operation, a cut,
  an extra byte, a length that claims more or less than is there, a level
  outside 0 to 3, a request over 4 MiB) is refused with this rule. The
  reply is framed too: `OBMp`, the version byte, a status byte, then the
  body, or for a refusal the rule code as five bytes of text. The one
  exception is the key operation of UR-31, whose refusal of the key itself
  has its own status byte and carries words after the code.
- **UR-22.** The call keeps at most four decoders open at a time. A handle
  is given once and never again in the life of the process. A handle that
  is not open, a fifth decoder, and a result asked for before the message
  is complete are refused with this rule. Each decoder holds no more than
  UR-02, UR-03, UR-11 and UR-16 allow.

## 7. A cosigner key from a code

A signer that is not an OpenBMT program shows its key for a shared wallet as
a BC-UR code. `ur_to_key_text` turns it into the one text the key ceremony
takes (`docs/SIGNER-RULES.md`, SR-64), `[<8 hex>/48h/<coin>h/0h/2h]` and
the extended public key, so that nothing after it changes. It reads the old
types (`crypto-account`, `crypto-hdkey`, tags 303 to 311 and 400 to 410),
which are the ones the signers write, and the JSON of a Coldcard-style
export in `bytes`. It takes nothing it cannot rebuild exactly.

- **UR-23.** The CBOR is read in a strict subset: unsigned integers, byte
  strings, text strings that are UTF-8, definite arrays and maps, tags, and
  the values false and true. A map has unsigned integer keys in strictly
  ascending order. Everything else is refused: negative integers, floats,
  null, undefined, indefinite lengths, a head longer than needed, bytes
  after the item. Arrays and maps nest at most 8 deep, at most 8 tags stand
  before one item, and a message holds at most 4096 items, so that what is
  held is bounded by those and not by the length of the message. A message
  over 64 KiB is refused unread.
- **UR-24.** The types read are `crypto-account`, `crypto-hdkey` and
  `bytes`. The newer types (`hdkey`, `account-descriptor`,
  `output-descriptor`) are refused with words that say the code type is not
  read and to show the key as text, and any other type is refused as not a
  key. The tags 303, 304 and 305 and 40303, 40304 and 40305 are the same
  wherever they stand in a key, as signers mix them; 308 and 40308 are not,
  and a 40308 is refused. A key or an account may stand in its own tag
  (303 or 40303, and 311). A layout that is none of these is refused.
- **UR-25.** In a `crypto-account` (a map of the master fingerprint, key 1,
  and the array of outputs, key 2) the outputs that are `wsh` over one key
  are `401(303(key))` and `401(410(303(key)))`, each with or without an
  outer 308. Exactly one of them must have the origin `48h/<coin>h/0h/2h`.
  No such key, and more than one, are each refused with their own words.
  The other outputs (`wpkh`, `sh(wsh)`, `tr`, multisig outputs) are not
  looked into, but one that is not of the old kind is refused.
- **UR-26.** The hdkey is a map of keys 2 to 10. Key 2, if there, is false:
  a key that says it is private is refused and never taken. Key 3 is 33
  bytes of a compressed public key on the curve, and key 4, the chain code,
  is 32 bytes and is required. Key 5, if there, is tagged with
  `{1: 0}` for bitcoin and `{2: 0}` for the main network or `{2: 1}` for a
  test one; without it the network is the main one. Anything else in key 5
  is refused.
- **UR-27.** Key 6 is the origin, tagged, with the path as pairs of an
  index and a hardened flag. The path must be exactly `48h/<coin>h/0h/2h`
  with the coin 0 on the main network and 1 on a test one, and the depth, if
  given, 4. The master fingerprint is key 1 of the account or key 2 of the
  origin, nonzero; at least one must be there, and if both are, they must be
  equal.
- **UR-28.** The key is written as its 78 bytes of BIP 32: the version of
  `xpub` or `tpub`, depth 4, the parent fingerprint (key 8, nonzero and
  required), the child number `2h`, the chain code and the key, in
  Base58Check, after the origin `[<fingerprint>/48h/<coin>h/0h/2h]` in lower
  case.
- **UR-29.** A `bytes` code holds a CBOR byte string of JSON: one object in
  strict JSON, with values read as strings or integers (nested values are
  walked and dropped), no duplicate key at any depth, UTF-8, nothing after
  it. `xfp` is 8 hexadecimal digits, `p2wsh_deriv` is `m/48'/<coin>'/0'/2'`
  (`'` or `h`) and `p2wsh` is an `xpub` or `tpub`, or a `Zpub` or `Vpub`,
  for that coin, at depth 4 with child number `2h`, on the curve. A `Zpub`
  and a `Vpub` are written out as the `xpub` and the `tpub` of the same key
  (only the four version bytes change), as the key ceremony reads them
  (SR-64); the network of the key must be the coin of the path. Every other
  SLIP-0132 public form (`ypub`, `zpub`, `upub`, `vpub`, `Ypub`, `Upub`) is
  refused with words that say to export the plain public key, and a secret
  key of any of the ten secret versions of SLIP-0132 is refused as in UR-26.
- **UR-30.** Every refusal of UR-23 to UR-29 is one value of `KeyRefusal`
  with its rule in its text and words (a heading, what happened, what to do)
  that pass the checks of `docs/SCREEN-RULES.md`, with no two alike. The
  reader never panics, whatever the type and the bytes. It is fuzzed
  (`fuzz/fuzz_targets/ur_key_text.rs`) and checked against Hummingbird
  (`scripts/ur-crosscheck.sh`).
- **UR-31.** The apps reach it through the framed call (section 6) by one
  more operation, which takes a type and the message of a finished UR and
  gives the key text, or the refusal with its rule and its three texts. A
  finished `crypto-account` or `crypto-hdkey` (and the newer types, so that
  they can be refused with words) is given back by the call as its CBOR; a
  `bytes` message is given back as before. A refusal of the key itself has
  status byte 2, then the rule code as five bytes of text, then three texts
  (the heading, what happened, what to do), each as a length of two bytes and
  UTF-8. A request that is not framed, or a type over 64 characters, is
  refused as in UR-21.

## 8. A shared wallet's descriptor as a code

So that a signer can be told the wallet and check the addresses and the
change of a PSBT against it, the library writes the descriptor
`wsh(sortedmulti(2, key, key, key))` as a `crypto-output` (BCR-2020-010),
the form SeedSigner, Krux and Sparrow read.

- **UR-32.** `wsh_sortedmulti_cbor` writes the message of a
  `ur:crypto-output` for `wsh(sortedmulti(k, keys...))` with one to fifteen
  keys and a threshold of one to the number of keys: tag 401 around tag 407
  around a map of the threshold (key 1) and the array of keys (key 2). Each
  key is a `crypto-hdkey` (tag 303) with the keys 3 to 8, in ascending
  order: the key data (33 bytes), the chain code (32), the coin and network
  (tag 305, `{1: 0, 2: 0}` or `{2: 1}` for a test network), the origin (tag
  304: the steps as pairs of an index and a hardened flag, the master
  fingerprint as an integer, the depth), the children (tag 304: one
  unhardened step, then the wildcard as an empty array and a flag) and the
  parent's fingerprint as an integer. Every head is in the shortest form.
  The keys stay in the order given, which for `sortedmulti` is not part of
  the wallet; an origin of none or over sixteen steps, and a threshold or
  count out of range, are refused. `output_to_ur_parts` makes the parts.

- **UR-33.** `text_to_ur_parts` writes a non-empty text as a `ur:bytes`
  whose message is the CBOR byte string of the text's bytes, which is how
  a signer is given a setup file or a descriptor as animated codes. The
  reader of this crate returns the type and the message as it does for any
  type; nothing here reads the text.

## What these rules do not do

- **No check that the PSBT is valid.** Only the five-byte magic is looked
  at. Reading the PSBT and deciding whether to sign is the signer's work
  (`docs/SIGNER-RULES.md`).
- **No secrecy and no authentication.** A UR is plain text in a picture.
  Anyone who sees the codes has the data, and nothing says who showed them.
  Its checksums find a misread, not a forgery.
- **No other registry types.** Only `bytes`, `crypto-psbt` and `psbt` have
  helpers for their payload here, and section 7 reads a cosigner key out of
  `crypto-account`, `crypto-hdkey` and a JSON `bytes`. A `crypto-output` is
  written (section 8) and not read as a key; the newer `hdkey`,
  `account-descriptor` and `output-descriptor`, and the text of BSMS are
  not read as keys. Other types are returned as the bytes they came in.
- **No other key than the first account's.** Only the account 0 key for
  `48h/<coin>h/0h/2h` is taken; a signer that writes another account, a
  `zpub` in JSON or a key without its chain code is refused, with words that
  say what to export instead.
- **No frame counts or timing in the library.** How many parts fit one
  code at what scanning distance, and how fast to show them, are for the
  app (DESIGN.md E8). The apps use fragments of at most 230 bytes, so that
  every part draws as a QR code of version 12 or less at level L, and show
  five parts a second; both are still to be measured on a device.
- **No picture.** The codes are drawn by `openbmt-qr` and read by the
  camera code; this crate sees text only.
- **Not every sender's variants.** A sender that writes a longer-than-needed
  CBOR head, an empty message, a fragment under 10 bytes in a sequence, or
  a sequence over 10,000 parts is refused, as the rules say, even though
  another reader may accept it.
- **No guarantee of completion.** With the parts of one stream the message
  is recovered once enough independent parts have come. A stream where the
  bound of UR-11 stops mixed parts from being held may need more parts, and
  a stream that is cut short never completes.
