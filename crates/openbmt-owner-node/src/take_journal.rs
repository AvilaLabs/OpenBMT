//! Durable roll-forward record for an owner response and key 1 policy state.

use bitcoin::hashes::{sha256, Hash};
use openbmt_core::wire::{self, Reader};

const MAGIC: [u8; 4] = *b"OBMi";
const MAX_PART: usize = 80 * 1024 * 1024;
/// Largest complete journal record, including framing and checksum.
pub(crate) const MAX_JOURNAL_BYTES: usize = 5 + 8 + 2 * MAX_PART + 32;

/// The committed owner queue and matching key 1 snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TakeJournal {
    pub(crate) queue: Vec<u8>,
    pub(crate) key1: Vec<u8>,
}

impl TakeJournal {
    pub(crate) fn to_bytes(&self) -> Result<Vec<u8>, String> {
        if self.queue.len() > MAX_PART || self.key1.len() > MAX_PART {
            return Err("take journal part is too large".to_string());
        }
        let queue_len = u32::try_from(self.queue.len()).map_err(|_| "queue is too large")?;
        let key_len = u32::try_from(self.key1.len()).map_err(|_| "key state is too large")?;
        let capacity = self
            .queue
            .len()
            .checked_add(self.key1.len())
            .and_then(|length| length.checked_add(8))
            .ok_or("take journal is too large")?;
        let mut out = wire::start(&MAGIC, capacity);
        out.extend_from_slice(&queue_len.to_le_bytes());
        out.extend_from_slice(&key_len.to_le_bytes());
        out.extend_from_slice(&self.queue);
        out.extend_from_slice(&self.key1);
        let checksum = sha256::Hash::hash(&out);
        out.extend_from_slice(&checksum.to_byte_array());
        Ok(out)
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err("take journal is too large".to_string());
        }
        let prefix_len = bytes
            .len()
            .checked_sub(32)
            .ok_or_else(|| "take journal is truncated".to_string())?;
        let mut r = Reader::framed(bytes, &MAGIC).map_err(|e| e.to_string())?;
        let queue_len = usize::try_from(r.u32_le().map_err(|e| e.to_string())?)
            .map_err(|_| "queue length is too large")?;
        let key_len = usize::try_from(r.u32_le().map_err(|e| e.to_string())?)
            .map_err(|_| "key state length is too large")?;
        if queue_len > MAX_PART || key_len > MAX_PART {
            return Err("take journal part is too large".to_string());
        }
        let queue = r.take(queue_len).map_err(|e| e.to_string())?.to_vec();
        let key1 = r.take(key_len).map_err(|e| e.to_string())?.to_vec();
        let checksum = r.array::<32>().map_err(|e| e.to_string())?;
        r.finish().map_err(|e| e.to_string())?;
        let prefix = bytes.get(..prefix_len).ok_or("take journal is truncated")?;
        if sha256::Hash::hash(prefix).to_byte_array() != checksum {
            return Err("take journal checksum does not match".to_string());
        }
        Ok(Self { queue, key1 })
    }
}

#[cfg(test)]
mod tests {
    use super::TakeJournal;

    #[test]
    fn journal_round_trips_and_rejects_truncation_trailing_bytes_and_bit_flips() {
        let journal = TakeJournal {
            queue: vec![1, 2, 3],
            key1: vec![4, 5, 6],
        };
        let bytes = journal.to_bytes().unwrap();
        assert_eq!(TakeJournal::from_bytes(&bytes).unwrap(), journal);

        assert!(TakeJournal::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(TakeJournal::from_bytes(&trailing).is_err());
        let mut changed = bytes;
        changed[13] ^= 1;
        assert!(TakeJournal::from_bytes(&changed).is_err());
    }

    #[test]
    fn journal_rejects_oversized_part_lengths_before_copying_payloads() {
        let mut bytes = openbmt_core::wire::start(&super::MAGIC, 8);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        assert!(TakeJournal::from_bytes(&bytes).is_err());
    }
}
