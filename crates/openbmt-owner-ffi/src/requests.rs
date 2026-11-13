//! The ops of the owner's requests (`docs/REQUEST-RULES.md`, RQ-01 to
//! RQ-37, ops 216 to 226): make a request, list them, share one, end one,
//! add a return, make the repeats that are due, and the state Home shows.
//!
//! This crate holds no rule of its own. Every call is given the facts the
//! node does not have: the time, the shop's offset from UTC and the newest
//! rate the app checked ([`RequestAsk`]). The node holds the book
//! (`openbmt_owner_node::requests`), and the words are those of
//! `openbmt_requests::say`; a text that library does not have is a
//! placeholder that names its rule.

use bitcoin::hashes::Hash;
use bitcoin::{Address, Network, Txid};
use openbmt_books::format_minor;
use openbmt_core::currency::ShopCurrency;
use openbmt_counter::{Currency, Rate, RateSource};
use openbmt_owner_node::{
    MadeRequest, OwnerNode, OwnerPort, RequestContext, RequestError, RequestsOff,
};
use openbmt_requests::{
    day_of, say, CheckedRate, Day, DueRepeat, Every, NewRequest, PriceIn, Pricing, Request,
    RequestBook, ReturnAmount, ReturnState, Row,
};
use openbmt_requests::{After, End, Status};

use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::error::{from_words, rule_in, AppError, Kind};
use crate::views::WordsView;

/// A rate the app checked: minor units of the shop's currency to a bitcoin,
/// where it came from and when (CR-57, CR-58).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestRateView {
    /// Minor units of the currency per bitcoin; never zero.
    pub per_btc: u64,
    /// The label of the source (CR-57).
    pub source: String,
    /// When the app got the rate, in Unix seconds.
    pub fetched_at: u64,
}

wire_record!(RequestRateView {
    per_btc,
    source,
    fetched_at
});
wire_record_get!(RequestRateView {
    per_btc,
    source,
    fetched_at
});

/// What every request op is given besides its own arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestAsk {
    /// Now, in Unix seconds.
    pub now: u64,
    /// The shop's offset from UTC in minutes east (RQ-09).
    pub offset_minutes: i32,
    /// The newest rate the app has checked, if any (RQ-12).
    pub rate: Option<RequestRateView>,
}

wire_record!(RequestAsk {
    now,
    offset_minutes,
    rate
});
wire_record_get!(RequestAsk {
    now,
    offset_minutes,
    rate
});

/// A day of the shop's calendar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayView {
    /// The year.
    pub year: u32,
    /// The month, 1 to 12.
    pub month: u32,
    /// The day of the month, from 1.
    pub day: u32,
}

wire_record!(DayView { year, month, day });
wire_record_get!(DayView { year, month, day });

impl DayView {
    fn of(day: Day) -> Self {
        let (year, month, day) = day.civil();
        Self {
            year: u32::try_from(year).unwrap_or(0),
            month,
            day,
        }
    }

    fn to_day(&self) -> Result<Day, AppError> {
        Day::from_civil(i64::from(self.year), self.month, self.day).ok_or_else(|| {
            from_words(
                Kind::Refused,
                "RQ-09",
                format!(
                    "RQ-09: {}-{}-{} is not a day the shop's calendar has",
                    self.year, self.month, self.day
                ),
                openbmt_core::say::Words::new(
                    openbmt_core::say::Tone::Ended,
                    "That day does not exist",
                    "The due date is not a day of the shop's calendar.",
                    openbmt_core::say::Who::Owner,
                    "Pick the due date again.",
                ),
            )
        })
    }
}

/// How the owner prices a request (RQ-07).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestPriceIn {
    /// In satoshis.
    Bitcoin,
    /// In the shop's currency.
    ShopCurrency,
}

wire_unit_enum!(RequestPriceIn {
    Bitcoin = 0,
    ShopCurrency = 1
});

/// How often a request repeats (RQ-24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestEvery {
    /// Every 7 days.
    Week,
    /// On the same day of each month.
    Month,
    /// On the same date of each year.
    Year,
}

wire_unit_enum!(RequestEvery {
    Week = 0,
    Month = 1,
    Year = 2
});

impl RequestEvery {
    fn every(self) -> Every {
        match self {
            Self::Week => Every::Week,
            Self::Month => Every::Month,
            Self::Year => Every::Year,
        }
    }
}

