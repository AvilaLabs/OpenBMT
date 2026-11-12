//! The body of every record the counter writes (CR-32).
//!
//! A record body is one tag byte, the sale's receive index (u32 LE), the time
//! (u64 LE), then the fields of the event. The decoder is strict: it refuses
//! an unknown tag, a field that is not canonical and any byte left over, and
//! re-encoding a decoded event gives the input.

use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::records::RecordKind;
use openbmt_core::wire::{self, DecodeError, Reader};

use bitcoin::ScriptBuf;

use crate::desk::Role;
use crate::money::{label_is_valid, Amount, Currency, RateSource};
use crate::refund::{is_payment_script, refund_text_ok, Ask, MAX_SCRIPT_BYTES};
use crate::{MAX_FINAL_PAYMENTS, MAX_NAME_BYTES};

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
const TAG_REQUOTED: u8 = 15;
const TAG_REFUND_REQUESTED: u8 = 16;
const TAG_REFUND_WITHDRAWN: u8 = 17;
const TAG_REFUND_PAID_IN_CASH: u8 = 18;
const TAG_PAID_BY_LIGHTNING: u8 = 19;
/// An `Opened` record of a sale with a tip (CR-189). A sale without one is
/// written with [`TAG_OPENED`], byte for byte as before.
const TAG_OPENED_WITH_TIP: u8 = 20;
const TAG_LATE_LIGHTNING: u8 = 21;
// CR-120: the two refund records that carry the chain height. A record
// without a known height keeps its earlier tag and bytes.
const TAG_REFUND_REQUESTED_AT_HEIGHT: u8 = 22;
const TAG_REFUND_PAID_IN_CASH_AT_HEIGHT: u8 = 23;
/// A note on a sale (CR-206).
const TAG_NOTED: u8 = 24;

