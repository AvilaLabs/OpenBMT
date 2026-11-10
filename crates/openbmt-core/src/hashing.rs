//! BIP340-style tagged hash, shared by `identity` and `words`.

use bitcoin::hashes::{sha256, Hash, HashEngine};

/// BIP340 tagged hash of `msg` under `tag`.
pub fn tagged_hash(tag: &str, msg: &[u8]) -> [u8; 32] {
    let th = sha256::Hash::hash(tag.as_bytes());
    let mut eng = sha256::Hash::engine();
    eng.input(th.as_byte_array());
    eng.input(th.as_byte_array());
    eng.input(msg);
    sha256::Hash::from_engine(eng).to_byte_array()
}
