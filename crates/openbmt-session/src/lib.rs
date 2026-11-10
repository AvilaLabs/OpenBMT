//! The approval session file: the request the owner phone hands to the Key
//! app and the response the Key app hands back.
//!
//! The rules are in `docs/SESSION-RULES.md`, section 1 (AS-01 to AS-15);
//! the errors and the tests name the rule they belong to. Both files use the
//! strict framing of `openbmt_core::wire`: a four-byte magic, a version byte,
//! exact lengths and no bytes after the end. The library has no network, no
//! clock, no storage and no randomness.
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

mod error;
mod item;
mod request;
mod response;
mod text;

pub use error::{BindError, EncodeError, VerifyError};
pub use item::{Item, ItemKind, Label, OutputLabel, RequestItem, ShopValue};
pub use request::Request;
pub use response::{
    health_message, health_proof, DeclineCode, ItemResult, Payload, Response, Verdict,
};
pub use text::{label_text_ok, reason_text_ok};

/// AS-01: the magic of a request.
pub const MAGIC_REQUEST: [u8; 4] = *b"OBMF";
/// AS-01: the magic of a response.
pub const MAGIC_RESPONSE: [u8; 4] = *b"OBM2";
/// AS-02: the largest request or response, in bytes.
pub const MAX_SESSION_BYTES: usize = 8 * 1024 * 1024;
/// AS-04: most items in one request.
pub const MAX_ITEMS: usize = 64;
/// AS-07: most characters in a label's text.
pub const MAX_LABEL_CHARS: usize = 64;
/// AS-07: most bytes in a label's text.
pub const MAX_LABEL_BYTES: usize = 255;
/// AS-12: most bytes in a decline's text.
pub const MAX_REASON_BYTES: usize = 160;
/// AS-06: most labels on one spend (a transaction has at most 100 outputs).
pub const MAX_OUTPUT_LABELS: usize = 100;
