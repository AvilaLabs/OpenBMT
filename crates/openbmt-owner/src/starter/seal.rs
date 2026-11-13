//! SS-20, SS-23: the seed sealed with ChaCha20-Poly1305 under a random
//! 32-byte data key, and the restore from 12 typed words.
//!
//! The sealed seed is `1 || nonce (12) || ciphertext and tag (32)`; the
//! additional data is `OpenBMT/starter/seal/v1`. The data key is given back
//! once, at the sealing, for the app to wrap with its Keystore key; every
//! later call is given it again as bytes and wipes its copy.

use std::fmt;

use bip39::{Language, Mnemonic};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

use super::chance::{checked_read, KernelRandom, Seed, StarterWallet, ENTROPY_BYTES};
use super::setup::{Key as PadKey, Pad, Typed};
use super::{wipe, StarterRefusal, STARTER_WORDS};

/// SS-20: bytes of the data key.
pub const DATA_KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const SEAL_VERSION: u8 = 1;
const SEAL_AAD: &[u8] = b"OpenBMT/starter/seal/v1";
/// FL-03: the refund float's seed is sealed the same way under a label of its
/// own, so a starter blob never opens as a float's and the reverse.
pub(crate) const FLOAT_SEAL_AAD: &[u8] = b"OpenBMT/float/seal/v1";

/// The data key, in memory only while a call uses it. The buffer is
/// overwritten when the value is dropped, and `Debug` hides it.
pub struct DataKey([u8; DATA_KEY_BYTES]);

impl DataKey {
    /// The data key of 32 bytes the app handed over.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, StarterRefusal> {
        <[u8; DATA_KEY_BYTES]>::try_from(bytes)
            .map(Self)
            .map_err(|_| StarterRefusal::DataKey)
    }

    /// The bytes, for the app to wrap with its Keystore key.
    pub fn as_bytes(&self) -> &[u8; DATA_KEY_BYTES] {
        &self.0
    }
}

impl fmt::Debug for DataKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DataKey(<hidden>)")
    }
}

impl Drop for DataKey {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// What sealing gives: the sealed seed, which the app stores, and the data
/// key, which it wraps with the Keystore at once and then overwrites.
#[derive(Debug)]
pub struct Sealed {
    /// The sealed seed.
    pub sealed: Vec<u8>,
    /// The data key that opens it.
    pub data_key: DataKey,
}

/// SS-20: the seed sealed under a new random data key and a new random
/// nonce, both from checked kernel reads (SS-02).
pub(crate) fn seal_seed(seed: &Seed, rng: &mut dyn KernelRandom) -> Result<Sealed, StarterRefusal> {
    seal_seed_with(seed, rng, SEAL_AAD)
}

/// [`seal_seed`] under the label `aad`.
pub(crate) fn seal_seed_with(
    seed: &Seed,
    rng: &mut dyn KernelRandom,
    aad: &'static [u8],
) -> Result<Sealed, StarterRefusal> {
    let mut key_bytes = checked_read(rng)?;
    let data_key = DataKey(key_bytes);
    wipe(&mut key_bytes);
    let mut nonce_bytes = checked_read(rng)?;
    let nonce: [u8; NONCE_BYTES] = {
        let mut n = [0u8; NONCE_BYTES];
        n.copy_from_slice(nonce_bytes.get(..NONCE_BYTES).unwrap_or(&[0; NONCE_BYTES]));
        n
    };
    wipe(&mut nonce_bytes);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&data_key.0));
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: seed.entropy(),
                aad,
            },
        )
        .map_err(|_| StarterRefusal::Open)?;
    let mut sealed = Vec::with_capacity(1 + NONCE_BYTES + ct.len());
    sealed.push(SEAL_VERSION);
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ct);
    Ok(Sealed { sealed, data_key })
}

