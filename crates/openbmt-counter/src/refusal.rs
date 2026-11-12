//! Why the counter refused something.
//!
//! Every refusal names the rule of `docs/COUNTER-RULES.md` that it enforces
//! ([`OpenRefused::rule`] and the like), and its `Display` text starts with
//! the rule ID. A refusal means the step did not happen; what had already
//! been stored by an earlier write of the same step is described on the
//! method that returns it.

use std::fmt;

use openbmt_core::batch::AddError;
use openbmt_core::command::CommandError;
use openbmt_core::heartbeat::HeartbeatError;
use openbmt_core::records::ChainError;
use openbmt_core::wire::DecodeError;
use openbmt_watch::{Problem, Refusal};

use crate::screen::{problem_text, State};
use crate::store::StoreFault;

/// Why a sale was not opened (CR-01 to CR-04, CR-09, CR-17, CR-75).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRefused<E> {
    /// CR-165: the settings in force do not list this counter: the owner
    /// removed it from the shop.
    Removed,
    /// CR-09: a rescan is pending after a restore or a reset.
    RescanPending,
    /// CR-01: the check is not healthy; the problem is named when known.
    Unhealthy(Option<Problem>),
    /// CR-05: the amount is zero.
    ZeroAmount,
    /// CR-05: the amount needs more than 21,000,000 bitcoin at the rate.
    AmountTooLarge,
    /// CR-190: the tip is more than the sale's own amount.
    TipTooLarge,
    /// CR-17: the amount needs less than the dust line, so a payment for it
    /// would be dust.
    BelowDust {
        /// What the amount needs at the rate.
        needs_sats: u64,
    },
    /// CR-75: the amount needs fewer satoshis than the shop's smallest
    /// bitcoin sale.
    BelowMinimum {
        /// What the amount needs at the rate.
        sats: u64,
        /// The smallest bitcoin sale, from the settings.
        min_sats: u64,
    },
    /// CR-58: the rate was fetched too long before the time of the sale, or
    /// after it.
    StaleRate {
        /// When the caller got the rate.
        fetched_at: u64,
        /// The time of the sale.
        now: u64,
    },
    /// CR-04: too many addresses were handed out in the last hour.
    RateLimited,
    /// CR-02: no address is left in the batches.
    NoAddress,
    /// CR-02: the address the batches handed out is already used by a sale.
    AddressInUse,
    /// CR-61: the name of the person who opens the sale is not a label.
    BadName,
    /// CR-206: the note on the sale is not plain text of 1 to 64 characters.
    BadNote,
    /// CR-03: the next index could not be stored, so no address was handed
    /// out.
    AddressNotStored(StoreFault<E>),
    /// CR-40: another write failed.
    Store(StoreFault<E>),
}

impl<E> OpenRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Removed => "CR-165",
            Self::RescanPending => "CR-09",
            Self::Unhealthy(_) => "CR-01",
            Self::ZeroAmount | Self::AmountTooLarge => "CR-05",
            Self::TipTooLarge => "CR-190",
            Self::BelowDust { .. } => "CR-17",
            Self::BelowMinimum { .. } => "CR-75",
            Self::StaleRate { .. } => "CR-58",
            Self::RateLimited => "CR-04",
            Self::NoAddress | Self::AddressInUse => "CR-02",
            Self::BadName => "CR-61",
            Self::BadNote => "CR-206",
            Self::AddressNotStored(_) => "CR-03",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for OpenRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Removed => write!(
                f,
                "this counter was removed from the shop by the owner; ask the owner"
            ),
            Self::RescanPending => write!(
                f,
                "a rescan is pending; do not offer Bitcoin, use another payment type"
            ),
            Self::Unhealthy(p) => write!(
                f,
                "the check is not healthy ({}); do not offer Bitcoin, use another payment type",
                problem_text(*p)
            ),
            Self::ZeroAmount => write!(f, "a sale needs an amount"),
            Self::AmountTooLarge => write!(f, "the amount is more than all the bitcoin there is"),
            Self::TipTooLarge => write!(f, "the tip is more than the sale itself"),
            Self::BelowDust { needs_sats } => write!(
                f,
                "the amount is only {needs_sats} sats, below the dust line; use another payment type"
            ),
            Self::BelowMinimum { sats, min_sats } => write!(
                f,
                "the amount is only {sats} sats, below the shop's smallest bitcoin sale of \
                 {min_sats} sats; use another payment type"
            ),
            Self::StaleRate { fetched_at, now } => write!(
                f,
                "the rate was fetched at {fetched_at} and the time is {now}; fetch a rate again"
            ),
            Self::RateLimited => write!(
                f,
                "too many addresses were handed out in the last hour; try again later"
            ),
            Self::NoAddress => write!(f, "no address is left; do not offer Bitcoin"),
            Self::AddressInUse => write!(f, "the address is already used by another sale"),
            Self::BadName => write!(
                f,
                "the name of the person is not a label of 1 to 64 characters with no control character"
            ),
            Self::BadNote => write!(
                f,
                "a note is plain text of 1 to 64 characters with no control character"
            ),
            Self::AddressNotStored(e) => write!(
                f,
                "the next address was not stored, so none was handed out: {e}"
            ),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for OpenRefused<E> {}

