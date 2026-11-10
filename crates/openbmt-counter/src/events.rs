//! The body of every record the counter writes (CR-32).
//!
//! A record body is one tag byte, the sale's receive index (u32 LE), the time
//! (u64 LE), then the fields of the event. The decoder is strict: it refuses
//! an unknown tag, a field that is not canonical and any byte left over, and
//! re-encoding a decoded event gives the input.

use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::records::RecordKind;
use openbmt_core::wire::{DecodeError, Reader};

use crate::desk::Role;
use crate::money::{Amount, Currency};
use crate::MAX_NAME_BYTES;

const TAG_OPENED: u8 = 1;
const TAG_EXPIRED: u8 = 2;
const TAG_CANCELLED: u8 = 3;
const TAG_PAID_AT_ONCE: u8 = 4;
const TAG_CONFIRMED: u8 = 5;
const TAG_FINAL: u8 = 6;
const TAG_LATE_PAYMENT: u8 = 7;
const TAG_SURPLUS_OWED: u8 = 8;
const TAG_DUST: u8 = 9;
const TAG_OVERRIDE: u8 = 10;
const TAG_REPLACED: u8 = 11;
const TAG_REOPENED: u8 = 12;
const TAG_POS_PAID: u8 = 13;
const TAG_POS_UNPAID: u8 = 14;

/// Something that happened to a sale and is written down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A sale opened (record kind `Sale`).
    Opened {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The amount in the shop's currency.
        amount: Amount,
        /// The currency.
        currency: Currency,
        /// The satoshis the first quote asked for.
        needs_sats: u64,
        /// The first quote's rate, in minor units per bitcoin.
        minor_per_btc: u64,
    },
    /// The sale ended with nothing received (CR-07; kind `Sale`).
    Expired {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
    },
    /// The cashier cancelled the sale (CR-07; kind `Sale`).
    Cancelled {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
    },
    /// The sale was accepted at once (CR-13; kind `Payment`).
    PaidAtOnce {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The amount put into the exposure ledger.
        at_risk_sats: u64,
        /// The label shown.
        label: InstantLabel,
        /// The check mode the limits were taken from.
        mode: CheckMode,
    },
    /// The sale is confirmed at the shop's depth (kind `Payment`).
    Confirmed {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// What has arrived.
        received_sats: u64,
        /// The depth reached.
        depth: u32,
    },
    /// The sale is confirmed at the refund depth: the books count it as money
    /// (CR-31; kind `Payment`).
    Final {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// What has arrived.
        received_sats: u64,
        /// The depth reached.
        depth: u32,
    },
    /// A payment arrived for a sale that had ended (CR-19; kind `Payment`).
    LatePayment {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The transaction.
        txid: [u8; 32],
        /// What it pays the sale's script.
        sats: u64,
    },
    /// More arrived than the sale needs (CR-16; kind `Payment`).
    SurplusOwed {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The surplus owed to the customer.
        sats: u64,
    },
    /// What arrived is below the dust line (CR-17; kind `Payment`).
    Dust {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// What arrived.
        sats: u64,
    },
    /// A person accepted the sale (CR-22; kind `Override`).
    Override {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// Cashier or manager.
        role: Role,
        /// Who.
        name: String,
        /// The amount put at risk.
        sats: u64,
    },
    /// A conflict was seen on a sale that had been reported paid or accepted
    /// at once (kind `Conflict`).
    Replaced {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
    },
    /// A block that confirmed the payment left the chain (kind `Conflict`).
    Reopened {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
    },
    /// The POS was told the sale is paid: written with `Report::PosPaid`
    /// (CR-30, CR-32; kind `Payment`).
    PosPaid {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
    },
    /// The POS was told the sale is not paid any more: written with
    /// `Report::PosUnpaid` (CR-30, CR-32; kind `Payment`).
    PosUnpaid {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
    },
}

fn label_byte(l: InstantLabel) -> u8 {
    match l {
        InstantLabel::LightCheck => 1,
        InstantLabel::YourNode => 2,
        InstantLabel::YourBox => 3,
    }
}

/// The byte a label is stored as (1 light check, 2 your node, 3 your Box).
pub(crate) fn label_to_u8(l: InstantLabel) -> u8 {
    label_byte(l)
}

