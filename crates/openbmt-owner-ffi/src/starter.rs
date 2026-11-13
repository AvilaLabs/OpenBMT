//! The ops of the starter wallet (ops 180 to 196, 204 to 207,
//! `docs/START-SIMPLE-RULES.md`, SS-01 to SS-43): make the wallet, draw its
//! words as images, take the check-back on a keypad, seal the seed, restore
//! it, sign a refund, make the move to the new wallet, and the limit and
//! the date as pure functions.
//!
//! The words never cross as text (SS-11): they are made and kept in the
//! Rust core, in the one setup or restore a process holds, and leave only
//! as the image of four of them. The keypad takes one letter at a time and
//! answers with a count and a yes. The data key that opens the sealed seed
//! (SS-20) comes out once, at the sealing, and goes in again as bytes for
//! each signing; the copy this layer reads is overwritten at once.
//!
//! No op names a node: the setup is held by the registry of the process, so
//! the ops take the zero handle.

use std::fmt;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};

use bitcoin::consensus::encode::serialize;
use bitcoin::{Address, ScriptBuf};
use openbmt_core::wallet_single::ImportedWallet;
use openbmt_owner::starter::{
    self, Coin, DataKey, KernelRandom, Key, Restore, Sealed, Setup, StarterRefusal, StarterWallet,
    Typed, KERNEL_BYTES,
};

use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::error::{from_words, refused_by, AppError, Kind};
use crate::float::{coin_with, FloatCoinView};
use crate::views::{OwnerNetwork, WordsView};

fn refused_with(rule: &str, text: String, words: openbmt_core::say::Words) -> AppError {
    from_words(Kind::Refused, rule, text, words)
}

/// A refusal of the starter wallet as the error that crosses the boundary.
pub(crate) fn refusal(e: &StarterRefusal) -> AppError {
    from_words(Kind::Refused, e.rule(), e, e.words())
}

/// SS-42: the new wallet's description does not read.
pub(crate) fn new_wallet_unreadable(why: impl fmt::Display) -> AppError {
    refused_with(
        "starter-new-wallet",
        format!("the new wallet: {why}"),
        starter::new_wallet_unreadable_words(),
    )
}

/// SS-42: the move waits for the counters. Nothing was built.
pub(crate) fn not_ready(e: &openbmt_owner::own_switch::MoveNotReady) -> AppError {
    refused_with(
        "starter-move-not-ready",
        e.to_string(),
        openbmt_owner::own_switch::not_ready_words(),
    )
}

/// The kernel's bytes, from the `getrandom` system call (SS-01).
struct Kernel;

impl KernelRandom for Kernel {
    fn read32(&mut self) -> Option<[u8; KERNEL_BYTES]> {
        openbmt_kernel_random::read32().ok()
    }
}

/// What the process holds of a starter wallet in the making.
enum Session {
    Idle,
    Setup(Box<Setup>),
    Restore(Box<Restore>),
}

/// The one setup or restore of a process (SS-10, SS-23), and, in a test, the
/// bytes it is given in place of the kernel's.
pub(crate) struct StarterSlot {
    session: Mutex<Session>,
    #[cfg(test)]
    pub(crate) rng: Mutex<Option<Box<dyn KernelRandom + Send>>>,
}

impl StarterSlot {
    pub(crate) fn new() -> Self {
        Self {
            session: Mutex::new(Session::Idle),
            #[cfg(test)]
            rng: Mutex::new(None),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_rng<R>(&self, f: impl FnOnce(&mut dyn KernelRandom) -> R) -> R {
        #[cfg(test)]
        {
            if let Some(rng) = self.rng.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                return f(rng.as_mut());
            }
        }
        f(&mut Kernel)
    }
}

impl Default for StarterSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for StarterSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StarterSlot")
    }
}

fn no_setup() -> AppError {
    from_words(
        Kind::State,
        "starter-no-setup",
        "there is no starter wallet being made: begin one first",
        starter::no_setup_words(),
    )
}

// ---- making -----------------------------------------------------------------

/// The arguments of `starter_make` (SS-01 to SS-03): the die rolls, each 1
/// to 6, that the owner added; none is allowed.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterMakeArgs {
    /// The rolls, one byte each, 1 to 6.
    pub rolls: Vec<u8>,
}

impl fmt::Debug for StarterMakeArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StarterMakeArgs")
            .field("rolls", &self.rolls.len())
            .finish()
    }
}

