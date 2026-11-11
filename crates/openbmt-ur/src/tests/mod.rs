//! Tests, named after the rule each proves.

mod hash;
mod meta;
mod refusals;
mod roundtrip;
mod vectors;

use crate::bytewords::encode_minimal;
use crate::fountain::encode_part;
use crate::xoshiro::Xoshiro256;

/// The reference test message of BCR-2020-005: `len` bytes drawn from the
/// generator seeded with `seed`.
pub(crate) fn make_message(len: usize, seed: &str) -> Vec<u8> {
    let mut rng = Xoshiro256::from_bytes(seed.as_bytes());
    (0..len).map(|_| rng.next_byte()).collect()
}

/// The text of a multi-part part from its fields, as any sender would write
/// it (the tests change one field at a time).
pub(crate) fn part_text(
    ur_type: &str,
    seq_num: u32,
    seq_len: usize,
    message_len: usize,
    checksum: u32,
    data: &[u8],
) -> String {
    let body = encode_part(seq_num, seq_len, message_len, checksum, data);
    format!("ur:{ur_type}/{seq_num}-{seq_len}/{}", encode_minimal(&body))
}
