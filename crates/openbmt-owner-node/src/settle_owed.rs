//! The settlements a block owes the owner's queue (`docs/OWNER-COINS-RULES.md`
//! OC-52, on AS-89).
//!
//! A block that pays or beats a transaction given to a hardware signer is
//! written down here before the coin view takes the block, and each item is
//! removed only after its change to the queue is stored. The file is written
//! whole (the watcher's atomic write) and read back strictly: a file that
//! does not read, that is not in order or that does not re-encode to the same
//! bytes is refused whole.
//!
//! Layout: the magic `OBM,`, the version byte 1, the number of items (u32
//! LE), and for each item its kind (one byte), the transaction's ID (32
//! bytes) and the block's hash (32 bytes). Items are in the order of kind,
//! then of ID, with no two of the same kind and ID.

use std::fmt;

use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, Txid};
use openbmt_core::wire::{self, DecodeError, Reader};

/// The file under the node's directory.
pub const SETTLE_OWED_FILE: &str = "settle-owed";

/// Most items the file holds at once.
pub const MAX_OWED: usize = 1024;

/// The magic of the file.
pub const MAGIC_SETTLE_OWED: [u8; 4] = *b"OBM,";

/// The size of one item in the file.
const OWED_LEN: usize = 1 + 32 + 32;

/// What a block did to a transaction that was given to a hardware signer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OwedKind {
    /// The transaction of an open approval is in the block.
    ExportedPaid,
    /// The transaction of an abandoned approval, kept as signed elsewhere, is
    /// in the block.
    ElsewherePaid,
    /// A coin of a transaction kept as signed elsewhere is spent in the block
    /// by another transaction.
    ElsewhereBeaten,
}

impl OwedKind {
    fn byte(self) -> u8 {
        match self {
            Self::ExportedPaid => 0,
            Self::ElsewherePaid => 1,
            Self::ElsewhereBeaten => 2,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::ExportedPaid),
            1 => Some(Self::ElsewherePaid),
            2 => Some(Self::ElsewhereBeaten),
            _ => None,
        }
    }
}

/// One settlement a block owes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Owed {
    /// What the block did.
    pub kind: OwedKind,
    /// The transaction that was given to the signer.
    pub txid: Txid,
    /// The block that paid or beat it.
    pub block: BlockHash,
}

impl Owed {
    fn key(&self) -> (OwedKind, [u8; 32]) {
        (self.kind, self.txid.to_byte_array())
    }
}

/// Why the file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwedError {
    /// The file is not in the form it is written in.
    Corrupt(&'static str),
    /// More items than the file may hold.
    TooMany,
}

impl fmt::Display for OwedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Corrupt(why) => write!(f, "OC-52: the file of owed settlements is bad: {why}"),
            Self::TooMany => write!(f, "OC-52: more than {MAX_OWED} settlements are owed"),
        }
    }
}

impl std::error::Error for OwedError {}

fn corrupt(e: DecodeError) -> OwedError {
    match e {
        DecodeError::NonCanonical(why) => OwedError::Corrupt(why),
        DecodeError::TrailingBytes => OwedError::Corrupt("bytes follow the last item"),
        DecodeError::BadMagic => OwedError::Corrupt("it does not start with the magic"),
        DecodeError::UnsupportedVersion(_) => OwedError::Corrupt("its version is not read"),
        DecodeError::TooLarge => OwedError::TooMany,
        _ => OwedError::Corrupt("it is cut short"),
    }
}

/// The settlements owed, in memory and (by the node) in their file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettleOwed {
    items: Vec<Owed>,
}

impl SettleOwed {
    /// Nothing owed.
    pub fn new() -> Self {
        Self::default()
    }

    /// The items, in the order of the file.
    pub fn items(&self) -> &[Owed] {
        &self.items
    }

