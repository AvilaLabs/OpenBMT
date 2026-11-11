//! The record of a change to the cashiers the counter keeps (CR-142, CR-143).
//!
//! A manager adds or removes a cashier on the counter. The counter app keeps
//! the list and the PINs (CR-140); the library writes one record for each
//! change so that the owner's books show who was added or removed, and by
//! whom. A PIN, or anything made from one, is never in a record, and a new PIN
//! for a cashier is not a record at all.
//!
//! The record is of kind `Pairing` (the kind of "who is on the counter's
//! list"), which no sale, payment, refund or override reader takes. Its body
//! is one tag byte (32 for added, 33 for removed; the sale events use 1 to
//! 19), the time (u64 LE), the cashier's name and the manager's name, each a
//! length (u8) and text. The decoder is strict, as that of [`crate::Event`]
//! is, and re-encoding a decoded body gives the input.

use openbmt_core::records::RecordKind;
use openbmt_core::wire::{DecodeError, Reader};

use crate::refund::refund_text_ok;

const TAG_STAFF_ADDED: u8 = 32;
const TAG_STAFF_REMOVED: u8 = 33;

/// What was done to the list of cashiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaffChange {
    /// A cashier was added.
    Added,
    /// A cashier was removed.
    Removed,
}

impl StaffChange {
    fn tag(self) -> u8 {
        match self {
            Self::Added => TAG_STAFF_ADDED,
            Self::Removed => TAG_STAFF_REMOVED,
        }
    }
}

/// A change to the list of cashiers, as the counter writes it (CR-143).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffEvent {
    /// When.
    pub at: u64,
    /// Added or removed.
    pub change: StaffChange,
    /// The cashier's name.
    pub name: String,
    /// The manager who made the change.
    pub by: String,
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    out.push(u8::try_from(text.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(text.as_bytes());
}

fn get_text(r: &mut Reader<'_>, what: &'static str) -> Result<String, DecodeError> {
    let len = usize::from(r.u8()?);
    let text = std::str::from_utf8(r.take(len)?)
        .map_err(|_| DecodeError::NonCanonical("a text is not text"))?
        .to_string();
    if !refund_text_ok(&text) {
        return Err(DecodeError::NonCanonical(what));
    }
    Ok(text)
}

impl StaffEvent {
    /// The kind of record the event is written as: `Pairing` (CR-143).
    pub fn kind(&self) -> RecordKind {
        RecordKind::Pairing
    }

    /// The record body.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![self.change.tag()];
        out.extend_from_slice(&self.at.to_le_bytes());
        put_text(&mut out, &self.name);
        put_text(&mut out, &self.by);
        out
    }

    /// Read a record body strictly: a tag that is not one of the two, a name
    /// that is not a label and any byte left over are refused.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        let change = match r.u8()? {
            TAG_STAFF_ADDED => StaffChange::Added,
            TAG_STAFF_REMOVED => StaffChange::Removed,
            _ => return Err(DecodeError::NonCanonical("a staff tag is not known")),
        };
        let at = r.u64_le()?;
        let name = get_text(&mut r, "a cashier's name is not a label")?;
        let by = get_text(&mut r, "a manager's name is not a label")?;
        r.finish()?;
        Ok(Self {
            at,
            change,
            name,
            by,
        })
    }
}
