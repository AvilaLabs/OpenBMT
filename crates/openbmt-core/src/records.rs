//! Signed, hash-chained records.
//!
//! Every sale, payment, refund, override, setting change, pairing and
//! check-in is a record signed by the device that wrote it. Each record names
//! the hash of the one before, so deleting, editing or reordering records
//! breaks the chain. The owner phone keeps the latest check-in; because a
//! check-in carries the author's head, cutting records off the end of a chain
//! shows up when the chain is compared with a later check-in.
//!
//! Canonical bytes of a record (what the author signs under
//! `OpenBMT/record/v1`):
//!
//! ```text
//! seq u64 LE | prev (32) | author (32) | kind u8 | body_len u32 LE | body
//! ```
//!
//! The record's hash (the link the next record carries) covers those bytes
//! and the signature, under the tag `OpenBMT/record-id/v1`.

use std::collections::HashSet;
use std::fmt;

use crate::identity::{tagged_hash, DeviceId, Identity, Sig, TAG_RECORD};

/// Tag for the hash that links records together.
pub const TAG_RECORD_ID: &str = "OpenBMT/record-id/v1";
/// The `prev` value of the first record.
pub const GENESIS: [u8; 32] = [0; 32];
/// Largest record body accepted.
pub const MAX_BODY: usize = 1 << 20;

/// What a record is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordKind {
    /// A sale was opened or closed.
    Sale = 1,
    /// A payment was seen, accepted or flagged.
    Payment = 2,
    /// A refund or cancel.
    Refund = 3,
    /// A red-screen override by a cashier or manager.
    Override = 4,
    /// A settings change was applied.
    Setting = 5,
    /// A device was paired or removed.
    Pairing = 6,
    /// A signed head report.
    CheckIn = 7,
    /// A conflicting spend was seen (catch and proof).
    Conflict = 8,
}

impl RecordKind {
    /// Read a kind byte.
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::Sale,
            2 => Self::Payment,
            3 => Self::Refund,
            4 => Self::Override,
            5 => Self::Setting,
            6 => Self::Pairing,
            7 => Self::CheckIn,
            8 => Self::Conflict,
            _ => return None,
        })
    }
}

/// Why a chain or record was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainError {
    /// The record's author is not on the device list.
    UnknownAuthor { seq: u64 },
    /// The signature does not match the record.
    BadSignature { seq: u64 },
    /// The sequence number is not the next one: a record was deleted, added
    /// or moved.
    SeqMismatch { expected: u64, found: u64 },
    /// `prev` is not the hash of the record before.
    PrevMismatch { seq: u64 },
    /// A check-in's body does not match the chain it sits in.
    BadCheckIn { seq: u64 },
    /// The chain is shorter than the check-in it is compared with: records
    /// were cut off the end.
    Truncated { have: u64, need: u64 },
    /// The chain has a different record where the check-in says there is
    /// one: the history was rewritten.
    Diverged { seq: u64 },
    /// The body is larger than [`MAX_BODY`].
    BodyTooLarge,
    /// The record passed to a check is not a check-in.
    NotACheckIn,
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAuthor { seq } => write!(f, "record {seq} is from an unknown device"),
            Self::BadSignature { seq } => write!(f, "record {seq} has a bad signature"),
            Self::SeqMismatch { expected, found } => {
                write!(
                    f,
                    "expected record {expected}, found {found}: a record is missing or moved"
                )
            }
            Self::PrevMismatch { seq } => write!(f, "record {seq} does not follow the one before"),
            Self::BadCheckIn { seq } => write!(f, "check-in {seq} does not match its chain"),
            Self::Truncated { have, need } => {
                write!(f, "chain has {have} records but a check-in needs {need}")
            }
            Self::Diverged { seq } => write!(f, "record {seq} differs from the check-in"),
            Self::BodyTooLarge => write!(f, "record body too large"),
            Self::NotACheckIn => write!(f, "not a check-in record"),
        }
    }
}

impl std::error::Error for ChainError {}

/// The devices whose records are accepted.
///
/// Verification must be given every device that was on the signed device list
/// at any time in the period being checked. Removing a device is enforced
/// when new records are taken in, not by rejecting its old records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceList(HashSet<DeviceId>);

impl DeviceList {
    /// A list of the given devices.
    pub fn new(devices: impl IntoIterator<Item = DeviceId>) -> Self {
        Self(devices.into_iter().collect())
    }

