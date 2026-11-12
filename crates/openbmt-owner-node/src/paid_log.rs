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
//! The file is `OBM&`, version 1, and then entries of a fixed length, each
//! ending in a check of its own (the first 8 bytes of the SHA-256 of the
//! entry before it). It is read strictly: a length that is not a whole number
//! of entries, a check that does not match, a flag that is not 0 or 1, a field
//! that a kind does not use and is not zero, an entry that repeats an earlier
//! one and a currency that is not three capital letters all refuse the whole
//! file. Nothing is repaired.

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
    /// The entry's kind, number and transaction: two entries with the same
    /// are the same payment, written once.
    fn key(&self) -> (u8, u32, Txid) {
        (self.kind_byte(), self.number, self.txid)
    }

    fn kind_byte(&self) -> u8 {
        match self.what {
            PaidWhat::Refund { .. } => 1,
            PaidWhat::CashOut => 2,
        }
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
            PaidWhat::CashOut => out.extend_from_slice(&[0; 32 + 4 + 1 + 8]),
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
        let mut r = Reader::framed(bytes, &MAGIC_PAID_LOG)?;
        let rest = r.take(bytes.len().saturating_sub(5))?;
        let (chunks, tail) = rest.as_chunks::<PAID_ENTRY_LEN>();
        if !tail.is_empty() {
            return Err(DecodeError::Truncated);
        }
        r.finish()?;
        let mut entries: Vec<PaidEntry> = Vec::with_capacity(chunks.len());
        let mut seen = std::collections::BTreeSet::new();
        for chunk in chunks {
            let entry = PaidEntry::read(chunk)?;
            if !seen.insert(entry.key()) {
                return Err(DecodeError::NonCanonical("an entry repeats an earlier one"));
            }
            entries.push(entry);
        }
        Ok(Self { entries })
    }

    /// The file for the log as it is.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_PAID_LOG, self.entries.len() * PAID_ENTRY_LEN);
        for e in &self.entries {
            e.write(&mut out);
        }
        out
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> &[PaidEntry] {
        &self.entries
    }

    /// Whether the log already holds this payment.
    pub fn holds(&self, entry: &PaidEntry) -> bool {
        self.entries.iter().any(|e| e.key() == entry.key())
    }

    /// OC-48: the file the log would be with `entries` added, built before
    /// anything is stored. Entries it already holds are left out. `None`
    /// when there is nothing to add; an error when the log would pass
    /// [`MAX_PAID_ENTRIES`].
    pub fn with_added(&self, entries: &[PaidEntry]) -> Result<Option<Self>, usize> {
        let mut next = self.clone();
        for e in entries {
            if !next.holds(e) {
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

    #[test]
    fn oc_49_value_is_rounded_down_and_never_wraps() {
        assert_eq!(value_at(10_001, 9_000_000), 900);
        assert_eq!(value_at(100_000_000, 9_000_000), 9_000_000);
        assert_eq!(value_at(1, 99_999_999), 0);
        assert_eq!(value_at(u64::MAX, u64::MAX), u64::MAX);
    }
}
