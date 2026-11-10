//! Fixed keys for tests: BIP39 test-vector mnemonics and numbered identities.

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::Network;
use std::convert::Infallible;
use std::str::FromStr;

use crate::identity::{secp, Identity};
use crate::wallet::{coin_type, KeySlot, WalletDescription};
use crate::wire::DecodeError;

/// BIP39 (Trezor) test-vector mnemonics, all distinct seeds.
pub const MNEMONICS: [&str; 4] = [
    "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
    "legal winner thank year wave sausage worth useful legal winner thank yellow",
    "letter advice cage absurd amount doctor acoustic avoid letter advice cage above",
    "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong",
];

pub fn slot_from_mnemonic(mnemonic: &str, index: u8, network: Network) -> KeySlot {
    let seed = bip39::Mnemonic::parse(mnemonic).unwrap().to_seed("");
    let master = Xpriv::new_master(network, &seed).unwrap();
    let path = DerivationPath::from_str(&format!("m/48h/{}h/0h/2h", coin_type(network))).unwrap();
    let account = master.derive_priv(secp(), &path).unwrap();
    KeySlot {
        index,
        origin_fingerprint: master.fingerprint(secp()),
        origin_path: path,
        xpub: Xpub::from_priv(secp(), &account),
    }
}

/// The standard test wallet from the first three mnemonics.
pub fn wallet(network: Network) -> WalletDescription {
    WalletDescription::new(
        network,
        [
            slot_from_mnemonic(MNEMONICS[0], 1, network),
            slot_from_mnemonic(MNEMONICS[1], 2, network),
            slot_from_mnemonic(MNEMONICS[2], 3, network),
        ],
    )
    .unwrap()
}

/// A numbered identity (n in 1..=250).
pub fn identity(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

/// A store that always succeeds, for tests that are not about persisting.
pub fn persist_ok<T: ?Sized>(_: &T) -> Result<(), Infallible> {
    Ok(())
}

/// A store that always fails.
pub fn persist_fail<T: ?Sized>(_: &T) -> Result<(), &'static str> {
    Err("disk full")
}

/// Strictness checks shared by every wire type. `valid` must be a message
/// the type accepts. Checks that it re-encodes to the same bytes, that every
/// proper prefix is refused, that one extra byte is `TrailingBytes`, and that
/// flipping any single bit never panics and either fails or decodes to a
/// value that encodes back to the mutated bytes.
pub fn assert_strict_decoding<T>(
    valid: &[u8],
    decode: impl Fn(&[u8]) -> Result<T, DecodeError>,
    encode: impl Fn(&T) -> Vec<u8>,
) {
    let value = decode(valid).expect("valid message decodes");
    assert_eq!(encode(&value), valid, "canonical re-encoding");
    for n in 0..valid.len() {
        assert!(decode(&valid[..n]).is_err(), "prefix of {n} bytes decoded");
    }
    let mut longer = valid.to_vec();
    longer.push(0);
    assert_eq!(decode(&longer).err(), Some(DecodeError::TrailingBytes));
    let mut m = valid.to_vec();
    for i in 0..m.len() {
        for bit in 0..8 {
            m[i] ^= 1 << bit;
            if let Ok(v) = decode(&m) {
                assert_eq!(encode(&v), m, "byte {i} bit {bit} decoded non-canonically");
            }
            m[i] ^= 1 << bit;
        }
    }
}
