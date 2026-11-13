//! Pure, network-free OpenBMT logic.
//!
//! This crate holds the rules that must be identical on every device: how the
//! 2-of-3 wallet is described, how address batches are made and checked, how
//! records and settings are signed, and when a payment may be accepted at
//! once. It does no I/O, keeps no clock and has no async code.

#![forbid(unsafe_code)]

pub mod approval;
pub mod batch;
pub mod bip321;
pub mod boxpair;
pub mod bundle;
pub mod command;
pub mod confirmed;
pub mod currency;
pub mod entropy;
pub mod font;
mod hashing;
pub mod heartbeat;
pub mod hkdf;
pub mod identity;
pub mod mix;
pub mod onion;
pub mod pairing;
pub mod pairing_own;
pub mod pinned_wallet;
pub mod policy;
pub mod records;
pub mod relay;
pub mod say;
pub mod say_policy;
pub mod settings;
pub mod sha3;
pub mod shop_lightning;
pub mod starter_line;
pub mod wallet;
pub mod wallet_single;
pub mod wire;
pub mod words;

#[cfg(test)]
pub(crate) mod testutil;
