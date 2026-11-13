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
//!
//! Version 2 (OC-54) is written when an item that a block paid carries the
//! transaction taken from that block, which the settlement keeps in the coin
//! view's undo record: after the block's hash, an item of a paid kind has the
//! transaction's length (u32 LE, 0 for none) and its bytes. A file in which
//! no item carries one is written as version 1, as before.

use std::fmt;

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, Transaction, Txid};
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::coins::MAX_TX_BYTES;

/// The file under the node's directory.
pub const SETTLE_OWED_FILE: &str = "settle-owed";

/// Most items the file holds at once.
pub const MAX_OWED: usize = 1024;

/// The magic of the file.
pub const MAGIC_SETTLE_OWED: [u8; 4] = *b"OBM,";

/// Most bytes of transactions the file holds at once (OC-54).
pub const MAX_OWED_TX_BYTES: usize = 8 * 1024 * 1024;

/// The size of one item in the file, without a transaction.
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
    /// AS-105: a coin of a transaction whose file was given to a shop's own
    /// wallet, in a session that is still open, is spent in the block by
    /// another transaction.
    ExportedBeaten,
}

impl OwedKind {
    fn byte(self) -> u8 {
        match self {
            Self::ExportedPaid => 0,
            Self::ElsewherePaid => 1,
            Self::ElsewhereBeaten => 2,
            Self::ExportedBeaten => 3,
        }
    }

    /// Whether the block paid the transaction, so that it may carry it.
    fn paid(self) -> bool {
        matches!(self, Self::ExportedPaid | Self::ElsewherePaid)
    }

    fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::ExportedPaid),
            1 => Some(Self::ElsewherePaid),
            2 => Some(Self::ElsewhereBeaten),
            3 => Some(Self::ExportedBeaten),
            _ => None,
        }
    }
}

/// One settlement a block owes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owed {
    /// What the block did.
    pub kind: OwedKind,
    /// The transaction that was given to the signer.
    pub txid: Txid,
    /// The block that paid or beat it.
    pub block: BlockHash,
    /// OC-54: for a paid kind, the transaction as that block holds it, with
    /// the ID `txid`. None for the other kinds, and for an item written
    /// before the file had it.
    pub tx: Option<Transaction>,
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