/// The arguments of `request_make` (RQ-05 to RQ-09, RQ-24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMakeArgs {
    /// The time, the offset and the rate.
    pub ask: RequestAsk,
    /// The note (RQ-06).
    pub note: String,
    /// Satoshis, or minor units of the shop's currency.
    pub amount: u64,
    /// How it is priced.
    pub price_in: RequestPriceIn,
    /// The due date.
    pub due: DayView,
    /// How often it repeats, if it does.
    pub repeat: Option<RequestEvery>,
}

wire_record!(RequestMakeArgs {
    ask,
    note,
    amount,
    price_in,
    due,
    repeat
});
wire_record_get!(RequestMakeArgs {
    ask,
    note,
    amount,
    price_in,
    due,
    repeat
});

/// A request that was made (RQ-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestMadeView {
    /// Its number.
    pub number: u64,
    /// The index of its address on the desk.
    pub index: u32,
    /// Its address.
    pub address: String,
}

wire_record!(RequestMadeView {
    number,
    index,
    address
});

impl From<MadeRequest> for RequestMadeView {
    fn from(m: MadeRequest) -> Self {
        Self {
            number: m.number,
            index: m.index,
            address: m.address,
        }
    }
}

/// The arguments of the ops that take only a request's number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestNumberArgs {
    /// The time, the offset and the rate.
    pub ask: RequestAsk,
    /// The request.
    pub number: u64,
}

wire_record!(RequestNumberArgs { ask, number });
wire_record_get!(RequestNumberArgs { ask, number });

/// The arguments of `request_share` (RQ-12, RQ-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestShareArgs {
    /// The time, the offset and the rate.
    pub ask: RequestAsk,
    /// The request.
    pub number: u64,
    /// The shop's name, for the line above the link and the link's label.
    pub shop_name: String,
}

wire_record!(RequestShareArgs {
    ask,
    number,
    shop_name
});
wire_record_get!(RequestShareArgs {
    ask,
    number,
    shop_name
});

/// What `request_share` gives (RQ-12, RQ-13, RQ-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestSharedView {
    /// The request.
    pub number: u64,
    /// What was still owed when it was quoted, in the request's unit.
    pub owed: u64,
    /// The satoshis the quote asks for.
    pub sats: u64,
    /// The rate it was made at, minor units per bitcoin, for a request in
    /// the currency.
    pub rate: Option<u64>,
    /// When the quote runs out, in Unix seconds, for a request in the
    /// currency.
    pub until: Option<u64>,
    /// The address.
    pub address: String,
    /// The BIP 321 link.
    pub link: String,
    /// The line above the link; the overdue one when the due day has passed.
    pub line: String,
}

wire_record!(RequestSharedView {
    number,
    owed,
    sats,
    rate,
    until,
    address,
    link,
    line
});

/// A row of the list (RQ-21, RQ-35).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestRowView {
    /// The number.
    pub number: u64,
    /// The note.
    pub note: String,
    /// What is still owed, in the request's unit.
    pub owed: u64,
    /// The due date.
    pub due: DayView,
    /// The second line of the row.
    pub line: String,
    /// The big line of the request's page: what it stands at.
    pub state: String,
    /// The small lines under it, one for each mark.
    pub marks: Vec<String>,
    /// It repeats.
    pub repeats: bool,
}

wire_record!(RequestRowView {
    number,
    note,
    owed,
    due,
    line,
    state,
    marks,
    repeats
});

/// What `request_list` gives (RQ-21, RQ-22, RQ-25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestListView {
    /// Made by a repeat and not shared once yet.
    pub ready_to_send: Vec<RequestRowView>,
    /// Past the due day and not paid.
    pub overdue: Vec<RequestRowView>,
    /// Due within 3 days.
    pub due_soon: Vec<RequestRowView>,
    /// The rest.
    pub waiting: Vec<RequestRowView>,
    /// The requests that ended, newest number first.
    pub finished: Vec<RequestRowView>,
    /// Home's row when some are overdue.
    pub overdue_row: Option<String>,
    /// The heading of the empty list (RQ-35).
    pub empty_heading: String,
    /// The text of the empty list (RQ-35).
    pub empty_what: String,
}

wire_record!(RequestListView {
    ready_to_send,
    overdue,
    due_soon,
    waiting,
    finished,
    overdue_row,
    empty_heading,
    empty_what
});

