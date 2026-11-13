//! The owner phone of a shop that uses its own wallet, and the pairing of a
//! counter with it (`docs/OWNER-APP-RULES.md` section 17, OA-105 to OA-113;
//! the counter's side is `docs/COUNTER-RULES.md` section 23).
//!
//! There is no Key app. The phone keeps the wallet's public description
//! (OA-105, the caller keeps that file), makes the offer a new counter scans
//! (OA-108), checks the counter's request and shows its four words (OA-109),
//! and after "They match" makes the first settings, the first range and the
//! signed pairing and puts them in one bundle (OA-110). The ranges of every
//! device come from one allocation book (OA-106).
//!
//! Like [`Owner`](crate::Owner), the shop takes the time and a `persist` step
//! from the caller and stores new state before it takes effect. The offer and
//! the request live in the caller's memory for the ten minutes of the
//! ceremony: nothing of an unfinished pairing is stored, and leaving before
//! "They match" queues nothing (OA-110).
//!
//! The snapshot:
//!
//! ```text
//! state: "OBM@" | 1 | owner root (32) | wallet hash (32)
//!          | known settings: flag, then len u32 LE and the `OBMS` bytes
//!          | book len u32 LE | book ("OBM?")
//!          | waiting count u32 LE | per bundle, ascending by counter:
//!            len u32 LE | bundle ("OBM/")
//! ```
//!
//! Decoding is strict: what decodes encodes to the same bytes.

use std::collections::BTreeSet;
use std::fmt;

use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::PAIRING_WINDOW_SECS;
use openbmt_core::pairing_own::{
    check_own_request, OwnBundle, OwnOffer, OwnPairError, OwnRequest, SwitchOwn,
    FIRST_RANGE_MIN_COUNT,
};
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
use openbmt_core::relay::SignedRelayHint;
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_core::words::WORD_COUNT;

use crate::counter::equal_shares;
use crate::edit::sats_per_usd_at;
use crate::error::Refusal;
use crate::own_book::{AllocationBook, BookError};

/// OA-105: the magic of the shop's state.
pub const MAGIC_OWN_STATE: [u8; 4] = *b"OBM@";

/// AS-100: the key a shop with its own wallet gives the owner queue in place
/// of a Key app's, made from the owner's secret and a tag, so that no device
/// holds the key that goes with it. The queue that takes this key is never
/// given a Key app's item. `None` only for a secret that makes no key.
pub fn own_approver(owner_secret: &[u8; 32]) -> Option<DeviceId> {
    use bitcoin::hashes::{sha256, Hash};
    let mut seed = Vec::with_capacity(32 + 32);
    seed.extend_from_slice(b"OpenBMT/own-wallet-no-key-app/v1");
    seed.extend_from_slice(owner_secret);
    let digest = sha256::Hash::hash(&seed).to_byte_array();
    seed.fill(0);
    Identity::from_secret_bytes(&digest)
        .map(|i| i.device_id())
        .ok()
}

/// OA-109: the most bytes of a counter's request the phone reads.
pub const MAX_REQUEST_BYTES: usize = 1_024;

/// OA-108: seconds an offer lives (CR-229).
pub const OFFER_WINDOW_SECS: u64 = PAIRING_WINDOW_SECS;

/// The most finished bundles that wait for their counter.
pub const MAX_WAITING: usize = 16;

/// The largest signed settings a snapshot holds, in bytes.
const MAX_SETTINGS_BYTES: usize = 64 * 1024;
/// The largest bundle a snapshot holds, in bytes.
const MAX_BUNDLE_BYTES: usize = 128 * 1024;

