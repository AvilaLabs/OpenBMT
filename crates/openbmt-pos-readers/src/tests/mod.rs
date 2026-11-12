//! Tests, named after the rule each proves.

mod clover;
mod common;
mod loyverse;
mod meta;
mod refusals;
mod say;
mod square;
mod time;

use openbmt_counter::Currency;
use openbmt_pos::{LocationCounters, Settings, Text};

/// The Square location and the Clover merchant of the fixtures.
pub(crate) const LOC: &str = "LOCSQ1";
pub(crate) const MERCHANT: &str = "CLMERCH0001";

pub(crate) fn text(s: &str) -> Text {
    Text::new(s).expect("a text")
}

pub(crate) fn usd() -> Currency {
    Currency::new(*b"USD").expect("a currency")
}

/// Settings that name "Bitcoin" as the Bitcoin tender, one Square location
/// and one Clover merchant, in dollars.
pub(crate) fn settings() -> Settings {
    Settings::new(
        vec![text("Bitcoin")],
        vec![
            LocationCounters {
                location: text(LOC),
                counters: vec![],
            },
            LocationCounters {
                location: text(MERCHANT),
                counters: vec![],
            },
        ],
        vec![usd()],
    )
    .expect("settings")
}
