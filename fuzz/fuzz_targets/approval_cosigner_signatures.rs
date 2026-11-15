#![no_main]

use bitcoin::secp256k1::{Secp256k1, SecretKey};
use bitcoin::{Address, CompressedPublicKey, Network};
use libfuzzer_sys::fuzz_target;
use openbmt_core::approval::{join_cosigner_sigs, split_cosigner_sigs, WalletSigners};

// The text of the 2-of-3 wallet's signatures is read strictly: what reads
// joins back to the input, with places 1 to 3 each once and one to three
// entries. Checking any text for the three cosigners and for a single key
// never panics, and nothing the fuzzer makes is a signature.
fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Some(sigs) = split_cosigner_sigs(text) {
        assert!((1..=3).contains(&sigs.len()));
        let mut places: Vec<u8> = sigs.iter().map(|(p, _)| *p).collect();
        places.sort_unstable();
        places.dedup();
        assert_eq!(places.len(), sigs.len());
        assert!(places.iter().all(|p| (1..=3).contains(p)));
        assert_eq!(join_cosigner_sigs(&sigs), text);
    }
    let secp = Secp256k1::new();
    let address = |n: u8| {
        let sk = SecretKey::from_slice(&[n; 32]).expect("a key");
        Address::p2wpkh(&CompressedPublicKey(sk.public_key(&secp)), Network::Bitcoin)
    };
    let three = WalletSigners::TwoOfThree([address(1), address(2), address(3)]);
    let one = WalletSigners::Single(address(4));
    assert!(three.verify("message", text).is_err());
    assert!(one.verify("message", text).is_err());
});
