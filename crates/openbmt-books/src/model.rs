//! The model: sales, disposals, returns and what the caller supplies.

use std::collections::BTreeMap;
use std::fmt;

use openbmt_core::identity::{tagged_hash, DeviceId};
use openbmt_counter::{BillEnd, BillKind, Currency, PartWay, RateSource, Role, StaffChange};

use crate::value::Value;

/// BK-07: the tag of the opaque sale ID.
pub const TAG_BOOKS_SALE: &str = "OpenBMT/books-sale/v1";

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Which sale: the counter's key and the sale index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SaleKey {
    /// The counter.
    pub counter: DeviceId,
    /// The sale index.
    pub index: u32,
}

/// BK-07: the opaque sale ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SaleId([u8; 8]);

impl SaleId {
    /// The ID of a sale.
    pub fn for_sale(key: &SaleKey) -> Self {
        let mut msg = Vec::with_capacity(36);
        msg.extend_from_slice(&key.counter.to_bytes());
        msg.extend_from_slice(&key.index.to_le_bytes());
        let digest = tagged_hash(TAG_BOOKS_SALE, &msg);
        let mut out = [0u8; 8];
        for (o, d) in out.iter_mut().zip(digest.iter()) {
            *o = *d;
        }
        Self(out)
    }

    /// The 8 bytes.
    pub fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }
}

impl fmt::Display for SaleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 64 bits and one zero bit: 13 characters of 5 bits.
        let v = u128::from(u64::from_be_bytes(self.0)) << 1;
        for i in 0..13u32 {
            if i == 4 || i == 8 {
                f.write_str("-")?;
            }
            let shift = 5 * (12 - i);
            let idx = usize::try_from((v >> shift) & 31).unwrap_or(0);
            let c = CROCKFORD.get(idx).copied().unwrap_or(b'0');
            write!(f, "{}", char::from(c))?;
        }
        Ok(())
    }
}

/// BK-05: where a sale stands after its last event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaleState {
    /// Opened, nothing else yet.
    Open = 1,
    /// Ended with nothing received.
    Expired = 2,
    /// Cancelled by the cashier.
    Cancelled = 3,
    /// Accepted before a block.
    PaidAtOnce = 4,
    /// Confirmed at the shop's depth.
    Confirmed = 5,
    /// Confirmed at the refund depth.
    Final = 6,
    /// A payment arrived after the sale ended.
    LatePayment = 7,
    /// A conflict replaced the payment.
    Replaced = 8,
    /// A block that confirmed the payment left the chain.
    Reopened = 9,
}

impl SaleState {
    /// Read a stored code.
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            1 => Self::Open,
            2 => Self::Expired,
            3 => Self::Cancelled,
            4 => Self::PaidAtOnce,
            5 => Self::Confirmed,
            6 => Self::Final,
            7 => Self::LatePayment,
            8 => Self::Replaced,
            9 => Self::Reopened,
            _ => return None,
        })
    }

    /// The word for the state.
    pub fn text(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Expired => "expired",
            Self::Cancelled => "cancelled",
            Self::PaidAtOnce => "paid at once",
            Self::Confirmed => "confirmed",
            Self::Final => "final",
            Self::LatePayment => "late payment",
            Self::Replaced => "replaced",
            Self::Reopened => "reopened",
        }
    }
}

/// BK-08: whether a sale is money in the books.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Counting {
    /// Final: counted.
    Counted,
    /// Funds were seen and the sale is not final yet.
    Pending,
    /// No funds: only in the full ledger.
    NoFunds,
}

impl Counting {
    /// The word for it.
    pub fn text(self) -> &'static str {
        match self {
            Self::Counted => "counted",
            Self::Pending => "pending",
            Self::NoFunds => "no funds",
        }
    }
}

/// What the POS was told (CR-32).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosMark {
    /// Nothing is recorded.
    NotRecorded = 0,
    /// The POS was told the sale is paid.
    Paid = 1,
    /// The POS was told the sale is not paid any more.
    Unpaid = 2,
}

