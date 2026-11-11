//! Core Lightning over clnrest (LN-08 to LN-10, LN-14, LN-15).
//!
//! Documents the calls come from (docs.corelightning.org, the version of the
//! site is not shown on its pages; the release this was written against is
//! v26.06.10, <https://github.com/ElementsProject/lightning/releases>):
//! - the REST calls and the `Rune` header: <https://docs.corelightning.org/docs/rest>
//! - `invoice`: <https://docs.corelightning.org/reference/invoice>
//! - `listinvoices`: <https://docs.corelightning.org/reference/listinvoices>
//! - runes: <https://docs.corelightning.org/reference/createrune>
//!
//! `waitinvoice` is not used: it blocks until the invoice is paid or
//! expires, so over Tor or REST it needs very long timeouts. The payment
//! is polled with `listinvoices` by payment hash instead (LN-10).

use crate::connect::Probe;
use crate::credential::ClnCredential;
use crate::error::ReplyRefused;
use crate::invoice::{judge_paid, Ask, CreateOutcome, Created, NotPaid, Payment};
use crate::request::{
    hash_hex, json, json_string, node_said, number, post, success_json, text, Reply, Request,
};
use crate::text::hex;
use openbmt_json::Value;

/// The header that carries the rune (LN-10).
pub const HEADER: &str = "Rune";

/// The error code of `invoice` for a label that is used already (LN-15).
pub const ERROR_LABEL_USED: i64 = 900;

/// The first call that must be refused to a rune limited to invoices (LN-09).
pub const PROBE_INFO: &str = "POST /v1/getinfo";
/// The second call that must be refused: it lists the node's funds. Both
/// probes only read; nothing the crate calls moves money (LN-09).
pub const PROBE_FUNDS: &str = "POST /v1/listfunds";

fn call(c: &ClnCredential, path: &str, body: String) -> Request {
    post(path, HEADER, c.header_value().to_string(), body)
}

/// The two read-only calls of the probe, with their names (LN-09).
pub(crate) fn probes(c: &ClnCredential) -> [(&'static str, Request); 2] {
    [
        (PROBE_INFO, call(c, "/v1/getinfo", "{}".to_string())),
        (PROBE_FUNDS, call(c, "/v1/listfunds", "{}".to_string())),
    ]
}

/// LN-09: judge the reply to a probe. A call the rune does not allow is
/// refused with the 401 or 403 status, or with an error text that begins
/// `Not permitted` or `Not authorized`; a call that is allowed is a success.
/// Any other failure proves nothing.
pub(crate) fn judge_probe(reply: &Reply) -> Probe {
    if reply.is_success() {
        return Probe::Allowed;
    }
    let message = match node_said(reply) {
        ReplyRefused::NodeSaid { message, .. } => message,
        _ => String::new(),
    };
    if matches!(reply.status, 401 | 403)
        || message.starts_with("Not permitted")
        || message.starts_with("Not authorized")
    {
        Probe::Denied
    } else {
        Probe::Unclear
    }
}

/// LN-10: `POST /v1/invoice` with the amount in millisatoshis, the label, the
/// description and the lifetime in seconds.
pub(crate) fn create_request(c: &ClnCredential, ask: &Ask) -> Request {
    let body = format!(
        "{{\"amount_msat\":{},\"label\":{},\"description\":{},\"expiry\":{}}}",
        ask.amount_msat(),
        json_string(ask.label()),
        json_string(ask.description()),
        ask.expiry_secs()
    );
    call(c, "/v1/invoice", body)
}

/// LN-10, LN-15: read the reply to an invoice request. Error 900 is a label
/// that is used already.
pub(crate) fn parse_create(reply: &Reply) -> Result<CreateOutcome, ReplyRefused> {
    if !reply.is_success() {
        let code = json(reply)
            .ok()
            .and_then(|v| v.get("code").and_then(Value::as_i64));
        if code == Some(ERROR_LABEL_USED) {
            return Ok(CreateOutcome::LabelExists);
        }
        return Err(node_said(reply));
    }
    let v = success_json(reply)?;
    Ok(CreateOutcome::Created(Created {
        payment_hash: hash_hex(&v, "payment_hash")?,
        bolt11: text(&v, "bolt11")?.to_string(),
    }))
}

/// LN-10: `listinvoices` for one payment hash, to see whether it is paid.
pub(crate) fn lookup_request(c: &ClnCredential, payment_hash: &[u8; 32]) -> Request {
    call(
        c,
        "/v1/listinvoices",
        format!("{{\"payment_hash\":\"{}\"}}", hex(payment_hash)),
    )
}

/// LN-15: `listinvoices` for one label, to find the invoice an earlier try
/// made.
pub(crate) fn label_request(c: &ClnCredential, ask: &Ask) -> Request {
    call(
        c,
        "/v1/listinvoices",
        format!("{{\"label\":{}}}", json_string(ask.label())),
    )
}

/// The one invoice of a `listinvoices` reply, if there is one.
fn only_invoice(v: &Value) -> Result<Option<&Value>, ReplyRefused> {
    let list = v
        .get("invoices")
        .ok_or(ReplyRefused::Missing("invoices"))?
        .as_array()
        .ok_or(ReplyRefused::WrongType("invoices"))?;
    match list {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        _ => Err(ReplyRefused::Several),
    }
}

/// LN-14: read the reply to a lookup by payment hash. Paid only when `status`
/// is `paid` and `amount_received_msat` is at least the amount asked.
pub(crate) fn parse_lookup(
    reply: &Reply,
    payment_hash: &[u8; 32],
    asked_msat: u64,
) -> Result<Payment, ReplyRefused> {
    let v = success_json(reply)?;
    let Some(inv) = only_invoice(&v)? else {
        return Ok(Payment::NotPaid(NotPaid::NotFound));
    };
    if hash_hex(inv, "payment_hash")? != *payment_hash {
        return Err(ReplyRefused::OtherInvoice);
    }
    Ok(match text(inv, "status")? {
        "paid" => {
            let received = number(inv, "amount_received_msat")?;
            let settled_at = match inv.get("paid_at") {
                None => None,
                Some(_) => Some(number(inv, "paid_at")?),
            };
            judge_paid(*payment_hash, received, settled_at, asked_msat)
        }
        "unpaid" => Payment::NotPaid(NotPaid::Waiting),
        "expired" => Payment::NotPaid(NotPaid::Expired),
        other => Payment::NotPaid(NotPaid::Unknown(openbmt_core::say::clean_foreign(other))),
    })
}

/// LN-15: read the reply to a lookup by label after error 900. The invoice is
/// taken up again only if it is this sale's: the label, the amount, the
/// description and the state `unpaid` all match. The payment hash is the
/// node's, and `bolt11::check` then proves the invoice is for it.
pub(crate) fn parse_existing(reply: &Reply, ask: &Ask) -> Result<Created, ReplyRefused> {
    let v = success_json(reply)?;
    let inv = only_invoice(&v)?.ok_or(ReplyRefused::LabelGone)?;
    let same = text(inv, "label")? == ask.label()
        && number(inv, "amount_msat")? == ask.amount_msat()
        && text(inv, "description")? == ask.description()
        && text(inv, "status")? == "unpaid";
    if !same {
        return Err(ReplyRefused::LabelTaken);
    }
    Ok(Created {
        payment_hash: hash_hex(inv, "payment_hash")?,
        bolt11: text(inv, "bolt11")?.to_string(),
    })
}
