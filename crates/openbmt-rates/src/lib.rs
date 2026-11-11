//! The price of bitcoin from several sources (`docs/RATE-RULES.md`).
//!
//! The app fetches the replies itself, over Tor, with the platform's own
//! TLS. Everything that reads an untrusted reply and decides is here:
//!
//! - [`parse_response`] reads the reply of one [`Source`] into a price in
//!   minor units per bitcoin, or refuses it;
//! - [`combine`] makes the [`Sample`]s of several sources into the one
//!   `Rate` and `RateSource` the counter prices a sale with (CR-57,
//!   CR-58), or refuses.
//!
//! It has no network, no clock, no storage and no screen. The schedule of
//! fetching is a rule for the app (RT-40 to RT-45); its numbers are here so
//! that the app and the tests read the same ones.

#![forbid(unsafe_code)]

pub mod combine;
pub mod error;
pub mod price;
pub mod say;
pub mod source;

pub use combine::{combine, label_for, Sample, LABEL_PREFIX, MAX_SPREAD_BPS, MIN_SOURCES};
pub use error::{ParseRefused, PriceRefused, RatesRefused};
pub use price::{parse_price_cents, MAX_CENTS_PER_BTC, MAX_DECIMALS, MIN_CENTS_PER_BTC};
pub use source::{parse_response, source, Shape, Source, SOURCES};

/// RT-10: the most bytes of a reply that are read.
pub const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// RT-11: the most levels of arrays and objects inside one another.
pub const MAX_DEPTH: usize = 16;

/// RT-40: the seconds between two fetches of one source. **[to tune]**
pub const FETCH_INTERVAL_SECS: u64 = 60;
/// RT-43: the seconds after which a fetch is given up. **[to tune]**
pub const FETCH_TIMEOUT_SECS: u64 = 20;

#[cfg(test)]
mod tests;
