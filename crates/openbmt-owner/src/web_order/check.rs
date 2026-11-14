//! WO-22: what the owner phone checks of the orders a store reports.

use std::collections::BTreeMap;

use openbmt_json::{Limits, Value};

use super::range::Range;
use super::WebOrderRefusal;

/// The most bytes of the store's list of orders that are read.
pub(super) const MAX_ORDERS_BYTES: usize = 256 * 1024;

/// One order as the store reports it (WO-20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreOrder {
    /// The order number.
    pub order: u64,
    /// The address the store gave the customer.
    pub address: String,
    /// The amount in satoshis.
    pub sats: u64,
    /// When the quote was made, in Unix seconds.
    pub quoted_at: u64,
    /// The order total in the minor units of the store's currency.
    pub total_minor: u64,
    /// The store's currency code.
    pub currency: String,
}

impl StoreOrder {
    /// Reads the store's reply to a request for orders:
    /// `{"orders":[{"order":1,"address":"..","sats":1,"quoted_at":1,
    /// "total_minor":1,"currency":"USD"}]}`. Any other shape is refused.
    pub fn parse_list(text: &[u8]) -> Result<Vec<Self>, WebOrderRefusal> {
        let bad = WebOrderRefusal::Orders;
        let limits = Limits {
            max_bytes: MAX_ORDERS_BYTES,
            max_depth: 4,
            max_values: 4096,
        };
        let doc = openbmt_json::parse(text, &limits).map_err(|_| bad)?;
        let list = doc.get("orders").and_then(Value::as_array).ok_or(bad)?;
        let mut out = Vec::with_capacity(list.len());
        for item in list {
            let number = |key: &str| item.get(key).and_then(Value::as_u64).ok_or(bad);
            let text = |key: &str| item.get(key).and_then(Value::as_str).ok_or(bad);
            out.push(Self {
                order: number("order")?,
                address: text("address")?.to_string(),
                sats: number("sats")?,
                quoted_at: number("quoted_at")?,
                total_minor: number("total_minor")?,
                currency: text("currency")?.to_string(),
            });
        }
        Ok(out)
    }
}

/// Why an order is refused and shown to the owner (WO-22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderRefusal {
    /// The address was not in a range the owner phone sent.
    AddressNotSent,
    /// The address belongs to another order already.
    AddressUsedBefore,
    /// The order was reported before with another address.
    OrderChanged,
    /// The amount is zero.
    NoAmount,
}

impl OrderRefusal {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        "WO-22"
    }
}

/// What a checked order is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderCheck {
    /// An order not seen before: the owner phone records it as a request.
    New,
    /// An order seen before with the same address: nothing to do.
    Known,
}

/// The addresses the owner phone has sent to a store and the orders it has
/// taken for them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SentRanges {
    sent: BTreeMap<String, u32>,
    used: BTreeMap<String, u64>,
    orders: BTreeMap<u64, String>,
}

impl SentRanges {
    /// Nothing sent yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a range as sent (WO-12). A range with an address that was
    /// already sent is refused and changes nothing.
    pub fn add_range(&mut self, range: &Range) -> Result<(), WebOrderRefusal> {
        if range.addresses.iter().any(|a| self.sent.contains_key(a)) {
            return Err(WebOrderRefusal::Range);
        }
        for (i, a) in range.addresses.iter().enumerate() {
            let offset = u32::try_from(i).map_err(|_| WebOrderRefusal::Range)?;
            let index = range
                .start
                .checked_add(offset)
                .ok_or(WebOrderRefusal::Range)?;
            self.sent.insert(a.clone(), index);
        }
        Ok(())
    }

    /// The addresses sent, with their indexes in the chain.
    pub fn sent_count(&self) -> usize {
        self.sent.len()
    }

    /// WO-22: checks an order. One whose address was not in a range sent, or
    /// was used before by another order, is refused; a refusal changes
    /// nothing. A new order is recorded as using its address.
    pub fn check(&mut self, order: &StoreOrder) -> Result<OrderCheck, OrderRefusal> {
        if order.sats == 0 {
            return Err(OrderRefusal::NoAmount);
        }
        if let Some(address) = self.orders.get(&order.order) {
            return if *address == order.address {
                Ok(OrderCheck::Known)
            } else {
                Err(OrderRefusal::OrderChanged)
            };
        }
        if !self.sent.contains_key(&order.address) {
            return Err(OrderRefusal::AddressNotSent);
        }
        if self.used.contains_key(&order.address) {
            return Err(OrderRefusal::AddressUsedBefore);
        }
        self.used.insert(order.address.clone(), order.order);
        self.orders.insert(order.order, order.address.clone());
        Ok(OrderCheck::New)
    }
}
