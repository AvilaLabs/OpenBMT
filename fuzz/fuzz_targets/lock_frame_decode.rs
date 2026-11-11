#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_lock::{decode, KdfParams, HEADER_LEN, MAGIC_LOCKED};

/// The input as it is, and the input behind a good magic and version (so
/// that mutated bytes reach the parameters, the salt and the length). A
/// frame that is read must stay inside the limit on its parameters, be of
/// exactly the length its header says and encode back to the input.
fn read(bytes: &[u8]) {
    let Ok(frame) = decode(bytes) else {
        return;
    };
    let p = frame.header.params;
    assert!(p.m_kib != 0 && p.t != 0 && p.p != 0);
    assert!(p.m_kib <= KdfParams::LIMIT.m_kib);
    assert!(p.t <= KdfParams::LIMIT.t);
    assert!(p.p <= KdfParams::LIMIT.p);
    assert_eq!(bytes.len(), HEADER_LEN + frame.ciphertext.len());
    assert_eq!(frame.header.body_len as usize, frame.ciphertext.len());
    assert_eq!(frame.to_bytes(), bytes);
}

fuzz_target!(|data: &[u8]| {
    read(data);
    let mut framed = MAGIC_LOCKED.to_vec();
    framed.push(1);
    framed.extend_from_slice(data);
    read(&framed);
});
