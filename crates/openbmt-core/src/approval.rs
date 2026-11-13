//! Approvals for a shop that uses its own wallet (`docs/APPROVAL-RULES.md`,
//! AP-01 to AP-13).
//!
//! A shop without a Key app still needs a second approval for any change
//! that can let money leave or be taken at risk. What kind of approval is
//! decided by the change itself, never by what a device says it is: the
//! class is computed from the old and new settings ([`classify`]). A change
//! that redirects Lightning money needs the wallet that holds the money to
//! sign a message; a change that only loosens needs one counter, with its PIN
//! typed by the manager, to confirm the four words both screens show.

use std::collections::BTreeMap;
use std::fmt;

use bitcoin::consensus::Decodable;
use bitcoin::ecdsa;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::ecdsa::{RecoverableSignature, RecoveryId};
use bitcoin::secp256k1::Message;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::sign_message::signed_msg_hash;
use bitcoin::{
    absolute, transaction, Address, Amount, CompressedPublicKey, Network, OutPoint, ScriptBuf,
    Sequence, Transaction, TxIn, TxOut, Witness,
};

use crate::hashing::tagged_hash;
use crate::identity::{secp, DeviceId, Identity, Sig};
use crate::settings::{changed_fields, Change, Settings, LIGHTNING_NODE_BYTES};
use crate::words::{words_from_digest, WORD_COUNT};

/// Tag of the digest the four confirmation words are read from.
pub const TAG_CONFIRM_WORDS: &str = "OpenBMT/confirm-words/v1";
/// Tag of a counter's confirmation signature.
pub const TAG_CONFIRM: &str = "OpenBMT/confirm/v1";

/// The kind of approval a change needs (AP-01, AP-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeClass {
    /// The owner phone alone.
    Free,
    /// A counter's confirmation.
    Loosen,
    /// The signature of the wallet that holds the money.
    Redirect,
}

/// The class of a change: the highest class among its fields (AP-02).
pub fn classify(old: &Settings, new: &Settings) -> ChangeClass {
    let mut class = ChangeClass::Free;
    for c in changed_fields(old, new) {
        if c.direction != Change::Loosening {
            continue;
        }
        class = class.max(field_class(old, new, c.field));
    }
    class
}

/// The class of one field that loosens (AP-01): what [`classify`] takes it
/// for. A field that tightens is Free whatever its name is.
pub fn field_class(old: &Settings, new: &Settings, field: &str) -> ChangeClass {
    match field {
        "lightning_node" => ChangeClass::Redirect,
        // AP-13: setting Shop Lightning, or changing its node's ID or its
        // counter, redirects; the provider, the second device and higher
        // limits loosen.
        "shop_lightning" => match (&old.shop_lightning, &new.shop_lightning) {
            (None, Some(_)) => ChangeClass::Redirect,
            (Some(a), Some(b)) if a.node_id != b.node_id || a.counter != b.counter => {
                ChangeClass::Redirect
            }
            _ => ChangeClass::Loosen,
        },
        // A watch key and a new Box are named in the settings but move
        // no money and lift no limit (AP-01).
        "watch_key" | "boxes" => ChangeClass::Free,
        // AP-11: the own node decides which node every device trusts for
        // its payments, but only while node-only is on in the new
        // settings; as a second check it moves nothing.
        "own_node" if !(new.node_only && new.own_node.is_some()) => ChangeClass::Free,
        // A Box is also on the device list (BX-20), so adding one adds
        // a device; that alone is not a loosening here. Any other
        // device added is.
        "devices"
            if new
                .devices
                .difference(&old.devices)
                .all(|d| new.boxes.contains(d) && !old.boxes.contains(d)) =>
        {
            ChangeClass::Free
        }
        _ => ChangeClass::Loosen,
    }
}

/// The four words both screens show for a change (AP-03): the first 44 bits
/// of `tagged_hash(TAG_CONFIRM_WORDS, settings_hash)`.
pub fn confirm_words(settings_hash: &[u8; 32]) -> [&'static str; WORD_COUNT] {
    words_from_digest(&tagged_hash(TAG_CONFIRM_WORDS, settings_hash))
}

/// Why a counter confirmation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmError {
    /// The confirming device is not a counter in the settings in force.
    NotACounter,
    /// The signature does not match the settings and the counter.
    BadSignature,
}

impl fmt::Display for ConfirmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotACounter => write!(
                f,
                "AP-05: the confirming device is not a counter of the shop"
            ),
            Self::BadSignature => write!(f, "AP-04: the counter's confirmation does not match"),
        }
    }
}

impl std::error::Error for ConfirmError {}

/// One counter's confirmation of a settings change (AP-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterConfirmation {
    /// The counter that confirmed.
    pub counter: DeviceId,
    /// Its signature over the settings hash and its own id.
    pub sig: Sig,
}

fn confirm_msg(settings_hash: &[u8; 32], counter: &DeviceId) -> [u8; 64] {
    let mut msg = [0u8; 64];
    msg[..32].copy_from_slice(settings_hash);
    msg[32..].copy_from_slice(&counter.to_bytes());
    msg
}

impl CounterConfirmation {
    /// Sign `settings_hash` with a counter's identity key.
    pub fn sign(identity: &Identity, settings_hash: &[u8; 32]) -> Self {
        let counter = identity.device_id();
        let sig = identity.sign_tagged(TAG_CONFIRM, &confirm_msg(settings_hash, &counter));
        Self { counter, sig }
    }

