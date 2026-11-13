//! What goes to the second device is sealed with a key made from the node's
//! seed (SL-20): the device holds the state and never the key.
//!
//! ChaCha20-Poly1305, 12-byte nonce in front of the ciphertext, the name the
//! blob is stored under as the associated data, so that a blob moved to
//! another name does not open. Pure: the nonce is passed in.

use crate::bip85::NodeSeed;
use bitcoin::hashes::{sha512, Hash, HashEngine, Hmac, HmacEngine};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

const SEAL_LABEL: &[u8] = b"openbmt-shop-lightning/second-device/v1";
/// The bytes in front of a sealed blob: the nonce.
pub const NONCE_LEN: usize = 12;
/// The bytes a sealed blob is longer than its content by: nonce and tag.
pub const OVERHEAD: usize = NONCE_LEN + 16;

/// The key for the second device's blobs.
#[derive(Clone, PartialEq, Eq)]
pub struct SealKey([u8; 32]);

impl std::fmt::Debug for SealKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SealKey(..)")
    }
}

impl SealKey {
    /// The key that comes from the node's seed. Another seed gives another key.
    pub fn from_seed(seed: &NodeSeed) -> Self {
        let mut engine = HmacEngine::<sha512::Hash>::new(&seed.0);
        engine.input(SEAL_LABEL);
        let out = Hmac::from_engine(engine).to_byte_array();
        let mut key = [0u8; 32];
        key.copy_from_slice(&out[..32]);
        SealKey(key)
    }
}

/// Why a blob did not open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealError {
    /// Shorter than the nonce and the tag.
    TooShort,
    /// The tag does not match: another key, another name, or a changed blob.
    BadTag,
}

/// Seals `content` under the name `name`, with the given nonce.
pub fn seal(key: &SealKey, nonce: [u8; NONCE_LEN], name: &str, content: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key.0));
    let body = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: content,
                aad: name.as_bytes(),
            },
        )
        .expect("encryption of a blob in memory does not fail");
    let mut out = Vec::with_capacity(NONCE_LEN + body.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    out
}

/// Opens a blob sealed under `name`.
pub fn open(key: &SealKey, name: &str, sealed: &[u8]) -> Result<Vec<u8>, SealError> {
    if sealed.len() < OVERHEAD {
        return Err(SealError::TooShort);
    }
    let (nonce, body) = sealed.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key.0));
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: body,
                aad: name.as_bytes(),
            },
        )
        .map_err(|_| SealError::BadTag)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u8) -> SealKey {
        SealKey::from_seed(&NodeSeed([n; 64]))
    }

    #[test]
    fn sl_20_a_sealed_blob_opens_with_its_key_and_name() {
        let sealed = seal(&key(1), [7; NONCE_LEN], "monitors~~a", b"state");
        assert_eq!(sealed.len(), 5 + OVERHEAD);
        assert_eq!(open(&key(1), "monitors~~a", &sealed).unwrap(), b"state");
    }

    #[test]
    fn sl_20_the_device_cannot_read_it_and_a_moved_blob_does_not_open() {
        let sealed = seal(&key(1), [7; NONCE_LEN], "monitors~~a", b"state");
        assert_eq!(
            open(&key(2), "monitors~~a", &sealed),
            Err(SealError::BadTag)
        );
        assert_eq!(
            open(&key(1), "monitors~~b", &sealed),
            Err(SealError::BadTag)
        );
        let mut flipped = sealed.clone();
        *flipped.last_mut().unwrap() ^= 1;
        assert_eq!(
            open(&key(1), "monitors~~a", &flipped),
            Err(SealError::BadTag)
        );
        assert_eq!(open(&key(1), "x", &sealed[..10]), Err(SealError::TooShort));
        assert!(!sealed.windows(5).any(|w| w == b"state"));
    }
}
