//! The ops of the owner phone of a shop with its own wallet (ops 147 to
//! 157, `docs/OWNER-APP-RULES.md` OA-105 to OA-113): keep the wallet's
//! description, make the offer, read the counter's request, make the bundle,
//! show it again, mark it handed over, and work out the gap limit.
//!
//! The ops work on no node: a shop with its own wallet has no Key app and no
//! running node yet. Each call is given the app's data folder, the owner root
//! secret and the wallet's file (the app keeps those two sealed, as it keeps
//! its other secrets), opens the shop's state under the folder, does one
//! thing, stores what changed before it returns, and wipes the secret. The
//! offer and the counter's request live in the app's memory for the ten
//! minutes of the ceremony and are handed back with each call, so that
//! nothing of an unfinished pairing is stored (OA-110). One call at a time
//! works on the folder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use openbmt_core::boxpair::Welcome;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pinned_wallet::{PinnedWallet, WalletKind};
use openbmt_core::relay::{RelayHint, RelayRoute, SignedRelayHint};
use openbmt_core::wallet_single::ImportedWallet;
use openbmt_owner::own_book::{gap_limit, gap_needs_notice};
use openbmt_owner::own_say::{shop_file_failed, they_differ, wallet_not_readable};
use openbmt_owner::own_shop::{offer_open_at, MadeOffer, OwnError, OwnRefusal};
use openbmt_owner::own_switch::{status_words, SwitchStatus};
use openbmt_owner_node::{OwnNode, OwnNodeError};
use std::fmt;

use crate::boxnet::{self, BoxLinkArgs};
use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::counter_views::WaitingCounterView;
use crate::error::{from_words, internal, AppError, Kind};
use crate::views::{CounterShareView, WordsView};

/// One call at a time works on the shop's folder.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// SS-41: the offer each shop's app shows, by data folder, with the time it
/// was made. The offer ops keep no node, so this is what tells the re-pin
/// that a pairing is open: an offer is open until its time is over, "They
/// differ" is said, or its counter is confirmed (the bundle then waits in
/// the shop's record).
static OPEN_OFFERS: Mutex<BTreeMap<PathBuf, u64>> = Mutex::new(BTreeMap::new());

fn open_offers() -> std::sync::MutexGuard<'static, BTreeMap<PathBuf, u64>> {
    OPEN_OFFERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// SS-41: whether an offer of the shop at `dir` is shown at `now`.
pub(crate) fn offer_open(dir: &Path, now: u64) -> bool {
    open_offers()
        .get(dir)
        .is_some_and(|made_at| offer_open_at(*made_at, now))
}

/// What every op that opens the shop is given.
#[derive(Clone, PartialEq, Eq)]
pub struct OwnShopArgs {
    /// The folder of the shop's state (a folder of the app's own).
    pub dir: String,
    /// The owner root's secret, 32 bytes.
    pub owner_secret: Vec<u8>,
    /// The wallet's file as the app read it back (`OBM)`, OA-105).
    pub wallet: Vec<u8>,
}

impl fmt::Debug for OwnShopArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnShopArgs")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

wire_record!(OwnShopArgs {
    dir,
    owner_secret,
    wallet
});
wire_record_get!(OwnShopArgs {
    dir,
    owner_secret,
    wallet
});

/// The owner root the phone made (op 147).
#[derive(Clone, PartialEq, Eq)]
pub struct OwnOwnerMade {
    /// The secret, 32 bytes, which the app keeps sealed.
    pub secret: Vec<u8>,
    /// The root's public key, 32 bytes.
    pub root: Vec<u8>,
}

impl fmt::Debug for OwnOwnerMade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnOwnerMade")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

wire_record!(OwnOwnerMade { secret, root });

/// The wallet that read (ops 148 and 149).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnWalletView {
    /// The wallet's file (`OBM)`), which the app keeps sealed.
    pub file: Vec<u8>,
    /// The first receive address.
    pub first_address: String,
    /// Whether the wallet is the 2-of-3 wallet.
    pub multisig: bool,
}

