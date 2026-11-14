//! The marker of a request book that has not yet taken back a reorg (RQ-37,
//! OC-53): the fork height the coin view was rolled back to, written before
//! the view's rollback is stored and deleted once the book has been stored
//! for it. A start that finds it runs the book's rollback again.

use std::fs;
use std::io;
use std::path::Path;

use bitcoin::hashes::{sha256, Hash};
use openbmt_core::wire::{self, Reader};

use crate::watcher::write_file;

/// The marker's file in the node's folder.
pub(crate) const REQUESTS_UNDO_FILE: &str = "requests-undo";

/// Magic of the marker: the free frame magic `OBM` and byte 0x1a.
const MAGIC: [u8; 4] = *b"OBM\x1a";

fn to_bytes(fork: u32) -> Vec<u8> {
    let mut out = wire::start(&MAGIC, 4);
    out.extend_from_slice(&fork.to_le_bytes());
    let checksum = sha256::Hash::hash(&out);
    out.extend_from_slice(&checksum.to_byte_array());
    out
}

fn from_bytes(bytes: &[u8]) -> Result<u32, String> {
    let prefix = bytes
        .len()
        .checked_sub(32)
        .and_then(|n| bytes.get(..n))
        .ok_or_else(|| "the marker is truncated".to_string())?;
    let mut r = Reader::framed(bytes, &MAGIC).map_err(|e| e.to_string())?;
    let fork = r.u32_le().map_err(|e| e.to_string())?;
    let checksum = r.array::<32>().map_err(|e| e.to_string())?;
    r.finish().map_err(|e| e.to_string())?;
    if sha256::Hash::hash(prefix).to_byte_array() != checksum {
        return Err("the marker's checksum does not match".to_string());
    }
    Ok(fork)
}

/// The fork height of the marker; `None` if there is none.
pub(crate) fn read(dir: &Path) -> Result<Option<u32>, String> {
    match fs::read(dir.join(REQUESTS_UNDO_FILE)) {
        Ok(bytes) if bytes.len() <= 64 => from_bytes(&bytes)
            .map(Some)
            .map_err(|e| format!("{REQUESTS_UNDO_FILE} does not read back: {e}")),
        Ok(_) => Err(format!("{REQUESTS_UNDO_FILE}: file is too large")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{REQUESTS_UNDO_FILE}: {e}")),
    }
}

/// Store the marker for `fork`; one already there keeps the lower height.
/// Returns the height stored.
pub(crate) fn mark(dir: &Path, fork: u32) -> Result<u32, String> {
    let fork = read(dir)?.map_or(fork, |held| held.min(fork));
    write_file(dir, REQUESTS_UNDO_FILE, &to_bytes(fork))?;
    Ok(fork)
}

/// Delete the marker; none there is done.
pub(crate) fn clear(dir: &Path) -> Result<(), String> {
    match fs::remove_file(dir.join(REQUESTS_UNDO_FILE)) {
        Ok(()) => fs::File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| format!("{REQUESTS_UNDO_FILE}: {e}")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{REQUESTS_UNDO_FILE}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_round_trips_and_rejects_truncation_trailing_bytes_and_bit_flips() {
        let bytes = to_bytes(1234);
        assert_eq!(from_bytes(&bytes), Ok(1234));
        assert!(from_bytes(&bytes[..bytes.len() - 1]).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(from_bytes(&trailing).is_err());
        let mut changed = bytes;
        changed[6] ^= 1;
        assert!(from_bytes(&changed).is_err());
    }
}
