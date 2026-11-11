//! The types of the call protocol: what a request carries and what a
//! result is made of. Each one writes itself (and, for a request, reads
//! itself) with the encodings of [`codec`](crate::codec) and describes
//! itself for `bindings/PROTOCOL.md`.

use bitcoin::Network;
use openbmt_core::say::Words;

use crate::codec::{
    bad_tag, wire_record, wire_record_get, wire_unit_enum, Define, Get, Put, Reader, Shaped,
    TypeDef,
};
use crate::error::AppError;
use crate::say::{tone_byte, who_byte};

// ---- requests -------------------------------------------------------------------

/// The bitcoin network the owner is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerNetwork {
    /// Bitcoin.
    Mainnet,
    /// Testnet 3.
    Testnet,
    /// Signet.
    Signet,
    /// Regtest.
    Regtest,
}

wire_unit_enum!(OwnerNetwork {
    Mainnet = 0,
    Testnet = 1,
    Signet = 2,
    Regtest = 3
});

impl OwnerNetwork {
    pub(crate) fn network(self) -> Network {
        match self {
            Self::Mainnet => Network::Bitcoin,
            Self::Testnet => Network::Testnet,
            Self::Signet => Network::Signet,
            Self::Regtest => Network::Regtest,
        }
    }
}

/// How the link reaches the relay (LK-31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteView {
    /// An address and port on the local network, `host:port`.
    Direct {
        /// The address.
        addr: String,
    },
    /// A Tor onion service, by name, through a SOCKS5 proxy.
    Onion {
        /// The proxy, `host:port`.
        proxy: String,
        /// The `.onion` name.
        host: String,
        /// The port.
        port: u32,
    },
}

impl Shaped for RouteView {
    fn shape() -> String {
        "RouteView".to_string()
    }
}

impl Put for RouteView {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Self::Direct { addr } => {
                out.push(0);
                addr.put(out);
            }
            Self::Onion { proxy, host, port } => {
                out.push(1);
                proxy.put(out);
                host.put(out);
                port.put(out);
            }
        }
    }
}

impl Get for RouteView {
    fn get(r: &mut Reader<'_>) -> Result<Self, AppError> {
        match r.u8()? {
            0 => Ok(Self::Direct {
                addr: String::get(r)?,
            }),
            1 => Ok(Self::Onion {
                proxy: String::get(r)?,
                host: String::get(r)?,
                port: u32::get(r)?,
            }),
            t => Err(bad_tag("RouteView", t)),
        }
    }
}

impl Define for RouteView {
    fn define() -> TypeDef {
        TypeDef {
            name: "RouteView",
            body: "enum, a tag byte, then: Direct(addr: string) | Onion(proxy: string, \
                   host: string, port: u32)"
                .to_string(),
            tags: vec![("Direct", 0), ("Onion", 1)],
        }
    }
}

/// The link to the counters through a relay (`docs/LINK-RULES.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkConfig {
    /// How to reach the relay.
    pub route: RouteView,
    /// The relay's key, 32 bytes.
    pub relay_key: Vec<u8>,
    /// The counters' roots, each 32 bytes.
    pub counters: Vec<Vec<u8>>,
    /// Seconds between the heartbeats sent to a counter (LK-61).
    pub heartbeat_every_secs: u64,
    /// Seconds without a check-in before the alert (LK-62).
    pub check_in_grace_secs: u64,
}

wire_record!(LinkConfig {
    route,
    relay_key,
    counters,
    heartbeat_every_secs,
    check_in_grace_secs
});
wire_record_get!(LinkConfig {
    route,
    relay_key,
    counters,
    heartbeat_every_secs,
    check_in_grace_secs
});

