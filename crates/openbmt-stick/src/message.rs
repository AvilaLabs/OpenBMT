//! SK-60 to SK-64: signing a change of the shop's settings. The code read is
//! the text `signmessage {path} ascii:{message}` of the owner phone; the
//! message must be exactly the wallet message of AP-06 for the numbers it
//! holds, and the path one of the two forms of SK-61. The key is derived
//! again from the typed words (and passphrase) at that path.

use bitcoin::bip32::{ChildNumber, Xpriv};
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sign_message::signed_msg_hash;
use bitcoin::{Address, CompressedPublicKey, Network, NetworkKind};
use openbmt_core::approval::{verify_wallet_message, wallet_message};
use openbmt_core::settings::LIGHTNING_NODE_BYTES;

/// The text every such code starts with (SK-60).
pub const PREFIX: &str = "signmessage ";

/// The header of a BIP-137 signature for a P2WPKH key is 39 plus the
/// recovery number (SK-64).
const HEADER_BASE: u8 = 39;

/// Why a code was refused (SK-61).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageError {
    /// The text does not start with `signmessage `.
    NotAMessage,
    /// The text is not `signmessage {path} ascii:{message}`.
    Malformed,
    /// The path is not one of the two forms.
    BadPath,
    /// The message is not the wallet message of a settings change.
    BadMessage,
    /// The key could not be worked out or the signature does not check.
    Internal,
}

/// The two forms of path (SK-61).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// `m/84h/0h/{N}h/0/0`, a single-key wallet.
    Single,
    /// `m/48h/0h/{N}h/2h/0/0`, a cosigner of the 2-of-3 wallet.
    Cosigner,
}

/// A code that passed every check of SK-61.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsMessage {
    pub kind: PathKind,
    /// The account number N, below 2^31.
    pub account: u32,
    /// The settings version in the message.
    pub version: u32,
    /// The Lightning node, `None` for "none".
    pub node: Option<[u8; LIGHTNING_NODE_BYTES]>,
    /// The settings hash.
    pub hash: [u8; 32],
    /// The message as signed.
    pub text: String,
}

fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Lower-case hex of exactly `N` bytes.
fn hex_bytes<const N: usize>(text: &str) -> Option<[u8; N]> {
    let b = text.as_bytes();
    if b.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, pair) in b.chunks(2).enumerate() {
        *out.get_mut(i)? = (hex_value(*pair.first()?)? << 4) | hex_value(*pair.get(1)?)?;
    }
    Some(out)
}

/// A number in its one canonical form: digits only, no sign, no leading zero.
fn number(text: &str) -> Option<u32> {
    let b = text.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if b.len() > 1 && b.first() == Some(&b'0') {
        return None;
    }
    text.parse().ok()
}

/// The account number of `{N}h`, below 2^31.
fn account(step: &str) -> Option<u32> {
    let n = number(step.strip_suffix('h')?)?;
    (n < 1 << 31).then_some(n)
}

fn parse_path(path: &str) -> Option<(PathKind, u32)> {
    let steps: Vec<&str> = path.split('/').collect();
    match steps.as_slice() {
        ["m", "84h", "0h", n, "0", "0"] => Some((PathKind::Single, account(n)?)),
        ["m", "48h", "0h", n, "2h", "0", "0"] => Some((PathKind::Cosigner, account(n)?)),
        _ => None,
    }
}

/// The numbers of a message; the caller rebuilds the message from them and
/// takes it only if the text is the same.
fn parse_message(message: &str) -> Option<(u32, Option<[u8; LIGHTNING_NODE_BYTES]>, [u8; 32])> {
    let rest = message.strip_prefix("OpenBMT settings change, shop settings version ")?;
    let (version, rest) = rest.split_once(", Lightning node ")?;
    let (node, hash) = rest.split_once(", settings ")?;
    let node = if node == "none" {
        None
    } else {
        Some(hex_bytes::<LIGHTNING_NODE_BYTES>(node)?)
    };
    Some((number(version)?, node, hex_bytes::<32>(hash)?))
}

