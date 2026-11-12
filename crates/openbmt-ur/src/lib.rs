//! Blockchain Commons Uniform Resources (BC-UR) for a PSBT too big for one
//! QR code (`docs/UR-RULES.md`): the animated sequence of codes that
//! Sparrow and hardware wallets read and write.
//!
//! - [`Encoder`] turns a message into `ur:` text, one part when it fits and
//!   an endless fountain-coded sequence when it does not;
//! - [`Decoder`] takes parts in any order, with repeats and losses, and
//!   gives the message back;
//! - [`psbt_to_ur_parts`] and [`ur_to_psbt`] are the `crypto-psbt` type;
//! - [`wsh_sortedmulti_cbor`] and [`output_to_ur_parts`] write the
//!   `crypto-output` of a shared wallet for a signer to read;
//! - [`ur_to_key_text`] reads a cosigner key from a `crypto-account`, a
//!   `crypto-hdkey` or a Coldcard-style JSON export.
//! - [`ur_to_wallet_text`] reads a supported mainnet wallet descriptor;
//!   [`WalletDecoder`] joins and validates its animated codes within a
//!   finite scan budget.
//!
//! No dependencies and no clock, no storage and no screen: the app draws
//! each part as a QR code and shows how far a scan has come.

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

mod base58;
pub mod bytewords;
pub mod cbor;
mod cbor_read;
pub mod decoder;
pub mod encoder;
pub mod error;
pub mod fountain;
pub mod hash;
mod json_read;
mod key;
mod output;
mod point;
mod psbt;
mod wallet;
mod wallet_decoder;
mod wallet_text;
mod xoshiro;

pub use decoder::{Decoder, Progress, MAX_TEXT_LEN};
pub use encoder::{check_type, Encoder};
pub use error::UrError;
pub use fountain::{MAX_FRAGMENT_LEN, MAX_MESSAGE_LEN, MAX_SEQ_LEN, MIN_FRAGMENT_LEN};
pub use key::{ur_to_key_text, KeyRefusal, KeyText, KeyWords, ALL_KEY_REFUSALS, MAX_KEY_CBOR};
pub use output::{
    output_to_ur_parts, text_to_ur_parts, wsh_sortedmulti_cbor, OutputKey, MAX_OUTPUT_KEYS,
    TYPE_BYTES, TYPE_CRYPTO_OUTPUT,
};
pub use psbt::{psbt_to_ur_parts, ur_to_psbt, TYPE_CRYPTO_PSBT, TYPE_PSBT};
pub use wallet::{ur_to_wallet_text, WalletRefusal, WalletText, MAX_WALLET_CBOR};
pub use wallet_decoder::{WalletDecoder, MAX_WALLET_PARTS, MAX_WALLET_SEQ_LEN, MAX_WALLET_TEXT};

#[cfg(test)]
mod tests;