/// The arguments of `request_add_return` and `request_return_ask`
/// (RQ-26 to RQ-29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestReturnArgs {
    /// The time, the offset and the rate.
    pub ask: RequestAsk,
    /// The request the money is returned from.
    pub number: u64,
    /// `amount` is minor units of the shop's currency, not satoshis.
    pub in_currency: bool,
    /// The amount.
    pub amount: u64,
    /// The address the customer sent, as pasted or scanned (RQ-28). Not
    /// read by `request_return_ask`.
    pub address: String,
    /// The shop's name, for the line that asks a customer for an address.
    /// Not read by `request_add_return`.
    pub shop_name: String,
}

wire_record!(RequestReturnArgs {
    ask,
    number,
    in_currency,
    amount,
    address,
    shop_name
});
wire_record_get!(RequestReturnArgs {
    ask,
    number,
    in_currency,
    amount,
    address,
    shop_name
});

impl RequestReturnArgs {
    fn amount(&self) -> ReturnAmount {
        if self.in_currency {
            ReturnAmount::Units(self.amount)
        } else {
            ReturnAmount::Sats(self.amount)
        }
    }
}

/// A return that was queued (RQ-29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestReturnAddedView {
    /// The return's number in the book.
    pub id: u64,
    /// The refund's number in the owner's queue.
    pub refund: u32,
    /// The satoshis.
    pub sats: u64,
    /// The address in groups of four, to read aloud (RQ-33).
    pub groups: String,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(RequestReturnAddedView {
    id,
    refund,
    sats,
    groups,
    words
});

/// What `request_repeats` made (RQ-25).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestRepeatsView {
    /// The requests the due repeats made, ready to send.
    pub made: Vec<RequestMadeView>,
}

wire_record!(RequestRepeatsView { made });

/// The state of the requests (RQ-01, RQ-22, RQ-29, RQ-31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestStateView {
    /// Why there are no requests: the shop has a Key app, or the book does
    /// not read. None when they are on.
    pub off: Option<WordsView>,
    /// The shop's settings were given, so payments are credited.
    pub ready: bool,
    /// How many requests are open.
    pub open: u32,
    /// How many are overdue.
    pub overdue: u32,
    /// How many wait for their first share.
    pub ready_to_send: u32,
    /// Home's row when some are overdue.
    pub overdue_row: Option<String>,
    /// Payments in a block that has not reached the confirmation depth.
    pub arrivals_waiting: u32,
    /// How many returns wait in the owner's queue.
    pub returns_queued: u32,
    /// The satoshis they come to.
    pub returns_sats: u64,
    /// What the owner is told of them.
    pub returns: Option<WordsView>,
}

wire_record!(RequestStateView {
    off,
    ready,
    open,
    overdue,
    ready_to_send,
    overdue_row,
    arrivals_waiting,
    returns_queued,
    returns_sats,
    returns
});

/// RQ-35: the app's rate as the node takes it.
fn checked_rate(rate: &RequestRateView) -> Result<CheckedRate, AppError> {
    let bad = |text: &str| {
        from_words(
            Kind::Refused,
            "RQ-12",
            format!("RQ-12: the rate is refused: {text}"),
            openbmt_core::say::Words::new(
                openbmt_core::say::Tone::Ended,
                "Price not usable",
                "The bitcoin price this phone fetched cannot be used to work out the amount.",
                openbmt_core::say::Who::Owner,
                "Fetch the price again, or ask for the amount in bitcoin.",
            ),
        )
    };
    Ok(CheckedRate {
        rate: Rate::new(rate.per_btc).ok_or_else(|| bad("it is zero"))?,
        source: RateSource::new(&rate.source, rate.fetched_at)
            .map_err(|_| bad("the source's label is not one CR-57 allows"))?,
    })
}

/// The shop's offset in seconds.
fn tz_secs(ask: &RequestAsk) -> i32 {
    ask.offset_minutes.saturating_mul(60)
}

/// Today in the shop's time zone.
fn today(ask: &RequestAsk) -> Day {
    day_of(ask.now, tz_secs(ask))
}

/// A refusal of the request calls, with the words of the library.
pub(crate) fn request_error(e: RequestError) -> AppError {
    let kind = if e.is_store() {
        Kind::Store
    } else if matches!(e, RequestError::Off(_) | RequestError::NoContext) {
        Kind::State
    } else {
        Kind::Refused
    };
    let text = e.to_string();
    from_words(kind, rule_in(&text, "RQ-35"), &text, e.words())
}