    /// Check the confirmation against the counters of the settings now in
    /// force (not the new ones): the counter must be one of them (AP-05),
    /// and the signature must be its own over `settings_hash` (AP-04).
    pub fn verify(
        &self,
        settings_hash: &[u8; 32],
        counters_in_force: &BTreeMap<DeviceId, u16>,
    ) -> Result<(), ConfirmError> {
        if !counters_in_force.contains_key(&self.counter) {
            return Err(ConfirmError::NotACounter);
        }
        let msg = confirm_msg(settings_hash, &self.counter);
        if self.counter.verify_tagged(TAG_CONFIRM, &msg, &self.sig) {
            Ok(())
        } else {
            Err(ConfirmError::BadSignature)
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// The message the owner's wallet signs for a redirect (AP-06). Lines end
/// with `\n`, the last one without; ASCII only.
pub fn wallet_message(
    version: u32,
    lightning_node: Option<&[u8; LIGHTNING_NODE_BYTES]>,
    settings_hash: &[u8; 32],
) -> String {
    let node = lightning_node.map_or_else(|| "none".to_string(), |n| hex(n));
    format!(
        "OpenBMT settings change\nShop settings version {version}\nLightning node {node}\nSettings {}",
        hex(settings_hash)
    )
}

/// The message the owner's wallet signs for a redirect of these settings
/// (AP-06, AP-13): the "Lightning node" line gives the node's ID of Shop
/// Lightning when it is set, else the shop's Lightning node.
pub fn wallet_message_for(settings: &Settings) -> String {
    let node = settings
        .shop_lightning
        .as_ref()
        .map(|sl| &sl.node_id)
        .or(settings.lightning_node.as_ref());
    wallet_message(settings.version, node, &settings.hash())
}

/// Why a wallet signature was refused. Every wrong signature names the
/// address that had to sign (AP-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletSigError {
    /// The address is not a mainnet P2WPKH address (AP-09).
    UnsupportedAddress,
    /// The signature is not base64 or is not a form that is accepted.
    Malformed {
        /// The address that had to sign.
        address: String,
    },
    /// The signature is well formed but was not made by the address's key.
    WrongSigner {
        /// The address that had to sign.
        address: String,
    },
}

impl fmt::Display for WalletSigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedAddress => write!(
                f,
                "AP-09: only a mainnet bech32 single-key address can sign"
            ),
            Self::Malformed { address } => write!(
                f,
                "AP-10: not a signature that is accepted; the address {address} has to sign"
            ),
            Self::WrongSigner { address } => write!(
                f,
                "AP-10: the signature is not from the address {address}, which has to sign"
            ),
        }
    }
}

impl std::error::Error for WalletSigError {}

fn b64_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Strict standard base64 with padding: no white space, canonical padding
/// bits.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let (quads, _) = bytes.as_chunks::<4>();
    for (i, chunk) in quads.iter().enumerate() {
        let last = i + 1 == quads.len();
        let pad = chunk.iter().rev().take_while(|c| **c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut acc: u32 = 0;
        for c in chunk.iter().take(4 - pad) {
            acc = (acc << 6) | u32::from(b64_value(*c)?);
        }
        acc <<= 6 * pad as u32;
        let triple = acc.to_be_bytes();
        let keep = 3 - pad;
        // The bits that padding drops must be zero.
        if triple
            .get(1 + keep..4)
            .is_some_and(|d| d.iter().any(|b| *b != 0))
        {
            return None;
        }
        out.extend_from_slice(triple.get(1..1 + keep)?);
    }
    Some(out)
}

/// Longest base64 text that is looked at (a BIP-322 simple signature is
/// about 150 characters).
const MAX_SIG_TEXT: usize = 1_000;

/// Check that `sig_base64` is a signature of `message` by the key of the
/// mainnet P2WPKH `address` (AP-07, AP-08, AP-09, AP-10).
pub fn verify_wallet_message(
    address: &Address,
    message: &str,
    sig_base64: &str,
) -> Result<(), WalletSigError> {
    if !address
        .as_unchecked()
        .is_valid_for_network(Network::Bitcoin)
    {
        return Err(WalletSigError::UnsupportedAddress);
    }
    let script = address.script_pubkey();
    if !script.is_p2wpkh() {
        return Err(WalletSigError::UnsupportedAddress);
    }
    let malformed = || WalletSigError::Malformed {
        address: address.to_string(),
    };
    let wrong = || WalletSigError::WrongSigner {
        address: address.to_string(),
    };
    if sig_base64.len() > MAX_SIG_TEXT {
        return Err(malformed());
    }
    let raw = base64_decode(sig_base64.trim()).ok_or_else(malformed)?;
    if raw.len() == 65 {
        return match verify_compact(&script, message, &raw) {
            Some(true) => Ok(()),
            Some(false) => Err(wrong()),
            None => Err(malformed()),
        };
    }
    match verify_bip322(&script, message, &raw) {
        Some(true) => Ok(()),
        Some(false) => Err(wrong()),
        None => Err(malformed()),
    }
}

