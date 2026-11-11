//! Device identity keys.
//!
//! An identity key is a BIP340 Schnorr key that belongs to one device (the
//! owner phone, the Key app, a counter). It signs batches, records, settings
//! and pairings. It is **never** derived from a wallet key: wallet keys move
//! money, identity keys only vouch for messages, and keeping them apart means
//! a stolen identity cannot spend and a stolen wallet key cannot forge
//! settings. The caller must supply the 32 secret bytes from a cryptographic random
//! source; this crate has none on purpose.
//!
//! All signatures are over BIP340 tagged hashes
//! `SHA256(SHA256(tag) || SHA256(tag) || msg)`, so a signature made for one
//! kind of message can never be replayed as another.

use std::fmt;
use std::sync::OnceLock;

use bitcoin::secp256k1::{
    schnorr, All, Keypair, Message, PublicKey, Scalar, Secp256k1, XOnlyPublicKey,
};

pub use crate::hashing::tagged_hash;
use crate::words::{fingerprint_words, WORD_COUNT};

/// Tag for address batches.
pub const TAG_BATCH: &str = "OpenBMT/batch/v1";
/// Tag for hash-chained records.
pub const TAG_RECORD: &str = "OpenBMT/record/v1";
/// Tag for shop settings.
pub const TAG_SETTINGS: &str = "OpenBMT/settings/v1";
/// Tag for device pairing.
pub const TAG_PAIRING: &str = "OpenBMT/pairing/v1";
/// Tag for the offer binding a counter checks against its roots and wallet.
pub const TAG_PAIR_OFFER: &str = "OpenBMT/pair-offer/v1";
/// Tag for the counter's signed pairing request.
pub const TAG_PAIR_REQUEST: &str = "OpenBMT/pair-request/v1";
/// Tag for the digest behind the session words.
pub const TAG_PAIR_SESSION: &str = "OpenBMT/pair-session/v1";
/// Tag for owner-signed commands to a counter.
pub const TAG_COMMAND: &str = "OpenBMT/command/v1";
/// Tag for the owner's signed watcher heartbeat to a counter.
pub const TAG_HEARTBEAT: &str = "OpenBMT/heartbeat/v1";
/// Tag for a heartbeat signed by the shop's watch key (CR-50, CR-55). It is
/// not the tag of any owner-root signature, so a watch-key signature can
/// never be read as anything but a heartbeat.
pub const TAG_WATCH_HEARTBEAT: &str = "OpenBMT/watch-heartbeat/v1";
/// Tag for the owner's signed relay for counters.
pub const TAG_RELAY_HINT: &str = "OpenBMT/relay-hint/v1";
/// Tag for the on-screen identity fingerprint words.
pub const TAG_FINGERPRINT: &str = "OpenBMT/fingerprint/v1";
/// Tag for the owner's signature on an approval session request.
pub const TAG_SESSION_REQUEST: &str = "OpenBMT/session-request/v1";
/// Tag for the approver's signature on an approval session response.
pub const TAG_SESSION_RESPONSE: &str = "OpenBMT/session-response/v1";
/// Tag for the hash of a session request, which a response names.
pub const TAG_SESSION_HASH: &str = "OpenBMT/session-hash/v1";
/// Tag for the approver's signed answer to a session health check.
pub const TAG_SESSION_HEALTH: &str = "OpenBMT/session-health/v1";

pub(crate) fn secp() -> &'static Secp256k1<All> {
    static CTX: OnceLock<Secp256k1<All>> = OnceLock::new();
    CTX.get_or_init(Secp256k1::new)
}

/// A 64-byte BIP340 signature.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sig(pub [u8; 64]);

impl fmt::Debug for Sig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sig({:02x}{:02x}..)", self.0[0], self.0[1])
    }
}

/// Why an identity could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// The 32 bytes are not a valid secret key (zero or above the curve order).
    InvalidSecret,
    /// The 32 bytes are not a valid x-only public key.
    InvalidPublicKey,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSecret => write!(f, "not a valid secret key"),
            Self::InvalidPublicKey => write!(f, "not a valid public key"),
        }
    }
}

