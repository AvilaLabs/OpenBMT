#![no_main]

use std::convert::Infallible;
use std::sync::OnceLock;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_signer::{Refusal, SignError, SignerState};

const NETWORK: Network = Network::Regtest;
const NOW: u64 = 10_000_000;

/// A registered state with fixed keys, and this device's account key.
/// Registration happens once.
fn signer() -> &'static (SignerState, Xpriv) {
    static SIGNER: OnceLock<(SignerState, Xpriv)> = OnceLock::new();
    SIGNER.get_or_init(|| {
        let secp = Secp256k1::new();
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let mut slots = Vec::new();
        let mut keys = Vec::new();
        let mut accounts = Vec::new();
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
            accounts.push(account);
        }
        let wallet = WalletDescription::new(NETWORK, slots.try_into().unwrap()).unwrap();
        let state =
            SignerState::register(wallet.canonical_string(), NETWORK, &keys[1], Some(&keys[2]))
                .unwrap();
        (state, accounts[1])
    })
}

fuzz_target!(|data: &[u8]| {
    // Any bytes: a review gives a result or a refusal and never panics.
    let (state, key) = signer();
    let Ok(review) = state.review(data, NOW) else {
        return;
    };
    // A file that passes is signed on a copy of the state. Signing gives a
    // file or a refusal; a file it gives is a PSBT that this signer then
    // reports as already signed, and the new state reads back from its
    // bytes unchanged.
    let txid = review.txid();
    let mut next = state.clone();
    let before = next.totals(NOW);
    let mut stored = Vec::new();
    let result = next.sign(review, key, NOW, |bytes: &[u8]| {
        stored = bytes.to_vec();
        Ok::<(), Infallible>(())
    });
    match result {
        Ok(signed) => {
            assert_eq!(signed.txid, txid);
            assert_eq!(stored, next.to_bytes());
            let xpub = next.wallet().slots()[1].xpub;
            let back = SignerState::from_bytes(&stored, NETWORK, &xpub).unwrap();
            assert_eq!(back.to_bytes(), stored);
            let after = next.totals(NOW);
            assert!(after.other_day >= before.other_day);
            assert!(after.other_week >= before.other_week);
            assert!(after.allowlisted_day >= before.allowlisted_day);
            assert!(after.allowlisted_week >= before.allowlisted_week);
            assert_eq!(
                next.review(&signed.psbt, NOW).unwrap_err(),
                Refusal::AlreadySigned
            );
        }
        Err(SignError::Refused(_)) => assert!(stored.is_empty()),
        Err(SignError::Persist(never)) => match never {},
    }
});
