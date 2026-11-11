#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_box::PosConfig;

fuzz_target!(|data: &[u8]| {
    // It must never panic; bytes that are read come back out unchanged, and
    // what was read is valid again and gives POS settings.
    if let Ok(config) = PosConfig::from_bytes(data) {
        assert_eq!(config.to_bytes(), data);
        assert_eq!(config.validate(), Ok(()));
        assert!(config.settings(Vec::new()).is_ok());
    }
});
