//! A strict inflate (RFC 1951, with the RFC 1950 wrapper as a fallback) for
//! the `Z` encoding of BBQr (BQ-11, BQ-12).
//!
//! The format says the data is compressed with zlib at a window of 10 bits
//! and no header, which is a raw deflate stream. This reader takes that, and
//! a stream that has a zlib header and checksum around it if the raw
//! reading fails. It refuses anything malformed rather than guessing:
//! incomplete or over-subscribed codes, a distance past the start, a stored
//! block whose length check fails, bits after the end that are not zero,
//! bytes after the end, and any output past the bound.

use crate::error::BbqrError;

const MAX_BITS: usize = 15;
const FIXED_LIT: usize = 288;
const MAX_LIT_CODES: usize = 286;
const MAX_DIST_CODES: usize = 30;

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn bad(why: &'static str) -> BbqrError {
    BbqrError::Inflate(why)
}

/// Bits of the input, least significant first.
struct Bits<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte: 0,
            bit: 0,
        }
    }

    fn one(&mut self) -> Result<u32, BbqrError> {
        let b = *self.data.get(self.byte).ok_or(bad("the data ends early"))?;
        let v = (u32::from(b) >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Ok(v)
    }

    fn take(&mut self, n: u32) -> Result<u32, BbqrError> {
        let mut v = 0;
        for i in 0..n {
            v |= self.one()? << i;
        }
        Ok(v)
    }

    /// Move to the next byte; the bits skipped must be zero.
    fn align(&mut self) -> Result<(), BbqrError> {
        if self.bit != 0 {
            let b = *self.data.get(self.byte).ok_or(bad("the data ends early"))?;
            if (u32::from(b) >> self.bit) != 0 {
                return Err(bad("bits that pad a byte are not zero"));
            }
            self.bit = 0;
            self.byte += 1;
        }
        Ok(())
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8], BbqrError> {
        let end = self.byte.checked_add(n).ok_or(bad("the data ends early"))?;
        let out = self
            .data
            .get(self.byte..end)
            .ok_or(bad("the data ends early"))?;
        self.byte = end;
        Ok(out)
    }
}