/// Why a sale could not be judged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JudgeError<E> {
    /// CR-11: the counter has no sale with this index.
    UnknownSale(u32),
    /// CR-40: a write failed. The caller must treat the sale as not safe
    /// and ask again; nothing was reported.
    Store(StoreFault<E>),
}

impl<E> JudgeError<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::UnknownSale(_) => "CR-11",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E> From<StoreFault<E>> for JudgeError<E> {
    fn from(e: StoreFault<E>) -> Self {
        Self::Store(e)
    }
}

impl<E: fmt::Display> fmt::Display for JudgeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::UnknownSale(i) => write!(f, "there is no sale with index {i}"),
            Self::Store(e) => write!(f, "{e}; do not hand over, ask again"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for JudgeError<E> {}

/// Why a new quote was refused (CR-05, CR-08).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequoteRefused<E> {
    /// CR-11: the counter has no sale with this index.
    UnknownSale(u32),
    /// CR-05: the sale has ended.
    SaleEnded,
    /// CR-08: enough has arrived; the sale takes no new quote.
    AlreadyEnough,
    /// CR-05: the sale already has the most quotes.
    TooManyQuotes,
    /// CR-207: a request to send keeps the one quote it was made with.
    IsRequest,
    /// CR-05: the amount needs more than 21,000,000 bitcoin at the rate.
    AmountTooLarge,
    /// CR-75: the new quote would put a sale with nothing received below
    /// the shop's smallest bitcoin sale.
    BelowMinimum {
        /// What the whole amount needs at the new rate.
        sats: u64,
        /// The smallest bitcoin sale, from the settings.
        min_sats: u64,
    },
    /// CR-58: the rate was fetched too long before the time of the quote, or
    /// after it.
    StaleRate {
        /// When the caller got the rate.
        fetched_at: u64,
        /// The time of the quote.
        now: u64,
    },
    /// CR-40: a write failed.
    Store(StoreFault<E>),
}

impl<E> RequoteRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::UnknownSale(_) => "CR-11",
            Self::SaleEnded | Self::TooManyQuotes | Self::AmountTooLarge => "CR-05",
            Self::AlreadyEnough => "CR-08",
            Self::IsRequest => "CR-207",
            Self::BelowMinimum { .. } => "CR-75",
            Self::StaleRate { .. } => "CR-58",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for RequoteRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::UnknownSale(i) => write!(f, "there is no sale with index {i}"),
            Self::SaleEnded => write!(f, "the sale has ended and takes no new quote"),
            Self::AlreadyEnough => write!(f, "enough has arrived; the sale takes no new quote"),
            Self::TooManyQuotes => write!(f, "the sale has the most quotes it can keep"),
            Self::IsRequest => write!(f, "a request to send keeps the quote it was made with"),
            Self::BelowMinimum { sats, min_sats } => write!(
                f,
                "at the new rate the sale is only {sats} sats, below the shop's smallest \
                 bitcoin sale of {min_sats} sats"
            ),
            Self::AmountTooLarge => write!(f, "the amount is more than all the bitcoin there is"),
            Self::StaleRate { fetched_at, now } => write!(
                f,
                "the rate was fetched at {fetched_at} and the time is {now}; fetch a rate again"
            ),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for RequoteRefused<E> {}

