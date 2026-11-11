//! The service's reply, read strictly (SW-10 to SW-13), and the check that
//! decides whether the shop goes on with it (SW-20 to SW-28).

use bitcoin::hex::FromHex;
use bitcoin::secp256k1::PublicKey;
use bitcoin::{Address, Network};
use openbmt_json::{Limits, Value};
use openbmt_lightning::bolt11;

use crate::error::Refused;
use crate::keys::Secrets;
use crate::tree::{claim_script, refund_script, KeyOrder, SwapTree};
use crate::{
    MAX_INVOICE_SATS, MAX_LEAF_BYTES, MAX_REPLY_BYTES, MAX_REPLY_DEPTH, MIN_CLAIM_BLOCKS,
    MIN_INVOICE_LIFE_SECS, MIN_ONCHAIN_SATS,
};

/// The version of a tapscript leaf (BIP 342).
const TAPSCRIPT: i64 = 192;
/// The first lock time that is a time and not a height.
const LOCKTIME_THRESHOLD: u32 = 500_000_000;

/// What a reply says, read but not yet believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    id: String,
    invoice: String,
    lockup_address: String,
    refund_key: PublicKey,
    timeout: u32,
    onchain_amount: u64,
    claim_leaf: Vec<u8>,
    refund_leaf: Vec<u8>,
}

fn member<'a>(v: &'a Value, name: &'static str) -> Result<&'a Value, Refused> {
    v.get(name).ok_or(Refused::MissingMember(name))
}

fn text<'a>(v: &'a Value, name: &'static str) -> Result<&'a str, Refused> {
    member(v, name)?.as_str().ok_or(Refused::BadMember(name))
}

fn whole(v: &Value, name: &'static str) -> Result<u64, Refused> {
    match member(v, name)? {
        Value::Int(n) => u64::try_from(*n).map_err(|_| Refused::BadMember(name)),
        Value::UInt(n) => Ok(*n),
        _ => Err(Refused::BadMember(name)),
    }
}

fn leaf(v: &Value, name: &'static str) -> Result<Vec<u8>, Refused> {
    let l = member(v, name)?;
    match member(l, "version")? {
        Value::Int(TAPSCRIPT) => {}
        _ => return Err(Refused::BadMember(name)),
    }
    let bytes = Vec::<u8>::from_hex(text(l, "output")?).map_err(|_| Refused::BadMember(name))?;
    if bytes.is_empty() || bytes.len() > MAX_LEAF_BYTES {
        return Err(Refused::BadMember(name));
    }
    Ok(bytes)
}

impl Reply {
    /// SW-10 to SW-13: read the body of the service's reply.
    pub fn parse(body: &[u8]) -> Result<Self, Refused> {
        let limits = Limits {
            max_bytes: MAX_REPLY_BYTES,
            max_depth: MAX_REPLY_DEPTH,
            ..Limits::DEFAULT
        };
        let doc = openbmt_json::parse(body, &limits).map_err(Refused::ReplyNotJson)?;
        let id = text(&doc, "id")?;
        if id.is_empty()
            || id.len() > 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(Refused::BadMember("id"));
        }
        let key_hex = text(&doc, "refundPublicKey")?;
        let refund_key = (key_hex.len() == 66)
            .then(|| Vec::<u8>::from_hex(key_hex).ok())
            .flatten()
            .and_then(|b| PublicKey::from_slice(&b).ok())
            .ok_or(Refused::BadMember("refundPublicKey"))?;
        let timeout = u32::try_from(whole(&doc, "timeoutBlockHeight")?)
            .map_err(|_| Refused::BadMember("timeoutBlockHeight"))?;
        let tree = member(&doc, "swapTree")?;
        Ok(Self {
            id: id.to_string(),
            invoice: text(&doc, "invoice")?.to_string(),
            lockup_address: text(&doc, "lockupAddress")?.to_string(),
            refund_key,
            timeout,
            onchain_amount: whole(&doc, "onchainAmount")?,
            claim_leaf: leaf(tree, "claimLeaf")?,
            refund_leaf: leaf(tree, "refundLeaf")?,
        })
    }

    /// The service's id for the swap.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// The service's fee schedule, as the caller read it from the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quote {
    /// The percentage fee, in parts per million of the invoice amount.
    pub fee_ppm: u32,
    /// The service's fee for its own lockup transaction, in satoshis.
    pub lockup_fee_sats: u64,
}

impl Quote {
    /// SW-28: the least on-chain amount the quote leaves for an invoice.
    pub fn least_onchain(&self, invoice_sats: u64) -> u64 {
        let pct = (u128::from(invoice_sats) * u128::from(self.fee_ppm)).div_ceil(1_000_000);
        let pct = u64::try_from(pct).unwrap_or(u64::MAX);
        invoice_sats
            .saturating_sub(pct)
            .saturating_sub(self.lockup_fee_sats)
    }
}