/// Something that happened to a sale and is written down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A sale opened (record kind `Sale`).
    Opened {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The amount of the sale itself in the shop's currency, without the
        /// tip.
        amount: Amount,
        /// The tip the customer chose, in minor units; 0 for none (CR-189).
        /// The first quote asked for the amount and the tip together.
        tip: Amount,
        /// The currency.
        currency: Currency,
        /// The satoshis the first quote asked for.
        needs_sats: u64,
        /// The first quote's rate, in minor units per bitcoin.
        minor_per_btc: u64,
        /// Where the first quote's rate came from (CR-57).
        source: RateSource,
        /// Who opened the sale, when the caller named a person (CR-151). A
        /// record from before the till lock has none.
        by: Option<(Role, String)>,
    },
    /// The sale was given a new quote (CR-59; kind `Sale`). Written by
    /// `Counter::requote` before the sale is stored: a stop between the two
    /// leaves this record for a quote the sale does not hold, and the caller
    /// is told the requote failed.
    Requoted {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The satoshis the whole amount needs at the new rate.
        needs_sats: u64,
        /// The new quote's rate, in minor units per bitcoin.
        minor_per_btc: u64,
        /// Where the new quote's rate came from (CR-57).
        source: RateSource,
    },
    /// The sale was given a note, when it opened or later (CR-206; kind
    /// `Sale`). The note is the owner's and the books': nothing of it goes to
    /// a POS reader.
    Noted {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The note: plain text of 1 to 64 characters.
        note: String,
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
        /// Who cancelled the sale, when the caller named a person (CR-151).
        by: Option<(Role, String)>,
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
        /// The earliest time any standing payment was first seen; for a
        /// payment only ever seen in a block, the time the counter learned
        /// of the block (CR-32).
        first_seen_at: u64,
        /// What the received satoshis are worth in the shop's currency, by
        /// the sale's own quotes (CR-06, CR-32).
        valued: Amount,
        /// The standing transactions that pay the sale and what each pays,
        /// ascending by transaction ID: at most [`MAX_FINAL_PAYMENTS`], and
        /// when there were more, the largest by amount.
        payments: Vec<([u8; 32], u64)>,
        /// There were more standing transactions than `payments` holds.
        more: bool,
    },
    /// The sale was paid over Lightning, to the invoice the shop's own node
    /// made for it (CR-109; kind `Payment`). A Lightning payment is final at
    /// once: the books count the sale as money from this record (CR-31).
    PaidByLightning {
        /// The receive index.
        index: u32,
        /// When the counter learned of it.
        at: u64,
        /// The payment hash of the invoice.
        payment_hash: [u8; 32],
        /// What the node received, in millisatoshis.
        amount_msat: u64,
        /// When the node says it settled, in seconds since 1970, if it says.
        settled_at: Option<u64>,
        /// What the satoshis received are worth in the shop's currency, by
        /// the sale's own quote in force at `at` (CR-06).
        valued: Amount,
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
    /// An invoice of a sale that had ended, or was paid another way or over
    /// Lightning by another invoice, was settled at the shop's node (CR-19,
    /// CR-117; kind `Payment`).
    LateLightning {
        /// The receive index.
        index: u32,
        /// When the counter learned of it.
        at: u64,
        /// The payment hash of the invoice.
        payment_hash: [u8; 32],
        /// What the node received, in millisatoshis.
        amount_msat: u64,
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
    /// The cashier asked for a refund (CR-60, CR-61; kind `Refund`). Written
    /// by `Counter::request_refund` before the sale is stored (CR-73).
    RefundRequested {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The request number: rising per counter from 1.
        number: u64,
        /// Cashier or manager.
        role: Role,
        /// Who.
        name: String,
        /// Why, as the person typed it.
        reason: String,
        /// Where the money goes: a script of a kind the signer pays.
        destination: ScriptBuf,
        /// What the counter saw received for the sale, in satoshis.
        received_sats: u64,
        /// The sale had not reached the refund depth (CR-66).
        waiting_for_depth: bool,
        /// What is asked for, in satoshis.
        sats: u64,
        /// How the amount was given (CR-61).
        ask: Ask,
        /// The chain height the counter knew when it wrote the record
        /// (CR-120); none when it did not know one.
        chain_height: Option<u32>,
    },
    /// A request was withdrawn (CR-70; kind `Refund`). The owner honours it
    /// only if the refund is still queued and not in an open session.
    RefundWithdrawn {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// The number of the request.
        number: u64,
        /// Cashier or manager.
        role: Role,
        /// Who.
        name: String,
    },
    /// A refund was paid in cash from the till (CR-71; kind `Refund`). It
    /// asks the owner for nothing.
    RefundPaidInCash {
        /// The receive index.
        index: u32,
        /// When.
        at: u64,
        /// Cashier or manager.
        role: Role,
        /// Who.
        name: String,
        /// Why, as the person typed it.
        reason: String,
        /// The amount in the shop's currency, in minor units.
        minor: u64,
        /// What that is in satoshis at the sale's last quote, rounded up.
        sats: u64,
        /// The chain height the counter knew when it wrote the record
        /// (CR-120); none when it did not know one.
        chain_height: Option<u32>,
    },
}

/// CR-120: the chain height at the end of a refund record, read only when
/// the tag says it is there. A height of zero is not a height.
fn read_chain_height(r: &mut Reader<'_>, present: bool) -> Result<Option<u32>, DecodeError> {
    if !present {
        return Ok(None);
    }
    match r.u32_le()? {
        0 => Err(DecodeError::NonCanonical("a chain height is zero")),
        h => Ok(Some(h)),
    }
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
        CheckMode::OwnNodeOnly => 4,
    }
}

/// Read a check mode byte.
pub(crate) fn mode_from_u8(b: u8) -> Option<CheckMode> {
    match b {
        1 => Some(CheckMode::Light),
        2 => Some(CheckMode::NodeAlsoSeenByLight),
        3 => Some(CheckMode::BoxReportsOnly),
        4 => Some(CheckMode::OwnNodeOnly),
        _ => None,
    }
}

