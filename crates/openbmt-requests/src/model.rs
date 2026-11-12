//! What a request is, what has arrived for it and what it is worth (RQ-05,
//! RQ-07, RQ-12, RQ-15 to RQ-20, RQ-26).

use bitcoin::ScriptBuf;
use openbmt_core::currency::ShopCurrency;
use openbmt_counter::money::{sats_for, Amount, Rate, RateSource};
use openbmt_counter::DUST_SATS;

use crate::date::{Day, DAY_SECS};

/// RQ-12: how long a quote in the currency holds, in seconds.
pub const QUOTE_HOLD_SECS: u64 = DAY_SECS;
/// RQ-12: most quotes kept for one request; the oldest goes first.
pub const MAX_QUOTES: usize = 64;
/// RQ-11: most requests open at once.
pub const MAX_OPEN: usize = 1_000;
/// RQ-06: most characters of a note.
pub const MAX_NOTE_CHARS: usize = 64;
/// RQ-06: whether a note is plain text of 1 to 64 characters with no control
/// character (the check of CR-206).
pub fn note_ok(note: &str) -> bool {
    (1..=MAX_NOTE_CHARS).contains(&note.chars().count()) && !note.chars().any(char::is_control)
}

/// RQ-08: most units of the shop's currency a request may ask for.
pub const MAX_CURRENCY_UNITS: u64 = 1_000_000;
/// RQ-09: most days from the day a request is made to its due date.
pub const MAX_DUE_DAYS: u32 = 366;
/// RQ-21: a request due within this many days is "Due soon".
pub const DUE_SOON_DAYS: u32 = 3;
/// RQ-25: most days of lead a repeat has.
pub const MAX_LEAD_DAYS: u32 = 31;
/// RQ-25: most requests a repeat makes at one time.
pub const MAX_REPEATS_AT_A_TIME: usize = 12;
/// RQ-15: most payments one request keeps. (A limit of this library, for the
/// size of the book: the rules name none.)
pub const MAX_CREDITS: usize = 1_000;

/// How a request is priced (RQ-07). Never changes after the request is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pricing {
    /// In satoshis: the amount is what is owed whatever the price does.
    Bitcoin,
    /// In the shop's currency: the satoshis are worked out at each share.
    /// Never [`ShopCurrency::Sats`], which has no price.
    Currency(ShopCurrency),
}

/// How often a request repeats (RQ-24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Every {
    /// Every 7 days.
    Week,
    /// On the same day of each month, or the last day of a shorter one.
    Month,
    /// On the same date of each year, or 28 February for 29 February.
    Year,
}

impl Every {
    /// The due date of the `index`th request of a series that began on
    /// `first_due` (index 0 is the first). Counted from the first, so a
    /// short month does not move the days after it.
    pub fn due(self, first_due: Day, index: u32) -> Option<Day> {
        match self {
            Self::Week => first_due.plus_days(index.checked_mul(7)?),
            Self::Month => first_due.plus_months(index),
            Self::Year => first_due.plus_years(index),
        }
    }
}

/// The series a repeating request belongs to (RQ-24, RQ-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repeat {
    /// How often.
    pub every: Every,
    /// The number of the first request of the series.
    pub series: u64,
    /// Which request of the series this is; 0 for the first.
    pub index: u32,
    /// The due date of the first request.
    pub first_due: Day,
    /// Days before a due date at which the next request is made: the first
    /// request's due date less the day it was made, at most 31 (RQ-25).
    pub lead: u8,
}

/// How a request ended (RQ-16, RQ-19, RQ-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    /// Cancelled with nothing received.
    Cancelled,
    /// Closed as it stood while part paid.
    Closed,
    /// An almost paid request the owner counted as paid.
    CountedPaid,
}

/// A quote of what a request asks for now (RQ-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote {
    /// When it was made, in Unix seconds.
    pub made_at: u64,
    /// What was still owed then, in the request's unit.
    pub owed: u64,
    /// The satoshis asked for.
    pub sats: u64,
    /// The rate, for a request in the currency.
    pub rate: Option<Rate>,
    /// The first second it no longer holds; none for a request in bitcoin.
    pub until: Option<u64>,
}

impl Quote {
    /// Whether the quote still holds at `time` (always, for bitcoin).
    pub fn holds_at(&self, time: u64) -> bool {
        self.until.is_none_or(|until| time < until)
    }
}

