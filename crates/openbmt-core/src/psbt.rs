//! Where a binary PSBT ends, shared by the signer, the Stick and the owner.

/// A BIP174 compact size at `*at`, which moves past it.
fn compact_size(bytes: &[u8], at: &mut usize) -> Option<usize> {
    let first = *bytes.get(*at)?;
    *at += 1;
    let width = match first {
        0xfd => 2,
        0xfe => 4,
        0xff => 8,
        small => return Some(usize::from(small)),
    };
    let raw = bytes.get(*at..at.checked_add(width)?)?;
    *at += width;
    let value = raw.iter().rev().fold(0u64, |v, b| (v << 8) | u64::from(*b));
    usize::try_from(value).ok()
}

/// Where a PSBT of `maps` key-value maps ends in `bytes`: after the five
/// magic bytes, each map is a run of (key length, key, value length, value)
/// closed by a zero key length. `None` if the bytes end first.
pub fn psbt_len(bytes: &[u8], maps: usize) -> Option<usize> {
    let mut at = 5usize;
    for _ in 0..maps {
        loop {
            let key_len = compact_size(bytes, &mut at)?;
            if key_len == 0 {
                break;
            }
            at = at.checked_add(key_len)?;
            let value_len = compact_size(bytes, &mut at)?;
            at = at.checked_add(value_len)?;
            if at > bytes.len() {
                return None;
            }
        }
    }
    Some(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Magic, one map with one pair (key 1 byte, value 2 bytes), its end.
    const ONE_MAP: [u8; 12] = [b'p', b's', b'b', b't', 0xff, 1, 0x01, 2, 0xaa, 0xbb, 0, 0];

    #[test]
    fn ends_after_the_last_map() {
        let one = &ONE_MAP[..11];
        assert_eq!(psbt_len(one, 1), Some(11));
        assert_eq!(psbt_len(&ONE_MAP, 1), Some(11));
        assert_eq!(psbt_len(&[b'p', b's', b'b', b't', 0xff, 0], 1), Some(6));
    }

    #[test]
    fn none_when_the_bytes_end_first() {
        assert_eq!(psbt_len(&ONE_MAP[..10], 1), None);
        assert_eq!(psbt_len(&ONE_MAP[..11], 2), None);
        assert_eq!(psbt_len(b"psbt\xff", 1), None);
        // A value length that runs past the end, and one that overflows.
        assert_eq!(psbt_len(b"psbt\xff\x01\x01\x05\x00", 1), None);
        let mut big = b"psbt\xff\x01\x01\xff".to_vec();
        big.extend_from_slice(&[0xff; 8]);
        assert_eq!(psbt_len(&big, 1), None);
    }
}
