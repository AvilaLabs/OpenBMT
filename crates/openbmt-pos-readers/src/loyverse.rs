//! The Loyverse reader (PS-100 to PS-104): the path and query of a request
//! for the receipts updated since a time and the reading of its reply into a
//! page of orders. Read-only: nothing here creates, cancels or refunds a
//! receipt.
//!
//! Vendor documentation the fields come from, as it was rendered on
//! 2026-11-12: the Loyverse API reference at <https://developer.loyverse.com/docs/>
//! (the receipts, payment types and merchant endpoints, the pagination, the
//! date and time format and the error list). What that page does not state is
//! marked **[verify, Loyverse]** in `docs/POS-RULES.md` (PS-100 to PS-104).

use openbmt_counter::Currency;
use openbmt_json::Value;
use openbmt_pos::{
    Cursor, Id, OrderState, Page, PosOrder, PosTender, Settings, TenderKind, TenderState, Text,
    MAX_PAGE_ORDERS, MAX_TENDERS,
};

use crate::clover::percent_encode;
use crate::common::{
    amount, array, currency, cut, id, join_cursor, name, newest, note, object, opt, parse_doc, req,
    rfc3339, same_name, split_cursor, string, whole_text,
};
use crate::error::{ReadRefused, RequestRefused};
use crate::square::no_service_error;
use crate::time::{format_rfc3339, MAX_TIME};

/// PS-100: the path of the list of receipts. **[verify, Loyverse]**
pub const RECEIPTS_PATH: &str = "/v1.0/receipts";

/// PS-100: the path of the merchant, which names the currency and its
/// decimal places.
pub const MERCHANT_PATH: &str = "/v1.0/merchant/";

/// PS-100: the most receipts Loyverse returns in a page.
pub const PAGE_LIMIT: usize = 250;

/// The most decimal places of a currency the reader takes from the merchant.
const MAX_DECIMAL_PLACES: u64 = 9;

/// PS-100: the path and query of the list of payment types, the deleted ones
/// too, so that a payment of a type that was deleted since still has a name.
/// **[verify, Loyverse]**
pub fn payment_types_path() -> String {
    "/v1.0/payment_types?show_deleted=true".to_string()
}

/// PS-100: the path and query of the receipts with these numbers (at most
/// 250), the sales that refunds name. **[verify, Loyverse]**
pub fn sales_path(numbers: &[Id]) -> Result<String, RequestRefused> {
    if numbers.is_empty() || numbers.len() > PAGE_LIMIT {
        return Err(RequestRefused::BadLimit);
    }
    let list: Vec<String> = numbers.iter().map(|n| percent_encode(n.as_str())).collect();
    Ok(format!(
        "{RECEIPTS_PATH}?receipt_numbers={}&limit={PAGE_LIMIT}",
        list.join("%2C")
    ))
}

/// PS-104: the currency of an account and the decimal places of its money
/// amounts, from the reply of the merchant request.
pub fn read_merchant(body: &[u8]) -> Result<(Currency, u8), ReadRefused> {
    let doc = parse_doc(body)?;
    object(&doc, "reply")?;
    no_service_error(&doc)?;
    let c = object(req(&doc, "currency", "currency")?, "currency")?;
    let code = currency(
        string(req(c, "code", "currency.code")?, "currency.code")?,
        "currency.code",
    )?;
    let places = amount(
        req(c, "decimal_places", "currency.decimal_places")?,
        "currency.decimal_places",
    )?;
    if places > MAX_DECIMAL_PLACES {
        return Err(ReadRefused::OutOfRange("currency.decimal_places"));
    }
    Ok((code, u8::try_from(places).unwrap_or(0)))
}

/// A payment type of the shop: its id, its name and its type, if given.
struct PayType {
    id: String,
    name: String,
    kind: Option<String>,
}

