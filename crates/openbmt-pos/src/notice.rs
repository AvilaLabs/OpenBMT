//! What the counter tells the Box and what the Box tells the owner
//! (BX-40, BX-42): the paid notice and the POS notice, and the sale facts
//! that come from the first.
//!
//! A [`PaidNotice`] is signed by a counter and says that one sale is in one
//! of four states. It carries the amount and the currency in the shop's
//! money and times, and never an amount in satoshis, a rate, an address or a
//! transaction ID. [`facts_from_notices`] builds the sale facts of PS-05 from
//! notices alone, and [`NoticeMaker`] is what a counter runs over its own
//! events to know which notices to send, so that the two ends fold the same
//! way.
//!
//! A [`PosNotice`] is signed by the Box and carries one thing it found: a
//! match, an alert, an order it closed, or a sale that went unpaid after its
//! order was closed. The owner shows it as the Box's report and never merges
//! it into its own matching.

use std::collections::BTreeMap;
use std::fmt;

use openbmt_core::identity::{DeviceId, Identity, Sig};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::{Amount, Currency, Event};

use crate::alerts::{Alert, TenderKey};
use crate::facts::{FactFold, FactsError, SaleFact, SaleKey};
use crate::ledger::{get_alert, get_sale, put_alert, put_sale};
use crate::matcher::{Match, MatchHow};
use crate::model::{get_id, put_str, OrderId};

/// The magic of a paid notice.
pub const MAGIC_PAID_NOTICE: [u8; 4] = *b"OBMc";
/// The magic of a POS notice.
pub const MAGIC_POS_NOTICE: [u8; 4] = *b"OBMd";
/// The tag of the hash a paid notice's signature covers.
pub const TAG_PAID_NOTICE: &str = "OpenBMT/paid-notice/v1";
/// The tag of the hash a POS notice's signature covers.
pub const TAG_POS_NOTICE: &str = "OpenBMT/pos-notice/v1";
/// The length of every paid notice: magic and version (5), counter key
/// (32), number (8), sale index (4), amount (8), currency (3), opened (8),
/// state (1), state time (8) and signature (64). Nothing else is in it
/// (BX-40).
pub const PAID_NOTICE_LEN: usize = 141;

/// The four things a paid notice says about a sale (BX-40).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoticeState {
    /// The sale opened.
    Opened,
    /// The counter reported the sale paid. The time is when it was first
    /// reported paid.
    Paid,
    /// The counter reported the sale unpaid after it was reported paid.
    Unpaid,
    /// The sale is final (CR-31).
    Final,
}

impl NoticeState {
    /// The byte the state is sent as.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Opened => 1,
            Self::Paid => 2,
            Self::Unpaid => 3,
            Self::Final => 4,
        }
    }

    /// Read a state byte.
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::Opened,
            2 => Self::Paid,
            3 => Self::Unpaid,
            4 => Self::Final,
            _ => return None,
        })
    }

    /// The word for a screen or a note.
    pub fn name(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Paid => "paid",
            Self::Unpaid => "unpaid again",
            Self::Final => "final",
        }
    }
}

/// A counter's signed statement about one sale (BX-40).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidNotice {
    /// The counter that signed it.
    pub counter: DeviceId,
    /// The notice number: from 1, strictly rising for each counter.
    pub number: u64,
    /// The sale index.
    pub index: u32,
    /// The sale's amount in minor units of the currency.
    pub amount: Amount,
    /// The currency.
    pub currency: Currency,
    /// When the sale opened.
    pub opened_at: u64,
    /// The state.
    pub state: NoticeState,
    /// When the state was reached; for "paid", when the sale was first
    /// reported paid.
    pub state_at: u64,
    /// The counter's signature over [`PaidNotice::signing_bytes`].
    pub sig: Sig,
}

