//! The allocation book of a shop that uses its own wallet (OA-106), and the
//! counts that follow from it: the indexes a counter has not used yet
//! (OA-112) and the gap limit the shop's wallet needs (OA-113).
//!
//! Every device that sells gets its ranges from one book. A range holds
//! [`RANGE_LEN`] indexes, and the next one starts where the highest range
//! ever given ends, so the ranges of all devices tile the indexes from 0
//! with no gap and no overlap, whoever got them and whether or not a counter
//! is forgotten later. Nothing is ever removed from the book.
//!
//! ```text
//! book: "OBM?" | 2 | device count u32 LE
//!         | per device, ascending by key: device (32) | range count u32 LE
//!           | per range, in the order given: start u32 LE | given at u64 LE
//! ```
//!
//! Every range is [`RANGE_LEN`] long, so only the start is written, with the
//! owner's clock in seconds at the moment the range was given (OC-10, OC-23:
//! the live watch sets a range aside only when it has been quiet for the
//! quiet time). Decoding is strict: the version is 2 (a version 1 book is
//! refused), the devices ascend, no device has no range, every start is a
//! multiple of [`RANGE_LEN`], and the starts of all devices together are
//! exactly 0, 1000, 2000 and so on up to the last, none twice.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use openbmt_core::identity::DeviceId;
use openbmt_core::pinned_wallet::IndexRange;
use openbmt_core::wire::{self, DecodeError, Reader};

use crate::types::IssuedRange;

/// The magic of the allocation book.
pub const MAGIC_ALLOCATION_BOOK: [u8; 4] = *b"OBM?";

/// OA-106, WL-23: indexes in a range.
pub const RANGE_LEN: u32 = 1_000;

/// OA-112: a counter with fewer unused indexes than this is given a range.
pub const TOP_UP_BELOW: u64 = 300;

/// OA-113: the gap most wallets look at by default. A shop that needs more
/// is told so.
pub const WALLET_GAP: u32 = 20;

/// The version byte of the book.
pub const BOOK_VERSION: u8 = 2;

/// Bytes before the first device: the magic, the version and the device
/// count.
pub const HEADER_BYTES: usize = 9;

/// Bytes a device costs: its key and its range count.
pub const DEVICE_BYTES: usize = 36;

/// Bytes a range costs: its start and the time it was given.
pub const RANGE_BYTES: usize = 12;

/// OA-106, OA-107: the most ranges the book holds, which tile the indexes
/// up to the wallet's last, 2^31.
pub const MAX_RANGES: usize = (INDEX_LIMIT / RANGE_LEN) as usize;

/// The most devices that hold a range.
pub const MAX_DEVICES: usize = 64;

/// The first index the wallet cannot make (hardened, WL-23).
pub const INDEX_LIMIT: u32 = 1 << 31;

/// The size of a book in bytes with `devices` devices and `ranges` ranges.
pub const fn encoded_len(devices: usize, ranges: usize) -> usize {
    HEADER_BYTES + devices * DEVICE_BYTES + ranges * RANGE_BYTES
}

/// Why the book gave no range, or did not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookError {
    /// OA-106: the next range would reach 2^31.
    Full,
    /// The book holds ranges for as many devices as it takes.
    TooManyDevices,
    /// The bytes are not a book, and say why.
    Corrupt(&'static str),
}

impl fmt::Display for BookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => write!(f, "OA-106: no index is left below 2^31"),
            Self::TooManyDevices => write!(f, "OA-106: the book holds {MAX_DEVICES} devices"),
            Self::Corrupt(why) => write!(f, "OA-106: the allocation book is bad: {why}"),
        }
    }
}

impl std::error::Error for BookError {}

fn corrupt(e: DecodeError) -> BookError {
    match e {
        DecodeError::NonCanonical(why) => BookError::Corrupt(why),
        DecodeError::TrailingBytes => BookError::Corrupt("bytes follow the last range"),
        _ => BookError::Corrupt("it is cut short or not framed as it is written"),
    }
}

/// OA-106: every range given, by device.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllocationBook {
    /// The first index of each range of a device, in the order given, and
    /// the owner's clock when it was given.
    given: BTreeMap<DeviceId, Vec<(u32, u64)>>,
    /// How many ranges were given in all: the next starts at this times
    /// [`RANGE_LEN`].
    taken: u32,
}

