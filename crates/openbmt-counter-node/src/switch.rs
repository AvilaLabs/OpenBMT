//! A switch from the starter wallet that waits for the manager
//! (`docs/COUNTER-RULES.md` section 26, CR-256 to CR-264).
//!
//! A shop that started simple pays into a starter wallet whose key is on the
//! owner phone. When it moves to a key made offline, the owner phone sends
//! each counter a switch (`OBM]`, CR-257). The counter checks it (CR-258)
//! and keeps it as its one switch to check in [`crate::dirstore::SWITCH_FILE`]
//! (CR-259). A manager compares the new wallet's first address with the new
//! wallet's own screen and confirms with the PIN (CR-260, CR-261); only then
//! do the counter's sales go to the new wallet.
//!
//! The order of what is written (CR-261): the range book, the pairing record,
//! the record the owner reads (CR-262), then the file is removed. The pairing
//! record is the app's, sealed with its key, so the node hands the app the
//! new record to keep ([`crate::node::Node::record_update`]) and goes on when
//! the app says it kept it ([`crate::node::Node::switch_record_kept`]). A start
//! that finds the book ahead of the record finishes from the second step.

use std::fmt;

use openbmt_core::identity::DeviceId;
use openbmt_core::pairing_own::{check_range, OwnPairError, SwitchOwn};
use openbmt_core::pinned_wallet::{PinnedWallet, WalletKind};
use openbmt_core::wire::DecodeError;

/// CR-259: how long a switch waits for the manager: seven days from the
/// time it arrived.
pub const SWITCH_PENDING_SECS: u64 = 7 * 86_400;

/// Why a switch was not taken to be checked (CR-257, CR-258, CR-263). Each
/// names its rule; nothing changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchRefused {
    /// CR-257: the frame is not a switch.
    Frame(DecodeError),
    /// CR-258, CR-263: the counter's record has no starter flag: it was
    /// paired with another kind of wallet, or it has switched already.
    NotStarter,
    /// CR-258: the signature is not the pinned owner root's.
    NotFromOwner,
    /// CR-258: the switch is for another counter.
    WrongCounter,
    /// CR-258: the hash is not that of the wallet this counter pins now.
    WrongWallet,
    /// CR-258: the new wallet does not pass WL-21 and WL-22.
    NewWalletRefused,
    /// CR-258: the new wallet is the wallet pinned now.
    SameWallet,
    /// CR-258: the range fails CR-232 for this counter and the new wallet, or
    /// holds the number of a sale this counter already has.
    Range(OwnPairError),
    /// CR-259: the file could not be written.
    NotWritten(String),
}

impl SwitchRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Frame(_) => "CR-257",
            Self::NotStarter => "CR-263",
            Self::NotFromOwner
            | Self::WrongCounter
            | Self::WrongWallet
            | Self::NewWalletRefused
            | Self::SameWallet
            | Self::Range(_) => "CR-258",
            Self::NotWritten(_) => "CR-259",
        }
    }
}

impl fmt::Display for SwitchRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Frame(e) => write!(f, "what came as a switch is not one: {e}"),
            Self::NotStarter => write!(
                f,
                "this counter was not paired with a starter wallet, or has switched already"
            ),
            Self::NotFromOwner => write!(f, "the switch is not signed by the owner root"),
            Self::WrongCounter => write!(f, "the switch is for another counter"),
            Self::WrongWallet => write!(
                f,
                "the switch names another wallet than the one this counter pins"
            ),
            Self::NewWalletRefused => write!(f, "the new wallet is refused"),
            Self::SameWallet => write!(f, "the new wallet is the wallet pinned now"),
            Self::Range(e) => write!(f, "the first range is refused: {e}"),
            Self::NotWritten(e) => write!(f, "the switch was not saved: {e}"),
        }
    }
}

impl std::error::Error for SwitchRefused {}

/// Why the manager's answer to a waiting switch did not go through
/// (CR-260, CR-261).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwitchStepRefused {
    /// No switch waits.
    NothingWaits,
    /// The counter has no pairing record to rewrite, so nothing was changed.
    NoRecord,
    /// The switch no longer passes the checks of CR-258, so nothing was
    /// changed.
    Refused(SwitchRefused),
    /// A write failed. What was written before it stays, and the start
    /// finishes the switch (CR-261).
    NotWritten(String),
}

