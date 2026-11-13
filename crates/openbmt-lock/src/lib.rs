//! `OBMx`: the PIN lock around the file that holds a phone's key
//! (`docs/LOCK-RULES.md`, LC-01 to LC-07), shared by the Key app and the
//! owner app.
//!
//! ```text
//! "OBMx" | 1 or 2 | m KiB u32 | t u32 | p u32 | salt (16) | nonce (12)
//!   | ciphertext length u32 | ciphertext (ChaCha20-Poly1305 of the
//!     bytes locked, tag included)
//! ```
//!
//! Version 2 holds, first inside the sealed bytes, the generation of the
//! stored state: a `u64` that every save raises by one (SR-92), so that a
//! handle can tell that the stored state is not the one it opened. A frame of
//! version 1 reads as generation 0. The generation is read only with the
//! key.
//!
//! The key is Argon2id of the PIN with the salt and the parameters in the
//! header; the header, all of it, is the associated data. An app that locks
//! a file seals the bytes with this frame on every write, with a new nonce
//! each time, and the Android Keystore wraps the frame again outside: the
//! device and the PIN are both needed. Decoding is strict: it succeeds only
//! for bytes that [`LockedFrame::to_bytes`] would have made. A wrong PIN and
//! a frame whose ciphertext was changed give the same answer (the tag does
//! not verify); a frame that is cut, lengthened or of another form is told
//! apart by its length and its fields.
//!
//! This crate holds no storage, no clock and no random source: the app
//! gives the salt and the nonce, and stores what is returned. It counts no
//! wrong tries and wipes nothing; that belongs to the app around it
//! (`apps/android-common`, `LockModel.kt`).
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet. Not for real funds.

#![deny(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use openbmt_core::wire::{DecodeError, Reader};

#[cfg(test)]
mod tests;

/// LC-01: the magic of the locked frame.
pub const MAGIC_LOCKED: [u8; 4] = *b"OBMx";
/// Bytes of salt.
pub const SALT_LEN: usize = 16;
/// Bytes of nonce.
pub const NONCE_LEN: usize = 12;
/// The frame without a generation: the owner app's, and the Key app's
/// before SR-92.
const VERSION_PLAIN: u8 = 1;
/// The frame whose sealed bytes start with the generation.
const VERSION_GENERATION: u8 = 2;
/// Bytes of the generation, the first sealed bytes of a version 2 frame.
const GENERATION_LEN: usize = 8;
/// Bytes of the AEAD's tag.
const TAG_LEN: usize = 16;
/// Bytes of the header: magic, version, three parameters, salt, nonce and
/// the length of the ciphertext.
pub const HEADER_LEN: usize = 4 + 1 + 12 + SALT_LEN + NONCE_LEN + 4;
/// The most a ciphertext may hold: the Key app's stored state is at most
/// 64 MiB of snapshot and a little more.
const MAX_CIPHERTEXT: usize = 80 * 1024 * 1024;

/// A PIN is at least this many digits.
pub const MIN_PIN_DIGITS: usize = 6;
/// A passphrase is at least this many characters.
pub const MIN_PASSPHRASE_CHARS: usize = 8;
/// The longest PIN or passphrase, in characters.
pub const MAX_PIN_CHARS: usize = 128;

