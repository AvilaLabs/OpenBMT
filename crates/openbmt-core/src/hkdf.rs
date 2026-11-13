//! HKDF with SHA-256 (RFC 5869), for the message key of LK-02.
//!
//! `bitcoin::hashes` has the HMAC but not the key derivation, so the two
//! steps are written out here and checked against the RFC's test vectors.

use bitcoin::hashes::{sha256, Hash, HashEngine, Hmac, HmacEngine};

/// The longest output HKDF-SHA256 gives: 255 blocks of 32 bytes.
pub const MAX_OUTPUT: usize = 255 * 32;

/// HMAC-SHA256 of `message` under `key`.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut engine = HmacEngine::<sha256::Hash>::new(key);
    engine.input(message);
    Hmac::<sha256::Hash>::from_engine(engine).to_byte_array()
}

/// HKDF-Extract: the pseudorandom key from `salt` and the input key
/// material.
pub fn extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    let mut engine = HmacEngine::<sha256::Hash>::new(salt);
    engine.input(ikm);
    Hmac::<sha256::Hash>::from_engine(engine).to_byte_array()
}

/// HKDF-Expand: fill `out` from the pseudorandom key and `info`. Returns
/// false, and writes nothing, if `out` is longer than [`MAX_OUTPUT`].
pub fn expand(prk: &[u8; 32], info: &[u8], out: &mut [u8]) -> bool {
    if out.len() > MAX_OUTPUT {
        return false;
    }
    let mut previous: Option<[u8; 32]> = None;
    let mut counter: u8 = 0;
    for chunk in out.chunks_mut(32) {
        counter = counter.wrapping_add(1);
        let mut engine = HmacEngine::<sha256::Hash>::new(prk);
        if let Some(prev) = &previous {
            engine.input(prev);
        }
        engine.input(info);
        engine.input(&[counter]);
        let block = Hmac::<sha256::Hash>::from_engine(engine).to_byte_array();
        for (slot, byte) in chunk.iter_mut().zip(block.iter()) {
            *slot = *byte;
        }
        previous = Some(block);
    }
    true
}

/// Extract then expand to 32 bytes.
pub fn derive32(salt: &[u8], ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let prk = extract(salt, ikm);
    let mut key = [0u8; 32];
    let ok = expand(&prk, info, &mut key);
    debug_assert!(ok);
    key
}