/// Why a note was not added to a sale (CR-206).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteRefused<E> {
    /// CR-11: the counter has no sale with this index.
    UnknownSale(u32),
    /// CR-206: the note is not plain text of 1 to 64 characters.
    BadNote,
    /// CR-206: the sale has a note already.
    AlreadyNoted,
    /// CR-40: a write failed.
    Store(StoreFault<E>),
}

impl<E> NoteRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::UnknownSale(_) => "CR-11",
            Self::BadNote | Self::AlreadyNoted => "CR-206",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for NoteRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::UnknownSale(i) => write!(f, "there is no sale with index {i}"),
            Self::BadNote => write!(
                f,
                "a note is plain text of 1 to 64 characters with no control character"
            ),
            Self::AlreadyNoted => write!(f, "the sale has a note already"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for NoteRefused<E> {}

/// Why a sale was not cancelled (CR-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelRefused<E> {
    /// CR-11: the counter has no sale with this index.
    UnknownSale(u32),
    /// CR-07: the sale has already ended.
    AlreadyEnded,
    /// CR-07: something has arrived for the sale. Money on a cancelled sale
    /// is a matter for the refund flow, not for cancelling.
    MoneyArrived,
    /// CR-61: the name of the person who cancels is not a label.
    BadName,
    /// CR-40: a write failed.
    Store(StoreFault<E>),
}

impl<E> CancelRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::UnknownSale(_) => "CR-11",
            Self::AlreadyEnded | Self::MoneyArrived => "CR-07",
            Self::BadName => "CR-61",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for CancelRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::UnknownSale(i) => write!(f, "there is no sale with index {i}"),
            Self::AlreadyEnded => write!(f, "the sale has already ended"),
            Self::MoneyArrived => write!(
                f,
                "something has arrived for this sale; it cannot be cancelled, \
                 money on it is for the refund flow"
            ),
            Self::BadName => write!(
                f,
                "the name of the person is not a label of 1 to 64 characters with no control character"
            ),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for CancelRefused<E> {}

/// Why a Lightning payment was not taken for a sale (CR-109).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightningRefused<E> {
    /// CR-11: the counter has no sale with this index.
    UnknownSale(u32),
    /// CR-109: the sale had already ended (expired or cancelled) or was
    /// final, so nothing is changed. The payment is in the shop's node.
    SaleClosed,
    /// CR-109: the sale was paid another way already: accepted at once, by a
    /// person, or with enough on its address. Nothing is changed.
    PaidAnotherWay,
    /// CR-109: the node reports a payment for less than the least any quote
    /// of the sale asks, or for nothing.
    TooLittle {
        /// The whole satoshis the node received.
        sats: u64,
        /// The least any quote of the sale asks.
        least: u64,
    },
    /// The sale could not be judged.
    Judge(JudgeError<E>),
    /// CR-40: a write failed. The payment is not taken; asking again is safe.
    Store(StoreFault<E>),
}

impl<E> LightningRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::UnknownSale(_) => "CR-11",
            Self::SaleClosed | Self::PaidAnotherWay | Self::TooLittle { .. } => "CR-109",
            Self::Judge(e) => e.rule(),
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for LightningRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::UnknownSale(i) => write!(f, "there is no sale with index {i}"),
            Self::SaleClosed => write!(
                f,
                "the sale has ended or is final, so a Lightning payment is not added to it"
            ),
            Self::PaidAnotherWay => write!(f, "the sale was paid another way already"),
            Self::TooLittle { sats, least } => write!(
                f,
                "the node received {sats} sats and the sale asks at least {least} sats"
            ),
            Self::Judge(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for LightningRefused<E> {}

/// Why an override was refused (CR-20 to CR-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverrideRefused<E> {
    /// CR-11: the counter has no sale with this index.
    UnknownSale(u32),
    /// CR-22: the person's name is empty or too long.
    BadName,
    /// CR-20: the sale is in a state a person may not accept.
    WrongState(State),
    /// CR-21: a cashier's override is above the per-sale instant limit.
    OverSaleLimit {
        /// The amount at risk.
        sats: u64,
        /// The limit.
        limit: u64,
    },
    /// CR-21: it would take the cashier's last 24 hours past the daily cap.
    OverDailyCap {
        /// The amount at risk.
        sats: u64,
        /// The cashier's overrides inside the window already.
        already: u64,
        /// The cap.
        cap: u64,
    },
    /// CR-21: it would take the manager's last 24 hours past the ceiling.
    OverCeiling {
        /// The amount at risk.
        sats: u64,
        /// The manager's overrides inside the window already.
        already: u64,
        /// The ceiling.
        ceiling: u64,
    },
    /// CR-21: the desk keeps no more override entries until older ones leave
    /// the window.
    TooManyOverrides,
    /// CR-20: the exposure ledger has no room for the amount.
    NoRoom,
    /// The sale could not be judged first.
    Judge(JudgeError<E>),
    /// CR-40: a write failed.
    Store(StoreFault<E>),
}

