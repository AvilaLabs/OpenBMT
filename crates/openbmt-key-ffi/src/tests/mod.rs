//! Tests that drive the exported API the way the Android app does, through
//! an in-memory `KeyStore`. The owner crate builds the requests.

mod call;
mod ceremony;
mod deposit;
mod kit;
mod kit_scan;
mod kotlin;
mod protocol;
mod screen;
mod session;
mod setup;
pub(crate) mod wire;
mod words;