/// Why a step of the pairing, or a range, was refused. Nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnRefusal {
    /// OA-108, OA-109: the offer's 10 minutes are over.
    OfferEnded,
    /// The clock is earlier than the time the offer was made.
    ClockWentBack,
    /// The offer is not the one this shop makes.
    NotThisShopsOffer,
    /// OA-109: the bytes are not a counter's request, or are more than
    /// 1,024 bytes.
    NotARequest,
    /// OA-109: the request was made for another offer.
    OtherOffer,
    /// OA-109: the request's signature fails.
    BadSignature,
    /// OA-109: the counter's key is the owner's own.
    OwnKey,
    /// OA-109: the name is not plain text.
    BadName,
    /// The counter has a range or a bundle waiting already.
    AlreadyAdded,
    /// SS-41: the new wallet is the shop's own wallet.
    SwitchSameWallet,
    /// The settings could not be made, as [`Refusal`] says.
    Settings(Refusal),
    /// OA-106, OA-107: the book gave no range.
    Book(BookError),
    /// No bundle waits for this counter.
    NoBundle,
    /// The book holds no range for this counter, so it gets no top-up.
    UnknownCounter,
    /// As many bundles wait as the shop keeps.
    TooManyWaiting,
    /// A step of the ceremony failed that the checks before it should have
    /// caught.
    Pairing(OwnPairError),
}

impl fmt::Display for OwnRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OfferEnded => write!(f, "OA-108: the offer's 10 minutes are over"),
            Self::ClockWentBack => write!(f, "OA-108: the clock is earlier than the offer"),
            Self::NotThisShopsOffer => write!(f, "OA-108: the offer is not this shop's"),
            Self::NotARequest => write!(f, "OA-109: the bytes are not a counter's request"),
            Self::OtherOffer => write!(f, "OA-109: the request was made for another offer"),
            Self::BadSignature => write!(f, "OA-109: the request's signature fails"),
            Self::OwnKey => write!(f, "OA-109: the counter's key is the owner's own"),
            Self::BadName => write!(f, "OA-109: the counter's name is not plain text"),
            Self::AlreadyAdded => write!(f, "OA-110: the counter is added already"),
            Self::SwitchSameWallet => write!(f, "SS-41: the new wallet is the wallet in use"),
            Self::Settings(r) => write!(f, "{r}"),
            Self::Book(e) => write!(f, "{e}"),
            Self::NoBundle => write!(f, "OA-111: no bundle waits for this counter"),
            Self::UnknownCounter => write!(f, "OA-112: the book holds no range for this counter"),
            Self::TooManyWaiting => {
                write!(f, "OA-111: {MAX_WAITING} bundles wait for their counters")
            }
            Self::Pairing(e) => write!(f, "OA-110: {e}"),
        }
    }
}

impl std::error::Error for OwnRefusal {}

impl From<BookError> for OwnRefusal {
    fn from(e: BookError) -> Self {
        Self::Book(e)
    }
}

/// A refusal, or a failed store (nothing is changed in memory then).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnError<E> {
    /// A rule refused.
    Refused(OwnRefusal),
    /// The store failed.
    Store(E),
}

impl<E> From<OwnRefusal> for OwnError<E> {
    fn from(r: OwnRefusal) -> Self {
        Self::Refused(r)
    }
}

impl<E> From<BookError> for OwnError<E> {
    fn from(e: BookError) -> Self {
        Self::Refused(OwnRefusal::Book(e))
    }
}

impl<E: fmt::Display> fmt::Display for OwnError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::Store(e) => write!(f, "OA-105: the new state could not be stored: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for OwnError<E> {}

/// Why the shop's state did not come back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnRestoreError {
    /// The bytes are not a state, and say why.
    Corrupt(&'static str),
    /// The state is for another owner or another wallet.
    NotThisShop,
}

impl fmt::Display for OwnRestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(why) => write!(f, "OA-105: the shop's state is bad: {why}"),
            Self::NotThisShop => write!(f, "OA-105: the state is for another wallet or owner"),
        }
    }
}

impl std::error::Error for OwnRestoreError {}

fn corrupt(e: DecodeError) -> OwnRestoreError {
    match e {
        DecodeError::NonCanonical(why) => OwnRestoreError::Corrupt(why),
        DecodeError::TrailingBytes => OwnRestoreError::Corrupt("bytes follow the last entry"),
        _ => OwnRestoreError::Corrupt("it is cut short or not framed as it is written"),
    }
}

