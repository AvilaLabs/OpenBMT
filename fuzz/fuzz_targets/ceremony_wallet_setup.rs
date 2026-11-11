#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_signer::WalletSetupBytes;

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged.
    if let Ok(setup) = WalletSetupBytes::from_bytes(data) {
        assert_eq!(setup.to_bytes(), data);
    }
});
