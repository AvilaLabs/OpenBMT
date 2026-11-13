//! A sale from the till, the counter's side (CR-266 to CR-270).
//!
//! A till that works with the Box asks one counter to take a payment of a
//! set amount. The Box hands the counter a request; this module holds the
//! requests that wait for the cashier, in memory only, and says which of
//! them the home screen shows. Starting one is [`Counter::till_start`]: it
//! opens an ordinary sale and writes the record that names the till's sale.
//!
//! The list is a plain value with no clock of its own: every call takes the
//! time in seconds. A request lapses at the time the Box gave it.
//!
//! [`Counter::till_start`]: crate::Counter::till_start

use crate::bills::{reference_ok, MAX_BILL_AMOUNT};
use crate::money::{Amount, Currency};

/// CR-266: most requests that wait at once.
pub const MAX_TILL_WAITING: usize = 8;
/// CR-266: longest reference a request carries, in bytes (BX-76).
pub const MAX_TILL_REFERENCE: usize = 40;

/// What the Box asked for (BX-76), as the counter holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TillRequest {
    /// The till's sale, 16 bytes the Box made.
    pub sale: [u8; 16],
    /// The amount in minor units of the shop's currency.
    pub amount: Amount,
    /// The currency the Box named.
    pub currency: Currency,
    /// The till's own words for the sale: printable ASCII.
    pub reference: String,
    /// When the request lapses, in seconds, on the Box's clock.
    pub lapses_at: u64,
}

impl TillRequest {
    /// CR-268: the reference as the sale's till reference (CR-245), when it
    /// fits that rule. A reference that does not fit is only shown.
    pub fn sale_reference(&self) -> Option<&str> {
        reference_ok(&self.reference).then_some(self.reference.as_str())
    }

    /// CR-267: whole minutes left at `now`, counted up, so that a request
    /// with seconds left never reads 0. Zero once it has lapsed.
    pub fn minutes_left(&self, now: u64) -> u64 {
        self.lapses_at.saturating_sub(now).div_ceil(60)
    }
}

/// Why a request was dropped when it came (CR-266). Nothing is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TillDropped {
    /// The currency is not the shop's.
    OtherCurrency,
    /// The amount is nothing, or more than the largest amount.
    BadAmount,
    /// The reference is not printable ASCII within its bound.
    BadReference,
    /// The lapse time is past.
    Lapsed,
    /// 8 requests wait already.
    Full,
    /// The counter holds this sale already, waiting or started.
    Known,
}

impl TillDropped {
    /// The rule the drop belongs to.
    pub fn rule(self) -> &'static str {
        "CR-266"
    }
}

impl std::fmt::Display for TillDropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: a sale request from the till was dropped: ",
            self.rule()
        )?;
        f.write_str(match self {
            Self::OtherCurrency => "its currency is not the shop's",
            Self::BadAmount => "its amount is nothing or too large",
            Self::BadReference => "its reference is not printable ASCII of at most 40 bytes",
            Self::Lapsed => "it had lapsed",
            Self::Full => "8 requests wait already",
            Self::Known => "the counter has this sale already",
        })
    }
}

/// CR-269: what a cancel from the till did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TillCancelled {
    /// A request not started was removed.
    Removed,
    /// A sale already started is marked as cancelled by the till; the
    /// number is the sale's receive index. The sale goes on.
    Marked(u32),
    /// The counter knows no such sale.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Waiting {
    request: TillRequest,
    /// Not now was pressed: the request is not shown.
    hidden: bool,
}

/// CR-266, CR-267: the requests that wait, oldest first. In memory only
/// (CR-270).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TillWaiting {
    list: Vec<Waiting>,
}

impl TillWaiting {
    /// An empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// CR-266: take a request. `shop` is the shop's currency. `started`
    /// says whether the counter already started a sale for this till's sale.
    /// The request goes last, so the list stays oldest first. Requests that
    /// lapsed are dropped from the list first, so that a lapsed one does not
    /// keep the room.
    pub fn take(
        &mut self,
        request: TillRequest,
        shop: Currency,
        started: bool,
        now: u64,
    ) -> Result<(), TillDropped> {
        self.prune(now);
        if request.currency != shop {
            return Err(TillDropped::OtherCurrency);
        }
        if request.amount.0 == 0 || request.amount.0 > MAX_BILL_AMOUNT {
            return Err(TillDropped::BadAmount);
        }
        if request.reference.len() > MAX_TILL_REFERENCE
            || !request.reference.bytes().all(|b| (0x20..0x7f).contains(&b))
        {
            return Err(TillDropped::BadReference);
        }
        if request.lapses_at <= now {
            return Err(TillDropped::Lapsed);
        }
        if started || self.list.iter().any(|w| w.request.sale == request.sale) {
            return Err(TillDropped::Known);
        }
        if self.list.len() >= MAX_TILL_WAITING {
            return Err(TillDropped::Full);
        }
        self.list.push(Waiting {
            request,
            hidden: false,
        });
        Ok(())
    }

    /// Drop the requests that lapsed by `now`.
    pub fn prune(&mut self, now: u64) {
        self.list.retain(|w| w.request.lapses_at > now);
    }

    /// CR-267: the requests the home screen may show at `now`, oldest
    /// first: not lapsed and not hidden by Not now. The first is the one
    /// shown.
    pub fn shown(&self, now: u64) -> Vec<&TillRequest> {
        self.list
            .iter()
            .filter(|w| !w.hidden && w.request.lapses_at > now)
            .map(|w| &w.request)
            .collect()
    }

    /// The request for this till's sale, when it waits and has not lapsed.
    pub fn get(&self, sale: &[u8; 16], now: u64) -> Option<&TillRequest> {
        self.list
            .iter()
            .find(|w| &w.request.sale == sale && w.request.lapses_at > now)
            .map(|w| &w.request)
    }

    /// CR-267: Not now. The request is hidden and nothing is sent; it
    /// lapses at the Box as BX-76 says. `false` when no such request waits.
    pub fn not_now(&mut self, sale: &[u8; 16]) -> bool {
        match self.list.iter_mut().find(|w| &w.request.sale == sale) {
            Some(w) => {
                w.hidden = true;
                true
            }
            None => false,
        }
    }

    /// CR-269: remove a request that was not started. `false` when none
    /// waits for this sale.
    pub fn remove(&mut self, sale: &[u8; 16]) -> bool {
        let before = self.list.len();
        self.list.retain(|w| &w.request.sale != sale);
        self.list.len() != before
    }

    /// How many requests are held, shown or not.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether no request is held.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}
