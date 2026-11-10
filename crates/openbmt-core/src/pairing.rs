//! Pairing a counter with the owner phone and the Key app.
//!
//! The counter pins two roots, the owner phone's and the Key app's. The
//! ceremony has two scans and one comparison:
//!
//! 1. The Key app shows a [`PairOffer`]: both roots and the wallet's
//!    checksum. The counter reads it ([`CounterPairing::begin`]) and fixes
//!    the roots and wallet. Only then does it make a fresh number.
//! 2. The counter shows a [`PairRequest`]: its key, its name, the number and
//!    a binding hash of the offer it was given, signed with its own key. The
//!    Key app compares the binding with its own offer in full
//!    ([`check_request`]).
//! 3. Both screens show the same four session words, made from the roots,
//!    the wallet, the counter's key and the number ([`session_words`]). The
//!    owner approves on the Key app ([`PendingPairing::approve`]) and taps
//!    "They match" on the counter.
//! 4. The Key app signs the [`PairingEntry`], then the owner phone adds its
//!    signature ([`owner_cosign`]). The counter accepts the result
//!    ([`CounterPairing::finish`]) only if both signatures check against the
//!    roots it fixed in step 1 and the entry names its own key, name and
//!    number.
//!
//! What the words protect against: someone who shows the counter a false
//! offer and relays a request of their own to the Key app has to make the
//! two screens show the same four words. The counter's number is fixed
//! before they can start, so they must search for a matching key and number
//! of their own inside the counter's time window: about 2^44 tries, each
//! costing the stretch of [`crate::words`].
//!
//! The library has no clock and no randomness. The counter's number must come
//! from a secure random source and must be new for every ceremony; callers
//! pass the time in seconds.
//!
//! Wire forms (all strict, see [`crate::wire`]):
//!
//! ```text
//! offer:   "OBMO" | 1 | owner (32) | approver (32) | wallet (32)
//! request: "OBMJ" | 1 | counter (32) | name len u8 | name | nonce (32)
//!            | binding (32) | sig (64)
//! signed:  "OBMD" | 1 | wallet (32) | counter (32) | name len u8 | name
//!            | nonce (32) | settings version u32 LE
//!            | approver sig (64) | owner sig (64)
//! ```
//!
//! Decoding only parses. Signatures are checked by the explicit steps above.

use std::fmt;

use crate::batch::PinnedRoots;
use crate::identity::{
    tagged_hash, DeviceId, Identity, Sig, TAG_PAIRING, TAG_PAIR_OFFER, TAG_PAIR_REQUEST,
    TAG_PAIR_SESSION,
};
use crate::wire::{self, DecodeError, Reader};
use crate::words::{fingerprint_words, WORD_COUNT};

/// How long a ceremony may take, in seconds, from `begin` to `finish`.
pub const PAIRING_WINDOW_SECS: u64 = 600;

/// Most characters in a counter's name.
const NAME_MAX_CHARS: usize = 32;
/// Most UTF-8 bytes in a counter's name.
const NAME_MAX_BYTES: usize = 128;

/// Why a pairing step was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    /// Owner and Key app roots must be different keys.
    SameRoot,
    /// The counter's key is the owner's or the Key app's.
    CounterIsRoot,
    /// The name is empty, too long, or holds a control character.
    InvalidName,
    /// The bytes were not a valid message.
    Decode(DecodeError),
    /// The request was made for other roots or another wallet.
    BindingMismatch,
    /// The request's signature does not verify under the counter's key.
    RequestSignatureInvalid,
    /// The owner did not confirm that the words match.
    NotConfirmed,
    /// The time given is earlier than the start of the ceremony.
    ClockWentBack,
    /// The ceremony took longer than [`PAIRING_WINDOW_SECS`].
    Expired,
    /// The entry is for another wallet.
    WrongWallet,
    /// The entry names another counter.
    WrongCounter,
    /// The entry carries another number.
    WrongNonce,
    /// The entry carries another name.
    WrongName,
    /// The Key app's signature does not verify.
    ApproverSignatureInvalid,
    /// The owner's signature does not verify.
    OwnerSignatureInvalid,
    /// This identity is not the Key app root of the offer.
    NotTheApprover,
    /// This identity is not the owner root of the offer.
    NotTheOwner,
}

impl fmt::Display for PairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SameRoot => write!(f, "owner and Key app roots must differ"),
            Self::CounterIsRoot => write!(f, "the counter's key is one of the roots"),
            Self::InvalidName => write!(f, "the counter's name is not allowed"),
            Self::Decode(e) => write!(f, "{e}"),
            Self::BindingMismatch => write!(f, "request was made for other roots or wallet"),
            Self::RequestSignatureInvalid => write!(f, "request signature is not valid"),
            Self::NotConfirmed => write!(f, "the words were not confirmed"),
            Self::ClockWentBack => write!(f, "the clock is earlier than the start of pairing"),
            Self::Expired => write!(f, "pairing took too long"),
            Self::WrongWallet => write!(f, "pairing is for another wallet"),
            Self::WrongCounter => write!(f, "pairing names another counter"),
            Self::WrongNonce => write!(f, "pairing carries another number"),
            Self::WrongName => write!(f, "pairing carries another name"),
            Self::ApproverSignatureInvalid => write!(f, "Key app signature is not valid"),
            Self::OwnerSignatureInvalid => write!(f, "owner signature is not valid"),
            Self::NotTheApprover => write!(f, "this is not the Key app root of the offer"),
            Self::NotTheOwner => write!(f, "this is not the owner root of the offer"),
        }
    }
}

