//! Shop Lightning: the shop's own Lightning node, built in
//! ([rules](../../../docs/SHOP-LIGHTNING-RULES.md)).

#![forbid(unsafe_code)]

pub mod bip85;
pub mod chain;
pub mod device;
pub mod device_tcp;
pub mod logger;
pub mod provider;
pub mod room;
pub mod seal;
pub mod state;
pub mod store;
pub mod words;

#[cfg(test)]
mod testutil;