/// What the shop knows when it checks a reply.
#[derive(Debug, Clone)]
pub struct Expect<'a> {
    /// The preimage and the claim key of this swap.
    pub secrets: &'a Secrets,
    /// The invoice amount that was asked for, in satoshis.
    pub invoice_sats: u64,
    /// The shop's network.
    pub network: Network,
    /// The service's fees.
    pub quote: Quote,
    /// The height of the chain tip.
    pub tip_height: u32,
    /// The time, in seconds since 1970.
    pub now_secs: u64,
}

/// A reply that passed every rule of SW-20 to SW-28.
#[derive(Debug, Clone)]
pub struct Swap {
    /// The service's id for the swap.
    pub id: String,
    /// The invoice in lower case, for the customer to pay.
    pub invoice: String,
    /// The invoice amount, in satoshis.
    pub invoice_sats: u64,
    /// When the invoice runs out, in seconds since 1970.
    pub invoice_expires_at: u64,
    /// The service's refund key.
    pub refund_key: PublicKey,
    /// The order the keys went into the output key.
    pub order: KeyOrder,
    /// The block height after which the service can take the lockup back.
    pub timeout: u32,
    /// The amount the service promised to lock, in satoshis.
    pub onchain_sats: u64,
    /// The lockup address.
    pub lockup_address: Address,
    /// The tree and the output.
    pub tree: SwapTree,
}

/// SW-20 to SW-28: believe the reply only if every rule holds.
pub fn check(reply: &Reply, expect: &Expect<'_>) -> Result<Swap, Refused> {
    // The invoice.
    let invoice = bolt11::decode(&reply.invoice).map_err(Refused::InvoiceUnreadable)?;
    if invoice.payment_hash != expect.secrets.preimage_hash() {
        return Err(Refused::WrongPaymentHash);
    }
    let asked = expect
        .invoice_sats
        .checked_mul(1000)
        .ok_or(Refused::AmountOutOfBand)?;
    match invoice.amount_msat {
        None => return Err(Refused::NoInvoiceAmount),
        Some(got) if got != asked => return Err(Refused::WrongInvoiceAmount { asked, got }),
        Some(_) => {}
    }
    if bolt11::network_prefix(expect.network) != Some(invoice.network) {
        return Err(Refused::WrongInvoiceNetwork);
    }
    let expires_at = invoice
        .timestamp
        .checked_add(invoice.expiry_secs)
        .ok_or(Refused::InvoiceTooShort)?;
    if expires_at < expect.now_secs.saturating_add(MIN_INVOICE_LIFE_SECS) {
        return Err(Refused::InvoiceTooShort);
    }

    // The tree and the address.
    let claim_key = expect.secrets.claim_public();
    if reply.refund_key == claim_key {
        return Err(Refused::BadRefundKey);
    }
    let hash = expect.secrets.preimage_hash();
    let address: Address = reply
        .lockup_address
        .parse::<Address<_>>()
        .map_err(|_| Refused::WrongLockupAddress)?
        .require_network(expect.network)
        .map_err(|_| Refused::WrongLockupAddress)?;
    if claim_script(&claim_key, &hash).as_bytes() != reply.claim_leaf.as_slice() {
        return Err(Refused::LeafMismatch("claim"));
    }
    if refund_script(&reply.refund_key, reply.timeout).as_bytes() != reply.refund_leaf.as_slice() {
        return Err(Refused::LeafMismatch("refund"));
    }
    let mut found = None;
    for order in [KeyOrder::RefundFirst, KeyOrder::ClaimFirst] {
        let tree = SwapTree::build(&claim_key, &reply.refund_key, &hash, reply.timeout, order)?;
        if tree.script_pubkey() == address.script_pubkey() {
            found = Some((order, tree));
            break;
        }
    }
    let (order, tree) = found.ok_or(Refused::WrongLockupAddress)?;
    if reply.timeout >= LOCKTIME_THRESHOLD
        || u64::from(reply.timeout) < u64::from(expect.tip_height).saturating_add(MIN_CLAIM_BLOCKS)
    {
        return Err(Refused::TimeoutTooSoon);
    }

    // The amount.
    if reply.onchain_amount < MIN_ONCHAIN_SATS {
        return Err(Refused::OnchainTooSmall);
    }
    if reply.onchain_amount > expect.invoice_sats || expect.invoice_sats > MAX_INVOICE_SATS {
        return Err(Refused::OnchainAboveInvoice);
    }
    let least = expect.quote.least_onchain(expect.invoice_sats);
    if reply.onchain_amount < least {
        return Err(Refused::OnchainBelowQuote {
            least,
            got: reply.onchain_amount,
        });
    }

    Ok(Swap {
        id: reply.id.clone(),
        invoice: reply.invoice.to_ascii_lowercase(),
        invoice_sats: expect.invoice_sats,
        invoice_expires_at: expires_at,
        refund_key: reply.refund_key,
        order,
        timeout: reply.timeout,
        onchain_sats: reply.onchain_amount,
        lockup_address: address,
        tree,
    })
}
