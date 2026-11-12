//! The counter: its sales, its ledger and its screens (CR-01 to CR-42).
//!
//! A [`Counter`] owns the state a counter must keep and nothing that it must
//! fetch. Every method that changes anything takes a [`Store`]. The rule for
//! all of them is the same: build the new value, hand its bytes to the
//! store, and put the value in place only if the store said `Ok`. Where a
//! step writes more than once, the order is chosen so that stopping between
//! any two writes leaves the shop on the safe side (CR-41); each method says
//! what its order is and why.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bitcoin::consensus::deserialize;
use bitcoin::hashes::Hash;
use bitcoin::{ScriptBuf, Transaction, Txid};
use openbmt_core::approval::CounterConfirmation;
use openbmt_core::batch::{
    AddError, BatchError, BatchPolicy, CounterBatches, PinnedRoots, SignedBatch,
};
use openbmt_core::command::{Action, SignedCommand};
use openbmt_core::heartbeat::SignedHeartbeat;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pinned_wallet::SignedRange;
use openbmt_core::policy::{
    amount_at_risk, evaluate, CheckMode, Decision, Exposure, ExposureError, ExposureWriteError,
    Heartbeat, Inputs, InstantPolicy, PauseStatus, PauseTracker, PaymentId,
};
use openbmt_core::records::{DeviceList, Record, RecordKind, Verifier};
use openbmt_core::settings::{SettingNote, Settings};
use openbmt_watch::{
    HeaderChain, Health, NodeOnlyHealth, OwnNodeHealth, Problem, Refusal, Sighted, Source,
    WellFormedBlock,
};

use crate::batch_held::BatchHeld;
use crate::confirm_held::ConfirmHeld;
use crate::desk::{valid_name, Desk, OverrideEntry, Role};
use crate::events::{final_payments, Event};
use crate::heartbeat::{judge_heartbeat, HeartbeatView, NotVerified};
use crate::money::{
    sats_for, tip_for_percent, value_of, Amount, Currency, Quote, Rate, RateSource, TipChoice,
    MAX_SATS,
};
use crate::own::{OwnPairing, OwnSource};
use crate::range_held::{range_digest, RangeHeld};
use crate::recordlog::{RecordLog, MAX_UNSENT_IN_MEMORY, READ_CHUNK};
use crate::refund::{
    dust_limit, refund_text_ok, sats_floor, Ask, CashRefund, EntryKind, RefundAsk, RefundEntry,
    RefundRequest, RefundState, RefundView, MAX_REFUNDS_PER_SALE,
};
use crate::refusal::{
    BatchTakeRefused, BlockError, CancelRefused, CommandRefused, HeartbeatRefused, JudgeError,
    LightningRefused, LoadError, NoteRefused, OpenRefused, OverrideRefused, RangeTakeRefused,
    RefundRefused, RequoteRefused, SightingError, StaffRefused, TotalsRefused, WithdrawRefused,
};
use crate::sale::{End, LightningPaid, OverrideMark, OwnCheck, PosState, RequestDays, Sale};
use crate::screen::{Next, Report, Screen, State, Why};
use crate::staff::StaffEvent;
use crate::store::{Slot, Store, StoreFault};
use crate::totals::{fold_day, record_time, DayTotals, TOTALS_BLOCK, TOTALS_MAX_RECORDS};
use crate::{
    DUST_SATS, MAX_LATE_PAYMENTS, MAX_QUOTES, MAX_SETTLE_PER_CALL, SETTLE_SECS, STUCK_SECS,
};
use openbmt_core::policy::{OffReason, WaitReason};

/// What a counter was paired with: the roots it pinned, the wallet it
/// belongs to and the rule for the size of a batch.
#[derive(Debug, Clone, Copy)]
pub struct Pairing {
    /// The owner and Key app roots pinned at pairing.
    pub roots: PinnedRoots,
    /// The checksum of the wallet description.
    pub wallet_checksum: [u8; 32],
    /// How big a batch must be.
    pub batch_policy: BatchPolicy,
}

/// What the store holds, as read back at start (CR-42). A slot that holds
/// nothing is `None` (a counter that has never run).
#[derive(Debug, Clone, Default)]
pub struct Stored<'a> {
    /// The address batches (`OBMC`).
    pub batches: Option<&'a [u8]>,
    /// The ranges of a counter paired with the shop's own wallet (`OBM;`).
    pub ranges: Option<&'a [u8]>,
    /// The exposure ledger (`OBME`).
    pub exposure: Option<&'a [u8]>,
    /// The pause tracker (`OBMP`).
    pub pause: Option<&'a [u8]>,
    /// The desk (`OBMG`).
    pub desk: Option<&'a [u8]>,
    /// Each sale (`OBML`) with the index of the slot it was in.
    pub sales: Vec<(u32, &'a [u8])>,
}

/// What a call of [`Counter::settle`] did (CR-77).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settled {
    /// Sales removed.
    pub count: usize,
    /// Addresses that left the counter's watch list (CR-79).
    pub released: usize,
    /// More sales are ready to be settled than this call took.
    pub remaining: bool,
}

/// What opening a sale needs to know (CR-01, CR-04, CR-09).
#[derive(Debug, Clone, Copy)]
pub struct OpenFacts {
    /// The answer of the watcher's health check, now.
    pub health: Health,
    /// A rescan is pending after a restore or a reset (CR-09).
    pub rescan_pending: bool,
    /// The time, in seconds.
    pub now: u64,
}

/// A sale that was opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// The receive index.
    pub index: u32,
    /// The address script to show the customer.
    pub script: ScriptBuf,
    /// What the first quote asks for, in satoshis.
    pub needs_sats: u64,
    /// The first quote.
    pub quote: Quote,
}

/// What a sale may carry besides its amount: a note (CR-206) and the days of
/// a request to send (CR-205).
#[derive(Debug, Clone, Copy, Default)]
pub struct SaleExtras<'a> {
    /// The note: plain text of 1 to 64 characters.
    pub note: Option<&'a str>,
    /// Make the sale a request to send that lasts these days.
    pub request: Option<RequestDays>,
}

/// What judging a sale needs to know (CR-12).
pub struct Facts<'a, C: HeaderChain + ?Sized> {
    /// The header chain.
    pub chain: &'a C,
    /// The answer of the watcher's health check, now.
    pub health: Health,
    /// The owner-phone heartbeat, as [`Counter::heartbeat_for_policy`]
    /// gives it (CR-50, CR-55).
    pub heartbeat: Heartbeat,
    /// The order is a remote or online order.
    pub remote_order: bool,
    /// The time, in seconds.
    pub now: u64,
}

/// What the counter keeps of a swap payment (CR-221): the facts the node
/// reports when the provider's bitcoin is locked and the claim is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapPaid {
    /// The payment hash of the provider's invoice.
    pub payment_hash: [u8; 32],
    /// What the customer paid, in satoshis: the invoice's amount.
    pub invoice_sats: u64,
    /// What the claim pays the sale's own address, in satoshis.
    pub received_sats: u64,
}

/// The person who overrides (CR-21). The caller has already checked the PIN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    /// Cashier or manager.
    pub role: Role,
    /// Who.
    pub name: String,
}

/// What offering one transaction did (see [`Counter::sighting`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sightings {
    /// For each sale the transaction touched, what the watcher said. Sales
    /// that ignored it are not listed. A refusal from one sale does not stop
    /// the others.
    pub results: Vec<(u32, Result<Sighted, Refusal>)>,
    /// Late payments (CR-19), stored first.
    pub reports: Vec<Report>,
}

/// What one block did to one sale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEffect {
    /// Payments the block proved for the sale.
    pub payments: usize,
    /// Spends of the sale's coins by other transactions.
    pub spends: usize,
}

/// What taking one block did (see [`Counter::block`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlockTaken {
    /// For each sale the block touched, what it did or why the watcher
    /// refused.
    pub results: Vec<(u32, Result<BlockEffect, Refusal>)>,
    /// Late payments (CR-19), stored first.
    pub reports: Vec<Report>,
}

/// What an override did (see [`Counter::override_sale`]).
#[derive(Debug)]
pub struct Overridden<E> {
    /// What the POS and the books must be told. The sale is judged before
    /// the override is tried, and what that judging reports is reported
    /// once: it is here whether or not the override was then refused.
    pub reports: Vec<Report>,
    /// Whether the override was made.
    pub result: Result<(), OverrideRefused<E>>,
}

/// CR-13: the id of a sale in the exposure ledger: the word `sale`, the
/// index (u32 LE) and zeros. It is under the sale, not under a transaction,
/// because a fee bump changes the transaction.
pub fn sale_payment_id(index: u32) -> PaymentId {
    let mut id = [0u8; 32];
    let index_bytes = index.to_le_bytes();
    let head = b"sale".iter().chain(index_bytes.iter());
    for (dst, src) in id.iter_mut().zip(head) {
        *dst = *src;
    }
    PaymentId(id)
}

/// Why accepting at once did not happen (CR-13).
enum AcceptFail {
    SliceFull,
    NotStored,
}

fn put<S: Store>(store: &mut S, slot: Slot, bytes: &[u8]) -> Result<(), StoreFault<S::Error>> {
    store.put(slot, bytes).map_err(StoreFault::Write)
}

fn screen_of(state: State, why: Why, next: Next) -> Screen {
    Screen {
        state,
        why,
        next,
        received_sats: 0,
        needs_sats: 0,
        stuck: false,
        late: false,
        check_mode: None,
        own_node_saw: false,
        own_node_counted: false,
        conflict_seen: false,
        reopened: false,
        reports: Vec::new(),
    }
}

fn ended_screen(sale: &Sale, end: End) -> Screen {
    let (state, why) = match end {
        End::Expired => (State::Expired, Why::EndedExpired),
        End::Cancelled => (State::Cancelled, Why::EndedCancelled),
    };
    let mut screen = screen_of(state, why, Next::Nothing);
    screen.needs_sats = sale.needs_sats();
    screen.late = sale.is_late();
    screen
}

/// The counter.
#[derive(Debug)]
pub struct Counter {
    identity: Identity,
    /// The owner root pinned at pairing: the one that signs commands and
    /// heartbeats, whichever way the counter was paired.
    owner: DeviceId,
    settings: Settings,
    /// The batches of a counter paired with the Key app. A counter paired
    /// with the shop's own wallet keeps this empty and never adds to it.
    batches: CounterBatches,
    /// The address source of a counter paired with the shop's own wallet
    /// (CR-234). `None` for a counter paired with the Key app, whose
    /// addresses come from `batches`.
    own: Option<OwnSource>,
    exposure: Exposure,
    pause: PauseTracker,
    /// The pause changed in memory and its write failed. It is written again
    /// before any payment is accepted at once (CR-14).
    pause_unsaved: bool,
    /// Sales whose cancel is already in the pause tracker in memory but not
    /// yet marked on the sale, so that a retry does not count it twice.
    cancel_applied: BTreeSet<u32>,
    /// Sales whose refund entries were taken from the counter's records and
    /// differ from what is stored, so that the stored sale is written again
    /// before the next refund call acts (CR-73).
    refund_unsaved: BTreeSet<u32>,
    records: RecordLog,
    /// CR-73: each held sale's refund entries as its records say, kept as
    /// the records are written so that the history need not be read again.
    refund_book: BTreeMap<u32, Vec<RefundEntry>>,
    /// CR-73: the highest refund request number any record holds.
    highest_refund: u64,
    desk: Desk,
    /// CR-54: what the last heartbeat taken in this run came to. Not stored:
    /// after a restart there is none until a new one comes.
    heartbeat: Option<HeartbeatView>,
    sales: BTreeMap<u32, Sale>,
    /// Scripts of sales that ended or were reported final. They stay watched
    /// for late payments (CR-07, CR-19).
    archived: BTreeMap<ScriptBuf, u32>,
    /// Sales whose stored form did not read back (CR-42).
    unreadable: BTreeSet<u32>,
    /// CR-75: sales refused for being below the smallest bitcoin sale since
    /// this counter started. Not stored.
    refused_below_minimum: u64,
    /// CR-76: how many of this counter's records the owner holds, as the
    /// link last took it (LK-55). Not stored: after a start there is none.
    owner_mark: Option<u64>,
    /// CR-76: for each sale the counter holds, the number and the time of the
    /// newest record that names it. Built from the records at start.
    last_record: BTreeMap<u32, (u64, u64)>,
    /// CR-162: the highest settings version named by a `Setting` record in
    /// the counter's own chain. Built from the records at start.
    settings_recorded: Option<u32>,
    /// CR-167, CR-168: the batches the counter's records say it took, each
    /// range (first index, first index after) with its digest. Built from the
    /// records at start.
    held_batches: BTreeMap<(u32, u32), [u8; 32]>,
    /// CR-235: the same for the ranges the counter's records say it took.
    held_ranges: BTreeMap<(u32, u32), [u8; 32]>,
    /// CR-221: sales whose swap provider has locked the bitcoin and whose
    /// claim is not yet in a block. Such a sale does not expire. Not stored:
    /// the node says it again after a start.
    swap_pending: BTreeSet<u32>,
}

impl Counter {
    /// A counter that has never run, for the identity it signs records with
    /// and the roots it was paired with. `settings` are plain: the caller
    /// has checked them with `SettingsStore`.
    pub fn new(identity: Identity, pairing: Pairing, settings: Settings) -> Self {
        let me = identity.device_id();
        Self {
            batches: CounterBatches::new(
                me,
                pairing.roots,
                pairing.wallet_checksum,
                pairing.batch_policy,
            ),
            identity,
            owner: pairing.roots.owner,
            own: None,
            settings,
            exposure: Exposure::new(),
            pause: PauseTracker::new(),
            pause_unsaved: false,
            cancel_applied: BTreeSet::new(),
            refund_unsaved: BTreeSet::new(),
            records: RecordLog::new(),
            refund_book: BTreeMap::new(),
            highest_refund: 0,
            desk: Desk::new(),
            heartbeat: None,
            sales: BTreeMap::new(),
            archived: BTreeMap::new(),
            unreadable: BTreeSet::new(),
            refused_below_minimum: 0,
            owner_mark: None,
            last_record: BTreeMap::new(),
            settings_recorded: None,
            held_batches: BTreeMap::new(),
            held_ranges: BTreeMap::new(),
            swap_pending: BTreeSet::new(),
        }
    }

    /// A counter paired with the shop's own wallet that has never run
    /// (CR-234). It has no Key app: the owner root signs its commands,
    /// heartbeats and settings, and its addresses come from the wallet. The
    /// ranges are those of the pairing's first range.
    pub fn new_own(
        identity: Identity,
        pairing: &OwnPairing,
        settings: Settings,
    ) -> Result<Self, LoadError> {
        let me = identity.device_id();
        let own = OwnSource::open(me, pairing, None)?;
        // The counter's own key stands where no Key app is. It never signs a
        // batch, and none is ever added to this empty set.
        let roots = PinnedRoots {
            owner: pairing.owner,
            approver: me,
        };
        let pairing_keyapp = Pairing {
            roots,
            wallet_checksum: pairing.wallet.checksum32(),
            batch_policy: pairing.batch_policy,
        };
        let mut counter = Self::new(identity, pairing_keyapp, settings);
        counter.own = Some(own);
        Ok(counter)
    }

    /// CR-42, CR-234: start a counter paired with the shop's own wallet from
    /// what the store holds, as [`Counter::load`] does for the Key app. The
    /// slot of ranges is read and every range in it is checked again against
    /// the owner root and this counter; a slot that does not read or check
    /// refuses the start. With no slot the ledger is made from the pairing's
    /// first range.
    pub fn load_own<S: Store>(
        identity: Identity,
        pairing: &OwnPairing,
        settings: Settings,
        stored: Stored<'_>,
        store: &S,
    ) -> Result<Self, LoadError>
    where
        S::Error: std::fmt::Display,
    {
        let me = identity.device_id();
        let own = OwnSource::open(me, pairing, stored.ranges)?;
        let roots = PinnedRoots {
            owner: pairing.owner,
            approver: me,
        };
        let pairing_keyapp = Pairing {
            roots,
            wallet_checksum: pairing.wallet.checksum32(),
            batch_policy: pairing.batch_policy,
        };
        // The slot of batches is not read: this counter has none.
        let stored = Stored {
            batches: None,
            ..stored
        };
        let mut counter = Self::load(identity, pairing_keyapp, settings, stored, store)?;
        counter.own = Some(own);
        Ok(counter)
    }

