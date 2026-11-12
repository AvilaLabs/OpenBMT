#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // Never a panic. A single-key wallet that reads must read again from its
    // canonical string and from its two single-path strings, as the same
    // wallet, and must derive an address.
    if let Ok(ImportedWallet::Single(wallet)) = ImportedWallet::read(text) {
        let again = SingleKeyWallet::parse(wallet.canonical_string())
            .expect("a canonical string parses");
        assert_eq!(again, wallet);
        let [receive, change] = wallet.single_path_strings();
        let joined = SingleKeyWallet::parse(&format!("{receive}\n{change}"))
            .expect("the single-path strings parse");
        assert_eq!(joined, wallet);
        assert!(wallet.first_receive_address().is_ok());
    }
});