/// The sale fields of a draft, before it has a number and a signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoticeDraft {
    /// The sale index.
    pub index: u32,
    /// The amount in minor units.
    pub amount: Amount,
    /// The currency.
    pub currency: Currency,
    /// When the sale opened.
    pub opened_at: u64,
    /// The state.
    pub state: NoticeState,
    /// When the state was reached.
    pub state_at: u64,
}

impl PaidNotice {
    /// Sign a notice with number `number`, which must be at least 1.
    pub fn sign(identity: &Identity, number: u64, draft: &NoticeDraft) -> Option<Self> {
        if number == 0 {
            return None;
        }
        let mut notice = Self {
            counter: identity.device_id(),
            number,
            index: draft.index,
            amount: draft.amount,
            currency: draft.currency,
            opened_at: draft.opened_at,
            state: draft.state,
            state_at: draft.state_at,
            sig: Sig([0; 64]),
        };
        notice.sig = identity.sign_tagged(TAG_PAID_NOTICE, &notice.signing_bytes());
        Some(notice)
    }

    /// The bytes the signature covers: everything before it.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_PAID_NOTICE, PAID_NOTICE_LEN);
        out.extend_from_slice(&self.counter.to_bytes());
        out.extend_from_slice(&self.number.to_le_bytes());
        out.extend_from_slice(&self.index.to_le_bytes());
        out.extend_from_slice(&self.amount.0.to_le_bytes());
        out.extend_from_slice(&self.currency.code());
        out.extend_from_slice(&self.opened_at.to_le_bytes());
        out.push(self.state.to_byte());
        out.extend_from_slice(&self.state_at.to_le_bytes());
        out
    }

    /// The notice as sent.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.signing_bytes();
        out.extend_from_slice(&self.sig.0);
        out
    }

    /// Read a notice strictly: the length is exact, the state and the
    /// currency are known and the number is not zero. The signature is not
    /// checked here: see [`PaidNotice::verify`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() > PAID_NOTICE_LEN {
            return Err(DecodeError::TrailingBytes);
        }
        let mut r = Reader::framed(bytes, &MAGIC_PAID_NOTICE)?;
        let counter = r.device_id()?;
        let number = r.u64_le()?;
        if number == 0 {
            return Err(DecodeError::NonCanonical("a notice number is zero"));
        }
        let index = r.u32_le()?;
        let amount = Amount(r.u64_le()?);
        let currency = Currency::new(r.array::<3>()?)
            .ok_or(DecodeError::NonCanonical("a currency code is not valid"))?;
        let opened_at = r.u64_le()?;
        let state = NoticeState::from_byte(r.u8()?)
            .ok_or(DecodeError::NonCanonical("a notice state is not known"))?;
        let state_at = r.u64_le()?;
        let sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            counter,
            number,
            index,
            amount,
            currency,
            opened_at,
            state,
            state_at,
            sig,
        })
    }

    /// Whether the counter's signature is good.
    pub fn verify(&self) -> bool {
        self.counter
            .verify_tagged(TAG_PAID_NOTICE, &self.signing_bytes(), &self.sig)
    }

    /// Which sale the notice is about.
    pub fn sale(&self) -> SaleKey {
        SaleKey {
            counter: self.counter,
            index: self.index,
        }
    }
}

/// What differs between a notice and what earlier notices said about the
/// same sale (BX-40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaleMismatch {
    /// The amount differs.
    Amount,
    /// The currency differs.
    Currency,
    /// The time the sale opened differs.
    OpenedAt,
}

impl fmt::Display for SaleMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self {
            Self::Amount => "amount",
            Self::Currency => "currency",
            Self::OpenedAt => "opening time",
        };
        write!(
            f,
            "BX-40: the notice gives the sale another {what} than earlier notices did"
        )
    }
}

impl std::error::Error for SaleMismatch {}

impl SaleFact {
    /// The fact a first notice about a sale gives.
    pub fn from_notice(notice: &PaidNotice) -> Self {
        let mut fact = Self::opened(
            notice.counter,
            notice.index,
            notice.amount,
            notice.currency,
            notice.opened_at,
        );
        fact.apply_notice(notice);
        fact
    }

