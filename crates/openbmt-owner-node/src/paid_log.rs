//! The log of what the owner paid out (`docs/OWNER-COINS-RULES.md` OC-48 to
//! OC-50).
//!
//! The owner's queue forgets a refund or a cash-out once it is paid and its
//! transaction is past the undo horizon (`Owner::archive_paid`). The books
//! need each of them with its time and its value in the shop's currency, so
//! the node keeps one entry for each in a file of its own, `paid-log.bin`:
//! the kind, the queue's number, the transaction, the satoshis that went
//! out, the time by the node's clock, the sale and request for a refund, and
//! the value. The file is only ever added to, and an entry is stored before
//! the queue is told the transaction was sent.
//!
//! The file is `OBM&`, version 1 (version 2 when it holds an entry that undoes
//! another, OC-50), and then entries of a fixed length, each
//! ending in a check of its own (the first 8 bytes of the SHA-256 of the
//! entry before it). It is read strictly: a length that is not a whole number
//! of entries, a check that does not match, a flag that is not 0 or 1, a field
//! that a kind does not use and is not zero, an entry that repeats an earlier
//! one that stands (a payment may be written again after its undoing, OC-55) and a currency that is not three capital letters all refuse the whole
//! file, and so does an undoing that names no earlier entry. Nothing is
//! repaired.

use bitcoin::hashes::{sha256, Hash};
use bitcoin::Txid;
use openbmt_core::identity::DeviceId;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_owner::SaleKey;

/// The file, in the node's directory.
pub const PAID_LOG_FILE: &str = "paid-log.bin";
/// The magic of the file.
pub const MAGIC_PAID_LOG: [u8; 4] = *b"OBM&";
/// Most entries the log holds. Past it nothing more is appended and the node
/// says so, rather than dropping the oldest.
pub const MAX_PAID_ENTRIES: usize = 100_000;
/// The bytes of one entry, with its check.
pub const PAID_ENTRY_LEN: usize = 118;
/// The longest file: the frame and every entry.
pub const MAX_PAID_LOG_BYTES: usize = 5 + MAX_PAID_ENTRIES * PAID_ENTRY_LEN;

const CHECK_LEN: usize = 8;
const SATS_PER_BTC: u128 = 100_000_000;

/// What was paid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaidWhat {
    /// A refund of a sale. `request` is the number of the counter's request
    /// it came from, when it came from one.
    Refund {
        /// The sale.
        sale: SaleKey,
        /// The request's number on the counter.
        request: Option<u64>,
    },
    /// A cash-out.
    CashOut,
    /// OC-50: the payment of the earlier entry of this kind, number and
    /// transaction did not happen. Its `sats` are 0 and it has no value.
    Undone(Undone),
}

/// What kind of entry an [`PaidWhat::Undone`] undoes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undone {
    /// A refund.
    Refund,
    /// A cash-out.
    CashOut,
}

/// What the payment was worth in the shop's currency at the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaidValue {
    /// Three upper-case ASCII letters (`SAT` for Bitcoin Standard).
    pub currency: [u8; 3],
    /// Minor units of the currency.
    pub minor: u64,
}

/// One entry of the log (OC-48).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaidEntry {
    /// A refund or a cash-out.
    pub what: PaidWhat,
    /// The number the entry had in the owner's queue.
    pub number: u32,
    /// The transaction that paid it.
    pub txid: Txid,
    /// The satoshis that went out to the destination.
    pub sats: u64,
    /// When, by the node's clock, in Unix seconds.
    pub at: u64,
    /// What it was worth, when that is known (OC-49).
    pub value: Option<PaidValue>,
}

impl PaidEntry {
    /// The kind, number and transaction of the payment this entry is, or
    /// undoes.
    fn payment_key(&self) -> (u8, u32, Txid) {
        (
            self.undoes_kind().unwrap_or(self.kind_byte()),
            self.number,
            self.txid,
        )
    }

    fn kind_byte(&self) -> u8 {
        match self.what {
            PaidWhat::Refund { .. } => 1,
            PaidWhat::CashOut => 2,
            PaidWhat::Undone(Undone::Refund) => 3,
            PaidWhat::Undone(Undone::CashOut) => 4,
        }
    }