/// What `start` is given. The owner's secret, the Key app's root, the wallet
/// and Key 1's account key are what the shop was set up with; the app keeps
/// them and hands them over at each start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartConfig {
    /// The data folder, the app's own (`noBackupFilesDir`).
    pub dir: String,
    /// The network.
    pub network: OwnerNetwork,
    /// The time now, in seconds since 1970.
    pub now: u64,
    /// The owner root's secret, 32 bytes.
    pub owner_secret: Vec<u8>,
    /// The Key app's root, 32 bytes.
    pub approver_root: Vec<u8>,
    /// The wallet's description in its canonical text form.
    pub wallet: String,
    /// Key 1's account private key, the 78 bytes of BIP 32 serialization.
    pub key1_account: Vec<u8>,
    /// Key 3's account public key as text, when the wallet has one on hand.
    pub key3_xpub: Option<String>,
    /// The fewest addresses a batch may hold.
    pub batch_min_count: u32,
    /// Confirmations at which a payment is done (1 to 100).
    pub refund_depth: u32,
    /// The wallet's first block.
    pub birthday: u32,
    /// The SOCKS5 proxy of Tor for the watching connections, `host:port`.
    pub proxy: Option<String>,
    /// Peers to connect to first, `host:port`.
    pub fixed_peers: Vec<String>,
    /// The SOCKS5 proxy of Tor for sending payments, `host:port`.
    pub broadcast_proxy: Option<String>,
    /// Send payments on the watching connections, which are not behind Tor.
    pub without_tor: bool,
    /// The link to the counters, or none.
    pub link: Option<LinkConfig>,
}

wire_record!(StartConfig {
    dir,
    network,
    now,
    owner_secret,
    approver_root,
    wallet,
    key1_account,
    key3_xpub,
    batch_min_count,
    refund_depth,
    birthday,
    proxy,
    fixed_peers,
    broadcast_proxy,
    without_tor,
    link
});
wire_record_get!(StartConfig {
    dir,
    network,
    now,
    owner_secret,
    approver_root,
    wallet,
    key1_account,
    key3_xpub,
    batch_min_count,
    refund_depth,
    birthday,
    proxy,
    fixed_peers,
    broadcast_proxy,
    without_tor,
    link
});

/// The arguments of `queue_refund`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundArgs {
    /// The counter's root, 32 bytes.
    pub counter: Vec<u8>,
    /// The sale's receive index.
    pub sale: u32,
    /// Who asked for the refund, for the Key app to show.
    pub cashier: String,
    /// Where the money goes, as the customer's wallet shows it.
    pub address: String,
    /// The amount in satoshis.
    pub sats: u64,
}

wire_record!(RefundArgs {
    counter,
    sale,
    cashier,
    address,
    sats
});
wire_record_get!(RefundArgs {
    counter,
    sale,
    cashier,
    address,
    sats
});

/// What a cash-out moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CashAmountView {
    /// This many satoshis.
    Sats {
        /// The satoshis.
        sats: u64,
    },
    /// Every coin left after the refunds.
    All,
}

impl Shaped for CashAmountView {
    fn shape() -> String {
        "CashAmountView".to_string()
    }
}

impl Put for CashAmountView {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Self::Sats { sats } => {
                out.push(0);
                sats.put(out);
            }
            Self::All => out.push(1),
        }
    }
}

impl Get for CashAmountView {
    fn get(r: &mut Reader<'_>) -> Result<Self, AppError> {
        match r.u8()? {
            0 => Ok(Self::Sats { sats: u64::get(r)? }),
            1 => Ok(Self::All),
            t => Err(bad_tag("CashAmountView", t)),
        }
    }
}

impl Define for CashAmountView {
    fn define() -> TypeDef {
        TypeDef {
            name: "CashAmountView",
            body: "enum, a tag byte, then: Sats(sats: u64) | All".to_string(),
            tags: vec![("Sats", 0), ("All", 1)],
        }
    }
}

/// The arguments of `cash_out`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashOutArgs {
    /// Where the money goes.
    pub address: String,
    /// How much.
    pub amount: CashAmountView,
    /// What the Key app shows for the destination.
    pub label: String,
}

wire_record!(CashOutArgs {
    address,
    amount,
    label
});
wire_record_get!(CashOutArgs {
    address,
    amount,
    label
});

/// The arguments of `sign_batch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchArgs {
    /// The counter's root, 32 bytes.
    pub counter: Vec<u8>,
    /// The first receive index of the batch.
    pub start: u32,
    /// How many addresses.
    pub count: u32,
}

wire_record!(BatchArgs {
    counter,
    start,
    count
});
wire_record_get!(BatchArgs {
    counter,
    start,
    count
});

