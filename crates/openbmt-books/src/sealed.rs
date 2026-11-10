//! The encrypted export file, `OBMY` (BK-23 to BK-27).

use std::collections::BTreeMap;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use openbmt_core::identity::DeviceId;
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::books::Books;
use crate::bundle::{get_marks, put_marks};
use crate::error::{OpenError, SealError};

/// BK-23: magic of the encrypted export.
pub const MAGIC_EXPORT: [u8; 4] = *b"OBMY";
/// BK-25: bytes of salt.
pub const SALT_LEN: usize = 16;
/// Bytes of the AEAD's tag.
const TAG_LEN: usize = 16;

/// BK-24, BK-27: Argon2id parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_kib: u32,
    /// Passes.
    pub t: u32,
    /// Lanes.
    pub p: u32,
}

impl KdfParams {
    /// BK-24: 64 MiB, 3 passes, 1 lane.
    pub const STANDARD: Self = Self {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };
    /// BK-27: the most a file may ask for: 256 MiB, 10 passes, 4 lanes.
    pub const LIMIT: Self = Self {
        m_kib: 256 * 1024,
        t: 10,
        p: 4,
    };

    /// Whether these are parameters Argon2 takes and the limit allows.
    fn within_limit(&self) -> Result<(), DecodeError> {
        if self.m_kib > Self::LIMIT.m_kib || self.t > Self::LIMIT.t || self.p > Self::LIMIT.p {
            return Err(DecodeError::TooLarge);
        }
        // Argon2 wants at least 8 blocks per lane, and one pass and one lane.
        if self.p == 0 || self.t == 0 || self.m_kib < self.p.saturating_mul(8) {
            return Err(DecodeError::NonCanonical(
                "the key parameters are not valid",
            ));
        }
        Ok(())
    }
}

/// The caller's randomness failed (BK-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomFailed;

/// BK-23: what the header of an encrypted export says. It is readable
/// without the passphrase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportHeader {
    /// The key parameters.
    pub params: KdfParams,
    /// The salt.
    pub salt: [u8; SALT_LEN],
    /// When the file was made, in Unix seconds, as the caller gave it.
    pub created_at: u64,
    /// The high-water mark of each counter (BK-11).
    pub high_water: BTreeMap<DeviceId, Option<u32>>,
}

impl ExportHeader {
    /// The header bytes: also the associated data of the AEAD (BK-26).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_EXPORT, 64);
        out.extend_from_slice(&self.params.m_kib.to_le_bytes());
        out.extend_from_slice(&self.params.t.to_le_bytes());
        out.extend_from_slice(&self.params.p.to_le_bytes());
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.created_at.to_le_bytes());
        put_marks(&mut out, &self.high_water);
        out
    }
}

/// BK-23, BK-27: split a file into its header, the header's bytes and the
/// ciphertext, strictly. The parameters are checked against the limit here,
/// before anything runs Argon2.
fn split(bytes: &[u8]) -> Result<(ExportHeader, &[u8], &[u8]), DecodeError> {
    let mut r = Reader::framed(bytes, &MAGIC_EXPORT)?;
    let params = KdfParams {
        m_kib: r.u32_le()?,
        t: r.u32_le()?,
        p: r.u32_le()?,
    };
    params.within_limit()?;
    let header = ExportHeader {
        params,
        salt: r.array::<SALT_LEN>()?,
        created_at: r.u64_le()?,
        high_water: get_marks(&mut r)?,
    };
    // The reader gives no position: the header is canonical, so its length
    // is that of its own encoding.
    let len = header.to_bytes().len();
    let aad = bytes.get(..len).ok_or(DecodeError::Truncated)?;
    let ciphertext = bytes.get(len..).ok_or(DecodeError::Truncated)?;
    if ciphertext.len() < TAG_LEN {
        return Err(DecodeError::Truncated);
    }
    Ok((header, aad, ciphertext))
}

/// BK-23: read the header of an encrypted export without the passphrase.
pub fn decode_export_header(bytes: &[u8]) -> Result<ExportHeader, DecodeError> {
    split(bytes).map(|(header, _, _)| header)
}

/// BK-24: the key from the passphrase and the salt.
fn derive_key(params: &KdfParams, passphrase: &str, salt: &[u8; SALT_LEN]) -> Option<[u8; 32]> {
    let p = Params::new(params.m_kib, params.t, params.p, Some(32)).ok()?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .ok()?;
    Some(key)
}

/// BK-25: the nonce is all zeros, sound because the key is new for every file.
fn cipher(key: [u8; 32]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(&Key::from(key))
}

fn zero_nonce() -> Nonce {
    Nonce::from([0u8; 12])
}

/// BK-23 to BK-27: seal the books under a passphrase, with the standard key
/// parameters. The salt comes from `random`; if it fails nothing is made.
pub fn seal_export(
    books: &Books,
    passphrase: &str,
    created_at: u64,
    random: impl FnOnce(&mut [u8; SALT_LEN]) -> Result<(), RandomFailed>,
) -> Result<Vec<u8>, SealError> {
    seal_with(books, passphrase, created_at, KdfParams::STANDARD, random)
}

/// The same with chosen parameters: the tests make small ones.
pub(crate) fn seal_with(
    books: &Books,
    passphrase: &str,
    created_at: u64,
    params: KdfParams,
    random: impl FnOnce(&mut [u8; SALT_LEN]) -> Result<(), RandomFailed>,
) -> Result<Vec<u8>, SealError> {
    if passphrase.is_empty() {
        return Err(SealError::EmptyPassphrase);
    }
    params.within_limit().map_err(|_| SealError::Key)?;
    let mut salt = [0u8; SALT_LEN];
    random(&mut salt).map_err(|_| SealError::Randomness)?;
    let header = ExportHeader {
        params,
        salt,
        created_at,
        high_water: books.high_water().clone(),
    };
    let aad = header.to_bytes();
    let key = derive_key(&params, passphrase, &salt).ok_or(SealError::Key)?;
    let ciphertext = cipher(key)
        .encrypt(
            &zero_nonce(),
            Payload {
                msg: &books.to_bundle(),
                aad: &aad,
            },
        )
        .map_err(|_| SealError::Cipher)?;
    let mut out = aad;
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// BK-26, BK-27: open an encrypted export. Wrong passphrase and a damaged
/// file give the same answer.
pub fn open_export(passphrase: &str, bytes: &[u8]) -> Result<Books, OpenError> {
    if passphrase.is_empty() {
        return Err(OpenError::EmptyPassphrase);
    }
    let bad = OpenError::WrongPassphraseOrDamaged;
    let (header, aad, ciphertext) = split(bytes).map_err(|_| bad)?;
    let key = derive_key(&header.params, passphrase, &header.salt).ok_or(bad)?;
    let plain = cipher(key)
        .decrypt(
            &zero_nonce(),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| bad)?;
    let books = Books::from_bundle(&plain).map_err(|_| bad)?;
    if books.high_water() != &header.high_water {
        return Err(bad);
    }
    Ok(books)
}
