//! The BOLT 11 invoice the node returned, read by this crate itself and
//! checked against what was asked and against the shop's signed settings
//! (LN-16, LN-17).
//!
//! The node's reply is not trusted: a wrong, wrong-chain or hostile endpoint
//! could return an invoice for another amount, another chain, another
//! payment hash or another node. The signature is what proves the invoice
//! belongs to the shop's node, so it is recovered here and its public key is
//! compared with the key in the settings.
//!
//! Format: <https://github.com/lightning/bolts/blob/master/11-payment-encoding.md>.

use bitcoin::bech32::primitives::decode::CheckedHrpstring;
use bitcoin::bech32::Bech32;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::ecdsa::{RecoverableSignature, RecoveryId};
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::Network;

use crate::error::InvoiceRefused;
use crate::invoice::Ask;

/// The longest invoice the crate reads, in characters. This is the longest
/// text the bech32 checksum covers.
pub const MAX_BOLT11_CHARS: usize = 1023;

/// How long an invoice lasts when it has no expiry field, in seconds
/// (BOLT 11).
pub const DEFAULT_EXPIRY_SECS: u64 = 3600;

/// Millisatoshis in one bitcoin.
const MSAT_PER_BTC: u128 = 100_000_000_000;

const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

/// The part of the prefix that names the network (LN-16): `bc` for the main
/// chain, `tb` for testnet, `tbs` for signet, `bcrt` for regtest.
pub fn network_prefix(network: Network) -> Option<&'static str> {
    match network {
        Network::Bitcoin => Some("bc"),
        Network::Testnet => Some("tb"),
        Network::Signet => Some("tbs"),
        Network::Regtest => Some("bcrt"),
        _ => None,
    }
}

/// What an invoice says it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Description {
    /// A text, as the node wrote it.
    Text(String),
    /// Only the hash of a description.
    Hash([u8; 32]),
}

/// An invoice as it is written, with the public key its signature gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The network part of the prefix (`bc`, `tb`, `tbs`, `bcrt`).
    pub network: &'static str,
    /// The amount in millisatoshis, if the invoice names one.
    pub amount_msat: Option<u64>,
    /// When the invoice was made, in seconds since 1970.
    pub timestamp: u64,
    /// The payment hash.
    pub payment_hash: [u8; 32],
    /// The description or the hash of it, if there is one.
    pub description: Option<Description>,
    /// How long it lasts, in seconds from the timestamp.
    pub expiry_secs: u64,
    /// The node's key if the invoice names one in the `n` field.
    pub payee: Option<[u8; 33]>,
    /// The public key that the signature gives.
    pub signer: [u8; 33],
}

fn charset_index(c: u8) -> Option<u8> {
    CHARSET
        .iter()
        .position(|x| *x == c)
        .and_then(|p| u8::try_from(p).ok())
}

/// Groups of 5 bits into bytes. With `pad`, the last partial byte is filled
/// with zero bits; without it, the bits left over must be zero and fewer than
/// the 5 of a group. `None` for a group above 31.
fn to_bytes(groups: &[u8], pad: bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(groups.len() * 5 / 8 + 1);
    let (mut acc, mut bits) = (0u32, 0u32);
    for g in groups {
        if *g > 31 {
            return None;
        }
        acc = (acc << 5 | u32::from(*g)) & 0x1fff;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits & 0xff).ok()?);
        }
    }
    let rest = acc & ((1 << bits) - 1);
    if bits > 0 {
        if pad {
            out.push(u8::try_from(rest << (8 - bits)).ok()?);
        } else if rest != 0 {
            return None;
        }
    }
    Some(out)
}

/// A number from groups, most significant first, in at most 7 groups.
fn to_number(groups: &[u8]) -> Option<u64> {
    if groups.len() > 7 {
        return None;
    }
    groups
        .iter()
        .try_fold(0u64, |n, g| (*g <= 31).then_some(n << 5 | u64::from(*g)))
}