/// Which alert of the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// No check-in for the grace period.
    MissedCheckIn,
    /// The counter's chain and the owner's copy differ.
    Diverged,
    /// A batch of addresses has waited a day for the counter to hold it
    /// (LK-73).
    BatchUndelivered,
}

wire_unit_enum!(AlertKind {
    MissedCheckIn = 0,
    Diverged = 1,
    BatchUndelivered = 2
});

/// The arguments of `clear_alert`: the alert as `link_state` showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearArgs {
    /// Which alert.
    pub kind: AlertKind,
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The time of the last check-in, the number of the record, or the time
    /// the oldest batch was first sent.
    pub value: u64,
}

wire_record!(ClearArgs {
    kind,
    counter,
    value
});
wire_record_get!(ClearArgs {
    kind,
    counter,
    value
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

/// What `pump` and `stop` return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PumpView {
    /// The lines the node and the driver logged since the last call.
    pub notes: Vec<String>,
    /// The driver has stopped.
    pub stopped: bool,
}

wire_record!(PumpView { notes, stopped });

/// What one turn of the link did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkStepView {
    /// The lines the link and the refund intake logged.
    pub notes: Vec<String>,
    /// Refund requests from the counters that became queued refunds.
    pub queued: u64,
    /// Requests the owner's queue refused; they are in the list of notes.
    pub refused: u64,
    /// Queued refunds removed because the counter withdrew the request.
    pub withdrawn: u64,
    /// Withdrawals that could not be applied.
    pub held: u64,
    /// A request waits for the wallet's view; what the owner is told.
    pub waiting: Option<WordsView>,
    /// Why the intake stopped early, if it did.
    pub stopped_early: Option<String>,
    /// The relay was reached at the last turn.
    pub reached: bool,
}

wire_record!(LinkStepView {
    notes,
    queued,
    refused,
    withdrawn,
    held,
    waiting,
    stopped_early,
    reached
});

/// The state of the wallet's coin view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    /// Ready for the next chain check.
    Ready,
    /// Checking older blocks.
    Rescanning,
    /// Needs a fresh check from the wallet's first block.
    DeepReorg,
}

wire_unit_enum!(ViewKind {
    Ready = 0,
    Rescanning = 1,
    DeepReorg = 2
});

/// Why the node stopped following the chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HaltedView {
    /// What the node logged.
    pub text: String,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(HaltedView { text, words });

/// The answer of the health check (WR-50 to WR-53).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthView {
    /// A `Peers` event has come, so there is an answer.
    pub known: bool,
    /// Every rule holds.
    pub healthy: bool,
    /// Some peer is ahead.
    pub behind: bool,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(HealthView {
    known,
    healthy,
    behind,
    words
});

/// What `status` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    /// The state of the coin view.
    pub view: ViewKind,
    /// While rescanning, the height the check goes up to.
    pub rescan_to: Option<u32>,
    /// What the owner is told about the view.
    pub view_words: WordsView,
    /// What the owner is told about the record of payments received.
    pub receipts_words: WordsView,
    /// The height up to which every block has been checked.
    pub checked_height: u32,
    /// The tip of the node's header chain.
    pub tip_height: u32,
    /// Why the node stopped following the chain, if it did.
    pub halted: Option<HaltedView>,
    /// The last answer of the health rules.
    pub health: HealthView,
    /// What the wallet can offer to a spend, in satoshis.
    pub balance_sats: u64,
    /// A committed approval must be rolled forward into Key 1's state.
    pub recovery_pending: bool,
    /// What the owner is told about that, when it is so.
    pub recovery_words: Option<WordsView>,
    /// The driver has stopped.
    pub stopped: bool,
}

wire_record!(StatusView {
    view,
    rescan_to,
    view_words,
    receipts_words,
    checked_height,
    tip_height,
    halted,
    health,
    balance_sats,
    recovery_pending,
    recovery_words,
    stopped
});

/// Which line about receipts (OC-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptsKind {
    /// The view is not ready.
    NotReady,
    /// The wallet holds no coin.
    NoCoins,
    /// The fee is not known yet.
    FeeUnknown,
    /// The count and the cost.
    Known,
}

wire_unit_enum!(ReceiptsKind {
    NotReady = 0,
    NoCoins = 1,
    FeeUnknown = 2,
    Known = 3
});