impl std::error::Error for IdentityError {}

/// A device's public identity: its x-only BIP340 public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(XOnlyPublicKey);

impl DeviceId {
    /// The 32-byte serialization.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.serialize()
    }

    /// Parse 32 bytes; fails if they are not a point on the curve.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, IdentityError> {
        XOnlyPublicKey::from_slice(bytes)
            .map(Self)
            .map_err(|_| IdentityError::InvalidPublicKey)
    }

    /// Four stretched words (see `words`) that identify this key on a screen,
    /// for pinning a root. A human cross-check only; software compares the
    /// full key.
    pub fn fingerprint_words(&self) -> [&'static str; WORD_COUNT] {
        fingerprint_words(&tagged_hash(TAG_FINGERPRINT, &self.to_bytes()))
    }

    /// Check `sig` over `tagged_hash(tag, msg)`.
    pub fn verify_tagged(&self, tag: &str, msg: &[u8], sig: &Sig) -> bool {
        self.verify_digest(&tagged_hash(tag, msg), sig)
    }

    /// Check `sig` over an already tagged-hashed 32-byte digest. Crate-private
    /// so that outside callers can only verify under a tag.
    pub(crate) fn verify_digest(&self, digest: &[u8; 32], sig: &Sig) -> bool {
        let Ok(s) = schnorr::Signature::from_slice(&sig.0) else {
            return false;
        };
        secp()
            .verify_schnorr(&s, &Message::from_digest(*digest), &self.0)
            .is_ok()
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = self.to_bytes();
        write!(
            f,
            "DeviceId({:02x}{:02x}{:02x}{:02x}..)",
            b[0], b[1], b[2], b[3]
        )
    }
}

/// A device's identity key pair. The secret never prints.
pub struct Identity {
    keypair: Keypair,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Identity({:?})", self.device_id())
    }
}

impl Identity {
    /// Build from 32 secret bytes supplied by a cryptographic random source.
    pub fn from_secret_bytes(secret: &[u8; 32]) -> Result<Self, IdentityError> {
        Keypair::from_seckey_slice(secp(), secret)
            .map(|keypair| Self { keypair })
            .map_err(|_| IdentityError::InvalidSecret)
    }

    /// The public identity.
    pub fn device_id(&self) -> DeviceId {
        DeviceId(self.keypair.x_only_public_key().0)
    }

    /// Sign `tagged_hash(tag, msg)`. Signatures are deterministic (BIP340
    /// with no auxiliary randomness), so the same input always gives the same
    /// signature.
    pub fn sign_tagged(&self, tag: &str, msg: &[u8]) -> Sig {
        self.sign_digest(&tagged_hash(tag, msg))
    }

    /// Sign an already tagged-hashed 32-byte digest. Crate-private so that
    /// outside callers can only sign under a tag.
    pub(crate) fn sign_digest(&self, digest: &[u8; 32]) -> Sig {
        let s = secp().sign_schnorr_no_aux_rand(&Message::from_digest(*digest), &self.keypair);
        Sig(*s.as_ref())
    }

    /// Four words for showing this device's root on screen.
    pub fn fingerprint_words(&self) -> [&'static str; WORD_COUNT] {
        self.device_id().fingerprint_words()
    }