/// What the caller keeps of an offer for its 10 minutes: the offer and the
/// time it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeOffer {
    /// The offer, to draw as a code ([`OwnOffer::to_bytes`]).
    pub offer: OwnOffer,
    /// When it was made, in seconds.
    pub made_at: u64,
}

impl MadeOffer {
    /// OA-108: the time the offer ends.
    pub fn ends_at(&self) -> u64 {
        self.made_at.saturating_add(OFFER_WINDOW_SECS)
    }
}

/// A request that read (OA-109): what the owner compares with the counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadRequest {
    /// The counter's key.
    pub counter: DeviceId,
    /// The name the counter gave itself. It is text from the counter.
    pub name: String,
    /// The four words, which the counter shows too.
    pub words: [&'static str; WORD_COUNT],
}

/// What "They match" made (OA-110).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paired {
    /// The bundle, to show as a code.
    pub bundle: OwnBundle,
    /// The first range in it.
    pub first_range: IndexRange,
    /// The version of the settings in it.
    pub settings_version: u32,
    /// Each counter's share of the instant limit in the settings, in basis
    /// points, in the order of the counters' keys.
    pub shares: Vec<(DeviceId, u16)>,
}

/// A counter whose bundle waits to be handed over (OA-111).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingOwnCounter {
    /// The counter's key.
    pub counter: DeviceId,
    /// The counter's name.
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OwnState {
    known: Option<SignedSettings>,
    book: AllocationBook,
    /// Ascending by counter.
    waiting: Vec<OwnBundle>,
}

impl OwnState {
    fn encode(&self, owner: &DeviceId, wallet: &[u8; 32]) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_OWN_STATE, 128);
        out.extend_from_slice(&owner.to_bytes());
        out.extend_from_slice(wallet);
        match &self.known {
            Some(s) => {
                let bytes = s.to_bytes();
                out.push(1);
                out.extend_from_slice(&wire::len_u32(bytes.len()).to_le_bytes());
                out.extend_from_slice(&bytes);
            }
            None => out.push(0),
        }
        let book = self.book.to_bytes();
        out.extend_from_slice(&wire::len_u32(book.len()).to_le_bytes());
        out.extend_from_slice(&book);
        out.extend_from_slice(&wire::len_u32(self.waiting.len()).to_le_bytes());
        for b in &self.waiting {
            let bytes = b.to_bytes();
            out.extend_from_slice(&wire::len_u32(bytes.len()).to_le_bytes());
            out.extend_from_slice(&bytes);
        }
        out
    }
}

/// The owner phone of a shop with its own wallet. See the module docs.
pub struct OwnShop {
    identity: Identity,
    wallet: PinnedWallet,
    state: OwnState,
}

impl fmt::Debug for OwnShop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnShop")
            .field("owner", &self.identity.device_id())
            .finish_non_exhaustive()
    }
}

impl OwnShop {
    /// A shop with nothing in it yet, for the owner root `secret` and the
    /// wallet that read back from its file (OA-105).
    pub fn new(identity: Identity, wallet: PinnedWallet) -> Self {
        Self {
            identity,
            wallet,
            state: OwnState::default(),
        }
    }

