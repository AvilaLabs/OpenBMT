//! The types of the call protocol: what a request carries and what a
//! result is made of. Each one writes itself (and, for a request, reads
//! itself) with the encodings of [`codec`](crate::codec) and describes
//! itself for `bindings/PROTOCOL.md`.

use openbmt_core::say::Words;
use openbmt_counter::{RefundState, Report, Role, State};

use crate::codec::{
    bad_tag, wire_record, wire_record_get, wire_unit_enum, Define, Get, Put, Reader, Shaped,
    TypeDef,
};
use crate::error::CounterError;
use crate::pay::Payment;
use crate::shop::SettingsView;
use crate::say::{tone_byte, who_byte};

// ---- requests -------------------------------------------------------------------

/// The bitcoin network the counter is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CounterNetwork {
    /// Bitcoin.
    Mainnet,
    /// Testnet 3.
    Testnet,
    /// Signet.
    Signet,
    /// Regtest.
    Regtest,
}

wire_unit_enum!(CounterNetwork {
    Mainnet = 0,
    Testnet = 1,
    Signet = 2,
    Regtest = 3
});

/// Who acts at the counter. The app has checked the person's PIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorRole {
    /// A cashier.
    Cashier,
    /// A manager.
    Manager,
}

wire_unit_enum!(ActorRole {
    Cashier = 0,
    Manager = 1
});

impl ActorRole {
    pub(crate) fn role(self) -> Role {
        match self {
            Self::Cashier => Role::Cashier,
            Self::Manager => Role::Manager,
        }
    }

    fn of(role: Role) -> Self {
        match role {
            Role::Cashier => Self::Cashier,
            Role::Manager => Self::Manager,
        }
    }
}

/// A person at the counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorView {
    /// Cashier or manager.
    pub role: ActorRole,
    /// The name.
    pub name: String,
}

wire_record!(ActorView { role, name });
wire_record_get!(ActorView { role, name });

/// What `start` is given: the counter's pairing record as the app kept it,
/// and what is not part of pairing (`docs/COUNTER-RULES.md`, CR-97).
///
/// The record (`PairingRecord::to_bytes`, magic `OBMt`) holds the identity
/// secret, the roots, the wallet, the network, the settings, the first batch
/// and the peers; `pair_finish` made it and the app keeps it sealed. The
/// network, the roots and the smallest batch are not fields of this request:
/// the first two are in the record and the last is the counter's own
/// (CR-96).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartConfig {
    /// The data folder, the app's own (`noBackupFilesDir`).
    pub dir: String,
    /// The time now, in seconds since 1970.
    pub now: u64,
    /// The pairing record, as `pair_finish` returned it.
    pub record: Vec<u8>,
    /// The SOCKS5 proxy of Tor, `host:port`, or none.
    pub proxy: Option<String>,
}

wire_record!(StartConfig {
    dir,
    now,
    record,
    proxy
});
wire_record_get!(StartConfig {
    dir,
    now,
    record,
    proxy
});

/// The arguments of `pair_begin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairBeginArgs {
    /// The Key app's offer, as scanned (`OBMO`).
    pub offer: Vec<u8>,
    /// The name the counter gives itself.
    pub name: String,
    /// The time now, in seconds since 1970.
    pub now: u64,
}

wire_record!(PairBeginArgs { offer, name, now });
wire_record_get!(PairBeginArgs { offer, name, now });

/// What `pair_begin` returns: the counter's request to show as a code, and
/// the four words to show beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairStartedView {
    /// The request (`OBMJ`) for the owner's phone to scan.
    pub request: Vec<u8>,
    /// The four session words.
    pub words: Vec<String>,
}

wire_record!(PairStartedView { request, words });

/// What `pairing_read` returns about a saved pairing record, for display
/// before the node starts. The record is read and its signatures are not
/// checked here: `start` checks them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingView {
    /// The network the counter was paired on.
    pub network: CounterNetwork,
    /// The shop's settings in the record.
    pub settings: SettingsView,
}

wire_record!(PairingView { network, settings });

