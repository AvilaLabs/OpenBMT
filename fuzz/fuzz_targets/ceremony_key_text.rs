#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_signer::KeyPart;

fuzz_target!(|data: &[u8]| {
    // It must never panic; a key that is read is a key at an account path,
    // and its raw form reads back as the same key.
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(part) = KeyPart::parse(text) {
        let (fingerprint, raw) = part.to_raw();
        let again = KeyPart::from_raw(fingerprint, &raw).expect("reads back");
        assert_eq!(again, part);
    }
});