impl<E> OverrideRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::UnknownSale(_) | Self::WrongState(_) | Self::NoRoom => "CR-20",
            Self::BadName => "CR-22",
            Self::OverSaleLimit { .. }
            | Self::OverDailyCap { .. }
            | Self::OverCeiling { .. }
            | Self::TooManyOverrides => "CR-21",
            Self::Judge(e) => e.rule(),
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for OverrideRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::UnknownSale(i) => write!(f, "there is no sale with index {i}"),
            Self::BadName => write!(f, "the person needs a name of 1 to 64 bytes"),
            Self::WrongState(s) => write!(f, "a sale that shows \"{}\" cannot be accepted", s.name()),
            Self::OverSaleLimit { sats, limit } => write!(
                f,
                "{sats} sats is above the {limit} sats a cashier may accept"
            ),
            Self::OverDailyCap { sats, already, cap } => write!(
                f,
                "{sats} sats on top of {already} sats in the last 24 hours is above the cashier's cap of {cap} sats"
            ),
            Self::OverCeiling {
                sats,
                already,
                ceiling,
            } => write!(
                f,
                "{sats} sats on top of {already} sats in the last 24 hours is above the manager's ceiling of {ceiling} sats"
            ),
            Self::TooManyOverrides => write!(f, "too many overrides in the last 24 hours"),
            Self::NoRoom => write!(f, "the exposure ledger has no room for this amount"),
            Self::Judge(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for OverrideRefused<E> {}

/// Why an owner command was not carried out (CR-14, CR-40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandRefused<E> {
    /// CR-14: the command is not for this counter, is not numbered above
    /// the last one taken, or is not signed by the owner. Nothing changed.
    Refused(CommandError),
    /// CR-40: a write failed. If the command number was stored, the command
    /// is used up even though its action may not have happened: a command
    /// that could be played again is worse than one that is lost, and the
    /// owner can send another.
    Store(StoreFault<E>),
}

impl<E> CommandRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Refused(_) => "CR-14",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for CommandRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Refused(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for CommandRefused<E> {}

/// Why an owner heartbeat was not taken (CR-50, CR-51, CR-54).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatRefused<E> {
    /// CR-50: the heartbeat is not for this counter, is not numbered above
    /// the last one taken, or is not signed by the owner. Nothing changed.
    Refused(HeartbeatError),
    /// CR-51: the heartbeat is timed too far ahead of the counter's clock.
    /// Nothing changed, and its number is not used up.
    FromTheFuture {
        /// How far ahead, in seconds.
        ahead_secs: u64,
    },
    /// CR-54: the number could not be stored, so the heartbeat does not
    /// count. Nothing changed.
    Store(StoreFault<E>),
}

impl<E> HeartbeatRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Refused(_) => "CR-50",
            Self::FromTheFuture { .. } => "CR-51",
            Self::Store(_) => "CR-54",
        }
    }
}

impl<E: fmt::Display> fmt::Display for HeartbeatRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Refused(e) => write!(f, "{e}"),
            Self::FromTheFuture { ahead_secs } => write!(
                f,
                "the heartbeat is timed {ahead_secs} seconds ahead of this counter's clock"
            ),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for HeartbeatRefused<E> {}

/// Why a sighting was not taken (WR-40, CR-40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SightingError<E> {
    /// The bytes are not a transaction. Nothing changed.
    Refused(Refusal),
    /// CR-40: a write failed. Sales stored before it keep what they took;
    /// offering the sighting again is safe.
    Store(StoreFault<E>),
}

