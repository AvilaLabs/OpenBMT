//! Bills, tabs and the till's reference (CR-245 to CR-253).
//!
//! A bill is the amount owed in the shop's money, paid in parts: each
//! bitcoin part is an ordinary sale, and a part paid another way is written
//! down by the cashier. A tab is a bill whose amount grows while it is open.
//! This module holds the pure side: the types, the split helper of CR-250,
//! the reference check of CR-245 and the stored form of the bills file
//! (`bills.bin`, magic `OBM[`, CR-253). What a bill has received depends on
//! its sales, so the counter works it out (`Counter::bill_view`).
//!
//! ```text
//! OBM[ | 1 | next bill number u32 | n u32 | n * bill
//!
//! bill = number u32 | kind u8 (1 bill, 2 tab) | amount u64 | currency (3)
//!   | opened_at u64 | reference (flag, len u8, text)
//!   | n_parts u8 | n_parts * part
//!   | state u8 (0 open, 1 settled, 2 closed short, 3 cancelled)
//!   | when the state is not 0: ended_at u64 | unpaid u64
//!
//! part = 1 | sale index u32
//!      | 2 | amount u64 | way u8 (1 cash, 2 card, 3 other)
//! ```
//!
//! Bills are in ascending order of number, the open ones and those that
//! ended in the last 7 days. The read is strict and bounded.

use std::collections::{BTreeMap, BTreeSet};

use openbmt_core::wire::{DecodeError, Reader};

use crate::desk::len32;
use crate::money::{Amount, Currency, MAX_SATS};

/// CR-253: the magic of the bills file.
pub const MAGIC_BILLS: [u8; 4] = *b"OBM[";
/// CR-253: the version of the bills file.
const BILLS_VERSION: u8 = 1;
/// CR-253: most bills open at once.
pub const MAX_OPEN_BILLS: usize = 256;
/// CR-253: most parts one bill has.
pub const MAX_BILL_PARTS: usize = 32;
/// CR-253: how long a bill that ended is kept, in seconds: 7 days.
pub const KEEP_ENDED_SECS: u64 = 7 * 86_400;
/// CR-253: most ended bills the file holds (the read is bounded). The oldest
/// are dropped first when the file is written.
pub const MAX_KEPT_ENDED: usize = 4_096;
/// CR-245: the longest reference, in bytes.
pub const MAX_REFERENCE_BYTES: usize = 32;
/// CR-250: the fewest and the most people a bill is split between.
pub const SPLIT_MIN: u8 = 2;
/// CR-250: the most people a bill is split between.
pub const SPLIT_MAX: u8 = 20;
/// CR-246: the largest amount of a bill, in minor units (CR-05: more than
/// this is more than all the bitcoin there is, at the satoshi's own rate).
pub const MAX_BILL_AMOUNT: u64 = MAX_SATS;

/// CR-245: whether `text` is a till reference: 1 to 32 characters of `A-Z`,
/// `a-z`, `0-9`, `-`, `_`, `/`, `#` and `.`.
pub fn reference_ok(text: &str) -> bool {
    (1..=MAX_REFERENCE_BYTES).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'/' | b'#' | b'.'))
}

/// A bill or a tab (CR-246).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillKind {
    /// An amount that does not change.
    Bill,
    /// An amount that grows while the tab is open (CR-251).
    Tab,
}

impl BillKind {
    /// The byte of the records and the file.
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Bill => 1,
            Self::Tab => 2,
        }
    }

    /// From the byte; none when it is not known.
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Bill),
            2 => Some(Self::Tab),
            _ => None,
        }
    }
}

/// How a part that is not bitcoin was paid (CR-247).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartWay {
    /// In cash.
    Cash,
    /// By card.
    Card,
    /// Another way.
    Other,
}

