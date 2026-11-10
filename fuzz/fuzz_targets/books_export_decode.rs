#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_books::{decode_export_header, open_export, Books};

fuzz_target!(|data: &[u8]| {
    // The bundle decoder (BK-28): it must never panic, and bundle bytes that
    // are read come back out unchanged.
    if let Ok(books) = Books::from_bundle(data) {
        assert_eq!(books.to_bundle(), data);
    }
    // The header of the encrypted file (BK-23, BK-27): it must never panic,
    // and a header that is read encodes back to the bytes it was read from.
    if let Ok(header) = decode_export_header(data) {
        let bytes = header.to_bytes();
        assert_eq!(data.get(..bytes.len()), Some(&bytes[..]));
        // Opening runs Argon2, so only a file that asks for tiny parameters is
        // opened here. Nothing in the input is a valid ciphertext for the
        // fixed passphrase, so it must always be refused, and never panic.
        let p = header.params;
        if p.m_kib <= 256 && p.t <= 2 && p.p <= 2 {
            assert!(open_export("a fixed passphrase", data).is_err());
        }
    }
});