/// A payment that arrived after the sale ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatePayment {
    /// The transaction.
    pub txid: [u8; 32],
    /// What it paid.
    pub sats: u64,
}

/// One transaction the final record names, and what it paid the sale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Payment {
    /// The transaction.
    pub txid: [u8; 32],
    /// What it paid the sale's address.
    pub sats: u64,
}

/// The transactions the final record names (CR-32), ascending by ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentList {
    /// The payments the record holds: all of them, or the largest 32.
    pub payments: Vec<Payment>,
    /// The sale had more standing transactions than the record holds.
    pub more: bool,
}

/// An override recorded on a sale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideNote {
    /// Cashier or manager.
    pub role: Role,
    /// Who.
    pub name: String,
    /// The amount put at risk.
    pub sats: u64,
}

/// BK-38: the tip a sale carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipNote {
    /// The tip in minor units of the sale's currency (never 0).
    pub minor: u64,
    /// The person at the till when the sale opened, as the counter wrote it.
    pub by: Option<(Role, String)>,
}

/// BK-35: a change to the cashiers of a counter, as its record says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffNote {
    /// The counter that wrote the record.
    pub counter: DeviceId,
    /// When, in Unix seconds (UTC).
    pub at: u64,
    /// Added or removed.
    pub change: StaffChange,
    /// The cashier's name, as the counter wrote it.
    pub name: String,
    /// The manager who made the change, as the counter wrote it.
    pub by: String,
}

/// BK-06: overpaid or underpaid, as far as the events tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settlement {
    /// A surplus is owed to the customer.
    Overpaid {
        /// The surplus in satoshis.
        surplus_sats: u64,
    },
    /// The satoshis received are below the last quote's satoshis.
    BelowLastQuote {
        /// How far below, in satoshis.
        short_sats: u64,
    },
}

/// BK-32: what the shop's node reported of a Lightning payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightningPayment {
    /// The payment hash of the invoice.
    pub payment_hash: [u8; 32],
    /// What the node received, in millisatoshis.
    pub amount_msat: u64,
    /// When the node says it settled, in Unix seconds, if it says.
    pub settled_at: Option<u64>,
}

/// BK-32: how a sale was paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentMode {
    /// On the chain, or not paid yet: what every sale was before the
    /// counter could take Lightning.
    OnChain,
    /// Over Lightning, to the shop's own node. Final at once.
    Lightning(LightningPayment),
}

impl PaymentMode {
    /// The word for the mode.
    pub fn text(&self) -> &'static str {
        match self {
            Self::OnChain => "on-chain",
            Self::Lightning(_) => "lightning",
        }
    }

    /// The Lightning payment, if the sale was paid that way.
    pub fn lightning(&self) -> Option<&LightningPayment> {
        match self {
            Self::OnChain => None,
            Self::Lightning(p) => Some(p),
        }
    }
}

