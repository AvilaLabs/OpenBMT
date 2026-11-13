//! The books for the owner's phone (`docs/BOOKS-RULES.md`, BK-01 to BK-22;
//! `docs/OWNER-APP-RULES.md` OA-103): one call that builds the books from
//! what the node holds and gives the totals by day and the two CSV exports.
//!
//! The node holds the verified copy of each counter's records and the signed
//! device list (the link's), and the log of what it paid out (OC-48). The
//! sales and the cash returns are read from the records by the books library;
//! the refunds and cash-outs the owner paid are given to it from the log as
//! disposals. Nothing is made up: a disposal whose value the log does not
//! know stays unknown, and the library says so when an export needs it.
//!
//! Every refusal is the books library's own, with its own words.

use openbmt_books::say::ExportKind;
use openbmt_books::{
    format_minor, Books, BooksError, Disposal, DisposalKind, ExportError, RevealsPayments,
    SaleKey as BooksSaleKey,
};
use openbmt_core::identity::DeviceId;
use openbmt_core::records::Chain;
use openbmt_counter::Currency;
use openbmt_owner_node::{standing, OwnerLink, OwnerNode, OwnerPort, PaidEntry, PaidWhat};

use crate::codec::{wire_record, wire_record_get};
use crate::error::{from_words, rule_in, AppError, Kind};
use crate::say::words_for;
use crate::views::WordsView;

/// The most days the page lists.
pub const MAX_DAYS: usize = 90;

/// The arguments of `books`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BooksArgs {
    /// The shop's offset from UTC in minutes east, from the phone (BK-16).
    pub offset_minutes: i32,
}

wire_record!(BooksArgs { offset_minutes });
wire_record_get!(BooksArgs { offset_minutes });

/// One shop-local day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BooksDayView {
    /// The day, `YYYY-MM-DD`.
    pub date: String,
    /// Sales that received funds that day.
    pub sales: u64,
    /// Of those, the ones not final yet.
    pub waiting: u64,
    /// What they came to, in the shop's currency, as the books write an
    /// amount: a decimal with the currency's minor digits, no separators.
    pub amount: String,
}

wire_record!(BooksDayView {
    date,
    sales,
    waiting,
    amount
});

/// One shop-local day of the payments that arrived for requests (RQ-32).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDayView {
    /// The day, `YYYY-MM-DD`.
    pub date: String,
    /// The satoshis that arrived on the day of their block.
    pub sats: u64,
}

wire_record!(RequestDayView { date, sats });

/// One file of the accounting export: a calendar month.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BooksFileView {
    /// The month, `YYYY-MM` (UTC).
    pub month: String,
    /// The CSV the library made.
    pub csv: Vec<u8>,
}

wire_record!(BooksFileView { month, csv });

/// What `books` gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BooksView {
    /// The shop's currency, three capital letters (`SAT` for Bitcoin
    /// Standard); empty when the phone holds no settings.
    pub currency: String,
    /// The days, newest first, at most [`MAX_DAYS`].
    pub days: Vec<BooksDayView>,
    /// The satoshis that arrived for requests on each day, newest first, at
    /// most [`MAX_DAYS`] (RQ-32). They are not sales, and no total above
    /// counts them.
    pub request_days: Vec<RequestDayView>,
    /// The accounting export, a file for each month, oldest first.
    pub accounting: Vec<BooksFileView>,
    /// Why an export was not made, in the library's words. The days are
    /// still given.
    pub refusal: Option<WordsView>,
}

wire_record!(BooksView {
    currency,
    days,
    request_days,
    accounting,
    refusal
});

impl BooksView {
    fn empty(currency: String) -> Self {
        Self {
            currency,
            days: Vec::new(),
            request_days: Vec::new(),
            accounting: Vec::new(),
            refusal: None,
        }
    }
}

/// The name a counter is called in the library's words: the label the app
/// shows for a counter whose name it does not keep.
fn counter_label(counter: &DeviceId) -> String {
    let b = counter.to_bytes();
    let hex: String = b.iter().take(4).map(|x| format!("{x:02x}")).collect();
    format!("Counter {hex}")
}

/// The counter a books refusal is about, when it is about one.
fn books_counter(e: &BooksError) -> Option<DeviceId> {
    match e {
        BooksError::CounterNotOnList { counter }
        | BooksError::DuplicateCounter { counter }
        | BooksError::BadChain { counter, .. }
        | BooksError::BadEvent { counter, .. }
        | BooksError::KindMismatch { counter, .. }
        | BooksError::UnknownSale { counter, .. }
        | BooksError::OpenedTwice { counter, .. } => Some(*counter),
        BooksError::SaleNotInBooks { sale } => Some(sale.counter),
        BooksError::TimeOutOfRange { .. }
        | BooksError::Overflow
        | BooksError::TooMany
        | BooksError::BadOffset { .. } => None,
    }
}