/// `None` when the signature is not of the accepted shape, `Some(false)`
/// when it is but the key is not the address's.
fn verify_compact(script: &ScriptBuf, message: &str, raw: &[u8]) -> Option<bool> {
    let (header, body) = raw.split_first()?;
    // 27-34: Electrum and legacy; 39-42: BIP-137 P2WPKH. 35-38 is for
    // P2SH-P2WPKH and is not accepted.
    let recid = match *header {
        27..=34 => (*header - 27) & 3,
        39..=42 => *header - 39,
        _ => return None,
    };
    let rec = RecoveryId::from_i32(i32::from(recid)).ok()?;
    let sig = RecoverableSignature::from_compact(body, rec).ok()?;
    let digest = signed_msg_hash(message).to_byte_array();
    let key = match secp().recover_ecdsa(&Message::from_digest(digest), &sig) {
        Ok(k) => k,
        Err(_) => return Some(false),
    };
    let expected = ScriptBuf::new_p2wpkh(&CompressedPublicKey(key).wpubkey_hash());
    Some(&expected == script)
}

/// The BIP-322 `to_spend` transaction for `script` and `message`.
fn bip322_to_spend(script: &ScriptBuf, message: &str) -> Transaction {
    let msg_hash = tagged_hash("BIP0322-signed-message", message.as_bytes());
    let script_sig = bitcoin::script::Builder::new()
        .push_int(0)
        .push_slice(msg_hash)
        .into_script();
    Transaction {
        version: transaction::Version(0),
        lock_time: absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: bitcoin::Txid::all_zeros(),
                vout: 0xFFFF_FFFF,
            },
            script_sig,
            sequence: Sequence::ZERO,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::ZERO,
            script_pubkey: script.clone(),
        }],
    }
}

/// The BIP-322 `to_sign` transaction that spends `to_spend`.
fn bip322_to_sign(to_spend: &Transaction) -> Transaction {
    Transaction {
        version: transaction::Version(0),
        lock_time: absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: to_spend.compute_txid(),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ZERO,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::ZERO,
            script_pubkey: bitcoin::script::Builder::new()
                .push_opcode(bitcoin::opcodes::all::OP_RETURN)
                .into_script(),
        }],
    }
}

/// The segwit v0 signature hash of the BIP-322 `to_sign` for `script`.
fn bip322_digest(script: &ScriptBuf, message: &str) -> Option<[u8; 32]> {
    let to_sign = bip322_to_sign(&bip322_to_spend(script, message));
    let mut cache = SighashCache::new(&to_sign);
    let h = cache
        .p2wpkh_signature_hash(0, script, Amount::ZERO, EcdsaSighashType::All)
        .ok()?;
    Some(h.to_byte_array())
}

/// A BIP-322 simple signature: the consensus encoding of the witness, which
/// is a signature (with its sighash byte) and a public key.
fn verify_bip322(script: &ScriptBuf, message: &str, raw: &[u8]) -> Option<bool> {
    let mut reader = raw;
    let witness = Witness::consensus_decode(&mut reader).ok()?;
    if !reader.is_empty() || witness.len() != 2 {
        return None;
    }
    let sig_bytes = witness.nth(0)?;
    let key_bytes = witness.nth(1)?;
    let sig = ecdsa::Signature::from_slice(sig_bytes).ok()?;
    if sig.sighash_type != EcdsaSighashType::All {
        return None;
    }
    let key = CompressedPublicKey::from_slice(key_bytes).ok()?;
    if ScriptBuf::new_p2wpkh(&key.wpubkey_hash()) != *script {
        return Some(false);
    }
    let digest = bip322_digest(script, message)?;
    Some(
        secp()
            .verify_ecdsa(&Message::from_digest(digest), &sig.signature, &key.0)
            .is_ok(),
    )
}

#[cfg(test)]
pub(crate) mod test_signing {
    //! Signing helpers for the tests and for the later slices' tests.
    use super::*;
    use bitcoin::secp256k1::SecretKey;

    pub fn key(n: u8) -> (SecretKey, CompressedPublicKey) {
        let sk = SecretKey::from_slice(&[n; 32]).unwrap();
        let pk = CompressedPublicKey(sk.public_key(secp()));
        (sk, pk)
    }

    pub fn address(pk: &CompressedPublicKey) -> Address {
        Address::p2wpkh(pk, Network::Bitcoin)
    }

    pub fn b64(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in bytes.chunks(3) {
            let n = c
                .iter()
                .enumerate()
                .fold(0u32, |a, (i, b)| a | (u32::from(*b) << (16 - 8 * i)));
            for i in 0..4 {
                if i <= c.len() {
                    s.push(char::from(A[((n >> (18 - 6 * i)) & 63) as usize]));
                } else {
                    s.push('=');
                }
            }
        }
        s
    }

    /// Compact signature with the given header byte (the recovery id is
    /// carried by the caller's choice of header).
    pub fn compact(sk: &SecretKey, message: &str, header_base: u8) -> Vec<u8> {
        let digest = signed_msg_hash(message).to_byte_array();
        let sig = secp().sign_ecdsa_recoverable(&Message::from_digest(digest), sk);
        let (rec, body) = sig.serialize_compact();
        let mut out = vec![header_base + rec.to_i32() as u8];
        out.extend_from_slice(&body);
        out
    }

