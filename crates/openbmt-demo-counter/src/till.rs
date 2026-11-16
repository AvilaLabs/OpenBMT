//! The till of the demo shop: what the shop's Box tells the counter when the
//! till asks for a sale. The page plays the Box and the register; the node
//! judges the request as it judges one from the Box's link (CR-266).

use openbmt_counter::{Amount, Currency};
use openbmt_counter_node::TillTaken;
use openbmt_pos::{SaleRequest, TillMessage};

use crate::record::demo_box;
use crate::registry;

/// The till's sale of the demo: the id the Box gives it.
pub const TILL_SALE: [u8; 16] = *b"demo-register-01";
/// What the till calls its order.
pub const REFERENCE: &str = "Order 1042";
/// Seconds a request waits before it lapses.
pub const WAITS_SECS: u64 = 900;

/// The Box asks the running counter to open a sale of `amount_minor` dollar
/// cents for the till's order; the till's sale id. Refused when no node runs,
/// or when the node drops the request.
pub fn request(amount_minor: u64) -> Result<[u8; 16], String> {
    let entry = registry()
        .only_entry()
        .ok_or_else(|| "one counter must be running".to_string())?;
    let from = demo_box().ok_or_else(|| "the Box key could not be made".to_string())?;
    let currency = Currency::new(*b"USD").ok_or_else(|| "the currency is not valid".to_string())?;
    let content = TillMessage::Request(SaleRequest {
        sale: TILL_SALE,
        amount: Amount(amount_minor),
        currency,
        reference: REFERENCE.to_string(),
        expires_at: crate::now().saturating_add(WAITS_SECS),
    })
    .to_bytes();
    match entry.take_till_notice(&from.device_id(), &content) {
        Ok(TillTaken::Waiting) => Ok(TILL_SALE),
        Ok(other) => Err(format!("the counter did not wait for it: {other:?}")),
        Err(e) => Err(e.to_string()),
    }
}