/// PS-102: the payment types of a reply of the payment types request. An id
/// that is in the reply twice is read once, from the later.
fn read_payment_types(doc: &Value) -> Result<Vec<PayType>, ReadRefused> {
    object(doc, "payment types reply")?;
    no_service_error(doc)?;
    let list = array(req(doc, "payment_types", "payment_types")?, "payment_types")?;
    let mut out: Vec<PayType> = Vec::with_capacity(list.len());
    for v in list {
        let t = object(v, "payment_types[]")?;
        let t_id = string(req(t, "id", "payment_types[].id")?, "payment_types[].id")?;
        let t_name = string(
            req(t, "name", "payment_types[].name")?,
            "payment_types[].name",
        )?;
        let kind = match opt(t, "type") {
            Some(k) => Some(string(k, "payment_types[].type")?.to_string()),
            None => None,
        };
        out.retain(|p| p.id != t_id);
        out.push(PayType {
            id: t_id.to_string(),
            name: t_name.to_string(),
            kind,
        });
    }
    Ok(out)
}

/// One request for the receipts of one poll, and the reading of its reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    bitcoin_names: Vec<Text>,
    currency: Currency,
    /// What a whole unit of the currency is in minor units; `None` when the
    /// decimal places are too many to count in a `u64`.
    scale: Option<u64>,
    since: u64,
    /// The newest change of the first page of this sync, from the cursor's
    /// marker; `None` on the first page.
    first_newest: Option<u64>,
    vendor_cursor: Option<String>,
}

/// A receipt as read.
enum Read {
    Sale(Box<Sale>),
    Refund(Refund),
    /// A receipt of a kind the reader does not know that carries no Bitcoin
    /// payment, or a refund that was cancelled.
    Left,
}

/// A sale receipt as an order, with the payment type of each of its tenders.
struct Sale {
    order: PosOrder,
    type_ids: Vec<String>,
}

/// A refund receipt: the sale it names, when it changed and what it paid
/// back.
struct Refund {
    sale: Id,
    updated_at: u64,
    payments: Vec<RefundPayment>,
}

struct RefundPayment {
    type_id: String,
    amount_minor: u64,
    /// The payment's name is a Bitcoin tender name, or is not known.
    maybe_bitcoin: bool,
}

impl Request {
    /// A request for the receipts updated since the time the cursor holds,
    /// at the largest page size, with the vendor's marker of the cursor when
    /// it holds one. `currency` and `decimal_places` are the account's own
    /// (the reply of the merchant request, [`read_merchant`]): a receipt
    /// carries no currency. The settings give the names of the Bitcoin
    /// tenders (PS-10); every store of the account is read, and each receipt
    /// names its own store.
    pub fn new(
        settings: &Settings,
        currency: Currency,
        decimal_places: u8,
        cursor: &Cursor,
    ) -> Result<Self, RequestRefused> {
        if settings.locations.is_empty() {
            return Err(RequestRefused::NoLocations);
        }
        let (since, marker) = split_cursor(cursor)?;
        // The start time must be writable as a time.
        format_rfc3339(since).ok_or(RequestRefused::BadCursor)?;
        // The marker is the newest change of the first page, a tilde and
        // Loyverse's own cursor (PS-101).
        let (first_newest, vendor_cursor) = match marker {
            Some(m) => {
                let (time, vendor) = m.split_once('~').ok_or(RequestRefused::BadCursor)?;
                let canonical = !time.is_empty()
                    && time.bytes().all(|b| b.is_ascii_digit())
                    && (time == "0" || !time.starts_with('0'));
                let time: u64 = time.parse().map_err(|_| RequestRefused::BadCursor)?;
                if !canonical || time > MAX_TIME || vendor.is_empty() {
                    return Err(RequestRefused::BadCursor);
                }
                (Some(time), Some(vendor.to_string()))
            }
            None => (None, None),
        };
        Ok(Self {
            bitcoin_names: settings.bitcoin_tenders.clone(),
            currency,
            scale: 10u64.checked_pow(u32::from(decimal_places)),
            since,
            first_newest,
            vendor_cursor,
        })
    }

    /// PS-100: the path and the query, ready to join to the host:
    /// `/v1.0/receipts?updated_at_min=…&limit=250` and, when the cursor holds
    /// the vendor's marker, `&cursor=…`. The time is the cursor's, written
    /// `YYYY-MM-DDTHH:MM:SS.000Z`.
    pub fn path_and_query(&self) -> String {
        // `new` checked that the time can be written.
        let at = format_rfc3339(self.since).unwrap_or_default();
        let at = format!("{}.000Z", at.trim_end_matches('Z'));
        let mut out = format!(
            "{RECEIPTS_PATH}?updated_at_min={}&limit={PAGE_LIMIT}",
            percent_encode(&at)
        );
        if let Some(c) = &self.vendor_cursor {
            out.push_str("&cursor=");
            out.push_str(&percent_encode(c));
        }
        out
    }

