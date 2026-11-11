//! `OBM7`: what the Key app hands to `KeyStore::save`.
//!
//! ```text
//! "OBM7" | 1 or 2 | identity secret (32)
//!   | key 2 flag u8, then if 1:
//!       network u8 | master fingerprint (4) | account key, BIP32 form (78)
//!   | version 2 only: key 3, public: master fingerprint (4) | account key (78)
//!   | wallet flag u8, then if 1:
//!       network u8 | description u32 length and text (UTF-8)
//!       | owner root (32) | batch minimum u32 LE
//!       | approver snapshot u32 length and `OBM3` bytes
//! ```
//!
//! The version is 1 until key 3 has been taken (SR-67) and 2 from then on:
//! a stored state without key 3 is written exactly as before. The networks
//! are 0 mainnet, 1 testnet, 2 signet, 3 regtest, as in the
//! signer's snapshot. Decoding is strict: it succeeds only for bytes that
//! `encode` would have produced. It only parses; the wallet, the owner and
//! the snapshot are checked by the crates that own them when the Key app is
//! restored. The bytes hold secrets and are not protected here.

use bitcoin::bip32::{ChildNumber, Fingerprint, Xpriv};
use bitcoin::Network;
use openbmt_core::identity::DeviceId;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_signer::KeyPart;

/// The magic of the envelope.
pub const MAGIC_KEY_APP: [u8; 4] = *b"OBM7";

/// Longest wallet description text read back.
const MAX_DESCRIPTION: usize = 4096;
/// Largest approver snapshot read back (the signer's spend log can be big).
const MAX_SNAPSHOT: usize = 64 * 1024 * 1024;

/// 32 secret bytes, overwritten when dropped. Best effort: the compiler and
/// the operating system may keep other copies.
pub(crate) struct Secret32([u8; 32]);

impl Secret32 {
    pub(crate) fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub(crate) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for Secret32 {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

impl std::fmt::Debug for Secret32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret32(<hidden>)")
    }
}

/// Overwrite a string's bytes before it is freed. Best effort.
pub(crate) fn wipe_string(s: String) {
    let mut bytes = s.into_bytes();
    bytes.fill(0);
    std::hint::black_box(&mut bytes);
}

/// Key 2 on this device: the account key at `m/48'/coin'/0'/2'` and the
/// fingerprint of the master key it came from.
pub(crate) struct Key2 {
    pub network: Network,
    pub master_fingerprint: Fingerprint,
    pub account: Xpriv,
}

impl Drop for Key2 {
    fn drop(&mut self) {
        self.account.private_key.non_secure_erase();
    }
}

/// What every state of the Key app holds.
pub(crate) struct Material {
    pub identity: Secret32,
    pub key2: Option<Key2>,
    /// Key 3's public parts, once taken (SR-67).
    pub key3: Option<KeyPart>,
}

/// What the Key app was given when the wallet was accepted.
#[derive(Clone)]
pub(crate) struct WalletPart {
    pub network: Network,
    /// The canonical description text.
    pub description: String,
    pub owner: DeviceId,
    pub batch_min_count: u32,
}

/// A decoded envelope.
pub(crate) struct Decoded {
    pub material: Material,
    /// The wallet and the `OBM3` snapshot, when a wallet was accepted.
    pub wallet: Option<(WalletPart, Vec<u8>)>,
}

pub(crate) fn network_byte(network: Network) -> Option<u8> {
    match network {
        Network::Bitcoin => Some(0),
        Network::Testnet => Some(1),
        Network::Signet => Some(2),
        Network::Regtest => Some(3),
        _ => None,
    }
}

fn network_from(byte: u8) -> Result<Network, DecodeError> {
    match byte {
        0 => Ok(Network::Bitcoin),
        1 => Ok(Network::Testnet),
        2 => Ok(Network::Signet),
        3 => Ok(Network::Regtest),
        _ => Err(DecodeError::NonCanonical("network byte")),
    }
}

/// The bytes of the envelope. `None` for the wallet part is a Key app that
/// has an identity and no wallet yet.
pub(crate) fn encode(material: &Material, wallet: Option<(&WalletPart, &[u8])>) -> Vec<u8> {
    encode_with(material, material.key3.as_ref(), wallet)
}

