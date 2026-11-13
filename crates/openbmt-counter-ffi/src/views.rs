//! The types of the call protocol: what a request carries and what a
//! result is made of. Each one writes itself (and, for a request, reads
//! itself) with the encodings of [`codec`](crate::codec) and describes
//! itself for `bindings/PROTOCOL.md`.

use openbmt_core::pinned_wallet::WalletKind;
use openbmt_core::say::Words;
use openbmt_counter::{DayTotals, RefundState, Report, Role, State, Tally};
use openbmt_counter_node::{FieldValue, LnRequest, LnTransport, PendingChange, PendingSwitch};
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

/// What kind of wallet an offer from the shop's own wallet names (CR-228).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnWalletKind {
    /// The offer is the Key app's: there is no wallet to check an address of.
    None,
    /// One key.
    SingleKey,
    /// Two of three keys.
    TwoOfThree,
}

wire_unit_enum!(OwnWalletKind {
    None = 0,
    SingleKey = 1,
    TwoOfThree = 2
});

/// What `pair_begin` and `pair_address_same` return. For the Key app's offer
/// `pair_begin` returns the counter's request to show as a code and the four
/// words to show beside it. For an offer from the shop's own wallet
/// `pair_begin` returns the first address and the kind of wallet, with no
/// request and no words (CR-228); `pair_address_same` then returns the
/// request and the four words (CR-229) and an empty address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairStartedView {
    /// The request (`OBMJ` or `OBM-`) for the owner's phone to scan. Empty
    /// until the counter has made one.
    pub request: Vec<u8>,
    /// The four session words. Empty until the counter has made a request.
    pub words: Vec<String>,
    /// The wallet's first receive address, for an offer from the shop's own
    /// wallet; empty otherwise.
    pub address: String,
    /// The kind of wallet of an own-wallet offer.
    pub wallet: OwnWalletKind,
}

wire_record!(PairStartedView {
    request,
    words,
    address,
    wallet
});

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

/// The arguments of `open_with_tip` (CR-189): those of `open` with the tip
/// the customer chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTipArgs {
    /// The time now.
    pub now: u64,
    /// The amount of the sale itself in minor units of the currency.
    pub amount_minor: u64,
    /// The tip in minor units of the currency, 0 for none.
    pub tip_minor: u64,
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

wire_record!(OpenTipArgs {
    now,
    amount_minor,
    tip_minor,
    currency,
    rate,
    source_label,
    source_fetched_at,
    actor
});
wire_record_get!(OpenTipArgs {
    now,
    amount_minor,
    tip_minor,
    currency,
    rate,
    source_label,
    source_fetched_at,
    actor
});

/// The arguments of `open_request` (CR-205, CR-206): those of `open` with a
/// note and the days of the request. A request has no tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequestArgs {
    /// The time now.
    pub now: u64,
    /// The amount of the request in minor units of the currency.
    pub amount_minor: u64,
    /// The currency, three capital letters.
    pub currency: String,
    /// The rate, minor units per bitcoin.
    pub rate: u64,
    /// Where the rate came from.
    pub source_label: String,
    /// When the rate was fetched.
    pub source_fetched_at: u64,
    /// The person at the till who makes the request, when one is signed in
    /// (CR-148, CR-151).
    pub actor: Option<ActorView>,
    /// The note, if the cashier wrote one (CR-206).
    pub note: Option<String>,
    /// How long the request lasts: 1, 3 or 7 days (CR-205).
    pub days: u8,
}

wire_record!(OpenRequestArgs {
    now,
    amount_minor,
    currency,
    rate,
    source_label,
    source_fetched_at,
    actor,
    note,
    days
});
wire_record_get!(OpenRequestArgs {
    now,
    amount_minor,
    currency,
    rate,
    source_label,
    source_fetched_at,
    actor,
    note,
    days
});

/// The arguments of `add_note` (CR-206).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddNoteArgs {
    /// The time now.
    pub now: u64,
    /// The sale.
    pub index: u32,
    /// The note.
    pub note: String,
}

wire_record!(AddNoteArgs { now, index, note });
wire_record_get!(AddNoteArgs { now, index, note });

/// What a sale carries besides its amount (CR-205, CR-206).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleInfoView {
    /// The note, if the sale has one.
    pub note: Option<String>,
    /// The days of a request to send: 1, 3 or 7; 0 for a sale shown at the
    /// till.
    pub request_days: u8,
    /// When a sale with nothing received expires, in seconds since 1970.
    pub expires_at: u64,
    /// CR-217: a bitcoin refund of the sale cannot be asked for: it was paid
    /// over Lightning and nothing of it is left to refund on-chain.
    pub cash_refund_only: bool,
    /// CR-245, CR-254: the till's reference of the sale, when it has one.
    pub reference: Option<String>,
    /// CR-254: the number of the bill the sale is a part of, when it is one.
    pub bill_number: Option<u32>,
}