    /// Bring a stored state back. It must be for this owner and this wallet.
    pub fn restore(
        identity: Identity,
        wallet: PinnedWallet,
        bytes: &[u8],
    ) -> Result<Self, OwnRestoreError> {
        let mut r = Reader::framed(bytes, &MAGIC_OWN_STATE).map_err(corrupt)?;
        let owner = r.device_id().map_err(corrupt)?;
        let wallet_hash = r.array::<32>().map_err(corrupt)?;
        if owner != identity.device_id() || wallet_hash != wallet.checksum32() {
            return Err(OwnRestoreError::NotThisShop);
        }
        let known = if r.flag().map_err(corrupt)? {
            let n = usize::try_from(r.u32_le().map_err(corrupt)?)
                .map_err(|_| OwnRestoreError::Corrupt("settings too long"))?;
            if n > MAX_SETTINGS_BYTES {
                return Err(OwnRestoreError::Corrupt("the settings are too long"));
            }
            Some(SignedSettings::from_bytes(r.take(n).map_err(corrupt)?).map_err(corrupt)?)
        } else {
            None
        };
        let n = usize::try_from(r.u32_le().map_err(corrupt)?)
            .map_err(|_| OwnRestoreError::Corrupt("book too long"))?;
        let book = AllocationBook::from_bytes(r.take(n).map_err(corrupt)?)
            .map_err(|e| OwnRestoreError::Corrupt(book_why(e)))?;
        let count = r.count(MAX_WAITING, 4).map_err(corrupt)?;
        let mut waiting: Vec<OwnBundle> = Vec::with_capacity(count);
        for _ in 0..count {
            let n = usize::try_from(r.u32_le().map_err(corrupt)?)
                .map_err(|_| OwnRestoreError::Corrupt("bundle too long"))?;
            if n > MAX_BUNDLE_BYTES {
                return Err(OwnRestoreError::Corrupt("a bundle is too long"));
            }
            let bundle = OwnBundle::from_bytes(r.take(n).map_err(corrupt)?).map_err(corrupt)?;
            if waiting.last().is_some_and(|l| {
                l.pairing.entry.counter.to_bytes() >= bundle.pairing.entry.counter.to_bytes()
            }) {
                return Err(OwnRestoreError::Corrupt("the bundles are not in order"));
            }
            waiting.push(bundle);
        }
        r.finish().map_err(corrupt)?;
        let state = OwnState {
            known,
            book,
            waiting,
        };
        if state.encode(&owner, &wallet_hash) != bytes {
            return Err(OwnRestoreError::Corrupt(
                "it does not encode to the same bytes",
            ));
        }
        Ok(Self {
            identity,
            wallet,
            state,
        })
    }

