//! Pairing a counter with the shop's own wallet (`docs/COUNTER-RULES.md`
//! section 23, CR-227 to CR-238).
//!
//! A shop that uses its own wallet has no Key app. The owner phone makes an
//! [`OwnOffer`] that names the owner root, the settings floor and the pinned
//! wallet (`OBM)`); the counter derives the wallet's first receive address
//! from it and the person compares that address with the wallet's own screen
//! before the counter makes a key or a number. Then the ceremony is that of
//! [`crate::pairing`] with one signature, the owner's:
//!
//! 1. The counter reads the offer ([`OwnOffer::from_bytes`]) and shows the
//!    first address and the kind of wallet.
//! 2. After "Same address" it makes a key and a number and shows its
//!    [`OwnRequest`] and four words ([`OwnCounterPairing::begin`]).
//! 3. The owner phone checks the request against its own offer
//!    ([`check_own_request`]), shows the same four words, and, after the
//!    owner approved, signs the pairing entry ([`OwnPending::sign`]) and
//!    builds the [`OwnBundle`] ([`OwnBundle::build`]).
//! 4. The counter takes the bundle ([`OwnCounterPairing::finish`] and the
//!    checks of the counter node).
//!
//! The binding and the four words are made under tags of their own, so a
//! request or words made for a Key app offer never match an own-wallet offer
//! and the reverse.
//!
//! The counter also keeps its ranges in a [`RangeLedger`], the `OBM;` slot
//! (CR-234): every signed range it took, in the order it took them, and the
//! next index of each.
//!
//! Wire forms (all strict, see [`crate::wire`]):
//!
//! ```text
//! offer:   "OBM-" | 1 | network u8 (0 only) | owner (32) | settings floor u32 LE
//!            | pinned wallet ("OBM)", to the end)
//! request: "OBM." | 1 | counter (32) | name len u8 | name | nonce (32)
//!            | binding (32) | sig (64)
//! pairing: wallet hash (32) | counter (32) | name len u8 | name | nonce (32)
//!            | settings version u32 LE | owner sig (64)
//! bundle:  "OBM/" | 1 or 2
//!            | pairing len u16 | pairing
//!            | settings len u32 | signed settings ("OBMS")
//!            | signed range ("OBM*", 109 bytes)
//!            | peer count u8 | per peer: len u8 | "host:port" (ascending, no repeats)
//!            | version 2 only: relay len u8 | the owner's signed relay ("OBMz")
//! ledger:  "OBM;" | 1 | count u32 | per range: signed range ("OBM*") | next u32 LE
//! ```
//!
//! The version of a bundle is 2 exactly when it carries a relay, as for
//! [`crate::bundle::CounterBundle`]. Decoding only parses: signatures and
//! the rules of the settings and the range are checked by the steps above
//! and by the counter node.

use std::fmt;

use bitcoin::{Address, Network};

use crate::batch::DEFAULT_MIN_COUNT;
use crate::bundle::{peer_is_valid, BundleError, MAX_PEERS};
use crate::identity::{
    tagged_hash, DeviceId, Identity, Sig, TAG_PAIRING_OWN, TAG_PAIR_OWN_BINDING,
    TAG_PAIR_OWN_REQUEST, TAG_PAIR_OWN_SESSION,
};
use crate::pairing::{
    name_is_valid, network_code, network_from_code, put_name, read_name, PairingEntry,
    PAIRING_WINDOW_SECS,
};
use crate::pinned_wallet::{
    IndexRange, PinnedError, PinnedWallet, RangeBook, RangeError, SignedRange, WalletKind,
};
use crate::relay::SignedRelayHint;
use crate::settings::SignedSettings;
use crate::wire::{self, DecodeError, Reader};
use crate::words::{fingerprint_words, WORD_COUNT};

/// Longest pairing the bundle reads, in bytes (the longest real one is under
/// 300).
const MAX_PAIRING_BYTES: usize = 512;
/// Longest signed settings the bundle reads, in bytes.
const MAX_SETTINGS_BYTES: usize = 64 * 1024;
/// Bytes of a signed range on the wire.
const RANGE_BYTES: usize = 109;
/// The most ranges a ledger decodes.
const MAX_LEDGER_RANGES: usize = 1 << 20;