wire_record!(OwnWalletView {
    file,
    first_address,
    multisig
});

/// The offer a call is given back (OA-108): its code and the time it was
/// made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnMade {
    /// The offer (`OBM-`).
    pub code: Vec<u8>,
    /// When it was made, in seconds.
    pub made_at: u64,
}

wire_record!(OwnMade { code, made_at });
wire_record_get!(OwnMade { code, made_at });

/// What `own_offer` made (OA-108).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnOfferView {
    /// The offer, to draw as a code, and to hand back with each later call.
    pub made: OwnMade,
    /// When the offer ends, in seconds.
    pub ends_at: u64,
    /// The settings version in force that the offer names.
    pub floor: u32,
    /// The wallet's first receive address, which the counter shows.
    pub first_address: String,
}

wire_record!(OwnOfferView {
    made,
    ends_at,
    floor,
    first_address
});

/// The arguments of `own_request` (OA-109).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnRequestArgs {
    /// The shop.
    pub shop: OwnShopArgs,
    /// The offer being answered.
    pub made: OwnMade,
    /// The time now.
    pub now: u64,
    /// The bytes of the counter's code.
    pub request: Vec<u8>,
}

wire_record!(OwnRequestArgs {
    shop,
    made,
    now,
    request
});
wire_record_get!(OwnRequestArgs {
    shop,
    made,
    now,
    request
});

/// What `own_request` read (OA-109).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnRequestView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The name the counter gave itself. It is text from the counter.
    pub name: String,
    /// The four words the counter shows too.
    pub session_words: Vec<String>,
    /// The phone holds no settings yet: "They match" needs today's price.
    pub needs_rate: bool,
    /// Each counter's share of the instant limit once this one is added.
    pub shares: Vec<CounterShareView>,
    /// Seconds the offer has left.
    pub seconds_left: u64,
}

wire_record!(OwnRequestView {
    counter,
    name,
    session_words,
    needs_rate,
    shares,
    seconds_left
});

/// The arguments of `own_confirm` (OA-110).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnConfirmArgs {
    /// The shop.
    pub shop: OwnShopArgs,
    /// The offer being answered.
    pub made: OwnMade,
    /// The time now.
    pub now: u64,
    /// The bytes of the counter's code, as read.
    pub request: Vec<u8>,
    /// Today's price of one bitcoin in whole dollars, for the first
    /// settings; ignored when the phone holds settings.
    pub usd_per_btc: u64,
    /// The owner's signed relay for the counter, if the shop has one.
    pub relay: Option<Vec<u8>>,
}

wire_record!(OwnConfirmArgs {
    shop,
    made,
    now,
    request,
    usd_per_btc,
    relay
});
wire_record_get!(OwnConfirmArgs {
    shop,
    made,
    now,
    request,
    usd_per_btc,
    relay
});

/// The arguments of `own_box_welcome` (BX-18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnBoxWelcomeArgs {
    /// The shop.
    pub shop: OwnShopArgs,
    /// Where the Box is, and the proxy for an onion name.
    pub link: BoxLinkArgs,
    /// The invitation's text, as `box_own_invitation` gave it.
    pub invitation: String,
    /// The claim (`OBM5`) the Box offered, as `box_own_check` read it.
    pub claim: Vec<u8>,
    /// Today's price of one bitcoin in whole dollars, for the first
    /// settings; ignored when the phone holds settings.
    pub usd_per_btc: u64,
}

wire_record!(OwnBoxWelcomeArgs {
    shop,
    link,
    invitation,
    claim,
    usd_per_btc
});
wire_record_get!(OwnBoxWelcomeArgs {
    shop,
    link,
    invitation,
    claim,
    usd_per_btc
});