fn books_error(e: &BooksError) -> AppError {
    let name = books_counter(e)
        .map(|c| counter_label(&c))
        .unwrap_or_default();
    let text = e.to_string();
    from_words(
        Kind::Refused,
        rule_in(&text, "BK-01"),
        &text,
        e.words(&name),
    )
}

fn export_error(e: &ExportError, kind: ExportKind) -> AppError {
    let name = match e {
        ExportError::SaleMissing { sale } => counter_label(&sale.counter),
        _ => String::new(),
    };
    let text = e.to_string();
    from_words(
        Kind::Refused,
        rule_in(&text, "BK-17"),
        &text,
        e.words(kind, &name),
    )
}

/// What the owner paid out, as the disposals the books take (BK-12). A value
/// the log does not know is left unknown, in the shop's currency.
///
/// The refunds of a sale of `desk`, the owner's own device, are left out when
/// it is given: those are returns of requests, which the request ledger
/// lines carry (RQ-32) and the books have no sale for.
pub(crate) fn disposals(
    paid: &[PaidEntry],
    shop: Currency,
    desk: Option<DeviceId>,
) -> Vec<Disposal> {
    // OC-50: a payment that a later entry undoes is left out with its undoing.
    standing(paid)
        .iter()
        .filter(|e| match (&e.what, desk) {
            (PaidWhat::Refund { sale, .. }, Some(desk)) => sale.counter != desk,
            _ => true,
        })
        .map(|e| {
            let currency = e
                .value
                .and_then(|v| Currency::new(v.currency))
                .unwrap_or(shop);
            Disposal {
                at: e.at,
                kind: match e.what {
                    PaidWhat::Refund { sale, .. } => DisposalKind::Refund {
                        sale: BooksSaleKey {
                            counter: sale.counter,
                            index: sale.index,
                        },
                    },
                    PaidWhat::CashOut | PaidWhat::Undone(_) => DisposalKind::CashOut,
                },
                currency,
                sats: e.sats,
                minor_value: e.value.map(|v| v.minor),
                basis: None,
            }
        })
        .collect()
}

/// The books from what the node holds, with the shop's currency code and
/// currency; none for a phone with no link or no settings.
fn make_books<P: OwnerPort>(
    node: &OwnerNode<P>,
    link: Option<&OwnerLink>,
) -> Result<Option<(Books, String, Currency)>, AppError> {
    let Some(shop) = node.owner().known_settings().map(|s| s.currency) else {
        return Ok(None);
    };
    let code = shop.code().to_string();
    let Some(link) = link else {
        return Ok(None);
    };
    let currency = Currency::of_shop(shop);
    let chains: Vec<(DeviceId, Chain)> = link
        .counters()
        .iter()
        .filter_map(|c| {
            link.copy(c)
                .map(|copy| (*c, Chain::from_records(copy.records().to_vec())))
        })
        .collect();
    let refs: Vec<(DeviceId, &Chain)> = chains.iter().map(|(c, ch)| (*c, ch)).collect();
    let books = Books::from_chains(link.devices(), &refs)
        .and_then(|b| {
            b.with_disposals(disposals(
                node.paid_log(),
                currency,
                Some(node.owner().owner_id()),
            ))
        })
        .map_err(|e| books_error(&e))?;
    Ok(Some((books, code, currency)))
}

/// OA-103: the books from what the node holds. A phone with no link or no
/// settings has none, and gives no days. The page holds the accounting
/// export and never the full ledger (BK-19).
pub(crate) fn build<P: OwnerPort>(
    node: &OwnerNode<P>,
    link: Option<&OwnerLink>,
    args: &BooksArgs,
) -> Result<BooksView, AppError> {
    let Some(shop) = node.owner().known_settings().map(|s| s.currency) else {
        return Ok(BooksView::empty(String::new()));
    };
    let Some((books, code, currency)) = make_books(node, link)? else {
        let mut view = BooksView::empty(shop.code().to_string());
        view.request_days = request_days(node, args.offset_minutes);
        return Ok(view);
    };
    let totals = books
        .local_day_totals(args.offset_minutes)
        .map_err(|e| books_error(&e))?;
    let days: Vec<BooksDayView> = totals
        .iter()
        .rev()
        .take(MAX_DAYS)
        .map(|d| BooksDayView {
            date: d.day.to_string(),
            sales: d.sales,
            waiting: d.waiting,
            amount: format_minor(d.minor.get(&currency).copied().unwrap_or(0), currency),
        })
        .collect();
    let mut view = BooksView::empty(code);
    view.days = days;
    view.request_days = request_days(node, args.offset_minutes);
    match books
        .accounting_export()
        .map_err(|e| export_error(&e, ExportKind::Accounting))
    {
        Ok(files) => {
            view.accounting = files
                .into_iter()
                .map(|(month, csv)| BooksFileView {
                    month: month.to_string(),
                    csv: csv.into_bytes(),
                })
                .collect();
        }
        Err(AppError::Refused { words, .. }) => view.refusal = Some(WordsView(*words)),
        Err(other) => return Err(other),
    }
    Ok(view)
}