/// Why a step of an own-wallet pairing was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnPairError {
    /// CR-227: the offer's network is not the main network.
    NotMainNetwork,
    /// CR-229: the name is empty, too long, or holds a control character.
    InvalidName,
    /// CR-229: the counter's key is the owner root.
    CounterIsOwner,
    /// The bytes were not a valid message.
    Decode(DecodeError),
    /// CR-229: the request was made for another offer.
    BindingMismatch,
    /// CR-229: the request's signature does not verify under the counter's
    /// key.
    RequestSignatureInvalid,
    /// CR-230: the person did not confirm that the words match.
    NotConfirmed,
    /// CR-230: the time given is earlier than the offer was read.
    ClockWentBack,
    /// CR-230: the ceremony took longer than [`PAIRING_WINDOW_SECS`].
    Expired,
    /// CR-230: the pairing is for another wallet than the offer's.
    WrongWallet,
    /// CR-230: the pairing names another counter.
    WrongCounter,
    /// CR-230: the pairing carries another number.
    WrongNonce,
    /// CR-230: the pairing carries another name.
    WrongName,
    /// CR-230: the owner's signature does not verify.
    OwnerSignatureInvalid,
    /// This identity is not the owner root of the offer.
    NotTheOwner,
    /// The bundle's peers are not acceptable.
    Bundle(BundleError),
    /// CR-232: the first range is not signed by the owner root, is for
    /// another counter, or has a shape that is refused.
    Range(RangeError),
    /// CR-232: the first range holds fewer indexes than the smallest range
    /// the counter takes.
    RangeTooSmall {
        /// How many indexes the range holds.
        got: u32,
        /// How many it must hold.
        need: u32,
    },
}

impl fmt::Display for OwnPairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotMainNetwork => write!(f, "CR-227: the offer is not for the main network"),
            Self::InvalidName => write!(f, "CR-229: the counter's name is not allowed"),
            Self::CounterIsOwner => write!(f, "CR-229: the counter's key is the owner's"),
            Self::Decode(e) => write!(f, "{e}"),
            Self::BindingMismatch => write!(f, "CR-229: the request was made for another offer"),
            Self::RequestSignatureInvalid => {
                write!(f, "CR-229: the request signature is not valid")
            }
            Self::NotConfirmed => write!(f, "CR-230: the words were not confirmed"),
            Self::ClockWentBack => {
                write!(f, "CR-230: the clock is earlier than the start of pairing")
            }
            Self::Expired => write!(f, "CR-230: pairing took too long"),
            Self::WrongWallet => write!(f, "CR-230: pairing is for another wallet"),
            Self::WrongCounter => write!(f, "CR-230: pairing names another counter"),
            Self::WrongNonce => write!(f, "CR-230: pairing carries another number"),
            Self::WrongName => write!(f, "CR-230: pairing carries another name"),
            Self::OwnerSignatureInvalid => write!(f, "CR-230: the owner signature is not valid"),
            Self::NotTheOwner => write!(f, "this is not the owner root of the offer"),
            Self::Bundle(e) => write!(f, "{e}"),
            Self::Range(e) => write!(f, "CR-232: {e}"),
            Self::RangeTooSmall { got, need } => write!(
                f,
                "CR-232: the first range holds {got} indexes and the least is {need}"
            ),
        }
    }
}

impl std::error::Error for OwnPairError {}

impl From<DecodeError> for OwnPairError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

impl From<BundleError> for OwnPairError {
    fn from(e: BundleError) -> Self {
        Self::Bundle(e)
    }
}

impl From<RangeError> for OwnPairError {
    fn from(e: RangeError) -> Self {
        Self::Range(e)
    }
}

/// A wallet the pinned-wallet decoder refused, as the error a frame that
/// holds it reports.
fn pinned_decode_error(e: PinnedError) -> DecodeError {
    match e {
        PinnedError::Decode(d) => d,
        PinnedError::Import(_) => DecodeError::NonCanonical("the wallet is refused by the import"),
        PinnedError::NotCanonical => DecodeError::NonCanonical("the wallet text is not canonical"),
        PinnedError::ChecksumMismatch => {
            DecodeError::NonCanonical("the wallet hash does not match its text")
        }
        PinnedError::Derive => DecodeError::NonCanonical("the wallet has no address"),
    }
}

/// What the owner phone shows (CR-227).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnOffer {
    /// The owner phone's root.
    pub owner: DeviceId,
    /// The settings version in force on the owner phone when it made the
    /// offer. The settings in the bundle must be above it.
    pub floor: u32,
    /// The wallet the counter pins.
    pub wallet: PinnedWallet,
}

