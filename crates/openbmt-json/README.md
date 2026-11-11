# openbmt-json

A strict, small JSON reader, with no dependencies. It reads a whole document
into a tree so that a malformed one is refused, and the code that follows
looks up the few fields it wants.

- Bounded before and while it reads: the bytes (`Limits::max_bytes`), the
  depth of arrays and objects inside one another and the number of values.
  `Limits::DEFAULT` is 8 MiB, 64 levels and 400,000 values; the price reader
  of `openbmt-rates` asks for 64 KiB and 16 levels.
- UTF-8 throughout, the grammar of RFC 8259, no key twice in one object, no
  bytes after the document, surrogate escapes in pairs.
- A number with no fraction and no exponent that fits is kept exactly, as an
  `i64` or (above `i64::MAX`) a `u64`. Any other number that has the grammar
  of a number (a fraction, an exponent, a value too large, `-0`) is kept as
  `Value::Number`, without its value, and `as_i64` and `as_u64` refuse it.
  Nothing is rounded.

It is the reader of the price replies (`docs/RATE-RULES.md`, RT-10 to RT-14)
and of the replies of a POS (`docs/POS-RULES.md`, PS-90). The fuzz targets
of both (`rates_parse`, `pos_square_parse`, `pos_clover_parse`) run it.
