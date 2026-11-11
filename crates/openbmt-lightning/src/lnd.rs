//! LND over its REST interface (LN-05 to LN-07, LN-14).
//!
//! Documents the calls come from:
//! - create: <https://lightning.engineering/api-docs/api/lnd/lightning/add-invoice/>
//! - lookup: `GET /v1/invoice/{r_hash_str}` in
//!   <https://github.com/lightningnetwork/lnd/blob/master/lnrpc/lightning.swagger.json>
//! - the header and the macaroons:
//!   <https://docs.lightning.engineering/lightning-network-tools/lnd/macaroons>
//! - the release this was written against: v0.21.6-beta,
//!   <https://github.com/lightningnetwork/lnd/releases>.
//!
//! Every 64-bit integer is a text of digits in LND's REST, in a request and
//! in a reply, and the reader takes no other form.

use openbmt_json::Value;

use crate::connect::Probe;
use crate::credential::LndCredential;
use crate::error::ReplyRefused;
use crate::invoice::{judge_paid, Ask, Created, NotPaid, Payment};
use crate::request::{
    get, int64_text, json_string, node_code, node_said, post, success_json, text, Reply, Request,
};
use crate::text::{hex, unbase64, Alphabet};

/// The header that carries the macaroon, in hex (LN-07).
pub const HEADER: &str = "Grpc-Metadata-macaroon";

/// The call that must be denied to a macaroon limited to invoices: it needs
/// `info:read` (LN-06).
pub const PROBE_INFO: &str = "GET /v1/getinfo";
/// The second call that must be denied: it needs `onchain:read`, which the
/// stock `invoice.macaroon` has and a baked one does not (LN-06).
pub const PROBE_BALANCE: &str = "GET /v1/balance/blockchain";

/// The two calls of the probe, with their names (LN-06).
pub(crate) fn probes(c: &LndCredential) -> [(&'static str, Request); 2] {
    [
        (
            PROBE_INFO,
            get("/v1/getinfo".to_string(), HEADER, c.header_value()),
        ),
        (
            PROBE_BALANCE,
            get(
                "/v1/balance/blockchain".to_string(),
                HEADER,
                c.header_value(),
            ),
        ),
    ]
}

/// LN-06: judge the reply to a probe. A call the macaroon does not allow is
/// answered by exactly HTTP 500 with the JSON error code 2 and the message
/// `permission denied`: the macaroon checker's `ErrPermissionDenied` leaves
/// LND as a plain error, which gRPC calls `Unknown` (2) and the gateway's
/// default handler writes as 500 (sources in LN-06). A call that is allowed
/// is a success; every other answer proves nothing.
pub(crate) fn judge_probe(reply: &Reply) -> Probe {
    if reply.is_success() {
        return Probe::Allowed;
    }
    let message = match node_said(reply) {
        ReplyRefused::NodeSaid { message, .. } => message,
        _ => String::new(),
    };
    if reply.status == 500 && node_code(reply) == Some(2) && message == "permission denied" {
        Probe::Denied
    } else {
        Probe::Unclear
    }
}

/// LN-07: `POST /v1/invoices` with the description, the amount in
/// millisatoshis and the lifetime.
pub(crate) fn create_request(c: &LndCredential, ask: &Ask) -> Request {
    let body = format!(
        "{{\"memo\":{},\"value_msat\":\"{}\",\"expiry\":\"{}\"}}",
        json_string(ask.description()),
        ask.amount_msat(),
        ask.expiry_secs()
    );
    post("/v1/invoices", HEADER, c.header_value(), body)
}

/// A hash written in base64, as LND writes bytes in REST (standard alphabet;
/// the URL one is read too).
fn hash_b64(v: &Value, key: &'static str) -> Result<[u8; 32], ReplyRefused> {
    let s = text(v, key)?;
    let bytes = unbase64(s, Alphabet::Standard)
        .or_else(|| unbase64(s, Alphabet::Url))
        .ok_or(ReplyRefused::OutOfRange(key))?;
    <[u8; 32]>::try_from(bytes).map_err(|_| ReplyRefused::OutOfRange(key))
}

/// LN-07: read the reply to an invoice request: the payment hash and the
/// BOLT 11 invoice.
pub(crate) fn parse_create(reply: &Reply) -> Result<Created, ReplyRefused> {
    let v = success_json(reply)?;
    Ok(Created {
        payment_hash: hash_b64(&v, "r_hash")?,
        bolt11: text(&v, "payment_request")?.to_string(),
    })
}

/// LN-07: `GET /v1/invoice/{hash in hex}`, the first version of the lookup.
/// The second version, `/v2/invoices/lookup`, needs `invoices:write` in
/// LND's source (invoices_server.go of v0.21.6-beta), so it would force a
/// wider macaroon.
pub(crate) fn lookup_request(c: &LndCredential, payment_hash: &[u8; 32]) -> Request {
    get(
        format!("/v1/invoice/{}", hex(payment_hash)),
        HEADER,
        c.header_value(),
    )
}

/// LN-14: read the reply to a lookup. Paid only when `state` is `SETTLED`
/// and `amt_paid_msat` is at least the amount asked. `state` is the name of
/// the enum value and `amt_paid_msat` a text of digits, because the REST
/// gateway writes an `int64` as a text; a number there is refused. The
/// invoice that is not there is the 404 with code 5 and one of the two
/// messages LND's `LookupInvoice` gives.
pub(crate) fn parse_lookup(
    reply: &Reply,
    payment_hash: &[u8; 32],
    asked_msat: u64,
) -> Result<Payment, ReplyRefused> {
    if reply.status == 404 && node_code(reply) == Some(5) {
        if let ReplyRefused::NodeSaid { message, .. } = node_said(reply) {
            if message == "unable to locate invoice" || message == "there are no existing invoices"
            {
                return Ok(Payment::NotPaid(NotPaid::NotFound));
            }
        }
    }
    let v = success_json(reply)?;
    if hash_b64(&v, "r_hash")? != *payment_hash {
        return Err(ReplyRefused::OtherInvoice);
    }
    let state = text(&v, "state")?;
    Ok(match state {
        "SETTLED" => {
            let received = int64_text(&v, "amt_paid_msat")?;
            let settled_at = match v.get("settle_date") {
                None => None,
                Some(_) => Some(int64_text(&v, "settle_date")?).filter(|t| *t != 0),
            };
            judge_paid(*payment_hash, received, settled_at, asked_msat)
        }
        "OPEN" => Payment::NotPaid(NotPaid::Waiting),
        "CANCELED" => Payment::NotPaid(NotPaid::Cancelled),
        "ACCEPTED" => Payment::NotPaid(NotPaid::Held),
        other => Payment::NotPaid(NotPaid::Unknown(openbmt_core::say::clean_foreign(other))),
    })
}