impl OwnOffer {
    /// An offer. The network must be the main network, the one every pinned
    /// wallet is for (CR-227).
    pub fn new(
        network: Network,
        owner: DeviceId,
        floor: u32,
        wallet: PinnedWallet,
    ) -> Result<Self, OwnPairError> {
        if network != Network::Bitcoin {
            return Err(OwnPairError::NotMainNetwork);
        }
        Ok(Self {
            owner,
            floor,
            wallet,
        })
    }

    /// The network of the offer: the main network.
    pub fn network(&self) -> Network {
        Network::Bitcoin
    }

    /// The offer's bytes without the frame: the network's code, the owner
    /// root, the floor and the pinned wallet. The binding and the session
    /// digest hash them.
    fn body(&self) -> Vec<u8> {
        let wallet = self.wallet.to_bytes();
        let mut out = Vec::with_capacity(37 + wallet.len());
        out.push(network_code(Network::Bitcoin).unwrap_or(u8::MAX));
        out.extend_from_slice(&self.owner.to_bytes());
        out.extend_from_slice(&self.floor.to_le_bytes());
        out.extend_from_slice(&wallet);
        out
    }

    /// CR-229: the hash of the whole offer that goes into the request.
    pub fn binding(&self) -> [u8; 32] {
        tagged_hash(TAG_PAIR_OWN_BINDING, &self.body())
    }

    /// CR-228: the first receive address of the wallet (receive chain,
    /// index 0).
    pub fn first_address(&self) -> Result<Address, PinnedError> {
        self.wallet.receive_address(0)
    }

    /// CR-228: the kind of wallet, for the line beneath the address.
    pub fn kind(&self) -> WalletKind {
        self.wallet.kind()
    }

    /// Wire form: `OBM-`, version 1, network, owner root, floor, pinned
    /// wallet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.body();
        let mut out = wire::start(&wire::MAGIC_OWN_OFFER, body.len());
        out.extend_from_slice(&body);
        out
    }

    /// Parse the wire form strictly (CR-227): exact length, the network the
    /// main one, the owner root a key, and the pinned wallet decoded and
    /// re-encoded to the same bytes under WL-21 and WL-22.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_OWN_OFFER)?;
        let network = network_from_code(r.u8()?)
            .ok_or(DecodeError::NonCanonical("the network code is unknown"))?;
        if network != Network::Bitcoin {
            return Err(DecodeError::NonCanonical(
                "the network is not the main network",
            ));
        }
        let owner = r.device_id()?;
        let floor = r.u32_le()?;
        // The pinned wallet runs to the end of the frame and is exact.
        let wallet = bytes.get(5 + 1 + 32 + 4..).ok_or(DecodeError::Truncated)?;
        let pinned = PinnedWallet::from_bytes(wallet).map_err(pinned_decode_error)?;
        if pinned.to_bytes() != wallet {
            return Err(DecodeError::NonCanonical("the wallet is not canonical"));
        }
        Ok(Self {
            owner,
            floor,
            wallet: pinned,
        })
    }
}

/// The counter's request (CR-229).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnRequest {
    /// The counter's own identity key.
    pub counter: DeviceId,
    /// The counter's name (see [`name_is_valid`]).
    pub name: String,
    /// The fresh number made for this ceremony.
    pub nonce: [u8; 32],
    /// [`OwnOffer::binding`] of the offer the counter read.
    pub binding: [u8; 32],
    /// The counter's signature over the other fields.
    pub sig: Sig,
}

impl OwnRequest {
    fn body(counter: &DeviceId, name: &str, nonce: &[u8; 32], binding: &[u8; 32]) -> Vec<u8> {
        let mut out = Vec::with_capacity(98 + name.len());
        out.extend_from_slice(&counter.to_bytes());
        put_name(&mut out, name);
        out.extend_from_slice(nonce);
        out.extend_from_slice(binding);
        out
    }

    /// Wire form: `OBM.`, version 1, the signed bytes, then the signature.
    /// The name must pass [`name_is_valid`].
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = Self::body(&self.counter, &self.name, &self.nonce, &self.binding);
        let mut out = wire::start(&wire::MAGIC_OWN_REQUEST, body.len() + 64);
        out.extend_from_slice(&body);
        out.extend_from_slice(&self.sig.0);
        out
    }

    /// Parse the wire form strictly and check the name rule. The signature
    /// is not checked: [`check_own_request`] does.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_OWN_REQUEST)?;
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
            .verify_tagged(TAG_PAIR_OWN_REQUEST, &body, &self.sig)
    }
}

