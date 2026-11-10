#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_courier::decode_records;

fuzz_target!(|data: &[u8]| {
    // The content of a Records message: whatever decodes must encode back to
    // the same bytes, each record behind its 4-byte little-endian length.
    if let Ok(records) = decode_records(data) {
        let mut again = Vec::new();
        for record in &records {
            let bytes = record.to_bytes();
            again.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            again.extend_from_slice(&bytes);
        }
        assert_eq!(again, data);
    }
});