    /// The kind byte of the entry an undoing names, or none for a payment.
    fn undoes_kind(&self) -> Option<u8> {
        match self.what {
            PaidWhat::Undone(Undone::Refund) => Some(1),
            PaidWhat::Undone(Undone::CashOut) => Some(2),
            _ => None,
        }
    }

    /// Whether this is an entry that undoes another.
    pub fn is_undone(&self) -> bool {
        self.undoes_kind().is_some()
    }

    /// OC-50: the entry that undoes this payment, at time `at`. None for an
    /// entry that is itself an undoing.
    pub fn undoing(&self, at: u64) -> Option<Self> {
        let kind = match self.what {
            PaidWhat::Refund { .. } => Undone::Refund,
            PaidWhat::CashOut => Undone::CashOut,
            PaidWhat::Undone(_) => return None,
        };
        Some(Self {
            what: PaidWhat::Undone(kind),
            number: self.number,
            txid: self.txid,
            sats: 0,
            at,
            value: None,
        })
    }

    fn body(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(PAID_ENTRY_LEN);
        out.push(self.kind_byte());
        out.extend_from_slice(&self.number.to_le_bytes());
        out.extend_from_slice(&self.txid.to_byte_array());
        out.extend_from_slice(&self.sats.to_le_bytes());
        out.extend_from_slice(&self.at.to_le_bytes());
        match self.what {
            PaidWhat::Refund { sale, request } => {
                out.extend_from_slice(&sale.counter.to_bytes());
                out.extend_from_slice(&sale.index.to_le_bytes());
                out.push(u8::from(request.is_some()));
                out.extend_from_slice(&request.unwrap_or(0).to_le_bytes());
            }
            PaidWhat::CashOut | PaidWhat::Undone(_) => {
                out.extend_from_slice(&[0; 32 + 4 + 1 + 8]);
            }
        }
        match self.value {
            Some(v) => {
                out.push(1);
                out.extend_from_slice(&v.currency);
                out.extend_from_slice(&v.minor.to_le_bytes());
            }
            None => out.extend_from_slice(&[0; 1 + 3 + 8]),
        }
        out
    }

    fn write(&self, out: &mut Vec<u8>) {
        let body = self.body();
        let check = sha256::Hash::hash(&body).to_byte_array();
        out.extend_from_slice(&body);
        out.extend_from_slice(check.get(..CHECK_LEN).unwrap_or(&[]));
    }

    fn read(chunk: &[u8]) -> Result<Self, DecodeError> {
        let body_len = PAID_ENTRY_LEN - CHECK_LEN;
        let (body, check) = chunk
            .split_at_checked(body_len)
            .ok_or(DecodeError::Truncated)?;
        let want = sha256::Hash::hash(body).to_byte_array();
        if want.get(..CHECK_LEN) != Some(check) {
            return Err(DecodeError::NonCanonical("an entry's check does not match"));
        }
        let mut r = Reader::new(body);
        let kind = r.u8()?;
        let number = r.u32_le()?;
        let txid = Txid::from_byte_array(r.array()?);
        let sats = r.u64_le()?;
        let at = r.u64_le()?;
        let counter = r.array::<32>()?;
        let index = r.u32_le()?;
        let has_request = r.flag()?;
        let request_raw = r.u64_le()?;
        let has_value = r.flag()?;
        let currency = r.array::<3>()?;
        let minor = r.u64_le()?;
        r.finish()?;
        let what = match kind {
            1 => {
                if !has_request && request_raw != 0 {
                    return Err(DecodeError::NonCanonical("an unused field is not zero"));
                }
                PaidWhat::Refund {
                    sale: SaleKey {
                        counter: DeviceId::from_bytes(&counter)
                            .map_err(|_| DecodeError::NonCanonical("a device key is not valid"))?,
                        index,
                    },
                    request: has_request.then_some(request_raw),
                }
            }
            2 => {
                if counter != [0; 32] || index != 0 || has_request || request_raw != 0 {
                    return Err(DecodeError::NonCanonical("an unused field is not zero"));
                }
                PaidWhat::CashOut
            }
            3 | 4 => {
                if counter != [0; 32]
                    || index != 0
                    || has_request
                    || request_raw != 0
                    || has_value
                    || sats != 0
                {
                    return Err(DecodeError::NonCanonical("an unused field is not zero"));
                }
                PaidWhat::Undone(if kind == 3 {
                    Undone::Refund
                } else {
                    Undone::CashOut
                })
            }
            k => return Err(DecodeError::UnknownKind(k)),
        };
        let value = if has_value {
            if !currency.iter().all(u8::is_ascii_uppercase) {
                return Err(DecodeError::NonCanonical("a currency code is not letters"));
            }
            Some(PaidValue { currency, minor })
        } else {
            if currency != [0; 3] || minor != 0 {
                return Err(DecodeError::NonCanonical("an unused field is not zero"));
            }
            None
        };
        let entry = Self {
            what,
            number,
            txid,
            sats,
            at,
            value,
        };
        // Canonical form: writing it again gives the same bytes.
        let mut again = Vec::with_capacity(PAID_ENTRY_LEN);
        entry.write(&mut again);
        if again != chunk {
            return Err(DecodeError::NonCanonical("an entry is not in its one form"));
        }
        Ok(entry)
    }
}

