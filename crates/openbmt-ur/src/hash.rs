//! SHA-256 (FIPS 180-4) and CRC-32 (the IEEE polynomial, as in zlib), both
//! written here so that the crate has no dependency. Checked against
//! `sha256sum` and Python's `zlib.crc32` (see `tests/hash.rs`).

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// One 64-byte block into the state.
fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w: Vec<u32> = block
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_be_bytes(*c))
        .collect();
    for i in 16..64usize {
        let g = |back: usize| w.get(i - back).copied().unwrap_or(0);
        let s0 = g(15).rotate_right(7) ^ g(15).rotate_right(18) ^ (g(15) >> 3);
        let s1 = g(2).rotate_right(17) ^ g(2).rotate_right(19) ^ (g(2) >> 10);
        let next = g(16).wrapping_add(s0).wrapping_add(g(7)).wrapping_add(s1);
        w.push(next);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (&k, &wi) in K.iter().zip(w.iter()) {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ (!e & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(k)
            .wrapping_add(wi);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s = s.wrapping_add(v);
    }
}

/// The SHA-256 digest of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut state = H0;
    let (blocks, rest) = data.as_chunks::<64>();
    for block in blocks {
        compress(&mut state, block);
    }
    let mut tail = rest.to_vec();
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in tail.as_chunks::<64>().0 {
        compress(&mut state, block);
    }
    let mut out = [0u8; 32];
    for (dst, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *dst = word.to_be_bytes();
    }
    out
}

/// Built at compile time: an index out of range would stop the build.
#[allow(clippy::indexing_slicing)]
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = crc_table();

/// The CRC-32 (IEEE, reflected, as `zlib.crc32`) of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in data {
        let idx = usize::from((c as u8) ^ b);
        let t = CRC_TABLE.get(idx).copied().unwrap_or(0);
        c = t ^ (c >> 8);
    }
    !c
}
