//! The host name of a Tor v3 address (NR-92).
//!
//! The functions live in `openbmt_core::onion`, so that the signed settings
//! check the own node's name with the same code (NR-98, NR-105); they are
//! re-exported here as before.

pub use openbmt_core::onion::{onion_name, parse_onion_name, SUFFIX};
