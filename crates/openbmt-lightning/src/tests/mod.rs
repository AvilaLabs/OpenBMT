//! Tests, named after the rule each proves.

mod bip321;
mod bolt11;
mod meta;
mod payment;
mod settings;

pub(crate) use crate::testing::*;

use bitcoin::Network;

use crate::invoice::Ask;