/// BK-06: one sale, with what the records carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sale {
    /// Which sale.
    pub key: SaleKey,
    /// The opaque ID (BK-07).
    pub id: SaleId,
    /// When the sale opened, in Unix seconds (UTC).
    pub opened_at: u64,
    /// The amount of the sale itself in minor units of the currency, without
    /// the tip.
    pub amount_minor: u64,
    /// BK-38: the tip, when the customer chose one.
    pub tip: Option<TipNote>,
    /// The currency.
    pub currency: Currency,
    /// The satoshis the first quote asked for.
    pub first_quote_sats: u64,
    /// The first quote's rate, in minor units per bitcoin.
    pub quote_rate_minor_per_btc: u64,
    /// Where the first quote's rate came from.
    pub rate_source: Value<RateSource>,
    /// The satoshis the whole amount needed at the last quote: the first
    /// quote's when the sale was never requoted.
    pub last_quote_sats: u64,
    /// The last quote's rate, in minor units per bitcoin.
    pub last_quote_rate_minor_per_btc: u64,
    /// Where the last quote's rate came from.
    pub last_rate_source: Value<RateSource>,
    /// The state after the last event.
    pub state: SaleState,
    /// Funds were recorded and not withdrawn by a conflict.
    pub funds_seen: bool,
    /// The sale reached the refund depth and has not been reopened.
    pub reached_final: bool,
    /// When funds were first recorded, in Unix seconds (UTC).
    pub first_funds_at: Option<u64>,
    /// The satoshis received (BK-09).
    pub sats_received: Value<u64>,
    /// The sales tax in minor units (BK-10).
    pub sales_tax_minor: Value<u64>,
    /// What the satoshis received were worth in the shop's currency when
    /// they were first seen, by the sale's own quotes (the final record).
    pub value_at_first_seen_minor: Value<u64>,
    /// The earliest time a payment of the sale was first seen, in Unix
    /// seconds (UTC), from the final record.
    pub first_seen_at: Option<u64>,
    /// The transactions that paid the sale, from the final record.
    pub payments: Value<PaymentList>,
    /// A payment event other than a late one was recorded and not withdrawn
    /// by a conflict.
    pub own_payment: bool,
    /// The address.
    pub address: Value<String>,
    /// Payments that arrived after the sale ended.
    pub late_payments: Vec<LatePayment>,
    /// The amount put at risk when the sale was accepted at once.
    pub at_risk_sats: Option<u64>,
    /// The surplus owed to the customer.
    pub surplus_owed_sats: u64,
    /// What arrived below the dust line.
    pub dust_sats: u64,
    /// Overrides on the sale.
    pub overrides: Vec<OverrideNote>,
    /// What the POS was told.
    pub pos: PosMark,
    /// BK-32: how the sale was paid.
    pub payment_mode: PaymentMode,
    /// BK-41: the note on the sale, as the counter wrote it.
    pub note: Option<String>,
    /// BK-43: the till's reference, as the counter wrote it: the first one
    /// the records name stands.
    pub till_reference: Option<String>,
}

impl SaleState {
    /// BK-44: whether the sale is paid by its own rules: accepted at once,
    /// confirmed or final. A sale that is open, ended, replaced or reopened is
    /// not.
    pub fn is_paid(self) -> bool {
        matches!(self, Self::PaidAtOnce | Self::Confirmed | Self::Final)
    }
}

impl Sale {
    /// BK-38: the tip in minor units; 0 when the sale had none.
    pub fn tip_minor(&self) -> u64 {
        self.tip.as_ref().map_or(0, |t| t.minor)
    }

    /// BK-38: the amount and the tip together, in minor units: what the
    /// sale asked the customer to pay. `None` when the two do not fit.
    pub fn due_minor(&self) -> Option<u64> {
        self.amount_minor.checked_add(self.tip_minor())
    }

    /// BK-08: counted, pending or no funds.
    pub fn counting(&self) -> Counting {
        if !self.funds_seen {
            Counting::NoFunds
        } else if self.reached_final {
            Counting::Counted
        } else {
            Counting::Pending
        }
    }

    /// BK-06: overpaid or underpaid, if the events say. Underpaid is read
    /// against the last quote.
    pub fn settlement(&self) -> Option<Settlement> {
        if self.surplus_owed_sats > 0 {
            return Some(Settlement::Overpaid {
                surplus_sats: self.surplus_owed_sats,
            });
        }
        // BK-34: the invoice of a Lightning sale may have been made at an
        // earlier quote than the last, and the counter took it as paid in
        // full; it is not read against the last quote.
        if self.payment_mode.lightning().is_some() {
            return None;
        }
        match self.sats_received {
            Value::Known(got) if self.funds_seen && got < self.last_quote_sats => {
                Some(Settlement::BelowLastQuote {
                    short_sats: self.last_quote_sats.saturating_sub(got),
                })
            }
            _ => None,
        }
    }
}