    /// Whether a notice agrees with this fact on what a sale never changes:
    /// its amount, its currency and when it opened (BX-40).
    pub fn check_notice(&self, notice: &PaidNotice) -> Result<(), SaleMismatch> {
        if self.amount != notice.amount {
            Err(SaleMismatch::Amount)
        } else if self.currency != notice.currency {
            Err(SaleMismatch::Currency)
        } else if self.opened_at != notice.opened_at {
            Err(SaleMismatch::OpenedAt)
        } else {
            Ok(())
        }
    }

    /// Take the state a notice gives (BX-40). The caller has checked the
    /// notice with [`SaleFact::check_notice`]. A notice is the state
    /// itself: "paid" sets the time first reported paid and clears the two
    /// flags, "unpaid again" and "final" set theirs and clear the other.
    pub fn apply_notice(&mut self, notice: &PaidNotice) {
        match notice.state {
            NoticeState::Opened => {}
            NoticeState::Paid => {
                self.paid_at = Some(notice.state_at);
                self.unpaid_after = false;
                self.is_final = false;
            }
            NoticeState::Unpaid => {
                self.unpaid_after = true;
                self.is_final = false;
            }
            NoticeState::Final => {
                self.paid_at.get_or_insert(notice.state_at);
                self.unpaid_after = false;
                self.is_final = true;
            }
        }
    }
}

/// BX-40: the sale facts of PS-05 built from notices alone, ordered by
/// counter and sale index. Notices are taken in the order of their numbers
/// for each counter, whatever order they are given in. A notice that
/// disagrees with an earlier one about a sale's amount, currency or opening
/// time is left out. The signatures are the caller's to have checked.
pub fn facts_from_notices(notices: &[PaidNotice]) -> Vec<SaleFact> {
    let mut sorted: Vec<&PaidNotice> = notices.iter().collect();
    sorted.sort_by_key(|n| (n.counter, n.number));
    let mut facts: BTreeMap<SaleKey, SaleFact> = BTreeMap::new();
    for n in sorted {
        match facts.get_mut(&n.sale()) {
            None => {
                facts.insert(n.sale(), SaleFact::from_notice(n));
            }
            Some(fact) => {
                if fact.check_notice(n).is_ok() {
                    fact.apply_notice(n);
                }
            }
        }
    }
    facts.into_values().collect()
}

/// The time of an event, whatever it is.
fn event_at(event: &Event) -> u64 {
    match event {
        Event::Opened { at, .. }
        | Event::Requoted { at, .. }
        | Event::Noted { at, .. }
        | Event::Expired { at, .. }
        | Event::Cancelled { at, .. }
        | Event::PaidAtOnce { at, .. }
        | Event::Confirmed { at, .. }
        | Event::Final { at, .. }
        | Event::PaidByLightning { at, .. }
        | Event::LatePayment { at, .. }
        | Event::LateLightning { at, .. }
        | Event::SurplusOwed { at, .. }
        | Event::Dust { at, .. }
        | Event::Override { at, .. }
        | Event::Replaced { at, .. }
        | Event::Reopened { at, .. }
        | Event::PosPaid { at, .. }
        | Event::PosUnpaid { at, .. }
        | Event::RefundRequested { at, .. }
        | Event::RefundWithdrawn { at, .. }
        | Event::RefundPaidInCash { at, .. } => *at,
    }
}

/// The state a fact is in as a notice says it. A sale that is unpaid again
/// is that, whatever else it is: the counter clears final when it reopens a
/// sale, so the two never meet in what a counter writes.
fn state_of(fact: &SaleFact) -> NoticeState {
    if fact.unpaid_after {
        NoticeState::Unpaid
    } else if fact.is_final {
        NoticeState::Final
    } else if fact.paid_at.is_some() {
        NoticeState::Paid
    } else {
        NoticeState::Opened
    }
}

