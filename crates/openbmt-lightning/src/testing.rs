//! Recorded replies and a writer of BOLT 11 invoices, for the tests of this
//! crate and of the crates that drive it (`testing` feature). The fixtures
//! are written from the projects' documents, not captured from a node. Not
//! for use outside tests.

#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use bitcoin::Network;

use crate::credential::{CertPin, ClnCredential, Credential, Endpoint, LndCredential};
use crate::invoice::Ask;
use crate::request::Reply;
use crate::text::{base64, hex, Alphabet};

pub const COUNTER: [u8; 4] = [0xc0, 0xff, 0xee, 0x01];

/// The secret key of a test node.
pub fn secret(seed: u8) -> SecretKey {
    SecretKey::from_slice(&[seed; 32]).expect("a key")
}

/// The public key of a test node.
pub fn node_key(seed: u8) -> [u8; 33] {
    PublicKey::from_secret_key(&Secp256k1::new(), &secret(seed)).serialize()
}

/// The bytes of a certificate-shaped DER sequence of `n` bytes inside.
pub fn der(n: usize, fill: u8) -> Vec<u8> {
    let mut out = match n {
        0..=127 => vec![0x30, n as u8],
        128..=255 => vec![0x30, 0x81, n as u8],
        _ => vec![0x30, 0x82, (n >> 8) as u8, n as u8],
    };
    out.extend(std::iter::repeat_n(fill, n));
    out
}

pub fn pin() -> CertPin {
    CertPin::from_der(&der(300, 7)).expect("a pin")
}

pub fn endpoint() -> Endpoint {
    Endpoint::parse("node.local:8080", pin()).expect("an endpoint")
}

/// A macaroon-shaped byte string.
pub fn macaroon() -> Vec<u8> {
    let mut m = vec![2u8];
    m.extend((0..120).map(|i| i as u8));
    m
}

pub fn lnd() -> Credential {
    Credential::Lnd(LndCredential::new(endpoint(), &macaroon()).expect("lnd"))
}

/// A rune whose hash is 32 bytes and whose restrictions are the text.
pub fn rune(restrictions: &str) -> String {
    let mut bytes = vec![0xab; 32];
    bytes.extend_from_slice(restrictions.as_bytes());
    base64(&bytes, Alphabet::Url)
        .trim_end_matches('=')
        .to_string()
}

pub const INVOICE_RUNE: &str = "=7&method=invoice|method=listinvoices";

pub fn cln() -> Credential {
    Credential::Cln(ClnCredential::new(endpoint(), &rune(INVOICE_RUNE)).expect("cln"))
}

pub fn reply(status: u16, body: &str) -> Reply {
    Reply {
        status,
        body: body.as_bytes().to_vec(),
    }
}

pub fn ask() -> Ask {
    // 25,000 satoshis, for as long as a quote lasts.
    Ask::sale(COUNTER, 17, 0, 25_000_000, 300).expect("an ask")
}

// What follows writes a BOLT 11 invoice, with its own bech32 writer so that
// the reader of the crate is tested against something it did not make.

const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