    /// Whether `id` is on the list.
    pub fn contains(&self, id: &DeviceId) -> bool {
        self.0.contains(id)
    }
}

/// One signed link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Position in the chain, from 0.
    pub seq: u64,
    /// Hash of the previous record ([`GENESIS`] for the first).
    pub prev: [u8; 32],
    /// The device that wrote and signed it.
    pub author: DeviceId,
    /// What it is about.
    pub kind: RecordKind,
    /// Kind-specific content.
    pub body: Vec<u8>,
    /// Author's signature over [`Record::signing_bytes`].
    pub sig: Sig,
}

impl Record {
    /// The canonical bytes that are signed.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(77 + self.body.len());
        out.extend_from_slice(&self.seq.to_le_bytes());
        out.extend_from_slice(&self.prev);
        out.extend_from_slice(&self.author.to_bytes());
        out.push(self.kind as u8);
        out.extend_from_slice(&(self.body.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.body);
        out
    }

    /// The record hash: what the next record's `prev` must equal.
    pub fn hash(&self) -> [u8; 32] {
        let mut bytes = self.signing_bytes();
        bytes.extend_from_slice(&self.sig.0);
        tagged_hash(TAG_RECORD_ID, &bytes)
    }

    fn signature_ok(&self) -> bool {
        self.author
            .verify_tagged(TAG_RECORD, &self.signing_bytes(), &self.sig)
    }

    /// For a check-in: `(records before it, head hash)` as the author
    /// reported them.
    pub fn check_in_claim(&self) -> Result<(u64, [u8; 32]), ChainError> {
        if self.kind != RecordKind::CheckIn || self.body.len() != 40 {
            return Err(ChainError::BadCheckIn { seq: self.seq });
        }
        let len = u64::from_le_bytes(self.body[..8].try_into().expect("8 bytes"));
        let head: [u8; 32] = self.body[8..].try_into().expect("32 bytes");
        Ok((len, head))
    }
}

/// An append-only chain of records.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Chain {
    records: Vec<Record>,
}

impl Chain {
    /// An empty chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Wrap records received from elsewhere. Nothing is checked: call
    /// [`Chain::verify`].
    pub fn from_records(records: Vec<Record>) -> Self {
        Self { records }
    }

    /// The records in order.
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Number of records.
    pub fn len(&self) -> u64 {
        self.records.len() as u64
    }

    /// Whether the chain has no records.
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Hash of the last record, or [`GENESIS`] if empty.
    pub fn head(&self) -> [u8; 32] {
        self.records.last().map_or(GENESIS, Record::hash)
    }

    /// Write, sign and append a record.
    pub fn append(
        &mut self,
        author: &Identity,
        kind: RecordKind,
        body: Vec<u8>,
    ) -> Result<&Record, ChainError> {
        if body.len() > MAX_BODY {
            return Err(ChainError::BodyTooLarge);
        }
        let mut rec = Record {
            seq: self.len(),
            prev: self.head(),
            author: author.device_id(),
            kind,
            body,
            sig: Sig([0; 64]),
        };
        rec.sig = author.sign_tagged(TAG_RECORD, &rec.signing_bytes());
        self.records.push(rec);
        Ok(self.records.last().expect("just pushed"))
    }

    /// Append a check-in that carries the current head: its body is the
    /// number of records before it and their head hash.
    pub fn append_check_in(&mut self, author: &Identity) -> &Record {
        let mut body = Vec::with_capacity(40);
        body.extend_from_slice(&self.len().to_le_bytes());
        body.extend_from_slice(&self.head());
        self.append(author, RecordKind::CheckIn, body)
            .expect("40-byte body")
    }

    /// Check the whole chain from the first record: known authors, bodies
    /// within [`MAX_BODY`], good signatures, consecutive numbers, each `prev`
    /// the hash of the record before, and every check-in matching the chain
    /// it sits in. This detects edited, deleted, inserted and reordered
    /// records. It cannot see records cut off the end: compare with a later
    /// check-in for that ([`Chain::check_against_check_in`]).
    pub fn verify(&self, devices: &DeviceList) -> Result<(), ChainError> {
        let mut prev = GENESIS;
        for (i, r) in self.records.iter().enumerate() {
            if !devices.contains(&r.author) {
                return Err(ChainError::UnknownAuthor { seq: r.seq });
            }
            if r.body.len() > MAX_BODY {
                return Err(ChainError::BodyTooLarge);
            }
            if !r.signature_ok() {
                return Err(ChainError::BadSignature { seq: r.seq });
            }
            if r.seq != i as u64 {
                return Err(ChainError::SeqMismatch {
                    expected: i as u64,
                    found: r.seq,
                });
            }
            if r.prev != prev {
                return Err(ChainError::PrevMismatch { seq: r.seq });
            }
            if r.kind == RecordKind::CheckIn {
                let (len, head) = r.check_in_claim()?;
                if len != r.seq || head != r.prev {
                    return Err(ChainError::BadCheckIn { seq: r.seq });
                }
            }
            prev = r.hash();
        }
        Ok(())
    }