impl<E> SightingError<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Refused(r) => r.rule(),
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for SightingError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::Store(e) => write!(f, "{}: {e}", self.rule()),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for SightingError<E> {}

/// Why a block was not taken (WR-01, CR-40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockError<E> {
    /// The block is not on the chain now. Nothing changed.
    Refused(Refusal),
    /// CR-40: a write failed. Sales stored before it keep what they took;
    /// offering the block again is safe.
    Store(StoreFault<E>),
}

impl<E> BlockError<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Refused(r) => r.rule(),
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for BlockError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::Store(e) => write!(f, "{}: {e}", self.rule()),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for BlockError<E> {}

/// Why stored state was not accepted when the counter started (CR-42).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// CR-42: the stored address batches do not read back.
    Batches(DecodeError),
    /// CR-42: the stored exposure ledger does not read back.
    Exposure(DecodeError),
    /// CR-42: the stored pause tracker does not read back.
    Pause(DecodeError),
    /// CR-42: the stored desk does not read back.
    Desk(DecodeError),
    /// CR-42: the stored record chain does not verify.
    Records(ChainError),
    /// CR-42, CR-81: the stored records could not be read, or one does not
    /// parse. The text says which.
    RecordsUnread(String),
}

impl LoadError {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        "CR-42"
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Batches(e) => write!(f, "the stored address batches do not read back: {e}"),
            Self::Exposure(e) => write!(f, "the stored exposure ledger does not read back: {e}"),
            Self::Pause(e) => write!(f, "the stored pause state does not read back: {e}"),
            Self::Desk(e) => write!(f, "the stored counter state does not read back: {e}"),
            Self::Records(e) => write!(f, "the stored records do not verify: {e}"),
            Self::RecordsUnread(e) => write!(f, "the stored records cannot be read: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Why a refund QR code was not taken as a destination (CR-62, CR-63).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationRefused {
    /// CR-62: the text is longer than the counter reads.
    TooLong,
    /// CR-62: the text holds a character that is not printable ASCII.
    NotPrintable,
    /// CR-62: no address could be read from it.
    NotAnAddress,
    /// CR-63: the address is for another network.
    WrongNetwork,
    /// CR-63: the address is of a kind the Key app's signer would not pay.
    ScriptKind,
}

impl DestinationRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::TooLong | Self::NotPrintable | Self::NotAnAddress => "CR-62",
            Self::WrongNetwork | Self::ScriptKind => "CR-63",
        }
    }
}

impl fmt::Display for DestinationRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::TooLong => write!(f, "the code is too long to be an address"),
            Self::NotPrintable => write!(
                f,
                "the code holds a character that an address does not carry"
            ),
            Self::NotAnAddress => write!(f, "no on-chain address could be read from the code"),
            Self::WrongNetwork => write!(f, "the address is for another network"),
            Self::ScriptKind => write!(
                f,
                "the address is not a P2WPKH, P2WSH, P2TR, P2PKH or P2SH address"
            ),
        }
    }
}

impl std::error::Error for DestinationRefused {}

