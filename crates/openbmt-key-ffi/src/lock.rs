//! `OBMx`: the PIN lock around the state the Key app stores
//! (`docs/SIGNER-RULES.md` SR-71 to SR-74).
//!
//! ```text
//! "OBMx" | 1 | m KiB u32 | t u32 | p u32 | salt (16) | nonce (12)
//!   | ciphertext length u32 | ciphertext (ChaCha20-Poly1305 of the
//!     `OBM7` bytes, tag included)
//! ```
//!
//! The key is Argon2id of the PIN with the salt and the parameters in the
//! header; the header, all of it, is the associated data. The Key app's
//! store seals the bytes it is given with this frame on every write, with a
//! new nonce each time, and the Android Keystore wraps the frame again
//! outside: the device and the PIN are both needed. Decoding is strict: it
//! succeeds only for bytes that [`LockedFrame::to_bytes`] would have made.
//! A wrong PIN and a frame whose ciphertext was changed give the same
//! answer (the tag does not verify); a frame that is cut, lengthened or of
//! another form is told apart by its length and its fields.

use std::sync::{Arc, Mutex};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::error::{refused_by, KeyError, StoreError};
use crate::store::KeyStore;

/// SR-71: the magic of the locked frame.
pub const MAGIC_LOCKED: [u8; 4] = *b"OBMx";
/// Bytes of salt.
pub const SALT_LEN: usize = 16;
/// Bytes of nonce.
pub const NONCE_LEN: usize = 12;
/// Bytes of the AEAD's tag.
const TAG_LEN: usize = 16;
/// Bytes of the header: magic, version, three parameters, salt, nonce and
/// the length of the ciphertext.
pub const HEADER_LEN: usize = 4 + 1 + 12 + SALT_LEN + NONCE_LEN + 4;
/// The most a ciphertext may hold: the stored state is at most 64 MiB of
/// snapshot and a little more.
const MAX_CIPHERTEXT: usize = 80 * 1024 * 1024;

/// A PIN is at least this many digits.
pub const MIN_PIN_DIGITS: usize = 6;
/// A passphrase is at least this many characters.
pub const MIN_PASSPHRASE_CHARS: usize = 8;
/// The longest PIN or passphrase, in characters.
pub const MAX_PIN_CHARS: usize = 128;

/// SR-71: Argon2id parameters.
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
    /// SR-71: 64 MiB, 3 passes, 1 lane.
    pub const STANDARD: Self = Self {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };
    /// SR-71: the most a file may ask for: 256 MiB, 10 passes, 4 lanes.
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
        // Argon2 wants at least 8 blocks per lane, one pass and one lane.
        if self.p == 0 || self.t == 0 || self.m_kib < self.p.saturating_mul(8) {
            return Err(DecodeError::NonCanonical(
                "the key parameters are not valid",
            ));
        }
        Ok(())
    }
}

/// What the Key app asks of Argon2: the standard parameters, and small ones
/// in this crate's own tests so that they run quickly.
#[cfg(not(test))]
pub(crate) const COST: KdfParams = KdfParams::STANDARD;
#[cfg(test)]
pub(crate) const COST: KdfParams = KdfParams {
    m_kib: 64,
    t: 1,
    p: 1,
};

/// What the header of a locked frame says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedHeader {
    /// The key parameters.
    pub params: KdfParams,
    /// The salt.
    pub salt: [u8; SALT_LEN],
    /// The nonce of this write.
    pub nonce: [u8; NONCE_LEN],
    /// The length of the ciphertext that follows, tag included.
    pub body_len: u32,
}

impl LockedHeader {
    /// The header's bytes: also the associated data of the AEAD.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_LOCKED, HEADER_LEN);
        out.extend_from_slice(&self.params.m_kib.to_le_bytes());
        out.extend_from_slice(&self.params.t.to_le_bytes());
        out.extend_from_slice(&self.params.p.to_le_bytes());
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.body_len.to_le_bytes());
        out
    }
}

/// A locked frame, split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedFrame<'a> {
    /// The header.
    pub header: LockedHeader,
    /// The ciphertext, tag included.
    pub ciphertext: &'a [u8],
}

impl LockedFrame<'_> {
    /// The frame's bytes again.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.header.to_bytes();
        out.extend_from_slice(self.ciphertext);
        out
    }
}