wire_record!(StarterMakeArgs { rolls });
wire_record_get!(StarterMakeArgs { rolls });

/// The public side of the starter wallet (ops 180 and 196): it holds no
/// secret and is what the app keeps and shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterWalletView {
    /// The description with its checksum.
    pub description: String,
    /// The master fingerprint, 8 hex digits.
    pub fingerprint: String,
    /// The first receive address, mainnet.
    pub first_address: String,
}

wire_record!(StarterWalletView {
    description,
    fingerprint,
    first_address
});

fn wallet_view(w: &StarterWallet) -> Result<StarterWalletView, AppError> {
    let first = w.receive_address(0).map_err(|e| refusal(&e))?;
    Ok(StarterWalletView {
        description: w.description().to_string(),
        fingerprint: w.fingerprint(),
        first_address: first.to_string(),
    })
}

/// Op 180: make the words (SS-01 to SS-04) and hold them in the process.
/// A setup that was begun and not finished is dropped, and its words wiped.
pub(crate) fn make(slot: &StarterSlot, a: StarterMakeArgs) -> Result<StarterWalletView, AppError> {
    let setup = slot
        .with_rng(|rng| Setup::begin(rng, &a.rolls))
        .map_err(|e| refusal(&e))?;
    let view = wallet_view(setup.wallet())?;
    *slot.lock() = Session::Setup(Box::new(setup));
    Ok(view)
}

/// The arguments of `starter_page` (SS-11): which screen of four words, 0
/// to 2, and the scale of the 10 x 20 font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterPageArgs {
    /// The screen, 0 to 2.
    pub page: u32,
    /// The scale, 1 to 16.
    pub scale: u32,
}

wire_record!(StarterPageArgs { page, scale });
wire_record_get!(StarterPageArgs { page, scale });

/// A grey image the Rust core drew (SS-11): one byte for each pixel, row by
/// row, 0 black on 255 white. The app shows it and clears it.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterImageView {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height` bytes.
    pub pixels: Vec<u8>,
}

impl fmt::Debug for StarterImageView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StarterImageView({}x{})", self.width, self.height)
    }
}

wire_record!(StarterImageView {
    width,
    height,
    pixels
});

/// Op 181: draw a screen of four words.
pub(crate) fn page(slot: &StarterSlot, a: StarterPageArgs) -> Result<StarterImageView, AppError> {
    let mut session = slot.lock();
    let Session::Setup(setup) = &mut *session else {
        return Err(no_setup());
    };
    let page = usize::try_from(a.page).map_err(|_| refusal(&StarterRefusal::Page))?;
    let mut image = setup.page(page, a.scale).map_err(|e| refusal(&e))?;
    Ok(StarterImageView {
        width: image.width,
        height: image.height,
        pixels: std::mem::take(&mut image.pixels),
    })
}

/// The positions the check-back asks (op 182), 1 to 12, ascending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterCheckView {
    /// The three positions.
    pub positions: Vec<u32>,
}

wire_record!(StarterCheckView { positions });

/// Op 182: pick the three positions from fresh kernel bytes and begin the
/// check-back (SS-10).
pub(crate) fn check_start(slot: &StarterSlot) -> Result<StarterCheckView, AppError> {
    let mut session = slot.lock();
    let Session::Setup(setup) = &mut *session else {
        return Err(no_setup());
    };
    let asked = slot
        .with_rng(|rng| setup.start_check(rng))
        .map_err(|e| refusal(&e))?;
    Ok(StarterCheckView {
        positions: asked
            .iter()
            .map(|p| u32::try_from(*p).unwrap_or(0))
            .collect(),
    })
}

/// What a key of the keypad did (op 183): counts and yes or no, never a
/// letter (SS-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterTypedView {
    /// Letters typed of the word now being answered.
    pub typed: u32,
    /// Whether they are accepted.
    pub accepted: bool,
    /// Words taken so far.
    pub step: u32,
    /// All the words were taken.
    pub finished: bool,
    /// The check was answered wrong: show the words again.
    pub failed: bool,
}

wire_record!(StarterTypedView {
    typed,
    accepted,
    step,
    finished,
    failed
});

fn typed_view(t: Typed) -> StarterTypedView {
    StarterTypedView {
        typed: u32::try_from(t.typed).unwrap_or(u32::MAX),
        accepted: t.accepted,
        step: u32::try_from(t.step).unwrap_or(u32::MAX),
        finished: t.finished,
        failed: t.failed,
    }
}