/// What the shop's settings give the requests.
pub(crate) struct ShopFacts {
    /// The shop's currency.
    pub currency: ShopCurrency,
    /// The smallest bitcoin sale in satoshis.
    pub min_sale_sats: u64,
    /// The confirmations the settings ask for.
    pub min_confirmations: u8,
}

/// Give the node what it needs of the app to credit and to quote: the
/// shop's settings (none when it holds none yet), its time zone and the
/// newest rate (RQ-12, RQ-17).
pub(crate) fn prepare<P: OwnerPort>(
    node: &mut OwnerNode<P>,
    facts: Option<ShopFacts>,
    ask: &RequestAsk,
) -> Result<(), AppError> {
    let rate = ask.rate.as_ref().map(checked_rate).transpose()?;
    let Some(settings) = facts else {
        return Ok(());
    };
    let context = RequestContext {
        currency: settings.currency,
        min_sale_sats: settings.min_sale_sats,
        min_confirmations: settings.min_confirmations,
        tz_offset_secs: tz_secs(ask),
    };
    node.set_request_context(context, rate);
    Ok(())
}

/// Whether the calls can go on: requests are on and the settings were given.
/// A call that takes an index of the desk checks this first, so that a
/// refused call does not take a range.
pub(crate) fn ready<P: OwnerPort>(node: &OwnerNode<P>) -> Result<(), AppError> {
    node.requests()
        .map_err(|e| request_error(RequestError::Off(e.clone())))?;
    node.request_context()
        .map(|_| ())
        .ok_or_else(|| request_error(RequestError::NoContext))
}

/// RQ-05: the request to make, from the app's arguments.
pub(crate) fn new_request(a: &RequestMakeArgs) -> Result<NewRequest, AppError> {
    Ok(NewRequest {
        note: a.note.clone(),
        amount: a.amount,
        price_in: match a.price_in {
            RequestPriceIn::Bitcoin => PriceIn::Bitcoin,
            RequestPriceIn::ShopCurrency => PriceIn::ShopCurrency,
        },
        due: a.due.to_day()?,
        repeat: a.repeat.map(RequestEvery::every),
    })
}

/// RQ-03: make the request at the desk's index `index`.
pub(crate) fn make<P: OwnerPort>(
    node: &mut OwnerNode<P>,
    a: &RequestMakeArgs,
    index: u32,
) -> Result<RequestMadeView, AppError> {
    let new = new_request(a)?;
    node.make_request(&new, index, a.ask.now)
        .map(Into::into)
        .map_err(request_error)
}

/// RQ-25: the repeats that are due today.
pub(crate) fn repeats_due<P: OwnerPort>(
    node: &OwnerNode<P>,
    ask: &RequestAsk,
) -> Result<Vec<DueRepeat>, AppError> {
    node.repeats_due(today(ask)).map_err(request_error)
}

/// RQ-25: make the requests of `due` at `indexes`.
pub(crate) fn make_repeats<P: OwnerPort>(
    node: &mut OwnerNode<P>,
    ask: &RequestAsk,
    due: &[DueRepeat],
    indexes: &[u32],
) -> Result<RequestRepeatsView, AppError> {
    let made = node
        .make_repeats(due, indexes, ask.now)
        .map_err(request_error)?;
    Ok(RequestRepeatsView {
        made: made.into_iter().map(Into::into).collect(),
    })
}

/// An amount in the unit of a request: a plain number and the code. A
/// request in bitcoin is in satoshis, written `SAT`.
fn amount_text(unit: Option<ShopCurrency>, n: u64) -> String {
    match unit {
        Some(currency) if !currency.is_sats() => {
            let code = Currency::of_shop(currency);
            format!("{} {code}", format_minor(n, code))
        }
        _ => format!("{n} SAT"),
    }
}

/// The unit of a request's amounts.
fn unit_of(request: &Request) -> Option<ShopCurrency> {
    match request.pricing {
        Pricing::Bitcoin => None,
        Pricing::Currency(c) => Some(c),
    }
}

fn date_text(day: Day) -> String {
    let (y, m, d) = day.civil();
    format!("{y:04}-{m:02}-{d:02}")
}

