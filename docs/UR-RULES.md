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

## What these rules do not do

- **No check that the PSBT is valid.** Only the five-byte magic is looked
  at. Reading the PSBT and deciding whether to sign is the signer's work
  (`docs/SIGNER-RULES.md`).
- **No secrecy and no authentication.** A UR is plain text in a picture.
  Anyone who sees the codes has the data, and nothing says who showed them.
  Its checksums find a misread, not a forgery.
- **No other registry types.** Only `bytes`, `crypto-psbt` and `psbt` have
  helpers here. The CBOR is a byte-string head and nothing else: a type
  whose payload is a map, a tag or a nested structure is returned as the
  bytes it came in, and is not read.
- **No frame counts or timing.** How many parts fit one code at what
  scanning distance, and how fast to show them, are for the app (DESIGN.md
  E8, still to be measured).
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
