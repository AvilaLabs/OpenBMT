#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use openbmt_counter_ffi::{dispatch, Registry};

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

/// A request frame for `op` and `handle` with these fields.
fn frame(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = b"OBMm".to_vec();
    out.push(1);
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(fields);
    out
}

// The calls of the counter's link to the owner (CR-132, CR-135, CR-136): 24
// `link_step`, 25 `link_state` and 56 `take_relay`, with arbitrary fields,
// and with a handle no node has. Nothing is started, so the calls that need a
// node must refuse and the one that does not (`take_relay`) must refuse what
// is not a record or a relay; none may panic, and every answer is a response
// frame.
fuzz_target!(|data: &[u8]| {
    for op in [24u16, 25, 56] {
        for handle in [0u64, 1, u64::MAX] {
            let response = dispatch(registry(), &frame(op, handle, data));
            assert_eq!(response.get(..4), Some(b"OBMn".as_slice()));
        }
    }
    // The input as a whole request, for what the frame's own checks do.
    let response = dispatch(registry(), data);
    assert_eq!(response.get(..4), Some(b"OBMn".as_slice()));
});
