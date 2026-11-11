//! The types of the call protocol: what a request carries and what a
//! result is made of. Each one writes itself (and, for a request, reads
//! itself) with the encodings of [`codec`](crate::codec) and describes
//! itself for `bindings/PROTOCOL.md`.

use openbmt_core::say::Words;
use openbmt_counter::{DayTotals, RefundState, Report, Role, State, Tally};
use openbmt_counter_node::{LnRequest, LnTransport};
use openbmt_lightning::request::Method;

use crate::codec::{
    bad_tag, wire_record, wire_record_get, wire_unit_enum, Define, Get, Put, Reader, Shaped,
    TypeDef,
};
use crate::error::CounterError;
use crate::pay::Payment;
use crate::say::{tone_byte, who_byte};
use crate::shop::SettingsView;

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

    pub(crate) fn of(role: Role) -> Self {
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
    /// CR-100: the own node's address as `own_node_check` made it, or none.
    /// An address that is refused, or that comes with no proxy, starts the
    /// node without an own node, and the status says so (CR-102).
    pub own_node: Option<String>,
    /// CR-115: the saved Lightning connection, as `ln_reply` returned it
    /// when the test passed and as the app kept it sealed, or none. It is
    /// not part of the pairing record.
    pub lightning: Option<Vec<u8>>,
    /// CR-134: the SOCKS5 proxy of Orbot for the link to the owner, when the
    /// relay is an onion service and the settings have Tor off (with Tor on
    /// the link uses `proxy`). The app gives it only when the proxy answers.
    pub link_proxy: Option<String>,
}

wire_record!(StartConfig {
    dir,
    now,
    record,
    proxy,
    own_node,
    lightning,
    link_proxy
});
wire_record_get!(StartConfig {
    dir,
    now,
    record,
    proxy,
    own_node,
    lightning,
    link_proxy
});

/// The arguments of `own_node_check`: what a manager typed, the network of
/// the pairing and whether the counter has a Tor proxy (CR-100 to CR-102).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnNodeCheckArgs {
    /// The address as typed.
    pub text: String,
    /// The network of the pairing, for the usual port.
    pub network: CounterNetwork,
    /// The counter has a Tor proxy (CR-87).
    pub tor: bool,
}

wire_record!(OwnNodeCheckArgs { text, network, tor });
wire_record_get!(OwnNodeCheckArgs { text, network, tor });

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

/// What kind of relay a pairing record holds (CR-131), for the app to know
/// what the link will need before the node starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayKind {
    /// No relay: the counter has no link.
    None,
    /// An address on the local network.
    Direct,
    /// An onion service, which needs Orbot.
    Onion,
}

wire_unit_enum!(RelayKind {
    None = 0,
    Direct = 1,
    Onion = 2
});

/// What `pairing_read` returns about a saved pairing record, for display
/// before the node starts. The record is read and its signatures are not
/// checked here: `start` checks them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingView {
    /// The network the counter was paired on.
    pub network: CounterNetwork,
    /// The shop's settings in the record.
    pub settings: SettingsView,
    /// What kind of relay the record holds.
    pub relay: RelayKind,
}

wire_record!(PairingView {
    network,
    settings,
    relay
});

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
    /// The person at the till who opens the sale, when one is signed in
    /// (CR-148, CR-151).
    pub actor: Option<ActorView>,
}

wire_record!(OpenArgs {
    now,
    amount_minor,
    currency,
    rate,
    source_label,
    source_fetched_at,
    actor
});
wire_record_get!(OpenArgs {
    now,
    amount_minor,
    currency,
    rate,
    source_label,
    source_fetched_at,
    actor
});

/// The arguments of `cancel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelArgs {
    /// The time now.
    pub now: u64,
    /// The sale.
    pub index: u32,
    /// The person at the till who cancels it, when one is signed in
    /// (CR-148, CR-151).
    pub actor: Option<ActorView>,
}

wire_record!(CancelArgs { now, index, actor });
wire_record_get!(CancelArgs { now, index, actor });

/// The arguments of `staff_record` (CR-143).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaffArgs {
    /// The time now.
    pub now: u64,
    /// The cashier was added; false when the cashier was removed.
    pub added: bool,
    /// The cashier's name.
    pub name: String,
    /// The manager who made the change.
    pub by: String,
}

