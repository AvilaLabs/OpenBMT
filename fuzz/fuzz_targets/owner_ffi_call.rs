#![no_main]

use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use openbmt_owner_ffi::{dispatch, Registry, REQUEST_MAGIC, RESPONSE_MAGIC};

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}

/// A request frame for `op` and `handle` with these fields.
fn frame(op: u16, handle: u64, fields: &[u8]) -> Vec<u8> {
    let mut out = REQUEST_MAGIC.to_vec();
    out.push(1);
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(fields);
    out
}

// The owner's call protocol with arbitrary fields: the first two bytes of the
// input pick the op (so any op number, defined or not), the rest are its
// fields, and the handle is one no node has. Nothing is started, so the calls
// that need a node must refuse; none may panic, and every answer is a
// response frame. The input as a whole request goes in too, for what the
// frame's own checks do.
fuzz_target!(|data: &[u8]| {
    if let Some((op, fields)) = data.split_first_chunk::<2>() {
        let op = u16::from_le_bytes(*op);
        for handle in [0u64, 1, u64::MAX] {
            let response = dispatch(registry(), &frame(op, handle, fields));
            assert_eq!(response.get(..4), Some(RESPONSE_MAGIC.as_slice()));
        }
    }
    let response = dispatch(registry(), data);
    assert_eq!(response.get(..4), Some(RESPONSE_MAGIC.as_slice()));
});