/// SK-60, SK-61: the settings change in `text`, or why it is refused.
pub fn parse(text: &str) -> Result<SettingsMessage, MessageError> {
    let rest = text.strip_prefix(PREFIX).ok_or(MessageError::NotAMessage)?;
    let (path, rest) = rest.split_once(' ').ok_or(MessageError::Malformed)?;
    let message = rest.strip_prefix("ascii:").ok_or(MessageError::Malformed)?;
    let (kind, account) = parse_path(path).ok_or(MessageError::BadPath)?;
    let (version, node, hash) = parse_message(message).ok_or(MessageError::BadMessage)?;
    // The text must be exactly what the core makes of these numbers: no
    // other spacing, case, sign or leading zero is taken.
    if wallet_message(version, node.as_ref(), &hash) != message {
        return Err(MessageError::BadMessage);
    }
    Ok(SettingsMessage {
        kind,
        account,
        version,
        node,
        hash,
        text: message.to_string(),
    })
}

impl SettingsMessage {
    /// The key's path as steps.
    pub fn steps(&self) -> Vec<ChildNumber> {
        let hard = |index| ChildNumber::Hardened { index };
        let soft = |index| ChildNumber::Normal { index };
        match self.kind {
            PathKind::Single => vec![hard(84), hard(0), hard(self.account), soft(0), soft(0)],
            PathKind::Cosigner => vec![
                hard(48),
                hard(0),
                hard(self.account),
                hard(2),
                soft(0),
                soft(0),
            ],
        }
    }
}

/// The settings hash in groups of four characters (SK-63).
pub fn grouped(hash: &[u8; 32]) -> String {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    hex.as_bytes()
        .chunks(4)
        .filter_map(|c| std::str::from_utf8(c).ok())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The node of the message as hex.
pub fn node_hex(node: &[u8; LIGHTNING_NODE_BYTES]) -> String {
    node.iter().map(|b| format!("{b:02x}")).collect()
}

/// Overwrites bytes with zeros, in a way the compiler may not drop.
fn wipe(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        *b = 0;
    }
    std::hint::black_box(&*bytes);
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// The key at the message's path for `words` and `passphrase`.
fn key_at(
    words: &[&str],
    passphrase: &str,
    msg: &SettingsMessage,
) -> Result<bitcoin::secp256k1::SecretKey, MessageError> {
    let secp = Secp256k1::new();
    let m =
        bip39::Mnemonic::parse_normalized(&words.join(" ")).map_err(|_| MessageError::Internal)?;
    if m.word_count() != 12 && m.word_count() != 24 {
        return Err(MessageError::Internal);
    }
    let mut seed = m.to_seed_normalized(passphrase);
    let master = Xpriv::new_master(NetworkKind::Main, &seed);
    wipe(&mut seed);
    let mut master = master.map_err(|_| MessageError::Internal)?;
    let derived = master.derive_priv(&secp, &msg.steps());
    master.private_key.non_secure_erase();
    let mut child = derived.map_err(|_| MessageError::Internal)?;
    let key = child.private_key;
    child.private_key.non_secure_erase();
    Ok(key)
}

/// SK-63: the address the signature is for.
pub fn address_of(
    words: &[&str],
    passphrase: &str,
    msg: &SettingsMessage,
) -> Result<Address, MessageError> {
    let secp = Secp256k1::new();
    let mut key = key_at(words, passphrase, msg)?;
    let public = key.public_key(&secp);
    key.non_secure_erase();
    Ok(Address::p2wpkh(
        &CompressedPublicKey(public),
        Network::Bitcoin,
    ))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                let v = usize::try_from((n >> (18 - 6 * i)) & 63).unwrap_or(0);
                out.push(char::from(B64.get(v).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// SK-64: the BIP-137 signature of the message by the key of the address
/// (header 39 to 42), in base64. The signature is checked with the core's
/// own check of AP-07 before it is given.
pub fn sign(
    words: &[&str],
    passphrase: &str,
    msg: &SettingsMessage,
) -> Result<String, MessageError> {
    let secp = Secp256k1::new();
    let mut key = key_at(words, passphrase, msg)?;
    let public = key.public_key(&secp);
    let digest = signed_msg_hash(&msg.text).to_byte_array();
    let signature = secp.sign_ecdsa_recoverable(&Message::from_digest(digest), &key);
    key.non_secure_erase();
    let (recovery, compact) = signature.serialize_compact();
    let header = u8::try_from(recovery.to_i32())
        .ok()
        .and_then(|r| HEADER_BASE.checked_add(r))
        .ok_or(MessageError::Internal)?;
    let mut raw = Vec::with_capacity(65);
    raw.push(header);
    raw.extend_from_slice(&compact);
    let text = base64(&raw);
    let address = Address::p2wpkh(&CompressedPublicKey(public), Network::Bitcoin);
    verify_wallet_message(&address, &msg.text, &text).map_err(|_| MessageError::Internal)?;
    Ok(text)
}