/// A canonical Huffman code, read the way RFC 1951 section 3.2.2 lays it
/// out: how many codes of each length, and the symbols in code order.
struct Huffman {
    count: [u16; MAX_BITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    /// The code of these lengths and how many code space is left over
    /// (negative: over-subscribed).
    fn build(lengths: &[u8]) -> Result<(Self, i32), BbqrError> {
        let mut count = [0u16; MAX_BITS + 1];
        for &l in lengths {
            let slot = count
                .get_mut(usize::from(l))
                .ok_or(bad("a code length over 15"))?;
            *slot += 1;
        }
        let mut left: i32 = 1;
        for len in 1..=MAX_BITS {
            left <<= 1;
            left -= i32::from(count.get(len).copied().unwrap_or(0));
            if left < 0 {
                return Err(bad("an over-subscribed code"));
            }
        }
        let mut offs = [0u16; MAX_BITS + 2];
        for len in 1..=MAX_BITS {
            let next = offs
                .get(len)
                .copied()
                .unwrap_or(0)
                .wrapping_add(count.get(len).copied().unwrap_or(0));
            if let Some(slot) = offs.get_mut(len + 1) {
                *slot = next;
            }
        }
        let used = lengths.iter().filter(|l| **l != 0).count();
        let mut symbol = vec![0u16; used];
        for (sym, &l) in lengths.iter().enumerate() {
            if l == 0 {
                continue;
            }
            let at = offs.get_mut(usize::from(l)).ok_or(bad("a code length"))?;
            let slot = symbol
                .get_mut(usize::from(*at))
                .ok_or(bad("a code table"))?;
            *slot = u16::try_from(sym).map_err(|_| bad("a symbol number"))?;
            *at += 1;
        }
        Ok((Self { count, symbol }, left))
    }

    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, BbqrError> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..=MAX_BITS {
            code |= i32::try_from(bits.one()?).unwrap_or(0);
            let count = i32::from(self.count.get(len).copied().unwrap_or(0));
            if code - count < first {
                let at = usize::try_from(index + (code - first)).map_err(|_| bad("a code"))?;
                return self
                    .symbol
                    .get(at)
                    .copied()
                    .ok_or(bad("a code with no symbol"));
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(bad("a code that is not in the table"))
    }
}

fn fixed_tables() -> Result<(Huffman, Huffman), BbqrError> {
    let mut lengths = [0u8; FIXED_LIT];
    for (sym, l) in lengths.iter_mut().enumerate() {
        *l = match sym {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let (lit, _) = Huffman::build(&lengths)?;
    let (dist, _) = Huffman::build(&[5u8; 32])?;
    Ok((lit, dist))
}

fn dynamic_tables(bits: &mut Bits<'_>) -> Result<(Huffman, Huffman), BbqrError> {
    let nlen = usize::try_from(bits.take(5)?).unwrap_or(0) + 257;
    let ndist = usize::try_from(bits.take(5)?).unwrap_or(0) + 1;
    let ncode = usize::try_from(bits.take(4)?).unwrap_or(0) + 4;
    if nlen > MAX_LIT_CODES || ndist > MAX_DIST_CODES {
        return Err(bad("more length or distance codes than exist"));
    }
    let mut clens = [0u8; 19];
    for &slot in CLEN_ORDER.iter().take(ncode) {
        let v = u8::try_from(bits.take(3)?).unwrap_or(0);
        if let Some(c) = clens.get_mut(slot) {
            *c = v;
        }
    }
    let (clen_code, left) = Huffman::build(&clens)?;
    if left != 0 {
        return Err(bad("an incomplete code of code lengths"));
    }
    let total = nlen + ndist;
    let mut lengths = vec![0u8; total];
    let mut at = 0usize;
    while at < total {
        let sym = clen_code.decode(bits)?;
        if sym < 16 {
            if let Some(slot) = lengths.get_mut(at) {
                *slot = u8::try_from(sym).unwrap_or(0);
            }
            at += 1;
            continue;
        }
        let (value, repeat) = match sym {
            16 => {
                let prev = at
                    .checked_sub(1)
                    .and_then(|p| lengths.get(p))
                    .copied()
                    .ok_or(bad("a repeat with nothing before it"))?;
                (prev, 3 + bits.take(2)?)
            }
            17 => (0, 3 + bits.take(3)?),
            _ => (0, 11 + bits.take(7)?),
        };
        let repeat = usize::try_from(repeat).unwrap_or(usize::MAX);
        let end = at
            .checked_add(repeat)
            .filter(|e| *e <= total)
            .ok_or(bad("a repeat past the last code length"))?;
        for slot in lengths.get_mut(at..end).unwrap_or(&mut []) {
            *slot = value;
        }
        at = end;
    }
    if lengths.get(256).copied().unwrap_or(0) == 0 {
        return Err(bad("no code for the end of a block"));
    }
    let (lit_lengths, dist_lengths) = lengths.split_at(nlen);
    let (lit, left) = Huffman::build(lit_lengths)?;
    if left != 0 {
        return Err(bad("an incomplete code of literals and lengths"));
    }
    let (dist, left) = Huffman::build(dist_lengths)?;
    let used = dist_lengths.iter().filter(|l| **l != 0).count();
    let single = used == 1 && dist_lengths.iter().all(|l| *l <= 1);
    if left != 0 && used != 0 && !single {
        return Err(bad("an incomplete code of distances"));
    }
    Ok((lit, dist))
}

fn block(
    bits: &mut Bits<'_>,
    lit: &Huffman,
    dist: &Huffman,
    out: &mut Vec<u8>,
    cap: usize,
) -> Result<(), BbqrError> {
    loop {
        let sym = usize::from(lit.decode(bits)?);
        if sym < 256 {
            if out.len() >= cap {
                return Err(BbqrError::InflateTooBig);
            }
            out.push(u8::try_from(sym).unwrap_or(0));
            continue;
        }
        if sym == 256 {
            return Ok(());
        }
        let k = sym - 257;
        let base = LEN_BASE
            .get(k)
            .ok_or(bad("a length symbol that does not exist"))?;
        let extra = LEN_EXTRA
            .get(k)
            .ok_or(bad("a length symbol that does not exist"))?;
        let len = usize::from(*base) + usize::try_from(bits.take(u32::from(*extra))?).unwrap_or(0);
        let d = usize::from(dist.decode(bits)?);
        let dbase = DIST_BASE
            .get(d)
            .ok_or(bad("a distance symbol that does not exist"))?;
        let dextra = DIST_EXTRA
            .get(d)
            .ok_or(bad("a distance symbol that does not exist"))?;
        let distance =
            usize::from(*dbase) + usize::try_from(bits.take(u32::from(*dextra))?).unwrap_or(0);
        if distance > out.len() {
            return Err(bad("a distance that reaches before the start"));
        }
        if out.len().saturating_add(len) > cap {
            return Err(BbqrError::InflateTooBig);
        }
        for _ in 0..len {
            let at = out.len() - distance;
            let b = out.get(at).copied().ok_or(bad("a distance"))?;
            out.push(b);
        }
    }
}

/// The bytes of a raw deflate stream and how many bytes of `input` it used.
fn stream(input: &[u8], cap: usize) -> Result<(Vec<u8>, usize), BbqrError> {
    let mut bits = Bits::new(input);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.one()? == 1;
        match bits.take(2)? {
            0 => {
                bits.align()?;
                let head = bits.bytes(4)?;
                let (len, nlen) = match head {
                    [a, b, c, d] => (u16::from_le_bytes([*a, *b]), u16::from_le_bytes([*c, *d])),
                    _ => return Err(bad("the data ends early")),
                };
                if len != !nlen {
                    return Err(bad("a stored block whose length check fails"));
                }
                let len = usize::from(len);
                if out.len().saturating_add(len) > cap {
                    return Err(BbqrError::InflateTooBig);
                }
                out.extend_from_slice(bits.bytes(len)?);
            }
            1 => {
                let (lit, dist) = fixed_tables()?;
                block(&mut bits, &lit, &dist, &mut out, cap)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                block(&mut bits, &lit, &dist, &mut out, cap)?;
            }
            _ => return Err(bad("a block of a type that does not exist")),
        }
        if last {
            break;
        }
    }
    bits.align()?;
    Ok((out, bits.byte))
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

/// BQ-11: the bytes inside a stream that is raw deflate; or, when that
/// fails and the stream starts with a valid zlib header, inside the zlib
/// wrapper, whose checksum must match. No more than `cap` bytes come out
/// (BQ-12), and nothing may follow the end of the stream.
pub fn inflate(input: &[u8], cap: usize) -> Result<Vec<u8>, BbqrError> {
    let raw = match stream(input, cap) {
        Ok((out, used)) if used == input.len() => return Ok(out),
        Ok(_) => bad("bytes after the end of the stream"),
        Err(e) => e,
    };
    let (Some(cmf), Some(flg)) = (input.first(), input.get(1)) else {
        return Err(raw);
    };
    let header_ok = cmf & 0x0f == 8
        && cmf >> 4 <= 7
        && (u32::from(*cmf) * 256 + u32::from(*flg)).is_multiple_of(31)
        && flg & 0x20 == 0;
    if !header_ok {
        return Err(raw);
    }
    let Some(body) = input.get(2..) else {
        return Err(raw);
    };
    let Ok((out, used)) = stream(body, cap) else {
        return Err(raw);
    };
    let tail = body.get(used..).unwrap_or(&[]);
    match tail {
        [a, b, c, d] if u32::from_be_bytes([*a, *b, *c, *d]) == adler32(&out) => Ok(out),
        _ => Err(raw),
    }
}
