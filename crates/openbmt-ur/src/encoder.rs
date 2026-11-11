//! The encoder: a message as `ur:` text, whole when it fits one part, and
//! as an endless sequence of fountain-coded parts when it does not.

use crate::bytewords::encode_minimal;
use crate::error::UrError;
use crate::fountain::{
    choose_fragments, encode_part, nominal_fragment_len, partition, xor_into, MAX_FRAGMENT_LEN,
    MAX_MESSAGE_LEN, MAX_SEQ_LEN, MIN_FRAGMENT_LEN,
};
use crate::hash::crc32;

/// UR-01: 1 to 64 characters of `[a-z0-9-]`.
pub fn check_type(ur_type: &str) -> Result<(), UrError> {
    let ok = (1..=64).contains(&ur_type.len())
        && ur_type
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(UrError::Type(
            "the type is not 1 to 64 characters of a-z, 0-9, -",
        ))
    }
}

#[derive(Debug, Clone)]
pub struct Encoder {
    ur_type: String,
    message: Vec<u8>,
    /// Empty for a single-part message.
    fragments: Vec<Vec<u8>>,
    seq_len: usize,
    checksum: u32,
    seq_num: u32,
}

impl Encoder {
    /// An encoder for `cbor_payload` (the CBOR of the type, which this crate
    /// carries as bytes) of `ur_type`. A message of at most
    /// `max_fragment_len` bytes is one part; a longer one is cut into the
    /// fewest fragments of at most `max_fragment_len` bytes, using no more
    /// fragments than the message holds `min_fragment_len` bytes of
    /// (UR-02, UR-03, UR-04).
    pub fn new(
        ur_type: &str,
        cbor_payload: &[u8],
        max_fragment_len: usize,
        min_fragment_len: usize,
    ) -> Result<Self, UrError> {
        check_type(ur_type)?;
        if cbor_payload.is_empty() || cbor_payload.len() > MAX_MESSAGE_LEN {
            return Err(UrError::MessageSize);
        }
        if !(MIN_FRAGMENT_LEN..=MAX_FRAGMENT_LEN).contains(&max_fragment_len) {
            return Err(UrError::FragmentLen(
                "the largest fragment is not 10 to 2000",
            ));
        }
        if min_fragment_len < MIN_FRAGMENT_LEN || min_fragment_len > max_fragment_len {
            return Err(UrError::FragmentLen(
                "the smallest fragment is under 10 or over the largest",
            ));
        }
        let checksum = crc32(cbor_payload);
        let message = cbor_payload.to_vec();
        if message.len() <= max_fragment_len {
            return Ok(Self {
                ur_type: ur_type.to_string(),
                message,
                fragments: Vec::new(),
                seq_len: 1,
                checksum,
                seq_num: 0,
            });
        }
        let fragment_len = nominal_fragment_len(message.len(), min_fragment_len, max_fragment_len);
        if fragment_len > max_fragment_len {
            return Err(UrError::FragmentLen(
                "no fragment length fits between the smallest and the largest",
            ));
        }
        let fragments = partition(&message, fragment_len);
        if fragments.len() > MAX_SEQ_LEN {
            return Err(UrError::SeqLen);
        }
        Ok(Self {
            ur_type: ur_type.to_string(),
            message,
            seq_len: fragments.len(),
            fragments,
            checksum,
            seq_num: 0,
        })
    }

    /// True when the whole message is one part.
    pub fn is_single_part(&self) -> bool {
        self.fragments.is_empty()
    }

    /// The number of parts that carry the message once (1 when single).
    pub fn seq_len(&self) -> usize {
        self.seq_len
    }

    /// The next part as lower-case `ur:` text. The parts of a single-part
    /// message are all the same; a multi-part sequence never ends (after
    /// the first `seq_len` parts the others are mixes).
    pub fn next_part(&mut self) -> String {
        if self.is_single_part() {
            return format!("ur:{}/{}", self.ur_type, encode_minimal(&self.message));
        }
        self.seq_num = self.seq_num.checked_add(1).unwrap_or(1);
        let indexes = choose_fragments(self.seq_num, self.seq_len, self.checksum);
        let fragment_len = self.fragments.first().map_or(0, Vec::len);
        let mut data = vec![0u8; fragment_len];
        for i in indexes {
            if let Some(f) = self.fragments.get(i) {
                xor_into(&mut data, f);
            }
        }
        let body = encode_part(
            self.seq_num,
            self.seq_len,
            self.message.len(),
            self.checksum,
            &data,
        );
        format!(
            "ur:{}/{}-{}/{}",
            self.ur_type,
            self.seq_num,
            self.seq_len,
            encode_minimal(&body)
        )
    }

    /// The next part in upper case, for the QR alphanumeric mode.
    pub fn next_part_upper(&mut self) -> String {
        self.next_part().to_ascii_uppercase()
    }
}
