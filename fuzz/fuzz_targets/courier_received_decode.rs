#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_courier::wire::decode_received;

fuzz_target!(|data: &[u8]| {
    // A Records received content is a count and a head, and nothing else:
    // one that is read is exactly 40 bytes and writes back to the input.
    if let Ok((count, head)) = decode_received(data) {
        assert_eq!(data.len(), 40);
        let mut again = Vec::with_capacity(40);
        again.extend_from_slice(&count.to_le_bytes());
        again.extend_from_slice(&head);
        assert_eq!(again, data);
    }
});