impl AllocationBook {
    /// An empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// OA-106: the next range for `device`: [`RANGE_LEN`] indexes from the
    /// end of the highest range ever given (0 for the first). The book
    /// holds it from here on, with `now`, the owner's clock in seconds.
    pub fn allocate(&mut self, device: DeviceId, now: u64) -> Result<IndexRange, BookError> {
        let start = self.taken.checked_mul(RANGE_LEN).ok_or(BookError::Full)?;
        let end = start.checked_add(RANGE_LEN).ok_or(BookError::Full)?;
        if end >= INDEX_LIMIT {
            return Err(BookError::Full);
        }
        if !self.given.contains_key(&device) && self.given.len() >= MAX_DEVICES {
            return Err(BookError::TooManyDevices);
        }
        let range = IndexRange::new(device, start, end).map_err(|_| BookError::Full)?;
        self.given.entry(device).or_default().push((start, now));
        self.taken += 1;
        Ok(range)
    }

    /// The first index nobody was given: the end of the highest range.
    pub fn next_start(&self) -> u32 {
        self.taken.saturating_mul(RANGE_LEN)
    }

    /// How many ranges were given in all.
    pub fn ranges_given(&self) -> u32 {
        self.taken
    }

    /// The ranges of `device`, in the order given.
    pub fn ranges_of(&self, device: &DeviceId) -> Vec<IndexRange> {
        self.given
            .get(device)
            .into_iter()
            .flatten()
            .filter_map(|(start, _)| {
                IndexRange::new(*device, *start, start.saturating_add(RANGE_LEN)).ok()
            })
            .collect()
    }

    /// OC-10: every range given, as the owner's issued ranges: the first
    /// index, how many, the device it was given to and the owner's clock
    /// when it was given. Ascending by start.
    pub fn issued(&self) -> Vec<IssuedRange> {
        let mut all: Vec<IssuedRange> = self
            .given
            .iter()
            .flat_map(|(device, ranges)| {
                ranges.iter().map(|(start, at)| IssuedRange {
                    start: *start,
                    count: RANGE_LEN,
                    counter: *device,
                    signed_at: *at,
                })
            })
            .collect();
        all.sort_by_key(|r| r.start);
        all
    }

    /// Whether `device` was given a range.
    pub fn has_device(&self, device: &DeviceId) -> bool {
        self.given.contains_key(device)
    }

    /// The devices that were given a range, ascending by key.
    pub fn devices(&self) -> impl Iterator<Item = &DeviceId> {
        self.given.keys()
    }

    /// OA-107: every index of every range, ascending: 0 up to, not
    /// including, the end of the highest range. The owner node watches all
    /// of them, as OC-22 to OC-24 say.
    pub fn indexes(&self) -> std::ops::Range<u32> {
        0..self.next_start()
    }

    /// The device a range was given to, for an index: `None` above the
    /// highest range.
    pub fn device_of(&self, index: u32) -> Option<DeviceId> {
        let start = index - index % RANGE_LEN;
        self.given
            .iter()
            .find(|(_, ranges)| ranges.binary_search_by_key(&start, |(s, _)| *s).is_ok())
            .map(|(d, _)| *d)
            .filter(|_| index < self.next_start())
    }

    /// The bytes of the book.
    pub fn to_bytes(&self) -> Vec<u8> {
        let ranges = usize::try_from(self.taken).unwrap_or(0);
        let mut out = Vec::with_capacity(encoded_len(self.given.len(), ranges));
        out.extend_from_slice(&MAGIC_ALLOCATION_BOOK);
        out.push(BOOK_VERSION);
        out.extend_from_slice(&wire::len_u32(self.given.len()).to_le_bytes());
        for (device, ranges) in &self.given {
            out.extend_from_slice(&device.to_bytes());
            out.extend_from_slice(&wire::len_u32(ranges.len()).to_le_bytes());
            for (start, at) in ranges {
                out.extend_from_slice(&start.to_le_bytes());
                out.extend_from_slice(&at.to_le_bytes());
            }
        }
        out
    }