/// OC-50, OC-55: the entries that stand: every payment that no later entry
/// undoes, and no undoing. A refund paid, undone and paid again by another
/// transaction is the last payment only, and one paid, undone because its
/// transaction was beaten and paid again by the same transaction, once it
/// confirmed after all, is that payment (its entry written again after the
/// undoing).
pub fn standing(entries: &[PaidEntry]) -> Vec<PaidEntry> {
    let mut at: std::collections::BTreeMap<(u8, u32, Txid), usize> = Default::default();
    for (i, e) in entries.iter().enumerate() {
        if e.is_undone() {
            at.remove(&e.payment_key());
        } else {
            at.insert(e.payment_key(), i);
        }
    }
    let mut keep: Vec<usize> = at.into_values().collect();
    keep.sort_unstable();
    keep.into_iter()
        .filter_map(|i| entries.get(i).copied())
        .collect()
}

/// OC-49: what `sats` are worth at a price in minor units per bitcoin,
/// rounded down, as the counter values satoshis (CR-06).
pub fn value_at(sats: u64, minor_per_btc: u64) -> u64 {
    let product = u128::from(sats).saturating_mul(u128::from(minor_per_btc));
    u64::try_from(product / SATS_PER_BTC).unwrap_or(u64::MAX)
}

/// The log's entries, as the file holds them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaidLog {
    entries: Vec<PaidEntry>,
}

impl PaidLog {
    /// An empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// OC-50: read a file back, strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() > MAX_PAID_LOG_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let (mut r, version) = Reader::framed_up_to(bytes, &MAGIC_PAID_LOG, 2)?;
        let rest = r.take(bytes.len().saturating_sub(5))?;
        let (chunks, tail) = rest.as_chunks::<PAID_ENTRY_LEN>();
        if !tail.is_empty() {
            return Err(DecodeError::Truncated);
        }
        r.finish()?;
        let mut entries: Vec<PaidEntry> = Vec::with_capacity(chunks.len());
        // Whether each payment stands now: true after its entry, false
        // after its undoing.
        let mut standing_now: std::collections::BTreeMap<(u8, u32, Txid), bool> =
            Default::default();
        for chunk in chunks {
            let entry = PaidEntry::read(chunk)?;
            let key = entry.payment_key();
            let now = standing_now.get(&key).copied();
            if entry.is_undone() {
                // OC-50: an undoing names a payment that is before it and
                // not undone yet.
                if now != Some(true) {
                    return Err(DecodeError::NonCanonical(
                        "an undoing names no earlier entry",
                    ));
                }
                standing_now.insert(key, false);
            } else {
                // OC-55: a payment may be written again only after its
                // undoing.
                if now == Some(true) {
                    return Err(DecodeError::NonCanonical("an entry repeats an earlier one"));
                }
                standing_now.insert(key, true);
            }
            entries.push(entry);
        }
        // Canonical form: version 2 only for a file that needs it.
        if (version == 2) != entries.iter().any(PaidEntry::is_undone) {
            return Err(DecodeError::NonCanonical(
                "the version is not the one the file needs",
            ));
        }
        Ok(Self { entries })
    }

    /// The file for the log as it is: version 2 only when an entry undoes
    /// another, so a reader that does not know them refuses it.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_PAID_LOG, self.entries.len() * PAID_ENTRY_LEN);
        if self.entries.iter().any(PaidEntry::is_undone) {
            if let Some(v) = out.get_mut(4) {
                *v = 2;
            }
        }
        for e in &self.entries {
            e.write(&mut out);
        }
        out
    }

    /// OC-50: the undoings that the payments `txid` made, and that are not
    /// undone yet, need: one for each entry of that transaction that stands.
    pub fn undoings_for(&self, txid: &Txid, at: u64) -> Vec<PaidEntry> {
        standing(&self.entries)
            .iter()
            .filter(|e| e.txid == *txid)
            .filter_map(|e| e.undoing(at))
            .collect()
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> &[PaidEntry] {
        &self.entries
    }

    /// Whether the log already holds this entry as it stands: a payment that
    /// is not undone, or an undoing of one that is.
    pub fn holds(&self, entry: &PaidEntry) -> bool {
        let stands = self.stands(&entry.payment_key());
        if entry.is_undone() {
            !stands
        } else {
            stands
        }
    }

    /// Whether the payment of this kind, number and transaction was written
    /// and not undone since.
    fn stands(&self, key: &(u8, u32, Txid)) -> bool {
        self.entries
            .iter()
            .rev()
            .find(|e| e.payment_key() == *key)
            .is_some_and(|e| !e.is_undone())
    }

    /// OC-48: the file the log would be with `entries` added, built before
    /// anything is stored. Entries it already holds are left out. `None`
    /// when there is nothing to add; an error when the log would pass
    /// [`MAX_PAID_ENTRIES`].
    pub fn with_added(&self, entries: &[PaidEntry]) -> Result<Option<Self>, usize> {
        let mut next = self.clone();
        for e in entries {
            // An undoing is only added for a payment the log holds.
            let names_one = !e.is_undone() || next.stands(&e.payment_key());
            if names_one && !next.holds(e) {
                next.entries.push(*e);
            }
        }
        if next.entries.len() == self.entries.len() {
            return Ok(None);
        }
        if next.entries.len() > MAX_PAID_ENTRIES {
            return Err(MAX_PAID_ENTRIES);
        }
        Ok(Some(next))
    }
}