/// Op 183: one key of the keypad: a letter, 8 for backspace, 10 or 13 for
/// enter. It goes to the check-back of the setup, or to the words of the
/// restore, whichever is open.
pub(crate) fn key(slot: &StarterSlot, byte: u8) -> Result<StarterTypedView, AppError> {
    let key = Key::from_byte(byte).map_err(|e| refusal(&e))?;
    let mut session = slot.lock();
    let typed = match &mut *session {
        Session::Setup(setup) => setup.key(key),
        Session::Restore(restore) => restore.key(key),
        Session::Idle => return Err(no_setup()),
    };
    typed.map(typed_view).map_err(|e| refusal(&e))
}

/// The sealed seed and the data key that opens it (ops 184 and 186,
/// SS-20): the app stores the sealed seed, wraps the data key with its
/// Keystore key at once, and overwrites it.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterSealedView {
    /// The sealed seed.
    pub sealed: Vec<u8>,
    /// The data key, 32 bytes.
    pub data_key: Vec<u8>,
}

impl fmt::Debug for StarterSealedView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StarterSealedView(<hidden>)")
    }
}

wire_record!(StarterSealedView { sealed, data_key });

fn sealed_view(s: Sealed) -> StarterSealedView {
    StarterSealedView {
        sealed: s.sealed.clone(),
        data_key: s.data_key.as_bytes().to_vec(),
    }
}

/// Op 184: seal the seed (SS-20) once the check-back is passed. The setup
/// is used up when it is sealed, and kept when the sealing is refused.
pub(crate) fn seal(slot: &StarterSlot) -> Result<StarterSealedView, AppError> {
    let mut session = slot.lock();
    let Session::Setup(setup) = &*session else {
        return Err(no_setup());
    };
    let sealed = slot
        .with_rng(|rng| setup.seal(rng))
        .map_err(|e| refusal(&e))?;
    *session = Session::Idle;
    Ok(sealed_view(sealed))
}

/// Op 185: begin the restore (SS-23), which the owner starts: the words
/// are typed on the keypad of op 183.
pub(crate) fn restore_start(slot: &StarterSlot) -> Result<(), AppError> {
    *slot.lock() = Session::Restore(Box::new(Restore::new()));
    Ok(())
}

/// Op 186: finish the restore (SS-23): the 12 typed words are checked
/// against the starter wallet's stored `description` and sealed again. The
/// words stay typed when they are refused, for the owner to fix one.
pub(crate) fn restore_finish(
    slot: &StarterSlot,
    description: String,
) -> Result<StarterSealedView, AppError> {
    let mut session = slot.lock();
    let Session::Restore(restore) = &*session else {
        return Err(no_setup());
    };
    let sealed = slot
        .with_rng(|rng| restore.finish(&description, rng))
        .map_err(|e| refusal(&e))?;
    *session = Session::Idle;
    Ok(sealed_view(sealed))
}

/// Op 193: drop the setup or the restore that is open, and wipe it.
pub(crate) fn cancel(slot: &StarterSlot) -> Result<(), AppError> {
    *slot.lock() = Session::Idle;
    Ok(())
}

// ---- using the key ---------------------------------------------------------

/// One coin of the starter wallet: the same record, on the wire and in the
/// app, as one coin of the refund float.
pub type StarterCoinView = FloatCoinView;

fn coin_of(v: &StarterCoinView) -> Result<Coin, AppError> {
    coin_with(v, |e| {
        refused_with(
            "starter-coin",
            format!("a coin's transaction: {e}"),
            starter::coin_unreadable_words(),
        )
    })
}

fn coins_of(list: &[StarterCoinView]) -> Result<Vec<Coin>, AppError> {
    list.iter().map(coin_of).collect()
}

/// The data key the app handed over, taken as a [`DataKey`] that wipes its
/// copy; the bytes this layer read are overwritten here too.
pub(crate) fn data_key_of(bytes: &mut [u8]) -> Result<DataKey, AppError> {
    let key = DataKey::from_slice(bytes).map_err(|e| refusal(&e));
    for b in bytes.iter_mut() {
        *b = 0;
    }
    std::hint::black_box(&bytes);
    key
}

