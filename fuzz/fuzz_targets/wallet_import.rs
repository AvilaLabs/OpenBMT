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
        let again =
            SingleKeyWallet::parse(wallet.canonical_string()).expect("a canonical string parses");
        assert_eq!(again, wallet);
        let [receive, change] = wallet.single_path_strings();
        let joined = SingleKeyWallet::parse(&format!("{receive}\n{change}"))
            .expect("the single-path strings parse");
        assert_eq!(joined, wallet);
        let (receive_body, _) = receive.split_once('#').unwrap();
        let mixed_pair = format!("{receive_body}\n{change}");
        assert_eq!(
            ImportedWallet::read(&mixed_pair),
            Ok(ImportedWallet::Single(wallet.clone()))
        );
        if wallet.account() == 0 {
            let body = wallet.canonical_string().split_once('#').unwrap().0;
            let (origin, key_suffix) = body
                .strip_prefix("wpkh([")
                .unwrap()
                .split_once(']')
                .unwrap();
            let xpub = key_suffix.split('/').next().unwrap();
            let bluewallet = format!("wpkh([{origin}]{xpub})");
            assert_eq!(
                ImportedWallet::read(&bluewallet),
                Ok(ImportedWallet::Single(wallet.clone()))
            );
            let core_export = format!(
                r#"{{"descriptors":[{{"desc":"{receive}","active":true,"internal":false}},{{"desc":"{change}","active":true,"internal":true}}]}}"#
            );
            assert_eq!(
                ImportedWallet::read(&core_export),
                Ok(ImportedWallet::Single(wallet.clone()))
            );
        }
        assert!(wallet.first_receive_address().is_ok());
    }
});