wire_record!(SaleInfoView {
    note,
    request_days,
    expires_at,
    cash_refund_only,
    reference,
    bill_number
});

/// How a value of a field of a change to the settings is to be written
/// (CR-240).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeValueKind {
    /// Whole units of the shop's currency; `number` holds them (0 is off).
    Units,
    /// Satoshis; `number` holds them.
    Sats,
    /// Basis points, 100 to the percent; `number` holds them.
    Bps,
    /// A count; `number` holds it.
    Count,
    /// Seconds; `number` holds them.
    Seconds,
    /// A switch; `number` is 1 for on and 0 for off.
    Flag,
    /// A currency; `text` holds its three letters.
    Code,
}

wire_unit_enum!(ChangeValueKind {
    Units = 0,
    Sats = 1,
    Bps = 2,
    Count = 3,
    Seconds = 4,
    Flag = 5,
    Code = 6
});

/// One value of a field, now or after the change (CR-240). The app writes
/// it, in the shop's currency where it is an amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeValueView {
    /// What the value is.
    pub kind: ChangeValueKind,
    /// The number, for every kind but [`ChangeValueKind::Code`]; 0 for it.
    pub number: u64,
    /// The currency's letters for [`ChangeValueKind::Code`]; empty for the
    /// others.
    pub text: String,
}

wire_record!(ChangeValueView { kind, number, text });

impl ChangeValueView {
    fn of(v: &FieldValue) -> Self {
        let (kind, number, text) = match v {
            FieldValue::Units(n) => (ChangeValueKind::Units, u64::from(*n), String::new()),
            FieldValue::Sats(n) => (ChangeValueKind::Sats, *n, String::new()),
            FieldValue::Bps(n) => (ChangeValueKind::Bps, u64::from(*n), String::new()),
            FieldValue::Count(n) => (ChangeValueKind::Count, u64::from(*n), String::new()),
            FieldValue::Seconds(n) => (ChangeValueKind::Seconds, u64::from(*n), String::new()),
            FieldValue::Flag(on) => (ChangeValueKind::Flag, u64::from(*on), String::new()),
            FieldValue::Code(c) => (ChangeValueKind::Code, 0, c.clone()),
        };
        Self { kind, number, text }
    }
}

/// One field a change loosens, with its value now and after (CR-240).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeFieldView {
    /// The name of the field, as the settings call it (`light_per_sale`).
    pub field: String,
    /// The value in the settings in force.
    pub now: ChangeValueView,
    /// The value the change would put in force.
    pub after: ChangeValueView,
}

wire_record!(ChangeFieldView { field, now, after });

/// The change that waits to be confirmed at a counter (CR-239, CR-240).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChangeView {
    /// The settings version the change would put in force.
    pub version: u32,
    /// The four words of AP-03, to compare with the owner phone.
    pub words: Vec<String>,
    /// Each field the change loosens.
    pub fields: Vec<ChangeFieldView>,
    /// When the change arrived, in seconds since 1970.
    pub arrived: u64,
    /// When it is dropped if nobody confirmed it, in seconds since 1970.
    pub expires_at: u64,
    /// The counter signed its confirmation since it last started.
    pub confirmed: bool,
}

wire_record!(PendingChangeView {
    version,
    words,
    fields,
    arrived,
    expires_at,
    confirmed
});

impl PendingChangeView {
    pub(crate) fn of(p: &PendingChange) -> Self {
        Self {
            version: p.version,
            words: p.words.iter().map(|w| (*w).to_string()).collect(),
            fields: p
                .fields
                .iter()
                .map(|f| ChangeFieldView {
                    field: f.field.to_string(),
                    now: ChangeValueView::of(&f.now),
                    after: ChangeValueView::of(&f.after),
                })
                .collect(),
            arrived: p.arrived,
            expires_at: p.expires_at,
            confirmed: p.confirmed,
        }
    }
}

/// What `pending_change` returns (CR-239, CR-244): the change that waits, if
/// one does, and the words of a change that came to nothing, if one did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeStateView {
    /// The change that waits.
    pub pending: Option<PendingChangeView>,
    /// What the manager is told about a change that was dropped or refused.
    pub notice: Option<WordsView>,
}

wire_record!(ChangeStateView { pending, notice });

/// The switch from the starter wallet that waits for the manager (CR-259,
/// CR-260): the new wallet's first receive address, the kind of wallet
/// beneath it and when the switch is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSwitchView {
    /// The new wallet's first receive address (receive chain, index 0).
    pub address: String,
    /// The kind of the new wallet.
    pub wallet: OwnWalletKind,
    /// When the switch arrived, in seconds since 1970.
    pub arrived: u64,
    /// When it is dropped if nobody answered it, in seconds since 1970.
    pub expires_at: u64,
}

wire_record!(PendingSwitchView {
    address,
    wallet,
    arrived,
    expires_at
});