/// What a counter runs over its own events, oldest first, to learn which
/// notices to send (BX-40). A notice is made when a sale's state changes, or
/// when the time it was first reported paid changes (a `PosPaid` record
/// after an earlier paid event replaces what was inferred, PS-09). Folding
/// the notices again with [`facts_from_notices`] gives the facts that
/// [`crate::facts_from_chain`] gives for the same events.
#[derive(Debug, Clone)]
pub struct NoticeMaker {
    fold: FactFold,
    sent: BTreeMap<u32, (NoticeState, Option<u64>)>,
}

impl NoticeMaker {
    /// A maker for this counter's events.
    pub fn new(counter: DeviceId) -> Self {
        Self {
            fold: FactFold::new(counter),
            sent: BTreeMap::new(),
        }
    }

    /// Fold one event. Returns the notice it calls for, if any. An event
    /// the fold refuses is refused here and changes nothing.
    pub fn apply(&mut self, event: &Event) -> Result<Option<NoticeDraft>, FactsError> {
        self.fold.apply(event)?;
        let Some(fact) = self.fold.fact(event.index()) else {
            return Ok(None);
        };
        let state = state_of(&fact);
        let paid_time = (state == NoticeState::Paid)
            .then_some(fact.paid_at)
            .flatten();
        if self.sent.get(&fact.index) == Some(&(state, paid_time)) {
            return Ok(None);
        }
        self.sent.insert(fact.index, (state, paid_time));
        let state_at = match state {
            NoticeState::Paid => fact.paid_at.unwrap_or_else(|| event_at(event)),
            NoticeState::Opened => fact.opened_at,
            NoticeState::Unpaid | NoticeState::Final => event_at(event),
        };
        Ok(Some(NoticeDraft {
            index: fact.index,
            amount: fact.amount,
            currency: fact.currency,
            opened_at: fact.opened_at,
            state,
            state_at,
        }))
    }
}

/// What a POS notice carries (BX-42).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosItem {
    /// A tender matched to a sale.
    Match {
        /// The tender.
        tender: TenderKey,
        /// The match.
        matched: Match,
    },
    /// An alert raised, with its why and next step as the alert holds them.
    Alert(Alert),
    /// The Box closed an order for a sale (BX-43).
    Closed {
        /// The sale.
        sale: SaleKey,
        /// The order.
        order: OrderId,
    },
    /// A sale whose order the Box closed was reported unpaid again by the
    /// counter (BX-43). Nothing was undone.
    UnpaidAfterClose {
        /// The sale.
        sale: SaleKey,
        /// The order.
        order: OrderId,
    },
}

/// The Box's signed report of one thing it found (BX-42).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosNotice {
    /// The Box that signed it.
    pub from: DeviceId,
    /// The notice number: from 1, strictly rising for the Box.
    pub number: u64,
    /// The Box's time when the notice was made.
    pub at: u64,
    /// What it carries.
    pub item: PosItem,
    /// The Box's signature over [`PosNotice::signing_bytes`].
    pub sig: Sig,
}

const ITEM_MATCH: u8 = 1;
const ITEM_ALERT: u8 = 2;
const ITEM_CLOSED: u8 = 3;
const ITEM_UNPAID_AFTER_CLOSE: u8 = 4;

impl PosNotice {
    /// Sign a notice with number `number`, which must be at least 1.
    pub fn sign(identity: &Identity, number: u64, at: u64, item: PosItem) -> Option<Self> {
        if number == 0 {
            return None;
        }
        let mut notice = Self {
            from: identity.device_id(),
            number,
            at,
            item,
            sig: Sig([0; 64]),
        };
        notice.sig = identity.sign_tagged(TAG_POS_NOTICE, &notice.signing_bytes());
        Some(notice)
    }