/// A Box of a shop with its own wallet that took its welcome (BX-18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnBoxDeliveredView {
    /// The Box's key, 32 bytes: the relay key the shop's link is made for.
    pub box_key: Vec<u8>,
    /// The counters the settings name, 32 bytes each, in key order. Each
    /// one that was paired before the Box needs its relay code (CR-132).
    pub counters: Vec<Vec<u8>>,
    /// The version of the settings the Box holds now.
    pub settings_version: u32,
}

wire_record!(OwnBoxDeliveredView {
    box_key,
    counters,
    settings_version
});

/// The arguments of `own_relay_sign` (CR-132, BX-18).
#[derive(Clone, PartialEq, Eq)]
pub struct OwnRelayArgs {
    /// The owner root's secret, 32 bytes.
    pub owner_secret: Vec<u8>,
    /// `host:port` of the Box, or a v3 `.onion` name and its port.
    pub address: String,
    /// The Box's key, 32 bytes: the relay's key.
    pub relay_key: Vec<u8>,
}

impl fmt::Debug for OwnRelayArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnRelayArgs")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

wire_record!(OwnRelayArgs {
    owner_secret,
    address,
    relay_key
});
wire_record_get!(OwnRelayArgs {
    owner_secret,
    address,
    relay_key
});

/// The arguments of the ops that name a counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnCounterArgs {
    /// The shop.
    pub shop: OwnShopArgs,
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
}

wire_record!(OwnCounterArgs { shop, counter });
wire_record_get!(OwnCounterArgs { shop, counter });

/// The bundle a counter takes (ops 152 and 155).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnBundleView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The bundle (`OBM/`), to show as a code.
    pub code: Vec<u8>,
    /// The first index of the first range in it.
    pub first_start: u32,
    /// One past the last index of the first range in it.
    pub first_end: u32,
    /// The version of the settings in it.
    pub settings_version: u32,
    /// Each counter's share of the instant limit in the settings.
    pub shares: Vec<CounterShareView>,
    /// The settings in it wait for a counter's confirmation on the owner
    /// phone instead of being in force: a counter was added while one is in
    /// force and the shop has a Box (OA-122).
    pub waiting: bool,
}

wire_record!(OwnBundleView {
    counter,
    code,
    first_start,
    first_end,
    settings_version,
    shares,
    waiting
});

/// The change that waits for a counter's confirmation, if one does
/// (OA-119 to OA-121). `present` is false when none waits, and the rest are
/// then empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnWaitingView {
    /// Whether a change waits.
    pub present: bool,
    /// The version of the settings that wait.
    pub version: u32,
    /// Their four words (AP-03).
    pub words: Vec<String>,
    /// The names of the fields they loosen.
    pub fields: Vec<String>,
    /// When the owner signed them.
    pub signed_at: u64,
    /// When they are dropped if no counter confirmed them.
    pub ends_at: u64,
}

wire_record!(OwnWaitingView {
    present,
    version,
    words,
    fields,
    signed_at,
    ends_at
});

/// What a counter's records say of the switch to the new wallet (SS-41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchStateKind {
    /// The counter's record says it switched.
    Switched,
    /// The counter has no record for the new wallet yet.
    Waiting,
    /// The counter's record says it did not switch.
    Refused,
}

wire_unit_enum!(SwitchStateKind {
    Switched = 0,
    Waiting = 1,
    Refused = 2
});

/// One counter of the settings in force and where its switch stands (ops
/// 200 to 202, SS-41).
#[derive(Debug, Clone, PartialEq)]
pub struct SwitchRowView {
    /// The counter's key.
    pub counter: Vec<u8>,
    /// The name the app's lists give the counter.
    pub name: String,
    /// Switched, waiting or refused.
    pub state: SwitchStateKind,
    /// The words of that state.
    pub words: WordsView,
}

wire_record!(SwitchRowView {
    counter,
    name,
    state,
    words
});