/// BK-12: what a disposal is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisposalKind {
    /// A refund of a sale.
    Refund {
        /// The sale refunded.
        sale: SaleKey,
    },
    /// A cash-out.
    CashOut,
    /// A network fee paid in bitcoin.
    Fee,
}

/// BK-12: the basis, in minor units, as the caller gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Basis {
    /// Minor units of the disposal's currency.
    pub minor: u64,
}

/// BK-12: bitcoin sent out, given by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disposal {
    /// When, in Unix seconds (UTC).
    pub at: u64,
    /// What it is.
    pub kind: DisposalKind,
    /// The currency of the value and the basis.
    pub currency: Currency,
    /// The satoshis sent out.
    pub sats: u64,
    /// The dollar value at the time, in minor units, if known.
    pub minor_value: Option<u64>,
    /// The basis, if known.
    pub basis: Option<Basis>,
}

/// BK-13: a cash refund from the till, given by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CashReturn {
    /// When, in Unix seconds (UTC).
    pub at: u64,
    /// The sale.
    pub sale: SaleKey,
    /// The amount returned, in minor units.
    pub minor: u64,
}

/// BK-10: what the POS match knows of each sale, given by the caller.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PosFacts {
    tax: BTreeMap<SaleKey, u64>,
}

impl PosFacts {
    /// No facts.
    pub fn new() -> Self {
        Self::default()
    }

    /// The sales tax of a sale, in minor units. A second call for the same
    /// sale replaces the first.
    pub fn insert_tax(&mut self, sale: SaleKey, tax_minor: u64) {
        self.tax.insert(sale, tax_minor);
    }

    /// The tax given for a sale.
    pub fn tax(&self, sale: &SaleKey) -> Option<u64> {
        self.tax.get(sale).copied()
    }

    /// Every sale with a tax amount.
    pub fn iter(&self) -> impl Iterator<Item = (&SaleKey, &u64)> {
        self.tax.iter()
    }
}

/// BK-44: which bill: the counter's key and the bill's number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BillKey {
    /// The counter.
    pub counter: DeviceId,
    /// The bill's number.
    pub number: u32,
}

/// BK-44: a part of a bill, as its records say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillPartNote {
    /// A bitcoin part: the sale (of the bill's own counter).
    Sale(SaleKey),
    /// A part paid another way, in the cashier's word: the amount the
    /// cashier typed, in minor units, and the way.
    Other {
        /// The amount in minor units.
        minor: u64,
        /// The way.
        way: PartWay,
    },
}

/// BK-44: how a bill ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BillEndNote {
    /// How.
    pub how: BillEnd,
    /// When, in Unix seconds (UTC).
    pub at: u64,
    /// What was not paid, for a bill closed short; otherwise 0.
    pub unpaid_minor: u64,
}

/// BK-44: one bill or tab, folded from its records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bill {
    /// Which bill.
    pub key: BillKey,
    /// When it opened, in Unix seconds (UTC).
    pub opened_at: u64,
    /// A bill or a tab.
    pub kind: BillKind,
    /// The amount in minor units: for a tab, the last one the records give.
    pub amount_minor: u64,
    /// The currency.
    pub currency: Currency,
    /// BK-43: the till's reference, the first the records name.
    pub reference: Option<String>,
    /// The parts, in the order the records give them.
    pub parts: Vec<BillPartNote>,
    /// How it ended, when it did.
    pub ended: Option<BillEndNote>,
}

/// BK-44: the word for how a bill stands.
pub fn bill_state_text(bill: &Bill) -> &'static str {
    match bill.ended.map(|e| e.how) {
        None => "open",
        Some(BillEnd::Settled) => "settled",
        Some(BillEnd::ClosedShort) => "closed short",
        Some(BillEnd::Cancelled) => "cancelled",
    }
}

/// BK-44: the word for the way a part was paid.
pub fn way_text(way: PartWay) -> &'static str {
    match way {
        PartWay::Cash => "cash",
        PartWay::Card => "card",
        PartWay::Other => "other",
    }
}