/// The arguments of `open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenArgs {
    /// The time now.
    pub now: u64,
    /// The amount in minor units of the currency.
    pub amount_minor: u64,
    /// The currency, three capital letters.
    pub currency: String,
    /// The rate, minor units per bitcoin.
    pub rate: u64,
    /// Where the rate came from.
    pub source_label: String,
    /// When the rate was fetched.
    pub source_fetched_at: u64,
}

wire_record!(OpenArgs {
    now,
    amount_minor,
    currency,
    rate,
    source_label,
    source_fetched_at
});
wire_record_get!(OpenArgs {
    now,
    amount_minor,
    currency,
    rate,
    source_label,
    source_fetched_at
});

/// The arguments of `requote`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequoteArgs {
    /// The time now.
    pub now: u64,
    /// The sale.
    pub index: u32,
    /// The rate, minor units per bitcoin.
    pub rate: u64,
    /// Where the rate came from.
    pub source_label: String,
    /// When the rate was fetched.
    pub source_fetched_at: u64,
}

wire_record!(RequoteArgs {
    now,
    index,
    rate,
    source_label,
    source_fetched_at
});
wire_record_get!(RequoteArgs {
    now,
    index,
    rate,
    source_label,
    source_fetched_at
});

/// The arguments of `override_sale`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideArgs {
    /// The time now.
    pub now: u64,
    /// The sale.
    pub index: u32,
    /// Who accepts it.
    pub actor: ActorView,
}

wire_record!(OverrideArgs { now, index, actor });
wire_record_get!(OverrideArgs { now, index, actor });

/// How a refund is asked for (CR-61).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefundAskView {
    /// This many satoshis.
    Sats {
        /// The satoshis.
        sats: u64,
    },
    /// An amount in the shop's currency at a rate.
    Value {
        /// The amount in minor units.
        minor: u64,
        /// The rate, minor units per bitcoin.
        rate: u64,
        /// Where the rate came from.
        source_label: String,
        /// When the rate was fetched.
        source_fetched_at: u64,
    },
}

impl Shaped for RefundAskView {
    fn shape() -> String {
        "RefundAskView".to_string()
    }
}

impl Put for RefundAskView {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Self::Sats { sats } => {
                out.push(0);
                sats.put(out);
            }
            Self::Value {
                minor,
                rate,
                source_label,
                source_fetched_at,
            } => {
                out.push(1);
                minor.put(out);
                rate.put(out);
                source_label.put(out);
                source_fetched_at.put(out);
            }
        }
    }
}

impl Get for RefundAskView {
    fn get(r: &mut Reader<'_>) -> Result<Self, CounterError> {
        match r.u8()? {
            0 => Ok(Self::Sats { sats: u64::get(r)? }),
            1 => Ok(Self::Value {
                minor: u64::get(r)?,
                rate: u64::get(r)?,
                source_label: String::get(r)?,
                source_fetched_at: u64::get(r)?,
            }),
            t => Err(bad_tag("RefundAskView", t)),
        }
    }
}

impl Define for RefundAskView {
    fn define() -> TypeDef {
        TypeDef {
            name: "RefundAskView",
            body: "enum, a tag byte, then: Sats(sats: u64) | Value(minor: u64, rate: u64, \
                   source_label: string, source_fetched_at: u64)"
                .to_string(),
            tags: vec![("Sats", 0), ("Value", 1)],
        }
    }
}

/// The arguments of `request_refund`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundArgs {
    /// The time now.
    pub now: u64,
    /// The sale.
    pub index: u32,
    /// The amount.
    pub ask: RefundAskView,
    /// Where the money goes, as the customer's wallet shows it.
    pub destination: String,
    /// Who asks.
    pub actor: ActorView,
    /// Why.
    pub reason: String,
}

wire_record!(RefundArgs {
    now,
    index,
    ask,
    destination,
    actor,
    reason
});
wire_record_get!(RefundArgs {
    now,
    index,
    ask,
    destination,
    actor,
    reason
});