/// The network and the amount from the part of the prefix after `ln`.
fn read_prefix(hrp: &str) -> Result<(&'static str, Option<u64>), InvoiceRefused> {
    let rest = hrp.strip_prefix("ln").ok_or(InvoiceRefused::NotAnInvoice)?;
    let (network, amount) = ["bcrt", "tbs", "bc", "tb"]
        .into_iter()
        .find_map(|n| rest.strip_prefix(n).map(|a| (n, a)))
        .ok_or(InvoiceRefused::UnknownNetwork)?;
    if amount.is_empty() {
        return Ok((network, None));
    }
    let (digits, multiplier) = match amount.char_indices().last() {
        Some((i, c @ ('m' | 'u' | 'n' | 'p'))) => (amount.get(..i).unwrap_or(""), Some(c)),
        _ => (amount, None),
    };
    // No sign, no leading zero, no digits that are not digits.
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(InvoiceRefused::BadAmount);
    }
    let n: u128 = digits.parse().map_err(|_| InvoiceRefused::BadAmount)?;
    let msat = match multiplier {
        None => n.checked_mul(MSAT_PER_BTC),
        Some('m') => n.checked_mul(MSAT_PER_BTC / 1_000),
        Some('u') => n.checked_mul(MSAT_PER_BTC / 1_000_000),
        Some('n') => n.checked_mul(MSAT_PER_BTC / 1_000_000_000),
        // A picobitcoin is a tenth of a millisatoshi: only whole tens.
        Some(_) => n.is_multiple_of(10).then_some(n / 10),
    }
    .and_then(|m| u64::try_from(m).ok())
    .ok_or(InvoiceRefused::BadAmount)?;
    Ok((network, Some(msat)))
}

/// Read an invoice: the checksum, the prefix, the fields and the signature.
/// Fields the crate does not need are skipped; the ones it needs must be well
/// formed and appear once. A field of the payment hash, the node key or the
/// description hash with the wrong length is refused, where BOLT 11 would
/// have a reader skip it.
pub fn decode(text: &str) -> Result<Decoded, InvoiceRefused> {
    if text.chars().count() > MAX_BOLT11_CHARS {
        return Err(InvoiceRefused::TooLong);
    }
    let lower = text.to_ascii_lowercase();
    if text != lower && text != text.to_ascii_uppercase() {
        return Err(InvoiceRefused::MixedCase);
    }
    let checked =
        CheckedHrpstring::new::<Bech32>(&lower).map_err(|_| InvoiceRefused::NotAnInvoice)?;
    let hrp = checked.hrp().to_string();
    let (network, amount_msat) = read_prefix(&hrp)?;
    let groups: Vec<u8> = checked
        .data_part_ascii_no_checksum()
        .iter()
        .map(|c| charset_index(*c))
        .collect::<Option<_>>()
        .ok_or(InvoiceRefused::NotAnInvoice)?;

    // The timestamp, the fields, and last the signature of 104 groups.
    let body_end = groups
        .len()
        .checked_sub(104)
        .filter(|e| *e >= 7)
        .ok_or(InvoiceRefused::Truncated)?;
    let (body, sig_groups) = groups.split_at(body_end);
    let (stamp, mut fields) = body.split_at(7);
    let timestamp = to_number(stamp).ok_or(InvoiceRefused::Truncated)?;

    let mut hash: Option<[u8; 32]> = None;
    let mut description: Option<Description> = None;
    let mut expiry: Option<u64> = None;
    let mut payee: Option<[u8; 33]> = None;
    while !fields.is_empty() {
        let (Some(tag), Some(hi), Some(lo)) = (fields.first(), fields.get(1), fields.get(2)) else {
            return Err(InvoiceRefused::Truncated);
        };
        let len = usize::from(*hi) * 32 + usize::from(*lo);
        let data = fields.get(3..3 + len).ok_or(InvoiceRefused::Truncated)?;
        fields = fields.get(3 + len..).unwrap_or(&[]);
        match *tag {
            // p: the payment hash.
            1 => {
                let bytes = (len == 52).then(|| to_bytes(data, false)).flatten();
                let h = bytes
                    .and_then(|b| <[u8; 32]>::try_from(b).ok())
                    .ok_or(InvoiceRefused::BadField("payment hash"))?;
                if hash.replace(h).is_some() {
                    return Err(InvoiceRefused::DuplicateField("payment hash"));
                }
            }
            // d: the description.
            13 => {
                let bytes = to_bytes(data, false).ok_or(InvoiceRefused::BadField("description"))?;
                let t = String::from_utf8(bytes)
                    .map_err(|_| InvoiceRefused::BadField("description"))?;
                if description.replace(Description::Text(t)).is_some() {
                    return Err(InvoiceRefused::DuplicateField("description"));
                }
            }
            // h: the hash of the description.
            23 => {
                let bytes = (len == 52).then(|| to_bytes(data, false)).flatten();
                let h = bytes
                    .and_then(|b| <[u8; 32]>::try_from(b).ok())
                    .ok_or(InvoiceRefused::BadField("description hash"))?;
                if description.replace(Description::Hash(h)).is_some() {
                    return Err(InvoiceRefused::DuplicateField("description"));
                }
            }
            // x: the expiry.
            6 => {
                let n = to_number(data).ok_or(InvoiceRefused::BadField("expiry"))?;
                if expiry.replace(n).is_some() {
                    return Err(InvoiceRefused::DuplicateField("expiry"));
                }
            }
            // n: the node's key.
            19 => {
                let bytes = (len == 53).then(|| to_bytes(data, false)).flatten();
                let k = bytes
                    .and_then(|b| <[u8; 33]>::try_from(b).ok())
                    .ok_or(InvoiceRefused::BadField("node key"))?;
                if payee.replace(k).is_some() {
                    return Err(InvoiceRefused::DuplicateField("node key"));
                }
            }
            _ => {}
        }
    }

    // The signature: 64 bytes and the recovery id, over the SHA-256 of the
    // prefix and the data before it, padded with zero bits to whole bytes.
    let sig = to_bytes(sig_groups, false).ok_or(InvoiceRefused::BadSignature)?;
    let (compact, recid) = match (sig.get(..64), sig.get(64)) {
        (Some(c), Some(r)) if sig.len() == 65 => (c, *r),
        _ => return Err(InvoiceRefused::BadSignature),
    };
    let mut signed = hrp.into_bytes();
    signed.extend(to_bytes(body, true).ok_or(InvoiceRefused::Truncated)?);
    let digest = sha256::Hash::hash(&signed).to_byte_array();
    let recoverable = RecoveryId::from_i32(i32::from(recid))
        .and_then(|id| RecoverableSignature::from_compact(compact, id))
        .map_err(|_| InvoiceRefused::BadSignature)?;
    let signer = Secp256k1::verification_only()
        .recover_ecdsa(&Message::from_digest(digest), &recoverable)
        .map_err(|_| InvoiceRefused::BadSignature)?
        .serialize();

    Ok(Decoded {
        network,
        amount_msat,
        timestamp,
        payment_hash: hash.ok_or(InvoiceRefused::MissingField("payment hash"))?,
        description,
        expiry_secs: expiry.unwrap_or(DEFAULT_EXPIRY_SECS),
        payee,
        signer,
    })
}

