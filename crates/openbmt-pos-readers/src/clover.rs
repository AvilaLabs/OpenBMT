//! The Clover reader (PS-97, PS-98): the path and query of a request for the
//! orders of one merchant and the reading of its reply into a page of orders.
//! Read-only: nothing here changes an order.
//!
//! Vendor documentation the fields come from:
//! <https://docs.clover.com/dev/reference/ordergetorders>,
//! <https://docs.clover.com/dev/reference/paygetpayments>,
//! <https://docs.clover.com/dev/reference/paygetmerchanttenders>,
//! <https://docs.clover.com/dev/docs/paginating-elements> and
//! <https://docs.clover.com/dev/docs/using-filters-to-retrieve-multiple-results-from-an-endpoint>.
//! What those pages do not confirm is marked **[verify, E6]** in
//! `docs/POS-RULES.md` (PS-97, PS-98).

use openbmt_json::Value;
use openbmt_pos::{
    Cursor, Id, OrderState, Page, PosOrder, PosTender, Settings, TenderKind, TenderState, Text,
    MAX_PAGE_ORDERS, MAX_TENDERS,
};

use crate::common::{
    amount, array, check_limit, currency, id, join_cursor, millis, name, newest, note, object, opt,
    parse_doc, req, same_name, split_cursor, string, whole_text,
};
use crate::error::{ReadRefused, RequestRefused, Shown};

/// PS-97: the longest merchant id taken, in bytes.
pub const MAX_MERCHANT_BYTES: usize = 32;

/// PS-97: the expansions asked for. **[verify, E6]**
const EXPAND: &str = "payments,payments.tender,refunds";

/// PS-97: the order of the results. **[verify, E6]**
const ORDER_BY: &str = "modifiedTime ASC";

/// PS-98: the results of a payment that are not money taken and not a void:
/// the payment is left out of the tenders. **[verify, E6]**
const NOT_A_TENDER: [&str; 9] = [
    "FAIL",
    "INITIATED",
    "VOIDING",
    "VOID_FAILED",
    "AUTH",
    "AUTH_COMPLETED",
    "DISCOUNT",
    "OFFLINE_RETRYING",
    "PENDING",
];

/// PS-97: text with every byte but the unreserved ones of RFC 3986
/// (letters, digits, `-`, `.`, `_`, `~`) written as `%` and two capital hex
/// digits.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// One request for the orders of one merchant for one poll, and the reading
/// of its reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    merchant: String,
    bitcoin_names: Vec<Text>,
    limit: usize,
    since: u64,
    offset: u64,
}

impl Request {
    /// A request for the orders of `merchant` (the Clover merchant id, which
    /// is also the location text of the settings), at most `limit` a page
    /// (1 to 500), from the time and the offset the cursor holds. The
    /// settings give the names of the Bitcoin tenders (PS-10).
    pub fn new(
        merchant: &str,
        settings: &Settings,
        limit: usize,
        cursor: &Cursor,
    ) -> Result<Self, RequestRefused> {
        check_limit(limit)?;
        if merchant.is_empty()
            || merchant.len() > MAX_MERCHANT_BYTES
            || !merchant.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return Err(RequestRefused::BadMerchant);
        }
        let (since, vendor) = split_cursor(cursor)?;
        let offset = match vendor {
            Some(v) => {
                let canonical =
                    v.bytes().all(|b| b.is_ascii_digit()) && (v == "0" || !v.starts_with('0'));
                if !canonical {
                    return Err(RequestRefused::BadCursor);
                }
                v.parse().map_err(|_| RequestRefused::BadCursor)?
            }
            None => 0,
        };
        Ok(Self {
            merchant: merchant.to_string(),
            bitcoin_names: settings.bitcoin_tenders.clone(),
            limit,
            since,
            offset,
        })
    }

    /// PS-97: the path and the query, ready to join to the host:
    /// `/v3/merchants/<mId>/orders?filter=…&expand=…&limit=…&offset=…&orderBy=…`.
    /// The filter is `modifiedTime>=<milliseconds>` from the cursor's time.
    pub fn path_and_query(&self) -> String {
        // The seconds were checked against the year 9999 by the cursor.
        let filter = format!("modifiedTime>={}", self.since.saturating_mul(1_000));
        format!(
            "/v3/merchants/{}/orders?filter={}&expand={}&limit={}&offset={}&orderBy={}",
            self.merchant,
            percent_encode(&filter),
            percent_encode(EXPAND),
            self.limit,
            self.offset,
            percent_encode(ORDER_BY),
        )
    }

    /// PS-98: read the reply into a page. A page with fewer orders than the
    /// limit is the last; a page of exactly the limit has more, and the next
    /// cursor goes on from the offset after it. **[verify, E6]**
    pub fn parse(&self, body: &[u8]) -> Result<Page, ReadRefused> {
        let doc = parse_doc(body)?;
        object(&doc, "reply")?;
        let list = array(req(&doc, "elements", "elements")?, "elements")?;
        if list.len() > MAX_PAGE_ORDERS || list.len() > self.limit {
            return Err(ReadRefused::TooManyOrders);
        }
        let location = whole_text(&self.merchant, "merchant")?;
        let mut orders = Vec::with_capacity(list.len());
        for v in list {
            if let Some(order) = read_order(v, &location, &self.bitcoin_names)? {
                orders.push(order);
            }
        }
        // The page is as long as the reply, though an order may be left out.
        let more = list.len() == self.limit;
        let next = if more {
            let offset = self
                .offset
                .checked_add(u64::try_from(list.len()).unwrap_or(u64::MAX))
                .ok_or(ReadRefused::OutOfRange("offset"))?;
            join_cursor(self.since, Some(&offset.to_string()))?
        } else {
            join_cursor(
                newest(self.since, orders.iter().map(|o| o.updated_at)),
                None,
            )?
        };
        Ok(Page { orders, next, more })
    }
}