/// The line about receipts (OC-16 to OC-18).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptsView {
    /// Which line.
    pub kind: ReceiptsKind,
    /// The coins the wallet holds (0 when not known).
    pub payments: u64,
    /// What spending all of them costs, in satoshis (0 unless known).
    pub cost_sats: u64,
    /// The rate it is worked out at, satoshis per 1000 virtual bytes.
    pub rate_per_kvb: u64,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(ReceiptsView {
    kind,
    payments,
    cost_sats,
    rate_per_kvb,
    words
});

/// The offer to merge the wallet's coins (OC-19).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOfferView {
    /// The coins one merge takes in.
    pub coins: u64,
    /// The coins the wallet holds that a merge may spend.
    pub of: u64,
    /// The rate the merge is built at, satoshis per 1000 virtual bytes.
    pub rate_per_kvb: u64,
    /// The fee of the merge, in satoshis.
    pub cost_sats: u64,
    /// The fee of the same merge at the usual rate.
    pub usual_cost_sats: u64,
    /// What merging now saves.
    pub saving_sats: u64,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(MergeOfferView {
    coins,
    of,
    rate_per_kvb,
    cost_sats,
    usual_cost_sats,
    saving_sats,
    words
});

/// Where a finished payment is (NR-86 to NR-89).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendKind {
    /// Held, and not sent: no way to send is set up.
    Waiting,
    /// Announced to the peers.
    Sent,
    /// Announced back by peers that were not asked.
    SeenBack,
    /// Not seen back; announced again.
    NotSeen,
    /// In a checked block.
    Confirmed,
    /// At the refund depth.
    Done,
    /// Another payment spent its coins.
    Replaced,
    /// The wallet cannot tell what happened.
    Unresolved,
}

wire_unit_enum!(SendKind {
    Waiting = 0,
    Sent = 1,
    SeenBack = 2,
    NotSeen = 3,
    Confirmed = 4,
    Done = 5,
    Replaced = 6,
    Unresolved = 7
});

/// One finished payment the node follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendRow {
    /// The transaction's ID, in hex.
    pub txid: String,
    /// Where it is.
    pub state: SendKind,
    /// The depth for `Confirmed`, the network groups for `SeenBack`, else 0.
    pub n: u32,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(SendRow {
    txid,
    state,
    n,
    words
});

/// An entry left out of the approval just built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedView {
    /// The entry's number.
    pub id: u32,
    /// Why, as the library words it.
    pub reason: String,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(SkippedView { id, reason, words });

/// An approval session built for the Key app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltView {
    /// The session's ID, 32 bytes.
    pub session_id: Vec<u8>,
    /// The request bytes the Key app takes.
    pub request: Vec<u8>,
    /// The entries left out.
    pub skipped: Vec<SkippedView>,
}

wire_record!(BuiltView {
    session_id,
    request,
    skipped
});

/// What happened to one item of an approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeKind {
    /// Signed and checked.
    Delivered,
    /// Sent back to the queue.
    Returned,
    /// The health check was answered.
    Healthy,
}

wire_unit_enum!(OutcomeKind {
    Delivered = 0,
    Returned = 1,
    Healthy = 2
});

/// The outcome of one item when a response was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeView {
    /// The item's number in the session.
    pub item: u32,
    /// What happened.
    pub kind: OutcomeKind,
    /// The finished object's number for `Delivered`, else 0.
    pub finished: u32,
    /// The key holder's reason for `Returned`.
    pub reason: Option<String>,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(OutcomeView {
    item,
    kind,
    finished,
    reason,
    words
});

/// What taking the Key app's response did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenView {
    /// The session's ID.
    pub session_id: Vec<u8>,
    /// What happened to each item.
    pub outcomes: Vec<OutcomeView>,
    /// The payments the node follows now, so the app sees the new one.
    pub sends: Vec<SendRow>,
}

wire_record!(TakenView {
    session_id,
    outcomes,
    sends
});

/// Where a queue entry is (AS-54).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    /// Waiting for the next session.
    Queued,
    /// In an open session.
    InSession,
    /// Sent back; not put in a session by itself.
    Returned,
    /// Signed; the payment waits to be sent.
    Signed,
    /// Reported as sent.
    Sent,
    /// Reported as confirmed.
    Paid,
}