wire_record!(StaffArgs {
    now,
    added,
    name,
    by
});
wire_record_get!(StaffArgs {
    now,
    added,
    name,
    by
});

/// The arguments of `day_totals` (CR-152).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayTotalsArgs {
    /// The start of the day, in seconds since 1970 (counted).
    pub from: u64,
    /// The end of the day (not counted).
    pub to: u64,
    /// Ask for the lines per person too (CR-154).
    pub people: bool,
}

wire_record!(DayTotalsArgs { from, to, people });
wire_record_get!(DayTotalsArgs { from, to, people });

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
    /// CR-109: paid over Lightning, final at once.
    PaidLightning,
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
    NeedsAPerson = 10,
    PaidLightning = 11
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
            State::PaidLightning => Self::PaidLightning,
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
    /// CR-105: which check was done, for a payment that waits.
    pub check: Option<WordsView>,
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
    /// What the customer scans, for a sale that is still live. When the
    /// sale has a Lightning invoice (CR-113) this is the code of LN-18.
    pub payment: Option<Payment>,
    /// CR-113: what the sale shows of Lightning; none when Lightning is off.
    pub lightning: Option<LightningSaleView>,
}

wire_record!(ScreenView {
    state,
    words,
    flags,
    check,
    received_sats,
    needs_sats,
    stuck,
    late,
    reopened,
    conflict_seen,
    reports,
    payment,
    lightning
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

/// The health line of the own node (CR-107).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnNodeView {
    /// The own node can be used as a second check now (WR-63).
    pub usable: bool,
    /// What the owner is told, with the time it was last heard.
    pub words: WordsView,
}

wire_record!(OwnNodeView { usable, words });

/// The counter's link to the owner through the relay (CR-135, CR-136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkView {
    /// The line for the home screen: "Connected to the Box", or why not.
    pub words: WordsView,
    /// The last call to the relay worked.
    pub connected: bool,
    /// When the relay was first not reachable, while it is not: seconds
    /// since 1970.
    pub since: Option<u64>,
    /// The relay answered and refused this counter.
    pub refused: bool,
    /// Heartbeats the node took through the link.
    pub heartbeats_taken: u64,
    /// How many of the counter's records the owner says it holds.
    pub owner_records: u64,
    /// When the last check-in was made.
    pub last_check_in_at: Option<u64>,
    /// The version of the settings in force (CR-162); 0 before the first
    /// step.
    pub settings_version: u32,
    /// CR-164: the settings changed Tor or the Lightning node, so the app
    /// starts the node again when no sale screen is open.
    pub restart_needed: bool,
    /// CR-164, CR-165: what Home says about the settings: that the counter
    /// was removed, or that a restart is coming.
    pub settings_line: Option<WordsView>,
}

wire_record!(LinkView {
    words,
    connected,
    since,
    refused,
    heartbeats_taken,
    owner_records,
    last_check_in_at,
    settings_version,
    restart_needed,
    settings_line
});

/// What `link_step` returns: the link as it is after the step, and the
/// lines the link logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkStepView {
    /// The link now.
    pub link: LinkView,
    /// The lines the link and its courier logged during the step.
    pub notes: Vec<String>,
}

wire_record!(LinkStepView { link, notes });

/// What `status` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    /// The height up to which every block has been checked and written down.
    pub checked_height: u32,
    /// The tip of the node's header chain.
    pub tip_height: u32,
    /// The last answer of the health rules, once a `Peers` event has come.
    pub health: Option<HealthView>,
    /// CR-107: the own node's line; none when no own node is set.
    pub own_node: Option<OwnNodeView>,
    /// The sales the node knows of, lowest first.
    pub sales: Vec<u32>,
    /// CR-75: sales refused for being below the floor since the start.
    pub refused_below_minimum: u64,
    /// The driver has stopped.
    pub stopped: bool,
    /// CR-116: the Lightning line; none when no credential is saved.
    pub lightning: Option<LightningStatusView>,
}

wire_record!(StatusView {
    checked_height,
    tip_height,
    health,
    own_node,
    sales,
    refused_below_minimum,
    stopped,
    lightning
});

// ---- Lightning (CR-112 to CR-116) -------------------------------------------------

