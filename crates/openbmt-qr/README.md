# openbmt-qr

A QR code encoder and reader (ISO/IEC 18004, model 2) with no dependencies,
for the counter, owner and Key apps. The encoder gives the modules of a
code; drawing them is the app's work. The reader takes the gray levels of a
camera frame (the Y plane of a YUV frame) and gives the bytes of the code in
it; which code is for whom is the caller's to decide from the bytes.

## Rules

A refusal's text starts with its code.

| Code | When |
|---|---|
| QR-01 | The data does not fit. Carries the bits needed and the bits the largest version asked for holds at the asked level. Input over the version-40 capacity is refused from its length, before anything is built. |
| QR-02 | A forced version outside 1 to 40. |
| QR-03 | A forced mask outside 0 to 7. |
| QR-04 | The data has a character the asked mode cannot carry. |

The reader's refusals, from `decode_luma` and `decode_luma_strided`:

| Code | When |
|---|---|
| QR-10 | A side under 16, or a buffer that does not fit: `decode_luma` wants exactly width times height bytes; the strided form wants a stride of at least the width and up to a million, and at least `(height - 1) * stride + width` bytes. |
| QR-11 | A side over 4096. |
| QR-12 | No code found in the frame. |
| QR-13 | A code was found and its format information cannot be read, on the grid as sampled or transposed. |
| QR-14 | More errors than the error correction of a block can repair (more than half its check codewords), or a result that is not a codeword. |
| QR-15 | A segment that is refused: Kanji, structured append, FNC1 in either position, an unassigned mode, or an ECI other than 26 (UTF-8) and 3 (ISO-8859-1). |
| QR-16 | The work budget of one frame ran out: 16 units a pixel and 8 million more, a unit being about one pixel visited. |
| QR-17 | The corrected data is not a well-formed segment stream (a count past the end, a numeric group over 999, an alphanumeric pair over 2024, a designator of four bytes). |

Choices of the reader, where the standard leaves room:

- Thresholding is local: the mean of each 8 by 8 block, smoothed over the 5
  by 5 blocks around it. A block is flat when its light and dark halves
  differ by under 12 in mean, so that noise up to about 7 levels in
  standard deviation does not draw speckles on a plain background; a flat
  light block takes a level half the usual block range under its own.
- Finder patterns are found by 1:1:3:1:1 runs on every row, confirmed down
  the column, along the row again and along a diagonal, and the best three
  that form a right angle are tried, at most six triples.
- The module size is measured along the axes of the code, so a turned code
  gives the right version. The version is the nearest to the distance
  between finders; the two nearest are also tried, and for versions 7 and
  up the version information (both copies, within 3 bits of a valid word)
  decides.
- The grid is sampled through a perspective transform fitted to the three
  finders and every alignment pattern found (each searched for near where
  the transform puts it); version 1 has no alignment pattern and is sampled
  through the parallelogram of the three finders. Each module is the
  majority of five samples.
- Format information is read from both copies, the nearest valid word
  within distance 3 of each; both words are tried if they differ. A code
  that fails is tried once more with its rows and columns swapped, which
  reads a mirrored code.
- `Decoded.bytes` is the data of the segments joined: numeric and
  alphanumeric segments as ASCII, byte segments as written. After an ECI of
  ISO-8859-1 a byte segment is turned into UTF-8, so that the bytes after
  any ECI are UTF-8. No ECI: the bytes as written, which is what this
  encoder writes.
- `corrected_errors` counts codewords changed over all blocks.
- A damaged code with a few more errors than a block can repair is refused
  with QR-14. Reed-Solomon cannot promise this: a block with very few check
  codewords may rarely read as a different, valid codeword.

Choices that are the library's, where the standard leaves room:

- The level asked for is the level used. A smaller code is never traded for
  a stronger level.
- The smallest version that fits is used. One segment per code.
- `encode_text` writes numeric mode for all digits, alphanumeric when every
  character is in the 45-character set, else byte mode (UTF-8, no ECI).
  `?` and `=` are not in that set, so a `BITCOIN:` URI with an amount is
  byte mode; without parameters, in upper case, it is alphanumeric.
- The mask with the lowest penalty wins, ties to the lower number. The
  1:1:3:1:1 rule counts the four light modules only inside the symbol, and
  a dark share that is a multiple of 5 percent is the lower of the two
  multiples around it.
- No panics: no `unwrap`, `expect`, `unsafe` or indexing that can panic.

## Checks

- Unit tests: the standard's worked example (`01234567`, version 1-M, data
  and error-correction codewords), all 32 format strings, the version
  strings of versions 7 to 40, the capacity at every version and level, and
  the four penalty rules on small matrices.
- `scripts/qr-crosscheck.sh`: encodes the same inputs with ZXing and
  compares every module, decodes each with ZXing, and compares mask choice
  and penalties. Every version at every level with all masks, and random
  inputs in each mode from a fixed seed.
- `fuzz/fuzz_targets/qr_encode.rs`: any bytes and level.
- Reader tests (`src/decode_tests.rs`): every version at every level with
  masks, drawn at module sizes 2 to 6 and read back; turns by 90 degrees
  and by other angles, perspective, blur, noise, low contrast, uneven
  light, mirrored and transposed codes (success rates are printed with
  `--nocapture`); errors at every block's capacity corrected and one past
  it refused; refused and malformed segments; frame bounds and stride; a
  4096 by 4096 frame of noise in budget. `qr_10_time_one_frame` (ignored,
  run with `--release --ignored --nocapture`) times a 1280 by 720 frame.
- `scripts/qr-crosscheck.sh` also draws random ZXing codes (digits, text,
  bytes, a UTF-8 ECI; levels, sizes, turns) for the reader, and reads this
  encoder's images with ZXing; where both read, the data must be the same.
- `fuzz/fuzz_targets/qr_decode.rs`: frames of any size and content.

## Use

```rust
use openbmt_qr::decode_luma_strided;

// The Y plane of a camera frame, rows `row_stride` apart.
let decoded = decode_luma_strided(width, height, row_stride, &y_plane)?;
let bytes = decoded.bytes;
```

```rust
use openbmt_qr::{encode_text, Ecc};

let code = encode_text("BITCOIN:BC1Q...", Ecc::M)?;
for y in 0..code.size() {
    for x in 0..code.size() {
        let dark = code.get(x, y);
        // ... draw a module, with a quiet zone of four around the square
    }
}
```