fn row(r: &Request, owed: u64, min_sale_sats: u64) -> RequestRowView {
    let unit = unit_of(r);
    let write = |n: u64| amount_text(unit, n);
    RequestRowView {
        number: r.number,
        note: r.note.clone(),
        owed,
        due: DayView::of(r.due),
        line: say::list_row(r.number, &write(owed), &date_text(r.due)),
        state: say::state_line(&r.status(min_sale_sats), &write),
        marks: say::mark_lines(&r.marks())
            .into_iter()
            .map(str::to_string)
            .collect(),
        repeats: r.repeat.is_some(),
    }
}

fn part(rows: &[Row<'_>], min_sale_sats: u64) -> Vec<RequestRowView> {
    rows.iter()
        .map(|r| row(r.request, r.owed, min_sale_sats))
        .collect()
}

/// RQ-21, RQ-22, RQ-25: the list in its parts, and the ones that ended.
pub(crate) fn list<P: OwnerPort>(
    node: &OwnerNode<P>,
    ask: &RequestAsk,
) -> Result<RequestListView, AppError> {
    let book = node
        .requests()
        .map_err(|e| request_error(RequestError::Off(e.clone())))?;
    let min = node.request_context().map_or(0, |c| c.min_sale_sats);
    let day = today(ask);
    let parts = book.list(day);
    let mut finished: Vec<RequestRowView> = book
        .requests()
        .iter()
        .rev()
        .filter(|r| !r.is_open())
        .map(|r| row(r, r.owed(), min))
        .collect();
    finished.truncate(MAX_FINISHED);
    Ok(RequestListView {
        ready_to_send: part(&parts.ready_to_send, min),
        overdue: part(&parts.overdue, min),
        due_soon: part(&parts.due_soon, min),
        waiting: part(&parts.waiting, min),
        finished,
        overdue_row: (!parts.overdue.is_empty()).then(|| say::overdue_row(parts.overdue.len())),
        empty_heading: say::empty_list().0.to_string(),
        empty_what: say::empty_list().1.to_string(),
    })
}

/// The most ended requests a list gives.
pub const MAX_FINISHED: usize = 200;

/// RQ-12, RQ-13, RQ-22: share a request: the quote, the link and the line
/// above it.
pub(crate) fn share<P: OwnerPort>(
    node: &mut OwnerNode<P>,
    a: &RequestShareArgs,
) -> Result<RequestSharedView, AppError> {
    let shared = node
        .share_request(a.number, a.ask.now, &a.shop_name)
        .map_err(request_error)?;
    let book = node
        .requests()
        .map_err(|e| request_error(RequestError::Off(e.clone())))?;
    let request = book.request(a.number);
    let unit = request.and_then(unit_of);
    let due = request.map_or(today(&a.ask), |r| r.due);
    let amount = amount_text(unit, shared.quote.owed);
    let line = if due < today(&a.ask) {
        say::overdue_line(&a.shop_name, &amount, a.number, &date_text(due))
    } else {
        say::share_line(&a.shop_name, &amount, a.number, &date_text(due))
    };
    Ok(RequestSharedView {
        number: a.number,
        owed: shared.quote.owed,
        sats: shared.quote.sats,
        rate: shared.quote.rate.map(|r| r.minor_per_btc()),
        until: shared.quote.until,
        address: shared.address,
        link: shared.link,
        line,
    })
}

/// RQ-26 to RQ-29: add a return; it is queued, never paid.
pub(crate) fn add_return<P: OwnerPort>(
    node: &mut OwnerNode<P>,
    a: &RequestReturnArgs,
) -> Result<RequestReturnAddedView, AppError> {
    let added = node
        .add_return(a.number, a.amount(), &a.address, a.ask.now)
        .map_err(request_error)?;
    let words = say::return_added(&amount_text(None, added.sats), a.number);
    Ok(RequestReturnAddedView {
        id: added.id,
        refund: added.refund,
        sats: added.sats,
        groups: added.groups,
        words: WordsView(words),
    })
}

/// RQ-27: the line that asks a customer for an address.
pub(crate) fn return_ask<P: OwnerPort>(
    node: &OwnerNode<P>,
    a: &RequestReturnArgs,
) -> Result<String, AppError> {
    let book = node
        .requests()
        .map_err(|e| request_error(RequestError::Off(e.clone())))?;
    let request = book.request(a.number).ok_or_else(|| {
        request_error(RequestError::Return(
            openbmt_requests::ReturnRefused::NoSuchRequest,
        ))
    })?;
    let unit = a.in_currency.then(|| unit_of(request)).flatten();
    Ok(say::ask_line(
        &a.shop_name,
        &amount_text(unit, a.amount),
        a.number,
    ))
}

/// RQ-22, RQ-29, RQ-31: what Home shows of the requests.
pub(crate) fn state<P: OwnerPort>(node: &OwnerNode<P>, ask: &RequestAsk) -> RequestStateView {
    let ready = node.request_context().is_some();
    let book: &RequestBook = match node.requests() {
        Ok(book) => book,
        Err(off) => return off_state(off, ready),
    };
    let parts = book.list(today(ask));
    let queued: Vec<_> = book
        .returns()
        .iter()
        .filter(|r| r.state == ReturnState::Queued)
        .collect();
    let returns_sats = queued.iter().fold(0u64, |t, r| t.saturating_add(r.sats));
    RequestStateView {
        off: None,
        ready,
        open: count(book.open_count()),
        overdue: count(parts.overdue.len()),
        ready_to_send: count(parts.ready_to_send.len()),
        overdue_row: (!parts.overdue.is_empty()).then(|| say::overdue_row(parts.overdue.len())),
        arrivals_waiting: count(node.request_arrivals().len()),
        returns_queued: count(queued.len()),
        returns_sats,
        returns: (!queued.is_empty()).then(|| {
            WordsView(say::returns_waiting(
                queued.len(),
                &amount_text(None, returns_sats),
            ))
        }),
    }
}

/// Where a request stands (RQ-16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestStatusView {
    /// Nothing counted yet.
    Waiting,
    /// Something arrived and something is still owed.
    PartPaid,
    /// Nothing is owed.
    Paid,
    /// More arrived than was owed.
    Overpaid,
    /// In the currency: a shortfall under the smallest bitcoin sale.
    Almost,
}