    /// The bytes the signature covers: everything before it.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_POS_NOTICE, 256);
        out.extend_from_slice(&self.from.to_bytes());
        out.extend_from_slice(&self.number.to_le_bytes());
        out.extend_from_slice(&self.at.to_le_bytes());
        match &self.item {
            PosItem::Match { tender, matched } => {
                out.push(ITEM_MATCH);
                put_str(&mut out, tender.order.as_str());
                put_str(&mut out, tender.tender.as_str());
                put_sale(&mut out, &matched.sale);
                out.push(match matched.how {
                    MatchHow::Reference => 1,
                    MatchHow::AmountAndTime => 2,
                });
                out.push(u8::from(matched.by_order));
                out.extend_from_slice(&matched.matched_at.to_le_bytes());
            }
            PosItem::Alert(alert) => {
                out.push(ITEM_ALERT);
                put_alert(&mut out, alert);
            }
            PosItem::Closed { sale, order } => {
                out.push(ITEM_CLOSED);
                put_sale(&mut out, sale);
                put_str(&mut out, order.as_str());
            }
            PosItem::UnpaidAfterClose { sale, order } => {
                out.push(ITEM_UNPAID_AFTER_CLOSE);
                put_sale(&mut out, sale);
                put_str(&mut out, order.as_str());
            }
        }
        out
    }

    /// The notice as sent.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.signing_bytes();
        out.extend_from_slice(&self.sig.0);
        out
    }

    /// Read a notice strictly: every field is in its bounds, the number is
    /// not zero and no byte is left over. The signature is not checked
    /// here: see [`PosNotice::verify`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_POS_NOTICE)?;
        let from = r.device_id()?;
        let number = r.u64_le()?;
        if number == 0 {
            return Err(DecodeError::NonCanonical("a notice number is zero"));
        }
        let at = r.u64_le()?;
        let item = match r.u8()? {
            ITEM_MATCH => {
                let tender = TenderKey {
                    order: get_id(&mut r)?,
                    tender: get_id(&mut r)?,
                };
                let sale = get_sale(&mut r)?;
                let how = match r.u8()? {
                    1 => MatchHow::Reference,
                    2 => MatchHow::AmountAndTime,
                    _ => return Err(DecodeError::NonCanonical("a match kind is not known")),
                };
                let by_order = r.flag()?;
                let matched_at = r.u64_le()?;
                PosItem::Match {
                    tender,
                    matched: Match {
                        sale,
                        how,
                        by_order,
                        matched_at,
                    },
                }
            }
            ITEM_ALERT => PosItem::Alert(get_alert(&mut r, None)?),
            ITEM_CLOSED => PosItem::Closed {
                sale: get_sale(&mut r)?,
                order: get_id(&mut r)?,
            },
            ITEM_UNPAID_AFTER_CLOSE => PosItem::UnpaidAfterClose {
                sale: get_sale(&mut r)?,
                order: get_id(&mut r)?,
            },
            _ => return Err(DecodeError::NonCanonical("a notice item is not known")),
        };
        let sig = r.sig()?;
        r.finish()?;
        Ok(Self {
            from,
            number,
            at,
            item,
            sig,
        })
    }

    /// Whether the Box's signature is good.
    pub fn verify(&self) -> bool {
        self.from
            .verify_tagged(TAG_POS_NOTICE, &self.signing_bytes(), &self.sig)
    }

    /// One line in plain words, for a list. It holds no amount in satoshis,
    /// no address and no transaction ID.
    pub fn text(&self) -> String {
        match &self.item {
            PosItem::Match { tender, matched } => format!(
                "order {} tender {} matches sale {}{}",
                tender.order,
                tender.tender,
                matched.sale.index,
                if matched.by_order {
                    " (by order of time)"
                } else {
                    ""
                }
            ),
            PosItem::Alert(alert) => format!("{}: {}", alert.key.kind.name(), alert.why()),
            PosItem::Closed { sale, order } => {
                format!("order {order} was closed for sale {}", sale.index)
            }
            PosItem::UnpaidAfterClose { sale, order } => format!(
                "sale {} was reported unpaid again after order {order} was closed for it",
                sale.index
            ),
        }
    }
}
