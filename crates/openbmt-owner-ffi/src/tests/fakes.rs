//! Fake random sources for the refusal tests of every key-making site.

use openbmt_kernel_random::{KernelRandom, READ_BYTES};

/// A source that gives the blocks it was given, then none.
pub(crate) struct Blocks(Vec<[u8; READ_BYTES]>);

fn good(n: u8) -> [u8; READ_BYTES] {
    std::array::from_fn(|i| u8::try_from(i).unwrap_or(0).wrapping_mul(5).wrapping_add(n))
}

impl Blocks {
    /// Good, different blocks, enough for any call here.
    pub(crate) fn good() -> Self {
        Self((1..=8).map(good).collect())
    }

    /// No bytes at all.
    pub(crate) fn dry() -> Self {
        Self(Vec::new())
    }

    /// Blocks of all zero.
    pub(crate) fn zeros() -> Self {
        Self(vec![[0; READ_BYTES]; 8])
    }

    /// The same good block every time.
    pub(crate) fn repeat() -> Self {
        Self(vec![good(1); 8])
    }
}

impl KernelRandom for Blocks {
    fn read32(&mut self) -> Option<[u8; READ_BYTES]> {
        if self.0.is_empty() {
            None
        } else {
            Some(self.0.remove(0))
        }
    }
}
