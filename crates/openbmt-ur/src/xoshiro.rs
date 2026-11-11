//! Xoshiro256** as BCR-2020-005 uses it for the fountain code: seeded from
//! a SHA-256 digest read as four big-endian words, with the draws of a
//! float, of an integer in a closed range and of a byte defined from it.

use crate::hash::sha256;

pub(crate) struct Xoshiro256 {
    s: [u64; 4],
}

impl Xoshiro256 {
    /// Seeds from the SHA-256 of `data`.
    pub(crate) fn from_bytes(data: &[u8]) -> Self {
        let digest = sha256(data);
        let mut s = [0u64; 4];
        for (slot, chunk) in s.iter_mut().zip(digest.as_chunks::<8>().0) {
            *slot = u64::from_be_bytes(*chunk);
        }
        Self { s }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let [s0, s1, s2, s3] = self.s;
        let result = s1.wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s1 << 17;
        let s2 = s2 ^ s0;
        let s3 = s3 ^ s1;
        let s1 = s1 ^ s2;
        let s0 = s0 ^ s3;
        let s2 = s2 ^ t;
        let s3 = s3.rotate_left(45);
        self.s = [s0, s1, s2, s3];
        result
    }

    /// A float in `[0, 1)`: the draw divided by 2^64.
    pub(crate) fn next_double(&mut self) -> f64 {
        self.next_u64() as f64 / (u64::MAX as f64 + 1.0)
    }

    /// An integer in `low..=high`: `floor(double * (high - low + 1)) + low`.
    pub(crate) fn next_int(&mut self, low: u64, high: u64) -> u64 {
        let span = high.saturating_sub(low).saturating_add(1);
        (self.next_double() * span as f64) as u64 + low
    }

    #[cfg(test)]
    pub(crate) fn next_byte(&mut self) -> u8 {
        self.next_int(0, 255) as u8
    }
}