    /// CR-42: start from what the store holds. Every slot is checked whole.
    /// A slot of batches, ledger, pause or desk that does not read back
    /// refuses the start: running without it could hand an address out
    /// twice or forget exposure. A sale that does not read back is kept as
    /// unreadable and shown as `NeedsAPerson`, never as paid. The records
    /// are read from `store` in order and verified as a chain signed by this
    /// counter, one chunk at a time: only the newest are kept in memory
    /// (CR-81).
    pub fn load<S: Store>(
        identity: Identity,
        pairing: Pairing,
        settings: Settings,
        stored: Stored<'_>,
        store: &S,
    ) -> Result<Self, LoadError>
    where
        S::Error: std::fmt::Display,
    {
        let me = identity.device_id();
        let mut counter = Self::new(identity, pairing, settings);
        if let Some(bytes) = stored.batches {
            counter.batches = CounterBatches::from_bytes(
                bytes,
                me,
                pairing.roots,
                pairing.wallet_checksum,
                pairing.batch_policy,
            )
            .map_err(LoadError::Batches)?;
        }
        if let Some(bytes) = stored.exposure {
            counter.exposure = Exposure::from_bytes(bytes).map_err(LoadError::Exposure)?;
        }
        if let Some(bytes) = stored.pause {
            counter.pause = PauseTracker::from_bytes(bytes).map_err(LoadError::Pause)?;
        }
        if let Some(bytes) = stored.desk {
            counter.desk = Desk::from_bytes(bytes).map_err(LoadError::Desk)?;
        }
        let (log, last, book, highest, setting, held, held_ranges) = Self::read_chain(me, store)?;
        counter.records = log;
        counter.settings_recorded = setting;
        counter.held_batches = held;
        counter.held_ranges = held_ranges;
        for (slot_index, bytes) in stored.sales {
            match Sale::from_bytes(bytes) {
                Ok(sale) if sale.index == slot_index => counter.install(sale),
                _ => {
                    counter.unreadable.insert(slot_index);
                }
            }
        }
        // CR-73: the counter's own records are the truth about refunds.
        counter.refund_book = book;
        counter
            .refund_book
            .retain(|i, _| counter.sales.contains_key(i));
        counter.highest_refund = highest;
        counter.adopt_refunds();
        // CR-76: the newest record that names each sale the counter holds.
        counter.last_record = last;
        counter
            .last_record
            .retain(|i, _| counter.sales.contains_key(i));
        Ok(counter)
    }

    /// CR-42, CR-81: read the whole record log in order, check it as a chain
    /// signed by this counter, and keep only its newest records. Returns the
    /// log, the newest record naming each sale, the refund entries of each
    /// sale, the highest refund number and the highest settings version a
    /// `Setting` record names.
    #[allow(clippy::type_complexity)]
    fn read_chain<S: Store>(
        me: DeviceId,
        store: &S,
    ) -> Result<
        (
            RecordLog,
            BTreeMap<u32, (u64, u64)>,
            BTreeMap<u32, Vec<RefundEntry>>,
            u64,
            Option<u32>,
            BTreeMap<(u32, u32), [u8; 32]>,
            BTreeMap<(u32, u32), [u8; 32]>,
        ),
        LoadError,
    >
    where
        S::Error: std::fmt::Display,
    {
        let devices = DeviceList::new([me]);
        let mut check = Verifier::new(&devices);
        let mut tail: VecDeque<Record> = VecDeque::new();
        let mut last: BTreeMap<u32, (u64, u64)> = BTreeMap::new();
        let mut book: BTreeMap<u32, Vec<RefundEntry>> = BTreeMap::new();
        let mut highest = 0u64;
        let mut setting: Option<u32> = None;
        let mut held: BTreeMap<(u32, u32), [u8; 32]> = BTreeMap::new();
        let mut held_ranges: BTreeMap<(u32, u32), [u8; 32]> = BTreeMap::new();
        let keep = usize::try_from(MAX_UNSENT_IN_MEMORY).unwrap_or(usize::MAX);
        loop {
            let chunk = store
                .read_records(check.len(), READ_CHUNK)
                .map_err(|e| LoadError::RecordsUnread(e.to_string()))?;
            if chunk.is_empty() {
                break;
            }
            for bytes in chunk {
                let record = Record::from_bytes(&bytes).map_err(|e| {
                    LoadError::RecordsUnread(format!("record {}: {e}", check.len()))
                })?;
                check.push(&record).map_err(LoadError::Records)?;
                if record.kind == RecordKind::Setting {
                    // CR-162: a `Setting` record names the settings the
                    // counter took; one that does not read names none.
                    // CR-167: or a batch the counter took. The two bodies
                    // differ in length (37 and 41 bytes) and both readers
                    // are strict, so a body reads as at most one of them.
                    if let Ok(note) = SettingNote::from_body(&record.body) {
                        setting = setting.max(Some(note.version));
                    } else if let Ok(batch) = BatchHeld::decode(&record.body) {
                        held.insert((batch.start, batch.end), batch.digest);
                    } else if let Ok(range) = RangeHeld::decode(&record.body) {
                        // CR-235: or a range, with its own tag byte.
                        held_ranges.insert((range.start, range.end), range.digest);
                    }
                } else if !matches!(record.kind, RecordKind::CheckIn | RecordKind::Pairing) {
                    if let Ok(event) = Event::decode(&record.body) {
                        let at = last.get(&event.index()).map_or(0, |(_, at)| *at);
                        last.insert(event.index(), (record.seq, at.max(event.at())));
                        if record.kind == RecordKind::Refund {
                            fold_refund(&mut book, &mut highest, event);
                        }
                    }
                }
                tail.push_back(record);
                if tail.len() > keep {
                    tail.pop_front();
                }
            }
        }
        let log = RecordLog::from_parts(check.len(), check.head(), tail);
        Ok((log, last, book, highest, setting, held, held_ranges))
    }

    /// CR-75: how many sales this counter has refused for being below the
    /// smallest bitcoin sale since it started. Kept in memory only.
    pub fn refused_below_minimum(&self) -> u64 {
        self.refused_below_minimum
    }

    /// New settings, already checked by the caller. A sale that is already
    /// open keeps the underpayment tolerance it was opened with (CR-160);
    /// everything else the counter reads from the settings is read again at
    /// the next call that needs it.
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// The settings in force.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// CR-165: whether the settings in force still list this counter, on the
    /// device list and among the counters. After settings that remove it no
    /// new sale is opened.
    pub fn is_listed(&self) -> bool {
        let me = self.identity.device_id();
        self.settings.devices.contains(&me) && self.settings.counters.contains_key(&me)
    }

    /// CR-162: the highest settings version a `Setting` record of the
    /// counter's own chain names, or `None` when none does.
    pub fn settings_recorded(&self) -> Option<u32> {
        self.settings_recorded
    }

    /// CR-162: append a `Setting` record that names settings the counter has
    /// taken: the version, whether the change loosened or tightened, and the
    /// hash. It goes through the same stored path as every other record
    /// (CR-40); on an error the chain is unchanged. Returns the record.
    pub fn record_setting<S: Store>(
        &mut self,
        note: SettingNote,
        store: &mut S,
    ) -> Result<Record, StoreFault<S::Error>> {
        let record = self.append_record(store, RecordKind::Setting, note.to_body())?;
        self.settings_recorded = self.settings_recorded.max(Some(note.version));
        Ok(record)
    }

    /// CR-241: sign the confirmation of AP-04 for `settings_hash` with the
    /// counter's identity key and append it as a `Setting` record. The caller
    /// asks this only after the manager's PIN was right. It does not change
    /// the settings in force. On an error the chain is unchanged. Returns the
    /// record.
    pub fn record_confirmation<S: Store>(
        &mut self,
        settings_hash: &[u8; 32],
        store: &mut S,
    ) -> Result<Record, StoreFault<S::Error>> {
        let held = ConfirmHeld {
            settings_hash: *settings_hash,
            sig: CounterConfirmation::sign(&self.identity, settings_hash).sig,
        };
        self.append_record(store, held.kind(), held.encode())
    }

    /// Take a new signed batch of addresses. The batches as they will be are
    /// handed to the store first; on any error nothing changed.
    pub fn add_batch<S: Store>(
        &mut self,
        signed: SignedBatch,
        store: &mut S,
    ) -> Result<(), AddError<S::Error>> {
        self.batches
            .add(signed, |bytes| store.put(Slot::Batches, bytes))
    }

    /// CR-167, CR-168: take a batch the owner sent, and say so in a record.
    /// A batch the records already show as taken (the same range and digest)
    /// is taken as nothing, and writes no second record. Otherwise the batch
    /// is checked as a copy of the batches would take it, with nothing
    /// stored; the record is written (CR-40: before the counter acts on the
    /// batch); and the batches are stored last. If that last write fails the
    /// record stays, the batches are as they were, and a second try of the
    /// same batch stores it without a second record.
    pub fn take_batch<S: Store>(
        &mut self,
        signed: SignedBatch,
        store: &mut S,
    ) -> Result<(), BatchTakeRefused<S::Error>> {
        if self.own.is_some() {
            return Err(BatchTakeRefused::OwnWallet);
        }
        let held = BatchHeld::of(&signed);
        let recorded =
            held.is_some_and(|h| self.held_batches.get(&(h.start, h.end)) == Some(&h.digest));
        // A copy takes the batch first, so that a batch that would be
        // refused is not written down as held. A batch the copy finds
        // overlapping, and that the records show as taken, is the batch
        // itself again.
        let mut probe = self.batches.clone();
        match probe.add(signed.clone(), |_| Ok::<(), S::Error>(())) {
            Err(AddError::Refused(BatchError::OverlapsExisting)) if recorded => return Ok(()),
            Err(e) => return Err(BatchTakeRefused::Add(e)),
            Ok(()) => {}
        }
        if let (false, Some(h)) = (recorded, held) {
            self.append_record(store, h.kind(), h.encode())
                .map_err(BatchTakeRefused::Record)?;
            self.held_batches.insert((h.start, h.end), h.digest);
        }
        self.batches
            .add(signed, |bytes| store.put(Slot::Batches, bytes))
            .map_err(BatchTakeRefused::Add)
    }

    /// CR-235: take a signed range of indexes the owner sent, and say so in
    /// a record, in the other order to [`Counter::take_batch`]: check, store,
    /// record. The record tells the owner the range arrived (LK-77), so it is
    /// never written before the range is stored. A counter paired with the
    /// Key app takes no range. The range must pass CR-232 (the owner root's
    /// signature, this counter, a valid shape, at least the smallest range)
    /// and overlap no index handed out and no range still in use (WL-25,
    /// WL-26). It is checked on a copy of the ledger with nothing stored;
    /// then the ranges are stored; then the record is written. A range the
    /// ledger already holds and the records show is taken as nothing. One the
    /// ledger holds with no record (the record failed) gets its record and
    /// nothing else changes. One the records show but the ledger does not
    /// hold (a counter that stopped between the two in an older version) is
    /// stored and no second record is written.
    pub fn take_range<S: Store>(
        &mut self,
        signed: SignedRange,
        store: &mut S,
    ) -> Result<(), RangeTakeRefused<S::Error>> {
        let me = self.identity.device_id();
        let Some(own) = self.own.as_ref() else {
            return Err(RangeTakeRefused::KeyApp);
        };
        let held = RangeHeld::of(&signed);
        let recorded = self.held_ranges.get(&(held.start, held.end)) == Some(&held.digest);
        own.check(&signed, &me).map_err(RangeTakeRefused::Range)?;
        let stored = own
            .ledger
            .signed()
            .iter()
            .any(|s| range_digest(s) == held.digest);
        if stored {
            if !recorded {
                self.append_record(store, held.kind(), held.encode())
                    .map_err(RangeTakeRefused::Record)?;
                self.held_ranges.insert((held.start, held.end), held.digest);
            }
            return Ok(());
        }
        let mut next = own.clone();
        next.ledger
            .accept(&signed)
            .map_err(|e| RangeTakeRefused::Range(e.into()))?;
        store
            .put(Slot::Ranges, &next.ledger.to_bytes())
            .map_err(RangeTakeRefused::Store)?;
        self.own = Some(next);
        if !recorded {
            self.append_record(store, held.kind(), held.encode())
                .map_err(RangeTakeRefused::Record)?;
            self.held_ranges.insert((held.start, held.end), held.digest);
        }
        Ok(())
    }

    // Getters.

    /// CR-14: the cancel alarm is up.
    pub fn alarm(&self) -> bool {
        self.desk.alarm()
    }

    /// The pause state at `now`.
    pub fn pause_status(&self, now: u64) -> PauseStatus {
        self.pause.status(now)
    }

    /// The pause could not be saved when it was last changed (CR-14).
    pub fn pause_unsaved(&self) -> bool {
        self.pause_unsaved
    }

    /// What is outstanding in the exposure ledger.
    pub fn exposure_outstanding(&self) -> u64 {
        self.exposure.outstanding()
    }

    /// One sale.
    pub fn sale(&self, index: u32) -> Option<&Sale> {
        self.sales.get(&index)
    }

    /// How many sales the counter holds in memory, live or ended. A settled
    /// sale (CR-76) is not counted.
    pub fn held_sales(&self) -> usize {
        self.sales.len()
    }

    /// The sales the counter holds a newest-record entry for (CR-76).
    #[cfg(test)]
    pub(crate) fn indexed_sales(&self) -> usize {
        self.last_record.len()
    }