/// The arguments of `starter_pay` (SS-30, SS-31): a queued refund to sign.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterPayArgs {
    /// The sealed seed the app stores.
    pub sealed: Vec<u8>,
    /// The data key, opened from the Keystore for this call only.
    pub data_key: Vec<u8>,
    /// The network of the refund's address.
    pub network: OwnerNetwork,
    /// The refund's address.
    pub address: String,
    /// The refund in satoshis.
    pub sats: u64,
    /// The fee rate in satoshis per 1000 virtual bytes (`starter_fee_rate`).
    pub fee_rate: u64,
    /// The index on the starter wallet's change chain for the change.
    pub change_index: u32,
    /// The fewest confirmations a coin needs.
    pub min_confirmations: u32,
    /// The lock time, a block height.
    pub lock_height: u32,
    /// The starter wallet's coins.
    pub coins: Vec<StarterCoinView>,
}

impl fmt::Debug for StarterPayArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StarterPayArgs")
            .field("sats", &self.sats)
            .finish_non_exhaustive()
    }
}

wire_record!(StarterPayArgs {
    sealed,
    data_key,
    network,
    address,
    sats,
    fee_rate,
    change_index,
    min_confirmations,
    lock_height,
    coins
});
wire_record_get!(StarterPayArgs {
    sealed,
    data_key,
    network,
    address,
    sats,
    fee_rate,
    change_index,
    min_confirmations,
    lock_height,
    coins
});

/// A finished transaction of the starter wallet (ops 187 and 188), ready
/// for the outbox. The destination is the address in full, for the owner to
/// see (SS-31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterPaidView {
    /// The transaction, consensus bytes.
    pub tx: Vec<u8>,
    /// Its ID, hex.
    pub txid: String,
    /// The fee in satoshis.
    pub fee: u64,
    /// The virtual size.
    pub vsize: u64,
    /// What the destination is paid.
    pub paid: u64,
    /// The index of the starter wallet's change, if there is change.
    pub change_index: Option<u32>,
    /// The change in satoshis, if there is any.
    pub change_sats: Option<u64>,
    /// The destination, as an address.
    pub destination: String,
}

wire_record!(StarterPaidView {
    tx,
    txid,
    fee,
    vsize,
    paid,
    change_index,
    change_sats,
    destination
});

fn paid_view(p: &starter::StarterPaid, destination: String) -> StarterPaidView {
    StarterPaidView {
        tx: serialize(&p.tx),
        txid: p.txid.to_string(),
        fee: p.fee,
        vsize: p.vsize,
        paid: p.paid,
        change_index: p.change.map(|(i, _)| i),
        change_sats: p.change.map(|(_, s)| s),
        destination,
    }
}

/// Op 187: sign a queued refund (SS-30).
pub(crate) fn pay(mut a: StarterPayArgs) -> Result<StarterPaidView, AppError> {
    let key = data_key_of(&mut a.data_key)?;
    let to = Address::from_str(&a.address)
        .map_err(|e| refused_by("address-invalid", format!("{:?}: {e}", a.address)))?
        .require_network(a.network.network())
        .map_err(|e| refused_by("address-invalid", format!("{:?}: {e}", a.address)))?;
    let coins = coins_of(&a.coins)?;
    let paid = starter::pay_refund(
        &a.sealed,
        &key,
        &coins,
        &to.script_pubkey(),
        a.sats,
        a.fee_rate,
        a.change_index,
        a.min_confirmations,
        a.lock_height,
    )
    .map_err(|e| refusal(&e))?;
    Ok(paid_view(&paid, to.to_string()))
}

/// The arguments of `starter_move` (SS-42): everything to the new wallet.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterMoveArgs {
    /// The sealed seed the app stores.
    pub sealed: Vec<u8>,
    /// The data key, opened from the Keystore for this call only.
    pub data_key: Vec<u8>,
    /// The new wallet's description, as it was connected.
    pub new_wallet: String,
    /// The index on the new wallet's receive chain: the next unused one.
    pub new_index: u32,
    /// The fee rate in satoshis per 1000 virtual bytes.
    pub fee_rate: u64,
    /// The fewest confirmations a coin needs; every coin must have them.
    pub min_confirmations: u32,
    /// The lock time, a block height.
    pub lock_height: u32,
    /// The starter wallet's coins.
    pub coins: Vec<StarterCoinView>,
}

impl fmt::Debug for StarterMoveArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StarterMoveArgs")
            .field("new_index", &self.new_index)
            .finish_non_exhaustive()
    }
}