/// What the screen of a sale shows of Lightning (CR-113).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightningSaleView {
    /// The invoice passed the checks and `payment` is the code of LN-18.
    pub invoice: bool,
    /// The invoice has been asked for and has not come yet.
    pub waiting: bool,
    /// The line that says Lightning is not available for this sale, with
    /// the reason in its details.
    pub unavailable: Option<WordsView>,
}

wire_record!(LightningSaleView {
    invoice,
    waiting,
    unavailable
});

/// The Lightning line of the status (CR-116).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightningStatusView {
    /// Sales get a Lightning code now.
    pub ready: bool,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(LightningStatusView { ready, words });

/// The kind of node a credential is for (LN-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LnKind {
    /// LND, over its REST interface.
    Lnd,
    /// Core Lightning, over clnrest.
    Cln,
}

wire_unit_enum!(LnKind { Lnd = 0, Cln = 1 });

/// The arguments of `ln_setup`: what a manager scanned or typed (CR-115).
///
/// For LND, `address` is the `lndconnect://` text and the other two are
/// empty. For Core Lightning, `address` is `host:port`, `secret` the rune and
/// `cert` the certificate as PEM text or as a SHA-256 fingerprint in hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnSetupArgs {
    /// The kind of node.
    pub kind: LnKind,
    /// The `lndconnect://` text, or `host:port`.
    pub address: String,
    /// The rune (Core Lightning only).
    pub secret: String,
    /// The certificate, as PEM or a SHA-256 fingerprint (Core Lightning
    /// only).
    pub cert: String,
}

wire_record!(LnSetupArgs {
    kind,
    address,
    secret,
    cert
});
wire_record_get!(LnSetupArgs {
    kind,
    address,
    secret,
    cert
});

/// The method of a request for the node (CR-114).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LnMethod {
    /// HTTP GET, no body.
    Get,
    /// HTTP POST with a JSON body.
    Post,
}

wire_unit_enum!(LnMethod { Get = 0, Post = 1 });

/// One request for the app to send to the shop's node (CR-114): HTTPS only,
/// no redirect, the certificate pinned with `ln_pin`. The header value is a
/// secret and is never logged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnRequestView {
    /// The number the answer comes back with.
    pub id: u64,
    /// `host:port` of the node.
    pub host_port: String,
    /// GET or POST.
    pub method: LnMethod,
    /// The path and query.
    pub path: String,
    /// The name of the header that carries the credential.
    pub header_name: String,
    /// The value of that header.
    pub header_value: String,
    /// The JSON body of a POST.
    pub body: Option<String>,
}

wire_record!(LnRequestView {
    id,
    host_port,
    method,
    path,
    header_name,
    header_value,
    body
});

impl From<LnRequest> for LnRequestView {
    fn from(r: LnRequest) -> Self {
        Self {
            id: r.id,
            host_port: r.host_port,
            method: match r.request.method {
                Method::Get => LnMethod::Get,
                Method::Post => LnMethod::Post,
            },
            path: r.request.path,
            header_name: r.request.header_name.to_string(),
            header_value: r.request.header_value,
            body: r.request.body,
        }
    }
}

/// What the node answered to a request (CR-114): the status and the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnReplyArgs {
    /// The number of the request.
    pub id: u64,
    /// The HTTP status.
    pub status: u32,
    /// The body, at most 1,000,000 bytes.
    pub body: Vec<u8>,
}

wire_record!(LnReplyArgs { id, status, body });
wire_record_get!(LnReplyArgs { id, status, body });

/// Why the request got no answer (CR-114).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LnTransportView {
    /// The node showed another certificate than the pinned one.
    PinRefused,
    /// No connection could be made.
    Unreachable,
    /// The connect or the read took too long.
    TimedOut,
    /// The body is longer than the app takes.
    TooBig,
    /// The node answered with a redirect.
    Redirected,
}

wire_unit_enum!(LnTransportView {
    PinRefused = 0,
    Unreachable = 1,
    TimedOut = 2,
    TooBig = 3,
    Redirected = 4
});

impl LnTransportView {
    pub(crate) fn of(self) -> LnTransport {
        match self {
            Self::PinRefused => LnTransport::PinRefused,
            Self::Unreachable => LnTransport::Unreachable,
            Self::TimedOut => LnTransport::TimedOut,
            Self::TooBig => LnTransport::TooBig,
            Self::Redirected => LnTransport::Redirected,
        }
    }
}

