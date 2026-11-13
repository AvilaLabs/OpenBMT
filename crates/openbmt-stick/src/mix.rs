//! Chance (SK-20 to SK-22): how the 16 bytes of entropy are made, from the
//! kernel's random bytes and the owner's die rolls, or from die rolls alone.
//! Pure functions: the bytes and rolls come in, the entropy goes out.

use bitcoin::hashes::{sha256, Hash};
use openbmt_core::mix::{faces_ok, first16};

pub use openbmt_core::mix::{MixError, ENTROPY_BYTES, KERNEL_BYTES};

/// The tag hashed first (SK-20).
pub const MIX_TAG: &[u8] = b"OpenBMT/stick/v1";
/// Values a dice-alone run needs (KM-31).
pub const DICE_VALUES: usize = 50;
/// Rolls that are enough on their own with the computer's bytes (SK-21).
pub const ENOUGH_ROLLS: usize = 50;

/// SK-20: the first 16 bytes of SHA-256 over the tag, a zero byte, the 32
/// kernel bytes and the rolls as ASCII digits. Any number of rolls from 0.
/// The function is `openbmt_core::mix::mix`, shared with the owner phone's
/// starter wallet (SS-03), with this program's tag.
pub fn mix(kernel: &[u8; KERNEL_BYTES], rolls: &[u8]) -> Result<[u8; ENTROPY_BYTES], MixError> {
    openbmt_core::mix::mix(MIX_TAG, kernel, rolls)
}

/// KM-30: rolls are taken in threes from the start. Three different numbers
/// give a value by the order of low (L), middle (M) and high (H) as they
/// came: LMH 1, LHM 2, MHL 3, HML 4, HLM 5, MLH 6. A three with a repeat is
/// dropped; a last open three gives nothing.
pub fn dice_values(rolls: &[u8]) -> Result<Vec<u8>, MixError> {
    faces_ok(rolls)?;
    Ok(rolls
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|t| {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                return None;
            }
            // The rank of each roll among the three: 0 low, 1 middle, 2 high.
            let rank = |x: u8| t.iter().filter(|o| **o < x).count();
            match (rank(t[0]), rank(t[1]), rank(t[2])) {
                (0, 1, 2) => Some(1), // LMH
                (0, 2, 1) => Some(2), // LHM
                (1, 2, 0) => Some(3), // MHL
                (2, 1, 0) => Some(4), // HML
                (2, 0, 1) => Some(5), // HLM
                (1, 0, 2) => Some(6), // MLH
                _ => None,
            }
        })
        .collect())
}

/// SK-22 and KM-31: exactly 50 values make a string of 50 ASCII digits; the
/// entropy is the first 16 bytes of its SHA-256. No kernel bytes.
pub fn dice_alone(values: &[u8]) -> Result<[u8; ENTROPY_BYTES], MixError> {
    if values.len() != DICE_VALUES {
        return Err(MixError::NeedFiftyValues);
    }
    faces_ok(values)?;
    let digits: Vec<u8> = values.iter().map(|v| b'0' + v).collect();
    let digest = sha256::Hash::hash(&digits);
    Ok(first16(digest.as_byte_array()))
}