/// SR-71: read a locked frame, strictly. The parameters are checked against
/// the limit here, before anything runs Argon2.
pub fn decode(bytes: &[u8]) -> Result<LockedFrame<'_>, DecodeError> {
    let mut r = Reader::framed(bytes, &MAGIC_LOCKED)?;
    let params = KdfParams {
        m_kib: r.u32_le()?,
        t: r.u32_le()?,
        p: r.u32_le()?,
    };
    params.within_limit()?;
    let salt = r.array::<SALT_LEN>()?;
    let nonce = r.array::<NONCE_LEN>()?;
    let body_len = r.u32_le()?;
    let n = usize::try_from(body_len).map_err(|_| DecodeError::TooLarge)?;
    if n > MAX_CIPHERTEXT {
        return Err(DecodeError::TooLarge);
    }
    if n < TAG_LEN {
        return Err(DecodeError::Truncated);
    }
    let ciphertext = r.take(n)?;
    r.finish()?;
    Ok(LockedFrame {
        header: LockedHeader {
            params,
            salt,
            nonce,
            body_len,
        },
        ciphertext,
    })
}

/// Whether `bytes` start as a locked frame.
pub(crate) fn is_locked(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(&MAGIC_LOCKED[..])
}

/// SR-72: why a PIN is not taken.
pub(crate) fn check_pin(pin: &str) -> Result<(), KeyError> {
    let chars = pin.chars().count();
    if chars > MAX_PIN_CHARS {
        return Err(refused_by(
            "pin-long",
            format!("a PIN or passphrase is at most {MAX_PIN_CHARS} characters"),
        ));
    }
    let digits = chars >= MIN_PIN_DIGITS && pin.bytes().all(|b| b.is_ascii_digit());
    if digits || chars >= MIN_PASSPHRASE_CHARS {
        return Ok(());
    }
    Err(refused_by(
        "pin-weak",
        format!(
            "a PIN is at least {MIN_PIN_DIGITS} digits, or a passphrase of at least \
             {MIN_PASSPHRASE_CHARS} characters"
        ),
    ))
}

/// The key that locks the stored state, and what is needed to write with it.
/// Overwritten when dropped.
pub(crate) struct LockKey {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key: [u8; 32],
    /// The nonce of the last frame written or read under this key.
    last_nonce: Option<[u8; NONCE_LEN]>,
}

impl Drop for LockKey {
    fn drop(&mut self) {
        self.key.fill(0);
        std::hint::black_box(&mut self.key);
    }
}

impl std::fmt::Debug for LockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LockKey(<hidden>)")
    }
}

/// The key from the PIN and the salt. The derived bytes are the caller's to
/// wipe.
fn derive(params: &KdfParams, pin: &str, salt: &[u8; SALT_LEN]) -> Option<[u8; 32]> {
    let p = Params::new(params.m_kib, params.t, params.p, Some(32)).ok()?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(pin.as_bytes(), salt, &mut key)
        .ok()?;
    Some(key)
}

fn cipher(key: &[u8; 32]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(&Key::from(*key))
}

impl LockKey {
    /// A key for `pin` under a new salt.
    pub(crate) fn new(params: KdfParams, pin: &str, salt: [u8; SALT_LEN]) -> Option<Self> {
        params.within_limit().ok()?;
        Some(Self {
            params,
            salt,
            key: derive(&params, pin, &salt)?,
            last_nonce: None,
        })
    }

