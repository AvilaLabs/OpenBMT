# openbmt-qr

A QR code encoder (ISO/IEC 18004, model 2) with no dependencies, for the
counter, owner and Key apps. It gives the modules of a code; drawing them
is the app's work.

## Rules

A refusal's text starts with its code.

| Code | When |
|---|---|
| QR-01 | The data does not fit. Carries the bits needed and the bits the largest version asked for holds at the asked level. Input over the version-40 capacity is refused from its length, before anything is built. |
| QR-02 | A forced version outside 1 to 40. |
| QR-03 | A forced mask outside 0 to 7. |
| QR-04 | The data has a character the asked mode cannot carry. |

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
  compares every module, decodes ours with ZXing, and compares mask choice
  and penalties. Every version at every level with all masks, and random
  inputs in each mode from a fixed seed.
- `fuzz/fuzz_targets/qr_encode.rs`: any bytes and level.

## Use

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