    /// The indexes of sales whose stored form did not read back (CR-42).
    pub fn unreadable(&self) -> impl Iterator<Item = u32> + '_ {
        self.unreadable.iter().copied()
    }

    /// The counter's own state.
    pub fn desk(&self) -> &Desk {
        &self.desk
    }

    /// CR-50 to CR-54: what the last heartbeat taken since this counter
    /// started came to, if one was taken.
    pub fn heartbeat_view(&self) -> Option<HeartbeatView> {
        self.heartbeat
    }

    /// CR-76: say how many of this counter's records the owner holds (the
    /// owner's mark, LK-55). The link calls it each time it takes a mark,
    /// and it replaces the last one, a lower number included. A number above
    /// the counter's own length is taken as the length. Nothing is settled
    /// until a mark has been given.
    pub fn set_owner_mark(&mut self, records_held: u64) {
        self.owner_mark = Some(records_held.min(self.records.len()));
        self.records.trim(self.owner_mark);
    }

    /// The owner's mark as last given, if one was.
    pub fn owner_mark(&self) -> Option<u64> {
        self.owner_mark
    }

    /// CR-81: how many records the counter has written.
    pub fn records_len(&self) -> u64 {
        self.records.len()
    }

    /// CR-81: the hash of the newest record, or `GENESIS` for none.
    pub fn head(&self) -> [u8; 32] {
        self.records.head()
    }

    /// CR-81: the number of the oldest record held in memory. Records below
    /// it are read from the store.
    pub fn records_base(&self) -> u64 {
        self.records.base()
    }

    /// CR-81: how many records are held in memory.
    pub fn records_in_memory(&self) -> usize {
        self.records.in_memory()
    }

    /// CR-81: the record numbered `n`, from memory or, below the base, from
    /// the store; `None` when there is no such record yet. A record read
    /// from the store is checked (CR-42); a read that fails or a record that
    /// is damaged is a fault and changes nothing.
    pub fn record_at<S: Store>(
        &self,
        n: u64,
        store: &S,
    ) -> Result<Option<Record>, StoreFault<S::Error>> {
        self.records.record_at(store, &self.device_id(), n)
    }

    /// CR-81: up to `max` records from number `from` on, oldest first, at
    /// most [`crate::MAX_READ`] in one call. Fewer come back at the end of
    /// the chain, none past it. Records below the base are read from the
    /// store and checked, as in [`Counter::record_at`].
    pub fn records_from<S: Store>(
        &self,
        from: u64,
        max: usize,
        store: &S,
    ) -> Result<Vec<Record>, StoreFault<S::Error>> {
        self.records
            .records_from(store, &self.device_id(), from, max)
    }

    /// LK-55: the hash of the record before number `n`: the head when `n` is
    /// the number of records, `GENESIS` when it is zero, `None` when it is
    /// above the number of records. Below the base it is read from the store.
    pub fn hash_at<S: Store>(
        &self,
        n: u64,
        store: &S,
    ) -> Result<Option<[u8; 32]>, StoreFault<S::Error>> {
        self.records.hash_at(store, &self.device_id(), n)
    }

    /// CR-143: write a change to the cashiers the counter keeps as a record.
    /// Names are checked first; the record goes through the same stored path
    /// as every other (CR-40), and on an error the chain is unchanged.
    pub fn record_staff<S: Store>(
        &mut self,
        event: &StaffEvent,
        store: &mut S,
    ) -> Result<Record, StaffRefused<S::Error>> {
        if !refund_text_ok(&event.name) {
            return Err(StaffRefused::BadName("cashier's name"));
        }
        if !refund_text_ok(&event.by) {
            return Err(StaffRefused::BadName("manager's name"));
        }
        self.append_record(store, event.kind(), event.encode())
            .map_err(StaffRefused::Store)
    }

    /// CR-152 to CR-155: the totals of the day `[from, to)` (seconds since
    /// 1970, on the caller's clock and zone), counted from the counter's own
    /// records. They are read from the newest back, a block at a time, until
    /// a block holds events and none is from `from` on; a day that needs more
    /// than [`TOTALS_MAX_RECORDS`] records is refused. `people` asks for the
    /// lines per person.
    pub fn day_totals<S: Store>(
        &self,
        from: u64,
        to: u64,
        people: bool,
        store: &S,
    ) -> Result<DayTotals, TotalsRefused<S::Error>> {
        self.day_totals_within(from, to, people, TOTALS_MAX_RECORDS, store)
    }

    /// [`Counter::day_totals`] with the most records one count reads given.
    pub(crate) fn day_totals_within<S: Store>(
        &self,
        from: u64,
        to: u64,
        people: bool,
        most: u64,
        store: &S,
    ) -> Result<DayTotals, TotalsRefused<S::Error>> {
        if to <= from {
            return Err(TotalsRefused::BadWindow);
        }
        let mut blocks: Vec<Vec<Record>> = Vec::new();
        let mut end = self.records.len();
        let mut read = 0u64;
        while end > 0 {
            let start = end.saturating_sub(TOTALS_BLOCK);
            let len = usize::try_from(end - start).unwrap_or(usize::MAX);
            let block = self
                .records_from(start, len, store)
                .map_err(TotalsRefused::Store)?;
            read += end - start;
            let times: Vec<u64> = block.iter().filter_map(record_time).collect();
            let older = !times.is_empty() && times.iter().all(|t| *t < from);
            blocks.push(block);
            end = start;
            if older {
                break;
            }
            if read >= most && end > 0 {
                return Err(TotalsRefused::TooManyRecords);
            }
        }
        let records = blocks.iter().rev().flat_map(|b| b.iter());
        Ok(fold_day(records, from, to, people)?)
    }

    /// Append a check-in to the records (`docs/DESIGN.md` §2.9): a signed
    /// record whose body is the number of records before it and their head.
    /// It goes through the same stored path as every other record (CR-40):
    /// the store is given its bytes first, and on an error the chain is
    /// unchanged. Returns the record.
    pub fn check_in<S: Store>(&mut self, store: &mut S) -> Result<Record, StoreFault<S::Error>> {
        let mut body = Vec::with_capacity(40);
        body.extend_from_slice(&self.records.len().to_le_bytes());
        body.extend_from_slice(&self.records.head());
        self.append_record(store, RecordKind::CheckIn, body)
    }

    /// Sign the next record, hand it to the store and, only if the store
    /// took it, add it to the chain and let memory shrink to its bound
    /// (CR-81). On an error the chain is unchanged.
    fn append_record<S: Store>(
        &mut self,
        store: &mut S,
        kind: RecordKind,
        body: Vec<u8>,
    ) -> Result<Record, StoreFault<S::Error>> {
        let record = self
            .records
            .next(&self.identity, kind, body)
            .map_err(StoreFault::Record)?;
        store
            .append_record(&record.to_bytes())
            .map_err(StoreFault::Write)?;
        self.records.push(record.clone());
        self.records.trim(self.owner_mark);
        Ok(record)
    }

    /// The counter's identity.
    pub fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// The address batches. Empty for a counter paired with the shop's own
    /// wallet, which has [`Counter::range_ledger`] instead.
    pub fn batches(&self) -> &CounterBatches {
        &self.batches
    }

    /// CR-234: the ranges of a counter paired with the shop's own wallet, or
    /// `None` for one paired with the Key app.
    pub fn range_ledger(&self) -> Option<&openbmt_core::pairing_own::RangeLedger> {
        self.own.as_ref().map(|o| &o.ledger)
    }

    /// The addresses left to hand out, from the batches or from the ranges.
    fn addresses_left(&self) -> u64 {
        match &self.own {
            Some(own) => own.ledger.remaining(),
            None => self.batches.remaining() as u64,
        }
    }

    /// Whether `script` is one of the shop's own that the counter holds: in a
    /// batch, or handed out from the wallet.
    fn holds_own_script(&self, script: &bitcoin::Script) -> bool {
        self.batches.holds_script(script)
            || self.own.as_ref().is_some_and(|o| o.known.contains(script))
    }

    /// Every script the counter watches: the live sales, the archived ones
    /// that are not settled (CR-07, CR-19) and the addresses the batches have
    /// shown or retired and not released (CR-79). The owner's wallet watches
    /// every issued address for good (CR-80).
    pub fn watched_scripts(&self) -> Vec<ScriptBuf> {
        let mut all: BTreeSet<ScriptBuf> = self.batches.watched_scripts().iter().cloned().collect();
        if let Some(own) = &self.own {
            all.extend(own.watched.iter().cloned());
        }
        all.extend(self.sales.values().map(|s| s.script.clone()));
        all.extend(self.archived.keys().cloned());
        all.into_iter().collect()
    }

    // CR-76 to CR-80: letting old sales go.

    /// The time a sale last changed, if the counter can say: the later of the
    /// time it ended and the time of the newest record that names it.
    fn last_change(&self, sale: &Sale) -> Option<u64> {
        let ended = sale.ended().map(|(_, at)| at);
        let recorded = self.last_record.get(&sale.index).map(|(_, at)| *at);
        ended.max(recorded)
    }

    /// CR-76: the time a sale last changed if it is settled at `now` given
    /// the owner's mark, else `None`.
    fn settled_since(&self, sale: &Sale, mark: u64, now: u64) -> Option<u64> {
        if sale.is_live() || self.unreadable.contains(&sale.index) {
            return None;
        }
        if sale.exposure_counted && !sale.exposure_released {
            return None;
        }
        let last = self.last_change(sale)?;
        if last.saturating_add(SETTLE_SECS) > now {
            return None;
        }
        if self.refund_views(sale.index).iter().any(|v| {
            matches!(
                v.state,
                RefundState::Requested | RefundState::WaitingForDepth
            )
        }) {
            return None;
        }
        if self
            .last_record
            .get(&sale.index)
            .is_some_and(|(seq, _)| *seq >= mark)
        {
            return None;
        }
        Some(last)
    }

    /// CR-76 to CR-79: let go of the sales the owner holds everything about,
    /// and of the addresses nothing holds any more.
    ///
    /// Settles at most [`MAX_SETTLE_PER_CALL`] sales, the one that last
    /// changed longest ago first. For each, the stored sale is removed and
    /// then the sale leaves memory, so that a store that refuses leaves the
    /// sale where it was and the fault is returned. No record is written.
    /// Then the addresses the batches showed or retired at least
    /// [`SETTLE_SECS`] ago and that no sale of the counter holds leave the
    /// watch list; the batches are stored first. With no owner's mark
    /// ([`Self::set_owner_mark`]) no sale is settled, and while a sale is
    /// unreadable (CR-42) no address is released. A call that returns an
    /// error may have settled sales before the fault: they stay settled.
    pub fn settle<S: Store>(
        &mut self,
        store: &mut S,
        now: u64,
    ) -> Result<Settled, StoreFault<S::Error>> {
        let mut ready: Vec<(u64, u32)> = match self.owner_mark {
            Some(mark) => self
                .sales
                .values()
                .filter_map(|sale| {
                    self.settled_since(sale, mark, now)
                        .map(|last| (last, sale.index))
                })
                .collect(),
            None => Vec::new(),
        };
        ready.sort_unstable();
        let remaining = ready.len() > MAX_SETTLE_PER_CALL;
        ready.truncate(MAX_SETTLE_PER_CALL);
        let mut count = 0;
        for (_, index) in ready {
            store.delete(Slot::Sale(index)).map_err(StoreFault::Write)?;
            if let Some(sale) = self.sales.remove(&index) {
                if self.archived.get(&sale.script) == Some(&index) {
                    self.archived.remove(&sale.script);
                }
            }
            self.last_record.remove(&index);
            self.refund_book.remove(&index);
            self.cancel_applied.remove(&index);
            self.refund_unsaved.remove(&index);
            count += 1;
        }
        let released = self.release_addresses(store, now)?;
        Ok(Settled {
            count,
            released,
            remaining,
        })
    }

    /// CR-79: take the old addresses no sale holds off the watch list.
    fn release_addresses<S: Store>(
        &mut self,
        store: &mut S,
        now: u64,
    ) -> Result<usize, StoreFault<S::Error>> {
        if !self.unreadable.is_empty() {
            return Ok(0);
        }
        // A counter paired with the shop's own wallet keeps every address it
        // handed out on its watch list: the ranges hold indexes, not times.
        if self.own.is_some() {
            return Ok(0);
        }
        let held: BTreeSet<&ScriptBuf> = self
            .sales
            .values()
            .map(|s| &s.script)
            .chain(self.archived.keys())
            .collect();
        let released = self.batches.release_watched(
            |script, at| at.saturating_add(SETTLE_SECS) <= now && !held.contains(script),
            |bytes| store.put(Slot::Batches, bytes),
        )?;
        Ok(released)
    }

    // Internal helpers.

    /// CR-03, CR-234: the next address and its index, stored before it is
    /// handed back. From the batches, or, for a counter paired with the
    /// shop's own wallet, the wallet's receive address at the next index of
    /// the ranges, with the whole ledger written first.
    fn issue_address<S: Store>(
        &mut self,
        now: u64,
        store: &mut S,
    ) -> Result<Option<(u32, ScriptBuf)>, OpenRefused<S::Error>> {
        if let Some(own) = &self.own {
            let Some((next, index, script)) = own.issue() else {
                return Ok(None);
            };
            store
                .put(Slot::Ranges, &next.ledger.to_bytes())
                .map_err(|e| OpenRefused::AddressNotStored(StoreFault::Write(e)))?;
            self.own = Some(next);
            return Ok(Some((index, script)));
        }
        self.batches
            .next_script(now, |bytes| store.put(Slot::Batches, bytes))
            .map_err(|e| OpenRefused::AddressNotStored(StoreFault::Write(e)))
    }

    fn install(&mut self, sale: Sale) {
        if !sale.is_live() {
            self.archived.insert(sale.script.clone(), sale.index);
        }
        self.sales.insert(sale.index, sale);
    }

    fn put_sale<S: Store>(
        &mut self,
        store: &mut S,
        sale: Sale,
    ) -> Result<(), StoreFault<S::Error>> {
        put(store, Slot::Sale(sale.index), &sale.to_bytes())?;
        self.install(sale);
        Ok(())
    }

    fn put_desk<S: Store>(
        &mut self,
        store: &mut S,
        desk: Desk,
    ) -> Result<(), StoreFault<S::Error>> {
        put(store, Slot::Desk, &desk.to_bytes())?;
        self.desk = desk;
        Ok(())
    }

    fn write_event<S: Store>(
        &mut self,
        store: &mut S,
        event: &Event,
    ) -> Result<(), StoreFault<S::Error>> {
        let record = self.append_record(store, event.kind(), event.encode())?;
        let at = self
            .last_record
            .get(&event.index())
            .map_or(0, |(_, at)| *at)
            .max(event.at());
        self.last_record.insert(event.index(), (record.seq, at));
        if record.kind == RecordKind::Refund {
            fold_refund(
                &mut self.refund_book,
                &mut self.highest_refund,
                event.clone(),
            );
        }
        Ok(())
    }

    /// Write the records, then the sale. A record that is written and a sale
    /// that then fails to store leaves a record for a step the sale does not
    /// know happened; the step is done again and the record is written again.
    /// That is the safe side: a report is never made without its record.
    fn finish<S: Store>(
        &mut self,
        store: &mut S,
        sale: Sale,
        events: &[Event],
    ) -> Result<(), StoreFault<S::Error>> {
        for event in events {
            self.write_event(store, event)?;
        }
        let unchanged = self
            .sales
            .get(&sale.index)
            .is_some_and(|old| old.to_bytes() == sale.to_bytes());
        if unchanged {
            return Ok(());
        }
        self.put_sale(store, sale)
    }

    /// CR-14: write the pause again if its last write failed.
    fn retry_pause<S: Store>(&mut self, store: &mut S) {
        if self.pause_unsaved && put(store, Slot::Pause, &self.pause.to_bytes()).is_ok() {
            self.pause_unsaved = false;
        }
    }

    // CR-01 to CR-05: opening a sale and its quotes.

    /// CR-01 to CR-04, CR-09, CR-17: open a sale and hand out its address.
    ///
    /// Refused, with nothing written, when a rescan is pending, the check is
    /// not healthy, the amount is zero, too large, below the dust line or below
    /// the smallest bitcoin sale (CR-75), the hour's addresses are used up, or
    /// no address is left.
    ///
    /// The writes, in this order:
    ///
    /// 1. the desk with the time of this address (CR-04). If the process
    ///    stops after it, an address time is counted and no address was
    ///    used: the hour's allowance is spent early, which is the safe side.
    /// 2. the batches, with the index taken (CR-03, through
    ///    `CounterBatches::next_script`). If the process stops after it, an
    ///    index is wasted and never reused.
    /// 3. the sale. If the process stops after it, the sale is there with
    ///    its address and expires by itself (CR-07); a payment to it is found.
    /// 4. a `Sale` record "opened". If it fails, the sale is stored and will
    ///    expire, but `Err` is returned and the address must not be shown.
    ///
    /// `source` says where the rate came from (CR-57). A rate fetched more
    /// than [`crate::MAX_RATE_AGE_SECS`] before `facts.now`, or more than
    /// [`crate::MAX_RATE_AHEAD_SECS`] after it, is refused (CR-58), with nothing
    /// written.
    ///
    /// CR-196: the sale is in the currency of the settings in force, whatever
    /// `currency` says. With Bitcoin Standard (CR-197) the amount is in
    /// satoshis, `rate` and `source` are not read, and the sale is quoted at
    /// the amount and records the source `sats`.
    pub fn open<S: Store>(
        &mut self,
        amount: Amount,
        currency: Currency,
        rate: Rate,
        source: &RateSource,
        facts: OpenFacts,
        store: &mut S,
    ) -> Result<Opened, OpenRefused<S::Error>> {
        self.open_by(amount, currency, rate, source, facts, None, store)
    }

    /// [`Counter::open`], naming the person at the till who opens the sale
    /// (CR-151): the record `Opened` carries the role and the name. A name
    /// that is not a label (CR-61) is refused before anything is written.
    #[allow(clippy::too_many_arguments)]
    pub fn open_by<S: Store>(
        &mut self,
        amount: Amount,
        currency: Currency,
        rate: Rate,
        source: &RateSource,
        facts: OpenFacts,
        by: Option<&Actor>,
        store: &mut S,
    ) -> Result<Opened, OpenRefused<S::Error>> {
        self.open_with_tip(amount, Amount(0), currency, rate, source, facts, by, store)
    }

    /// CR-188: the tips the counter offers for a sale of `amount`: one for
    /// each percent of the settings, in the order the settings give them.
    /// Empty when the settings do not ask for a tip. The customer picks one
    /// of these, types an amount or takes none, and the caller passes the
    /// tip to [`Counter::open_with_tip`].
    pub fn tip_choices(&self, amount: Amount) -> Vec<TipChoice> {
        if !self.settings.tips_on {
            return Vec::new();
        }
        self.settings
            .tip_percents
            .iter()
            .filter_map(|percent| {
                tip_for_percent(amount, *percent).map(|tip| TipChoice {
                    percent: *percent,
                    tip,
                })
            })
            .collect()
    }

    /// [`Counter::open_by`] for a sale with a tip (CR-189, CR-190): `amount`
    /// is the sale itself and `tip` what the customer chose to add, 0 for
    /// none. The code is for the two together: the quote, the underpayment
    /// tolerance, the instant limits, the exposure and every other check are
    /// made against the total. The record `Opened` carries the amount and the
    /// tip as two fields and the person at the till.
    ///
    /// A tip above `amount` is refused (CR-190), with nothing written. The
    /// counter takes the tip it is given: that the settings ask for one is
    /// the caller's to honour.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_tip<S: Store>(
        &mut self,
        amount: Amount,
        tip: Amount,
        currency: Currency,
        rate: Rate,
        source: &RateSource,
        facts: OpenFacts,
        by: Option<&Actor>,
        store: &mut S,
    ) -> Result<Opened, OpenRefused<S::Error>> {
        self.open_with_extras(
            amount,
            tip,
            currency,
            rate,
            source,
            facts,
            by,
            SaleExtras::default(),
            store,
        )
    }

    /// [`Counter::open_with_tip`] for a sale that carries a note (CR-206), or
    /// is a request to send (CR-205): `extras` says which. A note that is not
    /// plain text of 1 to 64 characters is refused with nothing written. A
    /// request has the lifetime of its days, one quote that holds for all of
    /// it, and is never accepted at once (CR-205, CR-208).
    ///
    /// The writes are those of [`Counter::open`], and then, for a note, a
    /// `Sale` record "noted" (CR-206): if it fails, the sale is stored, `Err`
    /// is returned and the address must not be shown.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_extras<S: Store>(
        &mut self,
        amount: Amount,
        tip: Amount,
        _currency: Currency,
        rate: Rate,
        source: &RateSource,
        facts: OpenFacts,
        by: Option<&Actor>,
        extras: SaleExtras<'_>,
        store: &mut S,
    ) -> Result<Opened, OpenRefused<S::Error>> {
        if by.is_some_and(|a| !refund_text_ok(&a.name)) {
            return Err(OpenRefused::BadName);
        }
        if extras.note.is_some_and(|n| !refund_text_ok(n)) {
            return Err(OpenRefused::BadNote);
        }
        let now = facts.now;
        // CR-196, CR-197: the currency is the settings'; Bitcoin Standard
        // has no price, so the rate and its source are the satoshi's own.
        let currency = Currency::of_shop(self.settings.currency);
        let (rate, source) = self.priced_at(rate, source, now);
        let source = &source;
        if !self.is_listed() {
            return Err(OpenRefused::Removed);
        }
        if facts.rescan_pending {
            return Err(OpenRefused::RescanPending);
        }
        // WR-54 turns instant acceptance off and does not stop a sale from
        // opening: it waits for a confirmation.
        if !facts.health.healthy && facts.health.problem != Some(Problem::BlocksBehind) {
            return Err(OpenRefused::Unhealthy(facts.health.problem));
        }
        if amount.0 == 0 {
            return Err(OpenRefused::ZeroAmount);
        }
        if tip.0 > amount.0 {
            return Err(OpenRefused::TipTooLarge);
        }
        let due = Amount(
            amount
                .0
                .checked_add(tip.0)
                .ok_or(OpenRefused::AmountTooLarge)?,
        );
        let needs_sats = sats_for(due, rate).ok_or(OpenRefused::AmountTooLarge)?;
        if needs_sats < DUST_SATS {
            return Err(OpenRefused::BelowDust { needs_sats });
        }
        let min_sats = self.settings.min_sale_sats;
        if needs_sats < min_sats {
            self.refused_below_minimum = self.refused_below_minimum.saturating_add(1);
            return Err(OpenRefused::BelowMinimum {
                sats: needs_sats,
                min_sats,
            });
        }
        if !source.is_fresh_at(now) {
            return Err(OpenRefused::StaleRate {
                fetched_at: source.fetched_at(),
                now,
            });
        }
        let mut desk = self.desk.pruned(now);
        if !desk.has_room_for_address() {
            return Err(OpenRefused::RateLimited);
        }
        if self.addresses_left() == 0 {
            return Err(OpenRefused::NoAddress);
        }
        desk.push_address_time(now);
        self.put_desk(store, desk).map_err(OpenRefused::Store)?;

        let issued = self.issue_address(now, store)?;
        let Some((index, script)) = issued else {
            return Err(OpenRefused::NoAddress);
        };
        if self.sales.contains_key(&index)
            || self.unreadable.contains(&index)
            || self.archived.contains_key(&script)
        {
            return Err(OpenRefused::AddressInUse);
        }

        // CR-205: the one quote of a request holds for its whole lifetime.
        let quote = match extras.request {
            Some(days) => Quote::lasting(rate, now, days.secs()),
            None => Quote::new(rate, now),
        };
        let mut sale = Sale::new(
            index,
            script.clone(),
            now,
            due,
            tip,
            currency,
            quote,
            needs_sats,
            self.settings.underpayment_tolerance_bps,
        );
        sale.request = extras.request;
        sale.note = extras.note.map(str::to_string);
        self.put_sale(store, sale).map_err(OpenRefused::Store)?;
        let event = Event::Opened {
            index,
            at: now,
            amount,
            tip,
            currency,
            needs_sats,
            minor_per_btc: rate.minor_per_btc(),
            source: source.clone(),
            by: by.map(|a| (a.role, a.name.clone())),
        };
        self.write_event(store, &event)
            .map_err(OpenRefused::Store)?;
        if let Some(note) = extras.note {
            let noted = Event::Noted {
                index,
                at: now,
                note: note.to_string(),
            };
            self.write_event(store, &noted)
                .map_err(OpenRefused::Store)?;
        }
        Ok(Opened {
            index,
            script,
            needs_sats,
            quote,
        })
    }

    /// CR-206: add a note to a sale that has none. The note is plain text of
    /// 1 to 64 characters (the check of a person's name, CR-61); a sale takes
    /// one note, set when it opened or added here once. Works on a sale in
    /// any state: a note is a word about the sale, not a change to it.
    ///
    /// The writes, in this order: a `Sale` record "noted", then the sale. If
    /// the process stops between them the sale has no note and `Err` is
    /// returned; asking again writes a second record.
    pub fn add_note<S: Store>(
        &mut self,
        index: u32,
        note: &str,
        now: u64,
        store: &mut S,
    ) -> Result<(), NoteRefused<S::Error>> {
        if !refund_text_ok(note) {
            return Err(NoteRefused::BadNote);
        }
        let mut sale = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(NoteRefused::UnknownSale(index))?;
        if sale.note.is_some() {
            return Err(NoteRefused::AlreadyNoted);
        }
        sale.note = Some(note.to_string());
        let event = Event::Noted {
            index,
            at: now,
            note: note.to_string(),
        };
        self.finish(store, sale, &[event])
            .map_err(NoteRefused::Store)
    }

    /// CR-197: the rate and its source a price is made with. The caller's,
    /// except for Bitcoin Standard, which has no price: there the rate is the
    /// satoshi's own and the source is `sats` at `now`.
    fn priced_at(&self, rate: Rate, source: &RateSource, now: u64) -> (Rate, RateSource) {
        if self.settings.currency.is_sats() {
            (Rate::sats(), RateSource::sats(now))
        } else {
            (rate, source.clone())
        }
    }

    /// CR-05, CR-08: give a sale a new quote. What has arrived is valued at
    /// the quote in force first; then the new quote is added and the rest of
    /// the amount is priced at it. Refused once enough has arrived, once the
    /// sale ended, and once the sale has [`MAX_QUOTES`] quotes (the last one
    /// stays in force). A rate that is stale (CR-58) is refused. A sale with
    /// nothing received is refused a quote that would put it below the
    /// smallest bitcoin sale (CR-75).
    ///
    /// The writes, in this order: a `Sale` record "requoted" (CR-59), then
    /// the sale. If the process stops between them the sale does not hold
    /// the new quote and `Err` is returned; the record names a quote the
    /// sale never took, and asking again writes a second record.
    pub fn requote<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        rate: Rate,
        source: &RateSource,
        now: u64,
        headers: &C,
        store: &mut S,
    ) -> Result<Quote, RequoteRefused<S::Error>> {
        let (rate, source) = self.priced_at(rate, source, now);
        let source = &source;
        let mut sale = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(RequoteRefused::UnknownSale(index))?;
        if !sale.is_live() {
            return Err(RequoteRefused::SaleEnded);
        }
        if sale.is_request() {
            return Err(RequoteRefused::IsRequest);
        }
        if sale.quotes.len() >= MAX_QUOTES {
            return Err(RequoteRefused::TooManyQuotes);
        }
        let Some(needs_sats) = sats_for(sale.amount, rate) else {
            return Err(RequoteRefused::AmountTooLarge);
        };
        let depth = self.depth();
        sale.refresh(
            headers,
            now,
            depth,
            &OwnCheck::Mixed(OwnNodeHealth::not_configured()),
        );
        if sale.frozen.is_some() {
            return Err(RequoteRefused::AlreadyEnough);
        }
        // CR-75: a sale with nothing received is not requoted below the
        // smallest bitcoin sale. One paid in part keeps its quotes moving.
        let min_sats = self.settings.min_sale_sats;
        if needs_sats < min_sats
            && sale.watch.transactions().next().is_none()
            && sale.accepted.is_none()
            && sale.overridden.is_none()
        {
            return Err(RequoteRefused::BelowMinimum {
                sats: needs_sats,
                min_sats,
            });
        }
        if !source.is_fresh_at(now) {
            return Err(RequoteRefused::StaleRate {
                fetched_at: source.fetched_at(),
                now,
            });
        }
        let quote = Quote::new(rate, now);
        sale.quotes.push(quote);
        sale.refresh(
            headers,
            now,
            depth,
            &OwnCheck::Mixed(OwnNodeHealth::not_configured()),
        );
        let event = Event::Requoted {
            index,
            at: now,
            needs_sats,
            minor_per_btc: rate.minor_per_btc(),
            source: source.clone(),
        };
        self.finish(store, sale, &[event])
            .map_err(RequoteRefused::Store)?;
        Ok(quote)
    }

    /// CR-07: the cashier cancels a sale. Refused if the sale has ended or
    /// anything has arrived for it (even dust): money on a cancelled sale is
    /// a matter for the refund flow, not for cancelling.
    ///
    /// The writes, in this order: a `Sale` record "cancelled", then the
    /// sale. If the process stops between them the sale is still open and
    /// expires by itself; the record is written again if it is cancelled
    /// again.
    pub fn cancel<S: Store>(
        &mut self,
        index: u32,
        now: u64,
        store: &mut S,
    ) -> Result<(), CancelRefused<S::Error>> {
        self.cancel_by(index, now, None, store)
    }

    /// [`Counter::cancel`], naming the person at the till who cancels the
    /// sale (CR-151).
    pub fn cancel_by<S: Store>(
        &mut self,
        index: u32,
        now: u64,
        by: Option<&Actor>,
        store: &mut S,
    ) -> Result<(), CancelRefused<S::Error>> {
        if by.is_some_and(|a| !refund_text_ok(&a.name)) {
            return Err(CancelRefused::BadName);
        }
        let mut sale = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(CancelRefused::UnknownSale(index))?;
        if sale.ended.is_some() {
            return Err(CancelRefused::AlreadyEnded);
        }
        if sale.watch.transactions().next().is_some()
            || sale.accepted.is_some()
            || sale.overridden.is_some()
            || !sale.is_live()
        {
            return Err(CancelRefused::MoneyArrived);
        }
        sale.ended = Some((End::Cancelled, now));
        let event = Event::Cancelled {
            index,
            at: now,
            by: by.map(|a| (a.role, a.name.clone())),
        };
        self.finish(store, sale, &[event])
            .map_err(CancelRefused::Store)
    }

    fn depth(&self) -> u32 {
        u32::from(self.settings.min_confirmations).max(1)
    }

    // What arrives: sightings and blocks.

    /// WR-40, CR-19: offer an unconfirmed transaction. It goes to every live
    /// sale, because a transaction that conflicts with a sale's payment does
    /// not pay the sale's script, so routing by script is not enough. A
    /// refusal from one sale does not stop the others. If the transaction
    /// pays the script of a sale that ended or is final, it is a late
    /// payment: a `Payment` record, then the sale, then a
    /// [`Report::LatePayment`].
    ///
    /// Each sale that changed is written (the sale is the only write for it).
    /// A write that fails stops the call; sales written before it keep what
    /// they took, and offering the same transaction again is safe.
    pub fn sighting<S: Store>(
        &mut self,
        tx_bytes: &[u8],
        source: Source,
        now: u64,
        store: &mut S,
    ) -> Result<Sightings, SightingError<S::Error>> {
        let tx: Transaction =
            deserialize(tx_bytes).map_err(|_| SightingError::Refused(Refusal::NotATransaction))?;
        let mut out = Sightings::default();
        let live: Vec<u32> = self
            .sales
            .iter()
            .filter(|(_, s)| s.is_live())
            .map(|(i, _)| *i)
            .collect();
        for index in live {
            let Some(base) = self.sales.get(&index) else {
                continue;
            };
            let mut next = base.clone();
            let result = next.watch.sighting(tx_bytes, source, now);
            if matches!(result, Ok(Sighted::Ignored)) {
                continue;
            }
            self.put_sale(store, next).map_err(SightingError::Store)?;
            out.results.push((index, result));
        }
        let txid = tx.compute_txid();
        for (index, sats) in self.late_payments_in(&tx) {
            self.note_late(index, txid, sats, now, store, &mut out.reports)
                .map_err(SightingError::Store)?;
        }
        Ok(out)
    }

    /// CR-103: the own node reports that it holds `txid` in its memory pool
    /// (WR-61). Returns the live sales that know the transaction and have
    /// recorded the report; the report is not a sighting and is kept in
    /// memory only (WR-65), so nothing is stored and nothing is reported.
    /// A transaction no sale knows records nothing and returns no sale: the
    /// caller asks the node again when the light client has sighted it.
    pub fn own_node_saw(&mut self, txid: Txid) -> Vec<u32> {
        let mut known = Vec::new();
        for (index, sale) in self.sales.iter_mut() {
            if sale.is_live() && sale.watch.own_node_saw(txid) {
                known.push(*index);
            }
        }
        known
    }

    /// As [`Counter::own_node_saw`], with the clock at the report: in
    /// node-only mode the conflict watch counts from it (WR-66).
    pub fn own_node_saw_at(&mut self, txid: Txid, at: u64) -> Vec<u32> {
        let mut known = Vec::new();
        for (index, sale) in self.sales.iter_mut() {
            if sale.is_live() && sale.watch.own_node_saw_at(txid, at) {
                known.push(*index);
            }
        }
        known
    }

    /// What a transaction pays to the scripts of sales that are not live,
    /// by sale.
    fn late_payments_in(&self, tx: &Transaction) -> BTreeMap<u32, u64> {
        let mut found: BTreeMap<u32, u64> = BTreeMap::new();
        for output in &tx.output {
            if output.value.to_sat() == 0 {
                continue;
            }
            if let Some(index) = self.archived.get(&output.script_pubkey) {
                let sum = found.entry(*index).or_insert(0);
                *sum = sum.saturating_add(output.value.to_sat());
            }
        }
        found
    }

    /// CR-19: record a payment to a sale that is not live. The sale's own
    /// payment, seen again, is not late. Each transaction is recorded once.
    /// The writes: the `Payment` record, then the sale; the report only after
    /// both.
    fn note_late<S: Store>(
        &mut self,
        index: u32,
        txid: Txid,
        sats: u64,
        now: u64,
        store: &mut S,
        reports: &mut Vec<Report>,
    ) -> Result<(), StoreFault<S::Error>> {
        let Some(base) = self.sales.get(&index) else {
            return Ok(());
        };
        // CR-221: what a swap's claim pays the sale's address is the sale's
        // own payment, not a late one.
        if base.via_swap {
            return Ok(());
        }
        if base.late.iter().any(|(t, _)| *t == txid)
            || base.watch.transactions().any(|t| t.compute_txid() == txid)
        {
            return Ok(());
        }
        let mut sale = base.clone();
        if sale.late.len() < MAX_LATE_PAYMENTS {
            sale.late.push((txid, sats));
        } else if sale.late_unlisted {
            return Ok(());
        } else {
            sale.late_unlisted = true;
        }
        let event = Event::LatePayment {
            index,
            at: now,
            txid: txid.to_byte_array(),
            sats,
        };
        self.write_event(store, &event)?;
        self.put_sale(store, sale)?;
        reports.push(Report::LatePayment { index, sats });
        Ok(())
    }

    /// WR-30, WR-42, CR-19: take a block. For every live sale the payments
    /// the block proves are given to the watcher as confirmed, and the spends
    /// of the sale's coins by other transactions are given as spent
    /// elsewhere. For sales that are not live, a payment in the block is a
    /// late payment.
    ///
    /// A block that is not on the chain now is refused for all sales. Each
    /// sale that changed is written. A write that fails stops the call;
    /// sales written before it keep what they took, and offering the same
    /// block again is safe.
    pub fn block<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        block: &WellFormedBlock,
        headers: &C,
        now: u64,
        store: &mut S,
    ) -> Result<BlockTaken, BlockError<S::Error>> {
        block.spends(headers, &[]).map_err(BlockError::Refused)?;
        let mut out = BlockTaken::default();
        let live: Vec<u32> = self
            .sales
            .iter()
            .filter(|(_, s)| s.is_live())
            .map(|(i, _)| *i)
            .collect();
        for index in live {
            let Some(base) = self.sales.get(&index) else {
                continue;
            };
            let mut next = base.clone();
            let script = next.script.clone();
            let mut effect = BlockEffect {
                payments: 0,
                spends: 0,
            };
            let mut refused: Option<Refusal> = None;
            match block.payments(headers, &script) {
                Ok(list) => {
                    for proven in list {
                        match next.watch.confirmed_at(proven, now) {
                            Ok(()) => effect.payments = effect.payments.saturating_add(1),
                            Err(r) => refused = refused.or(Some(r)),
                        }
                    }
                }
                Err(r) => refused = Some(r),
            }
            if let Ok(found) = block.spends(headers, &next.watch.watched_outpoints()) {
                for (outpoint, by, hash) in found {
                    next.watch.spent_elsewhere(outpoint, by, hash);
                    effect.spends = effect.spends.saturating_add(1);
                }
            }
            if effect.payments == 0 && effect.spends == 0 && refused.is_none() {
                continue;
            }
            self.put_sale(store, next).map_err(BlockError::Store)?;
            out.results.push((
                index,
                match refused {
                    Some(r) => Err(r),
                    None => Ok(effect),
                },
            ));
        }
        // One pass over the block's outputs finds the archived scripts it
        // pays; the watcher's check is then asked about those alone.
        let archived: BTreeMap<ScriptBuf, u32> = block
            .output_scripts()
            .filter_map(|s| self.archived.get_key_value(s))
            .map(|(s, i)| (s.clone(), *i))
            .collect();
        for (script, index) in archived {
            let Ok(list) = block.payments(headers, &script) else {
                continue;
            };
            for proven in list {
                let sats = proven
                    .tx()
                    .output
                    .iter()
                    .filter(|o| o.script_pubkey == script)
                    .fold(0u64, |sum, o| sum.saturating_add(o.value.to_sat()));
                let txid = proven.tx().compute_txid();
                self.note_late(index, txid, sats, now, store, &mut out.reports)
                    .map_err(BlockError::Store)?;
            }
        }
        Ok(out)
    }

    /// CR-109: the shop's own node says the Lightning invoice of a sale was
    /// paid. The sale is paid and final at once: no instant limit, no
    /// exposure, no waiting for a block (DESIGN section 8). The sale keeps
    /// the payment hash, the amount in millisatoshis and the time the node
    /// says it settled, and the books are told it is final (CR-31).
    ///
    /// Refused, with nothing written, for a sale the counter does not hold,
    /// a sale that ended or is final already, a sale paid another way (it
    /// was accepted at once or by a person, or enough has arrived on its
    /// address), and a payment for less than the least any quote of the sale
    /// asks. Asking again for a sale that is paid over Lightning already
    /// changes nothing and gives its screen.
    ///
    /// What had arrived on the address before is not lost: above the dust
    /// line it is recorded as owed to the customer (CR-16), as a payment
    /// beyond what the sale needed is. What arrives there after is a late
    /// payment (CR-19).
    ///
    /// The writes, in this order: the records (`PaidByLightning`, the
    /// surplus if there is one, `PosPaid`), then the sale. A stop between
    /// them leaves records for a payment the sale does not show; the node
    /// reports it again and the step is done again, which is the safe side
    /// (a report is never made without its record).
    pub fn lightning_paid<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        paid: LightningPaid,
        facts: &Facts<'_, C>,
        store: &mut S,
    ) -> Result<Screen, LightningRefused<S::Error>> {
        if self.unreadable.contains(&index) {
            return Err(LightningRefused::Judge(JudgeError::UnknownSale(index)));
        }
        let base = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(LightningRefused::UnknownSale(index))?;
        let own = OwnCheck::Mixed(OwnNodeHealth::not_configured());
        if base.lightning.is_some() {
            return self
                .judge_checked(index, facts, &own, store)
                .map_err(LightningRefused::Judge);
        }
        if !base.is_live() {
            return Err(LightningRefused::SaleClosed);
        }
        let now = facts.now;
        let mut sale = base;
        let (st, received) = sale.refresh(facts.chain, now, self.depth(), &own);
        if sale.accepted.is_some()
            || sale.overridden.is_some()
            || sale.frozen.is_some()
            || st.needs_a_person
        {
            return Err(LightningRefused::PaidAnotherWay);
        }
        let sats = paid.sats();
        let least = sale
            .quotes
            .iter()
            .filter_map(|q| sats_for(sale.amount, q.rate))
            .min()
            .unwrap_or(u64::MAX);
        if sats == 0 || sats < least {
            return Err(LightningRefused::TooLittle { sats, least });
        }
        let valued = match sale.rate_of(sale.quote_index_at(now)) {
            Some(rate) => value_of(sats, rate),
            None => Amount(0),
        };
        sale.lightning = Some(paid);
        sale.books_final = true;
        let mut events = vec![Event::PaidByLightning {
            index,
            at: now,
            payment_hash: paid.payment_hash,
            amount_msat: paid.amount_msat,
            settled_at: paid.settled_at,
            valued,
        }];
        let mut reports = Vec::new();
        if received > 0 {
            sale.surplus_reported = received;
            events.push(Event::SurplusOwed {
                index,
                at: now,
                sats: received,
            });
            reports.push(Report::SurplusOwed {
                index,
                sats: received,
            });
        }
        if sale.pos != PosState::Paid {
            sale.pos = PosState::Paid;
            events.push(Event::PosPaid { index, at: now });
            reports.push(Report::PosPaid { index });
        }
        reports.push(Report::BooksFinal {
            index,
            amount: sale.own_amount(),
            currency: sale.currency,
            received_sats: sats,
        });
        self.finish(store, sale, &events)
            .map_err(LightningRefused::Store)?;
        let mut screen = self
            .judge_checked(index, facts, &own, store)
            .map_err(LightningRefused::Judge)?;
        screen.reports = reports;
        Ok(screen)
    }

    /// CR-221: the node says the swap provider's lockup of a sale is on the
    /// way: set when its bitcoin is seen and until the claim is in a block,
    /// cleared with `false`. A sale in that state does not expire (CR-07).
    pub fn set_swap_pending(&mut self, index: u32, pending: bool) {
        if pending {
            self.swap_pending.insert(index);
        } else {
            self.swap_pending.remove(&index);
        }
    }

    /// CR-221: whether a swap payment of this sale may be taken at once,
    /// before its lockup is in a block, as an on-chain payment seen by
    /// `status` (the status of the lockup's watch, `SaleWatch::status`) may
    /// be (CR-13): the same policy, the sale's quote in satoshis as the
    /// amount, the same slice and the same heartbeat. `None` means the
    /// payment waits for the lockup's first confirmation. Reads only.
    pub fn swap_decide<C: HeaderChain + ?Sized>(
        &self,
        index: u32,
        invoice_sats: u64,
        status: &openbmt_watch::SaleStatus,
        facts: &Facts<'_, C>,
    ) -> Option<(openbmt_core::policy::InstantLabel, CheckMode)> {
        let sale = self.sales.get(&index)?;
        if !sale.is_live() || status.needs_a_person || !facts.health.healthy {
            return None;
        }
        let mode = status.mode?;
        let id = sale_payment_id(index);
        let outstanding = {
            let mut ledger = self.exposure.clone();
            ledger.release_by_owner(&id);
            ledger.outstanding()
        };
        let mut policy = InstantPolicy::for_counter(&self.settings, &self.identity.device_id());
        policy.underpayment_tolerance_bps = 0;
        let inputs = Inputs {
            mode,
            sale_sats: invoice_sats,
            received_sats: invoice_sats,
            outstanding_sats: outstanding,
            check_healthy: facts.health.healthy,
            heartbeat: facts.heartbeat,
            remote_order: facts.remote_order,
            customer_wifi: self.settings.customer_wifi,
            tor_on: self.settings.tor_on,
            pause: self.pause.status(facts.now),
            conflict_seen: status.conflict_seen,
            secs_since_first_seen: status.secs_since_first_seen,
        };
        match evaluate(&policy, &inputs) {
            Decision::Instant { label, mode } => Some((label, mode)),
            _ => None,
        }
    }

    /// CR-221, CR-109: the swap provider's bitcoin for a sale is locked and
    /// the claim to the sale's own address is built, so the sale is paid
    /// over Lightning. `instant` is the decision of [`Counter::swap_decide`]
    /// when the lockup is not yet in a block: the amount at risk is counted
    /// in the exposure ledger as for any instant sale (CR-13) until the
    /// lockup confirms ([`Counter::swap_lockup_confirmed`]). With `None` the
    /// lockup is in a block and nothing is counted.
    ///
    /// The sale keeps the payment hash and `received_sats`, what the claim
    /// pays the sale's address, in place of the node's millisatoshis. The
    /// customer paid `invoice_sats`, which must cover a quote of the sale.
    /// Refused as [`Counter::lightning_paid`] refuses. `Ok(None)` when the
    /// exposure could not be counted (the slice is full or the ledger could
    /// not be written): nothing was changed and the payment waits for the
    /// lockup's confirmation. The writes are those of `lightning_paid`,
    /// after the ledger.
    pub fn swap_paid<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        paid: SwapPaid,
        instant: Option<(openbmt_core::policy::InstantLabel, CheckMode)>,
        facts: &Facts<'_, C>,
        store: &mut S,
    ) -> Result<Option<Screen>, LightningRefused<S::Error>> {
        if self.unreadable.contains(&index) {
            return Err(LightningRefused::Judge(JudgeError::UnknownSale(index)));
        }
        let base = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(LightningRefused::UnknownSale(index))?;
        let own = OwnCheck::Mixed(OwnNodeHealth::not_configured());
        if base.lightning.is_some() {
            return self
                .judge_checked(index, facts, &own, store)
                .map(Some)
                .map_err(LightningRefused::Judge);
        }
        if !base.is_live() {
            return Err(LightningRefused::SaleClosed);
        }
        let now = facts.now;
        let mut sale = base;
        let (st, received) = sale.refresh(facts.chain, now, self.depth(), &own);
        if sale.accepted.is_some()
            || sale.overridden.is_some()
            || sale.frozen.is_some()
            || st.needs_a_person
        {
            return Err(LightningRefused::PaidAnotherWay);
        }
        let least = sale
            .quotes
            .iter()
            .filter_map(|q| sats_for(sale.amount, q.rate))
            .min()
            .unwrap_or(u64::MAX);
        if paid.received_sats == 0 || paid.invoice_sats < least {
            return Err(LightningRefused::TooLittle {
                sats: paid.invoice_sats,
                least,
            });
        }
        if let Some((label, mode)) = instant {
            if self
                .accept_exposure(index, paid.invoice_sats, mode, store)
                .is_err()
            {
                return Ok(None);
            }
            sale.accepted = Some((label, mode));
            sale.exposure_counted = true;
        }
        let amount_msat = paid.received_sats.saturating_mul(1_000);
        let valued = match sale.rate_of(sale.quote_index_at(now)) {
            Some(rate) => value_of(paid.received_sats, rate),
            None => Amount(0),
        };
        let lightning = LightningPaid {
            payment_hash: paid.payment_hash,
            amount_msat,
            settled_at: None,
        };
        sale.lightning = Some(lightning);
        sale.via_swap = true;
        sale.books_final = true;
        let mut events = vec![Event::PaidByLightning {
            index,
            at: now,
            payment_hash: paid.payment_hash,
            amount_msat,
            settled_at: None,
            valued,
        }];
        let mut reports = Vec::new();
        if received > 0 {
            sale.surplus_reported = received;
            events.push(Event::SurplusOwed {
                index,
                at: now,
                sats: received,
            });
            reports.push(Report::SurplusOwed {
                index,
                sats: received,
            });
        }
        if sale.pos != PosState::Paid {
            sale.pos = PosState::Paid;
            events.push(Event::PosPaid { index, at: now });
            reports.push(Report::PosPaid { index });
        }
        reports.push(Report::BooksFinal {
            index,
            amount: sale.own_amount(),
            currency: sale.currency,
            received_sats: paid.received_sats,
        });
        self.swap_pending.remove(&index);
        if let Err(e) = self.finish(store, sale, &events) {
            // The exposure counted above stays: the sale is accepted by the
            // next try, which does not count it twice.
            return Err(LightningRefused::Store(e));
        }
        let mut screen = self
            .judge_checked(index, facts, &own, store)
            .map_err(LightningRefused::Judge)?;
        screen.reports = reports;
        Ok(Some(screen))
    }

    /// CR-221, CR-15: the lockup of a sale paid before its bitcoin was in a
    /// block is now in one: the exposure leaves the ledger. Asking again
    /// changes nothing.
    pub fn swap_lockup_confirmed<S: Store>(
        &mut self,
        index: u32,
        store: &mut S,
    ) -> Result<(), StoreFault<S::Error>> {
        let Some(base) = self.sales.get(&index) else {
            return Ok(());
        };
        if !base.via_swap || base.exposure_released || !base.exposure_counted {
            return Ok(());
        }
        let mut sale = base.clone();
        self.release_exposure(index, store)?;
        sale.exposure_released = true;
        self.put_sale(store, sale)
    }

    /// CR-221, CR-14: a sale paid by a swap before its lockup was in a
    /// block, whose lockup was replaced or never came to the claim, is a
    /// cancel: the pause tracker and the alarm, the POS told the sale is not
    /// paid, and the sale shows that the goods are not to be handed over.
    /// Once per sale; the exposure stays counted until the owner releases it.
    pub fn swap_cancelled<S: Store>(
        &mut self,
        index: u32,
        now: u64,
        store: &mut S,
    ) -> Result<Vec<Report>, StoreFault<S::Error>> {
        let Some(base) = self.sales.get(&index) else {
            return Ok(Vec::new());
        };
        if !base.via_swap || base.cancel_recorded || base.accepted.is_none() {
            return Ok(Vec::new());
        }
        let mut sale = base.clone();
        self.raise_cancel(index, now, store)?;
        sale.cancel_recorded = true;
        let mut events = vec![Event::Replaced { index, at: now }];
        let mut reports = vec![Report::Alarm { index }];
        if sale.pos == PosState::Paid {
            sale.pos = PosState::Unpaid;
            events.push(Event::PosUnpaid { index, at: now });
            reports.push(Report::PosUnpaid { index });
        }
        self.finish(store, sale, &events)?;
        self.cancel_applied.remove(&index);
        Ok(reports)
    }

    /// CR-117: the shop's own node says an invoice of a sale was settled
    /// after the sale ended, or after it was paid another way, or when the sale
    /// was paid over Lightning by another invoice. It is a late payment
    /// (CR-19): the record `LateLightning`, then the sale, then a
    /// [`Report::LatePayment`] in satoshis. Nothing else changes. An invoice
    /// the sale holds as its own payment, or has recorded already, changes
    /// nothing and reports nothing, so asking again is safe.
    ///
    /// The sale lists the payment hash where it lists a transaction, with the
    /// whole satoshis (at least one) the node received.
    pub fn lightning_late<S: Store>(
        &mut self,
        index: u32,
        paid: LightningPaid,
        now: u64,
        store: &mut S,
    ) -> Result<Vec<Report>, LightningRefused<S::Error>> {
        if self.unreadable.contains(&index) {
            return Err(LightningRefused::Judge(JudgeError::UnknownSale(index)));
        }
        let base = self
            .sales
            .get(&index)
            .ok_or(LightningRefused::UnknownSale(index))?;
        let id = Txid::from_byte_array(paid.payment_hash);
        if base
            .lightning
            .is_some_and(|p| p.payment_hash == paid.payment_hash)
            || base.late.iter().any(|(t, _)| *t == id)
        {
            return Ok(Vec::new());
        }
        if paid.amount_msat == 0 {
            return Err(LightningRefused::TooLittle { sats: 0, least: 1 });
        }
        let sats = paid.sats().max(1);
        let mut sale = base.clone();
        if sale.late.len() < MAX_LATE_PAYMENTS {
            sale.late.push((id, sats));
        } else if sale.late_unlisted {
            return Ok(Vec::new());
        } else {
            sale.late_unlisted = true;
        }
        let event = Event::LateLightning {
            index,
            at: now,
            payment_hash: paid.payment_hash,
            amount_msat: paid.amount_msat,
        };
        self.write_event(store, &event)
            .map_err(LightningRefused::Store)?;
        self.put_sale(store, sale)
            .map_err(LightningRefused::Store)?;
        Ok(vec![Report::LatePayment { index, sats }])
    }

    // CR-10 to CR-19: judging a sale.

    /// CR-10 to CR-19, CR-30 to CR-32: work out the state of a sale. This is
    /// the one place the state is worked out. It asks the watcher for its
    /// status, keeps what must stick, values what has arrived (CR-06,
    /// CR-08) and then applies the order of CR-12.
    ///
    /// Several things can follow from one call, each stored before it is
    /// reported (CR-32):
    ///
    /// - Accepting at once (CR-13). The writes: the exposure ledger, then the
    ///   record `PaidAtOnce`, then the sale. Stopping after the ledger leaves
    ///   exposure counted and the sale not shown as paid; the next call
    ///   finds the exposure already counted and carries on (the ledger is not
    ///   charged twice). If the record or the sale cannot be stored the
    ///   state is `Waiting` and nothing is reported.
    /// - A cancel (CR-14), when a conflict or a reorg first shows on a sale
    ///   that was accepted at once or overridden. The writes: the pause
    ///   (which takes effect in memory even if its write fails, and is
    ///   written again before any payment is next accepted at once), the
    ///   desk with the alarm up, the `Conflict` record, then the sale. A cancel
    ///   is counted once per sale; if the process stops before the sale is
    ///   written it may be counted again after a restart, which only
    ///   tightens.
    /// - Releasing the exposure at the first confirmation (CR-15): the ledger
    ///   is written, then the sale is marked.
    /// - Records for what the POS and the books are told (CR-30, CR-31), and
    ///   for surplus, dust and expiry. The records are written first and the
    ///   sale last, so a report is never made without its record.
    ///
    /// A failed write that no rule above gives a state for is returned as
    /// [`JudgeError::Store`]; the caller must then treat the sale as not safe
    /// and ask again. Nothing is reported when it is returned.
    pub fn judge<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        facts: &Facts<'_, C>,
        store: &mut S,
    ) -> Result<Screen, JudgeError<S::Error>> {
        self.judge_with_own_node(index, facts, &OwnNodeHealth::not_configured(), store)
    }

    /// [`Counter::judge`] with the merchant's own node as a second check
    /// (CR-104). `own` is the health line of the own node as the app's
    /// tracker gives it now; the sale's status is read with it
    /// (`SaleWatch::status_with_own_node`), so the own-node tier of the
    /// instant decision applies only when the status says the own node
    /// counted. With `OwnNodeHealth::not_configured()` this is `judge`.
    pub fn judge_with_own_node<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        facts: &Facts<'_, C>,
        own: &OwnNodeHealth,
        store: &mut S,
    ) -> Result<Screen, JudgeError<S::Error>> {
        self.judge_checked(index, facts, &OwnCheck::Mixed(*own), store)
    }

    /// [`Counter::judge`] with `node_only` on (NR-105, WR-66 to WR-68):
    /// `health` is the node-only health line as the app's
    /// `NodeOnlyTracker` gives it now. A payment the own node reports while
    /// it can be used is judged in the mode `CheckMode::OwnNodeOnly`; the
    /// light client's groups count only with `light_fallback` on and the
    /// random peers running (WR-68).
    pub fn judge_with_node_only<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        facts: &Facts<'_, C>,
        health: &NodeOnlyHealth,
        store: &mut S,
    ) -> Result<Screen, JudgeError<S::Error>> {
        self.judge_checked(index, facts, &OwnCheck::NodeOnly(*health), store)
    }

    fn judge_checked<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        facts: &Facts<'_, C>,
        own: &OwnCheck,
        store: &mut S,
    ) -> Result<Screen, JudgeError<S::Error>> {
        if self.unreadable.contains(&index) {
            return Ok(screen_of(
                State::NeedsAPerson,
                Why::StoredFormUnreadable,
                Next::PersonLooks,
            ));
        }
        let mut sale = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(JudgeError::UnknownSale(index))?;
        if let Some((end, _)) = sale.ended {
            return Ok(ended_screen(&sale, end));
        }
        self.retry_pause(store);

        let now = facts.now;
        let depth = self.depth();
        let final_depth = u32::from(self.settings.final_confirmations).max(depth);
        let (st, received) = sale.refresh(facts.chain, now, depth, own);
        let needs = sale.watch.sale_sats();

        // CR-109, CR-110: a sale the shop's node says was paid over
        // Lightning is paid and final, whatever its address shows: coins
        // that arrive there are a late payment (CR-19) and nothing else is
        // judged.
        if let Some(paid) = sale.lightning {
            // CR-221: a swap payment accepted before its lockup was in a
            // block, whose lockup was then replaced, is a cancel (CR-14).
            if sale.via_swap && sale.cancel_recorded {
                let mut screen = screen_of(
                    State::DoNotHandOver,
                    Why::Replaced,
                    Next::AskOwnerOrOtherPayment,
                );
                screen.received_sats = paid.sats();
                screen.needs_sats = paid.sats();
                screen.late = sale.is_late();
                return Ok(screen);
            }
            let mut screen = screen_of(State::PaidLightning, Why::PaidByLightning, Next::Nothing);
            screen.received_sats = paid.sats();
            screen.needs_sats = paid.sats();
            screen.late = sale.is_late();
            return Ok(screen);
        }
        let confirmed = st.min_confirmations >= depth;
        let is_final = st.min_confirmations >= final_depth;
        let mut events: Vec<Event> = Vec::new();
        let mut reports: Vec<Report> = Vec::new();

        // CR-17: dust is logged and changes no state.
        if st.received_sats > 0 && st.received_sats < DUST_SATS && !sale.dust_logged {
            sale.dust_logged = true;
            events.push(Event::Dust {
                index,
                at: now,
                sats: st.received_sats,
            });
            reports.push(Report::Dust {
                index,
                sats: st.received_sats,
            });
        }

        // CR-07: nothing (above dust) arrived inside the time. A sale that
        // was accepted or overridden, or that saw a conflict, had a payment
        // and does not expire.
        if received == 0
            && !st.needs_a_person
            && !st.conflict_seen
            && sale.accepted.is_none()
            && sale.overridden.is_none()
            && now >= sale.expires_at()
            && !self.swap_pending.contains(&index)
        {
            sale.ended = Some((End::Expired, now));
            events.push(Event::Expired { index, at: now });
            self.finish(store, sale.clone(), &events)
                .map_err(JudgeError::Store)?;
            let mut screen = ended_screen(&sale, End::Expired);
            screen.reports = reports;
            return Ok(screen);
        }

        // CR-12: the order of judgment. An override remembers the flags as
        // they were; rules 2 and 3 apply again only to what is new.
        let (mark_conflict, mark_reopened) = sale
            .overridden
            .as_ref()
            .map_or((false, false), |m| (m.conflict, m.reopened));
        let reopened_eff = sale.reopened && !mark_reopened && !confirmed;
        let conflict_eff = st.conflict_seen && !mark_conflict && !confirmed;
        let mut instant: Option<(openbmt_core::policy::InstantLabel, CheckMode)> = None;
        let (mut state, mut why, mut next);
        if st.needs_a_person {
            (state, why, next) = (
                State::NeedsAPerson,
                Why::WatcherNeedsAPerson,
                Next::PersonLooks,
            );
        } else if reopened_eff {
            (state, why, next) = (
                State::DoNotHandOver,
                Why::Reopened,
                Next::AskOwnerOrOtherPayment,
            );
        } else if conflict_eff {
            (state, why, next) = (
                State::DoNotHandOver,
                Why::Replaced,
                Next::AskOwnerOrOtherPayment,
            );
        } else if is_final {
            (state, why, next) = (
                State::Final,
                Why::FinalAt(st.min_confirmations),
                Next::Nothing,
            );
        } else if confirmed {
            (state, why, next) = (
                State::Confirmed,
                Why::ConfirmedAt(st.min_confirmations),
                Next::FinalAt { depth: final_depth },
            );
        } else if let Some(mark) = &sale.overridden {
            (state, why, next) = (
                State::Overridden,
                Why::OverriddenBy { role: mark.role },
                Next::WaitForConfirmation { depth },
            );
        } else if let Some((label, _)) = sale.accepted {
            (state, why, next) = (
                State::PaidInstant,
                Why::Instant(label),
                Next::WaitForConfirmation { depth },
            );
        } else if sale.frozen.is_some() {
            // Rule 7: enough has arrived, unconfirmed.
            (state, why, next) = (State::Waiting, Why::NotSeenEnough, Next::KeepWatching);
            if !facts.health.healthy {
                (state, why, next) = (
                    State::DoNotHandOver,
                    Why::CannotCheck(facts.health.problem),
                    Next::AskOwnerOrOtherPayment,
                );
            } else if sale.is_request() {
                // CR-208: a request is paid when what arrived is at the
                // confirmation depth, as a sale above the instant limits is.
                (why, next) = (
                    Why::Wait(WaitReason::OverSaleLimit),
                    Next::WaitForConfirmation { depth },
                );
            } else if st.mode.is_none() {
                // Waiting: not yet seen by enough of the network.
            } else if self.pause_unsaved {
                (why, next) = (Why::PauseNotSaved, Next::WaitForConfirmation { depth });
            } else if let Some(mode) = st.mode {
                let id = sale_payment_id(index);
                // The sale's own exposure, counted by an earlier attempt that
                // stopped, is not counted against it again.
                let outstanding = {
                    let mut ledger = self.exposure.clone();
                    ledger.release_by_owner(&id);
                    ledger.outstanding()
                };
                let mut policy =
                    InstantPolicy::for_counter(&self.settings, &self.identity.device_id());
                policy.underpayment_tolerance_bps = sale.tolerance_bps;
                let inputs = Inputs {
                    mode,
                    sale_sats: needs,
                    received_sats: received,
                    outstanding_sats: outstanding,
                    check_healthy: facts.health.healthy,
                    heartbeat: facts.heartbeat,
                    remote_order: facts.remote_order,
                    customer_wifi: self.settings.customer_wifi,
                    tor_on: self.settings.tor_on,
                    pause: self.pause.status(now),
                    conflict_seen: st.conflict_seen,
                    secs_since_first_seen: st.secs_since_first_seen,
                };
                match evaluate(&policy, &inputs) {
                    Decision::Instant { label, mode } => {
                        instant = Some((label, mode));
                    }
                    Decision::WaitForConfirmation { reason } => {
                        why = Why::Wait(reason);
                        next = match reason {
                            WaitReason::ConflictWatch { secs_left } => {
                                Next::WatchSeconds { secs_left }
                            }
                            _ => Next::WaitForConfirmation { depth },
                        };
                    }
                    Decision::InstantOff { reason } => {
                        why = Why::InstantOff(reason);
                        next = match reason {
                            OffReason::PausedUntilOwnerReset => Next::OwnerClears,
                            _ => Next::WaitForConfirmation { depth },
                        };
                    }
                }
            }
        } else if received > 0 {
            (state, why, next) = (
                State::PartPaid,
                Why::Part {
                    received_sats: received,
                    needs_sats: needs,
                },
                Next::CustomerPaysRest {
                    missing_sats: needs.saturating_sub(received),
                },
            );
        } else {
            (state, why, next) = (State::NotPaidYet, Why::NothingArrived, Next::CustomerPays);
        }

        // CR-13: accept at once.
        let mut accepted_now = false;
        if let Some((label, mode)) = instant {
            match self.accept_exposure(index, amount_at_risk(needs, received), mode, store) {
                Ok(()) => {
                    sale.accepted = Some((label, mode));
                    sale.exposure_counted = true;
                    accepted_now = true;
                    (state, why, next) = (
                        State::PaidInstant,
                        Why::Instant(label),
                        Next::WaitForConfirmation { depth },
                    );
                }
                Err(AcceptFail::SliceFull) => {
                    (state, why, next) = (
                        State::Waiting,
                        Why::Wait(WaitReason::SliceFull),
                        Next::WaitForConfirmation { depth },
                    );
                }
                Err(AcceptFail::NotStored) => {
                    (state, why, next) = (
                        State::Waiting,
                        Why::NotStored,
                        Next::WaitForConfirmation { depth },
                    );
                }
            }
        }

        // CR-14: a conflict or a reorg on a sale that was accepted at once
        // or overridden is a cancel.
        let mut conflict_event: Option<Event> = None;
        if (sale.accepted.is_some() || sale.overridden.is_some())
            && !sale.cancel_recorded
            && (reopened_eff || conflict_eff)
        {
            self.raise_cancel(index, now, store)
                .map_err(JudgeError::Store)?;
            sale.cancel_recorded = true;
            reports.push(Report::Alarm { index });
            conflict_event = Some(if conflict_eff {
                Event::Replaced { index, at: now }
            } else {
                Event::Reopened { index, at: now }
            });
        }

        // CR-15: exposure leaves the ledger at the first confirmation.
        if st.min_confirmations >= 1 && !sale.exposure_released {
            self.release_exposure(index, store)
                .map_err(JudgeError::Store)?;
            sale.exposure_released = true;
        }

        // CR-30 to CR-32: what the POS and the books are told.
        if state.is_paid() {
            // A sale that is reported paid again after it was reported
            // unpaid gets a record again.
            if matches!(state, State::Confirmed | State::Final)
                && (!sale.confirmed_logged || sale.pos != PosState::Paid)
            {
                sale.confirmed_logged = true;
                events.push(Event::Confirmed {
                    index,
                    at: now,
                    received_sats: received,
                    depth: st.min_confirmations,
                });
            }
            if sale.pos != PosState::Paid {
                if state == State::PaidInstant {
                    if let Some((label, mode)) = sale.accepted {
                        events.push(Event::PaidAtOnce {
                            index,
                            at: now,
                            at_risk_sats: amount_at_risk(needs, received),
                            label,
                            mode,
                        });
                    }
                } else if let Some(mark) = &sale.overridden {
                    // Reported paid again after being reported unpaid: the
                    // override is written down again.
                    if state == State::Overridden {
                        events.push(Event::Override {
                            index,
                            at: now,
                            role: mark.role,
                            name: mark.name.clone(),
                            sats: amount_at_risk(needs, received),
                        });
                    }
                }
                sale.pos = PosState::Paid;
                events.push(Event::PosPaid { index, at: now });
                reports.push(Report::PosPaid { index });
            }
            if state == State::Final && !sale.books_final {
                sale.books_final = true;
                let standing: Vec<([u8; 32], u64)> = sale
                    .watch
                    .standing()
                    .into_iter()
                    .map(|(txid, sats)| (txid.to_byte_array(), sats))
                    .collect();
                let (payments, more) = final_payments(&standing);
                events.push(Event::Final {
                    index,
                    at: now,
                    received_sats: received,
                    depth: st.min_confirmations,
                    first_seen_at: sale.watch.standing_first_seen().unwrap_or(now),
                    valued: sale.valued_amount(received, now),
                    payments,
                    more,
                });
                reports.push(Report::BooksFinal {
                    index,
                    amount: sale.own_amount(),
                    currency: sale.currency,
                    received_sats: received,
                });
            }
            // CR-16: more than the sale needs is paid in full; the rest is
            // owed to the customer.
            if let Some(frozen) = sale.frozen {
                let surplus = received.saturating_sub(frozen);
                if surplus > sale.surplus_reported {
                    sale.surplus_reported = surplus;
                    events.push(Event::SurplusOwed {
                        index,
                        at: now,
                        sats: surplus,
                    });
                    reports.push(Report::SurplusOwed {
                        index,
                        sats: surplus,
                    });
                }
            }
        } else if state == State::DoNotHandOver && sale.pos == PosState::Paid {
            sale.pos = PosState::Unpaid;
            events.push(Event::PosUnpaid { index, at: now });
            reports.push(Report::PosUnpaid { index });
            if conflict_event.is_none() {
                conflict_event = Some(if reopened_eff {
                    Event::Reopened { index, at: now }
                } else {
                    Event::Replaced { index, at: now }
                });
            }
        }
        if let Some(event) = conflict_event {
            events.push(event);
        }

        let stuck =
            received > 0 && st.min_confirmations == 0 && st.secs_since_first_seen >= STUCK_SECS;
        let screen = Screen {
            state,
            why,
            next,
            received_sats: received,
            needs_sats: needs,
            stuck,
            late: sale.is_late(),
            check_mode: st.mode,
            own_node_saw: st.own_node_saw,
            own_node_counted: st.own_node_counted,
            conflict_seen: st.conflict_seen,
            reopened: sale.reopened,
            reports: Vec::new(),
        };
        let cancel_stored = sale.cancel_recorded;
        match self.finish(store, sale, &events) {
            Ok(()) => {
                if cancel_stored {
                    self.cancel_applied.remove(&index);
                }
                Ok(Screen { reports, ..screen })
            }
            Err(_) if accepted_now => Ok(Screen {
                state: State::Waiting,
                why: Why::NotStored,
                next: Next::WaitForConfirmation { depth },
                ..screen
            }),
            Err(e) => Err(JudgeError::Store(e)),
        }
    }

    /// CR-13: count the amount at risk in the exposure ledger, under the
    /// sale, against the slice of the mode the decision used. An earlier
    /// attempt that stored the exposure and stopped is not an error.
    fn accept_exposure<S: Store>(
        &mut self,
        index: u32,
        amount: u64,
        mode: CheckMode,
        store: &mut S,
    ) -> Result<(), AcceptFail> {
        let slice = self.settings.slice_sats(&self.identity.device_id(), mode);
        match self
            .exposure
            .accept_instant(sale_payment_id(index), amount, slice, |bytes| {
                store.put(Slot::Exposure, bytes)
            }) {
            Ok(()) | Err(ExposureWriteError::Refused(ExposureError::AlreadyCounted)) => Ok(()),
            Err(ExposureWriteError::Refused(ExposureError::SliceFull)) => {
                Err(AcceptFail::SliceFull)
            }
            Err(ExposureWriteError::Persist(_)) => Err(AcceptFail::NotStored),
        }
    }

    /// CR-14: the cancel goes into the pause tracker (once per sale), then
    /// the alarm goes up in the desk. The pause takes effect in memory even
    /// if its write fails; it is then written again before the next payment
    /// is accepted at once.
    fn raise_cancel<S: Store>(
        &mut self,
        index: u32,
        now: u64,
        store: &mut S,
    ) -> Result<(), StoreFault<S::Error>> {
        if !self.cancel_applied.contains(&index) {
            let stored = self
                .pause
                .record_cancel(now, |bytes| store.put(Slot::Pause, bytes));
            self.pause_unsaved = stored.is_err();
            self.cancel_applied.insert(index);
        }
        if !self.desk.alarm() {
            let mut desk = self.desk.clone();
            desk.set_alarm(true);
            self.put_desk(store, desk)?;
        }
        Ok(())
    }

    /// CR-15: take a sale's exposure out of the ledger. The ledger as it
    /// will be is stored before it is put in place.
    fn release_exposure<S: Store>(
        &mut self,
        index: u32,
        store: &mut S,
    ) -> Result<(), StoreFault<S::Error>> {
        let mut next = self.exposure.clone();
        if next
            .release_on_first_confirmation(&sale_payment_id(index))
            .is_some()
        {
            put(store, Slot::Exposure, &next.to_bytes())?;
            self.exposure = next;
        }
        Ok(())
    }

    // CR-20 to CR-22: overrides.

    /// CR-20 to CR-22: a person accepts a sale that shows `Waiting`,
    /// `PartPaid` or `DoNotHandOver`. The sale is judged first. The amount at
    /// risk is `max(needs, received)`. A cashier is refused above the
    /// per-sale instant limit of the check mode (`Light` if there is none)
    /// and when the cashier's overrides of the last 24 hours plus this one
    /// pass the cashier's daily cap; a manager when the manager's pass the
    /// ceiling. The caller says who the person is and has checked the PIN.
    ///
    /// The writes, in this order: the exposure ledger (`add_override`
    /// against the slice of the mode; a sale already counted adds nothing but
    /// the limits still count), the desk with the entry, the `Override`
    /// record, then the sale. Stopping after the ledger leaves exposure counted
    /// and the sale not accepted; stopping after the desk leaves an entry that
    /// counts against the person's limits; stopping after the record leaves
    /// a record for an override the sale does not show. Each is the safe side.
    ///
    /// An override does not clear the cancel alarm and does not make the sale
    /// `Confirmed`. The returned reports hold [`Report::PosPaid`] once.
    pub fn override_sale<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        actor: Actor,
        facts: &Facts<'_, C>,
        store: &mut S,
    ) -> Overridden<S::Error> {
        self.override_sale_with_own_node(
            index,
            actor,
            facts,
            &OwnNodeHealth::not_configured(),
            store,
        )
    }

    /// [`Counter::override_sale`] judged with the own node (CR-104): the
    /// sale is judged first, and a cashier's limit is the one of the check
    /// that judging found.
    pub fn override_sale_with_own_node<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        actor: Actor,
        facts: &Facts<'_, C>,
        own: &OwnNodeHealth,
        store: &mut S,
    ) -> Overridden<S::Error> {
        self.override_checked(index, actor, facts, &OwnCheck::Mixed(*own), store)
    }

    /// [`Counter::override_sale`] judged with `node_only` on (NR-105).
    pub fn override_sale_with_node_only<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        actor: Actor,
        facts: &Facts<'_, C>,
        health: &NodeOnlyHealth,
        store: &mut S,
    ) -> Overridden<S::Error> {
        self.override_checked(index, actor, facts, &OwnCheck::NodeOnly(*health), store)
    }

    fn override_checked<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        actor: Actor,
        facts: &Facts<'_, C>,
        own: &OwnCheck,
        store: &mut S,
    ) -> Overridden<S::Error> {
        let mut reports = Vec::new();
        let result = self.override_inner(index, actor, facts, own, store, &mut reports);
        Overridden { reports, result }
    }

    fn override_inner<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        actor: Actor,
        facts: &Facts<'_, C>,
        own: &OwnCheck,
        store: &mut S,
        reports: &mut Vec<Report>,
    ) -> Result<(), OverrideRefused<S::Error>> {
        if !valid_name(&actor.name) {
            return Err(OverrideRefused::BadName);
        }
        let mut screen = match self.judge_checked(index, facts, own, store) {
            Ok(screen) => screen,
            Err(JudgeError::UnknownSale(i)) => return Err(OverrideRefused::UnknownSale(i)),
            Err(e) => return Err(OverrideRefused::Judge(e)),
        };
        reports.append(&mut screen.reports);
        if !matches!(
            screen.state,
            State::Waiting | State::PartPaid | State::DoNotHandOver
        ) {
            return Err(OverrideRefused::WrongState(screen.state));
        }
        let now = facts.now;
        let amount = amount_at_risk(screen.needs_sats, screen.received_sats);
        let mode = screen.check_mode.unwrap_or(CheckMode::Light);
        let me = self.identity.device_id();
        let already = self.desk.override_total(actor.role, &actor.name, now);
        let over =
            |total: u64, limit: u64| u128::from(total) + u128::from(amount) > u128::from(limit);
        match actor.role {
            Role::Cashier => {
                let limit = InstantPolicy::for_counter(&self.settings, &me)
                    .limits(mode)
                    .per_sale_sats;
                if amount > limit {
                    return Err(OverrideRefused::OverSaleLimit {
                        sats: amount,
                        limit,
                    });
                }
                let cap = self.settings.cashier_override_daily_cap.sats;
                if over(already, cap) {
                    return Err(OverrideRefused::OverDailyCap {
                        sats: amount,
                        already,
                        cap,
                    });
                }
            }
            Role::Manager => {
                let ceiling = self.settings.manager_override_ceiling.sats;
                if over(already, ceiling) {
                    return Err(OverrideRefused::OverCeiling {
                        sats: amount,
                        already,
                        ceiling,
                    });
                }
            }
        }
        let mut desk = self.desk.pruned(now);
        if !desk.has_room_for_override() {
            return Err(OverrideRefused::TooManyOverrides);
        }
        let mut sale = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(OverrideRefused::UnknownSale(index))?;

        let slice = self.settings.slice_sats(&me, mode);
        match self
            .exposure
            .add_override(sale_payment_id(index), amount, slice, |bytes| {
                store.put(Slot::Exposure, bytes)
            }) {
            Ok(()) | Err(ExposureWriteError::Refused(ExposureError::AlreadyCounted)) => {}
            Err(ExposureWriteError::Refused(ExposureError::SliceFull)) => {
                return Err(OverrideRefused::NoRoom)
            }
            Err(ExposureWriteError::Persist(e)) => {
                return Err(OverrideRefused::Store(StoreFault::Write(e)))
            }
        }
        desk.push_override(OverrideEntry {
            role: actor.role,
            name: actor.name.clone(),
            at: now,
            sats: amount,
        });
        self.put_desk(store, desk).map_err(OverrideRefused::Store)?;
        let event = Event::Override {
            index,
            at: now,
            role: actor.role,
            name: actor.name.clone(),
            sats: amount,
        };
        sale.overridden = Some(OverrideMark {
            role: actor.role,
            name: actor.name,
            at: now,
            conflict: screen.conflict_seen,
            reopened: screen.reopened,
        });
        sale.exposure_counted = true;
        sale.exposure_released = false;
        let tell_pos = sale.pos != PosState::Paid;
        sale.pos = PosState::Paid;
        let mut events = vec![event];
        if tell_pos {
            events.push(Event::PosPaid { index, at: now });
        }
        self.finish(store, sale, &events)
            .map_err(OverrideRefused::Store)?;
        if tell_pos {
            reports.push(Report::PosPaid { index });
        }
        Ok(())
    }

    // CR-60 to CR-74: refunds, the cashier's side.

    /// CR-60 to CR-69, CR-73: a person asks for a refund of a sale. The
    /// request is a record and is not a payment: nothing is paid until the
    /// owner's device has checked it and an approval session has signed it.
    /// Returns the request number (CR-61).
    ///
    /// The sale is looked at as it is now: what has arrived for it and
    /// whether it has reached the refund depth (CR-66). A sale below the
    /// refund depth is not refused; the request is recorded and shown as
    /// "waiting for depth". Refused, with nothing written, for a bad name or
    /// reason, a destination that is not a script the signer pays, an unknown
    /// sale, a stale rate, a zero amount or one below the destination's dust
    /// limit, one of the wallet's own scripts, a sale with nothing received,
    /// an amount that would take the sale's refunds past what was received,
    /// and a cashier over the single or the daily cap.
    ///
    /// The writes, in this order: the desk with the next request number
    /// (CR-73), a `Refund` record "requested", then the sale with the new
    /// entry. If the process stops after the desk, a number is wasted and
    /// never reused. If it stops after the record, the record stands: the
    /// caller is told the call failed, the views already show the request,
    /// and the sale is written again before the next refund call (CR-73).
    pub fn request_refund<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        request: RefundRequest,
        now: u64,
        headers: &C,
        store: &mut S,
    ) -> Result<u64, RefundRefused<S::Error>> {
        let RefundRequest {
            ask,
            destination,
            actor,
            reason,
        } = request;
        self.settle_refunds(store).map_err(RefundRefused::Store)?;
        if !refund_text_ok(&actor.name) {
            return Err(RefundRefused::BadText("name"));
        }
        if !refund_text_ok(&reason) {
            return Err(RefundRefused::BadText("reason"));
        }
        let dust = dust_limit(&destination).ok_or(RefundRefused::DestinationKind)?;
        let base = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(RefundRefused::UnknownSale(index))?;
        let (sats, ask) = match ask {
            RefundAsk::Sats(sats) => (sats, Ask::Sats),
            RefundAsk::Value {
                minor,
                rate,
                source,
            } => {
                let (rate, source) = self.priced_at(rate, &source, now);
                if !source.is_fresh_at(now) {
                    return Err(RefundRefused::StaleRate {
                        fetched_at: source.fetched_at(),
                        now,
                    });
                }
                let sats = sats_floor(minor, rate).ok_or(RefundRefused::AmountTooLarge)?;
                let ask = Ask::Value {
                    minor,
                    minor_per_btc: rate.minor_per_btc(),
                    source,
                };
                (sats, ask)
            }
        };
        if sats == 0 {
            return Err(RefundRefused::ZeroAmount);
        }
        if sats > MAX_SATS {
            return Err(RefundRefused::AmountTooLarge);
        }
        if sats < dust {
            return Err(RefundRefused::BelowDust { sats, dust });
        }
        if self.holds_own_script(&destination) {
            return Err(RefundRefused::OwnScript);
        }
        let (received, confirmations) = self.refund_facts(&base, now, headers);
        let height = chain_height(headers);
        self.refund_limits(&base, &actor, false, sats, received, now, height)?;
        // CR-217: Lightning has nothing on the chain for the owner to check.
        if let Some(room) = self.onchain_refund_room(&base, now, headers) {
            if sats > room {
                return Err(RefundRefused::LightningCashOnly { room, sats });
            }
        }
        let final_depth = self.final_depth();
        let number = self.desk.last_refund_no().saturating_add(1);
        let mut desk = self.desk.clone();
        desk.set_last_refund_no(number);
        self.put_desk(store, desk).map_err(RefundRefused::Store)?;
        let value_minor = match &ask {
            Ask::Sats => None,
            Ask::Value { minor, .. } => Some(*minor),
        };
        let event = Event::RefundRequested {
            index,
            at: now,
            number,
            role: actor.role,
            name: actor.name.clone(),
            reason,
            destination,
            received_sats: received,
            waiting_for_depth: confirmations < final_depth,
            sats,
            ask,
            chain_height: height,
        };
        let mut sale = base;
        sale.refunds.push(RefundEntry {
            kind: EntryKind::Request,
            number,
            at: now,
            role: actor.role,
            name: actor.name,
            sats,
            value_minor,
            withdrawn: false,
            height,
        });
        let done = self.finish(store, sale, &[event]);
        // A record that was written stands even if the sale was not (CR-73).
        self.adopt_refunds();
        done.map_err(RefundRefused::Store)?;
        Ok(number)
    }

    /// CR-70, CR-73: withdraw a request. A `Refund` record "withdrawn" is
    /// written, then the sale with the entry marked. The counter cannot know
    /// whether the owner has taken the request: the owner honours the record
    /// only if the refund is still queued and not in an open session. A
    /// withdrawn request no longer counts for "never more than received"
    /// (CR-67), and still counts for the cashier's cap (CR-69).
    ///
    /// If the process stops between the two writes the record stands and
    /// the request is withdrawn from then on (CR-73).
    pub fn withdraw_refund<S: Store>(
        &mut self,
        index: u32,
        number: u64,
        actor: Actor,
        now: u64,
        store: &mut S,
    ) -> Result<(), WithdrawRefused<S::Error>> {
        self.settle_refunds(store).map_err(WithdrawRefused::Store)?;
        if !refund_text_ok(&actor.name) {
            return Err(WithdrawRefused::BadName);
        }
        let mut sale = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(WithdrawRefused::UnknownSale(index))?;
        let entry = sale
            .refunds
            .iter_mut()
            .find(|e| e.kind == EntryKind::Request && e.number == number)
            .ok_or(WithdrawRefused::UnknownRequest(number))?;
        if entry.withdrawn {
            return Err(WithdrawRefused::AlreadyWithdrawn(number));
        }
        entry.withdrawn = true;
        let event = Event::RefundWithdrawn {
            index,
            at: now,
            number,
            role: actor.role,
            name: actor.name,
        };
        let done = self.finish(store, sale, &[event]);
        self.adopt_refunds();
        done.map_err(WithdrawRefused::Store)
    }

    /// CR-71, CR-73: a refund paid in cash from the till. The amount is in
    /// the shop's currency; it is valued at the sale's last quote, rounded up
    /// to a whole satoshi, and counts for "never more than received" (CR-67)
    /// and for the cashier's cap (CR-69). It asks the owner for nothing and
    /// moves no bitcoin. Refused for the same reasons as a request, except
    /// those of a destination and a rate.
    ///
    /// The writes, in this order: a `Refund` record "paid in cash", then the
    /// sale. If the process stops between them the record stands and the
    /// refund is on the sale from then on (CR-73).
    pub fn refund_in_cash<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        index: u32,
        cash: CashRefund,
        now: u64,
        headers: &C,
        store: &mut S,
    ) -> Result<(), RefundRefused<S::Error>> {
        let CashRefund {
            actor,
            minor,
            reason,
        } = cash;
        self.settle_refunds(store).map_err(RefundRefused::Store)?;
        if !refund_text_ok(&actor.name) {
            return Err(RefundRefused::BadText("name"));
        }
        if !refund_text_ok(&reason) {
            return Err(RefundRefused::BadText("reason"));
        }
        let base = self
            .sales
            .get(&index)
            .cloned()
            .ok_or(RefundRefused::UnknownSale(index))?;
        if minor.0 == 0 {
            return Err(RefundRefused::ZeroAmount);
        }
        let rate = base
            .quotes
            .last()
            .map(|q| q.rate)
            .ok_or(RefundRefused::UnknownSale(index))?;
        let sats = sats_for(minor, rate).ok_or(RefundRefused::AmountTooLarge)?;
        let (received, _) = self.refund_facts(&base, now, headers);
        let height = chain_height(headers);
        self.refund_limits(&base, &actor, true, sats, received, now, height)?;
        let event = Event::RefundPaidInCash {
            index,
            at: now,
            role: actor.role,
            name: actor.name.clone(),
            reason,
            minor: minor.0,
            sats,
            chain_height: height,
        };
        let mut sale = base;
        sale.refunds.push(RefundEntry {
            kind: EntryKind::Cash,
            number: 0,
            at: now,
            role: actor.role,
            name: actor.name,
            sats,
            value_minor: Some(minor.0),
            withdrawn: false,
            height,
        });
        let done = self.finish(store, sale, &[event]);
        self.adopt_refunds();
        done.map_err(RefundRefused::Store)
    }

    /// CR-73: make every sale's refund entries what the counter's own
    /// records say, in memory. A request, a withdrawal or a cash refund whose
    /// record was written and whose sale was not (the process stopped, or the
    /// write failed) is taken in, in record order; requests are numbered as
    /// their records say, and the desk's last number is never below the
    /// highest of them. A sale that now differs from what is stored is
    /// marked, and [`Self::settle_refunds`] writes it before the next refund
    /// call acts. Nothing is dropped from the records, so the result is the
    /// same however often it is run.
    fn adopt_refunds(&mut self) {
        for (index, sale) in &mut self.sales {
            let entries = self.refund_book.get(index).cloned().unwrap_or_default();
            if sale.refunds != entries {
                sale.refunds = entries;
                self.refund_unsaved.insert(*index);
            }
        }
        if self.highest_refund > self.desk.last_refund_no() {
            self.desk.set_last_refund_no(self.highest_refund);
        }
    }

    /// CR-73: write the sales that [`Self::adopt_refunds`] changed. A refund
    /// call does nothing until this has worked, so that it never acts on a
    /// sale that the store does not hold.
    fn settle_refunds<S: Store>(&mut self, store: &mut S) -> Result<(), StoreFault<S::Error>> {
        self.adopt_refunds();
        let pending: Vec<u32> = self.refund_unsaved.iter().copied().collect();
        for index in pending {
            if let Some(sale) = self.sales.get(&index).cloned() {
                put(store, Slot::Sale(index), &sale.to_bytes())?;
            }
            self.refund_unsaved.remove(&index);
        }
        Ok(())
    }

    /// CR-72: the refunds of a sale, oldest first, for the screen and the
    /// receipt. Empty for a sale the counter does not hold.
    pub fn refund_views(&self, index: u32) -> Vec<RefundView> {
        let Some(sale) = self.sales.get(&index) else {
            return Vec::new();
        };
        // CR-192: what each entry asks for is counted against the sale's own
        // amount first; the part that goes past it is the tip, up to the tip
        // the sale holds. A withdrawn request asks for nothing any more.
        let rate = sale.quotes.last().map(|q| q.rate);
        let own = sale.own_amount().0;
        let tip_within = |cumulative: u64| cumulative.saturating_sub(own).min(sale.tip.0);
        let mut cumulative = 0u64;
        sale.refunds
            .iter()
            .map(|e| {
                let value = match (e.value_minor, rate) {
                    _ if e.withdrawn => 0,
                    (Some(minor), _) => minor,
                    (None, Some(rate)) => value_of(e.sats, rate).0,
                    (None, None) => 0,
                };
                let before = tip_within(cumulative);
                cumulative = cumulative.saturating_add(value);
                (e, tip_within(cumulative).saturating_sub(before))
            })
            .map(|(e, tip_minor)| RefundView {
                sale: index,
                number: (e.kind == EntryKind::Request).then_some(e.number),
                asked: e.sats,
                value_minor: e.value_minor,
                tip_minor,
                state: match e.kind {
                    EntryKind::Cash => RefundState::PaidInCash,
                    EntryKind::Request if e.withdrawn => RefundState::Withdrawn,
                    EntryKind::Request if sale.at_refund_depth() => RefundState::Requested,
                    EntryKind::Request => RefundState::WaitingForDepth,
                },
                by: Actor {
                    role: e.role,
                    name: e.name.clone(),
                },
                at: e.at,
            })
            .collect()
    }

    /// CR-217: what a bitcoin refund of a sale paid over Lightning can still
    /// ask: what arrived on-chain for it, less the requests that stand. `None`
    /// for a sale that no Lightning payment settled, which has no such limit
    /// beyond CR-67. `Some(0)` means the sale is refunded in cash only.
    pub fn refund_onchain_room<C: HeaderChain + ?Sized>(
        &self,
        index: u32,
        now: u64,
        headers: &C,
    ) -> Option<u64> {
        let sale = self.sales.get(&index)?;
        self.onchain_refund_room(sale, now, headers)
    }

    fn onchain_refund_room<C: HeaderChain + ?Sized>(
        &self,
        sale: &Sale,
        now: u64,
        headers: &C,
    ) -> Option<u64> {
        let paid = sale.lightning.as_ref()?;
        // CR-224: a payment through a swap provider is on the chain, in
        // the claim to the sale's own address: no limit but CR-67.
        if sale.via_swap {
            return None;
        }
        let (received, _) = self.refund_facts(sale, now, headers);
        let onchain = received.saturating_sub(paid.sats());
        let asked = sale
            .refunds
            .iter()
            .filter(|e| e.kind == EntryKind::Request && !e.withdrawn)
            .fold(0u64, |sum, e| sum.saturating_add(e.sats));
        Some(onchain.saturating_sub(asked))
    }

    /// The refund depth (CR-66): the setting `final_confirmations`, and never
    /// below the shop's own depth (as CR-12 reads it).
    fn final_depth(&self) -> u32 {
        u32::from(self.settings.final_confirmations).max(self.depth())
    }

    /// What has arrived for a sale and how deep it is, looked at on a copy:
    /// the sale itself is not changed (CR-65, CR-66).
    fn refund_facts<C: HeaderChain + ?Sized>(
        &self,
        sale: &Sale,
        now: u64,
        headers: &C,
    ) -> (u64, u32) {
        let mut copy = sale.clone();
        let (status, received) = copy.refresh(
            headers,
            now,
            self.depth(),
            &OwnCheck::Mixed(OwnNodeHealth::not_configured()),
        );
        // CR-111: what arrived over Lightning is received and final at once.
        if let Some(paid) = &sale.lightning {
            return (received.saturating_add(paid.sats()), u32::MAX);
        }
        (received, status.min_confirmations)
    }

    /// CR-65, CR-67, CR-69, CR-119: the checks that a request and a cash
    /// refund share, for `sats` more asked on a sale that has `received`.
    /// `cash` says the call is a cash refund; `height` is the chain height
    /// now, when known (CR-120).
    #[allow(clippy::too_many_arguments)]
    fn refund_limits<E>(
        &self,
        sale: &Sale,
        actor: &Actor,
        cash: bool,
        sats: u64,
        received: u64,
        now: u64,
        height: Option<u32>,
    ) -> Result<(), RefundRefused<E>> {
        if received == 0 {
            return Err(RefundRefused::NothingReceived);
        }
        if sale.refunds.len() >= MAX_REFUNDS_PER_SALE {
            return Err(RefundRefused::TooManyRefunds);
        }
        let sum = |cashier_cash: bool| {
            sale.refunds
                .iter()
                .filter(|e| e.counts_as_asked(cashier_cash))
                .fold(0u64, |sum, e| sum.saturating_add(e.sats))
        };
        let asked = sum(false);
        if u128::from(asked) + u128::from(sats) > u128::from(received) {
            return Err(RefundRefused::MoreThanReceived {
                received,
                asked,
                sats,
            });
        }
        // CR-119: the owner may still pay a withdrawn request, so a cashier's
        // cash refund counts it too.
        if cash && actor.role == Role::Cashier {
            let with_withdrawn = sum(true);
            if u128::from(with_withdrawn) + u128::from(sats) > u128::from(received) {
                return Err(RefundRefused::WithdrawnMayBePaid {
                    received,
                    asked: with_withdrawn,
                    sats,
                });
            }
        }
        if actor.role == Role::Cashier {
            let limit = self.settings.refund_cap.sats;
            if sats > limit {
                return Err(RefundRefused::NeedsManagerSingle { sats, limit });
            }
            let already = self.refund_total(actor, now, height);
            let cap = self.settings.refund_daily_cap.sats;
            if u128::from(already) + u128::from(sats) > u128::from(cap) {
                return Err(RefundRefused::NeedsManagerDaily { sats, already, cap });
            }
        }
        Ok(())
    }

    /// CR-69, CR-120: what a person has asked for in refunds inside the
    /// window before `now`, over every sale: requests (withdrawn ones too)
    /// and cash refunds. The window is a day by the clock, and also a day of
    /// blocks when the heights are known. The sum saturates.
    fn refund_total(&self, actor: &Actor, now: u64, height: Option<u32>) -> u64 {
        self.sales
            .values()
            .flat_map(|s| s.refunds.iter())
            .filter(|e| e.role == actor.role && e.name == actor.name)
            .filter(|e| e.counts_for_cap(now, height))
            .fold(0u64, |sum, e| sum.saturating_add(e.sats))
    }

    // CR-14: the owner's commands.

    /// CR-14: carry out an owner command. It is checked against the pinned
    /// roots, this counter and the last number taken. Nothing at the counter
    /// clears the alarm, ends a pause or releases exposure except this.
    ///
    /// The writes: the desk with the new command number first (and, for
    /// `ClearCancelAlarm`, with the alarm down in the same write, so the
    /// number and the clearing cannot part), then the pause tracker or the
    /// ledger for the other actions. A command is used up as soon as its
    /// number is stored, even if the later write fails: a command that could
    /// be played again is worse than one that is lost, and the owner can send
    /// another. A lost reset leaves the pause on and a lost release leaves
    /// the exposure counted; both are the safe side.
    pub fn command<S: Store>(
        &mut self,
        signed: &SignedCommand,
        _now: u64,
        store: &mut S,
    ) -> Result<(), CommandRefused<S::Error>> {
        signed
            .verify_owner(
                &self.owner,
                &self.identity.device_id(),
                self.desk.last_command_seq(),
            )
            .map_err(CommandRefused::Refused)?;
        let mut desk = self.desk.clone();
        desk.set_last_command_seq(signed.command.seq);
        if signed.command.action == Action::ClearCancelAlarm {
            desk.set_alarm(false);
        }
        self.put_desk(store, desk).map_err(CommandRefused::Store)?;
        match signed.command.action {
            Action::ClearCancelAlarm => {}
            Action::ResetPause => {
                let mut pause = self.pause.clone();
                pause.owner_reset();
                put(store, Slot::Pause, &pause.to_bytes()).map_err(CommandRefused::Store)?;
                self.pause = pause;
                self.pause_unsaved = false;
            }
            Action::ReleaseExposure(id) => {
                let mut ledger = self.exposure.clone();
                if ledger.release_by_owner(&id).is_some() {
                    put(store, Slot::Exposure, &ledger.to_bytes())
                        .map_err(CommandRefused::Store)?;
                    self.exposure = ledger;
                }
            }
        }
        Ok(())
    }

    // CR-50 to CR-56: the owner's heartbeat.

    /// CR-50: take a heartbeat the owner signed. It is checked against the
    /// pinned owner root and the watch key of the settings in force (CR-186),
    /// this counter and the last number taken, and then
    /// judged against the counter's own header chain and clock (CR-51 to
    /// CR-53). Nothing but this call makes a heartbeat count (CR-55).
    ///
    /// A heartbeat timed too far ahead of the clock is refused and its number
    /// is not used up. Every other heartbeat that passes the checks is taken:
    /// its number is stored first (CR-54), and then what the counter makes of
    /// it is kept, so that an unhealthy heartbeat, or one whose tip the
    /// counter does not hold, replaces a verified one with "not verified".
    /// If the store fails, nothing changed. The judgment is made now and is
    /// not made again as the chain moves: the next heartbeat is.
    pub fn heartbeat<C: HeaderChain + ?Sized, S: Store>(
        &mut self,
        signed: &SignedHeartbeat,
        chain: &C,
        now: u64,
        store: &mut S,
    ) -> Result<HeartbeatView, HeartbeatRefused<S::Error>> {
        let hb = signed
            .verify_owner(
                &self.owner,
                self.settings.watch_key.as_ref(),
                &self.identity.device_id(),
                self.desk.last_heartbeat_seq(),
            )
            .map_err(HeartbeatRefused::Refused)?;
        let view = judge_heartbeat(hb, chain, now);
        if let HeartbeatView::NotVerified(NotVerified::FromTheFuture { ahead_secs }) = view {
            return Err(HeartbeatRefused::FromTheFuture { ahead_secs });
        }
        let mut desk = self.desk.clone();
        desk.set_last_heartbeat_seq(hb.seq);
        self.put_desk(store, desk)
            .map_err(HeartbeatRefused::Store)?;
        self.heartbeat = Some(view);
        Ok(view)
    }

    /// CR-50, CR-51: the heartbeat as the instant decision reads it at
    /// `now`: verified only if the last one taken counts, and then as old as
    /// `now` less its own time. With none taken since the start (CR-54), it
    /// is not verified.
    pub fn heartbeat_for_policy(&self, now: u64) -> Heartbeat {
        match self.heartbeat {
            Some(HeartbeatView::Verified { sent_at }) => Heartbeat {
                verified: true,
                age_secs: now.saturating_sub(sent_at),
            },
            _ => Heartbeat {
                verified: false,
                age_secs: 0,
            },
        }
    }
}