wire_record!(StarterMoveArgs {
    sealed,
    data_key,
    new_wallet,
    new_index,
    fee_rate,
    min_confirmations,
    lock_height,
    coins
});
wire_record_get!(StarterMoveArgs {
    sealed,
    data_key,
    new_wallet,
    new_index,
    fee_rate,
    min_confirmations,
    lock_height,
    coins
});

/// Op 188: the one move of every coin to the new wallet's next address
/// (SS-42).
pub(crate) fn move_all(mut a: StarterMoveArgs) -> Result<StarterPaidView, AppError> {
    let key = data_key_of(&mut a.data_key)?;
    let coins = coins_of(&a.coins)?;
    move_with(
        &a.sealed,
        &key,
        &a.new_wallet,
        a.new_index,
        &coins,
        a.fee_rate,
        a.min_confirmations,
        a.lock_height,
    )
}

/// The move of every one of `coins` to the new wallet `description` at
/// `new_index` (SS-42).
#[allow(clippy::too_many_arguments)]
pub(crate) fn move_with(
    sealed: &[u8],
    key: &DataKey,
    description: &str,
    new_index: u32,
    coins: &[Coin],
    fee_rate: u64,
    min_confirmations: u32,
    lock_height: u32,
) -> Result<StarterPaidView, AppError> {
    let new = ImportedWallet::read(description).map_err(|e| {
        refused_with(
            "starter-new-wallet",
            format!("the new wallet: {e:?}"),
            starter::new_wallet_unreadable_words(),
        )
    })?;
    let to: ScriptBuf = starter::receive_script_of(&new, new_index).map_err(|e| refusal(&e))?;
    let destination = Address::from_script(&to, new.network())
        .map(|x| x.to_string())
        .unwrap_or_default();
    let paid = starter::move_all(
        sealed,
        key,
        coins,
        &new,
        new_index,
        fee_rate,
        min_confirmations,
        lock_height,
    )
    .map_err(|e| refusal(&e))?;
    Ok(paid_view(&paid, destination))
}

/// The arguments of `starter_open` (SS-20): the sealed seed and its data
/// key, to learn whether they open and what wallet is inside.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterOpenArgs {
    /// The sealed seed.
    pub sealed: Vec<u8>,
    /// The data key, for this call only.
    pub data_key: Vec<u8>,
}

impl fmt::Debug for StarterOpenArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StarterOpenArgs(<hidden>)")
    }
}

wire_record!(StarterOpenArgs { sealed, data_key });
wire_record_get!(StarterOpenArgs { sealed, data_key });

/// Op 196: the public wallet inside a sealed seed. A refusal of the open is
/// the sign for the app to offer the restore (SS-23).
pub(crate) fn open(mut a: StarterOpenArgs) -> Result<StarterWalletView, AppError> {
    let key = data_key_of(&mut a.data_key)?;
    let wallet = starter::wallet_of_sealed(&a.sealed, &key).map_err(|e| refusal(&e))?;
    wallet_view(&wallet)
}

/// The arguments of `starter_fee_rate` (SS-30): the recent rate of the
/// blocks the device checked, and the rate the owner typed, each in
/// satoshis per 1000 virtual bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterFeeArgs {
    /// `FeeHistory::recent_rate`, when there is one.
    pub recent: Option<u64>,
    /// What the owner typed, when anything.
    pub typed: Option<u64>,
}

wire_record!(StarterFeeArgs { recent, typed });
wire_record_get!(StarterFeeArgs { recent, typed });

/// Op 195: the fee rate of a payment of the starter wallet.
pub(crate) fn fee_rate(a: StarterFeeArgs) -> Result<u64, AppError> {
    starter::fee_rate(a.recent, a.typed).map_err(|e| refusal(&e))
}

// ---- the limit and the date -------------------------------------------------

/// Where the starter wallet stands (SS-37), without its time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarterLimitKind {
    /// Under the limit and before the date.
    Under,
    /// At the limit or over it.
    AtLimit,
    /// The date has come.
    PastDate,
    /// "Not yet" is in force.
    NotYet,
}

wire_unit_enum!(StarterLimitKind {
    Under = 0,
    AtLimit = 1,
    PastDate = 2,
    NotYet = 3
});

