#![no_main]

use std::sync::OnceLock;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_signer::SignerState;

const NETWORK: Network = Network::Regtest;

/// The fixed trust anchors a caller supplies at start-up: the network, this
/// device's account key, and the start of a valid snapshot of the wallet
/// (everything before the marks), so that mutated bytes get past the header.
struct Fixture {
    key: Xpub,
    prefix: Vec<u8>,
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let secp = Secp256k1::new();
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut slots = Vec::new();
        let mut keys = Vec::new();
        for n in 0..3u8 {
            let master = Xpriv::new_master(NETWORK, &[n + 1; 32]).unwrap();
            let account = master.derive_priv(&secp, &path).unwrap();
            let xpub = Xpub::from_priv(&secp, &account);
            slots.push(KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(&secp),
                origin_path: path.clone(),
                xpub,
            });
            keys.push(xpub);
        }
        let wallet = WalletDescription::new(NETWORK, slots.try_into().unwrap()).unwrap();
        let state =
            SignerState::register(wallet.canonical_string(), NETWORK, &keys[1], Some(&keys[2]))
                .unwrap();
        let bytes = state.to_bytes();
        // Magic, version, network, own slot, key 3 flag and slot, then the
        // wallet text with its two-byte length.
        let text_len_at = 9;
        let text_len = usize::from(u16::from_le_bytes([
            bytes[text_len_at],
            bytes[text_len_at + 1],
        ]));
        Fixture {
            key: keys[1],
            prefix: bytes[..text_len_at + 2 + text_len].to_vec(),
        }
    })
}

fuzz_target!(|data: &[u8]| {
    let f = fixture();
    // The input as it is, and the input as the fields after the wallet text.
    let with_prefix = [f.prefix.as_slice(), data].concat();
    for candidate in [data, with_prefix.as_slice()] {
        if let Ok(decoded) = SignerState::from_bytes(candidate, NETWORK, &f.key) {
            assert_eq!(decoded.to_bytes(), candidate);
        }
    }
});