/// The arguments of `ln_failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnFailArgs {
    /// The number of the request.
    pub id: u64,
    /// Why there was no answer.
    pub why: LnTransportView,
}

wire_record!(LnFailArgs { id, why });
wire_record_get!(LnFailArgs { id, why });

/// What an answer to a setup request led to (CR-115): the next request of
/// the test, or, when the test passed, the bytes to seal and keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnStepView {
    /// The next request of the test, if there is one.
    pub next: Option<LnRequestView>,
    /// The record to seal in `noBackupFilesDir` and hand back at every
    /// `start`, when the test passed.
    pub record: Option<Vec<u8>>,
}

wire_record!(LnStepView { next, record });

// ---- the day's totals (CR-152 to CR-154) ----------------------------------

/// A count with the sums counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TallyView {
    /// How many.
    pub count: u32,
    /// The sum of the amounts in minor units of the shop's currency.
    pub minor: u64,
    /// The sum of the satoshis.
    pub sats: u64,
}

wire_record!(TallyView { count, minor, sats });

impl From<Tally> for TallyView {
    fn from(t: Tally) -> Self {
        Self {
            count: t.count,
            minor: t.minor,
            sats: t.sats,
        }
    }
}

/// What one person did in the day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonLineView {
    /// Cashier or manager.
    pub role: ActorRole,
    /// The name.
    pub name: String,
    /// Sales this person opened.
    pub sales: u32,
    /// Overrides this person made.
    pub overrides: u32,
    /// Refunds this person asked for and did not withdraw.
    pub refunds: u32,
    /// Cash refunds this person gave.
    pub cash_refunds: u32,
}

wire_record!(PersonLineView {
    role,
    name,
    sales,
    overrides,
    refunds,
    cash_refunds
});

/// The totals of one day (CR-153).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayTotalsView {
    /// The start of the day.
    pub from: u64,
    /// The end of the day.
    pub to: u64,
    /// The currency of the sales, three letters, when there is one.
    pub currency: Option<String>,
    /// Sales paid.
    pub paid: TallyView,
    /// Of those, paid on the chain.
    pub on_chain: TallyView,
    /// Of those, paid over Lightning.
    pub lightning: TallyView,
    /// Sales still waiting.
    pub waiting: TallyView,
    /// Sales whose payment was replaced or reopened.
    pub stopped: u32,
    /// Sales cancelled or expired.
    pub ended: u32,
    /// Overrides.
    pub overrides: TallyView,
    /// Refunds asked for and not withdrawn.
    pub refunds: TallyView,
    /// Refund requests that were withdrawn.
    pub refunds_withdrawn: u32,
    /// Cash refunds.
    pub cash_refunds: TallyView,
    /// Sales whose record names no one.
    pub unnamed_sales: u32,
    /// The lines per person, when asked for.
    pub people: Vec<PersonLineView>,
    /// Persons left out of `people`.
    pub people_left_out: u32,
}

wire_record!(DayTotalsView {
    from,
    to,
    currency,
    paid,
    on_chain,
    lightning,
    waiting,
    stopped,
    ended,
    overrides,
    refunds,
    refunds_withdrawn,
    cash_refunds,
    unnamed_sales,
    people,
    people_left_out
});

impl DayTotalsView {
    pub(crate) fn of(t: &DayTotals) -> Self {
        Self {
            from: t.from,
            to: t.to,
            currency: t.currency.map(|c| c.to_string()),
            paid: t.paid.into(),
            on_chain: t.on_chain.into(),
            lightning: t.lightning.into(),
            waiting: t.waiting.into(),
            stopped: t.stopped,
            ended: t.ended,
            overrides: t.overrides.into(),
            refunds: t.refunds.into(),
            refunds_withdrawn: t.refunds_withdrawn,
            cash_refunds: t.cash_refunds.into(),
            unnamed_sales: t.unnamed_sales,
            people: t
                .people
                .iter()
                .map(|p| PersonLineView {
                    role: ActorRole::of(p.role),
                    name: p.name.clone(),
                    sales: p.sales,
                    overrides: p.overrides,
                    refunds: p.refunds,
                    cash_refunds: p.cash_refunds,
                })
                .collect(),
            people_left_out: t.people_left_out,
        }
    }
}