/// A change of the settings, as SS-37 sorts them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StarterChangeKind {
    /// Raising the limit.
    RaiseLimit,
    /// Moving the date later.
    LaterDate,
    /// Adding a counter.
    AddCounter,
    /// Turning "Stop bitcoin sales above my limit" off.
    StopAboveOff,
    /// Any other change.
    Other,
    /// Lowering the limit.
    LowerLimit,
    /// Moving the date earlier.
    EarlierDate,
    /// Turning "Stop bitcoin sales above my limit" on.
    StopAboveOn,
}

wire_unit_enum!(StarterChangeKind {
    RaiseLimit = 0,
    LaterDate = 1,
    AddCounter = 2,
    StopAboveOff = 3,
    Other = 4,
    LowerLimit = 5,
    EarlierDate = 6,
    StopAboveOn = 7
});

/// The arguments of `starter_limit_state` (SS-35 to SS-37). The limit and
/// the date are the phone's own records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterLimitArgs {
    /// What the starter wallet holds, counting what is on its way.
    pub held: u64,
    /// The most it may hold, in satoshis.
    pub limit: u64,
    /// The time now, seconds since the epoch.
    pub now: u64,
    /// The date to move, seconds since the epoch.
    pub move_by: u64,
    /// The end of the "Not yet" the owner took, if one.
    pub not_yet_until: Option<u64>,
}

wire_record!(StarterLimitArgs {
    held,
    limit,
    now,
    move_by,
    not_yet_until
});
wire_record_get!(StarterLimitArgs {
    held,
    limit,
    now,
    move_by,
    not_yet_until
});

/// Where the starter wallet stands (op 189).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterLimitView {
    /// The state.
    pub state: StarterLimitKind,
    /// When a "Not yet" ends, in that state only.
    pub until: Option<u64>,
}

wire_record!(StarterLimitView { state, until });

/// Op 189: the state of the limit and the date.
pub(crate) fn limit_state(a: StarterLimitArgs) -> Result<StarterLimitView, AppError> {
    use starter::LimitState;
    let (state, until) =
        match starter::limit_state(a.held, a.limit, a.now, a.move_by, a.not_yet_until) {
            LimitState::Under => (StarterLimitKind::Under, None),
            LimitState::AtLimit => (StarterLimitKind::AtLimit, None),
            LimitState::PastDate => (StarterLimitKind::PastDate, None),
            LimitState::NotYet { until } => (StarterLimitKind::NotYet, Some(until)),
        };
    Ok(StarterLimitView { state, until })
}

/// The arguments of `starter_change_held` (SS-37).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterHeldArgs {
    /// The state of the limit.
    pub state: StarterLimitKind,
    /// The change asked for.
    pub change: StarterChangeKind,
}

wire_record!(StarterHeldArgs { state, change });
wire_record_get!(StarterHeldArgs { state, change });

/// Op 190: whether a change of settings waits (SS-37). The state in the
/// arguments carries no end time, which the answer does not need.
pub(crate) fn change_held(a: StarterHeldArgs) -> Result<bool, AppError> {
    use starter::{LimitState, SettingsChange};
    let state = match a.state {
        StarterLimitKind::Under => LimitState::Under,
        StarterLimitKind::AtLimit => LimitState::AtLimit,
        StarterLimitKind::PastDate => LimitState::PastDate,
        StarterLimitKind::NotYet => LimitState::NotYet { until: 0 },
    };
    let change = match a.change {
        StarterChangeKind::RaiseLimit => SettingsChange::RaiseLimit,
        StarterChangeKind::LaterDate => SettingsChange::LaterDate,
        StarterChangeKind::AddCounter => SettingsChange::AddCounter,
        StarterChangeKind::StopAboveOff => SettingsChange::StopAboveOff,
        StarterChangeKind::Other => SettingsChange::Other,
        StarterChangeKind::LowerLimit => SettingsChange::LowerLimit,
        StarterChangeKind::EarlierDate => SettingsChange::EarlierDate,
        StarterChangeKind::StopAboveOn => SettingsChange::StopAboveOn,
    };
    Ok(starter::change_held(state, change))
}

/// The arguments of `starter_offers_bitcoin` (SS-38).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StarterOfferArgs {
    /// Whether "Stop bitcoin sales above my limit" is on.
    pub stop_above_limit: bool,
    /// What the starter wallet holds.
    pub held: u64,
    /// The limit.
    pub limit: u64,
}

wire_record!(StarterOfferArgs {
    stop_above_limit,
    held,
    limit
});
wire_record_get!(StarterOfferArgs {
    stop_above_limit,
    held,
    limit
});

