//! Shop Lightning's node ID, made without the Lightning library
//! (`docs/SHOP-LIGHTNING-RULES.md` SL-72).
//!
//! The node's seed is the 64-byte child of SL-10. Its BIP-32 master key
//! gives the 32 bytes the library takes as its seed, and the library makes
//! the node's key as the hardened child `0'` of the master key of those 32
//! bytes. The network does not change a private key, so none is asked for.

use std::fmt;

use bitcoin::bip32::{ChildNumber, Xpriv};
use bitcoin::secp256k1::PublicKey;
use bitcoin::Network;

use crate::identity::secp;
use crate::settings::LIGHTNING_NODE_BYTES;

/// The node's ID for this 64-byte seed: the compressed public key of the
/// hardened child `0'` of the master key of the seed's own master key.
///
/// A 64-byte seed always gives a master key, except with a probability of
/// about 2^-127; that case gives an all-zero ID, which no settings accept
/// (SL-71) and no seed check passes.
pub fn node_id(seed64: &[u8; 64]) -> [u8; LIGHTNING_NODE_BYTES] {
    let derive = || -> Option<PublicKey> {
        let master = Xpriv::new_master(Network::Bitcoin, seed64).ok()?;
        let seed32 = master.private_key.secret_bytes();
        let library_master = Xpriv::new_master(Network::Bitcoin, &seed32).ok()?;
        let child = library_master
            .derive_priv(secp(), &[ChildNumber::from_hardened_idx(0).ok()?])
            .ok()?;
        Some(PublicKey::from_secret_key(secp(), &child.private_key))
    };
    derive().map_or([0; LIGHTNING_NODE_BYTES], |key| key.serialize())
}

/// Why a scanned seed was refused (SL-72).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedRefused {
    /// The seed's node ID is not the pinned one. Nothing is stored.
    WrongNode {
        /// The ID the settings pin.
        pinned: [u8; LIGHTNING_NODE_BYTES],
        /// The ID the scanned seed gives.
        scanned: [u8; LIGHTNING_NODE_BYTES],
    },
}

impl fmt::Display for SeedRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongNode { .. } => write!(f, "SL-72: the seed does not give the pinned node ID"),
        }
    }
}

impl std::error::Error for SeedRefused {}

/// Check a scanned seed against the pinned node ID, before anything is
/// stored (SL-72).
pub fn check_seed(
    seed64: &[u8; 64],
    pinned: &[u8; LIGHTNING_NODE_BYTES],
) -> Result<(), SeedRefused> {
    let scanned = node_id(seed64);
    if &scanned == pinned {
        Ok(())
    } else {
        Err(SeedRefused::WrongNode {
            pinned: *pinned,
            scanned,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(n: u8) -> [u8; 64] {
        std::array::from_fn(|i| n.wrapping_add(i as u8))
    }

    #[test]
    fn sl_72_the_id_is_a_compressed_point_and_follows_the_seed() {
        let a = node_id(&seed(1));
        assert!(PublicKey::from_slice(&a).is_ok());
        assert_eq!(a, node_id(&seed(1)));
        assert_ne!(a, node_id(&seed(2)));
    }

    #[test]
    fn sl_72_the_id_is_the_hardened_child_of_the_master_of_the_masters_key() {
        // A second way to the same key, written out step by step with a path.
        let s = seed(9);
        let master = Xpriv::new_master(Network::Testnet, &s).unwrap();
        let again = Xpriv::new_master(Network::Testnet, &master.private_key.secret_bytes())
            .unwrap()
            .derive_priv(
                secp(),
                &"m/0'".parse::<bitcoin::bip32::DerivationPath>().unwrap(),
            )
            .unwrap();
        let want = PublicKey::from_secret_key(secp(), &again.private_key).serialize();
        assert_eq!(node_id(&s), want);
    }

    #[test]
    fn sl_72_a_scanned_seed_must_give_the_pinned_id() {
        let pinned = node_id(&seed(3));
        assert_eq!(check_seed(&seed(3), &pinned), Ok(()));
        assert_eq!(
            check_seed(&seed(4), &pinned),
            Err(SeedRefused::WrongNode {
                pinned,
                scanned: node_id(&seed(4)),
            })
        );
        assert!(check_seed(&seed(3), &[0; LIGHTNING_NODE_BYTES]).is_err());
    }
}
