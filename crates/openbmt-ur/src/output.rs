//! The `crypto-output` type for a shared wallet (UR-32): the descriptor
//! `wsh(sortedmulti(k, key, key, ...))` as the CBOR that SeedSigner, Krux and
//! Sparrow read, so that a signer can be told the wallet by a code and
//! check the addresses and the change of a PSBT against it.

use crate::cbor::{encode_bytes, push_head};
use crate::error::UrError;

/// The type this crate writes for a descriptor.
pub const TYPE_CRYPTO_OUTPUT: &str = "crypto-output";

/// The tags of the script expressions and the key types (BCR-2020-010 and
/// BCR-2020-007): `wsh`, `sortedmulti`, `crypto-hdkey`, `crypto-keypath`,
/// `crypto-coin-info`.
const TAG_WSH: u64 = 401;
const TAG_SORTEDMULTI: u64 = 407;
const TAG_HDKEY: u64 = 303;
const TAG_KEYPATH: u64 = 304;
const TAG_COIN_INFO: u64 = 305;

const MAJOR_UINT: u8 = 0;
const MAJOR_ARRAY: u8 = 4;
const MAJOR_MAP: u8 = 5;
const MAJOR_TAG: u8 = 6;
const FALSE: u8 = 0xf4;
const TRUE: u8 = 0xf5;

/// The most keys of a `sortedmulti` (UR-32): the script's own limit for a
/// `wsh` output.
pub const MAX_OUTPUT_KEYS: usize = 15;

/// One key of an output descriptor: an extended public key at the end of
/// its origin, with the single step and the wildcard that follow it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputKey {
    /// The master key's fingerprint, the first four bytes of its hash.
    pub master_fingerprint: [u8; 4],
    /// The path from the master key to this key, each step an index and
    /// whether it is hardened.
    pub origin: Vec<(u32, bool)>,
    /// The fingerprint of the key's parent, from the extended key.
    pub parent_fingerprint: [u8; 4],
    /// The compressed public key.
    pub key: [u8; 33],
    /// The chain code.
    pub chain_code: [u8; 32],
    /// Whether the key is for a test network.
    pub testnet: bool,
    /// The one unhardened step after the key, before the wildcard: 0 for
    /// receive addresses and 1 for change.
    pub chain: u32,
}

fn uint(out: &mut Vec<u8>, v: u64) {
    push_head(out, MAJOR_UINT, v);
}

fn fingerprint(b: [u8; 4]) -> u64 {
    u64::from(u32::from_be_bytes(b))
}

fn tagged(out: &mut Vec<u8>, tag: u64) {
    push_head(out, MAJOR_TAG, tag);
}

/// The CBOR of a `crypto-hdkey` that carries the key, its origin and the
/// steps after it, as the signers write it: the key data, the chain code,
/// the coin and network, the origin with its fingerprint and depth, the
/// children and the parent's fingerprint, under the map keys 3 to 8.
pub(crate) fn hdkey(out: &mut Vec<u8>, k: &OutputKey) {
    tagged(out, TAG_HDKEY);
    push_head(out, MAJOR_MAP, 6);
    uint(out, 3);
    out.extend_from_slice(&encode_bytes(&k.key));
    uint(out, 4);
    out.extend_from_slice(&encode_bytes(&k.chain_code));
    // The coin and the network.
    uint(out, 5);
    tagged(out, TAG_COIN_INFO);
    push_head(out, MAJOR_MAP, 2);
    uint(out, 1);
    uint(out, 0);
    uint(out, 2);
    uint(out, u64::from(k.testnet));
    // The origin.
    uint(out, 6);
    tagged(out, TAG_KEYPATH);
    push_head(out, MAJOR_MAP, 3);
    uint(out, 1);
    push_head(out, MAJOR_ARRAY, 2 * k.origin.len() as u64);
    for (index, hardened) in &k.origin {
        uint(out, u64::from(*index));
        out.push(if *hardened { TRUE } else { FALSE });
    }
    uint(out, 2);
    uint(out, fingerprint(k.master_fingerprint));
    uint(out, 3);
    uint(out, k.origin.len() as u64);
    // The children: one step, then the wildcard (an empty array and a flag).
    uint(out, 7);
    tagged(out, TAG_KEYPATH);
    push_head(out, MAJOR_MAP, 1);
    uint(out, 1);
    push_head(out, MAJOR_ARRAY, 4);
    uint(out, u64::from(k.chain));
    out.push(FALSE);
    push_head(out, MAJOR_ARRAY, 0);
    out.push(FALSE);
    uint(out, 8);
    uint(out, fingerprint(k.parent_fingerprint));
}

/// UR-32: the CBOR of the `crypto-output` for `wsh(sortedmulti(threshold,
/// keys...))`, which is the message of a `ur:crypto-output` (the keys in the
/// order given). The output is `401(407({1: threshold, 2: [303({...}), ...]}))`.
pub fn wsh_sortedmulti_cbor(threshold: u8, keys: &[OutputKey]) -> Result<Vec<u8>, UrError> {
    if keys.is_empty() || keys.len() > MAX_OUTPUT_KEYS {
        return Err(UrError::Output("one to fifteen keys"));
    }
    if threshold == 0 || usize::from(threshold) > keys.len() {
        return Err(UrError::Output(
            "a threshold of one up to the number of keys",
        ));
    }
    let mut out = Vec::with_capacity(keys.len() * 160 + 16);
    tagged(&mut out, TAG_WSH);
    tagged(&mut out, TAG_SORTEDMULTI);
    push_head(&mut out, MAJOR_MAP, 2);
    uint(&mut out, 1);
    uint(&mut out, u64::from(threshold));
    uint(&mut out, 2);
    push_head(&mut out, MAJOR_ARRAY, keys.len() as u64);
    for k in keys {
        if k.origin.is_empty() || k.origin.len() > 16 {
            return Err(UrError::Output("an origin of one to sixteen steps"));
        }
        hdkey(&mut out, k);
    }
    Ok(out)
}

/// UR-32: an encoder for the CBOR of an output descriptor
/// ([`wsh_sortedmulti_cbor`]), as a `crypto-output` UR.
pub fn output_to_ur_parts(
    cbor: &[u8],
    max_fragment_len: usize,
) -> Result<crate::encoder::Encoder, UrError> {
    crate::encoder::Encoder::new(
        TYPE_CRYPTO_OUTPUT,
        cbor,
        max_fragment_len,
        crate::fountain::MIN_FRAGMENT_LEN,
    )
}