/// Op 191: whether a counter may offer bitcoin (SS-38).
pub(crate) fn offers_bitcoin(a: StarterOfferArgs) -> Result<bool, AppError> {
    Ok(starter::counter_offers_bitcoin(
        a.stop_above_limit,
        a.held,
        a.limit,
    ))
}

/// Op 192: whether the sealed seed may be deleted: the move has one
/// confirmation (SS-43).
pub(crate) fn seed_may_go(confirmations: u32) -> Result<bool, AppError> {
    Ok(starter::seed_may_go(confirmations))
}

/// Op 194: the end of a "Not yet" taken at `now` and asked for `asked`
/// seconds: at most 7 days (SS-37).
pub(crate) fn not_yet(a: (u64, u64)) -> Result<u64, AppError> {
    Ok(starter::not_yet_until(a.0, a.1))
}

// ---- the core's words -------------------------------------------------------

/// Op 204: the words of the home line (SS-36).
pub(crate) fn line_words() -> Result<WordsView, AppError> {
    Ok(WordsView(starter::line_words()))
}

/// Op 205: the words of a state of the limit and the date (SS-37); none
/// when the wallet is under its limit and before its date.
pub(crate) fn state_words(kind: StarterLimitKind) -> Result<Option<WordsView>, AppError> {
    use starter::LimitState;
    let state = match kind {
        StarterLimitKind::Under => LimitState::Under,
        StarterLimitKind::AtLimit => LimitState::AtLimit,
        StarterLimitKind::PastDate => LimitState::PastDate,
        StarterLimitKind::NotYet => LimitState::NotYet { until: 0 },
    };
    Ok(starter::state_words(state).map(WordsView))
}

/// Op 206: the words said when a change of settings waits (SS-37).
pub(crate) fn held_words() -> Result<WordsView, AppError> {
    Ok(WordsView(starter::held_words()))
}

/// Op 207: the words said when the key is deleted (SS-43).
pub(crate) fn deleted_words() -> Result<WordsView, AppError> {
    Ok(WordsView(starter::deleted_words()))
}

// ---- the owner node's watch of the starter wallet ----------------------------

/// The arguments of `starter_pay_node` (SS-30, SS-31): the queued refund to
/// pay, by its number in the owner's queue. The coins are the owner node's
/// watch of the wallet, the address and the amount are the refund's, and the
/// lock time is the node's tip, so the app hands over none of them.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterNodePayArgs {
    /// The sealed seed the app stores.
    pub sealed: Vec<u8>,
    /// The data key, opened from the Keystore for this call only.
    pub data_key: Vec<u8>,
    /// The refund's number in the queue.
    pub refund: u32,
    /// The fee rate in satoshis per 1000 virtual bytes (`starter_fee_rate`).
    pub fee_rate: u64,
    /// The fewest confirmations a coin needs.
    pub min_confirmations: u32,
}

impl fmt::Debug for StarterNodePayArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StarterNodePayArgs")
            .field("refund", &self.refund)
            .finish_non_exhaustive()
    }
}

wire_record!(StarterNodePayArgs {
    sealed,
    data_key,
    refund,
    fee_rate,
    min_confirmations
});
wire_record_get!(StarterNodePayArgs {
    sealed,
    data_key,
    refund,
    fee_rate,
    min_confirmations
});

/// The arguments of `starter_move_switched` (SS-42): everything to the new
/// wallet. The coins are the owner node's watch of the starter wallet and
/// the new wallet's index is the next one of the owner's wallet state.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterNodeMoveArgs {
    /// The sealed seed the app stores.
    pub sealed: Vec<u8>,
    /// The data key, opened from the Keystore for this call only.
    pub data_key: Vec<u8>,
    /// The new wallet's description, as it was connected.
    pub new_wallet: String,
    /// The time, in seconds.
    pub now: u64,
    /// The fee rate in satoshis per 1000 virtual bytes.
    pub fee_rate: u64,
    /// The fewest confirmations a coin needs; every coin must have them.
    pub min_confirmations: u32,
}

impl fmt::Debug for StarterNodeMoveArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StarterNodeMoveArgs")
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

wire_record!(StarterNodeMoveArgs {
    sealed,
    data_key,
    new_wallet,
    now,
    fee_rate,
    min_confirmations
});
wire_record_get!(StarterNodeMoveArgs {
    sealed,
    data_key,
    new_wallet,
    now,
    fee_rate,
    min_confirmations
});

