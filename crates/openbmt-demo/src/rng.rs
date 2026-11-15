//! The demo's randomness: a seeded generator. The page passes the seed from
//! the browser's own random source; nothing here reads the system.

/// SplitMix64, which is enough to make the customer's coins differ.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// A generator that starts from `seed`.
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// The next 32 bytes.
    pub fn next_32(&mut self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for chunk in out.chunks_mut(8) {
            let word = self.next_u64().to_le_bytes();
            for (o, w) in chunk.iter_mut().zip(word.iter()) {
                *o = *w;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_bytes_and_another_gives_others() {
        let a = Rng::new(7).next_32();
        assert_eq!(a, Rng::new(7).next_32());
        assert_ne!(a, Rng::new(8).next_32());
        let mut r = Rng::new(7);
        assert_ne!(r.next_32(), r.next_32());
    }
}
