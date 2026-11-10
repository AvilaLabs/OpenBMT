#![no_main]

use std::sync::OnceLock;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_signer::SignerState;

const NETWORK: Network = Network::Regtest;
const NOW: u64 = 10_000_000;

/// A registered state with fixed keys. Registration happens once.
fn signer() -> &'static SignerState {
    static SIGNER: OnceLock<SignerState> = OnceLock::new();
    SIGNER.get_or_init(|| {
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
        SignerState::register(wallet.canonical_string(), NETWORK, &keys[1], Some(&keys[2]))
            .unwrap()
    })
}

fuzz_target!(|data: &[u8]| {
    // Any bytes: a review either gives a result or a refusal, and never
    // panics. It changes nothing.
    let state = signer();
    let before = state.latest_time_seen();
    let _ = state.review(data, NOW);
    assert_eq!(state.latest_time_seen(), before);
});