wire_unit_enum!(StageKind {
    Queued = 0,
    InSession = 1,
    Returned = 2,
    Signed = 3,
    Sent = 4,
    Paid = 5
});

/// A queue entry's stage with its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageView {
    /// Where it is.
    pub kind: StageKind,
    /// The reason for `Returned`, the transaction's ID (hex) for `Signed`,
    /// `Sent` and `Paid`, the session's ID (hex) for `InSession`.
    pub detail: Option<String>,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(StageView {
    kind,
    detail,
    words
});

/// A refund in the owner's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundRow {
    /// The entry's number.
    pub id: u32,
    /// The counter's root, 32 bytes.
    pub counter: Vec<u8>,
    /// The sale's receive index.
    pub sale: u32,
    /// Who asked.
    pub cashier: String,
    /// Where the money goes.
    pub address: String,
    /// The amount in satoshis.
    pub sats: u64,
    /// Where the entry is.
    pub stage: StageView,
    /// Whether it still waits for the sale to reach the refund depth.
    pub waiting_for_depth: Option<bool>,
    /// What the owner has been told this sale received, in satoshis.
    pub received_sats: Option<u64>,
}

wire_record!(RefundRow {
    id,
    counter,
    sale,
    cashier,
    address,
    sats,
    stage,
    waiting_for_depth,
    received_sats
});

/// A cash-out in the owner's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashOutRow {
    /// The entry's number.
    pub id: u32,
    /// Where the money goes.
    pub address: String,
    /// How much.
    pub amount: CashAmountView,
    /// What the Key app shows for the destination.
    pub label: String,
    /// Where the entry is.
    pub stage: StageView,
}

wire_record!(CashOutRow {
    id,
    address,
    amount,
    label,
    stage
});

/// The kinds of queue entry that are neither a refund nor a cash-out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKindView {
    /// A batch of addresses for a counter.
    Batch,
    /// A change of the shop's settings.
    SettingsChange,
    /// The pairing of a counter.
    Pairing,
}

wire_unit_enum!(EntryKindView {
    Batch = 0,
    SettingsChange = 1,
    Pairing = 2
});

/// A batch, settings change or pairing in the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRow {
    /// The entry's number.
    pub id: u32,
    /// What it is.
    pub kind: EntryKindView,
    /// Where it is.
    pub stage: StageView,
}

wire_record!(EntryRow { id, kind, stage });

/// The owner's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueView {
    /// The refunds.
    pub refunds: Vec<RefundRow>,
    /// The cash-outs.
    pub cash_outs: Vec<CashOutRow>,
    /// The batches, settings changes and pairings.
    pub others: Vec<EntryRow>,
}

wire_record!(QueueView {
    refunds,
    cash_outs,
    others
});

/// A refund that was queued, with the facts it was queued on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedRefund {
    /// The entry's number.
    pub id: u32,
    /// What the wallet's own view shows the sale received, in satoshis.
    pub received_sats: u64,
    /// The sale has reached the refund depth in the wallet's view.
    pub reached_refund_depth: bool,
}

wire_record!(QueuedRefund {
    id,
    received_sats,
    reached_refund_depth
});

/// What `queue_settings` did (AS-59).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsQueuedView {
    /// A tightening change, signed by the owner alone.
    pub tightening: bool,
    /// The entry's number.
    pub id: u32,
    /// The settings with the owner's signature, for a tightening change.
    pub signed: Option<Vec<u8>>,
}

wire_record!(SettingsQueuedView {
    tightening,
    id,
    signed
});

/// What `deliver` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliverView {
    /// Batches handed to the link.
    pub sent: u32,
    /// Finished objects the link cannot carry (settings and pairings).
    pub waiting: u32,
    /// What went wrong, for the batches left in the outbox.
    pub notes: Vec<String>,
}

wire_record!(DeliverView {
    sent,
    waiting,
    notes
});

/// An alert of the link (LK-62).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertView {
    /// Which alert.
    pub kind: AlertKind,
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The time of the last check-in, or the number of the record.
    pub value: u64,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(AlertView {
    kind,
    counter,
    value,
    words
});