    /// PS-103: the numbers of the sales that the refunds of a reply name and
    /// that the reply does not hold. The caller asks for them with
    /// [`sales_path`] and hands the reply to [`Request::parse`].
    pub fn refunded_sales(&self, receipts: &[u8]) -> Result<Vec<Id>, ReadRefused> {
        let doc = parse_doc(receipts)?;
        object(&doc, "reply")?;
        no_service_error(&doc)?;
        let list = receipt_list(&doc)?;
        let mut held: Vec<&str> = Vec::new();
        let mut named: Vec<Id> = Vec::new();
        for v in list {
            let o = object(v, "receipts[]")?;
            let number = string(
                req(o, "receipt_number", "receipts[].receipt_number")?,
                "receipts[].receipt_number",
            )?;
            match string(
                req(o, "receipt_type", "receipts[].receipt_type")?,
                "receipts[].receipt_type",
            )? {
                "REFUND" if opt(o, "cancelled_at").is_none() => {
                    let sale = id(
                        string(
                            req(o, "refund_for", "receipts[].refund_for")?,
                            "receipts[].refund_for",
                        )?,
                        "receipts[].refund_for",
                    )?;
                    if !named.contains(&sale) {
                        named.push(sale);
                    }
                }
                _ => held.push(number),
            }
        }
        named.retain(|n| !held.contains(&n.as_str()));
        Ok(named)
    }