/// BK-19: the full ledger, made only when the person has accepted the
/// warning. `acknowledged` is the app's answer; `false` refuses. This is the
/// only place that passes `RevealsPayments::Acknowledged`. A phone with no
/// link or no settings has no books and gives no bytes.
pub(crate) fn ledger<P: OwnerPort>(
    node: &OwnerNode<P>,
    link: Option<&OwnerLink>,
    acknowledged: bool,
) -> Result<Vec<u8>, AppError> {
    if !acknowledged {
        // The app always passes the person's answer after the warning, so no
        // person is shown this refusal: it gets the words of its kind.
        let text = "BK-19: the full ledger is made only after the person has accepted the warning";
        return Err(from_words(
            Kind::Refused,
            "BK-19",
            text,
            words_for(Kind::Refused, "BK-19"),
        ));
    }
    let mut bytes = match make_books(node, link)? {
        Some((books, _, _)) => full_ledger_bytes(&books)?,
        None => Vec::new(),
    };
    bytes.extend(request_lines(node));
    Ok(bytes)
}

/// RQ-32: the payments that arrived for requests on each shop-local day,
/// newest first.
fn request_days<P: OwnerPort>(node: &OwnerNode<P>, offset_minutes: i32) -> Vec<RequestDayView> {
    let Ok(book) = node.requests() else {
        return Vec::new();
    };
    book.payments_by_day(offset_minutes.saturating_mul(60))
        .iter()
        .rev()
        .take(MAX_DAYS)
        .map(|(day, sats)| RequestDayView {
            date: date_text(*day),
            sats: *sats,
        })
        .collect()
}

fn date_text(day: openbmt_requests::Day) -> String {
    let (y, m, d) = day.civil();
    format!("{y:04}-{m:02}-{d:02}")
}

/// BK-20, BK-21: one cell as the books write a text: a `\'` before one a
/// spreadsheet could run, and quotes when it needs them.
fn csv_text(s: &str) -> String {
    let guarded = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if guarded.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn time_text(secs: u64) -> String {
    openbmt_books::date::iso_time(secs).unwrap_or_default()
}

/// RQ-32: each request as a line, after the books' own ledger. The columns
/// and the way a request ended are placeholders for words that are still to
/// be written.
fn request_lines<P: OwnerPort>(node: &OwnerNode<P>) -> Vec<u8> {
    let Ok(book) = node.requests() else {
        return Vec::new();
    };
    let lines = book.ledger_lines();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut out = String::from("\r\n");
    let header = [
        "NUMBER",
        "NOTE",
        "MADE",
        "DUE",
        "PRICED_IN",
        "AMOUNT",
        "PAYMENTS",
        "MARKS",
        "ENDED",
        "ENDED_AT",
        "RETURNS",
    ];
    let cells: Vec<String> = header
        .iter()
        .map(|h| format!("WORDS_NEEDED_RQ_32_{h}"))
        .collect();
    out.push_str(&cells.join(","));
    out.push_str("\r\n");
    for l in lines {
        let priced = match l.priced_in {
            openbmt_requests::Pricing::Bitcoin => "SAT".to_string(),
            openbmt_requests::Pricing::Currency(c) => c.code().to_string(),
        };
        let payments: Vec<String> = l
            .payments
            .iter()
            .map(|p| format!("{}|{}|{}", time_text(p.block_time), p.sats, p.credit))
            .collect();
        let returns: Vec<String> = l
            .returns
            .iter()
            .map(|r| {
                format!(
                    "{}|{}|{}",
                    time_text(r.queued_at),
                    r.sats,
                    r.done_at.map(time_text).unwrap_or_default()
                )
            })
            .collect();
        let (ended, ended_at) = match l.ended {
            Some((end, at)) => (
                match end {
                    openbmt_requests::End::Cancelled => "WORDS_NEEDED_RQ_32_ENDED_CANCELLED",
                    openbmt_requests::End::Closed => "WORDS_NEEDED_RQ_32_ENDED_CLOSED",
                    openbmt_requests::End::CountedPaid => "WORDS_NEEDED_RQ_32_ENDED_COUNTED_PAID",
                },
                time_text(at),
            ),
            None => ("", String::new()),
        };
        let row = [
            l.number.to_string(),
            csv_text(&l.note),
            time_text(l.made_at),
            date_text(l.due),
            priced,
            l.amount.to_string(),
            csv_text(&payments.join(";")),
            csv_text(&openbmt_requests::say::mark_lines(&l.marks).join(";")),
            ended.to_string(),
            ended_at,
            csv_text(&returns.join(";")),
        ];
        out.push_str(&row.join(","));
        out.push_str("\r\n");
    }
    out.into_bytes()
}

pub(crate) fn full_ledger_bytes(books: &Books) -> Result<Vec<u8>, AppError> {
    books
        .full_ledger(RevealsPayments::Acknowledged)
        .map(String::into_bytes)
        .map_err(|e| export_error(&e, ExportKind::FullLedger))
}
