//! Tests that drive the exported API the way the Android app does, through
//! an in-memory `KeyStore`. The owner crate builds the requests.

mod allowlist;
mod boxpair;
mod call;
mod ceremony;
mod deposit;
mod history;
mod kit;
mod kit_scan;
mod kotlin;
mod lock;
mod protocol;
mod screen;
mod session;
mod setup;
pub(crate) mod wire;
mod words;
