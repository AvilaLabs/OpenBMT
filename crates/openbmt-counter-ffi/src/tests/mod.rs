//! Tests that drive the exported API the way the Android app does, through
//! `dispatch`, on a node whose driver the test feeds by hand.

mod call;
mod currency;
mod e2e;
pub(crate) mod kit;
mod kotlin;
mod lightning;
mod link;
mod lock;
mod own_node;
mod pairing;
mod pay;
mod protocol;
mod rates;
mod screen;
mod staff;
mod strict;
pub(crate) mod wire;
