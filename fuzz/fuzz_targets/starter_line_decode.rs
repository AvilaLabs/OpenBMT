#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_core::starter_line::{StarterLine, MAX_LINE_PAYMENTS};

/// What the decoder must hold of a line it took: the same bytes back, at
/// most 16 payments and none twice.
fn check(data: &[u8]) {
    if let Ok(decoded) = StarterLine::from_bytes(data) {
        assert_eq!(decoded.to_bytes(), data);
        assert!(decoded.payments.len() <= MAX_LINE_PAYMENTS);
        for (i, p) in decoded.payments.iter().enumerate() {
            assert!(decoded.payments[..i].iter().all(|q| q.txid != p.txid));
        }
    }
}

fuzz_target!(|data: &[u8]| {
    check(data);
    // The same bytes behind the frame header, so that the input gets past
    // the magic to the parts.
    let mut framed = b"OBM_".to_vec();
    framed.push(1);
    framed.extend_from_slice(data);
    check(&framed);
});
