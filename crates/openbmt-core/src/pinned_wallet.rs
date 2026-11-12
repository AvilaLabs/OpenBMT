//! The wallet a counter pins at pairing, and the index ranges the owner
//! phone gives it (`docs/WALLET-RULES.md`, WL-20 to WL-28).
//!
//! A counter that is paired the new way holds the public description of the
//! owner's wallet and derives its own receive addresses from it, so no
//! signed batch of scripts is needed. [`PinnedWallet`] is that description,
//! for the single-key account and for the 2-of-3 wallet alike, with the
//! derivation of each kind used as it is.
//!
//! The owner phone still says which indexes a counter may use. An
//! [`IndexRange`] names a counter and a run of receive indexes;
//! [`SignedRange`] carries the owner root's signature over it. A counter
//! keeps a [`RangeBook`] of the ranges it has taken and the indexes it has
//! used, and refuses a range that would show an address twice. Nothing here
//! touches a disk or a clock.
//!
//! Wire forms (strict, see [`crate::wire`]):
//!
//! ```text
//! pinned: "OBM)" | 1 | text len u16 LE | description text | sha256 (32)
//! range:  "OBM*" | 1 | counter (32) | start u32 LE | end u32 LE | sig (64)
//! ```
//!
//! The text of a pinned wallet is the canonical string of the description,
//! checksum included, and the hash is the wallet's `checksum32`. Decoding a
//! pinned wallet reads the text again through [`ImportedWallet::read`]: it
//! takes nothing that the import refuses. Decoding a range checks its shape
//! but not its signature: call [`SignedRange::verify`].

use std::fmt;

use bitcoin::{Address, ScriptBuf};

use crate::identity::{DeviceId, Identity, Sig};
use crate::wallet::Keychain;
use crate::wallet_single::{ImportRefused, ImportedWallet, MAX_IMPORT_BYTES};
use crate::wire::{self, DecodeError, Reader};

/// Magic of a [`PinnedWallet`].
pub const MAGIC_PINNED_WALLET: [u8; 4] = *b"OBM)";
/// Magic of a [`SignedRange`].
pub const MAGIC_SIGNED_RANGE: [u8; 4] = *b"OBM*";
/// Tag of the owner root's signature over an [`IndexRange`].
pub const TAG_INDEX_RANGE: &str = "OpenBMT/index-range/v1";

/// The first hardened child index: no range reaches it.
const HARDENED_LIMIT: u32 = 1 << 31;
/// The most indexes in one range.
pub const MAX_RANGE_LEN: u32 = 1_000_000;

/// Why a pinned wallet could not be made, read or used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PinnedError {
    /// The text is not a wallet the import accepts.
    Import(ImportRefused),
    /// The bytes are not a pinned wallet.
    Decode(DecodeError),
    /// The text is a wallet but not in its canonical spelling.
    NotCanonical,
    /// The stored hash is not the hash of the text.
    ChecksumMismatch,
    /// The index is not a valid non-hardened child index, or derivation
    /// failed.
    Derive,
}

impl fmt::Display for PinnedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Import(why) => write!(f, "WL-22: {why}"),
            Self::Decode(why) => write!(f, "WL-21: {why}"),
            Self::NotCanonical => write!(f, "WL-22: the text is not in its canonical form"),
            Self::ChecksumMismatch => write!(f, "WL-22: the hash does not match the text"),
            Self::Derive => write!(f, "WL-20: no address at that index"),
        }
    }
}

impl std::error::Error for PinnedError {}

impl From<DecodeError> for PinnedError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

/// The wallet a counter derives its addresses from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedWallet {
    wallet: ImportedWallet,
}

impl PinnedWallet {
    /// Pin a wallet the owner brought in.
    pub fn new(wallet: ImportedWallet) -> Self {
        Self { wallet }
    }

    /// Read description text and pin the wallet it describes.
    pub fn from_text(text: &str) -> Result<Self, PinnedError> {
        ImportedWallet::read(text)
            .map(Self::new)
            .map_err(PinnedError::Import)
    }

    /// The wallet.
    pub fn wallet(&self) -> &ImportedWallet {
        &self.wallet
    }

    /// The canonical description text, checksum included.
    pub fn canonical_string(&self) -> &str {
        match &self.wallet {
            ImportedWallet::Single(w) => w.canonical_string(),
            ImportedWallet::Multisig(w) => w.canonical_string(),
        }
    }