impl std::error::Error for PairError {}

impl From<DecodeError> for PairError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

/// Whether `name` may be used for a counter: 1 to 32 characters, at most 128
/// bytes of UTF-8, and no control characters.
pub fn name_is_valid(name: &str) -> bool {
    let chars = name.chars().count();
    (1..=NAME_MAX_CHARS).contains(&chars)
        && name.len() <= NAME_MAX_BYTES
        && !name.chars().any(char::is_control)
}

/// A name as the wire carries it: length byte, then the UTF-8 bytes. The
/// name must pass [`name_is_valid`].
fn put_name(out: &mut Vec<u8>, name: &str) {
    out.push(u8::try_from(name.len()).expect("a valid name fits the u8 length"));
    out.extend_from_slice(name.as_bytes());
}

fn read_name(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let n = usize::from(r.u8()?);
    let name = std::str::from_utf8(r.take(n)?)
        .map_err(|_| DecodeError::NonCanonical("name is not UTF-8"))?;
    if !name_is_valid(name) {
        return Err(DecodeError::NonCanonical("name breaks the name rule"));
    }
    Ok(name.to_owned())
}

/// What the Key app's "Pair a counter" screen shows (step 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairOffer {
    /// The owner phone's root.
    pub owner: DeviceId,
    /// The Key app's root.
    pub approver: DeviceId,
    /// The wallet's checksum (`WalletDescription::checksum32`).
    pub wallet: [u8; 32],
}

impl PairOffer {
    /// An offer for two different roots and a wallet.
    pub fn new(owner: DeviceId, approver: DeviceId, wallet: [u8; 32]) -> Result<Self, PairError> {
        if owner == approver {
            return Err(PairError::SameRoot);
        }
        Ok(Self {
            owner,
            approver,
            wallet,
        })
    }

    /// The hash of the offer that goes into the counter's request:
    /// `tagged_hash(TAG_PAIR_OFFER, owner | approver | wallet)`.
    pub fn binding(&self) -> [u8; 32] {
        let mut msg = Vec::with_capacity(96);
        msg.extend_from_slice(&self.owner.to_bytes());
        msg.extend_from_slice(&self.approver.to_bytes());
        msg.extend_from_slice(&self.wallet);
        tagged_hash(TAG_PAIR_OFFER, &msg)
    }

    /// Wire form: `OBMO`, version 1, owner, Key app, wallet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&wire::MAGIC_PAIR_OFFER, 96);
        out.extend_from_slice(&self.owner.to_bytes());
        out.extend_from_slice(&self.approver.to_bytes());
        out.extend_from_slice(&self.wallet);
        out
    }

    /// Parse the wire form strictly. Equal roots and invalid keys are
    /// refused.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_PAIR_OFFER)?;
        let owner = r.device_id()?;
        let approver = r.device_id()?;
        let wallet = r.array()?;
        r.finish()?;
        if owner == approver {
            return Err(DecodeError::NonCanonical(
                "owner and Key app roots are equal",
            ));
        }
        Ok(Self {
            owner,
            approver,
            wallet,
        })
    }
}

/// The counter's pairing QR (step 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairRequest {
    /// The counter's own identity key.
    pub counter: DeviceId,
    /// The counter's name (see [`name_is_valid`]).
    pub name: String,
    /// The fresh number made for this ceremony.
    pub nonce: [u8; 32],
    /// [`PairOffer::binding`] of the offer the counter was given.
    pub binding: [u8; 32],
    /// The counter's signature over the other fields; it proves the counter
    /// holds the key it names.
    pub sig: Sig,
}

impl PairRequest {
    /// The signed bytes: counter, name length, name, number, binding.
    fn body(counter: &DeviceId, name: &str, nonce: &[u8; 32], binding: &[u8; 32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(98 + name.len());
        out.extend_from_slice(&counter.to_bytes());
        put_name(&mut out, name);
        out.extend_from_slice(nonce);
        out.extend_from_slice(binding);
        out
    }

    /// Wire form: `OBMJ`, version 1, the signed bytes, then the signature.
    /// The name must pass [`name_is_valid`]; a longer one panics rather than
    /// truncating the length.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = Self::body(&self.counter, &self.name, &self.nonce, &self.binding);
        let mut out = wire::start(&wire::MAGIC_PAIR_REQUEST, body.len() + 64);
        out.extend_from_slice(&body);
        out.extend_from_slice(&self.sig.0);
        out
    }

    /// Parse the wire form strictly and check the name rule. This does not
    /// check the signature: [`check_request`] does.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_PAIR_REQUEST)?;
        let counter = r.device_id()?;
        let name = read_name(&mut r)?;
        let nonce = r.array()?;
        let binding = r.array()?;
        let sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            counter,
            name,
            nonce,
            binding,
            sig,
        })
    }

    fn signature_ok(&self) -> bool {
        let body = Self::body(&self.counter, &self.name, &self.nonce, &self.binding);
        self.counter
            .verify_tagged(TAG_PAIR_REQUEST, &body, &self.sig)
    }
}

/// The digest behind the session words:
/// `tagged_hash(TAG_PAIR_SESSION, owner | approver | wallet | counter | nonce)`.
pub fn session_digest(offer: &PairOffer, counter: &DeviceId, nonce: &[u8; 32]) -> [u8; 32] {
    let mut msg = Vec::with_capacity(160);
    msg.extend_from_slice(&offer.owner.to_bytes());
    msg.extend_from_slice(&offer.approver.to_bytes());
    msg.extend_from_slice(&offer.wallet);
    msg.extend_from_slice(&counter.to_bytes());
    msg.extend_from_slice(nonce);
    tagged_hash(TAG_PAIR_SESSION, &msg)
}

