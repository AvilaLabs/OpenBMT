#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_ur::cbor::encode_bytes;
use openbmt_ur::{ur_to_key_text, ALL_KEY_REFUSALS};

/// The types the reader takes, and some that it refuses.
const TYPES: [&str; 8] = [
    "crypto-account",
    "crypto-hdkey",
    "bytes",
    "hdkey",
    "account-descriptor",
    "output-descriptor",
    "crypto-output",
    "psbt",
];

fn check(ur_type: &str, message: &[u8]) {
    match ur_to_key_text(ur_type, message) {
        Ok(key) => {
            let text = key.as_str();
            let (origin, key) = text.split_once(']').expect("an origin");
            assert!(origin.starts_with('['));
            assert!(
                origin.ends_with("/48h/0h/0h/2h") || origin.ends_with("/48h/1h/0h/2h"),
                "{text}"
            );
            assert_eq!(origin.len(), "[56c4fac3/48h/0h/0h/2h".len());
            assert!(origin[1..9].bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(key.starts_with("xpub") || key.starts_with("tpub"));
            assert!(key.len() > 100 && key.len() < 120);
        }
        Err(e) => {
            assert!(e.to_string().starts_with("UR-"));
            assert!(ALL_KEY_REFUSALS.contains(&e));
            let w = e.words();
            assert!(!w.heading.is_empty() && !w.what.is_empty() && !w.next.is_empty());
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&first, message)) = data.split_first() else {
        return;
    };
    // The input as the message of each type, and as the content of a byte
    // string for `bytes` (the apps give the byte string back to the call).
    check(TYPES[usize::from(first) % TYPES.len()], message);
    check("bytes", &encode_bytes(message));
    for ty in TYPES {
        check(ty, data);
    }
});
