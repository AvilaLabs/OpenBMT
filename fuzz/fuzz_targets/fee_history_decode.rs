#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_watch::FeeHistory;

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged, and
    // the answers over a history that was read never panic either.
    if let Ok(history) = FeeHistory::from_bytes(data) {
        assert_eq!(history.to_bytes(), data);
        for tip in [0, 143, 144, 4_031, 4_032, u32::MAX] {
            let _ = history.recent_rate(tip);
            let _ = history.is_low(tip);
        }
        let mut copy = history.clone();
        if let Some(first) = history.samples().first() {
            copy.forget_from(first.height);
            assert!(copy.samples().is_empty());
        }
    }
});
