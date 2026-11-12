//! The OpenBMT books: the counter's verified records turned into the
//! merchant's books, the end-of-day totals, two CSV exports and an encrypted
//! export file the books can be read back from.
//!
//! The rules are in `docs/BOOKS-RULES.md`; each has an ID (BK-01 and on), the
//! errors say their rule in words and the tests name the rule they prove.
//! The export is not tax advice and covers US rules only.
//!
//! It has no network and no clock. It writes a file only through
//! [`write_whole`]. The caller passes in the chains, the facts of the POS,
//! the outflows, the time, the shop's offset from UTC, the passphrase and the
//! randomness for the salt.
//!
//! The parts are:
//!
//! 1. [`date`]: civil dates from Unix seconds.
//! 2. [`value`]: a field that is known or an unknown that says why.
//! 3. [`model`]: sales, the opaque sale ID, disposals and returns.
//! 4. [`books`]: [`Books::from_chains`], the folding of events and the totals.
//! 5. [`export`]: the accounting CSV and the full ledger.
//! 6. [`bundle`]: the books as a framed byte string (`OBM6`).
//! 7. [`sealed`]: the encrypted export file (`OBMY`).
//! 8. [`fsio`]: the file helper [`write_whole`].
//! 9. [`till`]: the till's own payments read from its CSV and matched
//!    against the books by reference (BK-46, BK-47).
//!
//! This crate has had internal review only. No independent security audit
//! has been done yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod books;
pub mod bundle;
mod csv;
pub mod date;
pub mod error;
pub mod export;
pub mod fsio;
pub mod model;
mod money;
pub mod say;
pub mod sealed;
pub mod till;
pub mod value;

#[cfg(test)]
mod tests;

pub use books::{Books, DayTotals, MAX_BILLS, MAX_BILL_PARTS};
pub use bundle::MAGIC_BUNDLE;
pub use date::{Date, YearMonth};
pub use error::{BooksError, ExportError, OpenError, SealError};
pub use export::{RevealsPayments, LEDGER_WARNING};
pub use fsio::write_whole;
pub use model::{
    Basis, Bill, BillEndNote, BillKey, BillPartNote, CashReturn, Counting, Disposal, DisposalKind,
    LatePayment, LightningPayment, OverrideNote, Payment, PaymentList, PaymentMode, PosFacts,
    PosMark, Sale, SaleId, SaleKey, SaleState, Settlement, StaffNote, TipNote,
};
pub use money::{format_minor, minor_digits};
pub use sealed::{
    decode_export_header, open_export, seal_export, ExportHeader, KdfParams, RandomFailed,
    MAGIC_EXPORT,
};
pub use till::{
    read_till_csv, MappingError, TillCsvError, TillCsvProblem, TillItem, TillItemKey, TillLine,
    TillMapping, TillMatch, TillPair, MAX_TILL_BYTES, MAX_TILL_LINES,
};
pub use value::{Unknown, UnknownField, Value};