/// SS-20: open a sealed seed. A wrong key, a changed byte or a blob of
/// another shape all give [`StarterRefusal::Open`].
pub(crate) fn open_seed(sealed: &[u8], key: &DataKey) -> Result<Seed, StarterRefusal> {
    open_seed_with(sealed, key, SEAL_AAD)
}

/// [`open_seed`] under the label `aad`.
pub(crate) fn open_seed_with(
    sealed: &[u8],
    key: &DataKey,
    aad: &'static [u8],
) -> Result<Seed, StarterRefusal> {
    let (version, rest) = sealed.split_first().ok_or(StarterRefusal::Open)?;
    if *version != SEAL_VERSION || rest.len() != NONCE_BYTES + ENTROPY_BYTES + TAG_BYTES {
        return Err(StarterRefusal::Open);
    }
    let (nonce, ct) = rest.split_at(NONCE_BYTES);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key.0));
    let mut plain = cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| StarterRefusal::Open)?;
    let out = <[u8; ENTROPY_BYTES]>::try_from(plain.as_slice())
        .map(Seed::from_entropy)
        .map_err(|_| StarterRefusal::Open);
    wipe(&mut plain);
    out
}

/// SS-20: the public wallet inside a sealed seed, to check that the data
/// key opens it (the app keeps the description too).
pub fn wallet_of_sealed(sealed: &[u8], key: &DataKey) -> Result<StarterWallet, StarterRefusal> {
    open_seed(sealed, key)?.wallet()
}

/// SS-23: the restore. The owner types the 12 words on the in-app keypad;
/// this holds the accepted words (as entries of the word list, never text
/// the app sent) until [`Restore::finish`].
#[derive(Default)]
pub struct Restore {
    words: Vec<&'static str>,
    pad: Pad,
}

impl fmt::Debug for Restore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Restore({} words)", self.words.len())
    }
}

impl Restore {
    /// A restore with no word typed.
    pub fn new() -> Self {
        Self::default()
    }

    /// SS-11, SS-23: a key of the keypad. A word is accepted when the
    /// letters are one word of the list, whole or by the unique start of
    /// it; enter takes it. Backspace with no letter typed takes back the
    /// last word.
    pub fn key(&mut self, key: PadKey) -> Result<Typed, StarterRefusal> {
        match key {
            PadKey::Letter(l) => self.pad.push(l),
            PadKey::Backspace => {
                if !self.pad.pop() {
                    self.words.pop();
                }
            }
            PadKey::Enter => {
                if self.words.len() < STARTER_WORDS {
                    if let Some(word) = self.pad.complete() {
                        self.words.push(word);
                        self.pad.clear();
                    }
                }
            }
        }
        Ok(Typed {
            typed: self.pad.len(),
            accepted: self.words.len() < STARTER_WORDS && self.pad.complete().is_some(),
            step: self.words.len(),
            finished: self.words.len() == STARTER_WORDS,
            failed: false,
        })
    }

    /// SS-23: check the 12 words (the list and the check word) against the
    /// starter wallet's stored `description` (its fingerprint and key), and
    /// seal them again under a new data key. The words stay typed when they are
    /// refused.
    pub fn finish(
        &self,
        description: &str,
        rng: &mut dyn KernelRandom,
    ) -> Result<Sealed, StarterRefusal> {
        if self.words.len() != STARTER_WORDS {
            return Err(StarterRefusal::Incomplete);
        }
        let mut typed = self.words.join(" ");
        let parsed = Mnemonic::parse_in_normalized(Language::English, &typed);
        let mut joined = std::mem::take(&mut typed).into_bytes();
        wipe(&mut joined);
        let mnemonic = parsed.map_err(|_| StarterRefusal::Words)?;
        let seed = Seed::from_mnemonic(&mnemonic)?;
        let wallet = seed.wallet()?;
        let stored = StarterWallet::parse(description)?;
        if wallet != stored {
            return Err(StarterRefusal::OtherWallet);
        }
        seal_seed(&seed, rng)
    }
}