/// The four words both screens show. The stretch makes this slow on purpose.
pub fn session_words(
    offer: &PairOffer,
    counter: &DeviceId,
    nonce: &[u8; 32],
) -> [&'static str; WORD_COUNT] {
    fingerprint_words(&session_digest(offer, counter, nonce))
}

/// What both roots sign (step 4): the pairing entry for one counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingEntry {
    /// The wallet's checksum.
    pub wallet: [u8; 32],
    /// The counter being paired.
    pub counter: DeviceId,
    /// The counter's name.
    pub name: String,
    /// The number from the counter's request.
    pub nonce: [u8; 32],
    /// The settings version in force when the counter is paired.
    pub settings_version: u32,
}

impl PairingEntry {
    /// The canonical bytes that are hashed and signed: wallet, counter, name
    /// length, name, number, settings version (u32 LE). The name must pass
    /// [`name_is_valid`]; every path in this module checks it first, and
    /// calling this on a hand-built entry with a name over 255 bytes panics.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(101 + self.name.len());
        out.extend_from_slice(&self.wallet);
        out.extend_from_slice(&self.counter.to_bytes());
        put_name(&mut out, &self.name);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.settings_version.to_le_bytes());
        out
    }

    /// `tagged_hash(TAG_PAIRING, encode())`, the digest both roots sign.
    pub fn hash(&self) -> [u8; 32] {
        tagged_hash(TAG_PAIRING, &self.encode())
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let wallet = r.array()?;
        let counter = r.device_id()?;
        let name = read_name(r)?;
        let nonce = r.array()?;
        let settings_version = r.u32_le()?;
        Ok(Self {
            wallet,
            counter,
            name,
            nonce,
            settings_version,
        })
    }
}

/// A pairing entry with both root signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedPairing {
    /// The entry.
    pub entry: PairingEntry,
    /// The Key app's signature over [`PairingEntry::hash`].
    pub approver_sig: Sig,
    /// The owner's signature over [`PairingEntry::hash`].
    pub owner_sig: Sig,
}

impl SignedPairing {
    /// Wire form: `OBMD`, version 1, the bytes of [`PairingEntry::encode`],
    /// the Key app's signature, then the owner's.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.entry.encode();
        let mut out = wire::start(&wire::MAGIC_SIGNED_PAIRING, body.len() + 128);
        out.extend_from_slice(&body);
        out.extend_from_slice(&self.approver_sig.0);
        out.extend_from_slice(&self.owner_sig.0);
        out
    }

    /// Parse the wire form strictly and check the name rule. This does not
    /// check signatures: [`CounterPairing::finish`] does.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_SIGNED_PAIRING)?;
        let entry = PairingEntry::read(&mut r)?;
        let approver_sig = r.sig()?;
        let owner_sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            entry,
            approver_sig,
            owner_sig,
        })
    }
}

/// The counter's side of one ceremony. It is used once: [`finish`] takes it
/// by value, so the number cannot be used for a second try.
///
/// [`finish`]: CounterPairing::finish
#[derive(Debug)]
pub struct CounterPairing {
    roots: PinnedRoots,
    wallet: [u8; 32],
    own: DeviceId,
    name: String,
    nonce: [u8; 32],
    started_at: u64,
}

impl CounterPairing {
    /// Steps 1 and 2 on the counter. Decodes the offer, fixes its roots and
    /// wallet, and builds the signed request.
    ///
    /// `nonce` must come from the caller's secure random source and must be
    /// new for every ceremony; the scheme rests on nobody knowing it when
    /// the roots were chosen. `now` is the time in seconds.
    pub fn begin(
        offer_bytes: &[u8],
        identity: &Identity,
        name: &str,
        nonce: [u8; 32],
        now: u64,
    ) -> Result<(Self, PairRequest), PairError> {
        let offer = PairOffer::from_bytes(offer_bytes)?;
        let own = identity.device_id();
        if own == offer.owner || own == offer.approver {
            return Err(PairError::CounterIsRoot);
        }
        if !name_is_valid(name) {
            return Err(PairError::InvalidName);
        }
        let binding = offer.binding();
        let body = PairRequest::body(&own, name, &nonce, &binding);
        let sig = identity.sign_tagged(TAG_PAIR_REQUEST, &body);
        let request = PairRequest {
            counter: own,
            name: name.to_owned(),
            nonce,
            binding,
            sig,
        };
        let pairing = Self {
            roots: PinnedRoots {
                owner: offer.owner,
                approver: offer.approver,
            },
            wallet: offer.wallet,
            own,
            name: name.to_owned(),
            nonce,
            started_at: now,
        };
        Ok((pairing, request))
    }

    fn offer(&self) -> PairOffer {
        PairOffer {
            owner: self.roots.owner,
            approver: self.roots.approver,
            wallet: self.wallet,
        }
    }

    /// The four words to show; they must equal the Key app's. Slow on
    /// purpose.
    pub fn session_words(&self) -> [&'static str; WORD_COUNT] {
        session_words(&self.offer(), &self.own, &self.nonce)
    }

    /// The roots fixed in step 1.
    pub fn roots(&self) -> PinnedRoots {
        self.roots
    }

