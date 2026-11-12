//! Payment requests on the owner phone (`docs/REQUEST-RULES.md`, RQ-01 to
//! RQ-36).
//!
//! A shop asks a customer who is not at the till to pay: a deposit, an
//! invoice, a stall fee. The owner phone makes the address, keeps the
//! request, counts what arrives and returns money when it must. This crate
//! holds the part that is judgment, as a library with no clock, no network
//! and no screen:
//!
//! 1. [`date`]: the shop's calendar, written out (no date library).
//! 2. [`model`]: a request, its quotes, payments and returns, and how the
//!    state is read from them.
//! 3. [`book`], [`list`], [`repeat`], [`returns`], [`ledger`]: the
//!    [`RequestBook`] and what is done with it.
//! 4. [`address`]: the address a customer sends for a return.
//! 5. [`codec`], [`fsio`]: the book as `OBM=` bytes and as a file.
//! 6. [`say`]: the owner's words.
//!
//! The desk's allocation of address indexes (RQ-02, RQ-04) belongs to the
//! owner state: the book takes an index from the caller and refuses one
//! already used. A `bitcoin:` link is not read here yet (RQ-28).

#![forbid(unsafe_code)]

pub mod address;
pub mod book;
pub mod codec;
pub mod date;
pub mod error;
pub mod fsio;
pub mod ledger;
pub mod list;
pub mod model;
pub mod repeat;
pub mod returns;
pub mod say;

#[cfg(test)]
mod codec_tests;

pub use address::{address_in_groups, check_return_address, MAX_ADDRESS_BYTES};
pub use book::{NewRequest, PriceIn, RequestBook, Shop, MAX_REQUESTS, MAX_RETURNS};
pub use codec::MAGIC_REQUEST_BOOK;
pub use date::{day_of, end_of_day, Day};
pub use error::{
    AddressRefused, BookError, CreditRefused, EndRefused, Limit, MakeRefused, RepeatRefused,
    ReturnRefused, ShareRefused,
};
pub use fsio::REQUEST_BOOK_FILE;
pub use ledger::{LedgerLine, LedgerPayment, LedgerReturn};
pub use list::{ListParts, Row};
pub use model::{
    After, Arrival, CheckedRate, Credit, End, Every, Marks, Pricing, Quote, Repeat, Request,
    Return, ReturnState, Status,
};
pub use repeat::DueRepeat;
pub use returns::{ReturnAmount, MAX_QUEUED_RETURNS};

/// RQ-13: the BIP 321 link a request shares: the address, the amount in
/// bitcoin and the shop's name as the label, written as CR-83 writes a
/// sale's, and nothing else. The label is percent-encoded.
pub fn share_link(address: &str, sats: u64, shop_name: &str) -> String {
    let whole = sats / 100_000_000;
    let frac = sats % 100_000_000;
    let amount = if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{}", format!("{frac:08}").trim_end_matches('0'))
    };
    let mut out = format!("bitcoin:{address}");
    out.push_str(if sats == 0 { "?" } else { "?amount=" });
    if sats != 0 {
        out.push_str(&amount);
        out.push('&');
    }
    out.push_str("label=");
    for b in shop_name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
