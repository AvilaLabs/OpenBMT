//! Tests that drive the exported API the way the Android app does, through
//! `dispatch`, on a node whose drivers the test feeds by hand.

mod books;
mod box_e2e;
mod call;
mod ceremony;
mod counter;
mod e2e;
pub(crate) mod fakes;
mod float;
mod import;
pub(crate) mod kit;
mod kotlin;
mod lock;
mod own;
mod own_box;
mod pos;
mod price;
mod protocol;
mod requests;
mod screen;
mod settings;
mod setup_lock;
mod shop_lightning;
mod starter;
mod strict;
mod watch;
mod web_orders;
pub(crate) mod wire;