    /// BIP-322 simple signature for a P2WPKH key.
    pub fn bip322(sk: &SecretKey, pk: &CompressedPublicKey, message: &str) -> Vec<u8> {
        let script = ScriptBuf::new_p2wpkh(&pk.wpubkey_hash());
        let digest = bip322_digest(&script, message).unwrap();
        let sig = secp().sign_ecdsa(&Message::from_digest(digest), sk);
        let mut w = Witness::new();
        w.push(ecdsa::Signature::sighash_all(sig).serialize());
        w.push(pk.to_bytes());
        bitcoin::consensus::serialize(&w)
    }
}

#[cfg(test)]
mod tests {
    use super::test_signing::*;
    use super::*;
    use crate::settings::{classify_change, OwnNode};
    use crate::testutil::{identity, node_key, with_shop_lightning};
    use std::str::FromStr;

    const RULES: &str = include_str!("../../../docs/APPROVAL-RULES.md");
    const SOURCE: &str = include_str!("approval.rs");

    fn base() -> Settings {
        let mut s = Settings::defaults(1_000);
        s.devices.insert(identity(40).device_id());
        s.tor_on = true;
        s.customer_wifi = true;
        s.min_sale_sats = 26_000;
        s
    }

    fn next(s: &Settings) -> Settings {
        Settings {
            version: s.version + 1,
            ..s.clone()
        }
    }

    fn mutated(f: impl FnOnce(&mut Settings)) -> (Settings, Settings) {
        let old = base();
        let mut new = next(&old);
        f(&mut new);
        (old, new)
    }

    #[test]
    fn ap_01_each_field_has_its_class() {
        use ChangeClass::*;
        let node = [2u8; LIGHTNING_NODE_BYTES];
        type Case = (&'static str, ChangeClass, Box<dyn Fn(&mut Settings)>);
        let cases: Vec<Case> = vec![
            (
                "node set",
                Redirect,
                Box::new(move |s| s.lightning_node = Some(node)),
            ),
            (
                "light_per_sale",
                Loosen,
                Box::new(|s| s.light_per_sale.sats += 1),
            ),
            ("budget", Loosen, Box::new(|s| s.node_budget.sats += 1)),
            (
                "tolerance",
                Loosen,
                Box::new(|s| s.underpayment_tolerance_bps += 1),
            ),
            (
                "confirmations",
                Loosen,
                Box::new(|s| s.min_confirmations -= 1),
            ),
            (
                "heartbeat",
                Loosen,
                Box::new(|s| s.heartbeat_max_age_secs += 1),
            ),
            ("tor off", Loosen, Box::new(|s| s.tor_on = false)),
            ("wifi", Loosen, Box::new(|s| s.customer_wifi = false)),
            ("min sale", Loosen, Box::new(|s| s.min_sale_sats -= 1)),
            (
                "device added",
                Loosen,
                Box::new(|s| {
                    s.devices.insert(identity(41).device_id());
                }),
            ),
            (
                "counter added",
                Loosen,
                Box::new(|s| {
                    s.counters.insert(identity(40).device_id(), 5_000);
                }),
            ),
            (
                "currency",
                Loosen,
                Box::new(|s| s.currency = crate::currency::ShopCurrency::Eur),
            ),
            (
                "box added",
                Free,
                Box::new(|s| {
                    let b = identity(42).device_id();
                    s.devices.insert(b);
                    s.boxes.insert(b);
                }),
            ),
            (
                "watch key",
                Free,
                Box::new(|s| s.watch_key = Some(identity(43).device_id())),
            ),
            (
                "tighter limit",
                Free,
                Box::new(|s| s.light_per_sale.sats = 0),
            ),
            ("tor stays", Free, Box::new(|s| s.min_sale_sats += 1)),
            ("tips", Free, Box::new(|s| s.tips_on = true)),
            (
                "swap provider",
                Free,
                Box::new(|s| {
                    s.swap = Some(crate::settings::SwapProvider {
                        name: "Pretend".to_string(),
                        onion: "w2mqd2fcbgiop7oyxoes2ozmdnvgon22oipj7vzh2rquepf7cxauauad.onion"
                            .to_string(),
                        path_prefix: "/api".to_string(),
                        max_sale_sats: 1_000_000,
                    })
                }),
            ),
        ];
        for (name, want, f) in cases {
            let (old, new) = mutated(f);
            assert_eq!(classify(&old, &new), want, "{name}");
        }
        // A device that is not a Box, added next to a Box, loosens.
        let (old, new) = mutated(|s| {
            let b = identity(42).device_id();
            s.devices.insert(b);
            s.boxes.insert(b);
            s.devices.insert(identity(44).device_id());
        });
        assert_eq!(classify(&old, &new), Loosen);
        // A counter's share raised.
        let mut old = base();
        old.counters.insert(identity(40).device_id(), 4_000);
        let mut up = next(&old);
        up.counters.insert(identity(40).device_id(), 5_000);
        assert_eq!(classify(&old, &up), Loosen);
        assert_eq!(classify(&up, &old), Free);
        // Nothing changed.
        assert_eq!(classify(&old, &old), Free);
        // A node changed, and a node cleared then set again, redirect; a
        // node cleared does not.
        let mut a = base();
        a.lightning_node = Some(node);
        let mut b = next(&a);
        b.lightning_node = Some([3u8; LIGHTNING_NODE_BYTES]);
        assert_eq!(classify(&a, &b), Redirect);
        let mut cleared = next(&a);
        cleared.lightning_node = None;
        assert_eq!(classify(&a, &cleared), Free);
        assert_eq!(classify(&cleared, &next(&a)), Redirect);
    }

    #[test]
    fn ap_02_the_highest_class_wins() {
        let (old, new) = mutated(|s| {
            s.lightning_node = Some([2u8; LIGHTNING_NODE_BYTES]);
            s.light_per_sale.sats += 1;
            s.tips_on = true;
        });
        assert_eq!(classify(&old, &new), ChangeClass::Redirect);
        let (old, new) = mutated(|s| {
            s.light_per_sale.sats += 1;
            s.watch_key = Some(identity(43).device_id());
        });
        assert_eq!(classify(&old, &new), ChangeClass::Loosen);
        assert!(ChangeClass::Free < ChangeClass::Loosen);
        assert!(ChangeClass::Loosen < ChangeClass::Redirect);
    }

    fn own(seed: u8) -> OwnNode {
        OwnNode::new(&crate::onion::onion_name(&[seed; 32]), 8333).unwrap()
    }

    /// Settings with an own node and node-only as given.
    fn with_node(node: Option<u8>, node_only: bool, fallback: bool) -> Settings {
        let mut s = base();
        s.own_node = node.map(own);
        s.node_only = node_only;
        s.light_fallback = fallback;
        s
    }

    #[test]
    fn ap_11_turning_node_only_on_is_a_loosening() {
        let old = with_node(Some(1), false, false);
        let new = next(&with_node(Some(1), true, false));
        assert_eq!(classify(&old, &new), ChangeClass::Loosen);
        assert_eq!(classify_change(&old, &new), Change::Loosening);
    }

    #[test]
    fn ap_11_setting_or_changing_the_node_while_node_only_is_on_loosens() {
        // Set, with node-only turned on in the same change.
        let old = base();
        let new = next(&with_node(Some(1), true, false));
        assert_eq!(classify(&old, &new), ChangeClass::Loosen);
        // Changed while node-only stays on: only the node differs.
        let old = with_node(Some(1), true, false);
        let new = next(&with_node(Some(2), true, false));
        let fields = changed_fields(&old, &new);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field, "own_node");
        assert_eq!(classify(&old, &new), ChangeClass::Loosen);
        assert_eq!(field_class(&old, &new, "own_node"), ChangeClass::Loosen);
    }

