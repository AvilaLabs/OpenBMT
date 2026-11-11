//! Tests, named after the rule each proves.

mod bip321;
mod bolt11;
mod cln;
mod connect;
mod credential;
mod lnd;
mod meta;
mod payment;
mod say;
mod settings;

pub(crate) use crate::testing::*;

use bitcoin::Network;

use crate::credential::{CertPin, ClnCredential, Credential, Endpoint, LndCredential};
use crate::invoice::{Ask, Created};
use crate::request::Reply;
use crate::text::{base64, hex, Alphabet};