/// CR-229: the digest behind the session words, made from the whole offer,
/// the counter's key and the number under its own tag.
pub fn own_session_digest(offer: &OwnOffer, counter: &DeviceId, nonce: &[u8; 32]) -> [u8; 32] {
    let mut msg = offer.body();
    msg.reserve(64);
    msg.extend_from_slice(&counter.to_bytes());
    msg.extend_from_slice(nonce);
    tagged_hash(TAG_PAIR_OWN_SESSION, &msg)
}

/// CR-229: the four words both screens show, by the stretch of
/// [`crate::words`]. Slow on purpose.
pub fn own_session_words(
    offer: &OwnOffer,
    counter: &DeviceId,
    nonce: &[u8; 32],
) -> [&'static str; WORD_COUNT] {
    fingerprint_words(&own_session_digest(offer, counter, nonce))
}

/// The pairing entry signed by the owner root alone (CR-230).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSignedPairing {
    /// The entry: the hash of the pinned wallet, the counter, its name, the
    /// number, the settings version.
    pub entry: PairingEntry,
    /// The owner root's signature over the entry under
    /// [`TAG_PAIRING_OWN`].
    pub owner_sig: Sig,
}

impl OwnSignedPairing {
    /// Sign an entry with the owner root.
    pub fn sign(owner: &Identity, entry: PairingEntry) -> Self {
        let owner_sig = owner.sign_tagged(TAG_PAIRING_OWN, &entry.encode());
        Self { entry, owner_sig }
    }

    /// CR-230: the pairing names this counter and this wallet hash and is
    /// signed by this owner root. The name and the number are the ceremony's
    /// to check.
    pub fn check(
        &self,
        owner: &DeviceId,
        wallet: &[u8; 32],
        counter: &DeviceId,
    ) -> Result<(), OwnPairError> {
        if self.entry.counter != *counter {
            return Err(OwnPairError::WrongCounter);
        }
        if self.entry.wallet != *wallet {
            return Err(OwnPairError::WrongWallet);
        }
        if !owner.verify_tagged(TAG_PAIRING_OWN, &self.entry.encode(), &self.owner_sig) {
            return Err(OwnPairError::OwnerSignatureInvalid);
        }
        Ok(())
    }

    /// The bytes of the entry, then the signature.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.entry.encode();
        out.extend_from_slice(&self.owner_sig.0);
        out
    }

    /// Parse strictly: the entry, the name rule, the signature, no byte
    /// over.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        let entry = PairingEntry::read(&mut r)?;
        let owner_sig = r.sig()?;
        r.finish()?;
        Ok(Self { entry, owner_sig })
    }
}

/// CR-229 on the owner phone: check the counter's request against the
/// phone's own offer. The binding is compared in full, then the name rule,
/// then that the counter is not the owner root, then the signature.
pub fn check_own_request(
    offer: &OwnOffer,
    request: &OwnRequest,
) -> Result<OwnPending, OwnPairError> {
    if request.binding != offer.binding() {
        return Err(OwnPairError::BindingMismatch);
    }
    if !name_is_valid(&request.name) {
        return Err(OwnPairError::InvalidName);
    }
    if request.counter == offer.owner {
        return Err(OwnPairError::CounterIsOwner);
    }
    if !request.signature_ok() {
        return Err(OwnPairError::RequestSignatureInvalid);
    }
    Ok(OwnPending {
        offer: offer.clone(),
        counter: request.counter,
        name: request.name.clone(),
        nonce: request.nonce,
    })
}

/// A request that passed [`check_own_request`] and waits for the owner to
/// compare the words.
#[derive(Debug)]
pub struct OwnPending {
    offer: OwnOffer,
    counter: DeviceId,
    name: String,
    nonce: [u8; 32],
}

impl OwnPending {
    /// The four words to show; they must equal the counter's. Slow on
    /// purpose.
    pub fn session_words(&self) -> [&'static str; WORD_COUNT] {
        own_session_words(&self.offer, &self.counter, &self.nonce)
    }

    /// The counter's key.
    pub fn counter(&self) -> DeviceId {
        self.counter
    }