    /// The wallet checksum fixed in step 1.
    pub fn wallet(&self) -> [u8; 32] {
        self.wallet
    }

    /// Step 4 on the counter. `words_confirmed` is whether the owner tapped
    /// "They match". The checks run in this order and the first failure
    /// wins: confirmed, time, wallet, counter, number, name, the Key app's
    /// signature, the owner's signature. The pairing is consumed whatever the
    /// outcome; a new try needs a new ceremony and a new number. `now` is the
    /// time in seconds.
    pub fn finish(
        self,
        signed: &SignedPairing,
        words_confirmed: bool,
        now: u64,
    ) -> Result<Paired, PairError> {
        if !words_confirmed {
            return Err(PairError::NotConfirmed);
        }
        if now < self.started_at {
            return Err(PairError::ClockWentBack);
        }
        if now - self.started_at > PAIRING_WINDOW_SECS {
            return Err(PairError::Expired);
        }
        let entry = &signed.entry;
        if entry.wallet != self.wallet {
            return Err(PairError::WrongWallet);
        }
        if entry.counter != self.own {
            return Err(PairError::WrongCounter);
        }
        if entry.nonce != self.nonce {
            return Err(PairError::WrongNonce);
        }
        if entry.name != self.name {
            return Err(PairError::WrongName);
        }
        let digest = entry.hash();
        if !self
            .roots
            .approver
            .verify_digest(&digest, &signed.approver_sig)
        {
            return Err(PairError::ApproverSignatureInvalid);
        }
        if !self.roots.owner.verify_digest(&digest, &signed.owner_sig) {
            return Err(PairError::OwnerSignatureInvalid);
        }
        Ok(Paired {
            roots: self.roots,
            wallet: self.wallet,
            settings_floor: entry.settings_version,
        })
    }
}

/// What a counter keeps after a finished ceremony.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    roots: PinnedRoots,
    wallet: [u8; 32],
    settings_floor: u32,
}

impl Paired {
    /// The pinned roots.
    pub fn roots(&self) -> PinnedRoots {
        self.roots
    }

    /// The wallet checksum.
    pub fn wallet(&self) -> [u8; 32] {
        self.wallet
    }

    /// The settings version in the pairing entry. Pass it to
    /// `SettingsStore::with_floor`. The counter then accepts only settings
    /// with a higher version. That is intended: the settings in force at
    /// pairing do not list this counter yet, and the settings that add it
    /// (signed by both roots) carry the next version.
    pub fn settings_floor(&self) -> u32 {
        self.settings_floor
    }
}

/// Step 2 on the Key app: check the counter's request against the Key app's
/// own offer. The binding is compared in full; then the name rule, then that
/// the counter is neither root, then the request signature.
pub fn check_request(own: &PairOffer, request: &PairRequest) -> Result<PendingPairing, PairError> {
    if request.binding != own.binding() {
        return Err(PairError::BindingMismatch);
    }
    if !name_is_valid(&request.name) {
        return Err(PairError::InvalidName);
    }
    if request.counter == own.owner || request.counter == own.approver {
        return Err(PairError::CounterIsRoot);
    }
    if !request.signature_ok() {
        return Err(PairError::RequestSignatureInvalid);
    }
    Ok(PendingPairing {
        offer: *own,
        counter: request.counter,
        name: request.name.clone(),
        nonce: request.nonce,
    })
}

/// A request that passed [`check_request`] and waits for the owner to compare
/// the words.
#[derive(Debug)]
pub struct PendingPairing {
    offer: PairOffer,
    counter: DeviceId,
    name: String,
    nonce: [u8; 32],
}

impl PendingPairing {
    /// The four words to show; they must equal the counter's. Slow on
    /// purpose.
    pub fn session_words(&self) -> [&'static str; WORD_COUNT] {
        session_words(&self.offer, &self.counter, &self.nonce)
    }

    /// The counter's key.
    pub fn counter(&self) -> DeviceId {
        self.counter
    }

    /// The counter's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Step 4 on the Key app, after the owner approved: the entry and the
    /// Key app's signature over [`PairingEntry::hash`]. `approver` must be
    /// the Key app root of the offer. `settings_version` is the version in
    /// force now.
    pub fn approve(
        self,
        approver: &Identity,
        settings_version: u32,
    ) -> Result<(PairingEntry, Sig), PairError> {
        if approver.device_id() != self.offer.approver {
            return Err(PairError::NotTheApprover);
        }
        let entry = PairingEntry {
            wallet: self.offer.wallet,
            counter: self.counter,
            name: self.name,
            nonce: self.nonce,
            settings_version,
        };
        let sig = approver.sign_digest(&entry.hash());
        Ok((entry, sig))
    }
}

