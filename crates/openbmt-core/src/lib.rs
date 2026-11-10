//! Pure, network-free OpenBMT logic.
//!
//! This crate holds the rules that must be identical on every device: how the
//! 2-of-3 wallet is described, how address batches are made and checked, how
//! records and settings are signed, and when a payment may be accepted at
//! once. It does no I/O, keeps no clock and has no async code.

#![forbid(unsafe_code)]

pub mod batch;
pub mod entropy;
mod hashing;
pub mod identity;
pub mod pairing;
pub mod policy;
pub mod records;
pub mod settings;
pub mod wallet;
pub mod wire;
pub mod words;

#[cfg(test)]
pub(crate) mod testutil;
