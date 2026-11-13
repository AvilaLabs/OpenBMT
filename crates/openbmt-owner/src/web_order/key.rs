//! WO-43: the web order key, Ed25519, made from 32 random bytes.

use std::fmt;

use bitcoin::hashes::{sha256, Hash};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};

use super::{hex, WebOrderRefusal};
use crate::starter::{checked_read, KernelRandom, KERNEL_BYTES};

/// The bytes of a public key.
pub const PUBLIC_BYTES: usize = 32;
/// The bytes of a signature.
pub const SIGNATURE_BYTES: usize = 64;
/// The bytes of the key's hash shown as its fingerprint (WO-05).
pub const FINGERPRINT_BYTES: usize = 8;

/// The key the owner phone signs web order messages with. It holds the
/// 32-byte seed; the seed is wiped when the key is dropped.
pub struct WebOrderKey {
    signing: SigningKey,
}

impl fmt::Debug for WebOrderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WebOrderKey(..)")
    }
}

impl WebOrderKey {
    /// WO-43: a key of 32 bytes of the kernel's randomness, read the way the
    /// starter wallet reads it (SS-02): two reads, neither all one byte, and
    /// different.
    pub fn generate(rng: &mut dyn KernelRandom) -> Result<Self, WebOrderRefusal> {
        let seed = checked_read(rng).map_err(|_| WebOrderRefusal::Random)?;
        Ok(Self::from_seed(&seed))
    }

    /// The key of a stored seed.
    pub fn from_seed(seed: &[u8; KERNEL_BYTES]) -> Self {
        Self {
            signing: SigningKey::from_bytes(seed),
        }
    }

    /// The seed, to be stored with the owner's other secrets. The caller
    /// wipes its copy.
    pub fn seed(&self) -> [u8; KERNEL_BYTES] {
        self.signing.to_bytes()
    }

    /// The public key the store keeps (WO-03).
    pub fn public(&self) -> [u8; PUBLIC_BYTES] {
        self.signing.verifying_key().to_bytes()
    }

    /// WO-05: the short fingerprint of a public key, the first 8 bytes of its
    /// SHA-256 in hex, in four groups of four with a dash between.
    pub fn fingerprint_of(public: &[u8; PUBLIC_BYTES]) -> String {
        let digest = sha256::Hash::hash(public).to_byte_array();
        let short = digest.get(..FINGERPRINT_BYTES).unwrap_or(&[]);
        let text = hex(short);
        let mut out = String::with_capacity(text.len() + 3);
        for (i, c) in text.chars().enumerate() {
            if i > 0 && i % 4 == 0 {
                out.push('-');
            }
            out.push(c);
        }
        out
    }

    /// The fingerprint of this key.
    pub fn fingerprint(&self) -> String {
        Self::fingerprint_of(&self.public())
    }

    /// Signs a message. Ed25519 is deterministic: the same message gives the
    /// same signature.
    pub(crate) fn sign(&self, message: &[u8]) -> [u8; SIGNATURE_BYTES] {
        self.signing.sign(message).to_bytes()
    }
}

/// Whether `signature` is the signature of `message` under `public`.
/// Malleable and small-order forms are refused.
pub(crate) fn verify(
    public: &[u8; PUBLIC_BYTES],
    message: &[u8],
    signature: &[u8; SIGNATURE_BYTES],
) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(public) else {
        return false;
    };
    key.verify_strict(message, &Signature::from_bytes(signature))
        .is_ok()
}