    /// The counter's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// After the owner approved: the owner root signs the pairing entry.
    /// `owner` must be the owner root of the offer. The entry names the
    /// offer's settings floor as its settings version.
    pub fn sign(self, owner: &Identity) -> Result<OwnSignedPairing, OwnPairError> {
        if owner.device_id() != self.offer.owner {
            return Err(OwnPairError::NotTheOwner);
        }
        let entry = PairingEntry {
            wallet: self.offer.wallet.checksum32(),
            counter: self.counter,
            name: self.name,
            nonce: self.nonce,
            settings_version: self.offer.floor,
        };
        Ok(OwnSignedPairing::sign(owner, entry))
    }
}

/// The counter's side of one own-wallet ceremony after "Same address". It is
/// used once: [`finish`](OwnCounterPairing::finish) takes it by value.
#[derive(Debug)]
pub struct OwnCounterPairing {
    offer: OwnOffer,
    own: DeviceId,
    name: String,
    nonce: [u8; 32],
    started_at: u64,
}

impl OwnCounterPairing {
    /// CR-229: make the request. `identity` and `nonce` come from the
    /// caller's cryptographic random source, new for this ceremony, and are
    /// made only after "Same address". `started_at` is the time the offer
    /// was read, in seconds: the ten minutes run from it.
    pub fn begin(
        offer: OwnOffer,
        identity: &Identity,
        name: &str,
        nonce: [u8; 32],
        started_at: u64,
    ) -> Result<(Self, OwnRequest), OwnPairError> {
        let own = identity.device_id();
        if own == offer.owner {
            return Err(OwnPairError::CounterIsOwner);
        }
        if !name_is_valid(name) {
            return Err(OwnPairError::InvalidName);
        }
        let binding = offer.binding();
        let body = OwnRequest::body(&own, name, &nonce, &binding);
        let sig = identity.sign_tagged(TAG_PAIR_OWN_REQUEST, &body);
        let request = OwnRequest {
            counter: own,
            name: name.to_owned(),
            nonce,
            binding,
            sig,
        };
        Ok((
            Self {
                offer,
                own,
                name: name.to_owned(),
                nonce,
                started_at,
            },
            request,
        ))
    }

    /// The four words to show. Slow on purpose.
    pub fn session_words(&self) -> [&'static str; WORD_COUNT] {
        own_session_words(&self.offer, &self.own, &self.nonce)
    }

    /// The counter's own key.
    pub fn counter(&self) -> DeviceId {
        self.own
    }

    /// The time, in seconds, the offer was read.
    pub fn started_at(&self) -> u64 {
        self.started_at
    }

    /// The offer the ceremony runs on.
    pub fn offer(&self) -> &OwnOffer {
        &self.offer
    }

    /// CR-230, the match and the pairing: `words_confirmed` is whether the
    /// person tapped "They match". The checks run in this order and the
    /// first failure wins: confirmed, time, this counter, this number, the
    /// hash of the offer's pinned wallet, the name, the owner root's
    /// signature. The pairing is consumed whatever the outcome.
    pub fn finish(
        self,
        signed: &OwnSignedPairing,
        words_confirmed: bool,
        now: u64,
    ) -> Result<OwnPaired, OwnPairError> {
        if !words_confirmed {
            return Err(OwnPairError::NotConfirmed);
        }
        if now < self.started_at {
            return Err(OwnPairError::ClockWentBack);
        }
        if now - self.started_at > PAIRING_WINDOW_SECS {
            return Err(OwnPairError::Expired);
        }
        let entry = &signed.entry;
        if entry.counter != self.own {
            return Err(OwnPairError::WrongCounter);
        }
        if entry.nonce != self.nonce {
            return Err(OwnPairError::WrongNonce);
        }
        let wallet = self.offer.wallet.checksum32();
        if entry.wallet != wallet {
            return Err(OwnPairError::WrongWallet);
        }
        if entry.name != self.name {
            return Err(OwnPairError::WrongName);
        }
        signed.check(&self.offer.owner, &wallet, &self.own)?;
        Ok(OwnPaired {
            offer: self.offer,
            pairing: signed.clone(),
        })
    }
}

/// What a counter keeps of a finished ceremony.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnPaired {
    /// The offer, as the counter read it.
    pub offer: OwnOffer,
    /// The pairing the owner root signed.
    pub pairing: OwnSignedPairing,
}