/// The arguments of `withdraw_refund`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawArgs {
    /// The time now.
    pub now: u64,
    /// The sale.
    pub index: u32,
    /// The request number.
    pub number: u64,
    /// Who withdraws it.
    pub actor: ActorView,
}

wire_record!(WithdrawArgs {
    now,
    index,
    number,
    actor
});
wire_record_get!(WithdrawArgs {
    now,
    index,
    number,
    actor
});

/// The arguments of `refund_in_cash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashArgs {
    /// The time now.
    pub now: u64,
    /// The sale.
    pub index: u32,
    /// Who gives the cash.
    pub actor: ActorView,
    /// The amount in minor units of the shop's currency.
    pub minor: u64,
    /// Why.
    pub reason: String,
}

wire_record!(CashArgs {
    now,
    index,
    actor,
    minor,
    reason
});
wire_record_get!(CashArgs {
    now,
    index,
    actor,
    minor,
    reason
});

// ---- results --------------------------------------------------------------------

/// What a person is shown (`Words`), as the frames carry it: tone u8, who
/// u8, heading, what, next, code (flag, string), at (flag, u64), quoted
/// (flag, from, text) and detail (flag, string), every string a u16 length
/// and UTF-8.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordsView(pub Words);

fn put_str16(out: &mut Vec<u8>, s: &str) {
    let mut n = s.len().min(usize::from(u16::MAX));
    while !s.is_char_boundary(n) {
        n = n.saturating_sub(1);
    }
    out.extend_from_slice(&u16::try_from(n).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(s.as_bytes().get(..n).unwrap_or_default());
}

fn put_flagged_str16(out: &mut Vec<u8>, s: Option<&str>) {
    match s {
        Some(s) => {
            out.push(1);
            put_str16(out, s);
        }
        None => out.push(0),
    }
}

/// The words as the frames carry them.
pub(crate) fn put_words(out: &mut Vec<u8>, w: &Words) {
    out.push(tone_byte(w.tone));
    out.push(who_byte(w.who));
    put_str16(out, &w.heading);
    put_str16(out, &w.what);
    put_str16(out, &w.next);
    put_flagged_str16(out, w.code);
    match w.at {
        Some(at) => {
            out.push(1);
            out.extend_from_slice(&at.to_le_bytes());
        }
        None => out.push(0),
    }
    match &w.quoted {
        Some(q) => {
            out.push(1);
            put_str16(out, q.from);
            put_str16(out, &q.text);
        }
        None => out.push(0),
    }
    put_flagged_str16(out, w.detail.as_deref());
}

impl Shaped for WordsView {
    fn shape() -> String {
        "Words".to_string()
    }
}

impl Put for WordsView {
    fn put(&self, out: &mut Vec<u8>) {
        put_words(out, &self.0);
    }
}

impl Define for WordsView {
    fn define() -> TypeDef {
        TypeDef {
            name: "Words",
            body: "record: tone: u8, who: u8, heading: str16, what: str16, next: str16, \
                   code: option<str16>, at: option<u64>, quoted: option<(str16, str16)>, \
                   detail: option<str16>; a str16 is a u16 length and UTF-8"
                .to_string(),
            tags: Vec::new(),
        }
    }
}

/// The state of a sale (CR-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateView {
    /// Nothing has arrived.
    NotPaidYet,
    /// Less than the sale needs has arrived.
    PartPaid,
    /// Enough has arrived and it waits for a confirmation.
    Waiting,
    /// Accepted at once.
    PaidInstant,
    /// Confirmed at the shop's depth.
    Confirmed,
    /// Confirmed at the refund depth.
    Final,
    /// A person accepted it.
    Overridden,
    /// The money is not safe.
    DoNotHandOver,
    /// Ended with nothing received.
    Expired,
    /// Cancelled with nothing received.
    Cancelled,
    /// The sale holds more than the counter can judge.
    NeedsAPerson,
}

wire_unit_enum!(StateView {
    NotPaidYet = 0,
    PartPaid = 1,
    Waiting = 2,
    PaidInstant = 3,
    Confirmed = 4,
    Final = 5,
    Overridden = 6,
    DoNotHandOver = 7,
    Expired = 8,
    Cancelled = 9,
    NeedsAPerson = 10
});