/// The arguments of `starter_late_move` (SS-43): the coins of a former
/// wallet, to the wallet in force.
#[derive(Clone, PartialEq, Eq)]
pub struct StarterLateMoveArgs {
    /// The sealed seed the app stores (or the one the restore made).
    pub sealed: Vec<u8>,
    /// The data key, opened from the Keystore for this call only.
    pub data_key: Vec<u8>,
    /// The checksum of the former wallet whose coins move.
    pub wallet: Vec<u8>,
    /// The time, in seconds.
    pub now: u64,
    /// The fee rate in satoshis per 1000 virtual bytes.
    pub fee_rate: u64,
    /// The fewest confirmations a coin needs; every coin must have them.
    pub min_confirmations: u32,
}

impl fmt::Debug for StarterLateMoveArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StarterLateMoveArgs")
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

wire_record!(StarterLateMoveArgs {
    sealed,
    data_key,
    wallet,
    now,
    fee_rate,
    min_confirmations
});
wire_record_get!(StarterLateMoveArgs {
    sealed,
    data_key,
    wallet,
    now,
    fee_rate,
    min_confirmations
});

/// A former wallet that holds a coin the node saw (SS-43).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterLateView {
    /// The checksum of the former wallet's pinned frame.
    pub wallet: Vec<u8>,
    /// What its coins add up to, in satoshis.
    pub sats: u64,
    /// How many coins it holds.
    pub coins: u32,
}

wire_record!(StarterLateView {
    wallet,
    sats,
    coins
});

/// The arguments of `starter_send` (SS-30, SS-42): a signed transaction for
/// the node's outbox, and the queued refunds it pays (none for the move).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterSendArgs {
    /// The transaction, consensus bytes, as `starter_pay_node` gave it.
    pub tx: Vec<u8>,
    /// The refunds it pays.
    pub refunds: Vec<u32>,
}

wire_record!(StarterSendArgs { tx, refunds });
wire_record_get!(StarterSendArgs { tx, refunds });

/// What a call to re-pin the owner's wallet did (SS-41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnRepinKind {
    /// Every counter had switched: the new wallet is pinned now.
    Done,
    /// The new wallet was pinned already.
    AlreadyDone,
    /// A counter has not switched: nothing changed.
    NotReady,
}

wire_unit_enum!(OwnRepinKind {
    Done = 0,
    AlreadyDone = 1,
    NotReady = 2
});

/// The answer of `own_repin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnRepinView {
    /// What the call did.
    pub kind: OwnRepinKind,
    /// The pinned wallet's file after the call: the file the app keeps and
    /// starts the node with, so that the node watches the new wallet. Empty
    /// when nothing was re-pinned.
    pub wallet: Vec<u8>,
    /// Counters with no record for the new wallet yet.
    pub waiting: u32,
    /// Counters whose record says they did not switch.
    pub refused: u32,
}

wire_record!(OwnRepinView {
    kind,
    wallet,
    waiting,
    refused
});

pub(crate) fn not_watching(why: impl fmt::Display) -> AppError {
    from_words(
        Kind::State,
        "starter-not-watching",
        format!("the node's watch of the starter wallet is not ready: {why}"),
        starter::not_watching_words(),
    )
}

/// SS-30: the queued refund is not there, or is not queued any more.
pub(crate) fn no_such_refund(id: u32) -> AppError {
    refused_with(
        "starter-refund",
        format!("refund {id} is not a refund that waits in the queue"),
        starter::no_such_refund_words(),
    )
}

/// The refund `id`, as the queue holds it, paid from the node's coins
/// (SS-30): the address and the amount are the refund's.
pub(crate) fn pay_from_node(
    mut a: StarterNodePayArgs,
    refund: &openbmt_owner::Refund,
    network: bitcoin::Network,
    coins: &[Coin],
    change_index: u32,
    lock_height: u32,
) -> Result<StarterPaidView, AppError> {
    let key = data_key_of(&mut a.data_key)?;
    let paid = starter::pay_refund(
        &a.sealed,
        &key,
        coins,
        &refund.script,
        refund.sats,
        a.fee_rate,
        change_index,
        a.min_confirmations,
        lock_height,
    )
    .map_err(|e| refusal(&e))?;
    let destination = Address::from_script(&refund.script, network)
        .map(|x| x.to_string())
        .unwrap_or_default();
    Ok(paid_view(&paid, destination))
}