/// CR-232, CR-235: a range is signed by the owner root, names this counter,
/// has a valid shape and holds at least `min_count` indexes. The smallest
/// size is the counter's own: pass [`DEFAULT_MIN_COUNT`] unless a test needs
/// less.
pub fn check_range(
    range: &SignedRange,
    owner: &DeviceId,
    counter: &DeviceId,
    min_count: u32,
) -> Result<(), OwnPairError> {
    range.verify(owner)?;
    if range.range.counter != *counter {
        return Err(RangeError::WrongCounter.into());
    }
    let got = range.range.end - range.range.start;
    if got < min_count {
        return Err(OwnPairError::RangeTooSmall {
            got,
            need: min_count,
        });
    }
    Ok(())
}

/// The smallest first range, in indexes: the batch minimum (CR-232).
pub const FIRST_RANGE_MIN_COUNT: u32 = DEFAULT_MIN_COUNT;

/// What the owner phone hands the counter (CR-230).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnBundle {
    /// The pairing the owner root signed.
    pub pairing: OwnSignedPairing,
    /// The counter's first settings.
    pub settings: SignedSettings,
    /// The first range of indexes.
    pub first_range: SignedRange,
    /// Peers to try first, in ascending order with no repeats (CR-99).
    pub peers: Vec<String>,
    /// The relay the counter reaches the owner through, if the shop has one
    /// (CR-131). Decoding does not check its signature.
    pub relay: Option<SignedRelayHint>,
}

impl OwnBundle {
    /// A bundle. The peers are sorted and a repeat is dropped; each must pass
    /// [`peer_is_valid`], and there may be at most [`MAX_PEERS`].
    pub fn new(
        pairing: OwnSignedPairing,
        settings: SignedSettings,
        first_range: SignedRange,
        mut peers: Vec<String>,
        relay: Option<SignedRelayHint>,
    ) -> Result<Self, BundleError> {
        peers.sort();
        peers.dedup();
        if peers.len() > MAX_PEERS {
            return Err(BundleError::TooManyPeers);
        }
        if !peers.iter().all(|p| peer_is_valid(p)) {
            return Err(BundleError::BadPeer);
        }
        Ok(Self {
            pairing,
            settings,
            first_range,
            peers,
            relay,
        })
    }

    /// The owner phone's side: check the counter's request against the
    /// offer ([`check_own_request`]), sign the pairing with the owner root
    /// and make the bundle.
    pub fn build(
        owner: &Identity,
        offer: &OwnOffer,
        request: &OwnRequest,
        settings: SignedSettings,
        first_range: SignedRange,
        peers: Vec<String>,
        relay: Option<SignedRelayHint>,
    ) -> Result<Self, OwnPairError> {
        let pending = check_own_request(offer, request)?;
        let pairing = pending.sign(owner)?;
        Ok(Self::new(pairing, settings, first_range, peers, relay)?)
    }