/// Read a label byte.
pub(crate) fn label_from_u8(b: u8) -> Option<InstantLabel> {
    match b {
        1 => Some(InstantLabel::LightCheck),
        2 => Some(InstantLabel::YourNode),
        3 => Some(InstantLabel::YourBox),
        _ => None,
    }
}

/// The byte a check mode is stored as (1 light, 2 node, 3 Box only).
pub(crate) fn mode_to_u8(m: CheckMode) -> u8 {
    match m {
        CheckMode::Light => 1,
        CheckMode::NodeAlsoSeenByLight => 2,
        CheckMode::BoxReportsOnly => 3,
    }
}

/// Read a check mode byte.
pub(crate) fn mode_from_u8(b: u8) -> Option<CheckMode> {
    match b {
        1 => Some(CheckMode::Light),
        2 => Some(CheckMode::NodeAlsoSeenByLight),
        3 => Some(CheckMode::BoxReportsOnly),
        _ => None,
    }
}

impl Event {
    /// The sale this event is about.
    pub fn index(&self) -> u32 {
        match self {
            Self::Opened { index, .. }
            | Self::Expired { index, .. }
            | Self::Cancelled { index, .. }
            | Self::PaidAtOnce { index, .. }
            | Self::Confirmed { index, .. }
            | Self::Final { index, .. }
            | Self::LatePayment { index, .. }
            | Self::SurplusOwed { index, .. }
            | Self::Dust { index, .. }
            | Self::Override { index, .. }
            | Self::Replaced { index, .. }
            | Self::Reopened { index, .. }
            | Self::PosPaid { index, .. }
            | Self::PosUnpaid { index, .. } => *index,
        }
    }

    /// The kind of record the event is written as.
    pub fn kind(&self) -> RecordKind {
        match self {
            Self::Opened { .. } | Self::Expired { .. } | Self::Cancelled { .. } => RecordKind::Sale,
            Self::PaidAtOnce { .. }
            | Self::Confirmed { .. }
            | Self::Final { .. }
            | Self::LatePayment { .. }
            | Self::SurplusOwed { .. }
            | Self::Dust { .. }
            | Self::PosPaid { .. }
            | Self::PosUnpaid { .. } => RecordKind::Payment,
            Self::Override { .. } => RecordKind::Override,
            Self::Replaced { .. } | Self::Reopened { .. } => RecordKind::Conflict,
        }
    }

    fn head(&self) -> (u8, u32, u64) {
        match self {
            Self::Opened { index, at, .. } => (TAG_OPENED, *index, *at),
            Self::Expired { index, at } => (TAG_EXPIRED, *index, *at),
            Self::Cancelled { index, at } => (TAG_CANCELLED, *index, *at),
            Self::PaidAtOnce { index, at, .. } => (TAG_PAID_AT_ONCE, *index, *at),
            Self::Confirmed { index, at, .. } => (TAG_CONFIRMED, *index, *at),
            Self::Final { index, at, .. } => (TAG_FINAL, *index, *at),
            Self::LatePayment { index, at, .. } => (TAG_LATE_PAYMENT, *index, *at),
            Self::SurplusOwed { index, at, .. } => (TAG_SURPLUS_OWED, *index, *at),
            Self::Dust { index, at, .. } => (TAG_DUST, *index, *at),
            Self::Override { index, at, .. } => (TAG_OVERRIDE, *index, *at),
            Self::Replaced { index, at } => (TAG_REPLACED, *index, *at),
            Self::Reopened { index, at } => (TAG_REOPENED, *index, *at),
            Self::PosPaid { index, at } => (TAG_POS_PAID, *index, *at),
            Self::PosUnpaid { index, at } => (TAG_POS_UNPAID, *index, *at),
        }
    }