    /// Read a book strictly (OA-106).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BookError> {
        let (mut r, version) =
            Reader::framed_up_to(bytes, &MAGIC_ALLOCATION_BOOK, BOOK_VERSION).map_err(corrupt)?;
        if version != BOOK_VERSION {
            return Err(BookError::Corrupt("the book is of an older version"));
        }
        let devices = r.count(MAX_DEVICES, DEVICE_BYTES).map_err(corrupt)?;
        let mut given: BTreeMap<DeviceId, Vec<(u32, u64)>> = BTreeMap::new();
        let mut all: BTreeSet<u32> = BTreeSet::new();
        let mut last: Option<DeviceId> = None;
        let mut total = 0usize;
        for _ in 0..devices {
            let device = r.device_id().map_err(corrupt)?;
            if last.is_some_and(|l| l.to_bytes() >= device.to_bytes()) {
                return Err(BookError::Corrupt("the devices are not in order"));
            }
            last = Some(device);
            let n = r.count(MAX_RANGES, RANGE_BYTES).map_err(corrupt)?;
            if n == 0 {
                return Err(BookError::Corrupt("a device has no range"));
            }
            total += n;
            if total > MAX_RANGES {
                return Err(BookError::Corrupt("more ranges than the book holds"));
            }
            let mut starts = Vec::with_capacity(n);
            for _ in 0..n {
                let start = r.u32_le().map_err(corrupt)?;
                let at = r.u64_le().map_err(corrupt)?;
                if start % RANGE_LEN != 0 {
                    return Err(BookError::Corrupt("a range does not start on a boundary"));
                }
                if starts.last().is_some_and(|(l, _)| *l >= start) {
                    return Err(BookError::Corrupt("a device's ranges are not in order"));
                }
                if !all.insert(start) {
                    return Err(BookError::Corrupt("two ranges start at the same index"));
                }
                starts.push((start, at));
            }
            given.insert(device, starts);
        }
        r.finish().map_err(corrupt)?;
        let taken = u32::try_from(total).map_err(|_| BookError::Corrupt("too many ranges"))?;
        // The starts of all devices are 0, 1000, ... with none missing.
        if !all
            .iter()
            .enumerate()
            .all(|(i, s)| u64::try_from(i).is_ok_and(|i| u64::from(*s) == i * u64::from(RANGE_LEN)))
        {
            return Err(BookError::Corrupt("the ranges leave a gap"));
        }
        let book = Self { given, taken };
        if book.to_bytes() != bytes {
            return Err(BookError::Corrupt("it does not encode to the same bytes"));
        }
        Ok(book)
    }
}

/// OA-112: the indexes of the ranges a counter holds that its records have
/// not used: those of `held` (start, end) that are not in `opened`. The
/// first range is held from its bundle, the later ones from the counter's
/// range-held records, so the caller passes both.
pub fn unused_indexes(held: &[(u32, u32)], opened: &BTreeSet<u32>) -> u64 {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut total: u64 = 0;
    for (start, end) in held {
        total += u64::from(end.saturating_sub(*start));
        for index in opened.range(*start..*end) {
            seen.insert(*index);
        }
    }
    total.saturating_sub(seen.len() as u64)
}

/// OA-112: whether a counter with `unused` indexes left, and no range
/// waiting for it, is given its next range.
pub fn needs_range(unused: u64, range_waiting: bool) -> bool {
    unused < TOP_UP_BELOW && !range_waiting
}

/// OA-113: how far ahead the shop's wallet must look to show every payment:
/// the longest run of indexes with no payment, counted from index 0 to the
/// highest index given out, plus one. No index given out, nothing needed
/// (0). `paid` are the indexes that received a payment.
pub fn gap_limit(paid: &BTreeSet<u32>, highest_given: Option<u32>) -> u32 {
    let Some(top) = highest_given else {
        return 0;
    };
    // The highest index that was paid counts as given out.
    let top = paid.iter().next_back().map_or(top, |p| top.max(*p));
    let mut longest: u32 = 0;
    // The index after the last paid one seen; the run of unpaid ones ends
    // at the next paid index or at `top`.
    let mut from: u32 = 0;
    for p in paid.range(..=top) {
        longest = longest.max(p - from);
        from = p.saturating_add(1);
    }
    longest = longest.max(top.saturating_add(1).saturating_sub(from));
    longest.saturating_add(1)
}

