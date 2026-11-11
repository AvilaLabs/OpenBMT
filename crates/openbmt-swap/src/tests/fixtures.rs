//! A swap service's reply, written from the shapes the Boltz API v2
//! documents (`POST /v2/swap/reverse`). None of it is captured from a
//! service: the tree is built by this crate's own code, and the BIP 327 and
//! script layouts are checked against literals in their own tests.

use bitcoin::hex::DisplayHex;
use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey};
use bitcoin::{Address, CompressedPublicKey, Network, ScriptBuf};
use openbmt_lightning::testing::Make;

use crate::reply::{check, Expect, Quote, Reply, Swap};
use crate::tree::{claim_script, refund_script, KeyOrder, SwapTree};
use crate::{Refused, Secrets};

pub const NOW: u64 = 1_800_000_000;
pub const TIP: u32 = 250_000;
pub const TIMEOUT: u32 = 250_144;
pub const INVOICE_SATS: u64 = 100_000;
pub const NET: Network = Network::Signet;
pub const QUOTE: Quote = Quote {
    fee_ppm: 2_500,
    lockup_fee_sats: 400,
};

pub fn secrets() -> Secrets {
    Secrets::from_random([7; 32], [9; 32]).unwrap()
}

pub fn service_key() -> PublicKey {
    let sk = SecretKey::from_slice(&[5; 32]).unwrap();
    PublicKey::from_secret_key(&Secp256k1::new(), &sk)
}

/// A fresh receive address of the shop, for the claim to pay.
pub fn destination() -> Address {
    let sk = SecretKey::from_slice(&[3; 32]).unwrap();
    let pk = PublicKey::from_secret_key(&Secp256k1::new(), &sk);
    Address::p2wpkh(&CompressedPublicKey(pk), NET)
}

pub fn invoice(
    hash: [u8; 32],
    sats: u64,
    hrp: &'static str,
    timestamp: u64,
    expiry: u64,
) -> String {
    let mut m = Make::good(hash);
    m.network = hrp;
    m.msat = Some(sats * 1000);
    m.timestamp = timestamp;
    m.expiry = Some(expiry);
    m.description = Some("swap".to_string());
    m.write()
}

/// The members of a reply, each one open to change.
#[derive(Clone)]
pub struct Fx {
    pub id: String,
    pub invoice: String,
    pub address: String,
    pub refund_key: String,
    pub timeout: String,
    pub onchain: String,
    pub claim_version: String,
    pub claim_leaf: String,
    pub refund_version: String,
    pub refund_leaf: String,
    pub extra: String,
}

impl Fx {
    pub fn good() -> Self {
        Self::with_order(KeyOrder::RefundFirst)
    }

    pub fn with_order(order: KeyOrder) -> Self {
        let s = secrets();
        let tree = SwapTree::build(
            &s.claim_public(),
            &service_key(),
            &s.preimage_hash(),
            TIMEOUT,
            order,
        )
        .unwrap();
        Self {
            id: "\"aBc123xyz\"".into(),
            invoice: format!(
                "\"{}\"",
                invoice(s.preimage_hash(), INVOICE_SATS, "tbs", NOW - 10, 3600)
            ),
            address: format!("\"{}\"", tree.address(NET)),
            refund_key: format!("\"{}\"", service_key().serialize().to_lower_hex_string()),
            timeout: TIMEOUT.to_string(),
            onchain: "99350".into(),
            claim_version: "192".into(),
            claim_leaf: format!("\"{}\"", tree.claim_leaf().as_bytes().to_lower_hex_string()),
            refund_version: "192".into(),
            refund_leaf: format!(
                "\"{}\"",
                tree.refund_leaf().as_bytes().to_lower_hex_string()
            ),
            extra: String::new(),
        }
    }

    pub fn json(&self) -> String {
        format!(
            "{{\"id\":{},\"invoice\":{},\"swapTree\":{{\"claimLeaf\":{{\"version\":{},\
             \"output\":{}}},\"refundLeaf\":{{\"version\":{},\"output\":{}}}}},\
             \"lockupAddress\":{},\"refundPublicKey\":{},\"timeoutBlockHeight\":{},\
             \"onchainAmount\":{}{}}}",
            self.id,
            self.invoice,
            self.claim_version,
            self.claim_leaf,
            self.refund_version,
            self.refund_leaf,
            self.address,
            self.refund_key,
            self.timeout,
            self.onchain,
            self.extra,
        )
    }

    pub fn parse(&self) -> Result<Reply, Refused> {
        Reply::parse(self.json().as_bytes())
    }

    pub fn checked(&self) -> Result<Swap, Refused> {
        let s = secrets();
        check(&self.parse()?, &expect(&s))
    }
}

pub fn expect(s: &Secrets) -> Expect<'_> {
    Expect {
        secrets: s,
        invoice_sats: INVOICE_SATS,
        network: NET,
        quote: QUOTE,
        tip_height: TIP,
        now_secs: NOW,
    }
}

pub fn good_swap() -> Swap {
    Fx::good().checked().unwrap()
}

/// The leaf scripts, for a test that needs a changed one.
pub fn leaves(timeout: u32) -> (ScriptBuf, ScriptBuf) {
    let s = secrets();
    (
        claim_script(&s.claim_public(), &s.preimage_hash()),
        refund_script(&service_key(), timeout),
    )
}