/// What the link says about one counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// Records in the owner's copy of the counter's chain.
    pub records: u64,
    /// The head of the copy, 32 bytes.
    pub head: Vec<u8>,
    /// The number of the latest check-in kept.
    pub check_in_seq: Option<u64>,
    /// Whether the copy holds the latest check-in at its place.
    pub check_in_held: Option<bool>,
    /// When a check-in was last taken.
    pub last_check_in_at: Option<u64>,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(CounterView {
    counter,
    records,
    head,
    check_in_seq,
    check_in_held,
    last_check_in_at,
    words
});

/// New addresses that wait for a counter to hold them (LK-70, LK-73).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchWaitingView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// How many batches wait.
    pub batches: u32,
    /// When the oldest was first sent.
    pub since: u64,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(BatchWaitingView {
    counter,
    batches,
    since,
    words
});

/// The link to the counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkView {
    /// The relay was reached at the last turn.
    pub reachable: bool,
    /// Messages the courier refused since the start.
    pub refused: u64,
    /// The alerts raised now.
    pub alerts: Vec<AlertView>,
    /// Each counter on the list.
    pub counters: Vec<CounterView>,
    /// The counters whose new addresses wait to be held.
    pub batches: Vec<BatchWaitingView>,
}

wire_record!(LinkView {
    reachable,
    refused,
    alerts,
    counters,
    batches
});

/// What a line of the refund requests' list is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKindView {
    /// The queue refused the request.
    Refused,
    /// A withdrawal could not be applied.
    WithdrawalNotApplied,
    /// The app stopped while taking in a request.
    Interrupted,
    /// The counter claims more than the wallet's own view shows.
    ClaimedMoreThanSeen,
}

wire_unit_enum!(NoteKindView {
    Refused = 0,
    WithdrawalNotApplied = 1,
    Interrupted = 2,
    ClaimedMoreThanSeen = 3
});

/// One line of the refund requests' list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundNoteView {
    /// The line's place in the list, for `dismiss_refund_note`.
    pub index: u32,
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The number of the record in the counter's chain.
    pub seq: u64,
    /// The request number.
    pub number: u64,
    /// The sale's receive index.
    pub sale: u32,
    /// What it is about.
    pub kind: NoteKindView,
    /// The request was withdrawn afterwards.
    pub withdrawn: bool,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(RefundNoteView {
    index,
    counter,
    seq,
    number,
    sale,
    kind,
    withdrawn,
    words
});

/// What the owner is told about the old addresses (OC-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OldKind {
    /// No address is set aside.
    None,
    /// Addresses are set aside and no check has run.
    FirstCheckWaiting,
    /// The last check finished days ago.
    Monthly,
    /// A check is running.
    Checking,
    /// The last check did not finish.
    Failed,
}

wire_unit_enum!(OldKind {
    None = 0,
    FirstCheckWaiting = 1,
    Monthly = 2,
    Checking = 3,
    Failed = 4
});

/// The line about old addresses that are checked once a month (OC-22 to
/// OC-26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldAddressesView {
    /// Which line.
    pub kind: OldKind,
    /// How many addresses are set aside.
    pub count: u64,
    /// Whole days since the last check, for `Monthly`.
    pub days_ago: u64,
    /// When the next check is due, for the lines that have a time.
    pub next: Option<u64>,
    /// A deep check is running.
    pub running: bool,
    /// What the owner is told; none when no address is set aside.
    pub words: Option<WordsView>,
}

wire_record!(OldAddressesView {
    kind,
    count,
    days_ago,
    next,
    running,
    words
});

/// What `request_deep_check` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeepRequestView {
    /// A check is asked for; false when no address is set aside.
    pub asked: bool,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(DeepRequestView { asked, words });

// ---- what the node holds, read-only -----------------------------------------------

/// One limit of the shop's settings: what is enforced, in satoshis, and the
/// whole dollars it was shown as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitView {
    /// The enforced amount, in satoshis.
    pub sats: u64,
    /// The display amount, in whole dollars.
    pub display_usd: u32,
}

wire_record!(LimitView { sats, display_usd });

/// A counter's share of every budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CounterShareView {
    /// The counter, 32 bytes.
    pub counter: Vec<u8>,
    /// Its share, in basis points (10,000 is the whole budget).
    pub bps: u32,
}