    /// PS-101: read the reply into a page. `payment_types` is the reply of the
    /// payment types request, where a payment that carries no name is named;
    /// `sales` is the reply of the request [`sales_path`] makes, where a
    /// refund finds a sale that is not in the page. A reply with a cursor has
    /// more. The orders of the page are oldest first by last change, then by
    /// number.
    pub fn parse(
        &self,
        receipts: &[u8],
        payment_types: Option<&[u8]>,
        sales: Option<&[u8]>,
    ) -> Result<Page, ReadRefused> {
        let doc = parse_doc(receipts)?;
        object(&doc, "reply")?;
        no_service_error(&doc)?;
        let types = match payment_types {
            Some(body) => read_payment_types(&parse_doc(body)?)?,
            None => Vec::new(),
        };
        let list = receipt_list(&doc)?;
        let mut held: Vec<Sale> = Vec::new();
        let mut refunds: Vec<Refund> = Vec::new();
        for v in list {
            match self.read_receipt(v, &types)? {
                Read::Sale(s) => held.push(*s),
                Read::Refund(r) => refunds.push(r),
                Read::Left => {}
            }
        }
        let mut found: Vec<Sale> = Vec::new();
        if let Some(body) = sales {
            let other = parse_doc(body)?;
            object(&other, "sales reply")?;
            no_service_error(&other)?;
            let other_list = array(req(&other, "receipts", "receipts")?, "receipts")?;
            if other_list.len() > MAX_PAGE_ORDERS {
                return Err(ReadRefused::TooManyOrders);
            }
            for v in other_list {
                if let Read::Sale(s) = self.read_receipt(v, &types)? {
                    found.push(*s);
                }
            }
        }
        // The sales a refund changed, from the page and from `found`.
        let mut changed: Vec<Sale> = Vec::new();
        for r in &refunds {
            self.apply_refund(r, &mut held, &mut found, &mut changed)?;
        }
        let mut orders: Vec<PosOrder> = held.into_iter().map(|s| s.order).collect();
        orders.extend(changed.into_iter().map(|s| s.order));
        for o in &orders {
            o.validate()?;
        }
        orders.sort_by(|a, b| (a.updated_at, &a.id).cmp(&(b.updated_at, &b.id)));
        let more = match opt(&doc, "cursor") {
            Some(v) => {
                let c = string(v, "cursor")?;
                if c.is_empty() || !c.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
                    return Err(ReadRefused::OutOfRange("cursor"));
                }
                Some(c)
            }
            None => None,
        };
        // PS-101: the place the next sync starts from is the newest change of
        // the first page, kept in the marker while there are more pages.
        let first = match self.first_newest {
            Some(t) => t,
            None => newest(self.since, orders.iter().map(|o| o.updated_at)),
        };
        let next = match more {
            Some(c) => join_cursor(self.since, Some(&format!("{first}~{c}")))?,
            None => join_cursor(first.max(self.since), None)?,
        };
        Ok(Page {
            orders,
            next,
            more: more.is_some(),
        })
    }

    /// PS-103: a refund receipt is a refund of the sale it names. The sale
    /// is taken from the page, else from the sales that were asked for; a
    /// sale taken from there joins `changed` once. Each payment of the refund
    /// is added to the refunded amount of a tender of that payment type.
    fn apply_refund(
        &self,
        refund: &Refund,
        held: &mut [Sale],
        found: &mut Vec<Sale>,
        changed: &mut Vec<Sale>,
    ) -> Result<(), ReadRefused> {
        let in_held = held.iter().position(|s| s.order.id == refund.sale);
        let in_changed = changed.iter().position(|s| s.order.id == refund.sale);
        let sale: &mut Sale = if let Some(i) = in_held {
            held.get_mut(i).ok_or(ReadRefused::RefundWithoutTender)?
        } else if let Some(i) = in_changed {
            changed.get_mut(i).ok_or(ReadRefused::RefundWithoutTender)?
        } else if let Some(i) = found.iter().position(|s| s.order.id == refund.sale) {
            changed.push(found.remove(i));
            changed.last_mut().ok_or(ReadRefused::RefundWithoutTender)?
        } else {
            // The sale is nowhere: only a refund that may be of a Bitcoin
            // payment matters.
            return if refund.payments.iter().any(|p| p.maybe_bitcoin) {
                Err(ReadRefused::RefundWithoutTender)
            } else {
                Ok(())
            };
        };
        for p in &refund.payments {
            if p.amount_minor == 0 {
                return Err(ReadRefused::OutOfRange("payments[].money_amount"));
            }
            let mut of_type = sale
                .type_ids
                .iter()
                .zip(sale.order.tenders.iter_mut())
                .filter(|(t, _)| **t == p.type_id)
                .map(|(_, tender)| tender)
                .peekable();
            if of_type.peek().is_none() {
                if p.maybe_bitcoin {
                    return Err(ReadRefused::RefundWithoutTender);
                }
                continue;
            }
            // The first tender of that type that has room for the refund.
            let mut done = false;
            for tender in of_type {
                if let TenderState::Voided = tender.state {
                    done = true;
                    break;
                }
                let held_back = match tender.state {
                    TenderState::Refunded { amount_minor } => amount_minor,
                    _ => 0,
                };
                let room = u128::from(tender.amount_minor) + u128::from(tender.tip_minor)
                    - u128::from(held_back);
                if u128::from(p.amount_minor) <= room {
                    let sum = held_back
                        .checked_add(p.amount_minor)
                        .ok_or(ReadRefused::OutOfRange("payments[].money_amount"))?;
                    tender.state = TenderState::Refunded { amount_minor: sum };
                    done = true;
                    break;
                }
            }
            if !done {
                return Err(ReadRefused::RefundTooLarge);
            }
        }
        sale.order.updated_at = sale.order.updated_at.max(refund.updated_at);
        Ok(())
    }

    /// PS-104: a money amount from a number of the reply, in minor units.
    /// Loyverse writes money in whole units and decimals; the reader takes
    /// only what `openbmt-json` keeps exactly, an integer, and scales it.
    fn minor(&self, v: &Value, what: &'static str) -> Result<u64, ReadRefused> {
        let whole = amount(v, what)?;
        let scale = self.scale.ok_or(ReadRefused::OutOfRange(what))?;
        let n = whole
            .checked_mul(scale)
            .ok_or(ReadRefused::OutOfRange(what))?;
        i64::try_from(n).map_err(|_| ReadRefused::OutOfRange(what))?;
        Ok(n)
    }

    /// PS-10: whether a payment's name is one of the Bitcoin tender names.
    fn is_bitcoin(&self, label: &str) -> bool {
        self.bitcoin_names.iter().any(|n| same_name(cut(label), n))
    }

    fn read_receipt(&self, v: &Value, types: &[PayType]) -> Result<Read, ReadRefused> {
        let o = object(v, "receipts[]")?;
        let number = id(
            string(
                req(o, "receipt_number", "receipts[].receipt_number")?,
                "receipts[].receipt_number",
            )?,
            "receipts[].receipt_number",
        )?;
        match string(
            req(o, "receipt_type", "receipts[].receipt_type")?,
            "receipts[].receipt_type",
        )? {
            "SALE" => self.read_sale(o, number, types),
            "REFUND" => self.read_refund(o, types),
            _ => {
                // PS-91: a kind that is not known matters only to a receipt
                // that carries a Bitcoin payment, or one that may.
                for p in payments(o)? {
                    let p = object(p, "receipts[].payments[]")?;
                    if resolve_name(p, types)?.is_none_or(|n| self.is_bitcoin(n)) {
                        return Err(ReadRefused::UnknownValue("receipts[].receipt_type"));
                    }
                }
                Ok(Read::Left)
            }
        }
    }

    /// PS-101: a sale receipt is an order: closed, or voided when it was
    /// cancelled.
    fn read_sale(&self, o: &Value, number: Id, types: &[PayType]) -> Result<Read, ReadRefused> {
        let location = whole_text(
            string(
                req(o, "store_id", "receipts[].store_id")?,
                "receipts[].store_id",
            )?,
            "receipts[].store_id",
        )?;
        let total_minor = self.minor(
            req(o, "total_money", "receipts[].total_money")?,
            "receipts[].total_money",
        )?;
        let opened_at = rfc3339(
            req(o, "created_at", "receipts[].created_at")?,
            "receipts[].created_at",
        )?;
        let updated_at = rfc3339(
            req(o, "updated_at", "receipts[].updated_at")?,
            "receipts[].updated_at",
        )?;
        if let Some(d) = opt(o, "receipt_date") {
            rfc3339(d, "receipts[].receipt_date")?;
        }
        let cancelled = match opt(o, "cancelled_at") {
            Some(c) => Some(rfc3339(c, "receipts[].cancelled_at")?),
            None => None,
        };
        if let Some(r) = opt(o, "refund_for") {
            string(r, "receipts[].refund_for")?;
        }
        let said = match opt(o, "note") {
            Some(n) => note(string(n, "receipts[].note")?),
            None => None,
        };
        let order_name = match opt(o, "order") {
            Some(n) => note(string(n, "receipts[].order")?),
            None => None,
        };
        let list = payments(o)?;
        if list.len() > MAX_TENDERS {
            return Err(ReadRefused::TooManyTenders);
        }
        let mut tenders = Vec::with_capacity(list.len());
        let mut type_ids: Vec<String> = Vec::with_capacity(list.len());
        for p in list {
            let (tender, type_id) = self.read_payment(p, &type_ids, types)?;
            tenders.push(tender);
            type_ids.push(type_id);
        }
        let order = PosOrder {
            id: number,
            location,
            state: if cancelled.is_some() {
                OrderState::Voided
            } else {
                OrderState::Closed
            },
            currency: self.currency,
            total_minor,
            tenders,
            opened_at,
            closed_at: None,
            updated_at,
            reference: said.or(order_name),
        };
        order.validate()?;
        Ok(Read::Sale(Box::new(Sale { order, type_ids })))
    }

    /// PS-102: a payment of a sale as a tender. A tender's id is its payment
    /// type id, and for a second payment of one type the id and `#2`, and so
    /// on; Loyverse gives a payment no id of its own. `before` holds the type
    /// ids of the payments before this one.
    fn read_payment(
        &self,
        v: &Value,
        before: &[String],
        types: &[PayType],
    ) -> Result<(PosTender, String), ReadRefused> {
        let p = object(v, "receipts[].payments[]")?;
        let type_id = string(
            req(p, "payment_type_id", "payments[].payment_type_id")?,
            "payments[].payment_type_id",
        )?;
        let label = resolve_name(p, types)?.ok_or(ReadRefused::Missing("payments[].name"))?;
        let kind_text = match opt(p, "type") {
            Some(k) => Some(string(k, "payments[].type")?),
            None => types
                .iter()
                .find(|t| t.id == type_id)
                .and_then(|t| t.kind.as_deref()),
        };
        let kind = match kind_text {
            Some("CASH") => TenderKind::Cash,
            Some("NONINTEGRATEDCARD") => TenderKind::Card,
            _ => TenderKind::Other,
        };
        let amount_minor = self.minor(
            req(p, "money_amount", "payments[].money_amount")?,
            "payments[].money_amount",
        )?;
        let at = rfc3339(
            req(p, "paid_at", "payments[].paid_at")?,
            "payments[].paid_at",
        )?;
        let nth = before.iter().filter(|t| *t == type_id).count() + 1;
        let tender_id = if nth == 1 {
            id(type_id, "payments[].payment_type_id")?
        } else {
            id(&format!("{type_id}#{nth}"), "payments[].payment_type_id")?
        };
        let tender = PosTender {
            id: tender_id,
            kind,
            name: name(label, "payments[].name")?,
            amount_minor,
            tip_minor: 0,
            state: TenderState::Paid,
            at,
            reference: None,
        };
        Ok((tender, type_id.to_string()))
    }

    /// PS-103: a refund receipt names its sale and what it paid back. One
    /// that was cancelled is not a refund.
    fn read_refund(&self, o: &Value, types: &[PayType]) -> Result<Read, ReadRefused> {
        let sale = id(
            string(
                req(o, "refund_for", "receipts[].refund_for")?,
                "receipts[].refund_for",
            )?,
            "receipts[].refund_for",
        )?;
        let updated_at = rfc3339(
            req(o, "updated_at", "receipts[].updated_at")?,
            "receipts[].updated_at",
        )?;
        if let Some(c) = opt(o, "cancelled_at") {
            rfc3339(c, "receipts[].cancelled_at")?;
            return Ok(Read::Left);
        }
        let list = payments(o)?;
        if list.len() > MAX_TENDERS {
            return Err(ReadRefused::TooManyTenders);
        }
        let mut out = Vec::with_capacity(list.len());
        for v in list {
            let p = object(v, "receipts[].payments[]")?;
            let type_id = string(
                req(p, "payment_type_id", "payments[].payment_type_id")?,
                "payments[].payment_type_id",
            )?;
            let amount_minor = self.minor(
                req(p, "money_amount", "payments[].money_amount")?,
                "payments[].money_amount",
            )?;
            let maybe_bitcoin = resolve_name(p, types)?.is_none_or(|n| self.is_bitcoin(n));
            out.push(RefundPayment {
                type_id: type_id.to_string(),
                amount_minor,
                maybe_bitcoin,
            });
        }
        Ok(Read::Refund(Refund {
            sale,
            updated_at,
            payments: out,
        }))
    }
}