    /// Wire form: see the module docs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let pairing = self.pairing.to_bytes();
        let settings = self.settings.to_bytes();
        let mut out = wire::start(
            &wire::MAGIC_OWN_BUNDLE,
            pairing.len() + settings.len() + RANGE_BYTES + 16 + self.peers.len() * 24 + 100,
        );
        if self.relay.is_some() {
            if let Some(version) = out.get_mut(4) {
                *version = 2;
            }
        }
        out.extend_from_slice(
            &u16::try_from(pairing.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        out.extend_from_slice(&pairing);
        out.extend_from_slice(&wire::len_u32(settings.len()).to_le_bytes());
        out.extend_from_slice(&settings);
        out.extend_from_slice(&self.first_range.to_bytes());
        out.push(u8::try_from(self.peers.len()).unwrap_or(u8::MAX));
        for p in &self.peers {
            out.push(u8::try_from(p.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(p.as_bytes());
        }
        if let Some(relay) = &self.relay {
            let frame = relay.to_bytes();
            out.push(u8::try_from(frame.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(&frame);
        }
        out
    }

    /// Parse the wire form strictly. Every length is checked against its
    /// limit and the bytes present before anything is read, each part is
    /// decoded by its own strict decoder, and peers must be valid, ascending
    /// and without repeats.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (mut r, version) = Reader::framed_up_to(bytes, &wire::MAGIC_OWN_BUNDLE, 2)?;
        let n = usize::from(r.u16_le()?);
        if n > MAX_PAIRING_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let pairing = OwnSignedPairing::from_bytes(r.take(n)?)?;
        let n = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if n > MAX_SETTINGS_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let settings = SignedSettings::from_bytes(r.take(n)?)?;
        let first_range = SignedRange::from_bytes(r.take(RANGE_BYTES)?)?;
        let count = usize::from(r.u8()?);
        if count > MAX_PEERS {
            return Err(DecodeError::TooLarge);
        }
        let mut peers: Vec<String> = Vec::with_capacity(count);
        for _ in 0..count {
            let len = usize::from(r.u8()?);
            let text = std::str::from_utf8(r.take(len)?)
                .map_err(|_| DecodeError::NonCanonical("a peer is not UTF-8"))?;
            if !peer_is_valid(text) {
                return Err(DecodeError::NonCanonical("a peer is not ip:port"));
            }
            if peers.last().is_some_and(|last| last.as_str() >= text) {
                return Err(DecodeError::NonCanonical(
                    "peers are not in ascending order",
                ));
            }
            peers.push(text.to_owned());
        }
        let relay = if version >= 2 {
            let n = usize::from(r.u8()?);
            Some(SignedRelayHint::from_bytes(r.take(n)?)?)
        } else {
            None
        };
        r.finish()?;
        Ok(Self {
            pairing,
            settings,
            first_range,
            peers,
            relay,
        })
    }
}

/// CR-234: the ranges a counter took and the indexes it used in them, as the
/// `OBM;` slot keeps them. The book is the [`RangeBook`]; the signed ranges
/// are kept beside it so that every range can be checked again at start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeLedger {
    owner: DeviceId,
    book: RangeBook,
    signed: Vec<SignedRange>,
}

impl RangeLedger {
    /// An empty ledger for `counter` under the pinned `owner` root.
    pub fn new(owner: DeviceId, counter: DeviceId) -> Self {
        Self {
            owner,
            book: RangeBook::new(counter),
            signed: Vec::new(),
        }
    }

    /// Check the owner's signature and take a range (WL-24 to WL-28). On
    /// refusal the ledger is unchanged.
    pub fn accept(&mut self, range: &SignedRange) -> Result<(), RangeError> {
        self.book.accept_signed(range, &self.owner)?;
        self.signed.push(*range);
        Ok(())
    }

    /// The owner root the ledger checks ranges against.
    pub fn owner(&self) -> DeviceId {
        self.owner
    }

    /// Hand out the next unused index (WL-27).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<u32, RangeError> {
        self.book.next()
    }

    /// How many unused indexes are left.
    pub fn remaining(&self) -> u64 {
        self.book.remaining()
    }

    /// The ranges taken, in the order they were taken, each with the first
    /// of its indexes not yet used.
    pub fn taken(&self) -> Vec<(IndexRange, u32)> {
        self.book.taken()
    }

    /// The signed ranges, in the order they were taken.
    pub fn signed(&self) -> &[SignedRange] {
        &self.signed
    }

    /// Wire form: see the module docs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let taken = self.book.taken();
        let mut out = wire::start(&wire::MAGIC_RANGE_BOOK, 4 + taken.len() * (RANGE_BYTES + 4));
        out.extend_from_slice(&wire::len_u32(self.signed.len()).to_le_bytes());
        for (signed, (_, next)) in self.signed.iter().zip(taken) {
            out.extend_from_slice(&signed.to_bytes());
            out.extend_from_slice(&next.to_le_bytes());
        }
        out
    }

    /// CR-234: read the slot strictly, and check every range again against
    /// the pinned `owner` root and `counter`. A slot that does not read or
    /// does not check is refused.
    pub fn from_bytes(
        bytes: &[u8],
        owner: DeviceId,
        counter: DeviceId,
    ) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_RANGE_BOOK)?;
        let n = r.count(MAX_LEDGER_RANGES, RANGE_BYTES + 4)?;
        let mut signed = Vec::with_capacity(n);
        let mut taken = Vec::with_capacity(n);
        for _ in 0..n {
            let range = SignedRange::from_bytes(r.take(RANGE_BYTES)?)?;
            let next = r.u32_le()?;
            range
                .verify(&owner)
                .map_err(|_| DecodeError::NonCanonical("a range is not signed by the owner"))?;
            signed.push(range);
            taken.push((range.range, next));
        }
        r.finish()?;
        let book = RangeBook::restore(counter, &taken).map_err(|_| {
            DecodeError::NonCanonical("the ranges are not a book this counter keeps")
        })?;
        Ok(Self {
            owner,
            book,
            signed,
        })
    }
}

#[cfg(test)]
mod tests;
