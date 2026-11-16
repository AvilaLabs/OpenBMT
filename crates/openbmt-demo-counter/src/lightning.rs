//! The pretend Lightning node of the demo counter: the shop's own node as the
//! counter's "your own node" connection talks to it (LN-01 to LN-23), in the
//! place of the HTTPS call the app makes to LND.
//!
//! The counter's own code asks for an invoice (`ln_next`), reads the answer
//! (`ln_reply`), checks it against the signed settings (LN-17), puts it in the
//! sale's code and asks whether it is paid; this node only answers, in the
//! text LND's REST interface writes. The invoice is a real BOLT 11 one on
//! regtest, signed with the demo's key, whose public key the demo's signed
//! settings name. The page pays an invoice with [`pay`], and the node then
//! answers the next lookup as settled: the counter judges the sale itself.
//!
//! The credential the counter holds is the demo's own, with a made-up address
//! and a made-up certificate: nothing is ever connected to.

use std::sync::{Mutex, MutexGuard, PoisonError};

use bitcoin::hashes::{sha256, Hash};
use openbmt_counter_node::LightningRecord;
use openbmt_json::{parse, Limits, Value};
use openbmt_lightning::testing::{self, Make};
use openbmt_lightning::Reply;

/// The seed of the demo node's key, as the demo's other identities are
/// seeded: public, and guarding nothing.
const NODE_SEED: u8 = 4;

/// One invoice the node made.
struct Made {
    text: String,
    hash: [u8; 32],
    msat: u64,
    /// When the page paid it, if it did.
    paid_at: Option<u64>,
}

static NODE: Mutex<Vec<Made>> = Mutex::new(Vec::new());

fn node() -> MutexGuard<'static, Vec<Made>> {
    NODE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The public key of the demo's Lightning node: the one the demo's signed
/// settings name.
pub fn node_key() -> [u8; 33] {
    testing::node_key(NODE_SEED)
}

/// What the app seals after a connection test and hands back at each start
/// (CR-115): the credential and the node's key. `None` if it cannot be made.
pub fn record() -> Option<Vec<u8>> {
    Some(
        LightningRecord {
            credential: testing::lnd(),
            node: node_key(),
        }
        .to_bytes(),
    )
}

/// The node starts again: the invoices it made are forgotten, as the chain's
/// pool is.
pub fn reset() {
    node().clear();
}

fn json_reply(status: u16, body: &str) -> (u16, Vec<u8>) {
    (status, body.as_bytes().to_vec())
}

fn not_found() -> (u16, Vec<u8>) {
    json_reply(
        404,
        "{\"code\":5,\"message\":\"unable to locate invoice\",\"details\":[]}",
    )
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// A request for an invoice: `POST /v1/invoices` with the memo, the amount in
/// millisatoshis and the lifetime, all of them as LN-07 writes them.
fn create(body: &str, now: u64) -> (u16, Vec<u8>) {
    let Ok(v) = parse(body.as_bytes(), &Limits::DEFAULT) else {
        return json_reply(400, "{\"code\":3,\"message\":\"bad request\"}");
    };
    let text = |key: &str| v.get(key).and_then(Value::as_str);
    let (Some(memo), Some(msat), Some(expiry)) = (
        text("memo"),
        text("value_msat").and_then(|s| s.parse::<u64>().ok()),
        text("expiry").and_then(|s| s.parse::<u64>().ok()),
    ) else {
        return json_reply(400, "{\"code\":3,\"message\":\"bad request\"}");
    };
    let mut made = node();
    // The preimage is the demo's own, a new one for each invoice.
    let mut seed = b"openbmt-demo-lightning".to_vec();
    seed.extend_from_slice(&(made.len() as u64).to_le_bytes());
    let preimage = sha256::Hash::hash(&seed).to_byte_array();
    let hash = sha256::Hash::hash(&preimage).to_byte_array();
    let invoice = Make {
        network: "bcrt",
        msat: Some(msat),
        timestamp: now,
        description: Some(memo.to_string()),
        expiry: Some(expiry),
        key: testing::secret(NODE_SEED),
        ..Make::good(hash)
    }
    .write();
    let reply: Reply = testing::lnd_created(&hash, &invoice);
    made.push(Made {
        text: invoice,
        hash,
        msat,
        paid_at: None,
    });
    (reply.status, reply.body)
}

/// A lookup: `GET /v1/invoice/<payment hash in hex>`.
fn lookup(path: &str) -> (u16, Vec<u8>) {
    let Some(hash) = path.strip_prefix("/v1/invoice/").and_then(unhex) else {
        return not_found();
    };
    let made = node();
    let Some(found) = made.iter().find(|m| m.hash == hash) else {
        return not_found();
    };
    let reply = match found.paid_at {
        Some(at) => testing::lnd_lookup(&hash, found.msat, "SETTLED", Some(found.msat), Some(at)),
        None => testing::lnd_lookup(&hash, found.msat, "OPEN", None, None),
    };
    (reply.status, reply.body)
}

/// The node's answer to a request of the counter at `now`: the status and the
/// body. A call it does not know is answered as LND answers a path it has no
/// handler for.
pub fn answer(method: &str, path: &str, body: &str, now: u64) -> (u16, Vec<u8>) {
    match (method, path) {
        ("POST", "/v1/invoices") => create(body, now),
        ("GET", p) if p.starts_with("/v1/invoice/") => lookup(p),
        _ => json_reply(404, "{\"code\":12,\"message\":\"Not Found\"}"),
    }
}

/// The page pays `invoice` (a customer's wallet reads it from the sale's code):
/// the node settles the invoice it made. Only an invoice this node made, not
/// yet paid, is taken.
pub fn pay(invoice: &str, now: u64) -> Result<u64, String> {
    let wanted = invoice.trim().to_ascii_lowercase();
    let mut made = node();
    let found = made
        .iter_mut()
        .find(|m| m.text == wanted)
        .ok_or("the node has no such invoice")?;
    if found.paid_at.is_some() {
        return Err("the invoice is already paid".to_string());
    }
    found.paid_at = Some(now);
    Ok(found.msat / 1_000)
}

#[cfg(test)]
mod tests;
