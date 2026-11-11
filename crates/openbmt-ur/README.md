# openbmt-ur

Blockchain Commons Uniform Resources (BC-UR) with no dependencies, so that
the apps can move a PSBT that does not fit one QR code as an animated
sequence of codes, in the format Sparrow and hardware wallets read and
write. The library makes and reads the text of each part; drawing it as a
QR code (upper case, for the alphanumeric mode) and reading the camera are
the apps' work, with `openbmt-qr`. It has no dependency, not even another
crate of this workspace.

The rules, with their codes, are in [docs/UR-RULES.md](../../docs/UR-RULES.md).
A refusal's text starts with its code.

## Use

```rust
use openbmt_ur::{psbt_to_ur_parts, ur_to_psbt, Decoder};

// Showing: one part after another, forever; the reader needs about as many
// as the message has fragments, in any order.
let mut encoder = psbt_to_ur_parts(&psbt_bytes, 200)?;
let text = encoder.next_part_upper(); // "UR:CRYPTO-PSBT/1-12/..."

// Reading: each scanned text goes in; the result comes when complete.
let mut decoder = Decoder::new();
let progress = decoder.receive(&scanned_text)?;
if progress.complete {
    if let Some((ur_type, cbor)) = decoder.result() {
        let psbt = ur_to_psbt(&ur_type, &cbor)?;
    }
}
```

| Item | What it is |
|---|---|
| `Encoder::new(type, cbor, max_fragment_len, min_fragment_len)` | The encoder for any type; `Result`, as the bounds are checked |
| `next_part()`, `next_part_upper()` | The next part's text, lower case or upper case |
| `is_single_part()`, `seq_len()` | Whether the message fits one part; the parts that carry it once |
| `Decoder::new()`, `receive(text)` | Reads one part, upper or lower case, giving `Progress { complete, received_parts, expected_parts, estimated_fraction }` |
| `Decoder::result()` | `Some((type, cbor))` once complete |
| `psbt_to_ur_parts(psbt, max_fragment_len)` | An `Encoder` for `crypto-psbt` (checks the `psbt\xff` magic) |
| `ur_to_psbt(type, cbor)` | The PSBT of a `crypto-psbt` or `psbt` UR |
| `cbor::encode_bytes`, `cbor::decode_bytes` | The CBOR byte string of the `bytes` type and of the PSBT types |
| `bytewords::{encode_minimal, decode_minimal}` | Minimal Bytewords with the CRC-32 |

A message of at most `max_fragment_len` bytes is one part,
`ur:<type>/<bytewords>`. A longer one is `ur:<type>/<n>-<m>/<bytewords>`
with an endless `n`: parts 1 to `m` are the fragments in order and the later
ones mix several, so any large enough subset completes it.

## Bounds

| Bound | Value | Rule |
|---|---|---|
| Type | 1 to 64 characters of `a-z`, `0-9`, `-` | UR-01 |
| Message | 1 byte to 1 MiB | UR-02 |
| Sequence length | 1 to 10,000 | UR-03 |
| Fragment length | 10 to 2000 bytes | UR-04 |
| Text | at most the longest UR within the bounds (about 2.1 million characters) | UR-16 |
| Mixed parts held | at most twice the sequence length | UR-11 |

A part that disagrees with the first accepted part on the type, the sequence
length, the message length, the checksum or the fragment length is refused
(UR-10), and a refused part changes nothing (UR-18).

## What was cross-checked

- **SHA-256** against `sha256sum`, and **CRC-32** against Python's
  `zlib.crc32`, on ten inputs from empty to 1079 bytes, including the
  padding edge lengths (55, 56, 64). The values are in the tests; nothing
  runs at run time but this crate's own code.
- **Published BC-UR vectors:** the Bytewords example (the bytes 00 01 02 80
  ff as `able acid also lava zoom jade need echo taxi`, minimal
  `aeadaolazmjendeoti`), the Xoshiro256** outputs for the seed `Wolf` (first
  41 words modulo 100 and first 19 integers in 1 to 10), the fragment
  choices of parts 1 to 30 for a 1024-byte message in 11 fragments (this
  covers the degree chooser, the alias sampler and the shuffle), and the
  first part of the 9-part sequence for 256 bytes from `Wolf`, with the
  first 44 characters of its second part. The remaining parts of that
  sequence are pinned from this code, and are marked so in the tests.
- **Round trips** with 30% of parts dropped, shuffled and repeated, on
  messages from 300 to 20,000 bytes; both cases of text; a decoder fed
  every prefix and every one-character change of valid parts and random
  text without a panic; and a fuzz target (`fuzz/fuzz_targets/ur_decode.rs`).

Not checked against a second implementation of the fountain code at run
time, and not checked against a device: no Sparrow or hardware wallet was
used. Which `crypto-psbt` parts they accept is known from their published
behaviour only.

## Not done

See "What these rules do not do" in the rules file: no PSBT parsing beyond
the magic, no other registry types, no timing or frame counts for the app,
no picture.