/// What an invoice must say to be the one asked for (LN-17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expectation {
    /// The shop's network.
    pub network: Network,
    /// The shop's Lightning node, from the signed settings.
    pub node: [u8; 33],
    /// The payment hash the node reported with the invoice.
    pub payment_hash: [u8; 32],
    /// What was asked for: the amount, the lifetime and the description.
    pub ask: Ask,
}

/// An invoice that passed every check: it is for this sale, this node and
/// this network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The invoice in lower case, as the code carries it.
    pub bolt11: String,
    /// The amount, in millisatoshis.
    pub amount_msat: u64,
    /// The payment hash.
    pub payment_hash: [u8; 32],
    /// When the invoice was made, in seconds since 1970, by the node's clock.
    pub timestamp: u64,
}

/// LN-17: read the invoice and refuse it unless the network is the shop's,
/// the signer is the node of the settings (and so is the `n` field, if there
/// is one), the amount is the one asked for, the payment hash is the node's,
/// the lifetime is the one asked for and the description is the sale's.
pub fn check(text: &str, expect: &Expectation) -> Result<Checked, InvoiceRefused> {
    let d = decode(text)?;
    if network_prefix(expect.network) != Some(d.network) {
        return Err(InvoiceRefused::WrongNetwork);
    }
    if d.signer != expect.node || d.payee.is_some_and(|p| p != expect.node) {
        return Err(InvoiceRefused::WrongNode);
    }
    let asked = expect.ask.amount_msat();
    let got = d.amount_msat.ok_or(InvoiceRefused::NoAmount)?;
    if got != asked {
        return Err(InvoiceRefused::WrongAmount { asked, got });
    }
    if d.payment_hash != expect.payment_hash {
        return Err(InvoiceRefused::WrongPaymentHash);
    }
    let asked = u64::from(expect.ask.expiry_secs());
    if d.expiry_secs != asked {
        return Err(InvoiceRefused::WrongExpiry {
            asked,
            got: d.expiry_secs,
        });
    }
    match &d.description {
        Some(Description::Text(t)) if t == expect.ask.description() => {}
        _ => return Err(InvoiceRefused::WrongDescription),
    }
    Ok(Checked {
        bolt11: text.to_ascii_lowercase(),
        amount_msat: got,
        payment_hash: d.payment_hash,
        timestamp: d.timestamp,
    })
}