/// The rows the app is shown, one for each counter in force.
pub(crate) fn switch_rows_view(
    rows: &[(DeviceId, SwitchStatus)],
    label: impl Fn(&DeviceId) -> String,
) -> Vec<SwitchRowView> {
    rows.iter()
        .map(|(counter, status)| SwitchRowView {
            counter: counter.to_bytes().to_vec(),
            name: label(counter),
            state: match status {
                SwitchStatus::Switched => SwitchStateKind::Switched,
                SwitchStatus::Waiting => SwitchStateKind::Waiting,
                SwitchStatus::Refused => SwitchStateKind::Refused,
            },
            words: WordsView(status_words(*status)),
        })
        .collect()
}

/// Op 200: the offer of a shop whose pinned wallet is the starter wallet
/// (CR-256): as op 150, marked so that the counter that takes it may switch.
pub(crate) fn offer_starter(a: OwnShopArgs, now: u64) -> Result<OwnOfferView, AppError> {
    offer_as(a, now, true)
}

/// The arguments of `own_gap` (OA-113).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnGapArgs {
    /// The indexes that received a payment, ascending.
    pub paid: Vec<u32>,
    /// The highest index any counter handed out, if one did.
    pub highest_given: Option<u32>,
}

wire_record!(OwnGapArgs {
    paid,
    highest_given
});
wire_record_get!(OwnGapArgs {
    paid,
    highest_given
});

/// How far ahead the wallet must look (OA-113).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnGapView {
    /// The addresses the wallet must look ahead to show every payment.
    pub needed: u32,
    /// Whether the page says to set the wallet further ahead (above 20).
    pub notice: bool,
}

wire_record!(OwnGapView { needed, notice });

fn refusal(r: &OwnRefusal) -> AppError {
    from_words(Kind::Refused, r.rule(), r, r.words())
}

pub(crate) fn node_error(e: OwnNodeError) -> AppError {
    match &e {
        OwnNodeError::Io(_) | OwnNodeError::NotCarried(_) => from_words(Kind::Store, "OA-105", &e, shop_file_failed()),
        OwnNodeError::State(_) => from_words(Kind::State, "OA-105", &e, shop_file_failed()),
        OwnNodeError::Switch(r) => refusal(r),
        OwnNodeError::DeskNoRoom => from_words(
            Kind::Refused,
            "RQ-02",
            &e,
            openbmt_core::say::Words::new(openbmt_core::say::Tone::Stop, "Too many addresses watched", "This phone already watches as many addresses as it can, so it cannot take more for requests.", openbmt_core::say::Who::Owner, "Ask for this payment at a counter for now."),
        ),
        OwnNodeError::DeskFull => from_words(
            Kind::Refused,
            "RQ-02",
            &e,
            openbmt_core::say::Words::new(openbmt_core::say::Tone::Stop, "No more address ranges", "The shop has used every range of addresses its wallet allows.", openbmt_core::say::Who::Owner, "Move the shop to a new wallet to make more requests."),
        ),
        OwnNodeError::Derive(_) | OwnNodeError::NoApprover => internal(e),
    }
}

fn own_error(e: OwnError<String>) -> AppError {
    match e {
        OwnError::Refused(r) => refusal(&r),
        OwnError::Store(text) => from_words(Kind::Store, "OA-105", text, shop_file_failed()),
    }
}

/// The wallet in its file, or the words of a file that does not read
/// (OA-105): the bytes must decode and encode to the same bytes.
pub(crate) fn wallet_of(file: &[u8]) -> Result<PinnedWallet, AppError> {
    let wallet = PinnedWallet::from_bytes(file)
        .map_err(|e| from_words(Kind::State, "OA-105", e, wallet_not_readable()))?;
    if wallet.to_bytes() != file {
        return Err(from_words(
            Kind::State,
            "OA-105",
            "the wallet's file does not encode to the same bytes",
            wallet_not_readable(),
        ));
    }
    Ok(wallet)
}

