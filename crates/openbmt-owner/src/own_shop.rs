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
//!          | the tail of the switch (SS-41, SS-42), only when it holds
//!            anything: 1 | move plan: flag, then wallet hash (32) and index
//!            u32 LE | re-pin: flag, then the old wallet's hash (32) and len
//!            u32 LE | the new wallet's file | former wallets: count u32 LE,
//!            per wallet: len u32 LE | wallet file | end u32 LE
//! ```
//!
//! Decoding is strict: what decodes encodes to the same bytes.
//!
//! The re-pin is stored first, with the wallet hash of the new wallet, and
//! then the caller writes the new wallet's file (CR-261's order for the
//! counter). A start that finds the shop's file for the new wallet and the
//! wallet's file still the old one, with the re-pin recorded, finishes it:
//! the shop takes the recorded new wallet ([`OwnShop::recovered_repin`]) and
//! the caller writes its file. Any other mismatch is refused.

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
use crate::own_switch::{move_ready, Repin, RepinHold, SwitchStatus};
use crate::own_waiting::{needs_confirmation, WaitingChange};
use openbmt_core::approval::CounterConfirmation;
use openbmt_core::confirmed::ConfirmedSettings;

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
    /// SS-42: the move of the starter wallet is planned for another new
    /// wallet, which is not the pinned wallet yet.
    OtherNewWallet,
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
            Self::OtherNewWallet => write!(
                f,
                "SS-42: the move is planned for another new wallet, which is not pinned yet"
            ),
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

/// SS-41: whether an offer made at `made_at` is still shown at `now`: its
/// time is not over. A clock that is behind the offer's time counts it as
/// shown.
pub fn offer_open_at(made_at: u64, now: u64) -> bool {
    now <= made_at.saturating_add(OFFER_WINDOW_SECS)
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
    /// Whether the settings in the bundle are not in force on the owner
    /// phone but wait for a counter's confirmation (OA-119, OA-122).
    pub waiting: bool,
}

/// A counter whose bundle waits to be handed over (OA-111).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingOwnCounter {
    /// The counter's key.
    pub counter: DeviceId,
    /// The counter's name.
    pub name: String,
}

/// The most former wallets a state keeps.
pub const MAX_FORMER: usize = 8;
/// The largest wallet file a snapshot holds, in bytes.
const MAX_WALLET_BYTES: usize = 16 * 1024;

/// SS-42: the index of the new wallet that the move pays, taken once.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MovePlan {
    /// The hash of the new wallet it is for.
    wallet: [u8; 32],
    /// The index in the new wallet.
    index: u32,
}

/// FL-35: the index of the shop's wallet that turning the float off pays,
/// taken once, as the starter's move takes its own.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FloatOffPlan {
    /// The hash of the wallet it is for.
    wallet: [u8; 32],
    /// The index in that wallet.
    index: u32,
}

/// SS-41: the re-pin that was made, kept so that a start between the state
/// and the wallet's file can finish it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RepinRecord {
    /// The hash of the wallet that was pinned before.
    old: [u8; 32],
    /// The wallet pinned by it.
    new: PinnedWallet,
}

/// SS-43: a wallet the shop had pinned before the re-pin. The owner node
/// keeps watching its receive indexes below `end`, so that a payment that
/// arrives later is seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormerWallet {
    /// The wallet.
    pub wallet: PinnedWallet,
    /// One past the highest index of the book when it was re-pinned: no
    /// index at or above it was ever given out for this wallet.
    pub end: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct OwnState {
    known: Option<SignedSettings>,
    book: AllocationBook,
    /// Ascending by counter.
    waiting: Vec<OwnBundle>,
    move_plan: Option<MovePlan>,
    float_off: Option<FloatOffPlan>,
    repin: Option<RepinRecord>,
    /// Oldest first.
    former: Vec<FormerWallet>,
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
        if self.move_plan.is_some()
            || self.repin.is_some()
            || !self.former.is_empty()
            || self.float_off.is_some()
        {
            out.push(1);
            match &self.move_plan {
                Some(p) => {
                    out.push(1);
                    out.extend_from_slice(&p.wallet);
                    out.extend_from_slice(&p.index.to_le_bytes());
                }
                None => out.push(0),
            }
            match &self.repin {
                Some(r) => {
                    out.push(1);
                    out.extend_from_slice(&r.old);
                    put_wallet(&mut out, &r.new);
                }
                None => out.push(0),
            }
            out.extend_from_slice(&wire::len_u32(self.former.len()).to_le_bytes());
            for f in &self.former {
                put_wallet(&mut out, &f.wallet);
                out.extend_from_slice(&f.end.to_le_bytes());
            }
            if let Some(p) = &self.float_off {
                out.push(1);
                out.extend_from_slice(&p.wallet);
                out.extend_from_slice(&p.index.to_le_bytes());
            }
        }
        out
    }
}

