#![no_main]

use bitcoin::Network;
use libfuzzer_sys::fuzz_target;
use openbmt_watch::HeaderStore;

fuzz_target!(|data: &[u8]| {
    // It must never panic. A refusal names a rule; bytes that are read
    // come back out unchanged.
    match HeaderStore::from_bytes(Network::Regtest, data) {
        Ok(store) => assert_eq!(store.to_bytes(), data),
        Err(refusal) => assert!(refusal.to_string().starts_with(refusal.rule())),
    }
});