fn device(name: &str, bytes: &[u8]) -> Result<DeviceId, AppError> {
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| crate::error::refused_by("id-length", format!("{name} must be 32 bytes")))?;
    DeviceId::from_bytes(&array)
        .map_err(|_| crate::error::refused_by("id-length", format!("{name} is not a key")))
}

fn shares_view(shares: Vec<(DeviceId, u16)>) -> Vec<CounterShareView> {
    shares
        .into_iter()
        .map(|(c, bps)| CounterShareView {
            counter: c.to_bytes().to_vec(),
            bps: u32::from(bps),
        })
        .collect()
}

fn made_of(made: &OwnMade) -> Result<MadeOffer, AppError> {
    let offer = openbmt_core::pairing_own::OwnOffer::from_bytes(&made.code)
        .map_err(|_| refusal(&OwnRefusal::NotThisShopsOffer))?;
    Ok(MadeOffer {
        offer,
        made_at: made.made_at,
    })
}

/// The owner node's turn on the shop's state (OA-112): the state is read
/// fresh from its file, under the lock the ops of the shop take, so that a
/// range the node gives and a counter an op pairs cannot take the same
/// indexes. The app's calls on the shop (`with_shop`) wait for it, and it
/// waits for them.
pub(crate) fn turn_on_shop<T>(
    dir: &Path,
    identity: Identity,
    wallet: PinnedWallet,
    f: impl FnOnce(&mut OwnNode) -> Result<T, OwnNodeError>,
) -> Result<T, OwnNodeError> {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut node = OwnNode::open(dir, identity, wallet)?;
    f(&mut node)
}

/// Open the shop for one call, run `f`, and wipe the secret.
fn with_shop<T>(
    a: OwnShopArgs,
    f: impl FnOnce(&mut OwnNode) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let OwnShopArgs {
        dir,
        mut owner_secret,
        wallet,
    } = a;
    let identity = match <[u8; 32]>::try_from(owner_secret.as_slice()) {
        Ok(secret) => {
            let mut secret = secret;
            let made = Identity::from_secret_bytes(&secret);
            secret.fill(0);
            made
        }
        Err(_) => {
            owner_secret.fill(0);
            return Err(crate::error::refused_by(
                "id-length",
                "the owner secret must be 32 bytes",
            ));
        }
    };
    owner_secret.fill(0);
    std::hint::black_box(&mut owner_secret);
    let identity = identity
        .map_err(|e| crate::error::refused_by("id-length", format!("the owner key: {e}")))?;
    let wallet = wallet_of(&wallet)?;
    let mut node = OwnNode::open(Path::new(&dir), identity, wallet).map_err(node_error)?;
    f(&mut node)
}

/// 32 bytes read straight from the system's random source, with no
/// fallback.
fn random32() -> Result<[u8; 32], AppError> {
    use std::io::Read;
    let mut out = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut out))
        .map_err(|e| internal(format!("no random bytes: {e}")))?;
    Ok(out)
}

/// Op 147: make the owner root of a shop with its own wallet.
pub(crate) fn owner_make() -> Result<OwnOwnerMade, AppError> {
    let mut secret = random32()?;
    let identity = Identity::from_secret_bytes(&secret)
        .map_err(|e| internal(format!("the owner key: {e}")))?;
    let made = OwnOwnerMade {
        secret: secret.to_vec(),
        root: identity.device_id().to_bytes().to_vec(),
    };
    secret.fill(0);
    std::hint::black_box(&mut secret);
    Ok(made)
}

/// Overwrite a text the call was given.
fn wipe(text: String) {
    let mut bytes = text.into_bytes();
    bytes.fill(0);
    std::hint::black_box(&mut bytes);
}

