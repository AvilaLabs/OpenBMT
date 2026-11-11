//! Tests that drive the exported API the way the Android app does, through
//! `dispatch`, on a node whose drivers the test feeds by hand.

mod call;
mod ceremony;
mod counter;
mod e2e;
pub(crate) mod kit;
mod kotlin;
mod lock;
mod pos;
mod protocol;
mod screen;
mod settings;
mod setup_lock;
mod strict;
pub(crate) mod wire;
