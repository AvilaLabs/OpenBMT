//! SK-32: the BIP-85 children of the 12 words that are the keys of the shop's
//! two everyday wallets: the refund float (a BIP39 child of 12 words) and Shop
//! Lightning (a HEX child of 64 bytes).

use std::str::FromStr;

use bitcoin::bip32::{DerivationPath, Xpriv};
use bitcoin::hashes::{hmac, sha512, Hash, HashEngine};
use bitcoin::secp256k1::Secp256k1;

use crate::mix::ENTROPY_BYTES;
use crate::outputs::{master_of, OutputsError};

/// The most the owner may raise the refund float's index to (SK-32).
pub const FLOAT_INDEX_MAX: u32 = 99;

/// The BIP-85 application number of a BIP39 child (BIP-85, section BIP39).
const APP_BIP39: u32 = 39;
/// The BIP-85 application number of a HEX child (BIP-85, section HEX).
const APP_HEX: u32 = 128169;
/// English, the BIP39 child's language (BIP-85).
const ENGLISH: u32 = 0;
/// The words of the refund float.
const FLOAT_WORDS: u32 = 12;
/// The bytes of the Shop Lightning child.
pub const LIGHTNING_BYTES: usize = 64;

/// The 64 bytes of BIP-85 entropy of the key at `path` under `master`: the
/// HMAC-SHA512, keyed with `bip-entropy-from-k`, of the child's private key.
pub fn entropy_at(master: &Xpriv, path: &str) -> Result<[u8; 64], OutputsError> {
    let secp = Secp256k1::new();
    let path = DerivationPath::from_str(path).map_err(|_| OutputsError)?;
    let mut child = master.derive_priv(&secp, &path).map_err(|_| OutputsError)?;
    let mut key = child.private_key.secret_bytes();
    let mut engine = hmac::HmacEngine::<sha512::Hash>::new(b"bip-entropy-from-k");
    engine.input(&key);
    let out = hmac::Hmac::<sha512::Hash>::from_engine(engine).to_byte_array();
    key.fill(0);
    child.private_key.non_secure_erase();
    Ok(out)
}

/// The path of the refund float's child at `index` (SK-32).
pub fn float_path(index: u32) -> String {
    format!("m/83696968'/{APP_BIP39}'/{ENGLISH}'/{FLOAT_WORDS}'/{index}'")
}

/// The path of the Shop Lightning child (SK-32, SL-10).
pub fn lightning_path() -> String {
    format!("m/83696968'/{APP_HEX}'/{LIGHTNING_BYTES}'/0'")
}

/// The 16 bytes of entropy of the refund float's 12 words at `index`, 0 to
/// [`FLOAT_INDEX_MAX`] (the first 16 bytes of the BIP-85 entropy).
pub fn float_entropy(words: &[&str], index: u32) -> Result<[u8; ENTROPY_BYTES], OutputsError> {
    if index > FLOAT_INDEX_MAX {
        return Err(OutputsError);
    }
    let master = master_of(words)?;
    let mut full = entropy_at(&master, &float_path(index))?;
    let mut out = [0u8; ENTROPY_BYTES];
    out.copy_from_slice(&full[..ENTROPY_BYTES]);
    full.fill(0);
    Ok(out)
}

/// The 64 bytes of the Shop Lightning child. The caller wipes them.
pub fn lightning_child(words: &[&str]) -> Result<[u8; LIGHTNING_BYTES], OutputsError> {
    let master = master_of(words)?;
    entropy_at(&master, &lightning_path())
}

/// The 128 lower-case hexadecimal characters of the Shop Lightning child.
pub fn lightning_hex(words: &[&str]) -> Result<String, OutputsError> {
    let mut full = lightning_child(words)?;
    let text: String = full.iter().map(|b| format!("{b:02x}")).collect();
    full.fill(0);
    Ok(text)
}

/// The 66 lower-case hexadecimal characters of the Shop Lightning node's ID
/// (SK-35, SL-72), made by `openbmt-core` as the counter makes it.
pub fn lightning_node_id_hex(words: &[&str]) -> Result<String, OutputsError> {
    let mut full = lightning_child(words)?;
    let id = openbmt_core::shop_lightning::node_id(&full);
    full.fill(0);
    Ok(id.iter().map(|b| format!("{b:02x}")).collect())
}