impl From<State> for StateView {
    fn from(s: State) -> Self {
        match s {
            State::NotPaidYet => Self::NotPaidYet,
            State::PartPaid => Self::PartPaid,
            State::Waiting => Self::Waiting,
            State::PaidInstant => Self::PaidInstant,
            State::Confirmed => Self::Confirmed,
            State::Final => Self::Final,
            State::Overridden => Self::Overridden,
            State::DoNotHandOver => Self::DoNotHandOver,
            State::Expired => Self::Expired,
            State::Cancelled => Self::Cancelled,
            State::NeedsAPerson => Self::NeedsAPerson,
        }
    }
}

/// The state of a refund entry (CR-72).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefundStateView {
    /// Asked for; with the owner.
    Requested,
    /// Asked for; the sale has not reached the refund depth.
    WaitingForDepth,
    /// Withdrawn.
    Withdrawn,
    /// Paid in cash from the till.
    PaidInCash,
}

wire_unit_enum!(RefundStateView {
    Requested = 0,
    WaitingForDepth = 1,
    Withdrawn = 2,
    PaidInCash = 3
});

impl From<RefundState> for RefundStateView {
    fn from(s: RefundState) -> Self {
        match s {
            RefundState::Requested => Self::Requested,
            RefundState::WaitingForDepth => Self::WaitingForDepth,
            RefundState::Withdrawn => Self::Withdrawn,
            RefundState::PaidInCash => Self::PaidInCash,
        }
    }
}

/// Something the connector must tell the POS, the books or the owner
/// (CR-30 to CR-32), once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportView(pub Report);

impl Shaped for ReportView {
    fn shape() -> String {
        "ReportView".to_string()
    }
}

impl Put for ReportView {
    fn put(&self, out: &mut Vec<u8>) {
        match &self.0 {
            Report::PosPaid { index } => {
                out.push(0);
                index.put(out);
            }
            Report::PosUnpaid { index } => {
                out.push(1);
                index.put(out);
            }
            Report::BooksFinal {
                index,
                amount,
                currency,
                received_sats,
            } => {
                out.push(2);
                index.put(out);
                amount.0.put(out);
                currency.to_string().put(out);
                received_sats.put(out);
            }
            Report::SurplusOwed { index, sats } => {
                out.push(3);
                index.put(out);
                sats.put(out);
            }
            Report::LatePayment { index, sats } => {
                out.push(4);
                index.put(out);
                sats.put(out);
            }
            Report::Alarm { index } => {
                out.push(5);
                index.put(out);
            }
            Report::Dust { index, sats } => {
                out.push(6);
                index.put(out);
                sats.put(out);
            }
        }
    }
}

impl Define for ReportView {
    fn define() -> TypeDef {
        TypeDef {
            name: "ReportView",
            body: "enum, a tag byte, then: PosPaid(index: u32) | PosUnpaid(index: u32) | \
                   BooksFinal(index: u32, amount_minor: u64, currency: string, \
                   received_sats: u64) | SurplusOwed(index: u32, sats: u64) | \
                   LatePayment(index: u32, sats: u64) | Alarm(index: u32) | \
                   Dust(index: u32, sats: u64)"
                .to_string(),
            tags: vec![
                ("PosPaid", 0),
                ("PosUnpaid", 1),
                ("BooksFinal", 2),
                ("SurplusOwed", 3),
                ("LatePayment", 4),
                ("Alarm", 5),
                ("Dust", 6),
            ],
        }
    }
}

/// A sale that was opened, or a new quote of one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedView {
    /// The receive index.
    pub index: u32,
    /// What the sale needs, in satoshis.
    pub needs_sats: u64,
    /// The rate of the quote, minor units per bitcoin.
    pub quote_rate: u64,
    /// First second at which the quote holds.
    pub quote_from: u64,
    /// First second at which it no longer holds.
    pub quote_until: u64,
    /// What the customer scans.
    pub payment: Payment,
}