fn wallet_view(wallet: &PinnedWallet) -> Result<OwnWalletView, AppError> {
    Ok(OwnWalletView {
        file: wallet.to_bytes(),
        first_address: wallet
            .receive_address(0)
            .map_err(|e| internal(format!("the first address: {e}")))?
            .to_string(),
        multisig: wallet.kind() == WalletKind::TwoOfThree,
    })
}

/// Op 148: the file that keeps the wallet, from the text the owner
/// scanned and checked (the import's own refusals apply, WL-08 to WL-12).
pub(crate) fn wallet_pin(text: String) -> Result<OwnWalletView, AppError> {
    let result = ImportedWallet::read(&text)
        .map_err(crate::import_view::refused)
        .and_then(|_| {
            PinnedWallet::from_text(&text)
                .map_err(|e| internal(format!("the wallet does not pin: {e}")))
        })
        .and_then(|w| wallet_view(&w));
    wipe(text);
    result
}

/// Op 149: read the file back (OA-105). It must decode and encode to the
/// same bytes, or the words say that it is not readable.
pub(crate) fn wallet_read(file: Vec<u8>) -> Result<OwnWalletView, AppError> {
    wallet_view(&wallet_of(&file)?)
}

/// Op 150: the offer (OA-108).
pub(crate) fn offer(a: OwnShopArgs, now: u64) -> Result<OwnOfferView, AppError> {
    offer_as(a, now, false)
}

fn offer_as(a: OwnShopArgs, now: u64, starter: bool) -> Result<OwnOfferView, AppError> {
    let dir = PathBuf::from(&a.dir);
    with_shop(a, |node| {
        let made = node
            .shop()
            .make_offer_as(now, starter)
            .map_err(|r| refusal(&r))?;
        let first_address = made
            .offer
            .first_address()
            .map_err(|e| internal(format!("the first address: {e}")))?
            .to_string();
        // SS-41: from here a pairing is open on this phone.
        open_offers().insert(dir, made.made_at);
        Ok(OwnOfferView {
            ends_at: made.ends_at(),
            floor: made.offer.floor,
            first_address,
            made: OwnMade {
                code: made.offer.to_bytes(),
                made_at: made.made_at,
            },
        })
    })
}

/// Op 151: read the counter's request (OA-109). Nothing is stored.
pub(crate) fn request(a: OwnRequestArgs) -> Result<OwnRequestView, AppError> {
    let OwnRequestArgs {
        shop,
        made,
        now,
        request,
    } = a;
    let made = made_of(&made)?;
    with_shop(shop, |node| {
        let read = node
            .shop()
            .read_request(&made, now, &request)
            .map_err(|r| refusal(&r))?;
        Ok(OwnRequestView {
            counter: read.counter.to_bytes().to_vec(),
            name: read.name.clone(),
            session_words: read.words.iter().map(|w| (*w).to_string()).collect(),
            needs_rate: !node.shop().has_settings_base(),
            shares: shares_view(node.shop().shares_with(read.counter)),
            seconds_left: made.ends_at().saturating_sub(now),
        })
    })
}

fn bundle_view(
    counter: DeviceId,
    bundle: &openbmt_core::pairing_own::OwnBundle,
    shares: Vec<(DeviceId, u16)>,
    waiting: bool,
) -> OwnBundleView {
    OwnBundleView {
        counter: counter.to_bytes().to_vec(),
        code: bundle.to_bytes(),
        first_start: bundle.first_range.range.start,
        first_end: bundle.first_range.range.end,
        settings_version: bundle.settings.settings.version,
        shares: shares_view(shares),
        waiting,
    }
}

