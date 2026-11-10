#![no_main]

use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_core::wallet::WalletDescription;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    for network in [Network::Bitcoin, Network::Testnet] {
        if let Ok(wallet) = WalletDescription::parse(text, network) {
            let again = WalletDescription::parse(wallet.canonical_string(), network)
                .expect("a canonical string parses");
            // `WalletDescription` has no equality; the canonical string is
            // its whole identity.
            assert_eq!(again.canonical_string(), wallet.canonical_string());
        }
    }
});
