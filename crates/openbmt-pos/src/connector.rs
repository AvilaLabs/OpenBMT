//! The boundary a POS connector fills (PS-80 to PS-82): traits and types
//! only, a scripted double for tests, and [`sync`], the poll that feeds the
//! matcher. No vendor and no network is here.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;

use openbmt_counter::Currency;

use crate::facts::{SaleFact, SaleKey};
use crate::ledger::{Delta, Ledger, LedgerWriteError};
use crate::model::{ModelError, OrderId, PosOrder, Text};
use crate::reference::IdempotencyKey;
use crate::settings::Settings;
use crate::{MAX_CURSOR_BYTES, MAX_PAGES, MAX_PAGE_ORDERS};

/// Where the next poll of a POS starts. Opaque to everything but the
/// connector that wrote it; at most 256 bytes. The empty cursor is the start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cursor(Vec<u8>);

impl Cursor {
    /// The start.
    pub fn start() -> Self {
        Self::default()
    }

    /// A cursor; refused above 256 bytes.
    pub fn new(bytes: Vec<u8>) -> Result<Self, ModelError> {
        if bytes.len() > MAX_CURSOR_BYTES {
            Err(ModelError::CursorTooLong)
        } else {
            Ok(Self(bytes))
        }
    }

    /// The bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// One page of orders (PS-80).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// The orders that changed since the cursor, at most 500.
    pub orders: Vec<PosOrder>,
    /// Where the next poll starts.
    pub next: Cursor,
    /// There are more pages after this one.
    pub more: bool,
}

/// Why a POS call failed. The words are for the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosError {
    /// The POS could not be reached.
    Unavailable,
    /// The POS refused the sign-in: it was revoked, or it expired.
    SignInRefused,
    /// The POS asked to be called less often.
    RateLimited {
        /// Seconds to wait.
        retry_after_secs: u64,
    },
    /// The POS answered with something that could not be read.
    BadResponse(&'static str),
    /// The connector does not do this (for a close, most of them).
    Unsupported,
}

impl fmt::Display for PosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => write!(f, "the POS could not be reached"),
            Self::SignInRefused => write!(f, "the POS refused the sign-in"),
            Self::RateLimited { retry_after_secs } => {
                write!(f, "the POS asks to wait {retry_after_secs} seconds")
            }
            Self::BadResponse(why) => {
                write!(f, "the POS answered with something unreadable: {why}")
            }
            Self::Unsupported => write!(f, "this connector does not do that"),
        }
    }
}

impl std::error::Error for PosError {}

/// PS-80: the read-only side of a connector.
pub trait PosReader {
    /// The orders that changed since `cursor`, one page at a time. Reading
    /// the same cursor again gives the same orders or newer versions of them.
    fn poll(&mut self, cursor: &Cursor) -> Result<Page, PosError>;
}

/// Why a close request was not made (PS-81).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseRefused {
    /// PS-81: the sale is not reported paid now.
    SaleNotPaid,
}

impl fmt::Display for CloseRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SaleNotPaid => write!(
                f,
                "PS-81: an order is closed only for a sale reported paid now (CR-30)"
            ),
        }
    }
}

impl std::error::Error for CloseRefused {}

/// PS-81: a request to close an order with the Bitcoin tender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseRequest {
    /// The order to close.
    pub order: OrderId,
    /// The sale it is closed for.
    pub sale: SaleKey,
    /// The name of the Bitcoin tender to add.
    pub tender_name: Text,
    /// The amount, in minor units.
    pub amount_minor: u64,
    /// The currency.
    pub currency: Currency,
    /// The key that makes a retry safe: the tagged hash of the counter's key
    /// and the sale index.
    pub key: IdempotencyKey,
}

impl CloseRequest {
    /// A request to close `order` for `sale`, with the tender named
    /// `tender_name`. Refused unless the sale is reported paid now.
    pub fn for_sale(
        sale: &SaleFact,
        order: OrderId,
        tender_name: Text,
    ) -> Result<Self, CloseRefused> {
        if !sale.is_paid_now() {
            return Err(CloseRefused::SaleNotPaid);
        }
        Ok(Self {
            order,
            sale: sale.key(),
            tender_name,
            amount_minor: sale.amount.0,
            currency: sale.currency,
            key: IdempotencyKey::for_sale(&sale.counter, sale.index),
        })
    }
}

/// What a close did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// The order was closed now.
    Closed,
    /// A request with this key had already closed it. Nothing was done.
    AlreadyClosed,
}

/// PS-81: the optional capability of a connector: closing an order through
/// the POS's own API.
pub trait PosCloser {
    /// Close an order. A retry with the same key does not close it twice.
    fn close(&mut self, request: &CloseRequest) -> Result<CloseOutcome, PosError>;
}