/// Why a refund was not requested or not paid in cash (CR-61 to CR-69,
/// CR-71, CR-73).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefundRefused<E> {
    /// CR-61: a name or a reason is not a label of 1 to 64 characters with
    /// no control character.
    BadText(&'static str),
    /// CR-63: the destination script is of a kind the signer would not pay.
    DestinationKind,
    /// CR-64: the counter has no sale with this index.
    UnknownSale(u32),
    /// CR-58: the rate that turns an amount into satoshis is stale.
    StaleRate {
        /// When the caller got the rate.
        fetched_at: u64,
        /// The time of the request.
        now: u64,
    },
    /// CR-67: the amount is zero.
    ZeroAmount,
    /// CR-67: the amount is more than all the bitcoin there is.
    AmountTooLarge,
    /// CR-67: the amount is below the dust limit of the destination.
    BelowDust {
        /// The amount asked for.
        sats: u64,
        /// The dust limit of the destination's kind.
        dust: u64,
    },
    /// CR-68: the destination is one of the wallet's own scripts.
    OwnScript,
    /// CR-65: nothing has arrived for the sale.
    NothingReceived,
    /// CR-67: the sale keeps the most refund entries it can.
    TooManyRefunds,
    /// CR-67: the sale's refunds would add up to more than was received.
    MoreThanReceived {
        /// What was received for the sale, in satoshis.
        received: u64,
        /// What the earlier requests and cash refunds ask, in satoshis.
        asked: u64,
        /// What this one asks, in satoshis.
        sats: u64,
    },
    /// CR-119: a cashier's cash refund would pass what was received only if
    /// a withdrawn request is counted, and the owner may still pay that
    /// request.
    WithdrawnMayBePaid {
        /// What was received for the sale, in satoshis.
        received: u64,
        /// What the earlier requests (withdrawn ones too) and cash refunds
        /// ask, in satoshis.
        asked: u64,
        /// What this one asks, in satoshis.
        sats: u64,
    },
    /// CR-69: a cashier asks for more than the largest single refund; a
    /// manager is needed.
    NeedsManagerSingle {
        /// What is asked, in satoshis.
        sats: u64,
        /// The largest single refund.
        limit: u64,
    },
    /// CR-69: the cashier's refunds of the last 24 hours would pass the
    /// daily cap; a manager is needed.
    NeedsManagerDaily {
        /// What is asked, in satoshis.
        sats: u64,
        /// The cashier's refunds inside the window already.
        already: u64,
        /// The cap.
        cap: u64,
    },
    /// CR-40: a write failed.
    Store(StoreFault<E>),
}

impl<E> RefundRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::BadText(_) => "CR-61",
            Self::DestinationKind => "CR-63",
            Self::UnknownSale(_) => "CR-64",
            Self::StaleRate { .. } => "CR-58",
            Self::ZeroAmount
            | Self::AmountTooLarge
            | Self::BelowDust { .. }
            | Self::TooManyRefunds
            | Self::MoreThanReceived { .. } => "CR-67",
            Self::OwnScript => "CR-68",
            Self::WithdrawnMayBePaid { .. } => "CR-119",
            Self::NothingReceived => "CR-65",
            Self::NeedsManagerSingle { .. } | Self::NeedsManagerDaily { .. } => "CR-69",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for RefundRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::BadText(what) => write!(
                f,
                "the {what} is not a label of 1 to 64 characters with no control character"
            ),
            Self::DestinationKind => write!(
                f,
                "the destination is not a P2WPKH, P2WSH, P2TR, P2PKH or P2SH script"
            ),
            Self::UnknownSale(i) => write!(f, "there is no sale with index {i}"),
            Self::StaleRate { fetched_at, now } => write!(
                f,
                "the rate was fetched at {fetched_at} and the time is {now}; fetch a rate again"
            ),
            Self::ZeroAmount => write!(f, "a refund needs an amount"),
            Self::AmountTooLarge => write!(f, "the amount is more than all the bitcoin there is"),
            Self::BelowDust { sats, dust } => write!(
                f,
                "{sats} sats is below the {dust} sats the destination can take"
            ),
            Self::OwnScript => write!(
                f,
                "the destination is one of this shop's own addresses; ask the customer for \
                 an address of their own wallet"
            ),
            Self::NothingReceived => write!(f, "nothing has arrived for this sale"),
            Self::TooManyRefunds => write!(f, "the sale has the most refund entries it can keep"),
            Self::MoreThanReceived {
                received,
                asked,
                sats,
            } => write!(
                f,
                "{sats} sats on top of {asked} sats already asked is more than the {received} \
                 sats received"
            ),
            Self::WithdrawnMayBePaid {
                received,
                asked,
                sats,
            } => write!(
                f,
                "{sats} sats on top of {asked} sats asked, withdrawn requests included, is more \
                 than the {received} sats received; the owner may still pay a withdrawn \
                 request, so a cashier cannot give this in cash"
            ),
            Self::NeedsManagerSingle { sats, limit } => write!(
                f,
                "{sats} sats is above the {limit} sats a cashier may refund; a manager is needed"
            ),
            Self::NeedsManagerDaily { sats, already, cap } => write!(
                f,
                "{sats} sats on top of {already} sats in the last 24 hours is above the \
                 cashier's cap of {cap} sats; a manager is needed"
            ),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for RefundRefused<E> {}