    #[test]
    fn ap_11_turning_the_light_fallback_on_is_a_loosening() {
        let old = with_node(Some(1), true, false);
        let new = next(&with_node(Some(1), true, true));
        assert_eq!(classify(&old, &new), ChangeClass::Loosen);
    }

    #[test]
    fn ap_11_taking_the_node_away_and_turning_the_modes_off_are_free() {
        let on = with_node(Some(1), true, true);
        // Off, one step at a time and all at once.
        for new in [
            with_node(Some(1), true, false),
            with_node(Some(1), false, false),
            with_node(None, false, false),
        ] {
            let new = next(&new);
            assert_eq!(classify(&on, &new), ChangeClass::Free);
            assert_eq!(classify_change(&on, &new), Change::Tightening);
        }
        // The node taken away while node-only is off.
        let old = with_node(Some(1), false, false);
        let new = next(&with_node(None, false, false));
        assert_eq!(classify(&old, &new), ChangeClass::Free);
    }

    #[test]
    fn ap_11_setting_or_changing_the_node_while_node_only_is_off_is_free() {
        let old = base();
        let new = next(&with_node(Some(1), false, false));
        assert_eq!(classify(&old, &new), ChangeClass::Free);
        assert_eq!(classify_change(&old, &new), Change::Tightening);
        let old = with_node(Some(1), false, false);
        let new = next(&with_node(Some(2), false, false));
        assert_eq!(classify(&old, &new), ChangeClass::Free);
        assert_eq!(field_class(&old, &new, "own_node"), ChangeClass::Free);
    }

    #[test]
    fn ap_11_a_free_node_change_with_node_only_turned_on_is_a_loosening() {
        let old = with_node(Some(1), false, false);
        let new = next(&with_node(Some(2), true, false));
        let fields: Vec<_> = changed_fields(&old, &new).iter().map(|c| c.field).collect();
        assert_eq!(fields, ["own_node", "node_only"]);
        assert_eq!(classify(&old, &new), ChangeClass::Loosen);
    }

    #[test]
    fn ap_11_the_light_fallback_does_not_hide_a_free_change() {
        // A change that only turns the fallback off is Free even with a
        // node change that is Free.
        let old = with_node(Some(1), true, true);
        let new = next(&with_node(Some(1), true, false));
        assert_eq!(classify(&old, &new), ChangeClass::Free);
    }

    fn sl_pair(f: impl FnOnce(&mut crate::settings::ShopLightning)) -> (Settings, Settings) {
        let old = with_shop_lightning(base());
        let mut new = next(&old);
        f(new.shop_lightning.as_mut().unwrap());
        (old, new)
    }

    #[test]
    fn ap_13_setting_the_field_or_changing_its_node_or_counter_redirects() {
        let with = with_shop_lightning(base());
        assert_eq!(classify(&base(), &next(&with)), ChangeClass::Redirect);
        let (old, new) = sl_pair(|sl| sl.node_id = node_key(9));
        assert_eq!(classify(&old, &new), ChangeClass::Redirect);
        assert_eq!(
            field_class(&old, &new, "shop_lightning"),
            ChangeClass::Redirect
        );
        let other = identity(62).device_id();
        let mut old = with_shop_lightning(base());
        old.devices.insert(other);
        old.counters.insert(other, 100);
        let mut new = next(&old);
        new.shop_lightning.as_mut().unwrap().counter = other;
        assert_eq!(classify(&old, &new), ChangeClass::Redirect);
    }

