//! The stick key maker's core and screens ([rules](../../docs/STICK-RULES.md)).
//!
//! The library has no I/O: it mixes chance (SK-20 to SK-22), makes the words
//! and the public result (SK-23, SK-26), the Compact SeedQR (SK-27), checks
//! typed words (SK-28), makes the BIP-85 keys of the shop's everyday wallets
//! (SK-32), judges and signs a payment read from the camera's frames
//! (SK-52 to SK-56), runs the screens (SK-25 to SK-29) and draws them
//! (SK-14). The program that runs as process 1 is `src/main.rs` and
//! `src/sys.rs`; no code in this crate opens a network connection (SK-12).

#![forbid(unsafe_code)]

pub mod app;
pub mod bip85;
pub mod check;
pub mod draw;
pub mod font;
pub mod frames;
pub mod keys;
pub mod mix;
pub mod modlist;
pub mod mounts;
pub mod outputs;
pub mod screen;
pub mod seedqr;
pub mod sign;
pub mod typed;
pub mod words;