/// LC-02: Argon2id parameters.
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
    /// LC-02: 64 MiB, 3 passes, 1 lane.
    pub const STANDARD: Self = Self {
        m_kib: 64 * 1024,
        t: 3,
        p: 1,
    };
    /// LC-02: the most a file may ask for: 256 MiB, 10 passes, 4 lanes.
    pub const LIMIT: Self = Self {
        m_kib: 256 * 1024,
        t: 10,
        p: 4,
    };
    /// LC-07: small parameters for tests and the fuzzer. Only [`cost!`]
    /// uses them, and only under `cfg(test)` and `cfg(fuzzing)`.
    #[doc(hidden)]
    pub const CHEAP: Self = Self {
        m_kib: 64,
        t: 1,
        p: 1,
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

/// LC-07: what an app asks of Argon2: the standard parameters, and small
/// ones in the tests of the crate that expands it and under the fuzzer
/// (`cfg(fuzzing)` is set by cargo-fuzz only), so that they run quickly. A
/// release build has neither. The `cfg` is read in the crate that expands
/// the macro, so each app library and this crate answer for their own build.
#[macro_export]
macro_rules! cost {
    () => {{
        #[cfg(not(any(test, fuzzing)))]
        let params = $crate::KdfParams::STANDARD;
        #[cfg(any(test, fuzzing))]
        let params = $crate::KdfParams::CHEAP;
        params
    }};
}

/// What the header of a locked frame says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedHeader {
    /// The frame's version, 1 or 2.
    pub version: u8,
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
        let mut out = Vec::with_capacity(HEADER_LEN);
        out.extend_from_slice(&MAGIC_LOCKED);
        out.push(self.version);
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

/// LC-01: read a locked frame, strictly. The parameters are checked against
/// the limit here, before anything runs Argon2.
pub fn decode(bytes: &[u8]) -> Result<LockedFrame<'_>, DecodeError> {
    let (mut r, version) = Reader::framed_up_to(bytes, &MAGIC_LOCKED, VERSION_GENERATION)?;
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
    let least = if version == VERSION_GENERATION {
        TAG_LEN + GENERATION_LEN
    } else {
        TAG_LEN
    };
    if n < least {
        return Err(DecodeError::Truncated);
    }
    let ciphertext = r.take(n)?;
    r.finish()?;
    Ok(LockedFrame {
        header: LockedHeader {
            version,
            params,
            salt,
            nonce,
            body_len,
        },
        ciphertext,
    })
}

/// Whether `bytes` start as a locked frame.
pub fn is_locked(bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(&MAGIC_LOCKED[..])
}

/// LC-03: why a PIN is not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinRefusal {
    /// `pin-long` or `pin-weak`.
    pub rule: &'static str,
    /// What the rule asks for.
    pub text: String,
}

impl std::fmt::Display for PinRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.rule, self.text)
    }
}

impl std::error::Error for PinRefusal {}

/// LC-03: a PIN is at least 6 digits, or a passphrase of at least 8
/// characters, and at most 128 characters in either case.
pub fn check_pin(pin: &str) -> Result<(), PinRefusal> {
    let chars = pin.chars().count();
    if chars > MAX_PIN_CHARS {
        return Err(PinRefusal {
            rule: "pin-long",
            text: format!("a PIN or passphrase is at most {MAX_PIN_CHARS} characters"),
        });
    }
    let digits = chars >= MIN_PIN_DIGITS && pin.bytes().all(|b| b.is_ascii_digit());
    if digits || chars >= MIN_PASSPHRASE_CHARS {
        return Ok(());
    }
    Err(PinRefusal {
        rule: "pin-weak",
        text: format!(
            "a PIN is at least {MIN_PIN_DIGITS} digits, or a passphrase of at least \
             {MIN_PASSPHRASE_CHARS} characters"
        ),
    })
}

/// The key that locks the stored bytes, and what is needed to write with
/// it. Overwritten when dropped.
pub struct LockKey {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key: [u8; 32],
    /// The nonce of the last frame written or read under this key.
    last_nonce: Option<[u8; NONCE_LEN]>,
    /// The generation of the state this key opened or last wrote (SR-92).
    generation: u64,
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
    /// A key for `pin` under a new salt. `None` when the parameters are
    /// outside the limit.
    pub fn new(params: KdfParams, pin: &str, salt: [u8; SALT_LEN]) -> Option<Self> {
        params.within_limit().ok()?;
        Some(Self {
            params,
            salt,
            key: derive(&params, pin, &salt)?,
            last_nonce: None,
            generation: 0,
        })
    }

