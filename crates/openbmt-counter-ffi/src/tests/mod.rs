//! Tests that drive the exported API the way the Android app does, through
//! `dispatch`, on a node whose driver the test feeds by hand.

mod bills;
mod call;
mod confirm;
mod currency;
mod e2e;
pub(crate) mod kit;
mod kotlin;
mod lightning;
mod link;
mod lock;
mod own_node;
mod own_owner;
mod pairing;
mod pairing_own;
mod pay;
mod protocol;
mod rates;
mod requests;
mod screen;
mod staff;
mod strict;
mod swap;
mod switch;
mod tap;
mod till;
mod tips;
pub(crate) mod wire;