/// One output paid to a request's address in a checked block (RQ-15), as the
/// owner app's watch finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrival {
    /// The id of the transaction.
    pub txid: [u8; 32],
    /// Which output of it.
    pub vout: u32,
    /// The height of the block.
    pub height: u32,
    /// The block's time, in Unix seconds.
    pub block_time: u64,
    /// The satoshis.
    pub sats: u64,
}

/// A payment to a request and what it was credited (RQ-15 to RQ-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credit {
    /// The id of the transaction.
    pub txid: [u8; 32],
    /// Which output of it.
    pub vout: u32,
    /// The height of the block.
    pub height: u32,
    /// The block's time, in Unix seconds.
    pub block_time: u64,
    /// The satoshis.
    pub sats: u64,
    /// What it was credited, in the request's unit; 0 for a payment after
    /// the request ended.
    pub value: u64,
    /// The rate it was credited at, for a request in the currency.
    pub rate: Option<Rate>,
    /// RQ-17: the block is dated after the end of the due day.
    pub late: bool,
    /// RQ-18: credited at a quote that had run out, or at none.
    pub old_price: bool,
    /// RQ-19, RQ-20: how the request had ended when it was counted.
    pub after: Option<After>,
}

/// A payment that came after the request ended (RQ-19, RQ-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    /// After it was cancelled.
    Cancel,
    /// After it was closed (or counted as paid).
    Close,
}

/// A request (RQ-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// "No. 12": counts up from 1, never reused.
    pub number: u64,
    /// RQ-06: 1 to 64 characters, no control character. Stays on the phone.
    pub note: String,
    /// The amount owed: satoshis, or minor units of the currency.
    pub amount: u64,
    /// How it is priced.
    pub pricing: Pricing,
    /// When it was made, in Unix seconds.
    pub made_at: u64,
    /// The day it was made, in the shop's time zone.
    pub made: Day,
    /// The due date, in the shop's time zone.
    pub due: Day,
    /// The index of its address on the desk (RQ-03).
    pub address_index: u32,
    /// The series, if it repeats.
    pub repeat: Option<Repeat>,
    /// How it ended, and when.
    pub end: Option<(End, u64)>,
    /// What was asked at each share, oldest first.
    pub quotes: Vec<Quote>,
    /// What arrived, in the order of the blocks.
    pub credits: Vec<Credit>,
}

/// What a request is, from what has arrived (RQ-16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Nothing counted yet.
    Waiting,
    /// Something arrived and something is still owed.
    PartPaid {
        /// What is still owed, in the request's unit.
        owed: u64,
    },
    /// Nothing is owed.
    Paid,
    /// More arrived than was owed.
    Overpaid {
        /// The excess, in the request's unit.
        excess: u64,
    },
    /// In the currency: a shortfall under the smallest bitcoin sale. The
    /// owner may count it as paid.
    Almost {
        /// The shortfall, in the currency.
        short: u64,
    },
}

/// The marks under a request's state (RQ-17 to RQ-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Marks {
    /// "Paid late".
    pub late: bool,
    /// "On an old price".
    pub old_price: bool,
    /// "Paid after it was cancelled".
    pub after_cancel: bool,
    /// "Paid after it was closed".
    pub after_close: bool,
}

impl Request {
    /// The smallest number of satoshis a request may ask for: the shop's
    /// smallest bitcoin sale, and never below the dust line (RQ-08, CR-75,
    /// CR-17). `min_sale_sats` is the setting; 0 is no floor.
    pub fn floor_sats(min_sale_sats: u64) -> u64 {
        min_sale_sats.max(DUST_SATS)
    }

    /// What the credits count for: all but the payments after the end.
    pub fn credited(&self) -> u64 {
        self.credits
            .iter()
            .filter(|c| c.after.is_none())
            .fold(0u64, |t, c| t.saturating_add(c.value))
    }

    /// RQ-15: what is still owed, the amount less every credit, never below
    /// zero. Nothing is owed on a request that ended.
    pub fn owed(&self) -> u64 {
        if self.end.is_some() {
            return 0;
        }
        self.amount.saturating_sub(self.credited())
    }

    /// Satoshis that arrived, whatever happened to the request (RQ-26).
    pub fn received_sats(&self) -> u64 {
        self.credits
            .iter()
            .fold(0u64, |t, c| t.saturating_add(c.sats))
    }

