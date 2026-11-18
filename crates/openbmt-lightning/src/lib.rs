//! The invoice and the code of a Lightning payment, for the counter.
//!
//! A sale's code can carry both an on-chain address and a Lightning invoice.
//! This crate checks the BOLT 11 invoice against the sale and the signed
//! settings and builds the BIP 321 code. The rules are in
//! `docs/LIGHTNING-RULES.md`; the tests name the rule they prove.
//!
//! - [`invoice`]: what is asked of an invoice and the facts of a payment that
//!   is settled (LN-13, LN-21).
//! - [`bolt11`]: the invoice, read here and checked against the sale and the
//!   signed settings (LN-16, LN-17).
//! - [`bip321`]: the payment code (LN-18, LN-19).
//!
//! There is no network, no clock, no storage and no TLS here. The node's
//! public key is part of the shop's settings
//! (`openbmt_core::settings::Settings::lightning_node`).
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

pub mod bip321;
pub mod bolt11;
pub mod error;
pub mod invoice;

/// An invoice writer for tests, here and in the crates that drive this one.
/// Only with the `testing` feature.
#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod tests;

pub use error::{AskRefused, InvoiceRefused, UriRefused};
pub use invoice::{Ask, PaidFacts};