    #[test]
    fn ap_13_the_provider_the_second_device_and_higher_limits_loosen() {
        type Case = (
            &'static str,
            Box<dyn FnOnce(&mut crate::settings::ShopLightning)>,
        );
        let cases: Vec<Case> = vec![
            (
                "provider id",
                Box::new(|sl| sl.provider_node_id = node_key(9)),
            ),
            (
                "provider address",
                Box::new(|sl| sl.provider_addr = "198.51.100.1:9735".parse().unwrap()),
            ),
            ("cap up", Box::new(|sl| sl.cap_sats += 1)),
            ("fee up", Box::new(|sl| sl.max_opening_fee_sats += 1)),
        ];
        for (what, f) in cases {
            let (old, new) = sl_pair(f);
            assert_eq!(classify(&old, &new), ChangeClass::Loosen, "{what}");
        }
        let second = identity(63).device_id();
        let mut old = with_shop_lightning(base());
        old.devices.insert(second);
        old.boxes.insert(second);
        let mut new = next(&old);
        new.shop_lightning.as_mut().unwrap().second_device = second;
        assert_eq!(classify(&old, &new), ChangeClass::Loosen);
    }

    #[test]
    fn ap_13_lower_limits_and_taking_the_field_away_are_free() {
        let (old, new) = sl_pair(|sl| sl.cap_sats -= 1);
        assert_eq!(classify(&old, &new), ChangeClass::Free);
        let (old, new) = sl_pair(|sl| sl.max_opening_fee_sats -= 1);
        assert_eq!(classify(&old, &new), ChangeClass::Free);
        let (old, new) = sl_pair(|sl| {
            sl.cap_sats -= 1;
            sl.max_opening_fee_sats -= 1;
        });
        assert_eq!(classify(&old, &new), ChangeClass::Free);
        let old = with_shop_lightning(base());
        let mut gone = next(&old);
        gone.shop_lightning = None;
        assert_eq!(classify(&old, &gone), ChangeClass::Free);
        assert_eq!(classify(&old, &next(&old)), ChangeClass::Free);
        // A lower cap with a changed provider is the higher class.
        let (old, new) = sl_pair(|sl| {
            sl.cap_sats -= 1;
            sl.provider_node_id = node_key(9);
        });
        assert_eq!(classify(&old, &new), ChangeClass::Loosen);
    }

    #[test]
    fn ap_13_the_wallet_message_gives_the_shop_lightning_node() {
        let s = with_shop_lightning(base());
        let sl = s.shop_lightning.unwrap();
        let m = wallet_message_for(&s);
        assert_eq!(
            m,
            format!(
                "OpenBMT settings change\nShop settings version {}\nLightning node {}\nSettings {}",
                s.version,
                hex(&sl.node_id),
                hex(&s.hash())
            )
        );
        // Without the field the line is as before.
        let mut plain = base();
        assert!(wallet_message_for(&plain).contains("Lightning node none\n"));
        plain.lightning_node = Some([2u8; LIGHTNING_NODE_BYTES]);
        assert!(
            wallet_message_for(&plain).contains(&format!("Lightning node {}\n", "02".repeat(33)))
        );
    }

    #[test]
    fn ap_03_confirm_words_follow_the_hash() {
        let h = [7u8; 32];
        let expect = words_from_digest(&tagged_hash("OpenBMT/confirm-words/v1", &h));
        assert_eq!(confirm_words(&h), expect);
        assert_ne!(confirm_words(&h), confirm_words(&[8u8; 32]));
        assert_eq!(confirm_words(&h), confirm_words(&h));
        assert_eq!(expect.len(), 4);
    }

    #[test]
    fn ap_04_a_confirmation_signs_hash_and_counter() {
        let c = identity(50);
        let h = [9u8; 32];
        let mut force = BTreeMap::new();
        force.insert(c.device_id(), 10_000u16);
        let conf = CounterConfirmation::sign(&c, &h);
        assert_eq!(conf.counter, c.device_id());
        assert_eq!(conf.verify(&h, &force), Ok(()));
        // The signature is over the tag, the hash and the counter id.
        let mut msg = h.to_vec();
        msg.extend_from_slice(&c.device_id().to_bytes());
        assert!(c
            .device_id()
            .verify_tagged("OpenBMT/confirm/v1", &msg, &conf.sig));
        // Another hash, another counter's signature under this id, or a
        // changed signature all fail.
        assert_eq!(
            conf.verify(&[10u8; 32], &force),
            Err(ConfirmError::BadSignature)
        );
        let other = CounterConfirmation::sign(&identity(51), &h);
        let forged = CounterConfirmation {
            counter: c.device_id(),
            sig: other.sig,
        };
        assert_eq!(forged.verify(&h, &force), Err(ConfirmError::BadSignature));
        let mut bad = conf;
        bad.sig.0[10] ^= 1;
        assert_eq!(bad.verify(&h, &force), Err(ConfirmError::BadSignature));
    }

