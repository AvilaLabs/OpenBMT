//! The Square reader (PS-95, PS-96): the body of a SearchOrders request and
//! the reading of its reply into a page of orders. Read-only: nothing here
//! closes or changes an order (`docs/DESIGN.md` section 5.6).
//!
//! Vendor documentation the fields come from:
//! <https://developer.squareup.com/docs/orders-api/manage-orders/search-orders>,
//! <https://developer.squareup.com/reference/square/objects/Order>,
//! <https://developer.squareup.com/reference/square/objects/Tender>,
//! <https://developer.squareup.com/reference/square/objects/Refund>,
//! <https://developer.squareup.com/reference/square/objects/Payment>,
//! <https://developer.squareup.com/reference/square/objects/ExternalPaymentDetails>
//! and <https://developer.squareup.com/docs/build-basics/common-api-patterns/pagination>.
//! What those pages do not confirm is marked **[verify, E5]** in
//! `docs/POS-RULES.md` (PS-95, PS-96).

use openbmt_counter::Currency;
use openbmt_json::Value;
use openbmt_pos::{
    Cursor, OrderState, Page, PosOrder, PosTender, Settings, TenderKind, TenderState, Text,
    MAX_PAGE_ORDERS, MAX_TENDERS,
};

use crate::common::{
    amount, array, check_limit, currency, id, join_cursor, name, newest, note, object, opt,
    parse_doc, req, rfc3339, same_name, split_cursor, string, whole_text,
};
use crate::error::{ReadRefused, RequestRefused};
use crate::time::format_rfc3339;

/// The largest `version` of an order: Square documents it as an int32.
const MAX_VERSION: u64 = 2_147_483_647;

/// PS-95: the path of the request, which is a POST. **[verify, E5]**
pub const SEARCH_PATH: &str = "/v2/orders/search";

/// PS-96: the tender types Square documents or lists, with the name each is
/// given in the model and whether the type is `OTHER`. **[verify, E5]**
const TENDER_TYPES: [(&str, &str, TenderKind); 10] = [
    ("CARD", "Card", TenderKind::Card),
    ("THIRD_PARTY_CARD", "Third-party card", TenderKind::Card),
    ("CASH", "Cash", TenderKind::Cash),
    ("SQUARE_GIFT_CARD", "Gift card", TenderKind::Other),
    ("BANK_ACCOUNT", "Bank account", TenderKind::Other),
    ("WALLET", "Wallet", TenderKind::Other),
    ("BUY_NOW_PAY_LATER", "Pay later", TenderKind::Other),
    ("SQUARE_ACCOUNT", "Square account", TenderKind::Other),
    ("NO_SALE", "No sale", TenderKind::Other),
    ("OTHER", "Other", TenderKind::Other),
];

/// One SearchOrders request for one poll, and the reading of its reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    locations: Vec<String>,
    bitcoin_names: Vec<Text>,
    limit: usize,
    since: u64,
    vendor_cursor: Option<String>,
}

impl Request {
    /// A request for the shop's locations (the location text of the
    /// settings is Square's location id), at most `limit` orders a page
    /// (1 to 500), from the time and the vendor's marker the cursor holds.
    pub fn new(settings: &Settings, limit: usize, cursor: &Cursor) -> Result<Self, RequestRefused> {
        check_limit(limit)?;
        let locations: Vec<String> = settings
            .locations
            .iter()
            .map(|l| l.location.as_str().to_string())
            .collect();
        if locations.is_empty() {
            return Err(RequestRefused::NoLocations);
        }
        let (since, vendor_cursor) = split_cursor(cursor)?;
        // The start time must be writable as a time.
        format_rfc3339(since).ok_or(RequestRefused::BadCursor)?;
        Ok(Self {
            locations,
            bitcoin_names: settings.bitcoin_tenders.clone(),
            limit,
            since,
            vendor_cursor,
        })
    }

