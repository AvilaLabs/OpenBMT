# BBQr rules: a file as a sequence of QR codes for a hardware signer

Status: implemented in `crates/openbmt-bbqr`. Each rule has an ID so that a
test can name the rule it proves (`bq_NN_...`). The rules implement the
BBQr transport of `docs/DESIGN.md` §2.7 and §5.4 (the owner phone shows a
PSBT to a hardware signer that has a camera and reads the signed one back).

BBQr is Coinkite's format, in `BBQr.md` of the repository
<https://github.com/coinkite/BBQr> (public domain). A file is cut into
parts, and each part is one QR code in alphanumeric mode at the lowest
error correction. The text of a part is an 8-character header and then
the data characters:

```text
B$        fixed
2         encoding: H hex, 2 base32, Z deflate then base32
P         file type: P PSBT, T transaction, J JSON, C CBOR, U text, B, X
05        parts in the set, two base-36 digits
00        this part, from 0, two base-36 digits
```

The library writes base32 (encoding `2`) and reads all three encodings. It
has no network, no clock, no storage and no screen. It takes and gives
bytes and text. The app draws the codes and words the progress.

## 1. The text of a part

- **BQ-01.** A part is at most 4296 characters, all ASCII, and starts with
  the eight characters `B$`, an encoding, a file type, two base-36 digits
  for the number of parts and two for this part's number. Letters may be
  in either case on the way in (a person may paste lower case); the writer
  gives upper case only. Anything else is refused.
- **BQ-02.** The encodings read are `H`, `2` and `Z`. Another character in
  that place is refused. The writer writes `2` only.
- **BQ-03.** The file types known are `P`, `T`, `J`, `C`, `U`, `B` and `X`.
  A reader is made with the types it takes, and a part of another type is
  refused, known or not.
- **BQ-04.** The number of parts is 1 to 1295 (`ZZ`) and the part's number
  is below it.
- **BQ-05.** A joined file is 1 byte to 2 MiB (2,097,152 bytes), before and
  after unpacking. A part has data in it. The writer takes a file of that
  size and a part length of 16 to 4296 characters, and refuses others.

## 2. A set of parts

- **BQ-06.** The first part fixes the encoding, the file type and the
  number of parts. A part that differs in any of them is refused. A part
  with the number of one held must have the same data, and is then not
  counted twice; with other data it is refused. A part that is refused
  changes nothing.
- **BQ-07.** Every part but the last has the same number of data
  characters, and the last has no more. With base32 every part but the last
  is a whole number of groups of eight characters, and with hex an even
  number of digits, so each part is whole bytes. A part that is not is
  refused.
- **BQ-08.** Hex is the digits 0-9 and A-F in an even number, and nothing
  else.
- **BQ-09.** Base32 is RFC 4648 section 6 (`A-Z` and `2-7`) with no
  padding. A part's last group is 2, 4, 5 or 7 characters or none (whole
  bytes), and the bits after the last byte are zero, so that one text is
  one file. A `=`, a character outside the alphabet and a group of another
  length are refused.
- **BQ-10.** The parts are taken in any order, with repeats. The file is
  given only when every part is held; before that, the answer says how many
  of how many. Nothing is consumed by asking: a refusal leaves the parts
  as they were.

## 3. Unpacking

- **BQ-11.** Encoding `Z` is the bytes of base32 parts joined, then
  deflate (RFC 1951) with no header, as zlib writes it at a window of 10
  bits. The reader is strict: a block type that does not exist, a code that
  is over-subscribed or incomplete (except a single distance code of one
  bit, or none), a length or distance symbol that does not exist, a
  distance before the start of the output, a stored block whose length and
  its complement differ, a repeat of code lengths with nothing before it or
  past the last, no code for the end of a block, bits that pad the last or a
  stored block's byte that are not zero, bytes after the last block, and a
  stream that ends early are all refused. If the raw reading fails and the
  stream starts with a valid zlib header (RFC 1950), it is read again with
  that header and its Adler-32 checksum, which must match. A window of up to
  32 KiB is accepted, though the format asks for 1 KiB.
- **BQ-12.** The output of unpacking is at most 2 MiB (BQ-05), checked
  before each byte or copy is written, so that a small input cannot make a
  large one; the work is bounded by the input and that output.

## 4. Writing

- **BQ-13.** The writer cuts the base32 text of a file into the fewest parts
  of at most the length asked, with every part but the last of one length (a
  whole number of groups of eight characters) and the last no longer. A
  file that fits one part is one part with its header. It reads back to the
  same bytes (BQ-07, BQ-09), in any order.
- **BQ-14.** Parts written by this library are read by the reference
  implementation in Coinkite's repository, and parts written by the
  reference (with encodings `2`, `H` and `Z`) are read here.
  `scripts/bbqr-crosscheck.sh` runs both ways on the same inputs.

## 5. Safety and words

- **BQ-15.** The reader never panics, whatever the text or the bytes: every
  length is checked before it is used, no index is taken that could be out
  of range, and the work and memory for one text are bounded by the rules
  above. The reader and the inflate are fuzzed
  (`fuzz/fuzz_targets/bbqr_join.rs`, `fuzz/fuzz_targets/bbqr_inflate.rs`).
- **BQ-16.** Every refusal has words that pass the checks of
  `docs/SCREEN-RULES.md`, and its text starts with its rule's code. The
  rule codes go in the small print, never in the sentences.