    #[test]
    fn ap_05_a_counter_not_in_force_is_refused() {
        let c = identity(50);
        let h = [9u8; 32];
        let conf = CounterConfirmation::sign(&c, &h);
        let mut force = BTreeMap::new();
        force.insert(identity(51).device_id(), 10_000u16);
        let e = conf.verify(&h, &force).unwrap_err();
        assert_eq!(e, ConfirmError::NotACounter);
        assert!(e.to_string().starts_with("AP-05"));
        assert_eq!(
            conf.verify(&h, &BTreeMap::new()),
            Err(ConfirmError::NotACounter)
        );
    }

    #[test]
    fn ap_06_the_wallet_message_is_exact() {
        let h = [0xabu8; 32];
        let node = [0x02u8; LIGHTNING_NODE_BYTES];
        let m = wallet_message(7, Some(&node), &h);
        let want = format!(
            "OpenBMT settings change\nShop settings version 7\nLightning node {}\nSettings {}",
            "02".repeat(33),
            "ab".repeat(32)
        );
        assert_eq!(m, want);
        assert!(m.is_ascii());
        assert!(!m.ends_with('\n'));
        assert_eq!(m.lines().count(), 4);
        let none = wallet_message(1, None, &[0u8; 32]);
        assert_eq!(
            none,
            format!(
                "OpenBMT settings change\nShop settings version 1\nLightning node none\nSettings {}",
                "0".repeat(64)
            )
        );
    }

    #[test]
    fn ap_07_compact_signatures_in_every_header() {
        let (sk, pk) = key(11);
        let addr = address(&pk);
        let msg = "OpenBMT settings change";
        // 27-34 (Electrum and legacy) and 39-42 (BIP-137).
        for base in [27u8, 31, 39] {
            let raw = compact(&sk, msg, base);
            let text = b64(&raw);
            assert_eq!(verify_wallet_message(&addr, msg, &text), Ok(()), "{base}");
        }
        // Every header that is in the ranges is read, and a recovery id
        // that is not the signer's does not match the address.
        let good = compact(&sk, msg, 31);
        let rec = good[0] - 31;
        for h in (27u8..=34).chain(39..=42) {
            let mut raw = good.clone();
            raw[0] = h;
            let r = verify_wallet_message(&addr, msg, &b64(&raw));
            let this_rec = if h <= 34 { (h - 27) & 3 } else { h - 39 };
            assert_eq!(r.is_ok(), this_rec == rec, "header {h}");
        }
        // Headers outside the ranges are refused, 35-38 (P2SH) among them.
        for h in (0u8..27).chain(35..39).chain(43..=255) {
            let mut raw = good.clone();
            raw[0] = h;
            assert!(
                verify_wallet_message(&addr, msg, &b64(&raw)).is_err(),
                "header {h}"
            );
        }
        // Any other byte, the message or the address fails.
        for i in 1..65 {
            let mut raw = good.clone();
            raw[i] ^= 0x01;
            assert!(
                verify_wallet_message(&addr, msg, &b64(&raw)).is_err(),
                "byte {i}"
            );
        }
        let text = b64(&good);
        assert!(verify_wallet_message(&addr, "OpenBMT settings changf", &text).is_err());
        let (_, other) = key(12);
        assert!(verify_wallet_message(&address(&other), msg, &text).is_err());
        // Length 64 and 66 are not signatures.
        assert!(verify_wallet_message(&addr, msg, &b64(&good[..64])).is_err());
        let mut long = good.clone();
        long.push(0);
        assert!(verify_wallet_message(&addr, msg, &b64(&long)).is_err());
    }

    #[test]
    fn ap_08_bip322_simple_signatures() {
        // Vectors of the BIP text for the P2WPKH address of the key
        // `L3VFeEujGtevx9w18HD1fhRaCCdRe6Qdm5vQc7ysVkUj9gxxyAiG`.
        let addr = Address::from_str("bc1q9vza2e8x573nczrlzms0wvx3gsqjx7vavgkx0l")
            .unwrap()
            .assume_checked();
        let empty = "AkcwRAIgM2gBAQqvZX15ZiysmKmQpDrG83avLIT492QBzLnQIxYCIBaTpOaD20qRlEylyxFSeEA2ba9YOixpX8z46TSDtS40ASECx/EgAxlkQpQ9hYjgGu6EBCPMVPwVIVJqO4XCsMvViHI=";
        let hello = "AkcwRAIgZRfIY3p7/DoVTty6YZbWS71bc5Vct9p9Fia83eRmw2QCICK/ENGfwLtptFluMGs2KsqoNSk89pO7F29zJLUx9a/sASECx/EgAxlkQpQ9hYjgGu6EBCPMVPwVIVJqO4XCsMvViHI=";
        assert_eq!(verify_wallet_message(&addr, "", empty), Ok(()));
        assert_eq!(verify_wallet_message(&addr, "Hello World", hello), Ok(()));
        assert!(verify_wallet_message(&addr, "Hello World", empty).is_err());
        assert!(verify_wallet_message(&addr, "", hello).is_err());

        // Signatures made in the test, both directions.
        let (sk, pk) = key(13);
        let a = address(&pk);
        let msg = wallet_message(2, None, &[5u8; 32]);
        let raw = bip322(&sk, &pk, &msg);
        let text = b64(&raw);
        assert_eq!(verify_wallet_message(&a, &msg, &text), Ok(()));
        assert!(verify_wallet_message(&a, "other", &text).is_err());
        let (_, pk2) = key(14);
        assert!(verify_wallet_message(&address(&pk2), &msg, &text).is_err());
        for i in 0..raw.len() {
            let mut bad = raw.clone();
            bad[i] ^= 0x01;
            assert!(
                verify_wallet_message(&a, &msg, &b64(&bad)).is_err(),
                "byte {i}"
            );
        }
        // Trailing bytes and a witness of another length are not accepted.
        let mut trailing = raw.clone();
        trailing.push(0);
        assert!(verify_wallet_message(&a, &msg, &b64(&trailing)).is_err());
        let mut one = Witness::new();
        one.push([1u8; 71]);
        let one = bitcoin::consensus::serialize(&one);
        assert!(verify_wallet_message(&a, &msg, &b64(&one)).is_err());
    }

