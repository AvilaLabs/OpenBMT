//! The secrets of one swap (SW-04) and the key an output is made from
//! (SW-26): the key aggregation of BIP 327, written here on the point
//! arithmetic of `secp256k1` because the package has no MuSig2 module.

use std::fmt;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{PublicKey, Scalar, Secp256k1, SecretKey};
use bitcoin::XOnlyPublicKey;

use crate::error::Refused;

/// The preimage of a swap and the claim key. Made from bytes the caller
/// gives; the claim key is never a wallet key.
#[derive(Clone, PartialEq, Eq)]
pub struct Secrets {
    preimage: [u8; 32],
    claim_key: SecretKey,
}

impl Secrets {
    /// SW-04: from 32 random bytes for the preimage and 32 for the claim key.
    pub fn from_random(preimage: [u8; 32], claim_key: [u8; 32]) -> Result<Self, Refused> {
        if preimage == [0u8; 32] {
            return Err(Refused::BadSecret);
        }
        let claim_key = SecretKey::from_slice(&claim_key).map_err(|_| Refused::BadSecret)?;
        Ok(Self {
            preimage,
            claim_key,
        })
    }

    /// The preimage. Showing it to anyone lets them settle the invoice.
    pub fn preimage(&self) -> &[u8; 32] {
        &self.preimage
    }

    /// The claim key.
    pub fn claim_key(&self) -> &SecretKey {
        &self.claim_key
    }

    /// The SHA-256 of the preimage: what the service is given.
    pub fn preimage_hash(&self) -> [u8; 32] {
        sha256::Hash::hash(&self.preimage).to_byte_array()
    }

    /// The public key of the claim key.
    pub fn claim_public(&self) -> PublicKey {
        PublicKey::from_secret_key(&Secp256k1::signing_only(), &self.claim_key)
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secrets(..)")
    }
}

/// BIP 340's tagged hash.
fn tagged(tag: &str, parts: &[&[u8]]) -> [u8; 32] {
    let t = sha256::Hash::hash(tag.as_bytes()).to_byte_array();
    let mut data = Vec::with_capacity(64 + parts.iter().map(|p| p.len()).sum::<usize>());
    data.extend_from_slice(&t);
    data.extend_from_slice(&t);
    for p in parts {
        data.extend_from_slice(p);
    }
    sha256::Hash::hash(&data).to_byte_array()
}

/// SW-26: the x-only aggregate key of BIP 327 for `keys`, in the order
/// given (the keys are not sorted). The coefficient of every key is the
/// hash of the list and the key, except the first key that differs from the
/// first of the list, whose coefficient is 1.
pub fn key_agg(keys: &[PublicKey]) -> Result<XOnlyPublicKey, Refused> {
    let secp = Secp256k1::new();
    let first = keys.first().ok_or(Refused::BadRefundKey)?;
    let serialized: Vec<[u8; 33]> = keys.iter().map(PublicKey::serialize).collect();
    let parts: Vec<&[u8]> = serialized.iter().map(|k| k.as_slice()).collect();
    let list = tagged("KeyAgg list", &parts);
    let second = keys.iter().find(|k| *k != first);
    let mut terms = Vec::with_capacity(keys.len());
    for key in keys {
        let coefficient = if Some(key) == second {
            Scalar::ONE
        } else {
            Scalar::from_be_bytes(tagged("KeyAgg coefficient", &[&list, &key.serialize()]))
                .map_err(|_| Refused::BadRefundKey)?
        };
        terms.push(
            key.mul_tweak(&secp, &coefficient)
                .map_err(|_| Refused::BadRefundKey)?,
        );
    }
    let refs: Vec<&PublicKey> = terms.iter().collect();
    let sum = PublicKey::combine_keys(&refs).map_err(|_| Refused::BadRefundKey)?;
    Ok(sum.x_only_public_key().0)
}
