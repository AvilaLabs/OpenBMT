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
use openbmt_owner_node::{OwnerLink, OwnerNode, OwnerPort, PaidEntry, PaidWhat};

use crate::codec::{wire_record, wire_record_get};
use crate::error::{from_words, rule_in, AppError, Kind};
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
    /// The accounting export, a file for each month, oldest first.
    pub accounting: Vec<BooksFileView>,
    /// The full ledger, empty when it was not made.
    pub ledger: Vec<u8>,
    /// Why an export was not made, in the library's words. The days are
    /// still given.
    pub refusal: Option<WordsView>,
}

wire_record!(BooksView {
    currency,
    days,
    accounting,
    ledger,
    refusal
});

impl BooksView {
    fn empty(currency: String) -> Self {
        Self {
            currency,
            days: Vec::new(),
            accounting: Vec::new(),
            ledger: Vec::new(),
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
pub(crate) fn disposals(paid: &[PaidEntry], shop: Currency) -> Vec<Disposal> {
    paid.iter()
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
                    PaidWhat::CashOut => DisposalKind::CashOut,
                },
                currency,
                sats: e.sats,
                minor_value: e.value.map(|v| v.minor),
                basis: None,
            }
        })
        .collect()
}

/// OA-103: the books from what the node holds. A phone with no link or no
/// settings has none, and gives no days.
pub(crate) fn build<P: OwnerPort>(
    node: &OwnerNode<P>,
    link: Option<&OwnerLink>,
    args: &BooksArgs,
) -> Result<BooksView, AppError> {
    let Some(shop) = node.owner().known_settings().map(|s| s.currency) else {
        return Ok(BooksView::empty(String::new()));
    };
    let code = shop.code().to_string();
    let Some(link) = link else {
        return Ok(BooksView::empty(code));
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
        .and_then(|b| b.with_disposals(disposals(node.paid_log(), currency)))
        .map_err(|e| books_error(&e))?;
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
    let made = books
        .accounting_export()
        .map_err(|e| export_error(&e, ExportKind::Accounting))
        .and_then(|files| {
            let ledger = books
                .full_ledger(RevealsPayments::Acknowledged)
                .map_err(|e| export_error(&e, ExportKind::FullLedger))?;
            Ok((files, ledger))
        });
    match made {
        Ok((files, ledger)) => {
            view.accounting = files
                .into_iter()
                .map(|(month, csv)| BooksFileView {
                    month: month.to_string(),
                    csv: csv.into_bytes(),
                })
                .collect();
            view.ledger = ledger.into_bytes();
        }
        Err(AppError::Refused { words, .. }) => view.refusal = Some(WordsView(*words)),
        Err(other) => return Err(other),
    }
    Ok(view)
}