/// The bytes of the envelope with `key3` in place of the material's own, so
/// that a new key 3 is stored before it replaces the old one.
pub(crate) fn encode_with(
    material: &Material,
    key3: Option<&KeyPart>,
    wallet: Option<(&WalletPart, &[u8])>,
) -> Vec<u8> {
    let mut out = wire::start(&MAGIC_KEY_APP, 512);
    if key3.is_some() {
        if let Some(version) = out.get_mut(4) {
            *version = 2;
        }
    }
    out.extend_from_slice(material.identity.bytes());
    match &material.key2 {
        Some(k) => {
            out.push(1);
            out.push(network_byte(k.network).unwrap_or(u8::MAX));
            out.extend_from_slice(k.master_fingerprint.as_bytes());
            out.extend_from_slice(&k.account.encode());
        }
        None => out.push(0),
    }
    if let Some(k) = key3 {
        let (fingerprint, raw) = k.to_raw();
        out.extend_from_slice(&fingerprint);
        out.extend_from_slice(&raw);
    }
    match wallet {
        Some((w, snapshot)) => {
            out.push(1);
            out.push(network_byte(w.network).unwrap_or(u8::MAX));
            out.extend_from_slice(&wire::len_u32(w.description.len()).to_le_bytes());
            out.extend_from_slice(w.description.as_bytes());
            out.extend_from_slice(&w.owner.to_bytes());
            out.extend_from_slice(&w.batch_min_count.to_le_bytes());
            out.extend_from_slice(&wire::len_u32(snapshot.len()).to_le_bytes());
            out.extend_from_slice(snapshot);
        }
        None => out.push(0),
    }
    out
}

/// Read an envelope, strictly.
pub(crate) fn decode(bytes: &[u8]) -> Result<Decoded, DecodeError> {
    let mut r = Reader::new(bytes);
    if r.take(4)? != MAGIC_KEY_APP {
        return Err(DecodeError::BadMagic);
    }
    let version = match r.u8()? {
        v @ (1 | 2) => v,
        v => return Err(DecodeError::UnsupportedVersion(v)),
    };
    let identity = Secret32::new(r.array::<32>()?);
    let key2 = if r.flag()? {
        let network = network_from(r.u8()?)?;
        let master_fingerprint = Fingerprint::from(r.array::<4>()?);
        let raw = r.array::<78>()?;
        let account = Xpriv::decode(&raw).map_err(|_| DecodeError::NonCanonical("key 2"))?;
        // `Xpriv::decode` ignores the padding byte before the key.
        if account.encode() != raw {
            return Err(DecodeError::NonCanonical("key 2 is not in its one form"));
        }
        if account.depth != 4 || account.child_number != (ChildNumber::Hardened { index: 2 }) {
            return Err(DecodeError::NonCanonical("key 2 is not an account key"));
        }
        if account.network != bitcoin::NetworkKind::from(network) {
            return Err(DecodeError::NonCanonical("key 2 is for another network"));
        }
        Some(Key2 {
            network,
            master_fingerprint,
            account,
        })
    } else {
        None
    };
    let key3 = if version == 2 {
        let fingerprint = r.array::<4>()?;
        let raw = r.array::<78>()?;
        let k = KeyPart::from_raw(fingerprint, &raw)
            .ok_or(DecodeError::NonCanonical("key 3 is not in its one form"))?;
        if let Some(k2) = &key2 {
            if k.kind() != bitcoin::NetworkKind::from(k2.network) {
                return Err(DecodeError::NonCanonical("key 3 is for another network"));
            }
            let secp = bitcoin::secp256k1::Secp256k1::new();
            let two = KeyPart {
                fingerprint: k2.master_fingerprint,
                xpub: bitcoin::bip32::Xpub::from_priv(&secp, &k2.account),
            };
            if two.same_as(&k) {
                return Err(DecodeError::NonCanonical("key 3 is key 2"));
            }
        }
        Some(k)
    } else {
        None
    };
    let material = Material {
        identity,
        key2,
        key3,
    };
    let wallet = if r.flag()? {
        let network = network_from(r.u8()?)?;
        if let Some(k) = &material.key2 {
            if k.network != network {
                return Err(DecodeError::NonCanonical(
                    "key 2 and wallet networks differ",
                ));
            }
        }
        let n = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if n > MAX_DESCRIPTION {
            return Err(DecodeError::TooLarge);
        }
        let description = String::from_utf8(r.take(n)?.to_vec())
            .map_err(|_| DecodeError::NonCanonical("description is not UTF-8"))?;
        let owner = r.device_id()?;
        let batch_min_count = r.u32_le()?;
        let m = usize::try_from(r.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if m > MAX_SNAPSHOT {
            return Err(DecodeError::TooLarge);
        }
        let snapshot = r.take(m)?.to_vec();
        Some((
            WalletPart {
                network,
                description,
                owner,
                batch_min_count,
            },
            snapshot,
        ))
    } else {
        None
    };
    r.finish()?;
    Ok(Decoded { material, wallet })
}