    /// Whether nothing is owed.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The list with `add` merged in: an item of the same kind and
    /// transaction takes the newer block. Built only; nothing is stored.
    pub fn merged(&self, add: &[Owed]) -> Result<Self, OwedError> {
        let mut items = self.items.clone();
        for item in add {
            match items.iter_mut().find(|o| o.key() == item.key()) {
                Some(held) => *held = *item,
                None => items.push(*item),
            }
        }
        items.sort_by_key(Owed::key);
        if items.len() > MAX_OWED {
            return Err(OwedError::TooMany);
        }
        Ok(Self { items })
    }

    /// The list without the item of this kind and transaction.
    pub fn without(&self, item: &Owed) -> Self {
        Self {
            items: self
                .items
                .iter()
                .filter(|o| o.key() != item.key())
                .copied()
                .collect(),
        }
    }

    /// The list without the items that wait on one of these blocks.
    pub fn without_blocks(&self, blocks: &[BlockHash]) -> Self {
        Self {
            items: self
                .items
                .iter()
                .filter(|o| !blocks.contains(&o.block))
                .copied()
                .collect(),
        }
    }

    /// The file for the list as it is.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_SETTLE_OWED, 4 + self.items.len() * OWED_LEN);
        out.extend_from_slice(&wire::len_u32(self.items.len()).to_le_bytes());
        for item in &self.items {
            out.push(item.kind.byte());
            out.extend_from_slice(&item.txid.to_byte_array());
            out.extend_from_slice(&item.block.to_byte_array());
        }
        out
    }

    /// OC-52: read a file back, strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, OwedError> {
        let mut r = Reader::framed(bytes, &MAGIC_SETTLE_OWED).map_err(corrupt)?;
        let n = usize::try_from(r.u32_le().map_err(corrupt)?)
            .map_err(|_| OwedError::Corrupt("too many items"))?;
        if n > MAX_OWED {
            return Err(OwedError::TooMany);
        }
        let mut items: Vec<Owed> = Vec::with_capacity(n);
        for _ in 0..n {
            let kind = OwedKind::from_byte(r.u8().map_err(corrupt)?)
                .ok_or(OwedError::Corrupt("a kind is not known"))?;
            let txid = Txid::from_byte_array(r.array::<32>().map_err(corrupt)?);
            let block = BlockHash::from_byte_array(r.array::<32>().map_err(corrupt)?);
            let item = Owed { kind, txid, block };
            if let Some(last) = items.last() {
                if last.key() >= item.key() {
                    return Err(OwedError::Corrupt(
                        "the items are not in order, or one is there twice",
                    ));
                }
            }
            items.push(item);
        }
        r.finish().map_err(corrupt)?;
        let read = Self { items };
        if read.to_bytes() != bytes {
            return Err(OwedError::Corrupt("it does not encode to the same bytes"));
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: OwedKind, n: u8, block: u8) -> Owed {
        Owed {
            kind,
            txid: Txid::from_byte_array([n; 32]),
            block: BlockHash::from_byte_array([block; 32]),
        }
    }

    fn sample() -> SettleOwed {
        SettleOwed::new()
            .merged(&[
                item(OwedKind::ElsewhereBeaten, 1, 9),
                item(OwedKind::ExportedPaid, 5, 8),
                item(OwedKind::ExportedPaid, 2, 8),
                item(OwedKind::ElsewherePaid, 3, 7),
            ])
            .unwrap()
    }

    #[test]
    fn oc_52_the_file_round_trips_in_the_order_of_kind_then_id() {
        let owed = sample();
        let kinds: Vec<_> = owed
            .items()
            .iter()
            .map(|o| (o.kind, o.txid.to_byte_array()[0]))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (OwedKind::ExportedPaid, 2),
                (OwedKind::ExportedPaid, 5),
                (OwedKind::ElsewherePaid, 3),
                (OwedKind::ElsewhereBeaten, 1),
            ]
        );
        let bytes = owed.to_bytes();
        assert_eq!(&bytes[..5], b"OBM,\x01");
        assert_eq!(bytes.len(), 9 + 4 * OWED_LEN);
        assert_eq!(SettleOwed::from_bytes(&bytes).unwrap(), owed);
        let empty = SettleOwed::new().to_bytes();
        assert_eq!(SettleOwed::from_bytes(&empty).unwrap(), SettleOwed::new());
    }

    #[test]
    fn oc_52_a_wrong_magic_version_length_or_trailing_byte_is_refused() {
        let bytes = sample().to_bytes();
        let mut magic = bytes.clone();
        magic[3] = b'-';
        assert!(SettleOwed::from_bytes(&magic).is_err());
        let mut version = bytes.clone();
        version[4] = 2;
        assert!(SettleOwed::from_bytes(&version).is_err());
        assert!(SettleOwed::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        assert!(SettleOwed::from_bytes(&bytes[..3]).is_err());
        assert!(SettleOwed::from_bytes(&[]).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(SettleOwed::from_bytes(&trailing).is_err());
        // A count that says more than the bytes hold.
        let mut count = bytes;
        count[5] = 5;
        assert!(SettleOwed::from_bytes(&count).is_err());
    }

    #[test]
    fn oc_52_an_unknown_kind_a_repeat_or_a_disordered_list_is_refused() {
        let bytes = sample().to_bytes();
        let mut kind = bytes.clone();
        kind[9] = 3;
        assert!(SettleOwed::from_bytes(&kind).is_err());
        // The first two items swapped.
        let mut swapped = bytes.clone();
        let (a, b) = (9..9 + OWED_LEN, 9 + OWED_LEN..9 + 2 * OWED_LEN);
        let first = swapped[a.clone()].to_vec();
        let second = swapped[b.clone()].to_vec();
        swapped[a].copy_from_slice(&second);
        swapped[b].copy_from_slice(&first);
        assert!(SettleOwed::from_bytes(&swapped).is_err());
        // The first item twice.
        let mut twice = bytes;
        let first = twice[9..9 + OWED_LEN].to_vec();
        twice[9 + OWED_LEN..9 + 2 * OWED_LEN].copy_from_slice(&first);
        assert!(SettleOwed::from_bytes(&twice).is_err());
    }

    #[test]
    fn oc_52_more_items_than_the_bound_are_refused_before_they_are_read() {
        let mut bytes = wire::start(&MAGIC_SETTLE_OWED, 4);
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(SettleOwed::from_bytes(&bytes), Err(OwedError::TooMany));
        let mut bytes = wire::start(&MAGIC_SETTLE_OWED, 4);
        bytes.extend_from_slice(&(MAX_OWED as u32 + 1).to_le_bytes());
        assert_eq!(SettleOwed::from_bytes(&bytes), Err(OwedError::TooMany));
        let many: Vec<Owed> = (0..=MAX_OWED)
            .map(|i| Owed {
                kind: OwedKind::ExportedPaid,
                txid: Txid::from_byte_array({
                    let mut b = [0u8; 32];
                    b[..4].copy_from_slice(&(i as u32).to_le_bytes());
                    b
                }),
                block: BlockHash::from_byte_array([1; 32]),
            })
            .collect();
        assert_eq!(SettleOwed::new().merged(&many), Err(OwedError::TooMany));
        assert!(SettleOwed::new().merged(&many[..MAX_OWED]).is_ok());
    }

    #[test]
    fn oc_52_merging_takes_the_newer_block_and_removing_takes_one_item_out() {
        let owed = sample();
        let newer = owed.merged(&[item(OwedKind::ExportedPaid, 2, 66)]).unwrap();
        assert_eq!(newer.items().len(), 4);
        assert_eq!(newer.items()[0].block, BlockHash::from_byte_array([66; 32]));
        let less = newer.without(&item(OwedKind::ExportedPaid, 2, 0));
        assert_eq!(less.items().len(), 3);
        let by_block = owed.without_blocks(&[BlockHash::from_byte_array([8; 32])]);
        assert_eq!(by_block.items().len(), 2);
    }
}
