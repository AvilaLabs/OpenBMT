//! The readers of two POS vendors' orders: Square and Clover.
//!
//! Each builds the text of one request for the orders that changed since a
//! cursor and reads the reply into a page of the model of `openbmt-pos`
//! (PS-80), or refuses it whole. The rules are in `docs/POS-RULES.md`,
//! section 9 (PS-90 to PS-99); the tests name the rule they prove.
//!
//! - [`square`]: the body of a SearchOrders request and the reading of its
//!   reply (PS-95, PS-96). Read-only.
//! - [`clover`]: the path and query of a request for a merchant's orders and
//!   the reading of its reply (PS-97, PS-98).
//!
//! There is no network, no clock, no storage and no sign-in here: the caller
//! sends the request with the sign-in it holds, hands the bytes of the reply
//! in, and stores the page. A field that is missing, of the wrong type, out
//! of range or of a kind the reader does not know is refused, never guessed
//! at; fields the reader does not need are ignored, whatever they are.
//!
//! The fixtures of the tests are written from the vendors' documentation,
//! not captured from a real account; every point that needs one is marked
//! **[verify, E5]** (Square) or **[verify, E6]** (Clover) in the rules.
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

pub mod clover;
mod common;
pub mod error;
pub mod say;
pub mod square;
pub mod time;

#[cfg(test)]
mod tests;

pub use common::cursor_at;
pub use error::{ReadRefused, RequestRefused, Shown};