impl PartWay {
    /// The byte of the records and the file.
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Cash => 1,
            Self::Card => 2,
            Self::Other => 3,
        }
    }

    /// From the byte; none when it is not known.
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Cash),
            2 => Some(Self::Card),
            3 => Some(Self::Other),
            _ => None,
        }
    }
}

/// How a bill ended (CR-247).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillEnd {
    /// It received its amount.
    Settled,
    /// It was closed with less than its amount, behind the manager's PIN.
    ClosedShort,
    /// It was cancelled before anything counted.
    Cancelled,
}

impl BillEnd {
    /// The byte of the records and the file.
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Settled => 1,
            Self::ClosedShort => 2,
            Self::Cancelled => 3,
        }
    }

    /// From the byte; none when it is not known.
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Settled),
            2 => Some(Self::ClosedShort),
            3 => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// One part of a bill (CR-253).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillPart {
    /// A bitcoin part: the sale's receive index.
    Sale(u32),
    /// A part paid another way: what the cashier typed and the way.
    Other {
        /// The amount in minor units.
        amount: Amount,
        /// The way.
        way: PartWay,
    },
}

/// How and when a bill ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ended {
    /// How.
    pub how: BillEnd,
    /// When.
    pub at: u64,
    /// What was not paid, for a bill closed short; otherwise 0.
    pub unpaid: Amount,
}

/// One bill (CR-246).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bill {
    pub(crate) number: u32,
    pub(crate) kind: BillKind,
    pub(crate) amount: Amount,
    pub(crate) currency: Currency,
    pub(crate) opened_at: u64,
    pub(crate) reference: Option<String>,
    pub(crate) parts: Vec<BillPart>,
    pub(crate) ended: Option<Ended>,
}

impl Bill {
    /// The bill's number.
    pub fn number(&self) -> u32 {
        self.number
    }
    /// A bill or a tab.
    pub fn kind(&self) -> BillKind {
        self.kind
    }
    /// The amount owed, in minor units.
    pub fn amount(&self) -> Amount {
        self.amount
    }
    /// The shop's currency.
    pub fn currency(&self) -> Currency {
        self.currency
    }
    /// When it opened.
    pub fn opened_at(&self) -> u64 {
        self.opened_at
    }
    /// The till's reference, when it has one.
    pub fn reference(&self) -> Option<&str> {
        self.reference.as_deref()
    }
    /// Its parts, in the order they were added.
    pub fn parts(&self) -> &[BillPart] {
        &self.parts
    }
    /// How it ended, when it did.
    pub fn ended(&self) -> Option<Ended> {
        self.ended
    }
    /// Whether it is still open.
    pub fn is_open(&self) -> bool {
        self.ended.is_none()
    }
}

/// CR-250: why a bill could not be split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    /// The number of people is not from 2 to 20.
    People,
    /// There is less left than one minor unit for each person.
    TooLittle,
}

/// CR-250: the amount each of `people` pays of `left`: each share rounded up
/// to a minor unit, the last share taking what is left over. It is refused
/// for fewer than 2 or more than 20 people, and when the last share would be
/// nothing.
pub fn split_shares(left: u64, people: u8) -> Result<Vec<u64>, SplitError> {
    if !(SPLIT_MIN..=SPLIT_MAX).contains(&people) {
        return Err(SplitError::People);
    }
    let n = u64::from(people);
    let each = left.div_ceil(n);
    let last = left.saturating_sub(each * (n - 1));
    if each == 0 || last == 0 {
        return Err(SplitError::TooLittle);
    }
    let mut shares = vec![each; usize::from(people) - 1];
    shares.push(last);
    Ok(shares)
}

/// The bills the counter holds (CR-253): the next number and every bill that
/// is open or ended in the last 7 days.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bills {
    pub(crate) next: u32,
    pub(crate) bills: BTreeMap<u32, Bill>,
}

impl Default for Bills {
    fn default() -> Self {
        Self::new()
    }
}

impl Bills {
    /// No bills, and the first number is 1 (CR-246).
    pub fn new() -> Self {
        Self {
            next: 1,
            bills: BTreeMap::new(),
        }
    }