    /// The bytes of the shop's state.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.state
            .encode(&self.identity.device_id(), &self.wallet.checksum32())
    }

    /// The owner root.
    pub fn owner_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// The shop's wallet.
    pub fn wallet(&self) -> &PinnedWallet {
        &self.wallet
    }

    /// The settings in force: the latest this phone signed.
    pub fn known_settings(&self) -> Option<&SignedSettings> {
        self.state.known.as_ref()
    }

    /// OA-106: the allocation book, to read.
    pub fn book(&self) -> &AllocationBook {
        &self.state.book
    }

    /// Store `next`, and take it into memory only if the store worked.
    fn commit<E>(
        &mut self,
        next: OwnState,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnError<E>> {
        let bytes = next.encode(&self.identity.device_id(), &self.wallet.checksum32());
        persist(&bytes).map_err(OwnError::Store)?;
        self.state = next;
        Ok(())
    }

    /// OA-108: the offer for a new counter: the owner root, the version of
    /// the settings in force (0 when there are none) and the wallet.
    pub fn make_offer(&self, now: u64) -> Result<MadeOffer, OwnRefusal> {
        self.make_offer_as(now, false)
    }

    /// OA-108, CR-256: the offer, marked as a starter wallet's when the
    /// pinned wallet is the starter wallet, so that the counter that takes it
    /// is one that may switch (SS-41).
    pub fn make_offer_as(&self, now: u64, starter: bool) -> Result<MadeOffer, OwnRefusal> {
        let floor = self.state.known.as_ref().map_or(0, |k| k.settings.version);
        let offer = OwnOffer::new(
            bitcoin::Network::Bitcoin,
            self.identity.device_id(),
            floor,
            self.wallet.clone(),
        )
        .map_err(OwnRefusal::Pairing)?
        .with_starter(starter);
        Ok(MadeOffer {
            offer,
            made_at: now,
        })
    }

    /// The offer is this shop's, and its time is not over.
    fn check_offer(&self, made: &MadeOffer, now: u64) -> Result<(), OwnRefusal> {
        let offer = &made.offer;
        if offer.owner != self.identity.device_id() || offer.wallet != self.wallet {
            return Err(OwnRefusal::NotThisShopsOffer);
        }
        if now < made.made_at {
            return Err(OwnRefusal::ClockWentBack);
        }
        if now - made.made_at > OFFER_WINDOW_SECS {
            return Err(OwnRefusal::OfferEnded);
        }
        Ok(())
    }

    fn read_checked(
        &self,
        made: &MadeOffer,
        now: u64,
        request: &[u8],
    ) -> Result<(OwnRequest, ReadRequest), OwnRefusal> {
        self.check_offer(made, now)?;
        if request.len() > MAX_REQUEST_BYTES {
            return Err(OwnRefusal::NotARequest);
        }
        let request = OwnRequest::from_bytes(request).map_err(|e| match e {
            // The name rule is the decoder's: a name that breaks it is its
            // own refusal.
            DecodeError::NonCanonical(why) if why.starts_with("name") => OwnRefusal::BadName,
            _ => OwnRefusal::NotARequest,
        })?;
        let pending = check_own_request(&made.offer, &request).map_err(|e| match e {
            OwnPairError::BindingMismatch => OwnRefusal::OtherOffer,
            OwnPairError::InvalidName => OwnRefusal::BadName,
            OwnPairError::CounterIsOwner => OwnRefusal::OwnKey,
            OwnPairError::RequestSignatureInvalid => OwnRefusal::BadSignature,
            other => OwnRefusal::Pairing(other),
        })?;
        let read = ReadRequest {
            counter: pending.counter(),
            name: pending.name().to_string(),
            // A deliberately slow stretch (CR-229).
            words: pending.session_words(),
        };
        Ok((request, read))
    }

    /// OA-109: read the counter's request against the offer: at most 1,024
    /// bytes, for this offer, with a good signature, from a key that is not
    /// the owner's, with a plain name, within the offer's 10 minutes. It
    /// gives the counter's name and its four words. Nothing is stored.
    pub fn read_request(
        &self,
        made: &MadeOffer,
        now: u64,
        request: &[u8],
    ) -> Result<ReadRequest, OwnRefusal> {
        self.read_checked(made, now, request).map(|(_, read)| read)
    }

    /// AS-70: the settings for the counter: the settings in force (or the
    /// starting values at `usd_per_btc` dollars to a bitcoin when there are
    /// none), with the counter on the device list and in the counters at
    /// equal shares, at a version above every one signed so far.
    fn settings_with_counter(
        &self,
        counter: DeviceId,
        usd_per_btc: u64,
    ) -> Result<Settings, Refusal> {
        let mut settings = match &self.state.known {
            Some(s) => s.settings.clone(),
            None if usd_per_btc == 0 => return Err(Refusal::NoSettingsRate),
            None => Settings::defaults(sats_per_usd_at(usd_per_btc)?),
        };
        settings.version = self
            .state
            .known
            .as_ref()
            .map_or(0, |k| k.settings.version)
            .checked_add(1)
            .ok_or_else(|| Refusal::SettingsInvalid("no version is left".to_string()))?;
        settings.devices.insert(counter);
        let mut counters: BTreeSet<DeviceId> = settings.counters.keys().copied().collect();
        counters.insert(counter);
        settings.counters = equal_shares(&counters);
        settings
            .validate()
            .map_err(|e| Refusal::SettingsInvalid(e.to_string()))?;
        Ok(settings)
    }

    /// OA-109 (the shares the owner is shown before "They match"): each
    /// counter's share once `counter` is added.
    pub fn shares_with(&self, counter: DeviceId) -> Vec<(DeviceId, u16)> {
        let mut counters: BTreeSet<DeviceId> = self
            .state
            .known
            .iter()
            .flat_map(|k| k.settings.counters.keys().copied())
            .collect();
        counters.insert(counter);
        equal_shares(&counters).into_iter().collect()
    }

    /// Whether the shop holds settings to build on: when it does not, the
    /// first pairing needs a price (OA-110, OA-23).
    pub fn has_settings_base(&self) -> bool {
        self.state.known.is_some()
    }

    /// OA-110: "They match". In this order: the request is read again; the
    /// first settings are made (the owner root's signature only, a version
    /// above the offer's floor, the counter on the device list and among
    /// the counters at equal shares); the first range is written to the book
    /// and stored (OA-106); the range is signed; the pairing entry is signed
    /// (CR-230); and the finished bundle is stored to wait for the counter.
    /// `relay` is the owner's signed relay for the bundle, if the shop has
    /// one (CR-130). Nothing is allocated when a check fails.
    #[allow(clippy::too_many_arguments)]
    pub fn confirm<E>(
        &mut self,
        made: &MadeOffer,
        now: u64,
        request: &[u8],
        usd_per_btc: u64,
        peers: Vec<String>,
        relay: Option<SignedRelayHint>,
        persist: impl Fn(&[u8]) -> Result<(), E>,
    ) -> Result<Paired, OwnError<E>> {
        let (request, read) = self.read_checked(made, now, request)?;
        if self.state.book.has_device(&read.counter)
            || self
                .state
                .waiting
                .iter()
                .any(|b| b.pairing.entry.counter == read.counter)
        {
            return Err(OwnRefusal::AlreadyAdded.into());
        }
        if self.state.waiting.len() >= MAX_WAITING {
            return Err(OwnRefusal::TooManyWaiting.into());
        }
        let settings = self
            .settings_with_counter(read.counter, usd_per_btc)
            .map_err(OwnRefusal::Settings)?;
        let version = settings.version;
        let shares: Vec<(DeviceId, u16)> =
            settings.counters.iter().map(|(c, s)| (*c, *s)).collect();
        let signed_settings = SignedSettings {
            owner_sig: settings.sign(&self.identity),
            approver_sig: None,
            settings: settings.clone(),
        };
        // The range is written to the book before it is signed (OA-106).
        let mut next = self.state.clone();
        let range = next.book.allocate(read.counter, now)?;
        self.commit(next, &persist)?;
        let first_range = SignedRange::sign(&self.identity, range)
            .map_err(|e| OwnRefusal::Pairing(OwnPairError::Range(e)))?;
        debug_assert!(range.end - range.start >= FIRST_RANGE_MIN_COUNT);
        let bundle = OwnBundle::build(
            &self.identity,
            &made.offer,
            &request,
            signed_settings.clone(),
            first_range,
            peers,
            relay,
        )
        .map_err(OwnRefusal::Pairing)?;
        let mut next = self.state.clone();
        next.known = Some(signed_settings);
        next.waiting.push(bundle.clone());
        next.waiting
            .sort_by_key(|b| b.pairing.entry.counter.to_bytes());
        self.commit(next, &persist)?;
        Ok(Paired {
            bundle,
            first_range: range,
            settings_version: version,
            shares,
        })
    }

    /// The counters whose bundle waits to be handed over (OA-111).
    pub fn counters_waiting(&self) -> Vec<WaitingOwnCounter> {
        self.state
            .waiting
            .iter()
            .map(|b| WaitingOwnCounter {
                counter: b.pairing.entry.counter,
                name: b.pairing.entry.name.clone(),
            })
            .collect()
    }

    /// OA-111: the bundle of a counter again, exactly as it was made.
    /// Nothing is allocated and nothing is changed.
    pub fn bundle_again(&self, counter: &DeviceId) -> Result<OwnBundle, OwnRefusal> {
        self.state
            .waiting
            .iter()
            .find(|b| b.pairing.entry.counter == *counter)
            .cloned()
            .ok_or(OwnRefusal::NoBundle)
    }

    /// OA-111: the owner says the counter shows Paired: its bundle no
    /// longer waits.
    pub fn mark_delivered<E>(
        &mut self,
        counter: &DeviceId,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnError<E>> {
        if !self
            .state
            .waiting
            .iter()
            .any(|b| b.pairing.entry.counter == *counter)
        {
            return Err(OwnRefusal::NoBundle.into());
        }
        let mut next = self.state.clone();
        next.waiting.retain(|b| b.pairing.entry.counter != *counter);
        self.commit(next, persist)
    }

    /// CR-236: the owner changes the settings. They are signed by the owner
    /// root alone and stored as the settings in force, and the link carries
    /// them to the counters. A change that loosens is carried as it is
    /// signed: a counter holds it for confirmation (CR-239). The version
    /// must be above the one in force.
    pub fn change_settings<E>(
        &mut self,
        settings: Settings,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<SignedSettings, OwnError<E>> {
        settings
            .validate()
            .map_err(|e| OwnRefusal::Settings(Refusal::SettingsInvalid(e.to_string())))?;
        if let Some(known) = &self.state.known {
            if settings.version <= known.settings.version {
                return Err(OwnRefusal::Settings(Refusal::SettingsVersion {
                    known: known.settings.version,
                    got: settings.version,
                })
                .into());
            }
        }
        let signed = SignedSettings {
            owner_sig: settings.sign(&self.identity),
            approver_sig: None,
            settings,
        };
        let mut next = self.state.clone();
        next.known = Some(signed.clone());
        self.commit(next, persist)?;
        Ok(signed)
    }

    /// OA-106: the next range for `device` (a counter, or the owner root
    /// for the desk, RQ-02), written to the book and stored before it is
    /// signed, and signed before it is returned.
    pub fn allocate<E>(
        &mut self,
        device: DeviceId,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<SignedRange, OwnError<E>> {
        let mut next = self.state.clone();
        let range = next.book.allocate(device, now)?;
        self.commit(next, persist)?;
        SignedRange::sign(&self.identity, range)
            .map_err(|e| OwnRefusal::Pairing(OwnPairError::Range(e)).into())
    }

    /// OA-106: the range of the desk, the owner root's own.
    pub fn allocate_desk<E>(
        &mut self,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<SignedRange, OwnError<E>> {
        self.allocate(self.identity.device_id(), now, persist)
    }

    /// OA-112: the range after the ones a counter holds, for a top-up.
    /// A range for a device the book does not know is refused: only a
    /// counter that was paired gets one.
    pub fn allocate_top_up<E>(
        &mut self,
        counter: DeviceId,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<SignedRange, OwnError<E>> {
        if !self.state.book.has_device(&counter) {
            return Err(OwnRefusal::UnknownCounter.into());
        }
        self.allocate(counter, now, persist)
    }

    /// SS-41, CR-257: the switch message for a counter paired with this
    /// shop's wallet (the starter wallet): the new wallet and a first range
    /// for it, taken from the book after every range given so far, so that no
    /// index of it was ever a sale number of the old wallet. The range is
    /// written to the book and stored before it is signed (OA-106). The new
    /// wallet must not be the wallet in use, and the counter must be one the
    /// book knows.
    pub fn make_switch<E>(
        &mut self,
        counter: DeviceId,
        new_wallet: &PinnedWallet,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<SwitchOwn, OwnError<E>> {
        if new_wallet.checksum32() == self.wallet.checksum32() {
            return Err(OwnRefusal::SwitchSameWallet.into());
        }
        if counter == self.identity.device_id() || !self.state.book.has_device(&counter) {
            return Err(OwnRefusal::UnknownCounter.into());
        }
        let range = self.allocate(counter, now, persist)?;
        Ok(SwitchOwn::sign(
            &self.identity,
            counter,
            self.wallet.checksum32(),
            new_wallet.clone(),
            range,
        ))
    }

    /// The first range of the bundle that waits for a counter, or of the
    /// book's first range for it, as the counter holds it from its bundle.
    pub fn first_range_of(&self, counter: &DeviceId) -> Option<IndexRange> {
        self.state.book.ranges_of(counter).into_iter().next()
    }
}

fn book_why(e: BookError) -> &'static str {
    match e {
        BookError::Corrupt(why) => why,
        BookError::Full => "the book is full",
        BookError::TooManyDevices => "the book holds too many devices",
    }
}