    /// PS-95: the JSON text of the body: the locations, the limit, the
    /// vendor's cursor when there is one, the states `COMPLETED` and
    /// `CANCELED`, the `updated_at` filter from the cursor's time, sorted by
    /// `UPDATED_AT` ascending.
    pub fn body(&self) -> String {
        let mut out = String::from("{\"location_ids\":[");
        for (i, l) in self.locations.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_json_string(&mut out, l);
        }
        out.push_str("],\"limit\":");
        out.push_str(&self.limit.to_string());
        if let Some(c) = &self.vendor_cursor {
            out.push_str(",\"cursor\":");
            push_json_string(&mut out, c);
        }
        out.push_str(
            ",\"query\":{\"filter\":{\"state_filter\":{\"states\":[\"COMPLETED\",\"CANCELED\"]},\
             \"date_time_filter\":{\"updated_at\":{\"start_at\":",
        );
        // `new` checked that the time can be written.
        push_json_string(&mut out, &format_rfc3339(self.since).unwrap_or_default());
        out.push_str("}}},\"sort\":{\"sort_field\":\"UPDATED_AT\",\"sort_order\":\"ASC\"}}}");
        out
    }

    /// PS-96: read the reply of SearchOrders into a page. `payments` is the
    /// reply of a ListPayments for the same orders, when the caller has one:
    /// it is where a tender's `external_details.source` and a cancelled
    /// payment are found. A page whose cursor is absent has no more.
    pub fn parse(&self, orders: &[u8], payments: Option<&[u8]>) -> Result<Page, ReadRefused> {
        let doc = parse_doc(orders)?;
        object(&doc, "reply")?;
        no_service_error(&doc)?;
        let known = match payments {
            Some(body) => read_payments(&parse_doc(body)?)?,
            None => Vec::new(),
        };
        // Square leaves `orders` out when nothing was found.
        let list = match opt(&doc, "orders") {
            Some(v) => array(v, "orders")?,
            None => &[],
        };
        if list.len() > MAX_PAGE_ORDERS {
            return Err(ReadRefused::TooManyOrders);
        }
        let mut read = Vec::with_capacity(list.len());
        for v in list {
            if let Some(order) = self.read_order(v, &known)? {
                read.push(order);
            }
        }
        let more = match opt(&doc, "cursor") {
            Some(v) => {
                let c = string(v, "cursor")?;
                if c.is_empty() {
                    return Err(ReadRefused::OutOfRange("cursor"));
                }
                Some(c)
            }
            None => None,
        };
        let next = match more {
            Some(c) => join_cursor(self.since, Some(c))?,
            None => join_cursor(newest(self.since, read.iter().map(|o| o.updated_at)), None)?,
        };
        Ok(Page {
            orders: read,
            next,
            more: more.is_some(),
        })
    }

    /// `None` for an order whose state is not known and that carries no
    /// Bitcoin tender: it is left out of the page (PS-91).
    fn read_order(&self, v: &Value, payments: &[Payment]) -> Result<Option<PosOrder>, ReadRefused> {
        let o = object(v, "orders[]")?;
        let order_id = id(
            string(req(o, "id", "orders[].id")?, "orders[].id")?,
            "orders[].id",
        )?;
        let location = whole_text(
            string(
                req(o, "location_id", "orders[].location_id")?,
                "orders[].location_id",
            )?,
            "orders[].location_id",
        )?;
        let state = match string(req(o, "state", "orders[].state")?, "orders[].state")? {
            "COMPLETED" => Some(OrderState::Closed),
            "CANCELED" => Some(OrderState::Voided),
            "OPEN" | "DRAFT" => Some(OrderState::Open),
            _ => None,
        };
        if let Some(version) = opt(o, "version") {
            let n = amount(version, "orders[].version")?;
            if n > MAX_VERSION {
                return Err(ReadRefused::OutOfRange("orders[].version"));
            }
        }
        let opened_at = rfc3339(
            req(o, "created_at", "orders[].created_at")?,
            "orders[].created_at",
        )?;
        let updated_at = rfc3339(
            req(o, "updated_at", "orders[].updated_at")?,
            "orders[].updated_at",
        )?;
        let closed_at = match (state, opt(o, "closed_at")) {
            (Some(OrderState::Closed), Some(v)) => Some(rfc3339(v, "orders[].closed_at")?),
            _ => None,
        };
        let (total_minor, order_currency) = money(
            req(o, "total_money", "orders[].total_money")?,
            "orders[].total_money",
        )?;
        let reference = ["ticket_name", "reference_id"]
            .iter()
            .filter_map(|k| opt(o, k))
            .find_map(|v| v.as_str().and_then(note));
        // A name or reference of the wrong type is a refusal, not a gap.
        for k in ["ticket_name", "reference_id"] {
            if let Some(v) = opt(o, k) {
                string(v, "orders[].ticket_name or reference_id")?;
            }
        }
        let mut tenders = Vec::new();
        let mut raw_ids: Vec<&str> = Vec::new();
        if let Some(list) = opt(o, "tenders") {
            let list = array(list, "orders[].tenders")?;
            if list.len() > MAX_TENDERS {
                return Err(ReadRefused::TooManyTenders);
            }
            for t in list {
                let (tender, raw) = self.read_tender(t, order_currency, payments)?;
                tenders.push(tender);
                raw_ids.push(raw);
            }
        }
        if let Some(list) = opt(o, "refunds") {
            for r in array(list, "orders[].refunds")? {
                apply_refund(
                    r,
                    order_currency,
                    &raw_ids,
                    &self.bitcoin_names,
                    &mut tenders,
                )?;
            }
        }
        let Some(state) = state else {
            // PS-91: a state that is not known matters only to an order that
            // carries a Bitcoin tender.
            if tenders.iter().any(|t| is_bitcoin(&self.bitcoin_names, t)) {
                return Err(ReadRefused::UnknownValue("orders[].state"));
            }
            return Ok(None);
        };
        let order = PosOrder {
            id: order_id,
            location,
            state,
            currency: order_currency,
            total_minor,
            tenders,
            opened_at,
            closed_at,
            updated_at,
            reference,
        };
        order.validate()?;
        Ok(Some(order))
    }

    fn read_tender<'a>(
        &self,
        v: &'a Value,
        order_currency: Currency,
        payments: &[Payment],
    ) -> Result<(PosTender, &'a str), ReadRefused> {
        let t = object(v, "orders[].tenders[]")?;
        let raw_id = string(
            req(t, "id", "orders[].tenders[].id")?,
            "orders[].tenders[].id",
        )?;
        let tender_id = id(raw_id, "orders[].tenders[].id")?;
        let kind = string(
            req(t, "type", "orders[].tenders[].type")?,
            "orders[].tenders[].type",
        )?;
        // PS-96: a type that is not on the list is an "Other" tender that is
        // never a Bitcoin tender, named with the type's text.
        let known = TENDER_TYPES.iter().find(|(k, _, _)| *k == kind);
        let mapped = known.map_or(TenderKind::Other, |(_, _, m)| *m);
        let at = rfc3339(
            req(t, "created_at", "orders[].tenders[].created_at")?,
            "orders[].tenders[].created_at",
        )?;
        let (with_tip, tender_currency) = money(
            req(t, "amount_money", "orders[].tenders[].amount_money")?,
            "orders[].tenders[].amount_money",
        )?;
        let tip_minor = match opt(t, "tip_money") {
            Some(m) => {
                let (tip, c) = money(m, "orders[].tenders[].tip_money")?;
                if c != tender_currency {
                    return Err(ReadRefused::CurrencyMismatch);
                }
                tip
            }
            None => 0,
        };
        if tender_currency != order_currency {
            return Err(ReadRefused::CurrencyMismatch);
        }
        // The tender's amount in Square includes the tip; the model's does
        // not (PS-02).
        let amount_minor = with_tip
            .checked_sub(tip_minor)
            .ok_or(ReadRefused::OutOfRange("orders[].tenders[].tip_money"))?;
        let note_text = match opt(t, "note") {
            Some(n) => Some(string(n, "orders[].tenders[].note")?),
            None => None,
        };
        let payment_id = match opt(t, "payment_id") {
            Some(p) => Some(string(p, "orders[].tenders[].payment_id")?),
            None => None,
        };
        let payment = payments
            .iter()
            .find(|p| p.id == payment_id.unwrap_or(raw_id));
        let mut bitcoin = false;
        let tender_name = if kind == "OTHER" {
            // PS-96: the note of the tender, or else the source of its
            // payment, names a Bitcoin tender when it is one of the names of
            // the settings; any other note leaves the tender "Other".
            let candidates = [note_text, payment.and_then(|p| p.source.as_deref())];
            let named = candidates
                .iter()
                .flatten()
                .find_map(|c| self.bitcoin_names.iter().find(|n| same_name(c, n)).cloned());
            match named {
                Some(n) => {
                    bitcoin = true;
                    n
                }
                None => name("Other", "orders[].tenders[].type")?,
            }
        } else if let Some((_, label, _)) = known {
            name(label, "orders[].tenders[].type")?
        } else {
            match note(&format!("Other ({kind})")) {
                Some(n) => n,
                None => name("Other", "orders[].tenders[].type")?,
            }
        };
        let reference = note_text
            .and_then(note)
            .or_else(|| payment.and_then(|p| p.note.as_deref()).and_then(note));
        let state = match payment.map(|p| p.voided) {
            Some(Some(true)) => TenderState::Voided,
            // PS-91: a payment status that is not known matters only to a
            // Bitcoin tender.
            Some(None) if bitcoin => return Err(ReadRefused::UnknownValue("payments[].status")),
            _ => TenderState::Paid,
        };
        Ok((
            PosTender {
                id: tender_id,
                kind: mapped,
                name: tender_name,
                amount_minor,
                tip_minor,
                state,
                at,
                reference,
            },
            raw_id,
        ))
    }
}