    /// The number the next bill gets.
    pub fn next_number(&self) -> u32 {
        self.next
    }

    /// One bill.
    pub fn get(&self, number: u32) -> Option<&Bill> {
        self.bills.get(&number)
    }

    /// Every bill held, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Bill> {
        self.bills.values()
    }

    /// How many bills are open.
    pub fn open_count(&self) -> usize {
        self.bills.values().filter(|b| b.is_open()).count()
    }

    /// The bill a sale is a part of, whether the bill is open or ended.
    pub fn bill_of_sale(&self, sale: u32) -> Option<u32> {
        self.bills
            .values()
            .find(|b| b.parts.contains(&BillPart::Sale(sale)))
            .map(|b| b.number)
    }

    /// Whether a sale is a part of a bill that is still open.
    pub(crate) fn in_open_bill(&self, sale: u32) -> bool {
        self.bills
            .values()
            .any(|b| b.is_open() && b.parts.contains(&BillPart::Sale(sale)))
    }

    /// A copy without the bills that ended more than 7 days before `now`
    /// and, past [`MAX_KEPT_ENDED`], the oldest that ended.
    pub(crate) fn pruned(&self, now: u64) -> Self {
        let mut next = self.clone();
        next.bills.retain(|_, b| {
            b.ended
                .is_none_or(|e| now.saturating_sub(e.at) < KEEP_ENDED_SECS)
        });
        let ended: Vec<u32> = next
            .bills
            .values()
            .filter(|b| !b.is_open())
            .map(|b| b.number)
            .collect();
        for number in ended
            .iter()
            .take(ended.len().saturating_sub(MAX_KEPT_ENDED))
        {
            next.bills.remove(number);
        }
        next
    }

    /// The stored form (see the module docs). The counts are bounded by the
    /// caps, so the length fields cannot overflow.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(13 + self.bills.len() * 48);
        out.extend_from_slice(&MAGIC_BILLS);
        out.push(BILLS_VERSION);
        out.extend_from_slice(&self.next.to_le_bytes());
        out.extend_from_slice(&len32(self.bills.len()).to_le_bytes());
        for b in self.bills.values() {
            out.extend_from_slice(&b.number.to_le_bytes());
            out.push(b.kind.to_u8());
            out.extend_from_slice(&b.amount.0.to_le_bytes());
            out.extend_from_slice(&b.currency.code());
            out.extend_from_slice(&b.opened_at.to_le_bytes());
            match &b.reference {
                Some(r) => {
                    out.push(1);
                    out.push(u8::try_from(r.len()).unwrap_or(u8::MAX));
                    out.extend_from_slice(r.as_bytes());
                }
                None => out.push(0),
            }
            out.push(u8::try_from(b.parts.len()).unwrap_or(u8::MAX));
            for p in &b.parts {
                match p {
                    BillPart::Sale(index) => {
                        out.push(1);
                        out.extend_from_slice(&index.to_le_bytes());
                    }
                    BillPart::Other { amount, way } => {
                        out.push(2);
                        out.extend_from_slice(&amount.0.to_le_bytes());
                        out.push(way.to_u8());
                    }
                }
            }
            match b.ended {
                None => out.push(0),
                Some(e) => {
                    out.push(e.how.to_u8());
                    out.extend_from_slice(&e.at.to_le_bytes());
                    out.extend_from_slice(&e.unpaid.0.to_le_bytes());
                }
            }
        }
        out
    }

    /// Read the stored form strictly: the magic and version, at most 256 open
    /// and [`MAX_KEPT_ENDED`] ended bills, in ascending order of number and
    /// all below the next number, at most 32 parts each, an amount above 0
    /// and at most [`MAX_BILL_AMOUNT`], a reference that is one, a sale that
    /// is a part of one bill once, and what was not paid only on a bill
    /// closed short. Any byte left over is refused.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_BILLS)?;
        let next = r.u32_le()?;
        if next == 0 {
            return Err(DecodeError::NonCanonical("the next bill number is 0"));
        }
        let n = r.count(MAX_OPEN_BILLS + MAX_KEPT_ENDED, 27)?;
        let mut bills = BTreeMap::new();
        let mut sales: BTreeSet<u32> = BTreeSet::new();
        let (mut open, mut ended_n) = (0usize, 0usize);
        let mut last = 0u32;
        for _ in 0..n {
            let number = r.u32_le()?;
            if number <= last || number >= next {
                return Err(DecodeError::NonCanonical(
                    "bill numbers are not ascending and below the next number",
                ));
            }
            last = number;
            let kind = BillKind::from_u8(r.u8()?)
                .ok_or(DecodeError::NonCanonical("a bill kind is not known"))?;
            let amount = r.u64_le()?;
            if amount == 0 || amount > MAX_BILL_AMOUNT {
                return Err(DecodeError::NonCanonical("a bill's amount is not allowed"));
            }
            let currency = Currency::new(r.array::<3>()?)
                .ok_or(DecodeError::NonCanonical("a currency code is not valid"))?;
            let opened_at = r.u64_le()?;
            let reference = if r.flag()? {
                let len = usize::from(r.u8()?);
                let text = std::str::from_utf8(r.take(len)?)
                    .map_err(|_| DecodeError::NonCanonical("a reference is not text"))?;
                if !reference_ok(text) {
                    return Err(DecodeError::NonCanonical("a reference is not allowed"));
                }
                Some(text.to_string())
            } else {
                None
            };
            let n_parts = usize::from(r.u8()?);
            if n_parts > MAX_BILL_PARTS {
                return Err(DecodeError::TooLarge);
            }
            let mut parts = Vec::with_capacity(n_parts);
            for _ in 0..n_parts {
                match r.u8()? {
                    1 => {
                        let sale = r.u32_le()?;
                        if !sales.insert(sale) {
                            return Err(DecodeError::NonCanonical(
                                "a sale is a part of more than one bill",
                            ));
                        }
                        parts.push(BillPart::Sale(sale));
                    }
                    2 => {
                        let amount = r.u64_le()?;
                        if amount == 0 {
                            return Err(DecodeError::NonCanonical("a part is for nothing"));
                        }
                        let way = PartWay::from_u8(r.u8()?)
                            .ok_or(DecodeError::NonCanonical("a way of paying is not known"))?;
                        parts.push(BillPart::Other {
                            amount: Amount(amount),
                            way,
                        });
                    }
                    _ => return Err(DecodeError::NonCanonical("a part kind is not known")),
                }
            }
            let ended = match r.u8()? {
                0 => None,
                state => {
                    let how = BillEnd::from_u8(state)
                        .ok_or(DecodeError::NonCanonical("a bill's state is not known"))?;
                    let at = r.u64_le()?;
                    let unpaid = r.u64_le()?;
                    if (how == BillEnd::ClosedShort) != (unpaid != 0) || unpaid > amount {
                        return Err(DecodeError::NonCanonical(
                            "only a bill closed short holds what was not paid",
                        ));
                    }
                    Some(Ended {
                        how,
                        at,
                        unpaid: Amount(unpaid),
                    })
                }
            };
            if ended.is_none() {
                open += 1;
                if open > MAX_OPEN_BILLS {
                    return Err(DecodeError::TooLarge);
                }
            } else {
                ended_n += 1;
                if ended_n > MAX_KEPT_ENDED {
                    return Err(DecodeError::TooLarge);
                }
            }
            bills.insert(
                number,
                Bill {
                    number,
                    kind,
                    amount: Amount(amount),
                    currency,
                    opened_at,
                    reference,
                    parts,
                    ended,
                },
            );
        }
        r.finish()?;
        Ok(Self { next, bills })
    }
}