/// A list that Clover expands: an object with `elements`, or an array.
/// **[verify, E6]**
fn list<'a>(o: &'a Value, key: &str, what: &'static str) -> Result<&'a [Value], ReadRefused> {
    match opt(o, key) {
        None => Ok(&[]),
        Some(v @ Value::Obj(_)) => array(req(v, "elements", what)?, what),
        Some(v) => array(v, what),
    }
}

/// `None` for an order whose payment state is not known and that carries no
/// Bitcoin tender: it is left out of the page (PS-91).
fn read_order(v: &Value, location: &Text, names: &[Text]) -> Result<Option<PosOrder>, ReadRefused> {
    let o = object(v, "elements[]")?;
    let order_id = id(
        string(req(o, "id", "elements[].id")?, "elements[].id")?,
        "elements[].id",
    )?;
    let order_currency = currency(
        string(
            req(o, "currency", "elements[].currency")?,
            "elements[].currency",
        )?,
        "elements[].currency",
    )?;
    let total_minor = amount(req(o, "total", "elements[].total")?, "elements[].total")?;
    // PS-98: the state of an order is its payment state. `state` is read for
    // its type only. **[verify, E6]**
    let state = match string(
        req(o, "paymentState", "elements[].paymentState")?,
        "elements[].paymentState",
    )? {
        "OPEN" | "PARTIALLY_PAID" => Some(OrderState::Open),
        "PAID" | "PARTIALLY_REFUNDED" | "REFUNDED" | "CREDITED" => Some(OrderState::Closed),
        _ => None,
    };
    if let Some(s) = opt(o, "state") {
        string(s, "elements[].state")?;
    }
    let opened_at = millis(
        req(o, "createdTime", "elements[].createdTime")?,
        "elements[].createdTime",
    )?;
    let updated_at = millis(
        req(o, "modifiedTime", "elements[].modifiedTime")?,
        "elements[].modifiedTime",
    )?;
    let reference = match opt(o, "note") {
        Some(n) => note(string(n, "elements[].note")?),
        None => None,
    };
    let payments = list(o, "payments", "elements[].payments")?;
    if payments.len() > MAX_TENDERS {
        return Err(ReadRefused::TooManyTenders);
    }
    let mut tenders = Vec::new();
    let mut left_out: Vec<Id> = Vec::new();
    for p in payments {
        match read_payment(p, names)? {
            Read::Tender(t) => tenders.push(t),
            Read::LeftOut(i) => left_out.push(i),
        }
    }
    for r in list(o, "refunds", "elements[].refunds")? {
        apply_refund(r, &left_out, &mut tenders)?;
    }
    let Some(state) = state else {
        // PS-91: a state that is not known matters only to an order that
        // carries a Bitcoin tender.
        if tenders.iter().any(|t| is_bitcoin(names, &t.name)) {
            return Err(ReadRefused::UnknownValue("elements[].paymentState"));
        }
        return Ok(None);
    };
    let order = PosOrder {
        id: order_id,
        location: location.clone(),
        state,
        currency: order_currency,
        total_minor,
        tenders,
        opened_at,
        closed_at: None,
        updated_at,
        reference,
    };
    order.validate()?;
    Ok(Some(order))
}