    /// Compare this chain with a check-in held elsewhere (the owner phone's
    /// copy). The check-in must be genuine, and this chain must contain it
    /// at its place. A shorter chain means the tail was cut off; a different
    /// record there means history was rewritten.
    pub fn check_against_check_in(
        &self,
        check_in: &Record,
        devices: &DeviceList,
    ) -> Result<(), ChainError> {
        if check_in.kind != RecordKind::CheckIn {
            return Err(ChainError::NotACheckIn);
        }
        if !devices.contains(&check_in.author) {
            return Err(ChainError::UnknownAuthor { seq: check_in.seq });
        }
        if !check_in.signature_ok() {
            return Err(ChainError::BadSignature { seq: check_in.seq });
        }
        check_in.check_in_claim()?;
        let need = check_in.seq + 1;
        if self.len() < need {
            return Err(ChainError::Truncated {
                have: self.len(),
                need,
            });
        }
        if self.records[check_in.seq as usize].hash() != check_in.hash() {
            return Err(ChainError::Diverged { seq: check_in.seq });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::identity;

    fn devices() -> DeviceList {
        DeviceList::new([identity(1).device_id(), identity(2).device_id()])
    }

    /// 6 records from two authors, with a check-in in the middle.
    fn sample() -> Chain {
        let (a, b) = (identity(1), identity(2));
        let mut c = Chain::new();
        c.append(&a, RecordKind::Sale, b"sale 1".to_vec()).unwrap();
        c.append(&a, RecordKind::Payment, b"pay 1".to_vec())
            .unwrap();
        c.append_check_in(&a);
        c.append(&b, RecordKind::Refund, b"refund".to_vec())
            .unwrap();
        c.append(&a, RecordKind::Override, b"override".to_vec())
            .unwrap();
        c.append(&b, RecordKind::Setting, b"setting".to_vec())
            .unwrap();
        c
    }

    fn resign(r: &mut Record, who: u8) {
        r.sig = identity(who).sign_tagged(TAG_RECORD, &r.signing_bytes());
    }

    #[test]
    fn empty_and_built_chains_verify() {
        assert_eq!(Chain::new().verify(&devices()), Ok(()));
        assert_eq!(Chain::new().head(), GENESIS);
        let c = sample();
        assert_eq!(c.len(), 6);
        assert_eq!(c.verify(&devices()), Ok(()));
        assert_eq!(c.records()[0].prev, GENESIS);
        for w in c.records().windows(2) {
            assert_eq!(w[1].prev, w[0].hash());
        }
        assert_eq!(c.head(), c.records()[5].hash());
    }

    #[test]
    fn head_changes_with_every_append() {
        let a = identity(1);
        let mut c = Chain::new();
        let mut heads = vec![c.head()];
        for i in 0..4 {
            c.append(&a, RecordKind::Sale, vec![i]).unwrap();
            assert!(!heads.contains(&c.head()));
            heads.push(c.head());
        }
    }

    #[test]
    fn kind_bytes_round_trip() {
        for k in [
            RecordKind::Sale,
            RecordKind::Payment,
            RecordKind::Refund,
            RecordKind::Override,
            RecordKind::Setting,
            RecordKind::Pairing,
            RecordKind::CheckIn,
            RecordKind::Conflict,
        ] {
            assert_eq!(RecordKind::from_u8(k as u8), Some(k));
        }
        assert_eq!(RecordKind::from_u8(0), None);
        assert_eq!(RecordKind::from_u8(99), None);
    }

    #[test]
    fn oversized_body_is_refused() {
        let mut c = Chain::new();
        assert_eq!(
            c.append(&identity(1), RecordKind::Sale, vec![0; MAX_BODY + 1])
                .unwrap_err(),
            ChainError::BodyTooLarge
        );
        assert!(c.is_empty());
    }

    #[test]
    fn verify_refuses_an_oversized_body_before_the_signature() {
        let mut recs = sample().records().to_vec();
        recs[1].body = vec![0; MAX_BODY + 1];
        // The signature no longer matches either, but the size is reported.
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::BodyTooLarge)
        );
        let mut recs = sample().records().to_vec();
        recs[1].body = vec![0; MAX_BODY];
        resign(&mut recs[1], 1);
        assert_ne!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::BodyTooLarge)
        );
    }

    #[test]
    fn edited_body_breaks_the_signature() {
        let mut recs = sample().records().to_vec();
        recs[1].body = b"pay 2".to_vec();
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::BadSignature { seq: 1 })
        );
    }

    #[test]
    fn edited_and_resigned_record_breaks_the_next_link() {
        let mut recs = sample().records().to_vec();
        recs[3].body = b"refund of 0".to_vec();
        resign(&mut recs[3], 2);
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::PrevMismatch { seq: 4 })
        );
    }

    #[test]
    fn deleted_record_is_a_sequence_gap() {
        let mut recs = sample().records().to_vec();
        recs.remove(1);
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::SeqMismatch {
                expected: 1,
                found: 2
            })
        );
    }

    #[test]
    fn deleted_and_renumbered_records_break_prev() {
        let mut recs = sample().records().to_vec();
        recs.remove(1);
        for (i, r) in recs.iter_mut().enumerate() {
            r.seq = i as u64;
            resign(r, 1); // attacker holds device 1's key; device 2's records fail
        }
        let err = Chain::from_records(recs).verify(&devices()).unwrap_err();
        assert!(matches!(err, ChainError::PrevMismatch { .. }), "{err:?}");
    }

    #[test]
    fn reordered_records_are_detected() {
        let mut recs = sample().records().to_vec();
        recs.swap(3, 4);
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::SeqMismatch {
                expected: 3,
                found: 4
            })
        );
    }

    #[test]
    fn inserted_record_is_detected() {
        let c = sample();
        let mut other = Chain::from_records(c.records()[..3].to_vec());
        other
            .append(&identity(1), RecordKind::Sale, b"extra".to_vec())
            .unwrap();
        let mut recs = c.records().to_vec();
        recs.insert(3, other.records()[3].clone());
        assert!(Chain::from_records(recs).verify(&devices()).is_err());
    }

    #[test]
    fn unknown_author_is_refused() {
        let mut c = sample();
        c.append(&identity(9), RecordKind::Sale, b"x".to_vec())
            .unwrap();
        assert_eq!(
            c.verify(&devices()),
            Err(ChainError::UnknownAuthor { seq: 6 })
        );
        // removing a device from the list makes its history fail too
        let only_a = DeviceList::new([identity(1).device_id()]);
        assert_eq!(
            sample().verify(&only_a),
            Err(ChainError::UnknownAuthor { seq: 3 })
        );
    }

    #[test]
    fn forged_author_or_signature_is_refused() {
        let mut recs = sample().records().to_vec();
        recs[0].author = identity(2).device_id(); // claims another known device wrote it
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::BadSignature { seq: 0 })
        );
        let mut recs = sample().records().to_vec();
        recs[4].sig.0[0] ^= 1;
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::BadSignature { seq: 4 })
        );
    }

    #[test]
    fn changed_kind_is_refused() {
        let mut recs = sample().records().to_vec();
        recs[0].kind = RecordKind::Refund;
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::BadSignature { seq: 0 })
        );
    }

    #[test]
    fn nonzero_genesis_prev_is_refused() {
        let mut recs = sample().records().to_vec();
        recs[0].prev = [1; 32];
        resign(&mut recs[0], 1);
        assert_eq!(
            Chain::from_records(recs).verify(&devices()),
            Err(ChainError::PrevMismatch { seq: 0 })
        );
    }

    #[test]
    fn check_in_carries_the_head() {
        let c = sample();
        let ci = &c.records()[2];
        assert_eq!(ci.kind, RecordKind::CheckIn);
        let (len, head) = ci.check_in_claim().unwrap();
        assert_eq!(len, 2);
        assert_eq!(head, c.records()[1].hash());
    }

    #[test]
    fn check_in_with_a_false_claim_is_refused() {
        let a = identity(1);
        let mut c = Chain::new();
        c.append(&a, RecordKind::Sale, vec![1]).unwrap();
        let mut body = Vec::new();
        body.extend_from_slice(&5u64.to_le_bytes()); // lies about how many records
        body.extend_from_slice(&c.head());
        c.append(&a, RecordKind::CheckIn, body).unwrap();
        assert_eq!(c.verify(&devices()), Err(ChainError::BadCheckIn { seq: 1 }));
        // wrong length body
        let mut c = Chain::new();
        c.append(&a, RecordKind::CheckIn, vec![1, 2, 3]).unwrap();
        assert_eq!(c.verify(&devices()), Err(ChainError::BadCheckIn { seq: 0 }));
    }

    #[test]
    fn tail_truncation_is_caught_by_a_later_check_in() {
        let a = identity(1);
        let mut c = sample();
        c.append(&a, RecordKind::Sale, b"sale 2".to_vec()).unwrap();
        let ci = c.append_check_in(&a).clone(); // seq 7, held by the owner phone
        c.append(&a, RecordKind::Payment, b"late".to_vec()).unwrap();
        assert_eq!(c.check_against_check_in(&ci, &devices()), Ok(()));
        // the cut chain still verifies on its own...
        let cut = Chain::from_records(c.records()[..5].to_vec());
        assert_eq!(cut.verify(&devices()), Ok(()));
        // ...but not against the check-in
        assert_eq!(
            cut.check_against_check_in(&ci, &devices()),
            Err(ChainError::Truncated { have: 5, need: 8 })
        );
        // cutting only the check-in record itself is caught too
        let cut = Chain::from_records(c.records()[..7].to_vec());
        assert_eq!(
            cut.check_against_check_in(&ci, &devices()),
            Err(ChainError::Truncated { have: 7, need: 8 })
        );
    }

    #[test]
    fn rewritten_history_is_caught_by_a_check_in() {
        let a = identity(1);
        let mut c = sample();
        let ci = c.append_check_in(&a).clone();
        // Attacker rebuilds an alternative chain of the same length.
        let mut forked = Chain::from_records(c.records()[..2].to_vec());
        forked
            .append(&a, RecordKind::Sale, b"different".to_vec())
            .unwrap();
        while forked.len() < c.len() {
            forked
                .append(&a, RecordKind::Sale, b"filler".to_vec())
                .unwrap();
        }
        assert_eq!(forked.verify(&devices()), Ok(()));
        assert_eq!(
            forked.check_against_check_in(&ci, &devices()),
            Err(ChainError::Diverged { seq: ci.seq })
        );
    }

    #[test]
    fn check_against_check_in_rejects_bad_check_ins() {
        let a = identity(1);
        let c = sample();
        assert_eq!(
            c.check_against_check_in(&c.records()[0], &devices()),
            Err(ChainError::NotACheckIn)
        );
        let mut ci = c.records()[2].clone();
        ci.sig.0[3] ^= 1;
        assert_eq!(
            c.check_against_check_in(&ci, &devices()),
            Err(ChainError::BadSignature { seq: 2 })
        );
        let ci = c.records()[2].clone();
        assert_eq!(
            c.check_against_check_in(&ci, &DeviceList::new([identity(2).device_id()])),
            Err(ChainError::UnknownAuthor { seq: 2 })
        );
        // a signed check-in with a malformed body
        let mut bad = Chain::new();
        let r = bad
            .append(&a, RecordKind::CheckIn, vec![0; 3])
            .unwrap()
            .clone();
        assert_eq!(
            bad.check_against_check_in(&r, &devices()),
            Err(ChainError::BadCheckIn { seq: 0 })
        );
    }

    #[test]
    fn edit_after_the_latest_check_in_waits_for_the_next_one() {
        let a = identity(1);
        let mut c = sample();
        let ci = c.append_check_in(&a).clone();
        c.append(&a, RecordKind::Payment, b"real".to_vec()).unwrap();
        let mut recs = c.records().to_vec();
        let last = recs.len() - 1;
        recs[last].body = b"edited".to_vec();
        resign(&mut recs[last], 1);
        let edited = Chain::from_records(recs);
        // Nothing after it links to it, so verify alone passes.
        assert_eq!(edited.verify(&devices()), Ok(()));
        // The check-in covers only what came before it, so this edit stays
        // invisible until the next check-in. That window is the check-in
        // interval (15 minutes by default).
        assert_eq!(edited.check_against_check_in(&ci, &devices()), Ok(()));
    }
}