/// PS-82: a reader that plays a script of pages, for tests. When the script
/// is spent it answers with an empty page and the cursor it was given.
#[derive(Debug, Default)]
pub struct MemoryPos {
    script: VecDeque<Result<Page, PosError>>,
    polled: Vec<Cursor>,
}

impl MemoryPos {
    /// A reader that answers each poll with the next page.
    pub fn new(pages: impl IntoIterator<Item = Page>) -> Self {
        Self::scripted(pages.into_iter().map(Ok))
    }

    /// A reader whose script may hold failures.
    pub fn scripted(script: impl IntoIterator<Item = Result<Page, PosError>>) -> Self {
        Self {
            script: script.into_iter().collect(),
            polled: Vec::new(),
        }
    }

    /// The cursors it was polled with, in order.
    pub fn polled(&self) -> &[Cursor] {
        &self.polled
    }

    /// How many scripted answers are left.
    pub fn remaining(&self) -> usize {
        self.script.len()
    }
}

impl PosReader for MemoryPos {
    fn poll(&mut self, cursor: &Cursor) -> Result<Page, PosError> {
        self.polled.push(cursor.clone());
        self.script.pop_front().unwrap_or_else(|| {
            Ok(Page {
                orders: Vec::new(),
                next: cursor.clone(),
                more: false,
            })
        })
    }
}

/// PS-82: a closer that remembers the keys it has seen, for tests.
#[derive(Debug, Default)]
pub struct MemoryCloser {
    seen: BTreeSet<IdempotencyKey>,
    /// The requests that closed an order, in order.
    pub closed: Vec<CloseRequest>,
}

impl PosCloser for MemoryCloser {
    fn close(&mut self, request: &CloseRequest) -> Result<CloseOutcome, PosError> {
        if self.seen.insert(request.key) {
            self.closed.push(request.clone());
            Ok(CloseOutcome::Closed)
        } else {
            Ok(CloseOutcome::AlreadyClosed)
        }
    }
}

/// Why a sync did not happen. The ledger and its cursor are unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError<E> {
    /// The reader failed.
    Pos(PosError),
    /// PS-80: a page holds more than 500 orders.
    PageTooLarge,
    /// PS-80: a page says there is more and does not move the cursor.
    NoProgress,
    /// The ledger refused the orders, the facts or the settings.
    Ledger(crate::ledger::LedgerError),
    /// PS-70: the caller's store failed.
    Persist(E),
}

impl<E: fmt::Display> fmt::Display for SyncError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pos(e) => write!(f, "{e}"),
            Self::PageTooLarge => {
                write!(f, "PS-80: a page holds more than {MAX_PAGE_ORDERS} orders")
            }
            Self::NoProgress => write!(f, "PS-80: the POS says there is more and does not move on"),
            Self::Ledger(e) => write!(f, "{e}"),
            Self::Persist(e) => write!(f, "could not store the ledger: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for SyncError<E> {}

/// What a sync did (PS-60).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Pages read.
    pub pages: usize,
    /// Orders read.
    pub orders_read: usize,
    /// More pages remain; the next sync starts from the stored cursor.
    pub more: bool,
    /// What is new: matches, alerts raised, alerts cleared, matches retired.
    pub delta: Delta,
}

/// PS-74, PS-80: pull the pages of orders (at most 64), build the new ledger
/// from them, store it and swap. All or nothing: if the reader, the ledger or
/// the store fails, the ledger and its cursor are as they were. Reading the
/// same pages again changes nothing and raises nothing again.
pub fn sync<R: PosReader, E>(
    reader: &mut R,
    ledger: &mut Ledger,
    facts: &[SaleFact],
    settings: &Settings,
    now: u64,
    persist: impl FnOnce(&[u8]) -> Result<(), E>,
) -> Result<SyncReport, SyncError<E>> {
    let mut cursor = ledger.cursor().clone();
    let mut orders: Vec<PosOrder> = Vec::new();
    let mut pages = 0usize;
    let mut more = false;
    loop {
        if pages >= MAX_PAGES {
            more = true;
            break;
        }
        let page = reader.poll(&cursor).map_err(SyncError::Pos)?;
        if page.orders.len() > MAX_PAGE_ORDERS {
            return Err(SyncError::PageTooLarge);
        }
        if page.more && page.next == cursor {
            return Err(SyncError::NoProgress);
        }
        pages += 1;
        orders.extend(page.orders);
        cursor = page.next;
        if !page.more {
            break;
        }
    }
    let orders_read = orders.len();
    let delta = ledger
        .update_stored(&orders, cursor, facts, settings, now, persist)
        .map_err(|e| match e {
            LedgerWriteError::Refused(e) => SyncError::Ledger(e),
            LedgerWriteError::Persist(e) => SyncError::Persist(e),
        })?;
    Ok(SyncReport {
        pages,
        orders_read,
        more,
        delta,
    })
}