impl PendingSwitchView {
    pub(crate) fn of(p: &PendingSwitch) -> Self {
        Self {
            address: p.address.clone(),
            wallet: match p.kind {
                WalletKind::SingleKey => OwnWalletKind::SingleKey,
                WalletKind::TwoOfThree => OwnWalletKind::TwoOfThree,
            },
            arrived: p.arrived,
            expires_at: p.expires_at,
        }
    }
}

/// What `pending_switch` returns (CR-259 to CR-261, CR-264): the switch that
/// waits, if one does, the words of a switch that came to nothing, and the
/// pairing record the app still has to keep, if a switch was begun and the
/// record not kept (CR-261, step two).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchStateView {
    /// The switch that waits for the manager.
    pub pending: Option<PendingSwitchView>,
    /// What the manager is told about a switch that was dropped.
    pub notice: Option<WordsView>,
    /// The pairing record to seal and keep, then `switch_record_kept`.
    pub record_update: Option<Vec<u8>>,
}

wire_record!(SwitchStateView {
    pending,
    notice,
    record_update
});

/// What `switch_same` returns (CR-261): the words to show, and the pairing
/// record the app seals and keeps before it asks `switch_record_kept`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchSameView {
    /// What the manager is told: the counter now pays into the new wallet.
    pub words: WordsView,
    /// The new pairing record: the new wallet pinned, no starter flag.
    pub record: Vec<u8>,
}

wire_record!(SwitchSameView { words, record });

/// One open request, for the list on Home (CR-215).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestRow {
    /// The receive index.
    pub index: u32,
    /// The note, if the request has one.
    pub note: Option<String>,
    /// The amount in minor units of the shop's currency.
    pub amount_minor: u64,
    /// When the request was made.
    pub opened_at: u64,
    /// When it expires if nothing is received.
    pub expires_at: u64,
    /// What it needs, in satoshis.
    pub needs_sats: u64,
}

wire_record!(RequestRow {
    index,
    note,
    amount_minor,
    opened_at,
    expires_at,
    needs_sats
});

/// A tip the counter offers for a sale (CR-188).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipChoiceView {
    /// The suggested percent.
    pub percent: u8,
    /// What it comes to, in minor units of the shop's currency.
    pub tip_minor: u64,
}

wire_record!(TipChoiceView { percent, tip_minor });

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
    /// How much of the refund is the tip, in minor units of the shop's
    /// currency (CR-192).
    pub tip_minor: u64,
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
    tip_minor,
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
            tip_minor: v.tip_minor,
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
    /// CR-219: through a swap provider, the sale has none, and the small
    /// line says so.
    pub not_for_this_sale: bool,
    /// CR-223: through a swap provider, the swap failed before its claim.
    pub failed: Option<WordsView>,
}

wire_record!(LightningSaleView {
    invoice,
    waiting,
    unavailable,
    not_for_this_sale,
    failed
});

/// One request for the app to send to the swap provider (CR-225): plain
/// HTTP/1.1 to the onion name through the counter's own Tor, never around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapRequestView {
    /// The number the answer comes back with.
    pub id: u64,
    /// `host:port` of the provider's onion service.
    pub host_port: String,
    /// The bytes to send, a whole HTTP/1.1 request.
    pub bytes: Vec<u8>,
}

wire_record!(SwapRequestView {
    id,
    host_port,
    bytes
});

impl From<openbmt_counter_node::SwapRequest> for SwapRequestView {
    fn from(r: openbmt_counter_node::SwapRequest) -> Self {
        Self {
            id: r.id,
            host_port: r.host_port,
            bytes: r.bytes,
        }
    }
}

/// What the provider answered (CR-225): everything it sent until it closed
/// the connection, at most 64 KiB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapReplyArgs {
    /// The number of the request.
    pub id: u64,
    /// The bytes that came back.
    pub bytes: Vec<u8>,
}

wire_record!(SwapReplyArgs { id, bytes });
wire_record_get!(SwapReplyArgs { id, bytes });

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
    /// The tips of the paid sales this person opened, in minor units
    /// (CR-191).
    pub tips_minor: u64,
}

wire_record!(PersonLineView {
    role,
    name,
    sales,
    overrides,
    refunds,
    cash_refunds,
    tips_minor
});

/// The tips of the paid sales of a day (CR-191).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipTallyView {
    /// How many paid sales carried a tip.
    pub count: u32,
    /// The sum of those tips in minor units of the shop's currency.
    pub minor: u64,
}

wire_record!(TipTallyView { count, minor });

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
    /// The tips of the paid sales (CR-191).
    pub tips: TipTallyView,
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
    tips,
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
            tips: TipTallyView {
                count: t.tips.count,
                minor: t.tips.minor,
            },
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
                    tips_minor: p.tips_minor,
                })
                .collect(),
            people_left_out: t.people_left_out,
        }
    }
}