wire_unit_enum!(RequestStatusView {
    Waiting = 0,
    PartPaid = 1,
    Paid = 2,
    Overpaid = 3,
    Almost = 4
});

/// How a request ended (RQ-16, RQ-19, RQ-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestEndView {
    /// Cancelled with nothing received.
    Cancelled,
    /// Closed as it stood.
    Closed,
    /// Almost paid, counted as paid.
    CountedPaid,
}

wire_unit_enum!(RequestEndView {
    Cancelled = 0,
    Closed = 1,
    CountedPaid = 2
});

/// A payment that came after the request ended (RQ-19, RQ-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentAfterView {
    /// After it was cancelled.
    Cancel,
    /// After it was closed or counted as paid.
    Close,
}

wire_unit_enum!(PaymentAfterView {
    Cancel = 0,
    Close = 1
});

/// Where a return stands (RQ-29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnStateView {
    /// Waiting with the other returns.
    Queued,
    /// In a spend not yet confirmed.
    Sent,
    /// The spend that pays it is confirmed.
    Done,
    /// The owner cancelled its payment in the queue.
    Cancelled,
}

wire_unit_enum!(ReturnStateView {
    Queued = 0,
    Sent = 1,
    Done = 2,
    Cancelled = 3
});

/// A question before something is done, with the library's words (RQ-16,
/// RQ-19, RQ-20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmView {
    /// The title.
    pub title: String,
    /// What happens.
    pub text: String,
    /// The button that does it.
    pub yes: String,
    /// The button that does not.
    pub no: String,
}

wire_record!(ConfirmView {
    title,
    text,
    yes,
    no
});

impl From<say::Confirm> for ConfirmView {
    fn from(c: say::Confirm) -> Self {
        Self {
            title: c.title,
            text: c.text,
            yes: c.yes.to_string(),
            no: c.no.to_string(),
        }
    }
}

/// One quote that was written for a request (RQ-12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteView {
    /// When it was made, in Unix seconds.
    pub made_at: u64,
    /// What was owed then, in the request's unit.
    pub owed: u64,
    /// The satoshis asked for.
    pub sats: u64,
    /// The rate, minor units per bitcoin, for a request in the currency.
    pub rate: Option<u64>,
    /// When it runs out, in Unix seconds, for a request in the currency.
    pub until: Option<u64>,
}

wire_record!(QuoteView {
    made_at,
    owed,
    sats,
    rate,
    until
});