    /// The x coordinate of `point` multiplied by this key's secret: the
    /// shared value of an ECDH exchange, for a message sealed to this device.
    /// Only the x coordinate leaves, so it does not matter which of the two
    /// points with that x the peer used. `None` only if the product is not a
    /// valid point, which a valid key and point never give.
    pub fn shared_x(&self, point: &PublicKey) -> Option<[u8; 32]> {
        let secret = Scalar::from_be_bytes(self.keypair.secret_bytes()).ok()?;
        let product = point.mul_tweak(secp(), &secret).ok()?;
        let full = product.serialize();
        let mut x = [0u8; 32];
        x.copy_from_slice(full.get(1..33)?);
        Some(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::identity;
    use bitcoin::hashes::{sha256, Hash};

    #[test]
    fn tagged_hash_matches_bip340_definition() {
        // Computed by hand from the definition.
        let tag = sha256::Hash::hash(b"OpenBMT/x/v1");
        let mut data = Vec::new();
        data.extend_from_slice(tag.as_byte_array());
        data.extend_from_slice(tag.as_byte_array());
        data.extend_from_slice(b"hello");
        let want = sha256::Hash::hash(&data).to_byte_array();
        assert_eq!(tagged_hash("OpenBMT/x/v1", b"hello"), want);
    }

    #[test]
    fn tagged_hash_differs_by_tag_and_message() {
        assert_ne!(tagged_hash(TAG_BATCH, b"a"), tagged_hash(TAG_RECORD, b"a"));
        assert_ne!(tagged_hash(TAG_BATCH, b"a"), tagged_hash(TAG_BATCH, b"b"));
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let id = identity(1);
        let sig = id.sign_tagged(TAG_BATCH, b"msg");
        assert!(id.device_id().verify_tagged(TAG_BATCH, b"msg", &sig));
    }

    #[test]
    fn signing_is_deterministic() {
        let id = identity(1);
        assert_eq!(
            id.sign_tagged(TAG_BATCH, b"msg"),
            id.sign_tagged(TAG_BATCH, b"msg")
        );
    }

    #[test]
    fn wrong_tag_message_or_key_fails() {
        let id = identity(1);
        let other = identity(2);
        let sig = id.sign_tagged(TAG_BATCH, b"msg");
        let dev = id.device_id();
        assert!(!dev.verify_tagged(TAG_RECORD, b"msg", &sig));
        assert!(!dev.verify_tagged(TAG_BATCH, b"msg2", &sig));
        assert!(!other.device_id().verify_tagged(TAG_BATCH, b"msg", &sig));
    }

    #[test]
    fn corrupt_signature_fails() {
        let id = identity(1);
        let mut sig = id.sign_tagged(TAG_BATCH, b"msg");
        sig.0[10] ^= 1;
        assert!(!id.device_id().verify_tagged(TAG_BATCH, b"msg", &sig));
        assert!(!id
            .device_id()
            .verify_tagged(TAG_BATCH, b"msg", &Sig([0; 64])));
    }

    #[test]
    fn invalid_secret_is_rejected() {
        assert_eq!(
            Identity::from_secret_bytes(&[0; 32]).unwrap_err(),
            IdentityError::InvalidSecret
        );
        assert_eq!(
            Identity::from_secret_bytes(&[0xff; 32]).unwrap_err(),
            IdentityError::InvalidSecret
        );
    }

    #[test]
    fn device_id_bytes_round_trip_and_reject_garbage() {
        let d = identity(3).device_id();
        assert_eq!(DeviceId::from_bytes(&d.to_bytes()).unwrap(), d);
        assert!(DeviceId::from_bytes(&[0; 32]).is_err());
    }

    #[test]
    fn fingerprint_words_are_stable_and_distinct() {
        let a = identity(1);
        let b = identity(2);
        assert_eq!(a.fingerprint_words(), a.device_id().fingerprint_words());
        assert_eq!(a.fingerprint_words(), identity(1).fingerprint_words());
        assert_ne!(a.fingerprint_words(), b.fingerprint_words());
    }

    #[test]
    fn shared_x_agrees_from_both_sides() {
        let a = identity(1);
        let b = identity(2);
        let lift = |id: &Identity| {
            PublicKey::from_x_only_public_key(
                XOnlyPublicKey::from_slice(&id.device_id().to_bytes()).unwrap(),
                bitcoin::secp256k1::Parity::Even,
            )
        };
        let ab = a.shared_x(&lift(&b)).unwrap();
        let ba = b.shared_x(&lift(&a)).unwrap();
        assert_eq!(ab, ba);
        assert_ne!(ab, a.shared_x(&lift(&identity(3))).unwrap());
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let s = format!("{:?}", identity(1));
        assert!(s.starts_with("Identity(DeviceId("));
        assert!(!s.contains("keypair"));
    }
}