/// Step 4 on the owner phone: add the owner's signature to an entry the Key
/// app has signed. The owner never signs first: the Key app's signature is
/// checked before anything is signed.
pub fn owner_cosign(
    own: &PairOffer,
    entry: PairingEntry,
    approver_sig: Sig,
    owner: &Identity,
) -> Result<SignedPairing, PairError> {
    if owner.device_id() != own.owner {
        return Err(PairError::NotTheOwner);
    }
    if entry.wallet != own.wallet {
        return Err(PairError::WrongWallet);
    }
    if !name_is_valid(&entry.name) {
        return Err(PairError::InvalidName);
    }
    if entry.counter == own.owner || entry.counter == own.approver {
        return Err(PairError::CounterIsRoot);
    }
    let digest = entry.hash();
    if !own.approver.verify_digest(&digest, &approver_sig) {
        return Err(PairError::ApproverSignatureInvalid);
    }
    let owner_sig = owner.sign_digest(&digest);
    Ok(SignedPairing {
        entry,
        approver_sig,
        owner_sig,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsStore;
    use crate::testutil::{assert_strict_decoding, identity};

    const T0: u64 = 1_000;

    fn offer() -> PairOffer {
        PairOffer::new(identity(1).device_id(), identity(2).device_id(), [7; 32]).unwrap()
    }

    /// A ceremony stopped just before `finish`.
    struct Fixture {
        offer: PairOffer,
        counter: CounterPairing,
        request: PairRequest,
        entry: PairingEntry,
        approver_sig: Sig,
        signed: SignedPairing,
    }

    fn fixture_with(counter_key: u8, name: &str, nonce: [u8; 32]) -> Fixture {
        let offer = offer();
        let (counter, request) =
            CounterPairing::begin(&offer.to_bytes(), &identity(counter_key), name, nonce, T0)
                .unwrap();
        let pending = check_request(&offer, &request).unwrap();
        let (entry, approver_sig) = pending.approve(&identity(2), 5).unwrap();
        let signed = owner_cosign(&offer, entry.clone(), approver_sig, &identity(1)).unwrap();
        Fixture {
            offer,
            counter,
            request,
            entry,
            approver_sig,
            signed,
        }
    }

    fn fixture() -> Fixture {
        fixture_with(3, "Front counter", [9; 32])
    }

    fn signed_by(entry: &PairingEntry, approver: &Identity, owner: &Identity) -> SignedPairing {
        let d = entry.hash();
        SignedPairing {
            entry: entry.clone(),
            approver_sig: approver.sign_digest(&d),
            owner_sig: owner.sign_digest(&d),
        }
    }

    #[test]
    fn honest_ceremony_pairs_and_both_sides_show_the_same_words() {
        let f = fixture();
        let pending = check_request(&f.offer, &f.request).unwrap();
        assert_eq!(pending.counter(), identity(3).device_id());
        assert_eq!(pending.name(), "Front counter");
        let words = f.counter.session_words();
        assert_eq!(words, pending.session_words());
        assert_eq!(
            words,
            session_words(&f.offer, &identity(3).device_id(), &[9; 32])
        );
        let paired = f.counter.finish(&f.signed, true, T0 + 30).unwrap();
        let roots = PinnedRoots::new(identity(1).device_id(), identity(2).device_id()).unwrap();
        assert_eq!(paired.roots(), roots);
        assert_eq!(paired.wallet(), [7; 32]);
        assert_eq!(paired.settings_floor(), 5);
        // The floor goes straight into a settings store.
        let _ = SettingsStore::with_floor(paired.roots(), paired.settings_floor());
    }

    #[test]
    fn counter_getters_show_the_fixed_roots_and_wallet() {
        let f = fixture();
        assert_eq!(f.counter.roots().owner, f.offer.owner);
        assert_eq!(f.counter.roots().approver, f.offer.approver);
        assert_eq!(f.counter.wallet(), f.offer.wallet);
    }

    #[test]
    fn words_change_when_the_number_changes() {
        let o = offer();
        let c = identity(3).device_id();
        assert_ne!(
            session_words(&o, &c, &[1; 32]),
            session_words(&o, &c, &[2; 32])
        );
    }

    #[test]
    fn session_digest_changes_with_every_input() {
        let o = offer();
        let c = identity(3).device_id();
        let n = [9; 32];
        let base = session_digest(&o, &c, &n);
        assert_eq!(base, session_digest(&o, &c, &n));
        let other_owner = PairOffer::new(identity(4).device_id(), o.approver, o.wallet).unwrap();
        let other_approver = PairOffer::new(o.owner, identity(4).device_id(), o.wallet).unwrap();
        let other_wallet = PairOffer::new(o.owner, o.approver, [8; 32]).unwrap();
        assert_ne!(base, session_digest(&other_owner, &c, &n));
        assert_ne!(base, session_digest(&other_approver, &c, &n));
        assert_ne!(base, session_digest(&other_wallet, &c, &n));
        assert_ne!(base, session_digest(&o, &identity(4).device_id(), &n));
        assert_ne!(base, session_digest(&o, &c, &[10; 32]));
    }

    #[test]
    fn offer_needs_two_different_roots() {
        let a = identity(1).device_id();
        assert_eq!(
            PairOffer::new(a, a, [0; 32]).unwrap_err(),
            PairError::SameRoot
        );
    }

    #[test]
    fn counter_that_scanned_another_offer_is_refused_by_the_key_app() {
        let own = offer();
        let o = identity(1).device_id();
        let a = identity(2).device_id();
        let others = [
            PairOffer::new(identity(4).device_id(), a, own.wallet).unwrap(),
            PairOffer::new(o, identity(4).device_id(), own.wallet).unwrap(),
            PairOffer::new(o, a, [8; 32]).unwrap(),
        ];
        for other in others {
            let (_, request) =
                CounterPairing::begin(&other.to_bytes(), &identity(3), "A", [9; 32], T0).unwrap();
            assert_eq!(
                check_request(&own, &request).unwrap_err(),
                PairError::BindingMismatch
            );
        }
    }

    #[test]
    fn request_with_a_swapped_counter_key_is_refused() {
        let f = fixture();
        let mut request = f.request.clone();
        request.counter = identity(4).device_id();
        assert_eq!(
            check_request(&f.offer, &request).unwrap_err(),
            PairError::RequestSignatureInvalid
        );
    }

    #[test]
    fn request_resigned_by_an_attacker_shows_as_another_counter_and_digest() {
        let f = fixture();
        let (_, forged) = CounterPairing::begin(
            &f.offer.to_bytes(),
            &identity(4),
            "Front counter",
            f.request.nonce,
            T0,
        )
        .unwrap();
        // It passes the checks, but it names the attacker's key ...
        let pending = check_request(&f.offer, &forged).unwrap();
        assert_ne!(pending.counter(), f.request.counter);
        // ... so the session digest, and with it the words, differ.
        assert_ne!(
            session_digest(&f.offer, &pending.counter(), &forged.nonce),
            session_digest(&f.offer, &f.request.counter, &f.request.nonce)
        );
    }

    #[test]
    fn request_with_changed_name_or_nonce_fails_its_signature() {
        let f = fixture();
        let mut r = f.request.clone();
        r.name = "Back counter".into();
        assert_eq!(
            check_request(&f.offer, &r).unwrap_err(),
            PairError::RequestSignatureInvalid
        );
        let mut r = f.request.clone();
        r.nonce[0] ^= 1;
        assert_eq!(
            check_request(&f.offer, &r).unwrap_err(),
            PairError::RequestSignatureInvalid
        );
    }

    #[test]
    fn check_request_refuses_bad_name_and_root_counters() {
        let f = fixture();
        let mut r = f.request.clone();
        r.name = "bad\nname".into();
        assert_eq!(
            check_request(&f.offer, &r).unwrap_err(),
            PairError::InvalidName
        );
        for root in [f.offer.owner, f.offer.approver] {
            let mut r = f.request.clone();
            r.counter = root;
            assert_eq!(
                check_request(&f.offer, &r).unwrap_err(),
                PairError::CounterIsRoot
            );
        }
    }

    #[test]
    fn begin_refuses_a_root_as_counter_a_bad_name_and_a_bad_offer() {
        let bytes = offer().to_bytes();
        for n in [1, 2] {
            assert_eq!(
                CounterPairing::begin(&bytes, &identity(n), "A", [9; 32], T0).unwrap_err(),
                PairError::CounterIsRoot
            );
        }
        assert_eq!(
            CounterPairing::begin(&bytes, &identity(3), "", [9; 32], T0).unwrap_err(),
            PairError::InvalidName
        );
        assert_eq!(
            CounterPairing::begin(&bytes[..50], &identity(3), "A", [9; 32], T0).unwrap_err(),
            PairError::Decode(DecodeError::Truncated)
        );
    }

    #[test]
    fn finish_refuses_when_words_not_confirmed() {
        let f = fixture();
        assert_eq!(
            f.counter.finish(&f.signed, false, T0).unwrap_err(),
            PairError::NotConfirmed
        );
    }

    #[test]
    fn finish_accepts_at_600_seconds_and_refuses_at_601() {
        let f = fixture();
        assert!(f.counter.finish(&f.signed, true, T0 + 600).is_ok());
        let f = fixture();
        assert_eq!(
            f.counter.finish(&f.signed, true, T0 + 601).unwrap_err(),
            PairError::Expired
        );
        let f = fixture();
        assert!(f.counter.finish(&f.signed, true, T0).is_ok());
    }

    #[test]
    fn finish_refuses_when_the_clock_went_back() {
        let f = fixture();
        assert_eq!(
            f.counter.finish(&f.signed, true, T0 - 1).unwrap_err(),
            PairError::ClockWentBack
        );
    }

    #[test]
    fn finish_checks_confirmation_before_time_and_time_before_content() {
        let f = fixture();
        let mut bad = f.signed.clone();
        bad.entry.wallet = [0; 32];
        assert_eq!(
            f.counter.finish(&bad, false, T0 + 9_999).unwrap_err(),
            PairError::NotConfirmed
        );
        let f = fixture();
        assert_eq!(
            f.counter.finish(&bad, true, T0 + 9_999).unwrap_err(),
            PairError::Expired
        );
    }

    #[test]
    fn finish_refuses_wrong_wallet_counter_nonce_and_name() {
        type Change = fn(&mut PairingEntry);
        let cases: [(Change, PairError); 4] = [
            (|e| e.wallet = [8; 32], PairError::WrongWallet),
            (
                |e| e.counter = identity(4).device_id(),
                PairError::WrongCounter,
            ),
            (|e| e.nonce = [1; 32], PairError::WrongNonce),
            (|e| e.name = "Other".into(), PairError::WrongName),
        ];
        for (change, want) in cases {
            let f = fixture();
            let mut entry = f.entry.clone();
            change(&mut entry);
            // Properly signed by both roots, so only the content is wrong.
            let signed = signed_by(&entry, &identity(2), &identity(1));
            assert_eq!(f.counter.finish(&signed, true, T0).unwrap_err(), want);
        }
    }

    #[test]
    fn finish_refuses_an_approver_signature_from_the_wrong_key() {
        for wrong in [1, 4] {
            let f = fixture();
            let mut s = f.signed.clone();
            s.approver_sig = identity(wrong).sign_digest(&s.entry.hash());
            assert_eq!(
                f.counter.finish(&s, true, T0).unwrap_err(),
                PairError::ApproverSignatureInvalid
            );
        }
    }

    #[test]
    fn finish_refuses_an_owner_signature_from_the_wrong_key() {
        for wrong in [2, 4] {
            let f = fixture();
            let mut s = f.signed.clone();
            s.owner_sig = identity(wrong).sign_digest(&s.entry.hash());
            assert_eq!(
                f.counter.finish(&s, true, T0).unwrap_err(),
                PairError::OwnerSignatureInvalid
            );
        }
    }

    #[test]
    fn finish_refuses_the_two_signatures_swapped() {
        let f = fixture();
        let mut s = f.signed.clone();
        std::mem::swap(&mut s.approver_sig, &mut s.owner_sig);
        assert_eq!(
            f.counter.finish(&s, true, T0).unwrap_err(),
            PairError::ApproverSignatureInvalid
        );
    }

    #[test]
    fn finish_refuses_a_changed_settings_version() {
        let f = fixture();
        let mut s = f.signed.clone();
        s.entry.settings_version = 0;
        assert_eq!(
            f.counter.finish(&s, true, T0).unwrap_err(),
            PairError::ApproverSignatureInvalid
        );
    }

    #[test]
    fn signed_pairing_from_one_ceremony_is_refused_by_the_next() {
        let first = fixture_with(3, "Front counter", [9; 32]);
        let second = fixture_with(3, "Front counter", [10; 32]);
        assert_eq!(
            second.counter.finish(&first.signed, true, T0).unwrap_err(),
            PairError::WrongNonce
        );
        let second = fixture_with(3, "Front counter", [10; 32]);
        assert!(second.counter.finish(&second.signed, true, T0).is_ok());
    }

    #[test]
    fn owner_cosign_refuses_each_bad_condition() {
        let f = fixture();
        let own = f.offer;
        let entry = f.entry.clone();
        let sig = f.approver_sig;
        // Not the owner.
        for n in [2, 4] {
            assert_eq!(
                owner_cosign(&own, entry.clone(), sig, &identity(n)).unwrap_err(),
                PairError::NotTheOwner
            );
        }
        // Wrong wallet.
        let mut e = entry.clone();
        e.wallet = [8; 32];
        assert_eq!(
            owner_cosign(&own, e, sig, &identity(1)).unwrap_err(),
            PairError::WrongWallet
        );
        // Bad names.
        for name in ["", "tab\there", &"x".repeat(33)] {
            let mut e = entry.clone();
            e.name = name.into();
            assert_eq!(
                owner_cosign(&own, e, sig, &identity(1)).unwrap_err(),
                PairError::InvalidName
            );
        }
        // Counter is a root.
        for root in [own.owner, own.approver] {
            let mut e = entry.clone();
            e.counter = root;
            assert_eq!(
                owner_cosign(&own, e, sig, &identity(1)).unwrap_err(),
                PairError::CounterIsRoot
            );
        }
        // Approver signature from the wrong key, made by the owner, empty,
        // or over a different entry.
        for bad in [
            identity(4).sign_digest(&entry.hash()),
            identity(1).sign_digest(&entry.hash()),
            Sig([0; 64]),
        ] {
            assert_eq!(
                owner_cosign(&own, entry.clone(), bad, &identity(1)).unwrap_err(),
                PairError::ApproverSignatureInvalid
            );
        }
        let mut e = entry.clone();
        e.settings_version += 1;
        assert_eq!(
            owner_cosign(&own, e, sig, &identity(1)).unwrap_err(),
            PairError::ApproverSignatureInvalid
        );
    }

    #[test]
    fn approve_refuses_an_identity_that_is_not_the_approver() {
        for n in [1, 3, 4] {
            let f = fixture();
            let pending = check_request(&f.offer, &f.request).unwrap();
            assert_eq!(
                pending.approve(&identity(n), 5).unwrap_err(),
                PairError::NotTheApprover
            );
        }
    }

    #[test]
    fn name_rule_edges() {
        assert!(!name_is_valid(""));
        assert!(name_is_valid("a"));
        assert!(name_is_valid(&"a".repeat(32)));
        assert!(!name_is_valid(&"a".repeat(33)));
        assert!(!name_is_valid("a\u{0}b"));
        assert!(!name_is_valid("a\nb"));
        assert!(!name_is_valid("a\u{7f}"));
        assert!(!name_is_valid("a\u{85}"));
        // 32 four-byte characters are 128 bytes.
        let long = "\u{1F600}".repeat(32);
        assert_eq!(long.len(), 128);
        assert!(name_is_valid(&long));
        assert!(!name_is_valid(&"\u{1F600}".repeat(33)));
    }

    #[test]
    fn offer_wire_form_is_strict() {
        let o = offer();
        let bytes = o.to_bytes();
        assert_eq!(bytes.len(), 101);
        assert_eq!(PairOffer::from_bytes(&bytes).unwrap(), o);
        assert_strict_decoding(&bytes, PairOffer::from_bytes, PairOffer::to_bytes);
        let mut m = bytes.clone();
        m[..4].copy_from_slice(b"OBMX");
        assert_eq!(
            PairOffer::from_bytes(&m).unwrap_err(),
            DecodeError::BadMagic
        );
        let mut v = bytes.clone();
        v[4] = 2;
        assert_eq!(
            PairOffer::from_bytes(&v).unwrap_err(),
            DecodeError::UnsupportedVersion(2)
        );
        // Equal roots are refused at decode.
        let mut same = bytes.clone();
        same.copy_within(5..37, 37);
        assert!(matches!(
            PairOffer::from_bytes(&same).unwrap_err(),
            DecodeError::NonCanonical(_)
        ));
        // An invalid key is refused.
        let mut bad = bytes;
        bad[5..37].fill(0);
        assert_eq!(
            PairOffer::from_bytes(&bad).unwrap_err(),
            DecodeError::InvalidKey
        );
    }

    #[test]
    fn request_wire_form_is_strict() {
        let f = fixture();
        let bytes = f.request.to_bytes();
        assert_eq!(PairRequest::from_bytes(&bytes).unwrap(), f.request);
        assert_strict_decoding(&bytes, PairRequest::from_bytes, PairRequest::to_bytes);
        let mut m = bytes.clone();
        m[..4].copy_from_slice(b"OBMX");
        assert_eq!(
            PairRequest::from_bytes(&m).unwrap_err(),
            DecodeError::BadMagic
        );
        let mut v = bytes.clone();
        v[4] = 0;
        assert_eq!(
            PairRequest::from_bytes(&v).unwrap_err(),
            DecodeError::UnsupportedVersion(0)
        );
        // A decoded request still passes the Key app's check.
        let back = PairRequest::from_bytes(&bytes).unwrap();
        assert!(check_request(&f.offer, &back).is_ok());
    }

    #[test]
    fn request_decoding_applies_the_name_rule_but_not_the_signature() {
        let f = fixture();
        // Name length byte of 0.
        let mut bytes = f.request.to_bytes();
        let name_len = f.request.name.len();
        bytes.drain(5 + 32 + 1..5 + 32 + 1 + name_len);
        bytes[5 + 32] = 0;
        assert!(matches!(
            PairRequest::from_bytes(&bytes).unwrap_err(),
            DecodeError::NonCanonical(_)
        ));
        // A control character.
        let mut bytes = f.request.to_bytes();
        bytes[5 + 32 + 1] = b'\t';
        assert!(matches!(
            PairRequest::from_bytes(&bytes).unwrap_err(),
            DecodeError::NonCanonical(_)
        ));
        // Not UTF-8.
        let mut bytes = f.request.to_bytes();
        bytes[5 + 32 + 1] = 0xff;
        assert!(matches!(
            PairRequest::from_bytes(&bytes).unwrap_err(),
            DecodeError::NonCanonical(_)
        ));
        // A changed name decodes, and fails only at the signature check.
        let mut r = f.request.clone();
        r.name = "Back".into();
        let decoded = PairRequest::from_bytes(&r.to_bytes()).unwrap();
        assert_eq!(decoded, r);
        assert_eq!(
            check_request(&f.offer, &decoded).unwrap_err(),
            PairError::RequestSignatureInvalid
        );
    }

    #[test]
    fn signed_pairing_wire_form_is_strict() {
        let f = fixture();
        let bytes = f.signed.to_bytes();
        assert_eq!(SignedPairing::from_bytes(&bytes).unwrap(), f.signed);
        assert_strict_decoding(&bytes, SignedPairing::from_bytes, SignedPairing::to_bytes);
        let mut m = bytes.clone();
        m[..4].copy_from_slice(b"OBMX");
        assert_eq!(
            SignedPairing::from_bytes(&m).unwrap_err(),
            DecodeError::BadMagic
        );
        let mut v = bytes.clone();
        v[4] = 9;
        assert_eq!(
            SignedPairing::from_bytes(&v).unwrap_err(),
            DecodeError::UnsupportedVersion(9)
        );
        // The entry bytes are exactly what is hashed.
        let enc = f.entry.encode();
        assert_eq!(&bytes[5..5 + enc.len()], enc.as_slice());
    }

    #[test]
    fn entry_encoding_is_wallet_counter_name_nonce_version() {
        let e = PairingEntry {
            wallet: [1; 32],
            counter: identity(3).device_id(),
            name: "ab".into(),
            nonce: [2; 32],
            settings_version: 0x0102_0304,
        };
        let mut want = vec![1u8; 32];
        want.extend_from_slice(&identity(3).device_id().to_bytes());
        want.extend_from_slice(&[2, b'a', b'b']);
        want.extend_from_slice(&[2u8; 32]);
        want.extend_from_slice(&[4, 3, 2, 1]);
        assert_eq!(e.encode(), want);
        assert_eq!(e.hash(), tagged_hash(TAG_PAIRING, &want));
    }

    #[test]
    fn tagged_signatures_do_not_cross() {
        let f = fixture();
        let body = PairRequest::body(
            &f.request.counter,
            &f.request.name,
            &f.request.nonce,
            &f.request.binding,
        );
        let c = f.request.counter;
        assert!(c.verify_tagged(TAG_PAIR_REQUEST, &body, &f.request.sig));
        assert!(!c.verify_tagged(TAG_PAIRING, &body, &f.request.sig));
        assert!(!c.verify_tagged(TAG_PAIR_OFFER, &body, &f.request.sig));
        // Nor does a pairing-entry signature pass as a request signature.
        let sig = identity(3).sign_tagged(TAG_PAIRING, &body);
        assert!(!c.verify_tagged(TAG_PAIR_REQUEST, &body, &sig));
    }

    #[test]
    fn errors_display_and_wrap_decode_errors() {
        assert_eq!(
            PairError::from(DecodeError::Truncated),
            PairError::Decode(DecodeError::Truncated)
        );
        assert!(!PairError::Expired.to_string().is_empty());
        assert_eq!(
            PairError::Decode(DecodeError::BadMagic).to_string(),
            DecodeError::BadMagic.to_string()
        );
    }
}