wire_record!(OpenedView {
    index,
    needs_sats,
    quote_rate,
    quote_from,
    quote_until,
    payment
});

/// The screen of a sale (CR-10): the state, the words for it, and what
/// leaves the counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenView {
    /// The state.
    pub state: StateView,
    /// What the screen says; the tone is the state's colour.
    pub words: WordsView,
    /// The extra lines for a slow payment (CR-18) and a late one (CR-19).
    pub flags: Vec<WordsView>,
    /// What has arrived.
    pub received_sats: u64,
    /// What the sale needs.
    pub needs_sats: u64,
    /// A seen payment has not confirmed for too long.
    pub stuck: bool,
    /// A payment arrived after the sale ended.
    pub late: bool,
    /// A block that confirmed the payment left the chain.
    pub reopened: bool,
    /// A conflict has been seen on the sale.
    pub conflict_seen: bool,
    /// What leaves the counter now, once.
    pub reports: Vec<ReportView>,
    /// What the customer scans, for a sale that is still live.
    pub payment: Option<Payment>,
}

wire_record!(ScreenView {
    state,
    words,
    flags,
    received_sats,
    needs_sats,
    stuck,
    late,
    reopened,
    conflict_seen,
    reports,
    payment
});

/// What `pump` and `stop` return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PumpView {
    /// What must leave the counter, once.
    pub reports: Vec<ReportView>,
    /// The lines the node and the driver logged since the last call.
    pub notes: Vec<String>,
    /// The driver has stopped.
    pub stopped: bool,
}

wire_record!(PumpView {
    reports,
    notes,
    stopped
});

/// One refund of a sale (CR-72).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundRow {
    /// The sale.
    pub sale: u32,
    /// The request number; none for a cash refund.
    pub number: Option<u64>,
    /// What was asked, in satoshis.
    pub asked: u64,
    /// The amount in minor units, when given that way.
    pub value_minor: Option<u64>,
    /// The state.
    pub state: RefundStateView,
    /// The role of who asked.
    pub by_role: ActorRole,
    /// The name of who asked.
    pub by_name: String,
    /// When.
    pub at: u64,
    /// What the screen says for the state.
    pub words: WordsView,
}

wire_record!(RefundRow {
    sale,
    number,
    asked,
    value_minor,
    state,
    by_role,
    by_name,
    at,
    words
});

impl RefundRow {
    pub(crate) fn of(v: &openbmt_counter::RefundView) -> Self {
        Self {
            sale: v.sale,
            number: v.number,
            asked: v.asked,
            value_minor: v.value_minor,
            state: v.state.into(),
            by_role: ActorRole::of(v.by.role),
            by_name: v.by.name.clone(),
            at: v.at,
            words: WordsView(v.words()),
        }
    }
}

/// What taking a heartbeat did (CR-50 to CR-56).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatOutcome {
    /// The counter can use it to accept at once.
    pub verified: bool,
    /// When the owner's watcher signed it, if verified.
    pub sent_at: Option<u64>,
    /// What the screen says.
    pub words: WordsView,
    /// What judging the sales again reported, once.
    pub reports: Vec<ReportView>,
}

wire_record!(HeartbeatOutcome {
    verified,
    sent_at,
    words,
    reports
});

/// The answer of the health check (WR-50 to WR-53).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthView {
    /// Every rule holds.
    pub healthy: bool,
    /// Some peer is ahead.
    pub behind: bool,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(HealthView {
    healthy,
    behind,
    words
});

/// What `status` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    /// The height up to which every block has been checked and written down.
    pub checked_height: u32,
    /// The tip of the node's header chain.
    pub tip_height: u32,
    /// The last answer of the health rules, once a `Peers` event has come.
    pub health: Option<HealthView>,
    /// The sales the node knows of, lowest first.
    pub sales: Vec<u32>,
    /// CR-75: sales refused for being below the floor since the start.
    pub refused_below_minimum: u64,
    /// The driver has stopped.
    pub stopped: bool,
}

wire_record!(StatusView {
    checked_height,
    tip_height,
    health,
    sales,
    refused_below_minimum,
    stopped
});