    /// Whether `pin` gives this key.
    pub(crate) fn matches(&self, pin: &str) -> bool {
        let Some(mut other) = derive(&self.params, pin, &self.salt) else {
            return false;
        };
        let same = other
            .iter()
            .zip(self.key.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0;
        other.fill(0);
        std::hint::black_box(&mut other);
        same
    }

    /// The frame that holds `plain`, under `nonce`.
    fn seal(&self, nonce: [u8; NONCE_LEN], plain: &[u8]) -> Option<Vec<u8>> {
        let body_len = u32::try_from(plain.len().checked_add(TAG_LEN)?).ok()?;
        let header = LockedHeader {
            params: self.params,
            salt: self.salt,
            nonce,
            body_len,
        };
        let aad = header.to_bytes();
        let ciphertext = cipher(&self.key)
            .encrypt(
                &Nonce::from(nonce),
                Payload {
                    msg: plain,
                    aad: &aad,
                },
            )
            .ok()?;
        let mut out = aad;
        out.extend_from_slice(&ciphertext);
        Some(out)
    }

    /// The bytes inside `frame`, if this is its key.
    fn open_frame(&self, frame: &LockedFrame<'_>) -> Option<Vec<u8>> {
        if frame.header.salt != self.salt || frame.header.params != self.params {
            return None;
        }
        cipher(&self.key)
            .decrypt(
                &Nonce::from(frame.header.nonce),
                Payload {
                    msg: frame.ciphertext,
                    aad: &frame.header.to_bytes(),
                },
            )
            .ok()
    }

    /// The frame that holds `plain`, with a nonce that is not the last one
    /// this key used. The nonce is remembered before the frame is returned,
    /// so that a write that fails after this still never repeats it.
    pub(crate) fn seal_next(&mut self, nonce: [u8; NONCE_LEN], plain: &[u8]) -> Option<Vec<u8>> {
        if self.last_nonce == Some(nonce) {
            return None;
        }
        self.last_nonce = Some(nonce);
        self.seal(nonce, plain)
    }
}

/// Why a locked frame did not open.
#[derive(Debug)]
pub(crate) enum OpenError {
    /// The bytes are not a locked frame.
    Unreadable(DecodeError),
    /// The PIN is wrong, or the ciphertext was changed.
    WrongPin,
}

/// SR-73: open a locked frame with the PIN. Returns the bytes inside and the
/// key, ready for the writes that follow.
pub(crate) fn open(bytes: &[u8], pin: &str) -> Result<(Vec<u8>, LockKey), OpenError> {
    let frame = decode(bytes).map_err(OpenError::Unreadable)?;
    let mut key =
        LockKey::new(frame.header.params, pin, frame.header.salt).ok_or(OpenError::WrongPin)?;
    let plain = key.open_frame(&frame).ok_or(OpenError::WrongPin)?;
    key.last_nonce = Some(frame.header.nonce);
    Ok((plain, key))
}

/// SR-73: the stored frame again under another key, for a change of PIN.
/// `None` when `old` is not the key of `stored`.
pub(crate) fn reseal(
    stored: &[u8],
    old: &LockKey,
    new: &mut LockKey,
    nonce: [u8; NONCE_LEN],
) -> Result<Vec<u8>, OpenError> {
    let frame = decode(stored).map_err(OpenError::Unreadable)?;
    let mut plain = old.open_frame(&frame).ok_or(OpenError::WrongPin)?;
    let sealed = new.seal_next(nonce, &plain);
    plain.fill(0);
    std::hint::black_box(&mut plain);
    sealed.ok_or(OpenError::WrongPin)
}

/// What the lock holds between two calls, shared by the Key app and the
/// store it writes through.
pub(crate) type LockCell = Mutex<Option<LockKey>>;

/// The store the Key app writes through: every `save` seals the bytes in a
/// locked frame under the held key (SR-71), and everything else is the
/// platform's.
pub(crate) struct Sealing {
    raw: Arc<dyn KeyStore>,
    cell: Arc<LockCell>,
}

impl Sealing {
    pub(crate) fn new(raw: Arc<dyn KeyStore>, cell: Arc<LockCell>) -> Self {
        Self { raw, cell }
    }
}

fn failed(text: &str) -> StoreError {
    StoreError::Failed {
        text: text.to_string(),
    }
}

/// A nonce from the platform's random source.
pub(crate) fn draw_nonce(raw: &dyn KeyStore) -> Result<[u8; NONCE_LEN], StoreError> {
    let bytes = raw.random(NONCE_LEN as u32)?;
    <[u8; NONCE_LEN]>::try_from(bytes.as_slice())
        .map_err(|_| failed("the random source gave a different number of bytes than asked for"))
}

impl KeyStore for Sealing {
    fn load(&self) -> Result<Option<Vec<u8>>, StoreError> {
        self.raw.load()
    }

    fn save(&self, mut bytes: Vec<u8>) -> Result<(), StoreError> {
        let result = (|| {
            let nonce = draw_nonce(self.raw.as_ref())?;
            let frame = {
                let mut guard = self
                    .cell
                    .lock()
                    .map_err(|_| failed("the lock of the stored state is not usable"))?;
                let key = guard
                    .as_mut()
                    .ok_or_else(|| failed("no PIN is set, so nothing can be stored"))?;
                key.seal_next(nonce, &bytes).ok_or_else(|| {
                    failed("the random source gave the same nonce twice, so nothing was stored")
                })?
            };
            self.raw.save(frame)
        })();
        bytes.fill(0);
        std::hint::black_box(&mut bytes);
        result
    }

    fn random(&self, n: u32) -> Result<Vec<u8>, StoreError> {
        self.raw.random(n)
    }

    fn now(&self) -> u64 {
        self.raw.now()
    }
}
