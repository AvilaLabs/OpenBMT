//! The node's seed: the 64-byte BIP-85 HEX child of the shop's words (SL-10).
//!
//! BIP-85 (the HEX application, number 128169') gives
//! `HMAC-SHA512(key = "bip-entropy-from-k", message = the private key at
//! m/83696968'/128169'/64'/0')`, all 64 bytes of it. The child cannot sign
//! for the savings and the words remake it.
//!
//! The library never takes the words. It takes the master key the stick or
//! the signer already holds (SL-11), or the 64 bytes themselves.

use bitcoin::bip32::{ChildNumber, DerivationPath, Xpriv};
use bitcoin::hashes::{sha512, Hash, HashEngine, Hmac, HmacEngine};
use bitcoin::secp256k1::Secp256k1;
use std::fmt;

const BIP85_PURPOSE: u32 = 83_696_968;
const HEX_APPLICATION: u32 = 128_169;
const HMAC_KEY: &[u8] = b"bip-entropy-from-k";
/// The length of the node's seed, in bytes.
pub const SEED_LEN: usize = 64;
/// The index of the node's seed under the shop's words: the first and only.
pub const NODE_SEED_INDEX: u32 = 0;

/// The 64 bytes the node is made from. Its debug output hides them.
#[derive(Clone, PartialEq, Eq)]
pub struct NodeSeed(pub [u8; SEED_LEN]);

impl Drop for NodeSeed {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

impl fmt::Debug for NodeSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NodeSeed(..)")
    }
}

/// The 64 bytes of BIP-85 entropy at `m/83696968'/{path...}'`. Every step of
/// `path` is hardened.
pub fn entropy_at(master: &Xpriv, path: &[u32]) -> Result<[u8; 64], bitcoin::bip32::Error> {
    let secp = Secp256k1::new();
    let mut steps = vec![ChildNumber::from_hardened_idx(BIP85_PURPOSE)?];
    for step in path {
        steps.push(ChildNumber::from_hardened_idx(*step)?);
    }
    let mut child = master.derive_priv(&secp, &DerivationPath::from(steps))?;
    let mut key = child.private_key.secret_bytes();
    child.private_key.non_secure_erase();
    let mut engine = HmacEngine::<sha512::Hash>::new(HMAC_KEY);
    engine.input(&key);
    key.fill(0);
    std::hint::black_box(&mut key);
    Ok(Hmac::from_engine(engine).to_byte_array())
}

/// The node's seed: `m/83696968'/128169'/64'/{index}'`, 64 bytes.
pub fn node_seed(master: &Xpriv, index: u32) -> Result<NodeSeed, bitcoin::bip32::Error> {
    Ok(NodeSeed(entropy_at(
        master,
        &[HEX_APPLICATION, SEED_LEN as u32, index],
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // The master key every vector of the BIP-85 text uses.
    const XPRV: &str = "xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb";

    fn master() -> Xpriv {
        Xpriv::from_str(XPRV).unwrap()
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sl_10_spec_hex_vector() {
        let seed = node_seed(&master(), NODE_SEED_INDEX).unwrap();
        assert_eq!(
            hex(&seed.0),
            "492db4698cf3b73a5a24998aa3e9d7fa96275d85724a91e71aa2d645442f878555d078fd1f1f67e368976f04137b1f7a0d19232136ca50c44614af72b5582a5c"
        );
    }

    #[test]
    fn sl_10_spec_base_vectors() {
        assert_eq!(
            hex(&entropy_at(&master(), &[0, 0]).unwrap()),
            "efecfbccffea313214232d29e71563d941229afb4338c21f9517c41aaa0d16f00b83d2a09ef747e7a64e8e2bd5a14869e693da66ce94ac2da570ab7ee48618f7"
        );
        assert_eq!(
            hex(&entropy_at(&master(), &[0, 1]).unwrap()),
            "70c6e3e8ebee8dc4c0dbba66076819bb8c09672527c4277ca8729532ad711872218f826919f6b67218adde99018a6df9095ab2b58d803b5b93ec9802085a690e"
        );
    }

    #[test]
    fn sl_10_each_index_is_another_seed() {
        let a = node_seed(&master(), 0).unwrap();
        let b = node_seed(&master(), 1).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn sl_10_debug_hides_the_bytes() {
        let seed = node_seed(&master(), 0).unwrap();
        assert!(!format!("{seed:?}").contains("492d"));
    }
}
