#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_ur::bytewords::encode_minimal;
use openbmt_ur::{Decoder, MAX_SEQ_LEN};

/// The most parts one input feeds, so that one run stays far under a second.
const MAX_PARTS: usize = 300;
const FRAGMENT_LEN: usize = 10;

/// A CBOR unsigned number in its shortest form.
fn uint(out: &mut Vec<u8>, v: u32) {
    match v {
        0..=23 => out.push(v as u8),
        24..=255 => out.extend_from_slice(&[0x18, v as u8]),
        256..=65535 => {
            out.push(0x19);
            out.extend_from_slice(&(v as u16).to_be_bytes());
        }
        _ => {
            out.push(0x1a);
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
}

/// The text of one part: the body is `[seq_num, seq_len, message_len,
/// checksum, data]` as the format says, with its CRC-32 in minimal Bytewords.
fn part(seq_num: u32, seq_len: u32, message_len: u32, checksum: u32, data: &[u8]) -> String {
    let mut body = vec![0x85];
    uint(&mut body, seq_num);
    uint(&mut body, seq_len);
    uint(&mut body, message_len);
    uint(&mut body, checksum);
    body.push(0x40 | data.len() as u8);
    body.extend_from_slice(data);
    format!("ur:bytes/{seq_num}-{seq_len}/{}", encode_minimal(&body))
}

// A stream of up to 10000 fragments fed parts the input picks: a header of the
// length of the sequence (100 to 10000), the length of the message and the
// checksum, then two bytes for each part giving its sequence number (the
// first `seq_len` are single fragments, the rest, three times as many, are
// mixes of many fragments) and one byte for its data. The decoder must never
// panic, and a finished message must have the length and the checksum the
// header says.
fuzz_target!(|data: &[u8]| {
    let Some((head, rest)) = data.split_first_chunk::<7>() else {
        return;
    };
    let seq_len = 100 + u32::from(u16::from_le_bytes([head[0], head[1]])) % 9901;
    assert!(seq_len as usize <= MAX_SEQ_LEN);
    let message_len = (seq_len - 1) * FRAGMENT_LEN as u32 + 1 + u32::from(head[2]) % 10;
    let checksum = u32::from_le_bytes([head[3], head[4], head[5], head[6]]);
    let mut d = Decoder::new();
    for chunk in rest.chunks_exact(3).take(MAX_PARTS) {
        let seq_num = 1 + u32::from(u16::from_le_bytes([chunk[0], chunk[1]])) % (seq_len * 4);
        let mut x = u64::from(seq_num) << 8 | u64::from(chunk[2]) | 1;
        let mut fragment = [0u8; FRAGMENT_LEN];
        for b in fragment.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        match d.receive(&part(seq_num, seq_len, message_len, checksum, &fragment)) {
            Ok(p) => assert!(p.estimated_fraction <= 1.0),
            Err(e) => assert!(e.to_string().starts_with("UR-")),
        }
    }
    if let Some((ty, got)) = d.result() {
        assert_eq!(ty, "bytes");
        assert_eq!(got.len(), message_len as usize);
    }
});