    /// Whether `pin` gives this key.
    pub fn matches(&self, pin: &str) -> bool {
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

    /// The generation of the state this key opened or last wrote (SR-92).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Remember `generation` as the one this key wrote last, once the frame
    /// that holds it is stored.
    pub fn set_generation(&mut self, generation: u64) {
        self.generation = generation;
    }

    /// The generation in `stored`, a locked frame under this key: 0 for a
    /// frame of version 1. `WrongPin` when the frame is not under this key
    /// or was changed, so that it is not the state this key was made for.
    pub fn stored_generation(&self, stored: &[u8]) -> Result<u64, OpenError> {
        let frame = decode(stored).map_err(OpenError::Unreadable)?;
        let mut plain = self.open_frame(&frame).ok_or(OpenError::WrongPin)?;
        let found = split_generation(frame.header.version, &mut plain);
        plain.fill(0);
        std::hint::black_box(&mut plain);
        found.ok_or(OpenError::WrongPin)
    }

    /// The frame that holds `plain` under `version` and `nonce`.
    fn seal(&self, version: u8, nonce: [u8; NONCE_LEN], plain: &[u8]) -> Option<Vec<u8>> {
        let body_len = u32::try_from(plain.len().checked_add(TAG_LEN)?).ok()?;
        let header = LockedHeader {
            version,
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

    /// LC-04: the frame that holds `plain`, with a nonce that is not the
    /// last one this key used. The nonce is remembered before the frame is
    /// returned, so that a write that fails after this still never repeats
    /// it.
    pub fn seal_next(&mut self, nonce: [u8; NONCE_LEN], plain: &[u8]) -> Option<Vec<u8>> {
        self.seal_next_as(VERSION_PLAIN, nonce, plain)
    }

    /// SR-92: [`seal_next`](Self::seal_next) in a version 2 frame, with
    /// `generation` first inside the sealed bytes. The key's own generation
    /// is not changed: the caller sets it once the frame is stored.
    pub fn seal_generation(
        &mut self,
        nonce: [u8; NONCE_LEN],
        generation: u64,
        plain: &[u8],
    ) -> Option<Vec<u8>> {
        let mut inside = Vec::with_capacity(GENERATION_LEN.checked_add(plain.len())?);
        inside.extend_from_slice(&generation.to_le_bytes());
        inside.extend_from_slice(plain);
        let sealed = self.seal_next_as(VERSION_GENERATION, nonce, &inside);
        inside.fill(0);
        std::hint::black_box(&mut inside);
        sealed
    }

    fn seal_next_as(
        &mut self,
        version: u8,
        nonce: [u8; NONCE_LEN],
        plain: &[u8],
    ) -> Option<Vec<u8>> {
        if self.last_nonce == Some(nonce) {
            return None;
        }
        self.last_nonce = Some(nonce);
        self.seal(version, nonce, plain)
    }
}

/// The generation at the front of `plain`, taken off it: 0 and nothing taken
/// for a frame of version 1. `None` when a version 2 frame is too short.
fn split_generation(version: u8, plain: &mut Vec<u8>) -> Option<u64> {
    if version != VERSION_GENERATION {
        return Some(0);
    }
    let head = <[u8; GENERATION_LEN]>::try_from(plain.get(..GENERATION_LEN)?).ok()?;
    plain.drain(..GENERATION_LEN);
    Some(u64::from_le_bytes(head))
}

/// Why a locked frame did not open.
#[derive(Debug)]
pub enum OpenError {
    /// The bytes are not a locked frame.
    Unreadable(DecodeError),
    /// The PIN is wrong, or the ciphertext was changed.
    WrongPin,
}

/// LC-05: open a locked frame with the PIN. Returns the bytes inside and the
/// key, ready for the writes that follow. The key remembers the frame's
/// generation (SR-92); the bytes inside do not include it.
pub fn open(bytes: &[u8], pin: &str) -> Result<(Vec<u8>, LockKey), OpenError> {
    let frame = decode(bytes).map_err(OpenError::Unreadable)?;
    let mut key =
        LockKey::new(frame.header.params, pin, frame.header.salt).ok_or(OpenError::WrongPin)?;
    let mut plain = key.open_frame(&frame).ok_or(OpenError::WrongPin)?;
    let Some(generation) = split_generation(frame.header.version, &mut plain) else {
        plain.fill(0);
        return Err(OpenError::WrongPin);
    };
    key.generation = generation;
    key.last_nonce = Some(frame.header.nonce);
    Ok((plain, key))
}

/// LC-06: the stored frame again under another key, for a change of PIN.
/// `WrongPin` when `old` is not the key of `stored`. A frame of version 1
/// stays as it is; a frame of version 2 keeps its generation raised by one
/// (SR-92), and the caller sets it on `new` once the frame is stored.
pub fn reseal(
    stored: &[u8],
    old: &LockKey,
    new: &mut LockKey,
    nonce: [u8; NONCE_LEN],
) -> Result<Vec<u8>, OpenError> {
    let frame = decode(stored).map_err(OpenError::Unreadable)?;
    let mut plain = old.open_frame(&frame).ok_or(OpenError::WrongPin)?;
    if frame.header.version == VERSION_GENERATION {
        let raised = split_generation(frame.header.version, &mut plain)
            .and_then(|g| g.checked_add(1))
            .and_then(|g| new.seal_generation(nonce, g, &plain));
        plain.fill(0);
        std::hint::black_box(&mut plain);
        return raised.ok_or(OpenError::WrongPin);
    }
    let sealed = new.seal_next(nonce, &plain);
    plain.fill(0);
    std::hint::black_box(&mut plain);
    sealed.ok_or(OpenError::WrongPin)
}

/// Why bytes were not locked, or not locked again.
#[derive(Debug)]
pub enum LockError {
    /// The PIN is not one the rules take (LC-03).
    Pin(PinRefusal),
    /// The current PIN is wrong, or the ciphertext was changed (LC-05).
    WrongPin,
    /// The frame to change is not a locked frame (LC-01).
    Unreadable(DecodeError),
    /// The key could not be made, or the nonce repeats the frame's own:
    /// nothing was locked.
    Fault(&'static str),
}

/// LC-06: bytes locked under a new PIN: `plain` in a new frame, under
/// `salt` and `nonce` the caller drew from the platform's random source.
pub fn lock(
    params: KdfParams,
    pin: &str,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
    plain: &[u8],
) -> Result<Vec<u8>, LockError> {
    check_pin(pin).map_err(LockError::Pin)?;
    let mut key =
        LockKey::new(params, pin, salt).ok_or(LockError::Fault("the key could not be made"))?;
    key.seal_next(nonce, plain)
        .ok_or(LockError::Fault("the bytes could not be locked"))
}

/// LC-05: the bytes inside a locked frame, for the PIN.
pub fn unlock(bytes: &[u8], pin: &str) -> Result<Vec<u8>, OpenError> {
    open(bytes, pin).map(|(plain, _)| plain)
}

/// LC-06: a locked frame under another PIN, in one step: the current PIN
/// opens it, the new PIN is checked, and the same bytes are locked again
/// under a new salt and a new nonce. Nothing is returned unless all three
/// worked. A salt and a nonce that are the frame's own are refused.
pub fn change(
    stored: &[u8],
    current: &str,
    params: KdfParams,
    new: &str,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
) -> Result<Vec<u8>, LockError> {
    let frame = decode(stored).map_err(LockError::Unreadable)?;
    if frame.header.salt == salt || frame.header.nonce == nonce {
        return Err(LockError::Fault(
            "the salt or the nonce repeats the frame's",
        ));
    }
    let (mut plain, _) = open(stored, current).map_err(|e| match e {
        OpenError::WrongPin => LockError::WrongPin,
        OpenError::Unreadable(e) => LockError::Unreadable(e),
    })?;
    let result = lock(params, new, salt, nonce, &plain);
    plain.fill(0);
    std::hint::black_box(&mut plain);
    result
}
