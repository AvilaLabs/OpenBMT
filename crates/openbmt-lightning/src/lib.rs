//! Lightning invoices from the merchant's own node, for the counter.
//!
//! A shop that runs its own Lightning node (LND or Core Lightning) can show
//! a BIP 321 code with both an on-chain address and a Lightning invoice the
//! node made. This crate builds the requests the counter sends to the node,
//! reads the replies, checks the invoice the node returned and builds the
//! code. The rules are in `docs/LIGHTNING-RULES.md`; the tests name the rule
//! they prove.
//!
//! - [`credential`]: the node's address, the pinned certificate and the
//!   credential limited to invoices (LN-04 to LN-12).
//! - [`invoice`]: what is asked of the node and what it may answer (LN-13,
//!   LN-14, LN-21).
//! - [`node`]: the calls, for either kind of node, as a request and a reader
//!   of the reply (LN-07, LN-10, LN-15).
//! - [`bolt11`]: the invoice the node returned, read here and checked against
//!   the sale and the signed settings (LN-16, LN-17).
//! - [`bip321`]: the payment code (LN-18, LN-19).
//! - [`connect`]: the test of a connection at setup (LN-06, LN-09, LN-20).
//! - [`say`]: the words of every state and refusal (LN-22).
//!
//! There is no network, no clock, no storage and no TLS here: the app sends
//! each [`Request`] over HTTPS to the node, connecting only to a node that
//! shows the pinned certificate ([`credential::CertPin::accepts`]), and hands
//! the status and the bytes of the answer back. The credential is held by the
//! counter and is never part of the shop's settings; the node's public key
//! is (`openbmt_core::settings::Settings::lightning_node`).
//!
//! The fixtures of the tests are written from the projects' documents and
//! source code, not captured from a running node; the rules name the source
//! files the error answers and the replies were read from.
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
mod cln;
pub mod connect;
pub mod credential;
pub mod error;
pub mod invoice;
mod lnd;
pub mod node;
pub mod request;
pub mod say;
mod text;

#[cfg(test)]
mod tests;

pub use credential::{CertPin, ClnCredential, Credential, Endpoint, LndCredential, CLN_METHODS};
pub use error::{
    AskRefused, ConnectionRefused, CredentialRefused, InvoiceRefused, ReplyRefused, UriRefused,
};
pub use invoice::{Ask, CreateOutcome, Created, NotPaid, PaidFacts, Payment};
pub use request::{Method, Reply, Request};

/// The command that bakes the macaroon the counter needs (LN-05): the
/// permissions to make and to read invoices, and nothing else.
pub const LND_BAKE_COMMAND: &str = "lncli bakemacaroon invoices:read invoices:write";