#[cfg(test)]
mod tests {
    use bitcoin::hashes::Hash;

    use super::*;

    fn counter() -> DeviceId {
        openbmt_core::identity::Identity::from_secret_bytes(&[0x31; 32])
            .unwrap()
            .device_id()
    }

    fn refund(number: u32, request: Option<u64>) -> PaidEntry {
        PaidEntry {
            what: PaidWhat::Refund {
                sale: SaleKey {
                    counter: counter(),
                    index: 5,
                },
                request,
            },
            number,
            txid: Txid::from_byte_array([number as u8; 32]),
            sats: 10_000,
            at: 1_700_000_000,
            value: Some(PaidValue {
                currency: *b"USD",
                minor: 912,
            }),
        }
    }

    fn cash_out(number: u32) -> PaidEntry {
        PaidEntry {
            what: PaidWhat::CashOut,
            number,
            txid: Txid::from_byte_array([number as u8; 32]),
            sats: 2_500_000,
            at: 1_700_000_100,
            value: None,
        }
    }

    fn log_of(entries: &[PaidEntry]) -> PaidLog {
        PaidLog::new().with_added(entries).unwrap().unwrap()
    }

    #[test]
    fn oc_50_entries_round_trip_and_the_file_is_the_frame_and_whole_entries() {
        let log = log_of(&[refund(1, Some(7)), refund(2, None), cash_out(3)]);
        let bytes = log.to_bytes();
        assert_eq!(&bytes[..4], b"OBM&");
        assert_eq!(bytes.len(), 5 + 3 * PAID_ENTRY_LEN);
        assert_eq!(PaidLog::from_bytes(&bytes).unwrap(), log);
        assert_eq!(
            PaidLog::from_bytes(&PaidLog::new().to_bytes())
                .unwrap()
                .entries()
                .len(),
            0
        );
    }