/// Op 152: "They match" (OA-110): the first settings, the first range and
/// the signed pairing, stored, and the bundle they make.
pub(crate) fn confirm(a: OwnConfirmArgs) -> Result<OwnBundleView, AppError> {
    let OwnConfirmArgs {
        shop,
        made,
        now,
        request,
        usd_per_btc,
        relay,
    } = a;
    let made = made_of(&made)?;
    let dir = PathBuf::from(&shop.dir);
    let relay = relay
        .map(|bytes| {
            SignedRelayHint::from_bytes(&bytes).map_err(|e| {
                crate::error::refused_by(
                    "request-malformed",
                    format!("the relay is not readable: {e}"),
                )
            })
        })
        .transpose()?;
    with_shop(shop, |node| {
        let writer = node.waiting_writer();
        let (shop, store) = node.shop_and_store();
        let paired = shop
            .confirm_with(
                &made,
                now,
                &request,
                usd_per_btc,
                Vec::new(),
                relay,
                &store,
                &writer,
            )
            .map_err(own_error)?;
        let counter = paired.bundle.pairing.entry.counter;
        // SS-41: the bundle waits in the shop's record from here.
        open_offers().remove(&dir);
        Ok(bundle_view(
            counter,
            &paired.bundle,
            paired.shares,
            paired.waiting,
        ))
    })
}

/// Op 153: "They differ" (OA-110): nothing was queued and nothing is
/// allocated, so there is nothing to forget but the app's own copy.
pub(crate) fn differ() -> Result<WordsView, AppError> {
    // SS-41: the offer is given up; the op names no shop, and one phone
    // shows one offer.
    open_offers().clear();
    Ok(WordsView(they_differ()))
}

/// Op 154: the counters whose bundle waits (OA-111).
pub(crate) fn waiting(a: OwnShopArgs) -> Result<Vec<WaitingCounterView>, AppError> {
    with_shop(a, |node| {
        Ok(node
            .shop()
            .counters_waiting()
            .into_iter()
            .map(|w| WaitingCounterView {
                counter: w.counter.to_bytes().to_vec(),
                name: w.name,
            })
            .collect())
    })
}

/// Op 155: the bundle of a waiting counter again, exactly as it was made
/// (OA-111); nothing is allocated.
pub(crate) fn bundle_again(a: OwnCounterArgs) -> Result<OwnBundleView, AppError> {
    let counter = device("the counter's root", &a.counter)?;
    with_shop(a.shop, |node| {
        let bundle = node
            .shop()
            .bundle_again(&counter)
            .map_err(|r| refusal(&r))?;
        let shares: Vec<(DeviceId, u16)> = bundle
            .settings
            .settings
            .counters
            .iter()
            .map(|(c, s)| (*c, *s))
            .collect();
        let waiting = node
            .shop()
            .waiting_change()
            .is_some_and(|w| w.signed.settings.version >= bundle.settings.settings.version);
        Ok(bundle_view(counter, &bundle, shares, waiting))
    })
}

/// Op 114: the change that waits for a counter's confirmation, with its four
/// words and the fields it loosens (OA-119, OA-121).
pub(crate) fn waiting_change(a: OwnShopArgs) -> Result<OwnWaitingView, AppError> {
    with_shop(a, |node| {
        Ok(match node.waiting_view() {
            Some(w) => OwnWaitingView {
                present: true,
                version: w.version,
                words: w.words.iter().map(|x| (*x).to_string()).collect(),
                fields: w.fields.iter().map(|x| (*x).to_string()).collect(),
                signed_at: w.signed_at,
                ends_at: w.ends_at,
            },
            None => OwnWaitingView {
                present: false,
                version: 0,
                words: Vec::new(),
                fields: Vec::new(),
                signed_at: 0,
                ends_at: 0,
            },
        })
    })
}

/// Op 156: the counter shows Paired (OA-111).
pub(crate) fn delivered(a: OwnCounterArgs) -> Result<(), AppError> {
    let counter = device("the counter's root", &a.counter)?;
    with_shop(a.shop, |node| {
        let (shop, store) = node.shop_and_store();
        shop.mark_delivered(&counter, &store).map_err(own_error)
    })
}