    /// The record body: tag, index (u32 LE), time (u64 LE), then the fields.
    pub fn encode(&self) -> Vec<u8> {
        let (tag, index, at) = self.head();
        let mut out = vec![tag];
        out.extend_from_slice(&index.to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        match self {
            Self::Opened {
                amount,
                currency,
                needs_sats,
                minor_per_btc,
                ..
            } => {
                out.extend_from_slice(&amount.0.to_le_bytes());
                out.extend_from_slice(&currency.code());
                out.extend_from_slice(&needs_sats.to_le_bytes());
                out.extend_from_slice(&minor_per_btc.to_le_bytes());
            }
            Self::PaidAtOnce {
                at_risk_sats,
                label,
                mode,
                ..
            } => {
                out.extend_from_slice(&at_risk_sats.to_le_bytes());
                out.push(label_byte(*label));
                out.push(mode_to_u8(*mode));
            }
            Self::Confirmed {
                received_sats,
                depth,
                ..
            }
            | Self::Final {
                received_sats,
                depth,
                ..
            } => {
                out.extend_from_slice(&received_sats.to_le_bytes());
                out.extend_from_slice(&depth.to_le_bytes());
            }
            Self::LatePayment { txid, sats, .. } => {
                out.extend_from_slice(txid);
                out.extend_from_slice(&sats.to_le_bytes());
            }
            Self::SurplusOwed { sats, .. } | Self::Dust { sats, .. } => {
                out.extend_from_slice(&sats.to_le_bytes());
            }
            Self::Override {
                role, name, sats, ..
            } => {
                out.push(*role as u8);
                out.push(u8::try_from(name.len()).unwrap_or(u8::MAX));
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(&sats.to_le_bytes());
            }
            Self::Expired { .. }
            | Self::Cancelled { .. }
            | Self::Replaced { .. }
            | Self::Reopened { .. }
            | Self::PosPaid { .. }
            | Self::PosUnpaid { .. } => {}
        }
        out
    }

    /// Read a record body strictly.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        let tag = r.u8()?;
        let index = r.u32_le()?;
        let at = r.u64_le()?;
        let event = match tag {
            TAG_OPENED => Self::Opened {
                index,
                at,
                amount: Amount(r.u64_le()?),
                currency: Currency::new(r.array::<3>()?)
                    .ok_or(DecodeError::NonCanonical("a currency code is not valid"))?,
                needs_sats: r.u64_le()?,
                minor_per_btc: r.u64_le()?,
            },
            TAG_EXPIRED => Self::Expired { index, at },
            TAG_CANCELLED => Self::Cancelled { index, at },
            TAG_PAID_AT_ONCE => Self::PaidAtOnce {
                index,
                at,
                at_risk_sats: r.u64_le()?,
                label: label_from_u8(r.u8()?)
                    .ok_or(DecodeError::NonCanonical("a label is not known"))?,
                mode: mode_from_u8(r.u8()?)
                    .ok_or(DecodeError::NonCanonical("a check mode is not known"))?,
            },
            TAG_CONFIRMED => Self::Confirmed {
                index,
                at,
                received_sats: r.u64_le()?,
                depth: r.u32_le()?,
            },
            TAG_FINAL => Self::Final {
                index,
                at,
                received_sats: r.u64_le()?,
                depth: r.u32_le()?,
            },
            TAG_LATE_PAYMENT => Self::LatePayment {
                index,
                at,
                txid: r.array::<32>()?,
                sats: r.u64_le()?,
            },
            TAG_SURPLUS_OWED => Self::SurplusOwed {
                index,
                at,
                sats: r.u64_le()?,
            },
            TAG_DUST => Self::Dust {
                index,
                at,
                sats: r.u64_le()?,
            },
            TAG_OVERRIDE => {
                let role = Role::from_u8(r.u8()?)
                    .ok_or(DecodeError::NonCanonical("a role is not known"))?;
                let len = usize::from(r.u8()?);
                if len == 0 || len > MAX_NAME_BYTES {
                    return Err(DecodeError::NonCanonical("a name has a bad length"));
                }
                let name = std::str::from_utf8(r.take(len)?)
                    .map_err(|_| DecodeError::NonCanonical("a name is not text"))?
                    .to_string();
                Self::Override {
                    index,
                    at,
                    role,
                    name,
                    sats: r.u64_le()?,
                }
            }
            TAG_REPLACED => Self::Replaced { index, at },
            TAG_REOPENED => Self::Reopened { index, at },
            TAG_POS_PAID => Self::PosPaid { index, at },
            TAG_POS_UNPAID => Self::PosUnpaid { index, at },
            _ => return Err(DecodeError::NonCanonical("an event tag is not known")),
        };
        r.finish()?;
        Ok(event)
    }
}