    #[test]
    fn oc_50_a_cut_padded_flipped_or_repeated_file_is_refused() {
        let bytes = log_of(&[refund(1, Some(7)), cash_out(3)]).to_bytes();
        assert!(PaidLog::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        let mut padded = bytes.clone();
        padded.push(0);
        assert!(PaidLog::from_bytes(&padded).is_err());
        for at in [5, 20, 60, 100, PAID_ENTRY_LEN + 4] {
            let mut flipped = bytes.clone();
            flipped[at] ^= 1;
            assert!(PaidLog::from_bytes(&flipped).is_err(), "byte {at}");
        }
        let mut wrong_magic = bytes.clone();
        wrong_magic[3] = b'#';
        assert!(PaidLog::from_bytes(&wrong_magic).is_err());
        let mut wrong_version = bytes.clone();
        wrong_version[4] = 2;
        assert!(PaidLog::from_bytes(&wrong_version).is_err());
        // The same entry twice.
        let mut twice = bytes.clone();
        twice.extend_from_slice(&bytes[5..5 + PAID_ENTRY_LEN]);
        assert!(PaidLog::from_bytes(&twice).is_err());
    }

    #[test]
    fn oc_50_a_field_a_kind_does_not_use_must_be_zero() {
        // A cash-out that names a sale, with its check made right.
        let mut entry = cash_out(3);
        let mut body = entry.body();
        body[53] = 1;
        let check = sha256::Hash::hash(&body).to_byte_array();
        body.extend_from_slice(&check[..CHECK_LEN]);
        let mut file = wire::start(&MAGIC_PAID_LOG, 0);
        file.extend_from_slice(&body);
        assert_eq!(
            PaidLog::from_bytes(&file),
            Err(DecodeError::NonCanonical("an unused field is not zero"))
        );
        // A value that is none but carries a currency.
        entry.value = None;
        let mut body = entry.body();
        body[98 + 1] = b'U';
        let check = sha256::Hash::hash(&body).to_byte_array();
        body.extend_from_slice(&check[..CHECK_LEN]);
        let mut file = wire::start(&MAGIC_PAID_LOG, 0);
        file.extend_from_slice(&body);
        assert!(PaidLog::from_bytes(&file).is_err());
    }

    #[test]
    fn oc_48_an_entry_is_added_once_and_the_log_only_grows() {
        let log = log_of(&[refund(1, Some(7))]);
        assert_eq!(log.with_added(&[refund(1, Some(7))]), Ok(None));
        let more = log
            .with_added(&[refund(1, Some(7)), cash_out(2)])
            .unwrap()
            .unwrap();
        assert_eq!(more.entries().len(), 2);
        assert_eq!(more.entries()[0], log.entries()[0]);
        // The same number paid by another transaction is another payment.
        let mut again = refund(1, Some(7));
        again.txid = Txid::from_byte_array([99; 32]);
        assert_eq!(
            log.with_added(&[again]).unwrap().unwrap().entries().len(),
            2
        );
    }

    fn undoing_of(e: &PaidEntry) -> PaidEntry {
        e.undoing(1_700_000_500).unwrap()
    }

    #[test]
    fn oc_50_an_undoing_round_trips_in_version_2_and_a_log_without_one_stays_version_1() {
        let paid = refund(1, Some(7));
        let log = log_of(&[paid, cash_out(3)]);
        assert_eq!(log.to_bytes()[4], 1);
        let undone = log.with_added(&[undoing_of(&paid)]).unwrap().unwrap();
        let bytes = undone.to_bytes();
        assert_eq!(bytes[4], 2);
        assert_eq!(bytes.len(), 5 + 3 * PAID_ENTRY_LEN);
        assert_eq!(PaidLog::from_bytes(&bytes).unwrap(), undone);
        // The same undoing twice is written once.
        assert_eq!(undone.with_added(&[undoing_of(&paid)]), Ok(None));
        // A file in the old version still reads, byte for byte.
        let old = log.to_bytes();
        assert_eq!(PaidLog::from_bytes(&old).unwrap().to_bytes(), old);
        // An older reader (version 1 only) would refuse the new file: the
        // version byte is the only difference it can see.
        assert_ne!(bytes[4], old[4]);
    }

    #[test]
    fn oc_50_an_undoing_that_names_nothing_is_refused() {
        // In the log API it is not added at all.
        let log = log_of(&[refund(1, Some(7))]);
        let stray = undoing_of(&cash_out(3));
        assert_eq!(log.with_added(&[stray]), Ok(None));
        // In a file, built by hand with a right check and version.
        let mut file = log.to_bytes();
        file[4] = 2;
        stray.write(&mut file);
        assert_eq!(
            PaidLog::from_bytes(&file),
            Err(DecodeError::NonCanonical(
                "an undoing names no earlier entry"
            ))
        );
        // Before the payment it names is not earlier.
        let paid = refund(1, Some(7));
        let mut file = wire::start(&MAGIC_PAID_LOG, 0);
        file[4] = 2;
        undoing_of(&paid).write(&mut file);
        paid.write(&mut file);
        assert!(PaidLog::from_bytes(&file).is_err());
    }

    #[test]
    fn oc_50_the_version_is_the_one_the_entries_need() {
        let paid = refund(1, Some(7));
        let with = log_of(&[paid])
            .with_added(&[undoing_of(&paid)])
            .unwrap()
            .unwrap();
        let mut as_v1 = with.to_bytes();
        as_v1[4] = 1;
        assert!(PaidLog::from_bytes(&as_v1).is_err());
        let mut as_v2 = log_of(&[paid]).to_bytes();
        as_v2[4] = 2;
        assert!(PaidLog::from_bytes(&as_v2).is_err());
        let mut as_v3 = with.to_bytes();
        as_v3[4] = 3;
        assert_eq!(
            PaidLog::from_bytes(&as_v3),
            Err(DecodeError::UnsupportedVersion(3))
        );
    }

    #[test]
    fn oc_50_an_undoing_has_no_amount_value_or_sale() {
        let paid = refund(1, Some(7));
        let mut loud = undoing_of(&paid);
        loud.sats = 5;
        let mut file = wire::start(&MAGIC_PAID_LOG, 0);
        file[4] = 2;
        paid.write(&mut file);
        loud.write(&mut file);
        assert!(PaidLog::from_bytes(&file).is_err());
    }

    #[test]
    fn oc_50_standing_leaves_out_a_payment_with_its_undoing_and_keeps_the_next_payment() {
        let a = refund(1, Some(7));
        let mut b = a;
        b.txid = Txid::from_byte_array([99; 32]);
        b.sats = 11_000;
        b.at = 1_700_001_000;
        let log = log_of(&[a, cash_out(3)])
            .with_added(&[undoing_of(&a), b])
            .unwrap()
            .unwrap();
        assert_eq!(standing(log.entries()), vec![cash_out(3), b]);
        assert_eq!(log.undoings_for(&a.txid, 1), Vec::new());
        assert_eq!(log.undoings_for(&b.txid, 1), vec![b.undoing(1).unwrap()]);
    }

    #[test]
    fn oc_55_a_payment_may_be_written_again_after_its_undoing_and_stands_once() {
        let paid = refund(1, Some(7));
        let log = log_of(&[paid])
            .with_added(&[undoing_of(&paid)])
            .unwrap()
            .unwrap();
        // Its transaction came back: the payment is written again.
        let mut again = paid;
        again.at += 9_000;
        let back = log.with_added(&[again]).unwrap().unwrap();
        assert_eq!(back.entries().len(), 3);
        assert_eq!(standing(back.entries()), vec![again]);
        let bytes = back.to_bytes();
        assert_eq!(bytes[4], 2);
        assert_eq!(PaidLog::from_bytes(&bytes).unwrap(), back);
        // Written once more while it stands, it is a no-op; and it can be
        // undone again.
        assert_eq!(back.with_added(&[again]), Ok(None));
        assert_eq!(back.undoings_for(&paid.txid, 5).len(), 1);
        // In a file, the same payment twice with no undoing between them,
        // and an undoing of what is undone already, are refused.
        let mut twice = wire::start(&MAGIC_PAID_LOG, 0);
        twice[4] = 2;
        for e in [paid, undoing_of(&paid), paid, paid] {
            e.write(&mut twice);
        }
        assert!(PaidLog::from_bytes(&twice).is_err());
        let mut undone_twice = wire::start(&MAGIC_PAID_LOG, 0);
        undone_twice[4] = 2;
        for e in [paid, undoing_of(&paid), undoing_of(&paid)] {
            e.write(&mut undone_twice);
        }
        assert!(PaidLog::from_bytes(&undone_twice).is_err());
    }

    #[test]
    fn oc_49_value_is_rounded_down_and_never_wraps() {
        assert_eq!(value_at(10_001, 9_000_000), 900);
        assert_eq!(value_at(100_000_000, 9_000_000), 9_000_000);
        assert_eq!(value_at(1, 99_999_999), 0);
        assert_eq!(value_at(u64::MAX, u64::MAX), u64::MAX);
    }
}