/// One payment to a request's address (RQ-15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentView {
    /// The transaction, as hex text.
    pub txid: String,
    /// Which output of it.
    pub vout: u32,
    /// The satoshis.
    pub sats: u64,
    /// The height of its block.
    pub height: u32,
    /// The block's time, in Unix seconds.
    pub block_time: u64,
    /// What it was credited, in the request's unit; 0 when it was not.
    pub credit: u64,
    /// The block has reached the confirmation depth and the payment is in
    /// the book; false for a payment still waiting for its depth.
    pub confirmed: bool,
    /// RQ-17: after the end of the due day.
    pub late: bool,
    /// RQ-18: credited at a price that had run out.
    pub old_price: bool,
    /// RQ-19, RQ-20: it came after the request ended.
    pub after: Option<PaymentAfterView>,
}

wire_record!(PaymentView {
    txid,
    vout,
    sats,
    height,
    block_time,
    credit,
    confirmed,
    late,
    old_price,
    after
});

/// One return from a request (RQ-26 to RQ-29).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnView {
    /// Its number in the book.
    pub id: u64,
    /// The satoshis.
    pub sats: u64,
    /// Where it stands.
    pub state: ReturnStateView,
    /// The customer's address.
    pub address: String,
    /// When it was queued, in Unix seconds.
    pub queued_at: u64,
    /// The spend that pays it, as hex text, once there is one.
    pub txid: Option<String>,
    /// When it was confirmed or cancelled, in Unix seconds.
    pub at: Option<u64>,
}

wire_record!(ReturnView {
    id,
    sats,
    state,
    address,
    queued_at,
    txid,
    at
});

/// What `request_read` gives: one request as the book keeps it (RQ-35).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDetailView {
    /// The row of the list: note, state line, marks.
    pub row: RequestRowView,
    /// Where it stands.
    pub status: RequestStatusView,
    /// How it ended, if it did.
    pub end: Option<RequestEndView>,
    /// When it ended, in Unix seconds.
    pub end_at: Option<u64>,
    /// How it is priced.
    pub price_in: RequestPriceIn,
    /// The code of the unit of its amounts: `SAT` for a request in bitcoin.
    pub unit: String,
    /// The amount, in its unit.
    pub amount: u64,
    /// What the payments came to, in its unit.
    pub paid: u64,
    /// The satoshis that arrived, whatever happened to the request.
    pub received_sats: u64,
    /// The satoshis of the returns that count.
    pub returned_sats: u64,
    /// When it was made, in Unix seconds.
    pub made_at: u64,
    /// The day it was made.
    pub made: DayView,
    /// How often it repeats.
    pub repeat: Option<RequestEvery>,
    /// The quotes, oldest first.
    pub quotes: Vec<QuoteView>,
    /// The payments, in the order of the blocks, then those waiting for
    /// their depth.
    pub payments: Vec<PaymentView>,
    /// The returns, oldest first.
    pub returns: Vec<ReturnView>,
    /// RQ-37: the line for more returned than it received.
    pub over_returned: Option<String>,
    /// The question before it is cancelled.
    pub cancel: ConfirmView,
    /// The question before it is closed.
    pub close: ConfirmView,
    /// The question before it is counted as paid.
    pub count_paid: ConfirmView,
}

wire_record!(RequestDetailView {
    row,
    status,
    end,
    end_at,
    price_in,
    unit,
    amount,
    paid,
    received_sats,
    returned_sats,
    made_at,
    made,
    repeat,
    quotes,
    payments,
    returns,
    over_returned,
    cancel,
    close,
    count_paid
});

fn status_view(s: Status) -> RequestStatusView {
    match s {
        Status::Waiting => RequestStatusView::Waiting,
        Status::PartPaid { .. } => RequestStatusView::PartPaid,
        Status::Paid => RequestStatusView::Paid,
        Status::Overpaid { .. } => RequestStatusView::Overpaid,
        Status::Almost { .. } => RequestStatusView::Almost,
    }
}

fn txid_text(txid: [u8; 32]) -> String {
    Txid::from_byte_array(txid).to_string()
}

fn return_view(r: &openbmt_requests::Return, network: Network) -> ReturnView {
    let (state, txid, at) = match r.state {
        ReturnState::Queued => (ReturnStateView::Queued, None, None),
        ReturnState::Sent { txid } => (ReturnStateView::Sent, Some(txid_text(txid)), None),
        ReturnState::Done { txid, at } => (ReturnStateView::Done, Some(txid_text(txid)), Some(at)),
        ReturnState::Cancelled { at } => (ReturnStateView::Cancelled, None, Some(at)),
    };
    ReturnView {
        id: r.id,
        sats: r.sats,
        state,
        address: Address::from_script(&r.script, network)
            .map_or_else(|_| String::new(), |a| a.to_string()),
        queued_at: r.queued_at,
        txid,
        at,
    }
}