    /// SHA-256 of the canonical text.
    pub fn checksum32(&self) -> [u8; 32] {
        match &self.wallet {
            ImportedWallet::Single(w) => w.checksum32(),
            ImportedWallet::Multisig(w) => w.checksum32(),
        }
    }

    /// The receive script at `index`, by the derivation of the wallet's kind.
    pub fn receive_script(&self, index: u32) -> Result<ScriptBuf, PinnedError> {
        match &self.wallet {
            ImportedWallet::Single(w) => w.receive_script(index).map_err(|_| PinnedError::Derive),
            ImportedWallet::Multisig(w) => w.receive_script(index).map_err(|_| PinnedError::Derive),
        }
    }

    /// The receive address at `index`.
    pub fn receive_address(&self, index: u32) -> Result<Address, PinnedError> {
        match &self.wallet {
            ImportedWallet::Single(w) => w
                .address(Keychain::Receive, index)
                .map_err(|_| PinnedError::Derive),
            ImportedWallet::Multisig(w) => w
                .address(Keychain::Receive, index)
                .map_err(|_| PinnedError::Derive),
        }
    }

    /// Wire form: `OBM)`, version 1, the text length, the text, its hash.
    pub fn to_bytes(&self) -> Vec<u8> {
        let text = self.canonical_string().as_bytes();
        let mut out = wire::start(&MAGIC_PINNED_WALLET, text.len() + 34);
        // The text is at most the import limit, which fits a u16.
        out.extend_from_slice(&u16::try_from(text.len()).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(text);
        out.extend_from_slice(&self.checksum32());
        out
    }

    /// Parse the wire form strictly. The text goes through the import, so
    /// nothing the import refuses is taken, and it must be the canonical
    /// spelling with the hash that goes with it.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PinnedError> {
        let mut r = Reader::framed(bytes, &MAGIC_PINNED_WALLET)?;
        let len = usize::from(r.u16_le()?);
        if len > MAX_IMPORT_BYTES {
            return Err(DecodeError::TooLarge.into());
        }
        let text = std::str::from_utf8(r.take(len)?)
            .map_err(|_| DecodeError::NonCanonical("text is not UTF-8"))?
            .to_owned();
        let stored: [u8; 32] = r.array()?;
        r.finish()?;
        let pinned = Self::from_text(&text)?;
        if pinned.canonical_string() != text {
            return Err(PinnedError::NotCanonical);
        }
        if pinned.checksum32() != stored {
            return Err(PinnedError::ChecksumMismatch);
        }
        Ok(pinned)
    }
}

/// Why an index range was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    /// The range has no index in it.
    Empty,
    /// The range reaches the hardened indexes.
    PastHardened,
    /// The range is longer than [`MAX_RANGE_LEN`].
    TooLong,
    /// The signature is not the owner root's over this range.
    BadSignature,
    /// The range is for another counter.
    WrongCounter,
    /// The range overlaps an index this counter has used.
    OverlapsUsed,
    /// The range overlaps a range already taken that still has unused
    /// indexes.
    OverlapsTaken,
    /// Every index of every range taken is used, or no range is taken.
    UsedUp,
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "WL-23: the range has no index"),
            Self::PastHardened => write!(f, "WL-23: the range reaches the hardened indexes"),
            Self::TooLong => write!(f, "WL-23: the range is longer than {MAX_RANGE_LEN}"),
            Self::BadSignature => write!(f, "WL-24: the range is not signed by the owner"),
            Self::WrongCounter => write!(f, "WL-28: the range is for another counter"),
            Self::OverlapsUsed => write!(f, "WL-25: the range overlaps an index already used"),
            Self::OverlapsTaken => write!(f, "WL-26: the range overlaps a range still in use"),
            Self::UsedUp => write!(f, "WL-27: no unused index is left"),
        }
    }
}

impl std::error::Error for RangeError {}

/// A run of receive indexes for one counter: `start` up to, not including,
/// `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexRange {
    /// The counter the range is for.
    pub counter: DeviceId,
    /// The first index.
    pub start: u32,
    /// One past the last index.
    pub end: u32,
}

impl IndexRange {
    /// A range, if it is not empty, stays below the hardened limit and is at
    /// most [`MAX_RANGE_LEN`] long.
    pub fn new(counter: DeviceId, start: u32, end: u32) -> Result<Self, RangeError> {
        let range = Self {
            counter,
            start,
            end,
        };
        range.check_shape()?;
        Ok(range)
    }