/// OA-113: whether the wallet's page says to look further ahead.
pub fn gap_needs_notice(needed: u32) -> bool {
    needed > WALLET_GAP
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(n: u8) -> DeviceId {
        openbmt_core::identity::Identity::from_secret_bytes(&[n; 32])
            .unwrap()
            .device_id()
    }

    #[test]
    fn the_book_round_trips_and_refuses_every_cut() {
        let mut b = AllocationBook::new();
        for (i, d) in [3, 4, 3, 5, 4].into_iter().enumerate() {
            b.allocate(dev(d), 1_000 + i as u64).unwrap();
        }
        let bytes = b.to_bytes();
        assert_eq!(AllocationBook::from_bytes(&bytes).unwrap(), b);
        for n in 0..bytes.len() {
            assert!(AllocationBook::from_bytes(&bytes[..n]).is_err(), "cut {n}");
        }
        let mut longer = bytes;
        longer.push(0);
        assert!(AllocationBook::from_bytes(&longer).is_err());
    }

    #[test]
    fn oa_106_a_book_of_a_hundred_ranges_round_trips_with_its_times() {
        let mut b = AllocationBook::new();
        for i in 0..100u64 {
            let d = dev(3 + u8::try_from(i % 4).unwrap());
            b.allocate(d, 5_000 + i * 60).unwrap();
        }
        assert_eq!(b.ranges_given(), 100);
        assert_eq!(b.next_start(), 100_000);
        let bytes = b.to_bytes();
        assert_eq!(bytes.len(), encoded_len(4, 100));
        let back = AllocationBook::from_bytes(&bytes).unwrap();
        assert_eq!(back, b);
        let issued = back.issued();
        assert_eq!(issued.len(), 100);
        assert_eq!(issued[7].start, 7_000);
        assert_eq!(issued[7].signed_at, 5_000 + 7 * 60);
        assert_eq!(issued[7].counter, dev(3 + 3));
        assert_eq!(back.device_of(99_999), Some(dev(3 + 3)));
    }

    #[test]
    fn oa_107_the_book_goes_past_ninety_thousand_indexes() {
        let mut b = AllocationBook::new();
        for i in 0..95u64 {
            b.allocate(dev(3), i).unwrap();
        }
        assert_eq!(b.next_start(), 95_000);
        assert_eq!(b.indexes().len(), 95_000);
    }

    #[test]
    fn oa_106_the_largest_book_costs_twelve_bytes_a_range() {
        // One range, then two: the cost of a range is what the bytes grow by.
        let mut b = AllocationBook::new();
        b.allocate(dev(3), 1).unwrap();
        let one = b.to_bytes();
        b.allocate(dev(3), 2).unwrap();
        let two = b.to_bytes();
        assert_eq!(two.len() - one.len(), RANGE_BYTES);
        // The header: the magic, the version and the device count.
        assert_eq!(&one[..5], b"OBM?\x02");
        assert_eq!(one.len(), HEADER_BYTES + DEVICE_BYTES + RANGE_BYTES);
        // A second device costs its key and its range count.
        b.allocate(dev(4), 3).unwrap();
        assert_eq!(b.to_bytes().len() - two.len(), DEVICE_BYTES + RANGE_BYTES);
        // The largest legal book: every range up to 2^31 for 64 devices.
        assert_eq!(MAX_RANGES, 2_147_483);
        assert_eq!(
            encoded_len(MAX_DEVICES, MAX_RANGES),
            9 + 64 * 36 + 2_147_483 * 12
        );
        assert!(encoded_len(MAX_DEVICES, MAX_RANGES) < 26_000_000);
        // The last range ends below 2^31, and the next would not fit.
        assert!(u64::try_from(MAX_RANGES).unwrap() * u64::from(RANGE_LEN) < u64::from(INDEX_LIMIT));
        assert!(
            (u64::try_from(MAX_RANGES).unwrap() + 1) * u64::from(RANGE_LEN)
                >= u64::from(INDEX_LIMIT)
        );
    }

    #[test]
    fn oa_106_the_book_is_full_at_two_to_the_thirty_first() {
        // The count is set, not reached: 2 million ranges are not made here.
        let mut b = AllocationBook::new();
        b.allocate(dev(3), 1).unwrap();
        b.taken = u32::try_from(MAX_RANGES).unwrap() - 1;
        let last = b.allocate(dev(3), 2).unwrap();
        assert_eq!(last.end, u32::try_from(MAX_RANGES).unwrap() * RANGE_LEN);
        assert!(last.end < INDEX_LIMIT);
        let before = b.clone();
        assert_eq!(b.allocate(dev(3), 3), Err(BookError::Full));
        assert_eq!(b, before, "a refusal changes nothing");
    }

    #[test]
    fn oa_106_a_version_one_book_is_refused() {
        let mut b = AllocationBook::new();
        b.allocate(dev(3), 1).unwrap();
        let mut bytes = b.to_bytes();
        bytes[4] = 1;
        assert_eq!(
            AllocationBook::from_bytes(&bytes),
            Err(BookError::Corrupt("the book is of an older version"))
        );
        bytes[4] = 3;
        assert!(AllocationBook::from_bytes(&bytes).is_err());
    }
}