/// RQ-35: one request as the book keeps it, for its page.
pub(crate) fn read<P: OwnerPort>(
    node: &OwnerNode<P>,
    a: &RequestNumberArgs,
) -> Result<RequestDetailView, AppError> {
    let book = node
        .requests()
        .map_err(|e| request_error(RequestError::Off(e.clone())))?;
    let request = book.request(a.number).ok_or_else(|| {
        request_error(RequestError::Return(
            openbmt_requests::ReturnRefused::NoSuchRequest,
        ))
    })?;
    let min = node.request_context().map_or(0, |c| c.min_sale_sats);
    let unit = unit_of(request);
    let write = |n: u64| amount_text(unit, n);
    let network = node.network();
    let returns: Vec<ReturnView> = book
        .returns()
        .iter()
        .filter(|r| r.request == request.number)
        .map(|r| return_view(r, network))
        .collect();
    let returned_sats = book
        .returns()
        .iter()
        .filter(|r| r.request == request.number && r.state.counts())
        .fold(0u64, |t, r| t.saturating_add(r.sats));
    let mut payments: Vec<PaymentView> = request
        .credits
        .iter()
        .map(|c| PaymentView {
            txid: txid_text(c.txid),
            vout: c.vout,
            sats: c.sats,
            height: c.height,
            block_time: c.block_time,
            credit: c.value,
            confirmed: true,
            late: c.late,
            old_price: c.old_price,
            after: c.after.map(|x| match x {
                After::Cancel => PaymentAfterView::Cancel,
                After::Close => PaymentAfterView::Close,
            }),
        })
        .collect();
    payments.extend(
        node.request_arrivals()
            .iter()
            .filter(|p| p.index == request.address_index)
            .map(|p| PaymentView {
                txid: txid_text(p.txid),
                vout: p.vout,
                sats: p.sats,
                height: p.height,
                block_time: p.block_time,
                credit: 0,
                confirmed: false,
                late: false,
                old_price: false,
                after: None,
            }),
    );
    let credited = request.credited();
    Ok(RequestDetailView {
        row: row(request, request.owed(), min),
        status: status_view(request.status(min)),
        end: request.end.map(|(e, _)| match e {
            End::Cancelled => RequestEndView::Cancelled,
            End::Closed => RequestEndView::Closed,
            End::CountedPaid => RequestEndView::CountedPaid,
        }),
        end_at: request.end.map(|(_, at)| at),
        price_in: match request.pricing {
            Pricing::Bitcoin => RequestPriceIn::Bitcoin,
            Pricing::Currency(_) => RequestPriceIn::ShopCurrency,
        },
        unit: match unit {
            Some(c) if !c.is_sats() => Currency::of_shop(c).to_string(),
            _ => "SAT".to_string(),
        },
        amount: request.amount,
        paid: credited,
        received_sats: request.received_sats(),
        returned_sats,
        made_at: request.made_at,
        made: DayView::of(request.made),
        repeat: request.repeat.map(|r| match r.every {
            Every::Week => RequestEvery::Week,
            Every::Month => RequestEvery::Month,
            Every::Year => RequestEvery::Year,
        }),
        quotes: request
            .quotes
            .iter()
            .map(|q| QuoteView {
                made_at: q.made_at,
                owed: q.owed,
                sats: q.sats,
                rate: q.rate.map(|r| r.minor_per_btc()),
                until: q.until,
            })
            .collect(),
        payments,
        returns,
        over_returned: book
            .over_returned(request.number)
            .map(|over| say::over_returned_line(&amount_text(None, over))),
        cancel: say::cancel_confirm(request.number).into(),
        close: say::close_confirm(request.number, &write(credited)).into(),
        count_paid: say::count_as_paid_confirm(
            request.number,
            &write(request.amount.saturating_sub(credited)),
        )
        .into(),
    })
}

fn off_state(off: &RequestsOff, ready: bool) -> RequestStateView {
    RequestStateView {
        off: Some(WordsView(RequestError::Off(off.clone()).words())),
        ready,
        open: 0,
        overdue: 0,
        ready_to_send: 0,
        overdue_row: None,
        arrivals_waiting: 0,
        returns_queued: 0,
        returns_sats: 0,
        returns: None,
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
