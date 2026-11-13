//! The mix of kernel bytes and die rolls into 16 bytes of entropy for 12
//! words: the stick's (`docs/STICK-RULES.md`, SK-20) and the owner phone's
//! starter wallet's (`docs/START-SIMPLE-RULES.md`, SS-03) are this one
//! function, each with its own tag. A pure function: the bytes and rolls
//! come in, the entropy goes out.

use bitcoin::hashes::{sha256, Hash, HashEngine};

/// Bytes asked of the kernel.
pub const KERNEL_BYTES: usize = 32;
/// Bytes of entropy for 12 words.
pub const ENTROPY_BYTES: usize = 16;

/// Why rolls were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixError {
    /// A roll that is not 1 to 6 (SK-21, KM-30).
    NotAFace,
    /// Dice alone needs exactly 50 values (SK-22, KM-31).
    NeedFiftyValues,
}

/// Every roll is a face of a die, 1 to 6.
pub fn faces_ok(rolls: &[u8]) -> Result<(), MixError> {
    if rolls.iter().all(|r| (1..=6).contains(r)) {
        Ok(())
    } else {
        Err(MixError::NotAFace)
    }
}

/// SK-20, SS-03: the first 16 bytes of SHA-256 over `tag`, a zero byte, the
/// 32 kernel bytes and the rolls as ASCII digits. Any number of rolls from 0.
pub fn mix(
    tag: &[u8],
    kernel: &[u8; KERNEL_BYTES],
    rolls: &[u8],
) -> Result<[u8; ENTROPY_BYTES], MixError> {
    faces_ok(rolls)?;
    let mut engine = sha256::HashEngine::default();
    engine.input(tag);
    engine.input(&[0]);
    engine.input(kernel);
    let digits: Vec<u8> = rolls.iter().map(|r| b'0' + r).collect();
    engine.input(&digits);
    let digest = sha256::Hash::from_engine(engine);
    Ok(first16(digest.as_byte_array()))
}

/// The first 16 bytes of a SHA-256 digest.
pub fn first16(digest: &[u8; 32]) -> [u8; ENTROPY_BYTES] {
    let mut out = [0u8; ENTROPY_BYTES];
    out.copy_from_slice(&digest[..ENTROPY_BYTES]);
    out
}