/// What a payment is.
enum Read {
    Tender(PosTender),
    /// Not money taken and not a void, or a result the reader does not know
    /// on a payment that is not a Bitcoin tender: left out, with its id.
    LeftOut(Id),
}

/// PS-10: whether a label equals a Bitcoin tender name.
fn is_bitcoin(names: &[Text], label: &Text) -> bool {
    names.iter().any(|n| same_name(label.as_str(), n))
}

/// PS-98: a payment as a tender, or left out. A result that is not known
/// leaves out a payment that is not a Bitcoin tender and refuses the page for
/// one that is.
fn read_payment(v: &Value, names: &[Text]) -> Result<Read, ReadRefused> {
    let p = object(v, "payments[]")?;
    let payment_id = id(
        string(req(p, "id", "payments[].id")?, "payments[].id")?,
        "payments[].id",
    )?;
    let result = string(req(p, "result", "payments[].result")?, "payments[].result")?;
    let tender = object(req(p, "tender", "payments[].tender")?, "payments[].tender")?;
    let label = string(
        req(tender, "label", "payments[].tender.label")?,
        "payments[].tender.label",
    )?;
    let state = match result {
        "SUCCESS" => TenderState::Paid,
        "VOIDED" => TenderState::Voided,
        r if NOT_A_TENDER.contains(&r) => return Ok(Read::LeftOut(payment_id)),
        r => {
            let bitcoin =
                name(label, "payments[].tender.label").is_ok_and(|l| is_bitcoin(names, &l));
            return if bitcoin {
                Err(ReadRefused::UnknownResult(Shown::new(r)))
            } else {
                Ok(Read::LeftOut(payment_id))
            };
        }
    };
    // Read for their types only: the id of the tender type, its key and the
    // vendor's own id for an outside payment.
    for (obj, key, what) in [
        (tender, "id", "payments[].tender.id"),
        (tender, "labelKey", "payments[].tender.labelKey"),
        (p, "externalPaymentId", "payments[].externalPaymentId"),
    ] {
        if let Some(x) = opt(obj, key) {
            string(x, what)?;
        }
    }
    // Clover's amount is without the tip. **[verify, E6]**
    let amount_minor = amount(req(p, "amount", "payments[].amount")?, "payments[].amount")?;
    let tip_minor = match opt(p, "tipAmount") {
        Some(t) => amount(t, "payments[].tipAmount")?,
        None => 0,
    };
    let at = millis(
        req(p, "createdTime", "payments[].createdTime")?,
        "payments[].createdTime",
    )?;
    let reference = match opt(p, "note") {
        Some(n) => note(string(n, "payments[].note")?),
        None => None,
    };
    Ok(Read::Tender(PosTender {
        id: payment_id,
        kind: TenderKind::Other,
        name: name(label, "payments[].tender.label")?,
        amount_minor,
        tip_minor,
        state,
        at,
        reference,
    }))
}

/// PS-98: a refund names its payment and has an amount above zero; it is
/// summed on that tender, unless the tender is voided. **[verify, E6]**
fn apply_refund(v: &Value, left_out: &[Id], tenders: &mut [PosTender]) -> Result<(), ReadRefused> {
    let r = object(v, "refunds[]")?;
    let payment = object(req(r, "payment", "refunds[].payment")?, "refunds[].payment")?;
    let payment_id = id(
        string(
            req(payment, "id", "refunds[].payment.id")?,
            "refunds[].payment.id",
        )?,
        "refunds[].payment.id",
    )?;
    let refunded = amount(req(r, "amount", "refunds[].amount")?, "refunds[].amount")?;
    if refunded == 0 {
        return Err(ReadRefused::OutOfRange("refunds[].amount"));
    }
    // A refund of a payment that was left out has nothing to be added to.
    if left_out.contains(&payment_id) {
        return Ok(());
    }
    let tender = tenders
        .iter_mut()
        .find(|t| t.id == payment_id)
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
                .ok_or(ReadRefused::OutOfRange("refunds[].amount"))?;
            tender.state = TenderState::Refunded { amount_minor: sum };
        }
    }
    Ok(())
}
