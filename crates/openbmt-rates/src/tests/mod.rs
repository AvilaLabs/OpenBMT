//! Tests, named after the rule each proves.

mod combine;
mod fixtures;
mod json;
mod price;
mod say;
mod table;

use crate::source::{source, Source};

/// A source of the table, by label.
fn src(id: &str) -> &'static Source {
    source(id).expect("in the table")
}