fn put_wallet(out: &mut Vec<u8>, wallet: &PinnedWallet) {
    let bytes = wallet.to_bytes();
    out.extend_from_slice(&wire::len_u32(bytes.len()).to_le_bytes());
    out.extend_from_slice(&bytes);
}

fn take_wallet(r: &mut Reader<'_>) -> Result<PinnedWallet, OwnRestoreError> {
    let n = usize::try_from(r.u32_le().map_err(corrupt)?)
        .map_err(|_| OwnRestoreError::Corrupt("a wallet is too long"))?;
    if n > MAX_WALLET_BYTES {
        return Err(OwnRestoreError::Corrupt("a wallet is too long"));
    }
    PinnedWallet::from_bytes(r.take(n).map_err(corrupt)?)
        .map_err(|_| OwnRestoreError::Corrupt("a wallet does not read back"))
}

/// The owner phone of a shop with its own wallet. See the module docs.
pub struct OwnShop {
    identity: Identity,
    wallet: PinnedWallet,
    state: OwnState,
    recovered: bool,
    /// The one change that waits for a counter's confirmation (OA-121). It
    /// is kept in a file of its own, not in the state.
    held: Option<WaitingChange>,
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
            recovered: false,
            held: None,
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
        if owner != identity.device_id() {
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
        let (mut move_plan, mut repin, mut former, mut float_off) = (None, None, Vec::new(), None);
        if !r.is_empty() {
            if r.u8().map_err(corrupt)? != 1 {
                return Err(OwnRestoreError::Corrupt("the tail is of another kind"));
            }
            if r.flag().map_err(corrupt)? {
                move_plan = Some(MovePlan {
                    wallet: r.array::<32>().map_err(corrupt)?,
                    index: r.u32_le().map_err(corrupt)?,
                });
            }
            if r.flag().map_err(corrupt)? {
                repin = Some(RepinRecord {
                    old: r.array::<32>().map_err(corrupt)?,
                    new: take_wallet(&mut r)?,
                });
            }
            let count = r.count(MAX_FORMER * 8, 8).map_err(corrupt)?;
            for _ in 0..count {
                let wallet = take_wallet(&mut r)?;
                let end = r.u32_le().map_err(corrupt)?;
                former.push(FormerWallet { wallet, end });
            }
            // FL-35: a plan of turning the float off, after the former wallets.
            if !r.is_empty() {
                if r.u8().map_err(corrupt)? != 1 {
                    return Err(OwnRestoreError::Corrupt(
                        "the float's plan is of another kind",
                    ));
                }
                float_off = Some(FloatOffPlan {
                    wallet: r.array::<32>().map_err(corrupt)?,
                    index: r.u32_le().map_err(corrupt)?,
                });
            }
        }
        r.finish().map_err(corrupt)?;
        let state = OwnState {
            known,
            book,
            waiting,
            move_plan,
            float_off,
            repin,
            former,
        };
        if state.encode(&owner, &wallet_hash) != bytes {
            return Err(OwnRestoreError::Corrupt(
                "it does not encode to the same bytes",
            ));
        }
        // CR-261's recovery: the state is for the wallet the re-pin made, and
        // the wallet's file is still the one it replaced.
        let (wallet, recovered) = if wallet_hash == wallet.checksum32() {
            (wallet, false)
        } else {
            match &state.repin {
                Some(r) if r.old == wallet.checksum32() && r.new.checksum32() == wallet_hash => {
                    (r.new.clone(), true)
                }
                _ => return Err(OwnRestoreError::NotThisShop),
            }
        };
        Ok(Self {
            identity,
            wallet,
            state,
            recovered,
            held: None,
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

    /// SS-41: whether the state was restored over a wallet file that the
    /// re-pin had replaced: the shop's wallet is the recorded new one and
    /// the caller writes its file (`wallet().to_bytes()`).
    pub fn recovered_repin(&self) -> bool {
        self.recovered
    }

    /// SS-43: the wallets the shop pinned before a re-pin, oldest first.
    pub fn former_wallets(&self) -> &[FormerWallet] {
        &self.state.former
    }

    /// The settings in force: the latest this phone signed.
    pub fn known_settings(&self) -> Option<&SignedSettings> {
        self.state.known.as_ref()
    }

    /// OA-106: the allocation book, to read.
    pub fn book(&self) -> &AllocationBook {
        &self.state.book
    }

    /// SS-42: the receive index of the new wallet that the planned move of
    /// the starter wallet pays, if one was taken. It is the first index of a
    /// range of the owner's own, so the desk (RQ-02) leaves it alone.
    pub fn move_plan_index(&self) -> Option<u32> {
        self.state.move_plan.as_ref().map(|plan| plan.index)
    }

    /// FL-35: the index of the shop's own wallet that turning the float off
    /// pays, if one was taken. It is the first index of a range of the
    /// owner's own, so the desk (RQ-02) leaves it alone.
    pub fn float_off_plan_index(&self) -> Option<u32> {
        let hash = self.wallet.checksum32();
        self.state
            .float_off
            .as_ref()
            .filter(|p| p.wallet == hash)
            .map(|p| p.index)
    }

    /// FL-35: the receive index of the shop's wallet that turning the float
    /// off pays: the first index of a new range of the owner's own, taken
    /// from the book after every range given so far, stored first. It is
    /// taken once for a wallet: asking again gives the same index and takes
    /// nothing, so signing the move again pays the same address.
    pub fn float_off_index<E>(
        &mut self,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<u32, OwnError<E>> {
        if let Some(index) = self.float_off_plan_index() {
            return Ok(index);
        }
        let mut next = self.state.clone();
        let range = next.book.allocate(self.identity.device_id(), now)?;
        next.float_off = Some(FloatOffPlan {
            wallet: self.wallet.checksum32(),
            index: range.start,
        });
        self.commit(next, persist)?;
        Ok(range.start)
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
        // A change that waits is built on: the next counter joins it, not
        // the settings in force (OA-121).
        let base = self
            .held
            .as_ref()
            .map(|h| &h.signed)
            .or(self.state.known.as_ref());
        let mut settings = match base {
            Some(s) => s.settings.clone(),
            None if usd_per_btc == 0 => return Err(Refusal::NoSettingsRate),
            None => Settings::defaults(sats_per_usd_at(usd_per_btc)?),
        };
        settings.version = base
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
            .held
            .as_ref()
            .map(|h| &h.signed)
            .or(self.state.known.as_ref())
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
        self.confirm_with(
            made,
            now,
            request,
            usd_per_btc,
            peers,
            relay,
            persist,
            |_| Ok(()),
        )
    }

    /// [`OwnShop::confirm`] for a caller that keeps the waiting change's
    /// file: when the shop has a Box and the settings that add the counter
    /// are of class Loosen (AP-01, AP-12), they are not put in force. They go
    /// to `persist_waiting` first, as the shop's one waiting change, and
    /// stay there until a counter confirms them (OA-119, OA-122); the new
    /// counter's bundle holds them, so that it takes sales at once.
    #[allow(clippy::too_many_arguments)]
    pub fn confirm_with<E>(
        &mut self,
        made: &MadeOffer,
        now: u64,
        request: &[u8],
        usd_per_btc: u64,
        peers: Vec<String>,
        relay: Option<SignedRelayHint>,
        persist: impl Fn(&[u8]) -> Result<(), E>,
        persist_waiting: impl FnOnce(&[u8]) -> Result<(), E>,
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
        let hold = self.holds(&settings);
        let held = WaitingChange {
            signed: signed_settings.clone(),
            signed_at: now,
        };
        if hold {
            // Written before anything is sent (OA-119).
            persist_waiting(&held.to_bytes()).map_err(OwnError::Store)?;
        }
        let mut next = self.state.clone();
        if !hold {
            next.known = Some(signed_settings);
        }
        next.waiting.push(bundle.clone());
        next.waiting
            .sort_by_key(|b| b.pairing.entry.counter.to_bytes());
        self.commit(next, &persist)?;
        if hold {
            self.held = Some(held);
        }
        Ok(Paired {
            bundle,
            first_range: range,
            settings_version: version,
            shares,
            waiting: hold,
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

    /// OA-119: whether `new` waits for a counter's confirmation instead of
    /// going in force: the shop has a Box in the settings in force, and the
    /// change from them is of class Loosen.
    pub fn holds(&self, new: &Settings) -> bool {
        match &self.state.known {
            Some(k) => !k.settings.boxes.is_empty() && needs_confirmation(&k.settings, new),
            None => false,
        }
    }

    /// The one change that waits for a counter's confirmation, if any
    /// (OA-121).
    pub fn waiting_change(&self) -> Option<&WaitingChange> {
        self.held.as_ref()
    }

    /// Take the waiting change from its file, strictly. A change that is
    /// already the settings in force is dropped, and the answer is `false`:
    /// the caller deletes the file (OA-120). A change of a version that is
    /// not above the one in force is refused as a file that does not read.
    pub fn restore_waiting(&mut self, bytes: &[u8]) -> Result<bool, DecodeError> {
        let change = WaitingChange::from_bytes(bytes)?;
        if change.is_in_force(self.state.known.as_ref()) {
            self.held = None;
            return Ok(false);
        }
        let above = self
            .state
            .known
            .as_ref()
            .is_none_or(|k| change.signed.settings.version > k.settings.version);
        if !above {
            return Err(DecodeError::NonCanonical(
                "the waiting settings are not above the ones in force",
            ));
        }
        self.held = Some(change);
        Ok(true)
    }

    /// Drop the waiting change from memory; the caller deletes its file.
    pub fn drop_waiting(&mut self) {
        self.held = None;
    }

    /// OA-120: the settings that wait, joined to `confirmation` as `OBM<`,
    /// when the confirmation is for them and from a counter of the settings
    /// in force. `None` otherwise.
    pub fn join_confirmation(
        &self,
        confirmation: CounterConfirmation,
    ) -> Option<ConfirmedSettings> {
        let held = self.held.as_ref()?;
        let current = self.state.known.as_ref()?;
        held.join(confirmation, &self.identity.device_id(), &current.settings)
    }

    /// OA-120: put the confirmed settings in force. They are stored before
    /// they take effect; the waiting change leaves memory with them and the
    /// caller deletes its file after.
    pub fn put_in_force<E>(
        &mut self,
        confirmed: &ConfirmedSettings,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnError<E>> {
        let mut next = self.state.clone();
        next.known = Some(confirmed.signed.clone());
        self.commit(next, persist)?;
        self.held = None;
        Ok(())
    }

    /// [`OwnShop::change_settings`] for a caller that keeps the waiting
    /// change's file: settings that wait for a confirmation (see
    /// [`OwnShop::holds`]) are written to `persist_waiting` and replace the
    /// change that waited, and are not put in force; the answer says which.
    pub fn change_settings_with<E>(
        &mut self,
        settings: Settings,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
        persist_waiting: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(SignedSettings, bool), OwnError<E>> {
        if !self.holds(&settings) {
            return self.change_settings(settings, persist).map(|s| (s, false));
        }
        settings
            .validate()
            .map_err(|e| OwnRefusal::Settings(Refusal::SettingsInvalid(e.to_string())))?;
        let above = self
            .held
            .as_ref()
            .map(|h| &h.signed)
            .or(self.state.known.as_ref())
            .map_or(0, |k| k.settings.version);
        if settings.version <= above {
            return Err(OwnRefusal::Settings(Refusal::SettingsVersion {
                known: above,
                got: settings.version,
            })
            .into());
        }
        let signed = SignedSettings {
            owner_sig: settings.sign(&self.identity),
            approver_sig: None,
            settings,
        };
        let held = WaitingChange {
            signed: signed.clone(),
            signed_at: now,
        };
        persist_waiting(&held.to_bytes()).map_err(OwnError::Store)?;
        self.held = Some(held);
        let _ = persist;
        Ok((signed, true))
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

    /// BX-18, AS-84: whether the settings this phone signed list `key` as a
    /// device or a Box, so that a Box already added is known.
    pub fn knows_box(&self, key: &DeviceId) -> bool {
        self.state
            .known
            .as_ref()
            .is_some_and(|k| k.settings.devices.contains(key) || k.settings.boxes.contains(key))
    }

    /// BX-18, AS-84: the settings that add `box_key`: the settings in force
    /// (or the starting values at `usd_per_btc` dollars to a bitcoin when
    /// there are none), with the Box on the device list and among the Boxes,
    /// the counters and their shares as they are, and the version one above
    /// the one in force. The owner root is no Box, and no counter is.
    pub fn settings_with_box(
        &self,
        box_key: DeviceId,
        usd_per_btc: u64,
    ) -> Result<Settings, Refusal> {
        if box_key == self.identity.device_id() {
            return Err(Refusal::BoxIsRoot);
        }
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
        settings.devices.insert(box_key);
        settings.boxes.insert(box_key);
        settings
            .validate()
            .map_err(|e| Refusal::SettingsInvalid(e.to_string()))?;
        Ok(settings)
    }

    /// BX-18: the settings a Box is handed with its welcome: the settings in
    /// force when they already list `box_key` (a welcome that is sent again
    /// carries the same settings, byte for byte, BX-16), and otherwise the
    /// settings that add it ([`OwnShop::settings_with_box`]), signed by the
    /// owner root alone and stored as the settings in force. A Box that is
    /// added is a Free change (AP-01), so counters take it with the owner's
    /// signature.
    pub fn add_box<E>(
        &mut self,
        box_key: DeviceId,
        usd_per_btc: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<SignedSettings, OwnError<E>> {
        if let Some(known) = &self.state.known {
            if known.settings.boxes.contains(&box_key) {
                return Ok(known.clone());
            }
            if known.settings.devices.contains(&box_key) {
                // A counter's key, which is no Box.
                return Err(OwnRefusal::Settings(Refusal::BoxKnown).into());
            }
        }
        let settings = self
            .settings_with_box(box_key, usd_per_btc)
            .map_err(OwnRefusal::Settings)?;
        self.change_settings(settings, persist)
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

    /// SS-41, SS-42: the receive index of `new_wallet` that the move of the
    /// starter wallet pays: the first index of a new range of the owner's
    /// own, taken from the book after every range given so far (the
    /// counters' switch ranges included), so that it was never a sale number
    /// of the old wallet. The range and the plan are written to the state
    /// together, and stored first. It is taken once for a new wallet: asking
    /// again for the same wallet gives the same index and takes nothing, and
    /// another wallet is refused while the planned one is not pinned yet.
    pub fn move_index<E>(
        &mut self,
        new_wallet: &PinnedWallet,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<u32, OwnError<E>> {
        let hash = new_wallet.checksum32();
        if let Some(plan) = &self.state.move_plan {
            if plan.wallet == hash {
                return Ok(plan.index);
            }
            if plan.wallet != self.wallet.checksum32() {
                return Err(OwnRefusal::OtherNewWallet.into());
            }
        }
        let mut next = self.state.clone();
        let range = next.book.allocate(self.identity.device_id(), now)?;
        next.move_plan = Some(MovePlan {
            wallet: hash,
            index: range.start,
        });
        self.commit(next, persist)?;
        Ok(range.start)
    }

    /// SS-41: once every counter of the settings in force has switched to
    /// `new_wallet` (`counters` is each one's status, as for
    /// [`move_ready`]), the shop's pinned wallet becomes `new_wallet`. The
    /// state is stored for it before it takes effect (OA-105), with the
    /// re-pin recorded and the old wallet kept as a former one, watched up
    /// to the book's end (SS-43); the caller then writes the new wallet's
    /// file, and a start between the two finishes it
    /// ([`OwnShop::restore`]). It happens once: with the new wallet already
    /// pinned nothing changes, and with a counter not switched nothing
    /// changes. The book and the settings stay as they are. It waits too,
    /// with nothing changed, while a counter's bundle is not confirmed
    /// ([`OwnShop::counters_waiting`]); [`OwnShop::repin_with`] adds what
    /// else is open.
    pub fn repin<E>(
        &mut self,
        new_wallet: &PinnedWallet,
        counters: &[(DeviceId, SwitchStatus)],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Repin, OwnError<E>> {
        self.repin_with(new_wallet, counters, RepinHold::default(), persist)
    }

    /// SS-41: [`OwnShop::repin`] that also waits while an offer is shown,
    /// while a payment from the shop's wallet is prepared and not finished,
    /// and while the queue of the starter wallet is unsettled (`hold`). A
    /// pairing started after the re-pin is for the new wallet: the offer is
    /// made from the wallet in force ([`OwnShop::make_offer`]), and an offer
    /// made before it is not this shop's any more.
    pub fn repin_with<E>(
        &mut self,
        new_wallet: &PinnedWallet,
        counters: &[(DeviceId, SwitchStatus)],
        hold: RepinHold,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Repin, OwnError<E>> {
        if new_wallet.checksum32() == self.wallet.checksum32() {
            return Ok(Repin::AlreadyDone);
        }
        if let Some(waits) = hold.refusal(!self.state.waiting.is_empty()) {
            return Ok(Repin::Waits(waits));
        }
        if let Err(not_ready) = move_ready(counters) {
            return Ok(Repin::NotReady(not_ready));
        }
        if self.state.former.len() >= MAX_FORMER * 8 {
            return Err(BookError::Full.into());
        }
        let mut next = self.state.clone();
        next.former.push(FormerWallet {
            wallet: self.wallet.clone(),
            end: next.book.next_start(),
        });
        next.repin = Some(RepinRecord {
            old: self.wallet.checksum32(),
            new: new_wallet.clone(),
        });
        let bytes = next.encode(&self.identity.device_id(), &new_wallet.checksum32());
        persist(&bytes).map_err(OwnError::Store)?;
        self.state = next;
        self.wallet = new_wallet.clone();
        Ok(Repin::Done)
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
