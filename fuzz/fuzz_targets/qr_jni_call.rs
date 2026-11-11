#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_qr_jni::call::{
    Calls, MAX_DECODERS, OK, OP_DECODER_NEW, OP_DECODER_RECEIVE, OP_KEY_TEXT, OP_QR_ENCODE,
    OP_UR_ENCODE, REFUSED, REFUSED_WITH_WORDS, REPLY_MAGIC, REQUEST_MAGIC, VERSION,
};

/// A reply is framed: the magic, the version, a status, and for a refusal
/// exactly a five-byte rule code (for a refusal with words, the code and
/// three texts of a 16-bit length each and nothing after). True for an ok
/// reply.
fn check(reply: &[u8]) -> bool {
    assert_eq!(reply.get(..4), Some(REPLY_MAGIC.as_slice()));
    assert_eq!(reply.get(4), Some(&VERSION));
    match reply.get(5) {
        Some(&OK) => true,
        Some(&REFUSED) => {
            let code = reply.get(6..).unwrap_or(&[]);
            assert_eq!(code.len(), 5);
            assert!(code.starts_with(b"UR-") || code.starts_with(b"QR-"));
            false
        }
        Some(&REFUSED_WITH_WORDS) => {
            let body = reply.get(6..).unwrap_or(&[]);
            assert!(body.starts_with(b"UR-"));
            let mut rest = body.get(5..).unwrap_or(&[]);
            for _ in 0..3 {
                let len = usize::from(u16::from_be_bytes([rest[0], rest[1]]));
                let (text, after) = rest[2..].split_at(len);
                assert!(!text.is_empty() && std::str::from_utf8(text).is_ok());
                rest = after;
            }
            assert!(rest.is_empty());
            false
        }
        _ => panic!("status"),
    }
}

fn head(op: u8) -> Vec<u8> {
    let mut v = REQUEST_MAGIC.to_vec();
    v.push(VERSION);
    v.push(op);
    v
}

fuzz_target!(|data: &[u8]| {
    // Arbitrary bytes, a fresh registry each time.
    let calls = Calls::new();
    check(&calls.call(data));
    // The same bytes behind a good head of each operation.
    for op in 0..=7u8 {
        let mut v = head(op);
        v.extend_from_slice(data);
        check(&calls.call(&v));
    }
    assert!(calls.open_decoders() <= MAX_DECODERS);

    // A well-formed key_text of each type that is read, and of some that
    // are not: a key text is the origin and a key.
    for ty in ["crypto-account", "crypto-hdkey", "bytes", "hdkey", "psbt"] {
        let mut v = head(OP_KEY_TEXT);
        v.push(ty.len() as u8);
        v.extend_from_slice(ty.as_bytes());
        v.extend_from_slice(&(data.len() as u32).to_be_bytes());
        v.extend_from_slice(data);
        let r = calls.call(&v);
        if check(&r) {
            assert!(r.get(6..).is_some_and(|k| k.starts_with(b"[")));
        }
    }

    // A well-formed qr_encode gives a well-formed reply.
    let text: Vec<u8> = data.iter().copied().skip(1).take(300).collect();
    let mut v = head(OP_QR_ENCODE);
    v.push(data.first().copied().unwrap_or(0) % 4);
    v.extend_from_slice(&(text.len() as u32).to_be_bytes());
    v.extend_from_slice(&text);
    let r = calls.call(&v);
    if check(&r) {
        let side = usize::from(u16::from_be_bytes([r[6], r[7]]));
        assert_eq!(r.len(), 8 + (side * side).div_ceil(8));
    }

    // A well-formed ur_encode, then a full turn of its parts into a decoder.
    let Some((&max, message)) = data.split_first() else {
        return;
    };
    let payload: Vec<u8> = message.iter().copied().take(2000).collect();
    let max = 10 + usize::from(max) * 5;
    let mut v = head(OP_UR_ENCODE);
    v.push(5);
    v.extend_from_slice(b"bytes");
    v.extend_from_slice(&(max as u16).to_be_bytes());
    v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    v.extend_from_slice(&payload);
    let r = calls.call(&v);
    if !check(&r) {
        return;
    }
    let n = u32::from_be_bytes([r[6], r[7], r[8], r[9]]) as usize;
    let h = calls.call(&head(OP_DECODER_NEW));
    assert!(check(&h));
    let handle = [h[6], h[7], h[8], h[9]];
    let mut at = 10;
    let mut done = false;
    for _ in 0..n {
        let len = u32::from_be_bytes([r[at], r[at + 1], r[at + 2], r[at + 3]]) as usize;
        at += 4;
        let mut q = head(OP_DECODER_RECEIVE);
        q.extend_from_slice(&handle);
        q.extend_from_slice(&(len as u32).to_be_bytes());
        q.extend_from_slice(&r[at..at + len]);
        at += len;
        let p = calls.call(&q);
        assert!(check(&p));
        if p[6] == 1 {
            done = true;
            break;
        }
    }
    assert!(done, "a full turn of the parts completes the message");
});