    fn check_shape(&self) -> Result<(), RangeError> {
        if self.end <= self.start {
            return Err(RangeError::Empty);
        }
        if self.end >= HARDENED_LIMIT {
            return Err(RangeError::PastHardened);
        }
        if self.end - self.start > MAX_RANGE_LEN {
            return Err(RangeError::TooLong);
        }
        Ok(())
    }

    /// The bytes that are signed: counter, start, end.
    fn body(&self) -> [u8; 40] {
        let mut out = [0u8; 40];
        out[..32].copy_from_slice(&self.counter.to_bytes());
        out[32..36].copy_from_slice(&self.start.to_le_bytes());
        out[36..].copy_from_slice(&self.end.to_le_bytes());
        out
    }

    fn overlaps(&self, start: u32, end: u32) -> bool {
        self.start < end && start < self.end
    }
}

/// An [`IndexRange`] with the owner root's signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedRange {
    /// The range.
    pub range: IndexRange,
    /// The owner root's signature over the range.
    pub sig: Sig,
}

impl SignedRange {
    /// Sign a range with the owner root. A range of the wrong shape is
    /// refused.
    pub fn sign(owner: &Identity, range: IndexRange) -> Result<Self, RangeError> {
        range.check_shape()?;
        let sig = owner.sign_tagged(TAG_INDEX_RANGE, &range.body());
        Ok(Self { range, sig })
    }

    /// Check the shape and the signature against the pinned owner root.
    pub fn verify(&self, owner: &DeviceId) -> Result<(), RangeError> {
        self.range.check_shape()?;
        if owner.verify_tagged(TAG_INDEX_RANGE, &self.range.body(), &self.sig) {
            Ok(())
        } else {
            Err(RangeError::BadSignature)
        }
    }

    /// Wire form: `OBM*`, version 1, counter, start, end, signature.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_SIGNED_RANGE, 104);
        out.extend_from_slice(&self.range.body());
        out.extend_from_slice(&self.sig.0);
        out
    }

    /// Parse the wire form strictly. The shape is checked, the signature is
    /// not: call [`SignedRange::verify`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_SIGNED_RANGE)?;
        let counter = r.device_id()?;
        let start = r.u32_le()?;
        let end = r.u32_le()?;
        let sig = r.sig()?;
        r.finish()?;
        let range = IndexRange::new(counter, start, end).map_err(|e| match e {
            RangeError::Empty => DecodeError::NonCanonical("range is empty"),
            RangeError::PastHardened => DecodeError::NonCanonical("range reaches hardened"),
            _ => DecodeError::TooLarge,
        })?;
        Ok(Self { range, sig })
    }
}

/// A range taken, and the first index of it not yet used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Taken {
    start: u32,
    end: u32,
    next: u32,
}

/// The ranges one counter has taken and the indexes it has used in them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeBook {
    counter: DeviceId,
    taken: Vec<Taken>,
}

impl RangeBook {
    /// An empty book for `counter`.
    pub fn new(counter: DeviceId) -> Self {
        Self {
            counter,
            taken: Vec::new(),
        }
    }

    /// Take a range. It must be for this counter, must not overlap an index
    /// already used, and must not overlap a range already taken that still
    /// has unused indexes. On refusal the book is unchanged.
    pub fn accept(&mut self, range: &IndexRange) -> Result<(), RangeError> {
        range.check_shape()?;
        if range.counter != self.counter {
            return Err(RangeError::WrongCounter);
        }
        if self
            .taken
            .iter()
            .any(|t| t.next > t.start && range.overlaps(t.start, t.next))
        {
            return Err(RangeError::OverlapsUsed);
        }
        if self
            .taken
            .iter()
            .any(|t| t.next < t.end && range.overlaps(t.start, t.end))
        {
            return Err(RangeError::OverlapsTaken);
        }
        self.taken.push(Taken {
            start: range.start,
            end: range.end,
            next: range.start,
        });
        Ok(())
    }

    /// Check the owner's signature, then take the range.
    pub fn accept_signed(
        &mut self,
        signed: &SignedRange,
        owner: &DeviceId,
    ) -> Result<(), RangeError> {
        signed.verify(owner)?;
        self.accept(&signed.range)
    }