/// PS-91: a reply whose `errors` is not empty is refused.
fn no_service_error(doc: &Value) -> Result<(), ReadRefused> {
    match opt(doc, "errors") {
        Some(e) if !array(e, "errors")?.is_empty() => Err(ReadRefused::ServiceError),
        _ => Ok(()),
    }
}

/// PS-92, PS-93: a Money object, an amount and its currency.
fn money(v: &Value, what: &'static str) -> Result<(u64, Currency), ReadRefused> {
    let m = object(v, what)?;
    let n = amount(req(m, "amount", what)?, what)?;
    let c = currency(string(req(m, "currency", what)?, what)?, what)?;
    Ok((n, c))
}

/// PS-96: refunds of the order are summed on the tender they name, for the
/// refunds that are pending or approved; a refund that was rejected or failed
/// is not one. A refund on a voided tender leaves it voided.
fn apply_refund(
    v: &Value,
    order_currency: Currency,
    raw_ids: &[&str],
    names: &[Text],
    tenders: &mut [PosTender],
) -> Result<(), ReadRefused> {
    let r = object(v, "orders[].refunds[]")?;
    let tender_ref = string(
        req(r, "tender_id", "orders[].refunds[].tender_id")?,
        "orders[].refunds[].tender_id",
    )?;
    let status = string(
        req(r, "status", "orders[].refunds[].status")?,
        "orders[].refunds[].status",
    )?;
    let (refunded, c) = money(
        req(r, "amount_money", "orders[].refunds[].amount_money")?,
        "orders[].refunds[].amount_money",
    )?;
    if c != order_currency {
        return Err(ReadRefused::CurrencyMismatch);
    }
    let counts = match status {
        "PENDING" | "APPROVED" => Some(true),
        "REJECTED" | "FAILED" => Some(false),
        _ => None,
    };
    let at = raw_ids
        .iter()
        .position(|i| *i == tender_ref)
        .ok_or(ReadRefused::RefundWithoutTender)?;
    let Some(counts) = counts else {
        // PS-91: a refund status that is not known matters only to a Bitcoin
        // tender.
        let bitcoin = tenders.get(at).is_some_and(|t| is_bitcoin(names, t));
        return if bitcoin {
            Err(ReadRefused::UnknownValue("orders[].refunds[].status"))
        } else {
            Ok(())
        };
    };
    if !counts {
        return Ok(());
    }
    if refunded == 0 {
        return Err(ReadRefused::OutOfRange("orders[].refunds[].amount_money"));
    }
    let tender = tenders
        .get_mut(at)
        .ok_or(ReadRefused::RefundWithoutTender)?;
    match tender.state {
        TenderState::Voided => {}
        TenderState::Paid => {
            tender.state = TenderState::Refunded {
                amount_minor: refunded,
            }
        }
        TenderState::Refunded { amount_minor } => {
            let sum = amount_minor
                .checked_add(refunded)
                .ok_or(ReadRefused::OutOfRange("orders[].refunds[].amount_money"))?;
            tender.state = TenderState::Refunded { amount_minor: sum };
        }
    }
    Ok(())
}