/// Why a refund request was not withdrawn (CR-61, CR-64, CR-70).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WithdrawRefused<E> {
    /// CR-61: the person's name is not a label.
    BadName,
    /// CR-64: the counter has no sale with this index.
    UnknownSale(u32),
    /// CR-70: the sale has no request with this number.
    UnknownRequest(u64),
    /// CR-70: the request was withdrawn already.
    AlreadyWithdrawn(u64),
    /// CR-40: a write failed.
    Store(StoreFault<E>),
}

impl<E> WithdrawRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::BadName => "CR-61",
            Self::UnknownSale(_) => "CR-64",
            Self::UnknownRequest(_) | Self::AlreadyWithdrawn(_) => "CR-70",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for WithdrawRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::BadName => write!(
                f,
                "the name is not a label of 1 to 64 characters with no control character"
            ),
            Self::UnknownSale(i) => write!(f, "there is no sale with index {i}"),
            Self::UnknownRequest(n) => write!(f, "the sale has no refund request {n}"),
            Self::AlreadyWithdrawn(n) => write!(f, "refund request {n} was withdrawn already"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for WithdrawRefused<E> {}

/// Why a change to the cashiers was not written down (CR-143).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaffRefused<E> {
    /// CR-143: a name is not a label.
    BadName(&'static str),
    /// CR-40: the record could not be written.
    Store(StoreFault<E>),
}

impl<E> StaffRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::BadName(_) => "CR-143",
            Self::Store(_) => "CR-40",
        }
    }
}

impl<E: fmt::Display> fmt::Display for StaffRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::BadName(which) => write!(
                f,
                "the {which} is not a label of 1 to 64 characters with no control character"
            ),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for StaffRefused<E> {}

/// Why the day's totals were not counted (CR-152, CR-153).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TotalsRefused<E> {
    /// CR-152: the day ends before it starts, or is empty.
    BadWindow,
    /// CR-153: the sales of the day are in two currencies.
    MixedCurrencies,
    /// CR-153: a sum does not fit.
    Overflow,
    /// CR-152: the day needs more records than one count reads.
    TooManyRecords,
    /// CR-81: the records could not be read back.
    Store(StoreFault<E>),
}

impl<E> TotalsRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::BadWindow | Self::TooManyRecords => "CR-152",
            Self::MixedCurrencies | Self::Overflow => "CR-153",
            Self::Store(_) => "CR-81",
        }
    }
}

impl<E> From<crate::totals::TotalsError> for TotalsRefused<E> {
    fn from(e: crate::totals::TotalsError) -> Self {
        match e {
            crate::totals::TotalsError::BadWindow => Self::BadWindow,
            crate::totals::TotalsError::MixedCurrencies => Self::MixedCurrencies,
            crate::totals::TotalsError::Overflow => Self::Overflow,
            crate::totals::TotalsError::TooManyRecords => Self::TooManyRecords,
        }
    }
}

impl<E: fmt::Display> fmt::Display for TotalsRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::BadWindow => write!(f, "the day ends before it starts"),
            Self::MixedCurrencies => write!(f, "the sales of the day are in two currencies"),
            Self::Overflow => write!(f, "a sum of the day does not fit"),
            Self::TooManyRecords => write!(f, "the day needs more records than one count reads"),
            Self::Store(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for TotalsRefused<E> {}

/// Why a batch of addresses was not taken (CR-167, CR-168).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchTakeRefused<E> {
    /// The batch failed a check, or the batches could not be stored. The
    /// batches are as they were; a record may have been written first, which
    /// a second try of the same batch does not repeat (CR-168).
    Add(AddError<E>),
    /// CR-40: the record that says the batch is held could not be written.
    /// Nothing changed.
    Record(StoreFault<E>),
}

impl<E> BatchTakeRefused<E> {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::Add(_) => "CR-168",
            Self::Record(_) => "CR-167",
        }
    }
}

impl<E: fmt::Display> fmt::Display for BatchTakeRefused<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Add(e) => write!(f, "{e}"),
            Self::Record(e) => write!(f, "{e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for BatchTakeRefused<E> {}