/// CR-120: the chain height the counter's headers are at, or none when they
/// are at the start (height zero) and nothing is known.
fn chain_height<C: HeaderChain + ?Sized>(headers: &C) -> Option<u32> {
    Some(headers.tip_height()).filter(|h| *h > 0)
}

/// CR-73: fold one refund event into what the sales' refund entries are.
/// Events are taken in record order; a withdrawal belongs to the request of
/// its number.
fn fold_refund(book: &mut BTreeMap<u32, Vec<RefundEntry>>, highest: &mut u64, event: Event) {
    match event {
        Event::RefundRequested {
            index,
            at,
            number,
            role,
            name,
            sats,
            ask,
            chain_height,
            ..
        } => {
            *highest = (*highest).max(number);
            book.entry(index).or_default().push(RefundEntry {
                kind: EntryKind::Request,
                number,
                at,
                role,
                name,
                sats,
                value_minor: match ask {
                    Ask::Sats => None,
                    Ask::Value { minor, .. } => Some(minor),
                },
                withdrawn: false,
                height: chain_height,
            });
        }
        Event::RefundWithdrawn { index, number, .. } => {
            if let Some(entry) = book
                .entry(index)
                .or_default()
                .iter_mut()
                .find(|e| e.kind == EntryKind::Request && e.number == number)
            {
                entry.withdrawn = true;
            }
        }
        Event::RefundPaidInCash {
            index,
            at,
            role,
            name,
            minor,
            sats,
            chain_height,
            ..
        } => book.entry(index).or_default().push(RefundEntry {
            kind: EntryKind::Cash,
            number: 0,
            at,
            role,
            name,
            sats,
            value_minor: Some(minor),
            withdrawn: false,
            height: chain_height,
        }),
        _ => {}
    }
}