/// A rate source as written: label length (u8), the label, the time (u64 LE).
fn write_source(out: &mut Vec<u8>, source: &RateSource) {
    let label = source.label().as_bytes();
    out.push(u8::try_from(label.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(label);
    out.extend_from_slice(&source.fetched_at().to_le_bytes());
}

fn read_source(r: &mut Reader<'_>) -> Result<RateSource, DecodeError> {
    let len = usize::from(r.u8()?);
    let label = r.take(len)?;
    if !label_is_valid(label) {
        return Err(DecodeError::NonCanonical(
            "a rate source label is not valid",
        ));
    }
    let label = std::str::from_utf8(label)
        .map_err(|_| DecodeError::NonCanonical("a rate source label is not text"))?;
    let fetched_at = r.u64_le()?;
    RateSource::new(label, fetched_at)
        .map_err(|_| DecodeError::NonCanonical("a rate source label is not valid"))
}

const ASK_SATS: u8 = 1;
const ASK_VALUE: u8 = 2;

/// A person as written: role (u8), then the name as a text.
fn write_person(out: &mut Vec<u8>, role: Role, name: &str) {
    out.push(role as u8);
    write_text(out, name);
}

/// A text as written: length (u8), the bytes.
fn write_text(out: &mut Vec<u8>, text: &str) {
    out.push(u8::try_from(text.len()).unwrap_or(u8::MAX));
    out.extend_from_slice(text.as_bytes());
}

fn read_text(r: &mut Reader<'_>, what: &'static str) -> Result<String, DecodeError> {
    let len = usize::from(r.u8()?);
    let text = std::str::from_utf8(r.take(len)?)
        .map_err(|_| DecodeError::NonCanonical("a text is not text"))?
        .to_string();
    if !refund_text_ok(&text) {
        return Err(DecodeError::NonCanonical(what));
    }
    Ok(text)
}

/// The person who did a step, written after the fixed fields only when there
/// is one (CR-151): nothing, or role (u8) and name.
fn write_by(out: &mut Vec<u8>, by: &Option<(Role, String)>) {
    if let Some((role, name)) = by {
        write_person(out, *role, name);
    }
}

/// The person who did a step, if the body goes on past its fixed fields. A
/// body that ends there has none.
fn read_by(r: &mut Reader<'_>) -> Result<Option<(Role, String)>, DecodeError> {
    let role = match r.u8() {
        Ok(b) => b,
        Err(DecodeError::Truncated) => return Ok(None),
        Err(e) => return Err(e),
    };
    let role = Role::from_u8(role).ok_or(DecodeError::NonCanonical("a role is not known"))?;
    let name = read_text(r, "a name is not a label")?;
    Ok(Some((role, name)))
}

fn read_person(r: &mut Reader<'_>) -> Result<(Role, String), DecodeError> {
    let role = Role::from_u8(r.u8()?).ok_or(DecodeError::NonCanonical("a role is not known"))?;
    let name = read_text(r, "a name is not a label")?;
    Ok((role, name))
}

/// CR-32: the transactions a `Final` record names, from the standing
/// transactions the watcher gave: at most [`MAX_FINAL_PAYMENTS`], the largest
/// by amount (the smaller transaction ID first among equal amounts), put in
/// ascending order of ID; and whether any were left out.
pub(crate) fn final_payments(standing: &[([u8; 32], u64)]) -> (Vec<([u8; 32], u64)>, bool) {
    let mut all = standing.to_vec();
    let more = all.len() > MAX_FINAL_PAYMENTS;
    if more {
        all.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        all.truncate(MAX_FINAL_PAYMENTS);
    }
    all.sort_unstable_by_key(|p| p.0);
    (all, more)
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
            | Self::PaidByLightning { index, .. }
            | Self::LatePayment { index, .. }
            | Self::LateLightning { index, .. }
            | Self::SurplusOwed { index, .. }
            | Self::Dust { index, .. }
            | Self::Requoted { index, .. }
            | Self::Noted { index, .. }
            | Self::Override { index, .. }
            | Self::Replaced { index, .. }
            | Self::Reopened { index, .. }
            | Self::PosPaid { index, .. }
            | Self::PosUnpaid { index, .. }
            | Self::RefundRequested { index, .. }
            | Self::RefundWithdrawn { index, .. }
            | Self::RefundPaidInCash { index, .. } => *index,
        }
    }

    /// When the event happened, as the record says.
    pub fn at(&self) -> u64 {
        match self {
            Self::Opened { at, .. }
            | Self::Expired { at, .. }
            | Self::Cancelled { at, .. }
            | Self::PaidAtOnce { at, .. }
            | Self::Confirmed { at, .. }
            | Self::Final { at, .. }
            | Self::PaidByLightning { at, .. }
            | Self::LatePayment { at, .. }
            | Self::LateLightning { at, .. }
            | Self::SurplusOwed { at, .. }
            | Self::Dust { at, .. }
            | Self::Requoted { at, .. }
            | Self::Noted { at, .. }
            | Self::Override { at, .. }
            | Self::Replaced { at, .. }
            | Self::Reopened { at, .. }
            | Self::PosPaid { at, .. }
            | Self::PosUnpaid { at, .. }
            | Self::RefundRequested { at, .. }
            | Self::RefundWithdrawn { at, .. }
            | Self::RefundPaidInCash { at, .. } => *at,
        }
    }

    /// The kind of record the event is written as.
    pub fn kind(&self) -> RecordKind {
        match self {
            Self::Opened { .. }
            | Self::Requoted { .. }
            | Self::Noted { .. }
            | Self::Expired { .. }
            | Self::Cancelled { .. } => RecordKind::Sale,
            Self::PaidAtOnce { .. }
            | Self::Confirmed { .. }
            | Self::Final { .. }
            | Self::PaidByLightning { .. }
            | Self::LatePayment { .. }
            | Self::LateLightning { .. }
            | Self::SurplusOwed { .. }
            | Self::Dust { .. }
            | Self::PosPaid { .. }
            | Self::PosUnpaid { .. } => RecordKind::Payment,
            Self::Override { .. } => RecordKind::Override,
            Self::Replaced { .. } | Self::Reopened { .. } => RecordKind::Conflict,
            Self::RefundRequested { .. }
            | Self::RefundWithdrawn { .. }
            | Self::RefundPaidInCash { .. } => RecordKind::Refund,
        }
    }

    fn head(&self) -> (u8, u32, u64) {
        match self {
            Self::Opened { index, at, tip, .. } => (
                if tip.0 == 0 {
                    TAG_OPENED
                } else {
                    TAG_OPENED_WITH_TIP
                },
                *index,
                *at,
            ),
            Self::Requoted { index, at, .. } => (TAG_REQUOTED, *index, *at),
            Self::Noted { index, at, .. } => (TAG_NOTED, *index, *at),
            Self::Expired { index, at } => (TAG_EXPIRED, *index, *at),
            Self::Cancelled { index, at, .. } => (TAG_CANCELLED, *index, *at),
            Self::PaidAtOnce { index, at, .. } => (TAG_PAID_AT_ONCE, *index, *at),
            Self::Confirmed { index, at, .. } => (TAG_CONFIRMED, *index, *at),
            Self::Final { index, at, .. } => (TAG_FINAL, *index, *at),
            Self::PaidByLightning { index, at, .. } => (TAG_PAID_BY_LIGHTNING, *index, *at),
            Self::LatePayment { index, at, .. } => (TAG_LATE_PAYMENT, *index, *at),
            Self::LateLightning { index, at, .. } => (TAG_LATE_LIGHTNING, *index, *at),
            Self::SurplusOwed { index, at, .. } => (TAG_SURPLUS_OWED, *index, *at),
            Self::Dust { index, at, .. } => (TAG_DUST, *index, *at),
            Self::Override { index, at, .. } => (TAG_OVERRIDE, *index, *at),
            Self::Replaced { index, at } => (TAG_REPLACED, *index, *at),
            Self::Reopened { index, at } => (TAG_REOPENED, *index, *at),
            Self::PosPaid { index, at } => (TAG_POS_PAID, *index, *at),
            Self::PosUnpaid { index, at } => (TAG_POS_UNPAID, *index, *at),
            Self::RefundRequested {
                index,
                at,
                chain_height,
                ..
            } => (
                if chain_height.is_some() {
                    TAG_REFUND_REQUESTED_AT_HEIGHT
                } else {
                    TAG_REFUND_REQUESTED
                },
                *index,
                *at,
            ),
            Self::RefundWithdrawn { index, at, .. } => (TAG_REFUND_WITHDRAWN, *index, *at),
            Self::RefundPaidInCash {
                index,
                at,
                chain_height,
                ..
            } => (
                if chain_height.is_some() {
                    TAG_REFUND_PAID_IN_CASH_AT_HEIGHT
                } else {
                    TAG_REFUND_PAID_IN_CASH
                },
                *index,
                *at,
            ),
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
                tip,
                currency,
                needs_sats,
                minor_per_btc,
                source,
                by,
                ..
            } => {
                out.extend_from_slice(&amount.0.to_le_bytes());
                out.extend_from_slice(&currency.code());
                out.extend_from_slice(&needs_sats.to_le_bytes());
                out.extend_from_slice(&minor_per_btc.to_le_bytes());
                write_source(&mut out, source);
                if tip.0 != 0 {
                    out.extend_from_slice(&tip.0.to_le_bytes());
                }
                write_by(&mut out, by);
            }
            Self::Cancelled { by, .. } => write_by(&mut out, by),
            Self::Requoted {
                needs_sats,
                minor_per_btc,
                source,
                ..
            } => {
                out.extend_from_slice(&needs_sats.to_le_bytes());
                out.extend_from_slice(&minor_per_btc.to_le_bytes());
                write_source(&mut out, source);
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
            } => {
                out.extend_from_slice(&received_sats.to_le_bytes());
                out.extend_from_slice(&depth.to_le_bytes());
            }
            Self::Final {
                received_sats,
                depth,
                first_seen_at,
                valued,
                payments,
                more,
                ..
            } => {
                out.extend_from_slice(&received_sats.to_le_bytes());
                out.extend_from_slice(&depth.to_le_bytes());
                out.extend_from_slice(&first_seen_at.to_le_bytes());
                out.extend_from_slice(&valued.0.to_le_bytes());
                out.push(u8::from(*more));
                out.push(u8::try_from(payments.len()).unwrap_or(u8::MAX));
                for (txid, sats) in payments {
                    out.extend_from_slice(txid);
                    out.extend_from_slice(&sats.to_le_bytes());
                }
            }
            Self::PaidByLightning {
                payment_hash,
                amount_msat,
                settled_at,
                valued,
                ..
            } => {
                out.extend_from_slice(payment_hash);
                out.extend_from_slice(&amount_msat.to_le_bytes());
                wire::put_opt_u64(&mut out, *settled_at);
                out.extend_from_slice(&valued.0.to_le_bytes());
            }
            Self::Noted { note, .. } => write_text(&mut out, note),
            Self::LatePayment { txid, sats, .. } => {
                out.extend_from_slice(txid);
                out.extend_from_slice(&sats.to_le_bytes());
            }
            Self::LateLightning {
                payment_hash,
                amount_msat,
                ..
            } => {
                out.extend_from_slice(payment_hash);
                out.extend_from_slice(&amount_msat.to_le_bytes());
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
            Self::RefundRequested {
                number,
                role,
                name,
                reason,
                destination,
                received_sats,
                waiting_for_depth,
                sats,
                ask,
                chain_height,
                ..
            } => {
                out.extend_from_slice(&number.to_le_bytes());
                write_person(&mut out, *role, name);
                write_text(&mut out, reason);
                let script = destination.as_bytes();
                out.push(u8::try_from(script.len()).unwrap_or(u8::MAX));
                out.extend_from_slice(script);
                out.extend_from_slice(&received_sats.to_le_bytes());
                out.push(u8::from(*waiting_for_depth));
                out.extend_from_slice(&sats.to_le_bytes());
                match ask {
                    Ask::Sats => out.push(ASK_SATS),
                    Ask::Value {
                        minor,
                        minor_per_btc,
                        source,
                    } => {
                        out.push(ASK_VALUE);
                        out.extend_from_slice(&minor.to_le_bytes());
                        out.extend_from_slice(&minor_per_btc.to_le_bytes());
                        write_source(&mut out, source);
                    }
                }
                if let Some(height) = chain_height {
                    out.extend_from_slice(&height.to_le_bytes());
                }
            }
            Self::RefundWithdrawn {
                number, role, name, ..
            } => {
                out.extend_from_slice(&number.to_le_bytes());
                write_person(&mut out, *role, name);
            }
            Self::RefundPaidInCash {
                role,
                name,
                reason,
                minor,
                sats,
                chain_height,
                ..
            } => {
                write_person(&mut out, *role, name);
                write_text(&mut out, reason);
                out.extend_from_slice(&minor.to_le_bytes());
                out.extend_from_slice(&sats.to_le_bytes());
                if let Some(height) = chain_height {
                    out.extend_from_slice(&height.to_le_bytes());
                }
            }
            Self::Expired { .. }
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
            TAG_OPENED | TAG_OPENED_WITH_TIP => {
                let amount = Amount(r.u64_le()?);
                let currency = Currency::new(r.array::<3>()?)
                    .ok_or(DecodeError::NonCanonical("a currency code is not valid"))?;
                let needs_sats = r.u64_le()?;
                let minor_per_btc = r.u64_le()?;
                let source = read_source(&mut r)?;
                // A record from before tips, and one of a sale without a
                // tip, hold none: the tip of 0 (CR-189).
                let tip = if tag == TAG_OPENED_WITH_TIP {
                    let tip = r.u64_le()?;
                    if tip == 0 {
                        return Err(DecodeError::NonCanonical(
                            "a record of a sale with a tip holds a tip of nothing",
                        ));
                    }
                    Amount(tip)
                } else {
                    Amount(0)
                };
                Self::Opened {
                    index,
                    at,
                    amount,
                    tip,
                    currency,
                    needs_sats,
                    minor_per_btc,
                    source,
                    by: read_by(&mut r)?,
                }
            }
            TAG_REQUOTED => Self::Requoted {
                index,
                at,
                needs_sats: r.u64_le()?,
                minor_per_btc: r.u64_le()?,
                source: read_source(&mut r)?,
            },
            TAG_EXPIRED => Self::Expired { index, at },
            TAG_CANCELLED => Self::Cancelled {
                index,
                at,
                by: read_by(&mut r)?,
            },
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
            TAG_FINAL => {
                let received_sats = r.u64_le()?;
                let depth = r.u32_le()?;
                let first_seen_at = r.u64_le()?;
                let valued = Amount(r.u64_le()?);
                let more = r.flag()?;
                let n = usize::from(r.u8()?);
                if n > MAX_FINAL_PAYMENTS {
                    return Err(DecodeError::NonCanonical(
                        "a final record has too many payments",
                    ));
                }
                if more && n < MAX_FINAL_PAYMENTS {
                    return Err(DecodeError::NonCanonical(
                        "a final record says there are more payments but holds fewer than the most",
                    ));
                }
                let mut payments = Vec::with_capacity(n);
                let mut last: Option<[u8; 32]> = None;
                for _ in 0..n {
                    let txid = r.array::<32>()?;
                    if last.is_some_and(|prev| prev >= txid) {
                        return Err(DecodeError::NonCanonical(
                            "payments are not in ascending order of transaction ID",
                        ));
                    }
                    last = Some(txid);
                    payments.push((txid, r.u64_le()?));
                }
                Self::Final {
                    index,
                    at,
                    received_sats,
                    depth,
                    first_seen_at,
                    valued,
                    payments,
                    more,
                }
            }
            TAG_PAID_BY_LIGHTNING => {
                let payment_hash = r.array::<32>()?;
                let amount_msat = r.u64_le()?;
                if amount_msat == 0 {
                    return Err(DecodeError::NonCanonical(
                        "a Lightning payment is for nothing",
                    ));
                }
                Self::PaidByLightning {
                    index,
                    at,
                    payment_hash,
                    amount_msat,
                    settled_at: r.opt_u64()?,
                    valued: Amount(r.u64_le()?),
                }
            }
            TAG_NOTED => Self::Noted {
                index,
                at,
                note: read_text(&mut r, "a note is not plain text of 1 to 64 characters")?,
            },
            TAG_LATE_PAYMENT => Self::LatePayment {
                index,
                at,
                txid: r.array::<32>()?,
                sats: r.u64_le()?,
            },
            TAG_LATE_LIGHTNING => {
                let payment_hash = r.array::<32>()?;
                let amount_msat = r.u64_le()?;
                if amount_msat == 0 {
                    return Err(DecodeError::NonCanonical(
                        "a late Lightning payment is for nothing",
                    ));
                }
                Self::LateLightning {
                    index,
                    at,
                    payment_hash,
                    amount_msat,
                }
            }
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
            TAG_REFUND_REQUESTED | TAG_REFUND_REQUESTED_AT_HEIGHT => {
                let number = r.u64_le()?;
                if number == 0 {
                    return Err(DecodeError::NonCanonical("a request number is zero"));
                }
                let (role, name) = read_person(&mut r)?;
                let reason = read_text(&mut r, "a reason is not a label")?;
                let script_len = usize::from(r.u8()?);
                if script_len == 0 || script_len > MAX_SCRIPT_BYTES {
                    return Err(DecodeError::NonCanonical("a destination has a bad length"));
                }
                let destination = ScriptBuf::from_bytes(r.take(script_len)?.to_vec());
                if !is_payment_script(&destination) {
                    return Err(DecodeError::NonCanonical(
                        "a destination is not a script the signer pays",
                    ));
                }
                let received_sats = r.u64_le()?;
                let waiting_for_depth = r.flag()?;
                let sats = r.u64_le()?;
                if sats == 0 || sats > received_sats {
                    return Err(DecodeError::NonCanonical(
                        "a request asks for nothing or for more than was received",
                    ));
                }
                let ask = match r.u8()? {
                    ASK_SATS => Ask::Sats,
                    ASK_VALUE => {
                        let minor = r.u64_le()?;
                        let minor_per_btc = r.u64_le()?;
                        if minor == 0 || minor_per_btc == 0 {
                            return Err(DecodeError::NonCanonical(
                                "a value or a rate in a request is zero",
                            ));
                        }
                        Ask::Value {
                            minor,
                            minor_per_btc,
                            source: read_source(&mut r)?,
                        }
                    }
                    _ => return Err(DecodeError::NonCanonical("an ask is not known")),
                };
                let chain_height =
                    read_chain_height(&mut r, tag == TAG_REFUND_REQUESTED_AT_HEIGHT)?;
                Self::RefundRequested {
                    index,
                    at,
                    number,
                    role,
                    name,
                    reason,
                    destination,
                    received_sats,
                    waiting_for_depth,
                    sats,
                    ask,
                    chain_height,
                }
            }
            TAG_REFUND_WITHDRAWN => {
                let number = r.u64_le()?;
                if number == 0 {
                    return Err(DecodeError::NonCanonical("a request number is zero"));
                }
                let (role, name) = read_person(&mut r)?;
                Self::RefundWithdrawn {
                    index,
                    at,
                    number,
                    role,
                    name,
                }
            }
            TAG_REFUND_PAID_IN_CASH | TAG_REFUND_PAID_IN_CASH_AT_HEIGHT => {
                let (role, name) = read_person(&mut r)?;
                let reason = read_text(&mut r, "a reason is not a label")?;
                let minor = r.u64_le()?;
                let sats = r.u64_le()?;
                if minor == 0 || sats == 0 {
                    return Err(DecodeError::NonCanonical("a cash refund is for nothing"));
                }
                let chain_height =
                    read_chain_height(&mut r, tag == TAG_REFUND_PAID_IN_CASH_AT_HEIGHT)?;
                Self::RefundPaidInCash {
                    index,
                    at,
                    role,
                    name,
                    reason,
                    minor,
                    sats,
                    chain_height,
                }
            }
            _ => return Err(DecodeError::NonCanonical("an event tag is not known")),
        };
        r.finish()?;
        Ok(event)
    }
}