    /// The rate the request's newest credit was valued at.
    fn last_rate(&self) -> Option<Rate> {
        self.credits.iter().rev().find_map(|c| c.rate)
    }

    /// RQ-16: the state, for a shop whose smallest bitcoin sale is
    /// `min_sale_sats` (0 for no floor).
    pub fn status(&self, min_sale_sats: u64) -> Status {
        if self.credits.iter().all(|c| c.after.is_some()) {
            return Status::Waiting;
        }
        let credited = self.credited();
        let floor = Self::floor_sats(min_sale_sats);
        // A part of the amount in the currency is under the smallest sale
        // when it is worth fewer satoshis than that at the rate the last
        // payment was credited at.
        let under_floor = |units: u64, up: bool| -> bool {
            let Some(rate) = self.last_rate() else {
                return false;
            };
            let sats = if up {
                sats_for(Amount(units), rate)
            } else {
                Some(sats_floor(units, rate))
            };
            sats.is_some_and(|s| s < floor)
        };
        let currency = matches!(self.pricing, Pricing::Currency(_));
        if credited < self.amount {
            let owed = self.amount - credited;
            if matches!(self.end, Some((End::CountedPaid, _))) {
                return Status::Paid;
            }
            if currency && under_floor(owed, true) {
                return Status::Almost { short: owed };
            }
            return Status::PartPaid { owed };
        }
        let excess = credited - self.amount;
        if excess == 0 || (currency && under_floor(excess, false)) {
            Status::Paid
        } else {
            Status::Overpaid { excess }
        }
    }

    /// RQ-17 to RQ-20: the marks.
    pub fn marks(&self) -> Marks {
        let mut marks = Marks::default();
        for c in &self.credits {
            marks.late |= c.late;
            marks.old_price |= c.old_price;
            marks.after_cancel |= c.after == Some(After::Cancel);
            marks.after_close |= c.after == Some(After::Close);
        }
        marks
    }

    /// Whether the request is open (RQ-11): not ended and something is
    /// still owed.
    pub fn is_open(&self) -> bool {
        self.end.is_none() && self.owed() > 0
    }

    /// RQ-25: made by a repeat and not shared once yet.
    pub fn is_ready_to_send(&self) -> bool {
        self.repeat.is_some_and(|r| r.index > 0) && self.quotes.is_empty()
    }

    /// The newest quote.
    pub fn latest_quote(&self) -> Option<&Quote> {
        self.quotes.last()
    }
}

/// Satoshis for an amount in the currency at a rate, rounded down (CR-06,
/// CR-61): what some money is worth, not what a price needs.
pub fn sats_floor(units: u64, rate: Rate) -> u64 {
    let n = u128::from(units).saturating_mul(100_000_000);
    u64::try_from(n / u128::from(rate.minor_per_btc())).unwrap_or(u64::MAX)
}

/// A rate with where it came from: the checked value the caller passes
/// when a request in the currency is made or shared (RQ-12, CR-57, CR-58).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedRate {
    /// Minor units of the currency per bitcoin.
    pub rate: Rate,
    /// Where the rate came from and when.
    pub source: RateSource,
}

impl CheckedRate {
    /// The rate, if it may be used at `now` (CR-58).
    pub fn fresh_at(&self, now: u64) -> Option<Rate> {
        self.source.is_fresh_at(now).then_some(self.rate)
    }
}

/// Where a return stands (RQ-29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnState {
    /// Waiting with the other returns.
    Queued,
    /// In a spend that was built and not yet confirmed.
    Sent {
        /// The transaction.
        txid: [u8; 32],
    },
    /// The spend that pays it is confirmed.
    Done {
        /// The transaction.
        txid: [u8; 32],
        /// When it was confirmed, in Unix seconds.
        at: u64,
    },
}

/// Money given back from a request (RQ-26 to RQ-29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Return {
    /// Counts up from 1 in the order returns are queued.
    pub id: u64,
    /// The number of the request it is returned from.
    pub request: u64,
    /// Where it goes: the script of the customer's address.
    pub script: ScriptBuf,
    /// The satoshis.
    pub sats: u64,
    /// When it was queued, in Unix seconds.
    pub queued_at: u64,
    /// Where it stands.
    pub state: ReturnState,
}