    #[test]
    fn ap_09_other_addresses_are_refused() {
        let (sk, pk) = key(15);
        let msg = "m";
        let text = b64(&compact(&sk, msg, 31));
        let p2pkh = Address::p2pkh(pk, Network::Bitcoin);
        let wrapped = Address::p2shwpkh(&pk, Network::Bitcoin);
        let taproot = Address::p2tr(secp(), pk.0.x_only_public_key().0, None, Network::Bitcoin);
        let testnet = Address::p2wpkh(&pk, Network::Testnet);
        for a in [p2pkh, wrapped, taproot, testnet] {
            let e = verify_wallet_message(&a, msg, &text).unwrap_err();
            assert_eq!(e, WalletSigError::UnsupportedAddress, "{a}");
            assert!(e.to_string().starts_with("AP-09"));
        }
    }

    #[test]
    fn ap_10_a_wrong_signature_names_the_address() {
        let (sk, pk) = key(16);
        let addr = address(&pk);
        let (_, other) = key(17);
        let msg = "m";
        let good = b64(&compact(&sk, msg, 31));
        let wrong = verify_wallet_message(&address(&other), msg, &good).unwrap_err();
        assert!(matches!(wrong, WalletSigError::WrongSigner { .. }));
        assert!(wrong.to_string().contains(&address(&other).to_string()));
        for text in ["", "!!!!", "AAAA", &"A".repeat(2_000), " "] {
            let e = verify_wallet_message(&addr, msg, text).unwrap_err();
            assert!(matches!(e, WalletSigError::Malformed { .. }), "{text}");
            assert!(e.to_string().contains(&addr.to_string()));
            assert!(e.to_string().starts_with("AP-10"));
        }
    }

    #[test]
    fn base64_is_strict() {
        for n in 0..8usize {
            let bytes: Vec<u8> = (0..n as u8).map(|i| i.wrapping_mul(73)).collect();
            if !bytes.is_empty() {
                assert_eq!(base64_decode(&b64(&bytes)), Some(bytes));
            }
        }
        // Non-zero padding bits, padding in the middle, a bad character.
        assert_eq!(base64_decode("QQ=="), Some(b"A".to_vec()));
        assert_eq!(base64_decode("QR=="), None);
        assert_eq!(base64_decode("QQ==QQ=="), None);
        assert_eq!(base64_decode("Q-=="), None);
        assert_eq!(base64_decode("QQ"), None);
    }

    /// Every rule ID that the rules file defines.
    fn rule_ids() -> Vec<String> {
        RULES
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("- **AP-"))
            .map(|rest| format!("AP-{}", rest.chars().take(2).collect::<String>()))
            .collect()
    }

    #[test]
    fn the_rules_file_defines_the_rules_the_tests_name() {
        // AP-11 sits with the field classes of section 1, so the order of
        // the file is not the order of the numbers.
        let mut ids = rule_ids();
        ids.sort();
        assert_eq!(ids.len(), 12);
        assert_eq!(ids.first().map(String::as_str), Some("AP-01"));
        assert_eq!(ids.last().map(String::as_str), Some("AP-13"));
    }

    #[test]
    fn every_rule_has_at_least_one_test_named_for_it() {
        let missing: Vec<_> = rule_ids()
            .into_iter()
            .filter(|id| !SOURCE.contains(&format!("fn ap_{}_", &id[3..])))
            .collect();
        assert!(missing.is_empty(), "rules without a test: {missing:?}");
    }

    #[test]
    fn every_refusal_names_a_rule_of_the_file() {
        let ids = rule_ids();
        let shown = [
            ConfirmError::NotACounter.to_string(),
            ConfirmError::BadSignature.to_string(),
            WalletSigError::UnsupportedAddress.to_string(),
            WalletSigError::Malformed {
                address: "a".into(),
            }
            .to_string(),
            WalletSigError::WrongSigner {
                address: "a".into(),
            }
            .to_string(),
        ];
        for s in shown {
            assert!(ids.iter().any(|i| s.starts_with(i.as_str())), "{s}");
        }
    }

    #[test]
    fn the_sighash_helper_is_the_same_for_signing_and_checking() {
        // Guards the test signer against drifting from the checker.
        let (_, pk) = key(18);
        let script = ScriptBuf::new_p2wpkh(&pk.wpubkey_hash());
        assert_eq!(bip322_digest(&script, "x"), bip322_digest(&script, "x"));
        assert_ne!(bip322_digest(&script, "x"), bip322_digest(&script, "y"));
    }
}
