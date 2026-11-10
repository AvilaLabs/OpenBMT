//! What a POS says about an order (PS-01 to PS-03). Everything is bounded and
//! checked when it is built or handed in.

use std::fmt;

use openbmt_core::wire::{DecodeError, Reader};
use openbmt_counter::Currency;

use crate::{MAX_ID_BYTES, MAX_TENDERS, MAX_TEXT_BYTES};

/// Why a value was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelError {
    /// PS-01: an id is empty, longer than 64 bytes or not printable ASCII.
    BadId,
    /// PS-03: a text is empty, longer than 64 bytes or holds a control
    /// character.
    BadText,
    /// PS-02: an order has more than 16 tenders.
    TooManyTenders,
    /// PS-02: two tenders of one order have the same id.
    DuplicateTender,
    /// PS-02: a refund is larger than the tender's amount and tip.
    RefundTooLarge,
    /// PS-72: a cursor is longer than its limit.
    CursorTooLong,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadId => write!(
                f,
                "PS-01: an id must be 1 to {MAX_ID_BYTES} printable ASCII characters"
            ),
            Self::BadText => write!(
                f,
                "PS-03: a text must be 1 to {MAX_TEXT_BYTES} bytes with no control character"
            ),
            Self::TooManyTenders => {
                write!(f, "PS-02: an order may have at most {MAX_TENDERS} tenders")
            }
            Self::DuplicateTender => write!(f, "PS-02: two tenders of one order share an id"),
            Self::RefundTooLarge => {
                write!(f, "PS-02: a refund is larger than the tender and its tip")
            }
            Self::CursorTooLong => write!(f, "PS-72: the POS cursor is too long"),
        }
    }
}

impl std::error::Error for ModelError {}

/// An opaque id: 1 to 64 bytes of printable ASCII (PS-01).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(String);

/// The id of an order (PS-01).
pub type OrderId = Id;
/// The id of a tender (PS-02).
pub type TenderId = Id;