/// Op 157: how far ahead the wallet must look (OA-113). It works on no
/// shop: the numbers are the caller's.
pub(crate) fn gap(a: OwnGapArgs) -> Result<OwnGapView, AppError> {
    let paid: BTreeSet<u32> = a.paid.into_iter().collect();
    let needed = gap_limit(&paid, a.highest_given);
    Ok(OwnGapView {
        needed,
        notice: gap_needs_notice(needed),
    })
}

/// Op 112: hand a Box of a shop with its own wallet its welcome (BX-18).
/// The claim is checked against the invitation again, the settings that
/// add the Box are signed by the owner root alone and stored (a welcome
/// sent again carries the same ones), and the welcome and the settings are
/// sent to the Box at the address, which must answer with the key the claim
/// names. The shop's file is not held during the call to the Box.
pub(crate) fn box_welcome(a: OwnBoxWelcomeArgs) -> Result<OwnBoxDeliveredView, AppError> {
    let OwnBoxWelcomeArgs {
        shop,
        link,
        invitation,
        claim,
        usd_per_btc,
    } = a;
    let route = boxnet::route_for(&link.address, &link.proxy)?;
    let (invitation, claim) = boxnet::check_own_claim(&invitation, &claim)?;
    let (welcome, settings, version, counters) = with_shop(shop, |node| {
        let owner = node.shop().owner_id();
        let (shop, store) = node.shop_and_store();
        let signed = shop
            .add_box(claim.box_key, usd_per_btc, &store)
            .map_err(own_error)?;
        let counters: Vec<Vec<u8>> = signed
            .settings
            .counters
            .keys()
            .map(|c| c.to_bytes().to_vec())
            .collect();
        Ok((
            Welcome::make_own(&invitation, &claim, owner).to_bytes(),
            signed.to_bytes(),
            signed.settings.version,
            counters,
        ))
    })?;
    boxnet::send_welcome(&route, &claim.box_key, &welcome, &settings)?;
    Ok(OwnBoxDeliveredView {
        box_key: claim.box_key.to_bytes().to_vec(),
        counters,
        settings_version: version,
    })
}

/// Op 113: the owner's signed relay (`OBMz`, CR-132) for the Box at the
/// stored address and key: the bytes of a bundle's relay (CR-130) and of the
/// code a counter paired before the Box scans. The proxy is no part of it.
pub(crate) fn relay_sign(a: OwnRelayArgs) -> Result<Vec<u8>, AppError> {
    let OwnRelayArgs {
        mut owner_secret,
        address,
        relay_key,
    } = a;
    let identity = <[u8; 32]>::try_from(owner_secret.as_slice())
        .map_err(|_| crate::error::refused_by("id-length", "the owner secret must be 32 bytes"))
        .and_then(|s| {
            Identity::from_secret_bytes(&s)
                .map_err(|e| crate::error::refused_by("id-length", format!("the owner key: {e}")))
        });
    owner_secret.fill(0);
    std::hint::black_box(&mut owner_secret);
    let identity = identity?;
    let key = <[u8; 32]>::try_from(relay_key.as_slice())
        .map_err(|_| crate::error::refused_by("id-length", "the relay key must be 32 bytes"))?;
    let route = match boxnet::route_for(&address, "127.0.0.1:9050")? {
        openbmt_link::Route::Direct(addr) => RelayRoute::Direct(addr),
        openbmt_link::Route::Onion { host, port, .. } => RelayRoute::Onion {
            name: host.strip_suffix(".onion").unwrap_or(&host).to_string(),
            port,
        },
        openbmt_link::Route::Proxied { .. } => {
            return Err(crate::error::refused_by(
                "box-address",
                "CR-133: the relay is an address or an onion name",
            ))
        }
    };
    let hint = RelayHint::new(route, key)
        .map_err(|e| crate::error::refused_by("box-address", e.to_string()))?;
    Ok(SignedRelayHint::sign(hint, &identity).to_bytes())
}