wire_record!(CounterShareView { counter, bps });
wire_record_get!(CounterShareView { counter, bps });

/// The shop's settings that the owner phone holds as in force, decoded: the
/// fields of `openbmt_core::settings::Settings`, for a read-only view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsHeldView {
    /// The version; it goes up with every signed change.
    pub version: u32,
    /// Light check: largest instant sale.
    pub light_per_sale: LimitView,
    /// Light check: shop-wide outstanding instant total.
    pub light_budget: LimitView,
    /// Own node: largest instant sale.
    pub node_per_sale: LimitView,
    /// Own node: shop-wide outstanding instant total.
    pub node_budget: LimitView,
    /// Box reports only: largest instant sale.
    pub box_per_sale: LimitView,
    /// Box reports only: shop-wide outstanding instant total.
    pub box_budget: LimitView,
    /// How far under the sale amount a payment may fall and still count, in
    /// basis points.
    pub underpayment_tolerance_bps: u32,
    /// Largest single refund.
    pub refund_cap: LimitView,
    /// Largest refunds per day.
    pub refund_daily_cap: LimitView,
    /// The cashier's daily override cap.
    pub cashier_override_daily_cap: LimitView,
    /// The manager's override ceiling.
    pub manager_override_ceiling: LimitView,
    /// Confirmations that settle a sale.
    pub min_confirmations: u8,
    /// Confirmations at which a sale is final and a refund may be paid.
    pub final_confirmations: u8,
    /// The longest age of the owner phone's heartbeat that still allows an
    /// instant sale, in seconds.
    pub heartbeat_max_age_secs: u32,
    /// Tor is on.
    pub tor_on: bool,
    /// Customers can join the Wi-Fi the counter is on, or the owner is unsure.
    pub customer_wifi: bool,
    /// The smallest sale a counter opens, in satoshis; 0 is no floor.
    pub min_sale_sats: u64,
    /// The devices that may write records, 32 bytes each.
    pub devices: Vec<Vec<u8>>,
    /// Each counter's share of every budget.
    pub counters: Vec<CounterShareView>,
    /// The devices that are Boxes, 32 bytes each.
    pub boxes: Vec<Vec<u8>>,
    /// The key of the shop's own Lightning node, 33 bytes, when it names one.
    pub lightning_node: Option<Vec<u8>>,
}

wire_record!(SettingsHeldView {
    version,
    light_per_sale,
    light_budget,
    node_per_sale,
    node_budget,
    box_per_sale,
    box_budget,
    underpayment_tolerance_bps,
    refund_cap,
    refund_daily_cap,
    cashier_override_daily_cap,
    manager_override_ceiling,
    min_confirmations,
    final_confirmations,
    heartbeat_max_age_secs,
    tor_on,
    customer_wifi,
    min_sale_sats,
    devices,
    counters,
    boxes,
    lightning_node
});

/// The kinds of signed object that wait in the outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxKind {
    /// A signed transaction.
    Transaction,
    /// A batch of addresses for a counter.
    Batch,
    /// Signed settings.
    Settings,
    /// A pairing.
    Pairing,
    /// A Box's welcome.
    BoxWelcome,
}

wire_unit_enum!(OutboxKind {
    Transaction = 0,
    Batch = 1,
    Settings = 2,
    Pairing = 3,
    BoxWelcome = 4
});

/// One signed object that waits to be delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    /// The object's number.
    pub id: u32,
    /// What it is.
    pub kind: OutboxKind,
    /// The transaction's ID, for a transaction.
    pub txid: Option<String>,
    /// The counter, for a batch that reads back.
    pub counter: Option<Vec<u8>>,
    /// The version, for settings that read back.
    pub version: Option<u32>,
    /// When the owner phone signed it, in seconds since 1970: known for a
    /// batch only, from the range the phone issued. The node keeps no time
    /// for the others.
    pub signed_at: Option<u64>,
    /// The size of the object, in bytes.
    pub size: u32,
    /// What the owner is told.
    pub words: WordsView,
}

wire_record!(OutboxRow {
    id,
    kind,
    txid,
    counter,
    version,
    signed_at,
    size,
    words
});