impl Id {
    /// An id; refused unless it is 1 to 64 bytes of printable ASCII.
    pub fn new(s: &str) -> Result<Self, ModelError> {
        let ok = !s.is_empty()
            && s.len() <= MAX_ID_BYTES
            && s.bytes().all(|b| (0x20..=0x7e).contains(&b));
        if ok {
            Ok(Self(s.to_string()))
        } else {
            Err(ModelError::BadId)
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Bounded text: 1 to 64 bytes, no control character (PS-03).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Text(String);

impl Text {
    /// A text; refused unless it is 1 to 64 bytes with no control character.
    pub fn new(s: &str) -> Result<Self, ModelError> {
        if s.is_empty() || s.len() > MAX_TEXT_BYTES || s.chars().any(char::is_control) {
            Err(ModelError::BadText)
        } else {
            Ok(Self(s.to_string()))
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The state of an order in the POS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OrderState {
    /// Still being rung up.
    Open,
    /// Closed and paid.
    Closed,
    /// Voided.
    Voided,
}

/// What kind of tender the POS says it is (PS-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TenderKind {
    /// A tender the POS itself marks as bitcoin.
    Bitcoin,
    /// Cash.
    Cash,
    /// A card.
    Card,
    /// Anything else, including a shop's own tender types.
    Other,
}

/// The state of a tender (PS-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TenderState {
    /// Taken.
    Paid,
    /// Voided.
    Voided,
    /// Refunded, in whole or in part.
    Refunded {
        /// What was refunded, in minor units.
        amount_minor: u64,
    },
}

/// One payment on an order (PS-02).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosTender {
    /// The tender's id, unique within its order.
    pub id: TenderId,
    /// The kind.
    pub kind: TenderKind,
    /// The name the shop gave it.
    pub name: Text,
    /// The amount, without the tip, in minor units.
    pub amount_minor: u64,
    /// The tip, in minor units.
    pub tip_minor: u64,
    /// The state.
    pub state: TenderState,
    /// When it was taken.
    pub at: u64,
    /// The note the cashier may have typed, where a reference can be found.
    pub reference: Option<Text>,
}

/// An order as the POS reports it (PS-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosOrder {
    /// The order's id.
    pub id: OrderId,
    /// The POS location, as the POS names it.
    pub location: Text,
    /// The state.
    pub state: OrderState,
    /// The currency.
    pub currency: Currency,
    /// The total, in minor units.
    pub total_minor: u64,
    /// The tenders, at most 16.
    pub tenders: Vec<PosTender>,
    /// When it was opened.
    pub opened_at: u64,
    /// When it was closed, if it was.
    pub closed_at: Option<u64>,
    /// When it last changed.
    pub updated_at: u64,
    /// A note the cashier may have typed, where a reference can be found.
    pub reference: Option<Text>,
}

impl PosOrder {
    /// Check the order against its bounds (PS-01, PS-02). Ids and text are
    /// checked when they are built, so this checks what is left: the number
    /// of tenders, that ids are unique and that refunds fit.
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.tenders.len() > MAX_TENDERS {
            return Err(ModelError::TooManyTenders);
        }
        for (i, t) in self.tenders.iter().enumerate() {
            if self.tenders.iter().skip(i + 1).any(|u| u.id == t.id) {
                return Err(ModelError::DuplicateTender);
            }
            if let TenderState::Refunded { amount_minor } = t.state {
                if u128::from(amount_minor) > u128::from(t.amount_minor) + u128::from(t.tip_minor) {
                    return Err(ModelError::RefundTooLarge);
                }
            }
        }
        Ok(())
    }

    /// The tender with this id.
    pub fn tender(&self, id: &TenderId) -> Option<&PosTender> {
        self.tenders.iter().find(|t| &t.id == id)
    }

    /// The stored form of the order, with the tenders in id order.
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        put_str(out, self.id.as_str());
        put_str(out, self.location.as_str());
        out.push(match self.state {
            OrderState::Open => 1,
            OrderState::Closed => 2,
            OrderState::Voided => 3,
        });
        out.extend_from_slice(&self.currency.code());
        out.extend_from_slice(&self.total_minor.to_le_bytes());
        out.extend_from_slice(&self.opened_at.to_le_bytes());
        openbmt_core::wire::put_opt_u64(out, self.closed_at);
        out.extend_from_slice(&self.updated_at.to_le_bytes());
        match &self.reference {
            Some(t) => {
                out.push(1);
                put_str(out, t.as_str());
            }
            None => out.push(0),
        }
        out.extend_from_slice(
            &u32::try_from(self.tenders.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for t in &self.tenders {
            put_str(out, t.id.as_str());
            out.push(match t.kind {
                TenderKind::Bitcoin => 1,
                TenderKind::Cash => 2,
                TenderKind::Card => 3,
                TenderKind::Other => 4,
            });
            put_str(out, t.name.as_str());
            out.extend_from_slice(&t.amount_minor.to_le_bytes());
            out.extend_from_slice(&t.tip_minor.to_le_bytes());
            match t.state {
                TenderState::Paid => out.push(1),
                TenderState::Voided => out.push(2),
                TenderState::Refunded { amount_minor } => {
                    out.push(3);
                    out.extend_from_slice(&amount_minor.to_le_bytes());
                }
            }
            out.extend_from_slice(&t.at.to_le_bytes());
            match &t.reference {
                Some(n) => {
                    out.push(1);
                    put_str(out, n.as_str());
                }
                None => out.push(0),
            }
        }
    }

    /// Read one order strictly. Tenders must be in ascending id order.
    pub(crate) fn decode(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let id = get_id(r)?;
        let location = get_text(r)?;
        let state = match r.u8()? {
            1 => OrderState::Open,
            2 => OrderState::Closed,
            3 => OrderState::Voided,
            _ => return Err(DecodeError::NonCanonical("an order state is not known")),
        };
        let currency = Currency::new(r.array::<3>()?)
            .ok_or(DecodeError::NonCanonical("a currency code is not valid"))?;
        let total_minor = r.u64_le()?;
        let opened_at = r.u64_le()?;
        let closed_at = r.opt_u64()?;
        let updated_at = r.u64_le()?;
        let reference = if r.flag()? { Some(get_text(r)?) } else { None };
        // A tender takes at least 2+1+2+8+8+1+8+1 bytes.
        let n = r.count(MAX_TENDERS, 31)?;
        let mut tenders: Vec<PosTender> = Vec::with_capacity(n);
        for _ in 0..n {
            let tid = get_id(r)?;
            if tenders.last().is_some_and(|p| p.id >= tid) {
                return Err(DecodeError::NonCanonical("tenders are not in id order"));
            }
            let kind = match r.u8()? {
                1 => TenderKind::Bitcoin,
                2 => TenderKind::Cash,
                3 => TenderKind::Card,
                4 => TenderKind::Other,
                _ => return Err(DecodeError::NonCanonical("a tender kind is not known")),
            };
            let name = get_text(r)?;
            let amount_minor = r.u64_le()?;
            let tip_minor = r.u64_le()?;
            let state = match r.u8()? {
                1 => TenderState::Paid,
                2 => TenderState::Voided,
                3 => TenderState::Refunded {
                    amount_minor: r.u64_le()?,
                },
                _ => return Err(DecodeError::NonCanonical("a tender state is not known")),
            };
            let at = r.u64_le()?;
            let note = if r.flag()? { Some(get_text(r)?) } else { None };
            tenders.push(PosTender {
                id: tid,
                kind,
                name,
                amount_minor,
                tip_minor,
                state,
                at,
                reference: note,
            });
        }
        let order = Self {
            id,
            location,
            state,
            currency,
            total_minor,
            tenders,
            opened_at,
            closed_at,
            updated_at,
            reference,
        };
        order
            .validate()
            .map_err(|_| DecodeError::NonCanonical("an order breaks its bounds"))?;
        Ok(order)
    }
}

/// Write a short string: one length byte, then the bytes. Every string in
/// this crate is at most 64 bytes.
pub(crate) fn put_str(out: &mut Vec<u8>, s: &str) {
    out.push(u8::try_from(s.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(s.as_bytes());
}

/// Read a short string written by [`put_str`].
pub(crate) fn get_str<'a>(r: &mut Reader<'a>) -> Result<&'a str, DecodeError> {
    let len = usize::from(r.u8()?);
    std::str::from_utf8(r.take(len)?).map_err(|_| DecodeError::NonCanonical("text is not UTF-8"))
}

/// Read a text.
pub(crate) fn get_text(r: &mut Reader<'_>) -> Result<Text, DecodeError> {
    Text::new(get_str(r)?).map_err(|_| DecodeError::NonCanonical("a text is not valid"))
}

/// Read an id.
pub(crate) fn get_id(r: &mut Reader<'_>) -> Result<Id, DecodeError> {
    Id::new(get_str(r)?).map_err(|_| DecodeError::NonCanonical("an id is not valid"))
}