fn polymod(values: &[u8]) -> u32 {
    const GEN: [u32; 5] = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
    let mut chk = 1u32;
    for v in values {
        let top = chk >> 25;
        chk = (chk & 0x1ffffff) << 5 ^ u32::from(*v);
        for (i, g) in GEN.iter().enumerate() {
            if top >> i & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

pub fn bech32_write(hrp: &str, groups: &[u8]) -> String {
    let mut values: Vec<u8> = hrp.bytes().map(|b| b >> 5).collect();
    values.push(0);
    values.extend(hrp.bytes().map(|b| b & 31));
    values.extend_from_slice(groups);
    values.extend_from_slice(&[0; 6]);
    let pm = polymod(&values) ^ 1;
    let mut out = format!("{hrp}1");
    for g in groups {
        out.push(char::from(CHARSET[usize::from(*g)]));
    }
    for i in 0..6 {
        out.push(char::from(CHARSET[(pm >> (5 * (5 - i)) & 31) as usize]));
    }
    out
}

/// Bytes as groups of 5 bits, padded with zero bits.
pub fn groups_of(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in bytes {
        acc = acc << 8 | u32::from(*b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push((acc >> bits & 31) as u8);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push((acc << (5 - bits) & 31) as u8);
    }
    out
}

/// A number as groups, most significant first, without leading zero groups.
pub fn number_groups(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while n > 0 {
        out.push((n & 31) as u8);
        n >>= 5;
    }
    out.reverse();
    out
}

pub fn field(tag: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![tag, (data.len() >> 5) as u8, (data.len() & 31) as u8];
    out.extend_from_slice(data);
    out
}

/// The amount of an invoice as the prefix writes it.
pub fn amount_text(msat: u64) -> String {
    for (unit, per) in [
        ("", 100_000_000_000u64),
        ("m", 100_000_000),
        ("u", 100_000),
        ("n", 100),
    ] {
        if msat.is_multiple_of(per) {
            return format!("{}{unit}", msat / per);
        }
    }
    format!("{}p", msat * 10)
}

/// What goes into a test invoice.
#[derive(Clone)]
pub struct Make {
    pub network: &'static str,
    pub msat: Option<u64>,
    pub timestamp: u64,
    pub hash: [u8; 32],
    pub description: Option<String>,
    pub description_hash: Option<[u8; 32]>,
    pub expiry: Option<u64>,
    pub payee: Option<[u8; 33]>,
    pub extra: Vec<Vec<u8>>,
    pub twice: Option<&'static str>,
    pub key: SecretKey,
    pub recid_delta: u8,
    /// All the fields as groups, in place of the ones above.
    pub raw: Option<Vec<u8>>,
}

impl Make {
    /// An invoice that passes every check for `ask()` signed by test node 1
    /// on regtest.
    pub fn good(hash: [u8; 32]) -> Self {
        Self {
            network: "bcrt",
            msat: Some(25_000_000),
            timestamp: 1_700_000_000,
            hash,
            description: Some("Sale 17".to_string()),
            description_hash: None,
            expiry: Some(300),
            payee: None,
            extra: vec![field(16, &groups_of(&[9; 32]))],
            twice: None,
            key: secret(1),
            recid_delta: 0,
            raw: None,
        }
    }

    /// The invoice as text.
    pub fn write(&self) -> String {
        let mut hrp = format!("ln{}", self.network);
        if let Some(m) = self.msat {
            hrp.push_str(&amount_text(m));
        }
        let mut data = vec![0u8; 7];
        let mut ts = self.timestamp;
        for i in (0..7).rev() {
            data[i] = (ts & 31) as u8;
            ts >>= 5;
        }
        if let Some(raw) = &self.raw {
            data.extend_from_slice(raw);
        } else {
            data.extend(field(1, &groups_of(&self.hash)));
            if self.twice == Some("hash") {
                data.extend(field(1, &groups_of(&self.hash)));
            }
            if let Some(d) = &self.description {
                data.extend(field(13, &groups_of(d.as_bytes())));
                if self.twice == Some("description") {
                    data.extend(field(13, &groups_of(d.as_bytes())));
                }
            }
            if let Some(h) = &self.description_hash {
                data.extend(field(23, &groups_of(h)));
            }
            if let Some(x) = self.expiry {
                data.extend(field(6, &number_groups(x)));
                if self.twice == Some("expiry") {
                    data.extend(field(6, &number_groups(x)));
                }
            }
            if let Some(p) = &self.payee {
                data.extend(field(19, &groups_of(p)));
            }
            for e in &self.extra {
                data.extend_from_slice(e);
            }
        }
        // The signature is over the prefix and the data padded to bytes.
        let mut padded = Vec::new();
        let (mut acc, mut bits) = (0u32, 0u32);
        for g in &data {
            acc = acc << 5 | u32::from(*g);
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                padded.push((acc >> bits & 255) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        if bits > 0 {
            padded.push((acc << (8 - bits) & 255) as u8);
        }
        let mut signed = hrp.clone().into_bytes();
        signed.extend(padded);
        let digest = sha256::Hash::hash(&signed).to_byte_array();
        let sig = Secp256k1::new().sign_ecdsa_recoverable(&Message::from_digest(digest), &self.key);
        let (recid, compact) = sig.serialize_compact();
        let mut bytes = compact.to_vec();
        bytes.push(recid.to_i32() as u8 + self.recid_delta);
        data.extend(groups_of(&bytes));
        bech32_write(&hrp, &data)
    }
}

/// The reply of LND to a request for an invoice.
pub fn lnd_created(hash: &[u8; 32], bolt11: &str) -> Reply {
    reply(
        200,
        &format!(
            "{{\"r_hash\":\"{}\",\"payment_request\":\"{bolt11}\",\"add_index\":\"7\",\
             \"payment_addr\":\"AAAA\"}}",
            base64(hash, Alphabet::Standard)
        ),
    )
}

/// The reply of Core Lightning to a request for an invoice.
pub fn cln_created(hash: &[u8; 32], bolt11: &str) -> Reply {
    reply(
        200,
        &format!(
            "{{\"bolt11\":\"{bolt11}\",\"payment_hash\":\"{}\",\"payment_secret\":\"{}\",\
             \"expires_at\":1700000300,\"created_index\":3}}",
            hex(hash),
            hex(&[9; 32])
        ),
    )
}

/// The reply of LND to a lookup of an invoice by payment hash.
pub fn lnd_lookup(
    hash: &[u8; 32],
    asked_msat: u64,
    state: &str,
    paid_msat: Option<u64>,
    settled: Option<u64>,
) -> Reply {
    let mut body = format!(
        "{{\"r_hash\":\"{}\",\"value_msat\":\"{asked_msat}\",\"state\":\"{state}\"",
        base64(hash, Alphabet::Standard)
    );
    if let Some(p) = paid_msat {
        body.push_str(&format!(",\"amt_paid_msat\":\"{p}\""));
    }
    if let Some(s) = settled {
        body.push_str(&format!(",\"settle_date\":\"{s}\""));
    }
    body.push('}');
    reply(200, &body)
}

/// The reply of Core Lightning to a lookup of an invoice by payment hash.
pub fn cln_lookup(
    hash: &[u8; 32],
    asked_msat: u64,
    status: &str,
    received_msat: Option<u64>,
    paid_at: Option<u64>,
) -> Reply {
    let mut item = format!(
        "{{\"label\":\"x\",\"payment_hash\":\"{}\",\"status\":\"{status}\",\
         \"amount_msat\":{asked_msat}",
        hex(hash)
    );
    if let Some(r) = received_msat {
        item.push_str(&format!(",\"amount_received_msat\":{r}"));
    }
    if let Some(t) = paid_at {
        item.push_str(&format!(",\"paid_at\":{t}"));
    }
    item.push('}');
    reply(200, &format!("{{\"invoices\":[{item}]}}"))
}

/// The reply of Core Lightning to a lookup of an invoice by label.
pub fn cln_listed(
    label: &str,
    msat: u64,
    status: &str,
    description: &str,
    hash: &[u8; 32],
    bolt11: &str,
) -> Reply {
    reply(
        200,
        &format!(
            "{{\"invoices\":[{{\"label\":\"{label}\",\"description\":\"{description}\",\
             \"payment_hash\":\"{}\",\"status\":\"{status}\",\"expires_at\":1700000300,\
             \"amount_msat\":{msat},\"bolt11\":\"{bolt11}\",\"created_index\":3}}]}}",
            hex(hash)
        ),
    )
}

/// The `lndconnect://` text of the credential `lnd()`.
pub fn lndconnect_text() -> String {
    format!(
        "lndconnect://node.local:8080?cert={}&macaroon={}",
        base64(&der(300, 7), Alphabet::Url).trim_end_matches('='),
        base64(&macaroon(), Alphabet::Url).trim_end_matches('=')
    )
}

/// The SHA-256 fingerprint of the certificate of `pin()`, in hex.
pub fn pin_fingerprint_hex() -> String {
    hex(&pin().sha256())
}

/// The refusal of LND to a call its macaroon does not allow.
pub fn lnd_denied() -> Reply {
    reply(
        500,
        "{\"code\":2,\"message\":\"permission denied\",\"details\":[]}",
    )
}

/// The refusal of Core Lightning to a call its rune does not allow.
pub fn cln_denied() -> Reply {
    reply(
        401,
        "{\"code\":1502,\"message\":\"Not permitted: method is not equal to getinfo\",\"data\":null}",
    )
}

/// The reply of Core Lightning when the label of an invoice is used.
pub fn cln_duplicate_label(label: &str) -> Reply {
    reply(
        500,
        &format!("{{\"code\":900,\"message\":\"Duplicate label '{label}'\",\"data\":null}}"),
    )
}

pub fn hash(seed: u8) -> [u8; 32] {
    sha256::Hash::hash(&[seed]).to_byte_array()
}

pub const NETWORK: Network = Network::Regtest;