impl SwitchStepRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NothingWaits | Self::NoRecord => "CR-261",
            Self::Refused(e) => e.rule(),
            Self::NotWritten(_) => "CR-40",
        }
    }
}

impl fmt::Display for SwitchStepRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::NothingWaits => write!(f, "no switch waits"),
            Self::NoRecord => write!(f, "the counter has no pairing record to write again"),
            Self::Refused(e) => write!(f, "{e}"),
            Self::NotWritten(e) => write!(f, "a write of the switch failed: {e}"),
        }
    }
}

impl std::error::Error for SwitchStepRefused {}

/// Something the manager is told about a switch that came to nothing
/// (CR-259, CR-264).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchNotice {
    /// A switch waited seven days and was dropped.
    Dropped,
}

/// What the manager is shown of the waiting switch (CR-260).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSwitch {
    /// The new wallet's first receive address (receive chain, index 0).
    pub address: String,
    /// The kind of wallet beneath it.
    pub kind: WalletKind,
    /// The hash of the new wallet.
    pub wallet: [u8; 32],
    /// When the switch arrived.
    pub arrived: u64,
    /// When the switch is dropped if nobody answered it.
    pub expires_at: u64,
}

/// What the node keeps of the one waiting switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    /// The switch.
    pub switch: SwitchOwn,
    /// The `OBM]` frame as it arrived, which is the file's content.
    pub bytes: Vec<u8>,
    /// When it arrived, by the node's clock.
    pub arrived: u64,
}

impl Waiting {
    /// The time after which the switch is dropped (CR-259).
    pub fn expires_at(&self) -> u64 {
        self.arrived.saturating_add(SWITCH_PENDING_SECS)
    }
}

/// CR-258: the checks of a switch, in the order the rule gives them. `owner`
/// is the pinned owner root, `me` this counter, `pinned` the wallet pinned
/// now and `starter` whether the record has the flag. `min_count` is the
/// counter's smallest range, and `holds_sale_in` says whether a sale of this
/// counter has a number in `start..end` (a sale is kept by its address
/// index, so a range of the new wallet must not hold one).
pub fn check_switch(
    sw: &SwitchOwn,
    owner: &DeviceId,
    me: &DeviceId,
    starter: bool,
    pinned: &PinnedWallet,
    min_count: u32,
    holds_sale_in: impl Fn(u32, u32) -> bool,
) -> Result<(), SwitchRefused> {
    if !starter {
        return Err(SwitchRefused::NotStarter);
    }
    if !sw.verify(owner) {
        return Err(SwitchRefused::NotFromOwner);
    }
    if sw.counter != *me {
        return Err(SwitchRefused::WrongCounter);
    }
    if sw.from != pinned.checksum32() {
        return Err(SwitchRefused::WrongWallet);
    }
    // WL-21, WL-22: the wallet is decoded again from its own bytes. The
    // network is the main one for every pinned wallet.
    if PinnedWallet::from_bytes(&sw.wallet.to_bytes()).is_err() {
        return Err(SwitchRefused::NewWalletRefused);
    }
    if sw.wallet.checksum32() == pinned.checksum32() {
        return Err(SwitchRefused::SameWallet);
    }
    check_range(&sw.first_range, owner, me, min_count).map_err(SwitchRefused::Range)?;
    let r = sw.first_range.range;
    if holds_sale_in(r.start, r.end) {
        return Err(SwitchRefused::Range(OwnPairError::Range(
            openbmt_core::pinned_wallet::RangeError::OverlapsUsed,
        )));
    }
    Ok(())
}

impl PendingSwitch {
    /// The page's data for a waiting switch, or `None` when the new wallet
    /// has no first address.
    pub fn of(waiting: &Waiting) -> Option<Self> {
        let wallet = &waiting.switch.wallet;
        Some(Self {
            address: wallet.receive_address(0).ok()?.to_string(),
            kind: wallet.kind(),
            wallet: wallet.checksum32(),
            arrived: waiting.arrived,
            expires_at: waiting.expires_at(),
        })
    }
}