    /// Hand out the next unused index of the current range (the first taken
    /// range with an index left) and mark it used.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<u32, RangeError> {
        let slot = self
            .taken
            .iter_mut()
            .find(|t| t.next < t.end)
            .ok_or(RangeError::UsedUp)?;
        let index = slot.next;
        slot.next += 1;
        Ok(index)
    }

    /// How many unused indexes are left over all ranges taken.
    pub fn remaining(&self) -> u64 {
        self.taken.iter().map(|t| u64::from(t.end - t.next)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{assert_strict_decoding, identity, wallet};
    use crate::wallet::descriptor_checksum;
    use crate::wallet_single::SingleKeyWallet;
    use bitcoin::bip32::{Fingerprint, Xpub};
    use bitcoin::Network;
    use std::str::FromStr;

    const RULES: &str = include_str!("../../../docs/WALLET-RULES.md");
    const SOURCE: &str = include_str!("pinned_wallet.rs");

    /// The Key Maker's test seed (fingerprint `818bc9b6`), account 0.
    fn single() -> SingleKeyWallet {
        let xpub = Xpub::from_str(
            "xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8",
        )
        .unwrap();
        SingleKeyWallet::new(Fingerprint::from_str("818bc9b6").unwrap(), 0, xpub).unwrap()
    }

    fn pinned_single() -> PinnedWallet {
        PinnedWallet::new(ImportedWallet::Single(single()))
    }

    fn pinned_multi() -> PinnedWallet {
        PinnedWallet::new(ImportedWallet::Multisig(wallet(Network::Bitcoin)))
    }

    fn range(counter: u8, start: u32, end: u32) -> IndexRange {
        IndexRange::new(identity(counter).device_id(), start, end).unwrap()
    }

    fn signed(start: u32, end: u32) -> SignedRange {
        SignedRange::sign(&identity(1), range(3, start, end)).unwrap()
    }

    #[test]
    fn wl_20_single_key_addresses_are_the_single_key_derivation() {
        let direct = single();
        let pinned = pinned_single();
        for i in [0, 1, 999] {
            assert_eq!(
                pinned.receive_script(i).unwrap(),
                direct.receive_script(i).unwrap()
            );
            assert_eq!(
                pinned.receive_address(i).unwrap(),
                direct.address(Keychain::Receive, i).unwrap()
            );
        }
        assert_eq!(
            pinned.receive_address(0).unwrap().to_string(),
            "bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj"
        );
        assert_eq!(pinned.receive_script(0).unwrap().len(), 22);
    }

    #[test]
    fn wl_20_two_of_three_addresses_are_the_description_derivation() {
        let direct = wallet(Network::Bitcoin);
        let pinned = pinned_multi();
        for i in [0, 1, 999] {
            assert_eq!(
                pinned.receive_script(i).unwrap(),
                direct.receive_script(i).unwrap()
            );
            assert_eq!(
                pinned.receive_address(i).unwrap(),
                direct.address(Keychain::Receive, i).unwrap()
            );
        }
        assert_eq!(pinned.receive_script(0).unwrap().len(), 34);
        assert_eq!(pinned.receive_script(1 << 31), Err(PinnedError::Derive));
        assert_eq!(
            pinned_single().receive_address(1 << 31),
            Err(PinnedError::Derive)
        );
    }

    #[test]
    fn wl_21_a_pinned_wallet_round_trips_strictly() {
        for p in [pinned_single(), pinned_multi()] {
            let bytes = p.to_bytes();
            assert_eq!(&bytes[..5], b"OBM)\x01");
            assert_eq!(PinnedWallet::from_bytes(&bytes).unwrap(), p);
            assert_strict_decoding(
                &bytes,
                |b| {
                    PinnedWallet::from_bytes(b).map_err(|e| match e {
                        PinnedError::Decode(d) => d,
                        _ => DecodeError::NonCanonical("x"),
                    })
                },
                PinnedWallet::to_bytes,
            );
        }
        let mut bad_magic = pinned_single().to_bytes();
        bad_magic[3] = b'*';
        assert_eq!(
            PinnedWallet::from_bytes(&bad_magic),
            Err(PinnedError::Decode(DecodeError::BadMagic))
        );
        let mut bad_version = pinned_single().to_bytes();
        bad_version[4] = 2;
        assert_eq!(
            PinnedWallet::from_bytes(&bad_version),
            Err(PinnedError::Decode(DecodeError::UnsupportedVersion(2)))
        );
        // A length above the import limit is refused before the text is read.
        let mut long = pinned_single().to_bytes();
        long[5..7].copy_from_slice(&5000u16.to_le_bytes());
        assert_eq!(
            PinnedWallet::from_bytes(&long),
            Err(PinnedError::Decode(DecodeError::TooLarge))
        );
    }

    /// Bytes of a pinned wallet with `text` in place of the description and
    /// the hash of that text.
    fn framed(text: &str) -> Vec<u8> {
        use bitcoin::hashes::{sha256, Hash};
        let mut out = wire::start(&MAGIC_PINNED_WALLET, text.len() + 34);
        out.extend_from_slice(&u16::try_from(text.len()).unwrap().to_le_bytes());
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&sha256::Hash::hash(text.as_bytes()).to_byte_array());
        out
    }

    #[test]
    fn wl_22_decoding_refuses_what_the_import_refuses() {
        let good = pinned_single().canonical_string().to_owned();
        assert!(PinnedWallet::from_bytes(&framed(&good)).is_ok());

        let refused = |text: &str| PinnedWallet::from_bytes(&framed(text)).unwrap_err();
        // A private key.
        let xprv = "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi";
        assert_eq!(
            refused(xprv),
            PinnedError::Import(ImportRefused::HasPrivateKey)
        );
        // A test network.
        let test = good.replace("/84h/0h/", "/84h/1h/");
        assert_eq!(
            refused(&test),
            PinnedError::Import(ImportRefused::TestNetwork)
        );
        // A damaged checksum.
        let mut damaged = good.clone();
        damaged.pop();
        damaged.push('q');
        assert_eq!(
            refused(&damaged),
            PinnedError::Import(ImportRefused::Damaged)
        );
        // Another type, and text that is not a wallet.
        let body = "pkh(03a34b99f22c790c4e36b2b3c2c35a36db06226e41c692fc82b8b56ac1c540c5bd)";
        let other = format!("{body}#{}", descriptor_checksum(body).unwrap());
        assert_eq!(
            refused(&other),
            PinnedError::Import(ImportRefused::Unsupported)
        );
        assert_eq!(
            refused("hello"),
            PinnedError::Import(ImportRefused::Damaged)
        );
    }

    #[test]
    fn wl_22_decoding_refuses_a_wrong_spelling_and_a_wrong_hash() {
        // The apostrophe spelling is accepted by the import but is not the
        // canonical string.
        let spelled = pinned_single()
            .canonical_string()
            .replace("/84h/0h/0h]", "/84'/0'/0']");
        let body = spelled.split('#').next().unwrap().to_owned();
        let text = format!("{body}#{}", descriptor_checksum(&body).unwrap());
        assert!(ImportedWallet::read(&text).is_ok());
        assert_eq!(
            PinnedWallet::from_bytes(&framed(&text)),
            Err(PinnedError::NotCanonical)
        );
        // The same wallet with another wallet's hash.
        let mut bytes = pinned_single().to_bytes();
        let n = bytes.len();
        bytes[n - 1] ^= 1;
        assert_eq!(
            PinnedWallet::from_bytes(&bytes),
            Err(PinnedError::ChecksumMismatch)
        );
        // Text that is not UTF-8.
        let mut raw = wire::start(&MAGIC_PINNED_WALLET, 40);
        raw.extend_from_slice(&1u16.to_le_bytes());
        raw.push(0xff);
        raw.extend_from_slice(&[0; 32]);
        assert!(matches!(
            PinnedWallet::from_bytes(&raw),
            Err(PinnedError::Decode(DecodeError::NonCanonical(_)))
        ));
    }

    #[test]
    fn wl_23_a_range_is_not_empty_below_hardened_and_not_too_long() {
        let c = identity(3).device_id();
        assert!(IndexRange::new(c, 0, 1).is_ok());
        assert!(IndexRange::new(c, 0, MAX_RANGE_LEN).is_ok());
        assert!(IndexRange::new(c, 5, HARDENED_LIMIT - 1).is_err());
        assert!(IndexRange::new(c, HARDENED_LIMIT - 1 - MAX_RANGE_LEN, HARDENED_LIMIT - 1).is_ok());
        assert_eq!(IndexRange::new(c, 5, 5), Err(RangeError::Empty));
        assert_eq!(IndexRange::new(c, 6, 5), Err(RangeError::Empty));
        assert_eq!(
            IndexRange::new(c, 0, MAX_RANGE_LEN + 1),
            Err(RangeError::TooLong)
        );
        assert_eq!(
            IndexRange::new(c, HARDENED_LIMIT - 10, HARDENED_LIMIT),
            Err(RangeError::PastHardened)
        );
        assert_eq!(
            IndexRange::new(c, 0, u32::MAX),
            Err(RangeError::PastHardened)
        );
        // Signing refuses a range of the wrong shape.
        let bad = IndexRange {
            counter: c,
            start: 4,
            end: 4,
        };
        assert_eq!(SignedRange::sign(&identity(1), bad), Err(RangeError::Empty));
    }

    #[test]
    fn wl_24_a_signed_range_checks_against_the_owner_root_only() {
        let s = signed(0, 100);
        assert_eq!(s.verify(&identity(1).device_id()), Ok(()));
        assert_eq!(
            s.verify(&identity(2).device_id()),
            Err(RangeError::BadSignature)
        );
        for t in [
            SignedRange {
                range: range(3, 0, 101),
                ..s
            },
            SignedRange {
                range: range(3, 1, 100),
                ..s
            },
            SignedRange {
                range: range(4, 0, 100),
                ..s
            },
        ] {
            assert_eq!(
                t.verify(&identity(1).device_id()),
                Err(RangeError::BadSignature)
            );
        }
        // The signature is under its own tag: another tag's signature over
        // the same bytes is not accepted.
        let other = SignedRange {
            sig: identity(1).sign_tagged("OpenBMT/batch/v1", &s.range.body()),
            ..s
        };
        assert_eq!(
            other.verify(&identity(1).device_id()),
            Err(RangeError::BadSignature)
        );
    }

    #[test]
    fn wl_24_a_signed_range_round_trips_strictly() {
        let s = signed(10, 20);
        let bytes = s.to_bytes();
        assert_eq!(bytes.len(), 109);
        assert_eq!(&bytes[..5], b"OBM*\x01");
        assert_eq!(SignedRange::from_bytes(&bytes), Ok(s));
        assert_strict_decoding(&bytes, SignedRange::from_bytes, SignedRange::to_bytes);
        let mut bad_magic = bytes.clone();
        bad_magic[3] = b')';
        assert_eq!(
            SignedRange::from_bytes(&bad_magic),
            Err(DecodeError::BadMagic)
        );
        // A range of the wrong shape does not decode.
        let mut empty = bytes.clone();
        empty[41..45].copy_from_slice(&10u32.to_le_bytes());
        assert!(SignedRange::from_bytes(&empty).is_err());
        let mut hardened = bytes.clone();
        hardened[41..45].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(SignedRange::from_bytes(&hardened).is_err());
        let mut long = bytes;
        long[41..45].copy_from_slice(&(10 + MAX_RANGE_LEN + 1).to_le_bytes());
        assert!(SignedRange::from_bytes(&long).is_err());
    }

    fn book() -> RangeBook {
        RangeBook::new(identity(3).device_id())
    }

    #[test]
    fn wl_25_a_range_over_a_used_index_is_refused() {
        let mut b = book();
        b.accept(&range(3, 0, 10)).unwrap();
        for _ in 0..4 {
            b.next().unwrap();
        }
        // Indexes 0 to 3 are used: any range touching them is refused.
        for (s, e) in [(0, 1), (3, 4), (2, 12), (0, 100)] {
            let before = b.clone();
            assert_eq!(b.accept(&range(3, s, e)), Err(RangeError::OverlapsUsed));
            assert_eq!(b, before);
        }
        // Used up entirely: the whole range stays refused.
        for _ in 4..10 {
            b.next().unwrap();
        }
        assert_eq!(b.accept(&range(3, 9, 20)), Err(RangeError::OverlapsUsed));
        assert_eq!(b.accept(&range(3, 10, 20)), Ok(()));
    }

    #[test]
    fn wl_26_a_range_over_a_range_still_in_use_is_refused() {
        let mut b = book();
        b.accept(&range(3, 100, 200)).unwrap();
        // Nothing used yet.
        for (s, e) in [(100, 101), (150, 160), (50, 101), (199, 300), (0, 1000)] {
            assert_eq!(b.accept(&range(3, s, e)), Err(RangeError::OverlapsTaken));
        }
        // Next to it is fine, and so is the same indexes once used.
        assert_eq!(b.accept(&range(3, 0, 100)), Ok(()));
        assert_eq!(b.accept(&range(3, 200, 300)), Ok(()));
        assert_eq!(
            b.accept(&range(3, 100, 200)),
            Err(RangeError::OverlapsTaken)
        );
        // Part used: the unused part is still refused, as taken.
        let mut c = book();
        c.accept(&range(3, 0, 10)).unwrap();
        c.next().unwrap();
        assert_eq!(c.accept(&range(3, 5, 6)), Err(RangeError::OverlapsTaken));
    }

    #[test]
    fn wl_27_next_hands_out_in_order_and_says_when_used_up() {
        let mut b = book();
        assert_eq!(b.next(), Err(RangeError::UsedUp));
        b.accept(&range(3, 5, 8)).unwrap();
        assert_eq!(b.remaining(), 3);
        assert_eq!((b.next(), b.next(), b.next()), (Ok(5), Ok(6), Ok(7)));
        assert_eq!(b.next(), Err(RangeError::UsedUp));
        assert_eq!(b.remaining(), 0);
        // A new range continues the run.
        b.accept(&range(3, 1000, 1002)).unwrap();
        assert_eq!(
            (b.next(), b.next(), b.next()),
            (Ok(1000), Ok(1001), Err(RangeError::UsedUp))
        );
        // Ranges taken before the first is used up are used in order.
        let mut c = book();
        c.accept(&range(3, 50, 52)).unwrap();
        c.accept(&range(3, 10, 12)).unwrap();
        let got: Vec<u32> = std::iter::from_fn(|| c.next().ok()).collect();
        assert_eq!(got, [50, 51, 10, 11]);
    }

    #[test]
    fn wl_28_a_range_for_another_counter_is_refused() {
        let mut b = book();
        assert_eq!(b.accept(&range(4, 0, 10)), Err(RangeError::WrongCounter));
        assert_eq!(b.remaining(), 0);
        let owner = identity(1).device_id();
        assert_eq!(b.accept_signed(&signed(0, 10), &owner), Ok(()));
        // A signature that does not check is refused before anything is taken.
        assert_eq!(
            b.accept_signed(&signed(20, 30), &identity(2).device_id()),
            Err(RangeError::BadSignature)
        );
        assert_eq!(b.remaining(), 10);
        let other = SignedRange::sign(&identity(1), range(4, 20, 30)).unwrap();
        assert_eq!(
            b.accept_signed(&other, &owner),
            Err(RangeError::WrongCounter)
        );
    }

    /// Every rule ID in the section of this file (WL-20 and up).
    fn rule_ids() -> Vec<String> {
        RULES
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("- **WL-"))
            .map(|rest| rest.chars().take(2).collect::<String>())
            .filter(|n| n.as_str() >= "20")
            .collect()
    }

    #[test]
    fn the_rules_file_defines_wl_20_to_wl_28_and_each_has_a_test() {
        let ids = rule_ids();
        assert_eq!(ids.len(), 9);
        assert_eq!(ids.first().map(String::as_str), Some("20"));
        assert_eq!(ids.last().map(String::as_str), Some("28"));
        for id in &ids {
            assert!(SOURCE.contains(&format!("fn wl_{id}_")), "WL-{id}");
        }
    }

    #[test]
    fn every_refusal_names_a_rule_of_the_file() {
        let ids = rule_ids();
        let shown = [
            PinnedError::Import(ImportRefused::Damaged).to_string(),
            PinnedError::Decode(DecodeError::Truncated).to_string(),
            PinnedError::NotCanonical.to_string(),
            PinnedError::ChecksumMismatch.to_string(),
            PinnedError::Derive.to_string(),
            RangeError::Empty.to_string(),
            RangeError::PastHardened.to_string(),
            RangeError::TooLong.to_string(),
            RangeError::BadSignature.to_string(),
            RangeError::WrongCounter.to_string(),
            RangeError::OverlapsUsed.to_string(),
            RangeError::OverlapsTaken.to_string(),
            RangeError::UsedUp.to_string(),
        ];
        for s in shown {
            let id = &s[3..5];
            assert!(ids.iter().any(|i| i == id), "{s}");
        }
    }
}