/// The bytes of the transactions the items carry.
fn tx_bytes(items: &[Owed]) -> usize {
    items
        .iter()
        .filter_map(|i| i.tx.as_ref())
        .map(|t| serialize(t).len())
        .sum()
}

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
                Some(held) => *held = item.clone(),
                None => items.push(item.clone()),
            }
        }
        items.sort_by_key(Owed::key);
        if items.len() > MAX_OWED || tx_bytes(&items) > MAX_OWED_TX_BYTES {
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
                .cloned()
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
                .cloned()
                .collect(),
        }
    }

    /// The file for the list as it is.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_SETTLE_OWED, 4 + self.items.len() * OWED_LEN);
        let with_tx = self.items.iter().any(|i| i.tx.is_some());
        if with_tx {
            if let Some(version) = out.get_mut(4) {
                *version = 2;
            }
        }
        out.extend_from_slice(&wire::len_u32(self.items.len()).to_le_bytes());
        for item in &self.items {
            out.push(item.kind.byte());
            out.extend_from_slice(&item.txid.to_byte_array());
            out.extend_from_slice(&item.block.to_byte_array());
            if with_tx && item.kind.paid() {
                let bytes = item.tx.as_ref().map(serialize).unwrap_or_default();
                out.extend_from_slice(&wire::len_u32(bytes.len()).to_le_bytes());
                out.extend_from_slice(&bytes);
            }
        }
        out
    }

    /// OC-52: read a file back, strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, OwedError> {
        let (mut r, version) =
            Reader::framed_up_to(bytes, &MAGIC_SETTLE_OWED, 2).map_err(corrupt)?;
        if bytes.len() > MAX_OWED_TX_BYTES + MAX_OWED * OWED_LEN + 16 {
            return Err(OwedError::TooMany);
        }
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
            let tx = if version >= 2 && kind.paid() {
                let len = usize::try_from(r.u32_le().map_err(corrupt)?)
                    .map_err(|_| OwedError::Corrupt("a transaction is too long"))?;
                if len > MAX_TX_BYTES {
                    return Err(OwedError::Corrupt("a transaction is too long"));
                }
                if len == 0 {
                    None
                } else {
                    let tx: Transaction = deserialize(r.take(len).map_err(corrupt)?)
                        .map_err(|_| OwedError::Corrupt("a transaction does not read"))?;
                    if tx.compute_txid() != txid {
                        return Err(OwedError::Corrupt("a transaction is not the one named"));
                    }
                    Some(tx)
                }
            } else {
                None
            };
            let item = Owed {
                kind,
                txid,
                block,
                tx,
            };
            if let Some(last) = items.last() {
                if last.key() >= item.key() {
                    return Err(OwedError::Corrupt(
                        "the items are not in order, or one is there twice",
                    ));
                }
            }
            items.push(item);
        }
        if tx_bytes(&items) > MAX_OWED_TX_BYTES {
            return Err(OwedError::TooMany);
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
            tx: None,
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
        // AS-105: the kind of an own-wallet item sorts after the others.
        let with_own = owed
            .merged(&[item(OwedKind::ExportedBeaten, 4, 6)])
            .unwrap();
        assert_eq!(
            with_own.items().last().map(|o| o.kind),
            Some(OwedKind::ExportedBeaten)
        );
        assert_eq!(
            SettleOwed::from_bytes(&with_own.to_bytes()).unwrap(),
            with_own
        );
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
        version[4] = 3;
        assert!(SettleOwed::from_bytes(&version).is_err());
        // Version 2 is the version with a transaction after a paid item.
        let mut two = bytes.clone();
        two[4] = 2;
        assert!(SettleOwed::from_bytes(&two).is_err());
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
        kind[9] = 4;
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
                tx: None,
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
    fn paid_with_tx(n: u8) -> Owed {
        use bitcoin::absolute::LockTime;
        use bitcoin::transaction::Version;
        use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, TxIn, TxOut, Witness};
        let tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::new(Txid::from_byte_array([n; 32]), 0),
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(1_000),
                script_pubkey: ScriptBuf::new(),
            }],
        };
        Owed {
            kind: OwedKind::ExportedPaid,
            txid: tx.compute_txid(),
            block: BlockHash::from_byte_array([6; 32]),
            tx: Some(tx),
        }
    }

    #[test]
    fn oc_54_a_paid_item_keeps_its_transaction_in_version_2_and_the_rest_stay_version_1() {
        let paid = paid_with_tx(1);
        let owed = sample().merged(std::slice::from_ref(&paid)).unwrap();
        let bytes = owed.to_bytes();
        assert_eq!(bytes[4], 2);
        let back = SettleOwed::from_bytes(&bytes).unwrap();
        assert_eq!(back, owed);
        assert!(back.items().iter().any(|o| o.tx == paid.tx));
        // Without a transaction in any item the file is version 1.
        assert_eq!(sample().to_bytes()[4], 1);
        // The transaction must be the one the item names, and is read
        // strictly.
        let other = Owed {
            txid: Txid::from_byte_array([3; 32]),
            ..paid.clone()
        };
        let wrong = SettleOwed::new().merged(&[other]).unwrap().to_bytes();
        assert!(SettleOwed::from_bytes(&wrong).is_err());
        for cut in 0..bytes.len() {
            assert!(SettleOwed::from_bytes(&bytes[..cut]).is_err(), "cut {cut}");
        }
        // A version 2 file in which no item has a transaction is not the
        // file that would be written.
        let mut bare = sample().to_bytes();
        bare[4] = 2;
        assert!(SettleOwed::from_bytes(&bare).is_err());
    }
}