/// What the reader takes from a payment of a ListPayments reply.
#[derive(Debug)]
struct Payment {
    id: String,
    /// `None` when the status is not known.
    voided: Option<bool>,
    source: Option<String>,
    note: Option<String>,
}

/// PS-96: the payments of a ListPayments reply: the id, the status (`CANCELED`
/// and `FAILED` are voided; `APPROVED`, `PENDING` and `COMPLETED` are not;
/// anything else is refused), the `external_details.source` and the note.
fn read_payments(doc: &Value) -> Result<Vec<Payment>, ReadRefused> {
    object(doc, "payments reply")?;
    no_service_error(doc)?;
    let list = match opt(doc, "payments") {
        Some(v) => array(v, "payments")?,
        None => &[],
    };
    if list.len() > MAX_PAGE_ORDERS {
        return Err(ReadRefused::TooManyOrders);
    }
    let mut out = Vec::with_capacity(list.len());
    for v in list {
        let p = object(v, "payments[]")?;
        let pid = string(req(p, "id", "payments[].id")?, "payments[].id")?.to_string();
        let voided = match string(req(p, "status", "payments[].status")?, "payments[].status")? {
            "APPROVED" | "PENDING" | "COMPLETED" => Some(false),
            "CANCELED" | "FAILED" => Some(true),
            _ => None,
        };
        let source = match opt(p, "external_details") {
            Some(e) => {
                let e = object(e, "payments[].external_details")?;
                match opt(e, "source") {
                    Some(s) => Some(string(s, "payments[].external_details.source")?.to_string()),
                    None => None,
                }
            }
            None => None,
        };
        let note = match opt(p, "note") {
            Some(n) => Some(string(n, "payments[].note")?.to_string()),
            None => None,
        };
        out.push(Payment {
            id: pid,
            voided,
            source,
            note,
        });
    }
    Ok(out)
}

/// A JSON string literal: quotes and backslashes escaped, control characters
/// as `\u00XX`.
fn push_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// PS-10: whether a tender, as the reader named it, is one of the shop's
/// Bitcoin tenders.
fn is_bitcoin(names: &[Text], t: &PosTender) -> bool {
    names.iter().any(|n| same_name(t.name.as_str(), n))
}
