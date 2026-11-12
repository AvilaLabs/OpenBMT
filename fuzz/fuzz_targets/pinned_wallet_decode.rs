#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::pinned_wallet::PinnedWallet;

fuzz_target!(|data: &[u8]| {
    if let Ok(decoded) = PinnedWallet::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        // The text it holds is the one the import reads back, and the first
        // addresses derive without a panic.
        assert!(PinnedWallet::from_text(decoded.canonical_string()).is_ok());
        for index in [0, 1, 999, 1 << 31] {
            let _ = decoded.receive_script(index);
            let _ = decoded.receive_address(index);
        }
    }
});