/// PS-101: the `receipts` of a reply, at most one page of them.
fn receipt_list(doc: &Value) -> Result<&[Value], ReadRefused> {
    let list = array(req(doc, "receipts", "receipts")?, "receipts")?;
    if list.len() > PAGE_LIMIT {
        return Err(ReadRefused::TooManyOrders);
    }
    Ok(list)
}

/// The payments of a receipt; a receipt without any has none.
fn payments(o: &Value) -> Result<&[Value], ReadRefused> {
    match opt(o, "payments") {
        Some(v) => array(v, "receipts[].payments"),
        None => Ok(&[]),
    }
}

/// PS-102: the name of a payment: its own `name` when the reply carries it,
/// else the name of its payment type among the payment types handed in. `None`
/// when neither is there.
fn resolve_name<'a>(p: &'a Value, types: &'a [PayType]) -> Result<Option<&'a str>, ReadRefused> {
    if let Some(n) = opt(p, "name") {
        return Ok(Some(string(n, "payments[].name")?));
    }
    let Some(type_id) = opt(p, "payment_type_id") else {
        return Ok(None);
    };
    let type_id = string(type_id, "payments[].payment_type_id")?;
    Ok(types
        .iter()
        .find(|t| t.id == type_id)
        .map(|t| t.name.as_str()))
}
