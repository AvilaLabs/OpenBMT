//! WO-05: the pairing code the owner phone shows and the plugin reads.
//!
//! Bytes: the magic, the version, the public key (32), the secret (32), the
//! length of the shop's name (1), the name, and the first 4 bytes of the
//! SHA-256 of everything before. The text of the code is those bytes in
//! base32 (RFC 4648 alphabet, upper case, no padding).

use bitcoin::hashes::{sha256, Hash};

use super::key::{WebOrderKey, PUBLIC_BYTES};
use super::{Reader, WebOrderRefusal, VERSION};
use crate::starter::{checked_read, KernelRandom};

/// The first four bytes of a pairing code.
pub const PAIRING_MAGIC: [u8; 4] = *b"OBM`";
/// The bytes of the secret of the requests (WO-20).
pub const SECRET_BYTES: usize = 32;
/// The longest shop name, in bytes.
pub const MAX_SHOP_NAME_BYTES: usize = 64;

/// The bytes of a pairing id: the start of the SHA-256 of the secret.
pub const PAIRING_ID_BYTES: usize = 8;

const CHECK_BYTES: usize = 4;
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// What a pairing code holds.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingCode {
    public: [u8; PUBLIC_BYTES],
    secret: [u8; SECRET_BYTES],
    shop_name: String,
}

impl std::fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingCode")
            .field("public", &self.public)
            .field("shop_name", &self.shop_name)
            .finish_non_exhaustive()
    }
}

/// WO-10: the id of a pairing, the first 8 bytes of the SHA-256 of its secret.
/// A range carries it, signed, so that a store accepts only the ranges of the
/// pairing it holds.
pub fn pairing_id_of(secret: &[u8; SECRET_BYTES]) -> [u8; PAIRING_ID_BYTES] {
    let digest = sha256::Hash::hash(secret).to_byte_array();
    let mut out = [0u8; PAIRING_ID_BYTES];
    for (o, d) in out.iter_mut().zip(digest.iter()) {
        *o = *d;
    }
    out
}

/// WO-05: a name of 1 to 64 bytes with no byte below 0x20 and no 0x7F.
fn name_is_valid(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SHOP_NAME_BYTES
        && name.bytes().all(|b| b >= 0x20 && b != 0x7f)
}

impl PairingCode {
    /// A code from the key's public half, a secret and the shop's name.
    pub fn new(
        public: [u8; PUBLIC_BYTES],
        secret: [u8; SECRET_BYTES],
        shop_name: &str,
    ) -> Result<Self, WebOrderRefusal> {
        if !name_is_valid(shop_name) {
            return Err(WebOrderRefusal::ShopName);
        }
        Ok(Self {
            public,
            secret,
            shop_name: shop_name.to_string(),
        })
    }

    /// WO-05: a code made with a fresh secret of the kernel's bytes.
    pub fn generate(
        key: &WebOrderKey,
        shop_name: &str,
        rng: &mut dyn KernelRandom,
    ) -> Result<Self, WebOrderRefusal> {
        let secret = checked_read(rng).map_err(|_| WebOrderRefusal::Random)?;
        Self::new(key.public(), secret, shop_name)
    }

    /// The owner phone's public key for web orders.
    pub fn public(&self) -> &[u8; PUBLIC_BYTES] {
        &self.public
    }

    /// The secret of the requests.
    pub fn secret(&self) -> &[u8; SECRET_BYTES] {
        &self.secret
    }

    /// The shop's name.
    pub fn shop_name(&self) -> &str {
        &self.shop_name
    }

    /// The id of this pairing (WO-10).
    pub fn id(&self) -> [u8; PAIRING_ID_BYTES] {
        pairing_id_of(&self.secret)
    }

    /// The fingerprint of the key in the code (WO-05).
    pub fn fingerprint(&self) -> String {
        WebOrderKey::fingerprint_of(&self.public)
    }

    /// The bytes of the code.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(80 + self.shop_name.len());
        out.extend_from_slice(&PAIRING_MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&self.public);
        out.extend_from_slice(&self.secret);
        out.push(u8::try_from(self.shop_name.len()).unwrap_or(0));
        out.extend_from_slice(self.shop_name.as_bytes());
        let check = sha256::Hash::hash(&out).to_byte_array();
        out.extend_from_slice(check.get(..CHECK_BYTES).unwrap_or(&[]));
        out
    }

    /// Reads the bytes of a code, strictly: the magic, the version, the
    /// name, the check and no byte more.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::PairingCode;
        let body_len = bytes.len().checked_sub(CHECK_BYTES).ok_or(bad)?;
        let (body, check) = bytes.split_at_checked(body_len).ok_or(bad)?;
        let digest = sha256::Hash::hash(body).to_byte_array();
        if digest.get(..CHECK_BYTES) != Some(check) {
            return Err(bad);
        }
        let mut r = Reader::new(body);
        if r.array::<4>().ok_or(bad)? != PAIRING_MAGIC || r.u8().ok_or(bad)? != VERSION {
            return Err(bad);
        }
        let public = r.array::<PUBLIC_BYTES>().ok_or(bad)?;
        let secret = r.array::<SECRET_BYTES>().ok_or(bad)?;
        let name_len = usize::from(r.u8().ok_or(bad)?);
        let name = std::str::from_utf8(r.take(name_len).ok_or(bad)?)
            .map_err(|_| WebOrderRefusal::ShopName)?;
        if !r.is_empty() {
            return Err(bad);
        }
        Self::new(public, secret, name)
    }

    /// The text of the code: base32, upper case, no padding.
    pub fn to_text(&self) -> String {
        let bytes = self.to_bytes();
        let mut out = String::with_capacity(bytes.len() * 8 / 5 + 1);
        let (mut buffer, mut bits) = (0u32, 0u32);
        for b in bytes {
            buffer = buffer << 8 | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                let index = usize::try_from(buffer >> bits & 31).unwrap_or(0);
                out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
            }
            buffer &= (1 << bits) - 1;
        }
        if bits > 0 {
            let index = usize::try_from(buffer << (5 - bits) & 31).unwrap_or(0);
            out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
        }
        out
    }

    /// Reads the text of a code. White space around it is ignored; any other
    /// character outside the alphabet, and leftover bits that are not zero,
    /// are refused.
    pub fn from_text(text: &str) -> Result<Self, WebOrderRefusal> {
        let bad = WebOrderRefusal::PairingCode;
        let mut bytes = Vec::with_capacity(text.len() * 5 / 8);
        let (mut buffer, mut bits) = (0u32, 0u32);
        for c in text.trim().bytes() {
            let value = ALPHABET.iter().position(|a| *a == c).ok_or(bad)?;
            buffer = buffer << 5 | u32::try_from(value).map_err(|_| bad)?;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                bytes.push(u8::try_from(buffer >> bits & 255).map_err(|_| bad)?);
                buffer &= (1 << bits) - 1;
            }
        }
        if bits >= 5 || buffer != 0 {
            return Err(bad);
        }
        Self::from_bytes(&bytes)
    }
}
