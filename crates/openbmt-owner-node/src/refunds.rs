//! The owner's intake of the counters' refund requests.
//!
//! A counter writes a refund request as a record (`docs/COUNTER-RULES.md`
//! CR-60 to CR-71). The record travels to the owner with the others and is
//! taken into the owner's verified copy of the counter's chain (`copy`,
//! LK-58). Nothing is queued by the counter: a request becomes a queued
//! refund here, and only if the owner's own queue accepts it
//! (`Owner::queue_refund`, AS-51, AS-52). Nothing is paid until an approval
//! session.
//!
//! [`RefundIntake`] holds what the owner node must remember about that: how
//! far into each counter's chain the requests have been applied, which queued
//! refund each request became, and the list of what could not be applied. It
//! is stored in a file of its own next to the copies (`refund-intake`),
//! written whole to a temporary file and renamed. The app calls
//! [`RefundIntake::run`] after each turn of the link; a copy that has not
//! grown since the last call costs nothing.
//!
//! Where the rules are silent:
//!
//! - Only records that the counter itself wrote are read, as the books do
//!   (BK-02). The copy has already checked every signature and the chain
//!   (LK-58), so a record with a wrong signature is never in it.
//! - **Applied once.** The position in the counter's chain up to which
//!   requests were applied is stored before the effect, together with a mark
//!   of the request being applied. If the process stops between the two, the
//!   next start finds the mark and lists the request as "stopped before the
//!   result was stored" for the owner to check against the queue. A request is
//!   never applied twice and never dropped without a line in the list. If the
//!   owner's own store refuses the new queue, the position is put back and the
//!   request is tried again on the next call.
//! - **A refusal is listed, not retried.** A request that `queue_refund`
//!   refuses stays in the list with the refusal and its rule, until the app
//!   dismisses it. It is not tried again.
//! - **The sale's facts** come from the same copy: what was received (the
//!   last `Confirmed` or `Final` record, and for a sale that has neither, the
//!   request's own figure), whether the sale reached the refund depth (a
//!   `Final` record not undone by a later `Replaced` or `Reopened`). They are
//!   given to the owner before the request is queued and are refreshed for
//!   every sale that has a refund in the queue when the copy grows, so that
//!   "waiting for depth" ends by itself (AS-53). The owner's checks are not
//!   loosened.
//! - **A withdrawal** removes a refund that is still queued (or returned and
//!   not in a session) with the owner's `cancel`. A refund in an open
//!   session stays, and the list says so; so does one that is signed. A
//!   withdrawal is not tried again either.
//! - **A refund paid in cash** asks the owner for nothing. The owner has no
//!   cash-return bookkeeping of its own: the books read the record (BK-13).
//!   [`RefundIntake::cash_refunds`] lists them for a screen.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use openbmt_core::identity::DeviceId;
use openbmt_core::records::RecordKind;
use openbmt_core::wire::{DecodeError, Reader};
use openbmt_counter::{Ask, Event};
use openbmt_courier::Post;
use openbmt_owner::{Owner, OwnerError, SaleFacts, SaleKey, Stage};
use openbmt_session::ShopValue;

use crate::copy::CounterCopy;
use crate::link::OwnerLink;

/// The file of the intake state, in the owner node's directory.
pub const STATE_FILE: &str = "refund-intake";
/// The magic of the stored intake state.
pub const MAGIC_INTAKE: [u8; 4] = *b"OBM8";
/// Most lines the list keeps. Past it the oldest goes first, and the link's
/// notes say so.
pub const MAX_NOTES: usize = 256;
/// The longest text of a line, in bytes.
const MAX_TEXT: usize = 512;
/// Most counters whose position is kept.
const MAX_COUNTERS: usize = 4_096;
/// Most requests whose queued refund is remembered.
const MAX_QUEUED: usize = 4_096;

/// What a line of the list is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// The owner's queue refused the request (or the record did not read).
    Refused,
    /// A withdrawal that could not be applied: the refund is in a session,
    /// signed, or no longer there.
    WithdrawalNotApplied,
    /// The process stopped while the request was being applied; the owner
    /// checks the queue.
    Interrupted,
}

impl NoteKind {
    fn byte(self) -> u8 {
        match self {
            Self::Refused => 1,
            Self::WithdrawalNotApplied => 2,
            Self::Interrupted => 3,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Refused),
            2 => Some(Self::WithdrawalNotApplied),
            3 => Some(Self::Interrupted),
            _ => None,
        }
    }
}

/// One line of the list the owner app can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefundNote {
    /// The counter.
    pub counter: DeviceId,
    /// The number of the record in the counter's chain.
    pub seq: u64,
    /// The request number, or 0 when the record did not read.
    pub number: u64,
    /// The sale's receive index.
    pub sale: u32,
    /// What it is about.
    pub kind: NoteKind,
    /// The words: for a refusal, "refused: " and the owner's refusal with its
    /// rule.
    pub text: String,
    /// The request was withdrawn at the counter afterwards.
    pub withdrawn: bool,
}

/// A refund paid in cash at a counter, for a screen. Nothing is asked of the
/// owner for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CashRefundSeen {
    /// The sale's receive index.
    pub sale: u32,
    /// When.
    pub at: u64,
    /// Who gave the cash.
    pub name: String,
    /// The amount in the shop's currency, in minor units.
    pub minor: u64,
}

/// Why the intake state could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeError {
    /// The operating system refused an operation on the file.
    Io {
        /// What was being done.
        op: &'static str,
        /// What the system said.
        kind: io::ErrorKind,
    },
    /// The file does not read back as what was written.
    Corrupt(DecodeError),
}

impl fmt::Display for IntakeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { op, kind } => write!(f, "CR-60: could not {op} {STATE_FILE}: {kind}"),
            Self::Corrupt(e) => write!(f, "CR-60: {STATE_FILE} cannot be read back: {e}"),
        }
    }
}

impl std::error::Error for IntakeError {}

/// Why a call stopped before the end of the copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// The intake state could not be written.
    State(IntakeError),
    /// The owner's own store refused the new queue; the request is tried
    /// again on the next call.
    Owner(String),
}

impl fmt::Display for Stopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(e) => write!(f, "{e}"),
            Self::Owner(e) => write!(f, "{e}"),
        }
    }
}

/// What one call did for one counter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Requests that became queued refunds.
    pub queued: u64,
    /// Requests the owner's queue refused, now in the list.
    pub refused: u64,
    /// Queued refunds removed because the request was withdrawn.
    pub withdrawn: u64,
    /// Withdrawals that could not be applied, now in the list.
    pub held: u64,
    /// Why the call stopped early, if it did.
    pub stopped: Option<Stopped>,
}

/// A request being applied when the state was last written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    counter: DeviceId,
    seq: u64,
    number: u64,
    sale: u32,
}

/// What the records of a counter say about one sale.
#[derive(Debug, Clone, Default)]
struct SaleFold {
    received: u64,
    reached: bool,
    confirmed_seen: bool,
    currency: Option<[u8; 3]>,
}

/// The facts of the sales of one counter, folded from its copy as far as
/// `upto` records.
#[derive(Debug, Default)]
struct Fold {
    upto: u64,
    sales: BTreeMap<u32, SaleFold>,
}

impl Fold {
    fn add(&mut self, event: &Event) {
        match event {
            Event::Opened {
                index, currency, ..
            } => {
                self.sales.entry(*index).or_default().currency = Some(currency.code());
            }
            Event::Confirmed {
                index,
                received_sats,
                ..
            } => {
                let s = self.sales.entry(*index).or_default();
                s.received = *received_sats;
                s.confirmed_seen = true;
            }
            Event::Final {
                index,
                received_sats,
                ..
            } => {
                let s = self.sales.entry(*index).or_default();
                s.received = *received_sats;
                s.reached = true;
                s.confirmed_seen = true;
            }
            Event::Replaced { index, .. } => {
                let s = self.sales.entry(*index).or_default();
                s.received = 0;
                s.reached = false;
                s.confirmed_seen = false;
            }
            Event::Reopened { index, .. } => {
                self.sales.entry(*index).or_default().reached = false;
            }
            Event::RefundRequested {
                index,
                received_sats,
                ..
            } => {
                let s = self.sales.entry(*index).or_default();
                if !s.confirmed_seen {
                    s.received = *received_sats;
                }
            }
            _ => {}
        }
    }

    fn facts(&self, index: u32) -> Option<SaleFacts> {
        self.sales.get(&index).map(|s| SaleFacts {
            received_sats: s.received,
            reached_refund_depth: s.reached,
        })
    }
}

/// The intake of refund requests. See the module docs.
#[derive(Debug)]
pub struct RefundIntake {
    dir: PathBuf,
    applied: BTreeMap<DeviceId, u64>,
    pending: Option<Pending>,
    queued: BTreeMap<(DeviceId, u64), u32>,
    notes: Vec<RefundNote>,
    folds: BTreeMap<DeviceId, Fold>,
    /// Lines the app has not been told were dropped from the list.
    dropped: u64,
}

fn io_err(op: &'static str) -> impl FnOnce(io::Error) -> IntakeError {
    move |e| IntakeError::Io { op, kind: e.kind() }
}

impl RefundIntake {
    /// The intake state under `dir` (the owner node's directory), empty if
    /// there is none. A request that was being applied when the state was
    /// last written is turned into a line of the list (`Interrupted`) and the
    /// mark is cleared.
    pub fn open(dir: &Path) -> Result<Self, IntakeError> {
        let mut intake = Self {
            dir: dir.to_path_buf(),
            applied: BTreeMap::new(),
            pending: None,
            queued: BTreeMap::new(),
            notes: Vec::new(),
            folds: BTreeMap::new(),
            dropped: 0,
        };
        match fs::read(dir.join(STATE_FILE)) {
            Ok(bytes) => intake.read_back(&bytes).map_err(IntakeError::Corrupt)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_err("read")(e)),
        }
        if let Some(p) = intake.pending.take() {
            intake.push_note(RefundNote {
                counter: p.counter,
                seq: p.seq,
                number: p.number,
                sale: p.sale,
                kind: NoteKind::Interrupted,
                text: "stopped before the result was stored: check the queue for this \
                       request"
                    .to_string(),
                withdrawn: false,
            });
            intake.write()?;
        }
        Ok(intake)
    }

    /// How far into a counter's chain requests were applied: the number of
    /// records looked at.
    pub fn applied_up_to(&self, counter: &DeviceId) -> u64 {
        self.applied.get(counter).copied().unwrap_or(0)
    }

    /// The queued refund a request became, if it is still in the queue.
    pub fn queued_id(&self, counter: &DeviceId, number: u64) -> Option<u32> {
        self.queued.get(&(*counter, number)).copied()
    }

    /// The list of what could not be applied, oldest first.
    pub fn notes(&self) -> &[RefundNote] {
        &self.notes
    }

    /// How many lines went off the list because it was full.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// The owner's word that a line is dealt with.
    pub fn dismiss(&mut self, at: usize) -> Result<(), IntakeError> {
        if at < self.notes.len() {
            let before = self.notes.clone();
            self.notes.remove(at);
            if let Err(e) = self.write() {
                self.notes = before;
                return Err(e);
            }
        }
        Ok(())
    }

    /// The refunds paid in cash that a counter's copy holds, oldest first.
    /// They ask the owner for nothing.
    pub fn cash_refunds(copy: &CounterCopy) -> Vec<CashRefundSeen> {
        copy.records()
            .iter()
            .filter(|r| r.kind == RecordKind::Refund && r.author == copy.counter())
            .filter_map(|r| match Event::decode(&r.body) {
                Ok(Event::RefundPaidInCash {
                    index,
                    at,
                    name,
                    minor,
                    ..
                }) => Some(CashRefundSeen {
                    sale: index,
                    at,
                    name,
                    minor,
                }),
                _ => None,
            })
            .collect()
    }

    /// Apply the new records of every counter on the link. Returns what was
    /// done for each counter that had something new.
    pub fn run<R: Post, E: fmt::Display>(
        &mut self,
        owner: &mut Owner,
        link: &OwnerLink<R>,
        persist: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Vec<(DeviceId, Applied)> {
        let mut done = Vec::new();
        for counter in link.counters() {
            let Some(copy) = link.copy(counter) else {
                continue;
            };
            let applied = self.apply(owner, *counter, copy, persist);
            if applied != Applied::default() {
                done.push((*counter, applied));
            }
        }
        done
    }

    /// Apply the records of one counter's copy that were not looked at yet,
    /// in order (see the module docs). Records the counter did not write and
    /// records of kinds other than refund are passed over.
    pub fn apply<E: fmt::Display>(
        &mut self,
        owner: &mut Owner,
        counter: DeviceId,
        copy: &CounterCopy,
        persist: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Applied {
        let mut out = Applied::default();
        let len = copy.len();
        let start = self.applied_up_to(&counter);
        if start >= len && self.queued.keys().all(|(c, _)| *c != counter) {
            return out;
        }
        self.catch_up(counter, copy);
        let before = (start, self.queued.clone(), self.notes.len());
        let records = copy.records();
        let from = usize::try_from(start).unwrap_or(usize::MAX);
        for record in records.iter().skip(from) {
            if record.kind != RecordKind::Refund || record.author != counter {
                if self.advance(counter, record.seq.saturating_add(1)).is_err() {
                    // Not stored: the same records are passed over again.
                    break;
                }
                continue;
            }
            match Event::decode(&record.body) {
                Ok(Event::RefundRequested {
                    index,
                    number,
                    name,
                    destination,
                    sats,
                    ask,
                    ..
                }) => {
                    let sale = SaleKey { counter, index };
                    let currency = self
                        .folds
                        .get(&counter)
                        .and_then(|f| f.sales.get(&index))
                        .and_then(|s| s.currency);
                    let shop_value = match (&ask, currency) {
                        (Ask::Value { minor, .. }, Some(currency)) => Some(ShopValue {
                            minor: *minor,
                            currency,
                        }),
                        _ => None,
                    };
                    let pending = Pending {
                        counter,
                        seq: record.seq,
                        number,
                        sale: index,
                    };
                    if let Err(stopped) = self.begin(&pending) {
                        out.stopped = Some(stopped);
                        break;
                    }
                    let facts = self
                        .folds
                        .get(&counter)
                        .and_then(|f| f.facts(index))
                        .unwrap_or(SaleFacts {
                            received_sats: 0,
                            reached_refund_depth: false,
                        });
                    let effect = Self::queue(
                        owner,
                        sale,
                        facts,
                        (&name, destination, sats, shop_value),
                        persist,
                    );
                    match effect {
                        Ok(id) => {
                            self.queued.insert((counter, number), id);
                            out.queued = out.queued.saturating_add(1);
                            self.finish(None);
                        }
                        Err(OwnerError::Refused(r)) => {
                            self.push_note(RefundNote {
                                counter,
                                seq: record.seq,
                                number,
                                sale: index,
                                kind: NoteKind::Refused,
                                text: format!("refused: {r}"),
                                withdrawn: false,
                            });
                            out.refused = out.refused.saturating_add(1);
                            self.finish(None);
                        }
                        Err(OwnerError::Store(e)) => {
                            // Nothing was queued: put the position back.
                            self.finish(Some(record.seq));
                            out.stopped = Some(Stopped::Owner(e.to_string()));
                            break;
                        }
                    }
                }
                Ok(Event::RefundWithdrawn { index, number, .. }) => {
                    let pending = Pending {
                        counter,
                        seq: record.seq,
                        number,
                        sale: index,
                    };
                    if let Err(stopped) = self.begin(&pending) {
                        out.stopped = Some(stopped);
                        break;
                    }
                    match self.withdraw(owner, counter, record.seq, index, number, persist) {
                        Withdrawal::Removed => {
                            out.withdrawn = out.withdrawn.saturating_add(1);
                            self.finish(None);
                        }
                        Withdrawal::Held => {
                            out.held = out.held.saturating_add(1);
                            self.finish(None);
                        }
                        Withdrawal::Nothing => self.finish(None),
                        Withdrawal::StoreFailed(why) => {
                            self.finish(Some(record.seq));
                            out.stopped = Some(Stopped::Owner(why));
                            break;
                        }
                    }
                }
                Ok(_) => {
                    // A cash refund asks the owner for nothing.
                    if self.advance(counter, record.seq.saturating_add(1)).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    self.push_note(RefundNote {
                        counter,
                        seq: record.seq,
                        number: 0,
                        sale: 0,
                        kind: NoteKind::Refused,
                        text: format!("refused: a record of the refund kind does not read: {e}"),
                        withdrawn: false,
                    });
                    out.refused = out.refused.saturating_add(1);
                    if self.advance(counter, record.seq.saturating_add(1)).is_err() {
                        break;
                    }
                }
            }
        }
        if out.stopped.is_none() {
            self.refresh_facts(owner, counter, persist, &mut out);
            // Forget what became of requests whose refund has left the queue.
            let live: Vec<u32> = owner.refunds().iter().map(|r| r.id).collect();
            self.queued.retain(|_, id| live.contains(id));
            let after = (
                self.applied_up_to(&counter),
                self.queued.clone(),
                self.notes.len(),
            );
            if (after != before || out != Applied::default()) && self.pending.is_none() {
                if let Err(e) = self.write() {
                    out.stopped = Some(Stopped::State(e));
                }
            }
        }
        out
    }

    /// Fold the records of the copy that the fold has not seen.
    fn catch_up(&mut self, counter: DeviceId, copy: &CounterCopy) {
        let fold = self.folds.entry(counter).or_default();
        let from = usize::try_from(fold.upto).unwrap_or(usize::MAX);
        for record in copy.records().iter().skip(from) {
            if record.author == counter {
                if let Ok(event) = Event::decode(&record.body) {
                    // Only the kinds the counter's events are written as.
                    if event.kind() == record.kind {
                        fold.add(&event);
                    }
                }
            }
        }
        fold.upto = copy.len();
    }

    /// Give the owner the sale's facts, then queue the refund.
    fn queue<E: fmt::Display>(
        owner: &mut Owner,
        sale: SaleKey,
        facts: SaleFacts,
        refund: (&str, bitcoin::ScriptBuf, u64, Option<ShopValue>),
        persist: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<u32, OwnerError<E>> {
        if owner.sale_facts(&sale) != Some(facts) {
            owner.set_sale_facts(sale, facts, &mut *persist)?;
        }
        let (name, script, sats, shop_value) = refund;
        owner.queue_refund(sale, name, script, sats, shop_value, persist)
    }

    /// A withdrawal: the owner's `cancel` for a refund that is queued.
    fn withdraw<E: fmt::Display>(
        &mut self,
        owner: &mut Owner,
        counter: DeviceId,
        seq: u64,
        sale: u32,
        number: u64,
        persist: &mut impl FnMut(&[u8]) -> Result<(), E>,
    ) -> Withdrawal {
        let held = |this: &mut Self, text: String| {
            this.push_note(RefundNote {
                counter,
                seq,
                number,
                sale,
                kind: NoteKind::WithdrawalNotApplied,
                text,
                withdrawn: true,
            });
            Withdrawal::Held
        };
        let Some(id) = self.queued.get(&(counter, number)).copied() else {
            // Never queued: refused, or lost. A line in the list says it was
            // withdrawn.
            for n in &mut self.notes {
                if n.counter == counter && n.number == number && n.kind == NoteKind::Refused {
                    n.withdrawn = true;
                }
            }
            return Withdrawal::Nothing;
        };
        let stage = owner
            .refunds()
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.stage.clone());
        match stage {
            None => {
                self.queued.remove(&(counter, number));
                held(
                    self,
                    format!("withdrawn at the counter: refund {id} is no longer in the queue"),
                )
            }
            Some(Stage::Queued | Stage::Returned { .. }) => match owner.cancel(id, persist) {
                Ok(()) => {
                    self.queued.remove(&(counter, number));
                    Withdrawal::Removed
                }
                Err(OwnerError::Refused(r)) => held(
                    self,
                    format!("withdrawn at the counter, and the refund stays: {r}"),
                ),
                Err(OwnerError::Store(e)) => Withdrawal::StoreFailed(e.to_string()),
            },
            Some(Stage::InSession { .. }) => held(
                self,
                format!(
                    "withdrawn at the counter, and refund {id} stays: it is in an open session"
                ),
            ),
            Some(_) => held(
                self,
                format!("withdrawn at the counter, and refund {id} stays: it was already signed"),
            ),
        }
    }

    /// Refresh the facts of every sale of this counter that has a refund in
    /// the queue, so that "waiting for depth" ends when the sale is final
    /// (AS-53).
    fn refresh_facts<E: fmt::Display>(
        &mut self,
        owner: &mut Owner,
        counter: DeviceId,
        persist: &mut impl FnMut(&[u8]) -> Result<(), E>,
        out: &mut Applied,
    ) {
        let mut sales: Vec<SaleKey> = owner
            .refunds()
            .iter()
            .filter(|r| r.sale.counter == counter)
            .map(|r| r.sale)
            .collect();
        sales.sort();
        sales.dedup();
        for sale in sales {
            let Some(facts) = self.folds.get(&counter).and_then(|f| f.facts(sale.index)) else {
                continue;
            };
            if owner.sale_facts(&sale) == Some(facts) {
                continue;
            }
            if let Err(e) = owner.set_sale_facts(sale, facts, &mut *persist) {
                out.stopped = Some(Stopped::Owner(e.to_string()));
                return;
            }
        }
    }

    fn push_note(&mut self, note: RefundNote) {
        if self.notes.len() >= MAX_NOTES {
            self.notes.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
        self.notes.push(note);
    }

    /// Store the position and the mark of the request being applied, before
    /// the effect.
    fn begin(&mut self, p: &Pending) -> Result<(), Stopped> {
        let before = (self.applied.clone(), self.pending);
        self.applied.insert(p.counter, p.seq.saturating_add(1));
        self.pending = Some(*p);
        if let Err(e) = self.write() {
            (self.applied, self.pending) = before;
            return Err(Stopped::State(e));
        }
        Ok(())
    }

    /// The effect is known: clear the mark, or put the position back to
    /// `retry_at` so that the request is tried again. A state that cannot be
    /// written now is written by the next call; the mark stays until then.
    fn finish(&mut self, retry_at: Option<u64>) {
        let Some(p) = self.pending else { return };
        if let Some(seq) = retry_at {
            self.applied.insert(p.counter, seq);
        }
        let mark = self.pending.take();
        if self.write().is_err() {
            self.pending = mark;
        }
    }

    /// Move the position past a record that holds nothing to apply.
    fn advance(&mut self, counter: DeviceId, to: u64) -> Result<(), IntakeError> {
        let before = self.applied.get(&counter).copied();
        if before.is_some_and(|b| b >= to) {
            return Ok(());
        }
        self.applied.insert(counter, to);
        // Written with the next effect or at the end of the call; a position
        // that is not written only means those records are passed over again.
        Ok(())
    }

    fn write(&self) -> Result<(), IntakeError> {
        let bytes = self.to_bytes();
        let temp = self.dir.join(format!("{STATE_FILE}.tmp"));
        File::create(&temp)
            .and_then(|mut f| {
                f.write_all(&bytes)?;
                f.sync_all()
            })
            .map_err(io_err("write"))?;
        fs::rename(&temp, self.dir.join(STATE_FILE)).map_err(io_err("replace"))?;
        File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(io_err("flush"))
    }

    /// The stored form.
    ///
    /// ```text
    /// OBM8 | 1 | n_counters u32 | n * (counter (32) | applied u64)
    ///   | pending (flag; counter (32) | seq u64 | number u64 | sale u32)
    ///   | n_queued u32 | n * (counter (32) | number u64 | refund id u32)
    ///   | n_notes u32 | n * (counter (32) | seq u64 | number u64 | sale u32
    ///       | kind u8 | withdrawn flag | text_len u16 | text)
    /// ```
    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + self.notes.len() * 96);
        out.extend_from_slice(&MAGIC_INTAKE);
        out.push(1);
        out.extend_from_slice(&len32(self.applied.len()).to_le_bytes());
        for (c, n) in &self.applied {
            out.extend_from_slice(&c.to_bytes());
            out.extend_from_slice(&n.to_le_bytes());
        }
        match &self.pending {
            Some(p) => {
                out.push(1);
                out.extend_from_slice(&p.counter.to_bytes());
                out.extend_from_slice(&p.seq.to_le_bytes());
                out.extend_from_slice(&p.number.to_le_bytes());
                out.extend_from_slice(&p.sale.to_le_bytes());
            }
            None => out.push(0),
        }
        out.extend_from_slice(&len32(self.queued.len()).to_le_bytes());
        for ((c, number), id) in &self.queued {
            out.extend_from_slice(&c.to_bytes());
            out.extend_from_slice(&number.to_le_bytes());
            out.extend_from_slice(&id.to_le_bytes());
        }
        out.extend_from_slice(&len32(self.notes.len()).to_le_bytes());
        for n in &self.notes {
            out.extend_from_slice(&n.counter.to_bytes());
            out.extend_from_slice(&n.seq.to_le_bytes());
            out.extend_from_slice(&n.number.to_le_bytes());
            out.extend_from_slice(&n.sale.to_le_bytes());
            out.push(n.kind.byte());
            out.push(u8::from(n.withdrawn));
            let text = truncated(&n.text);
            out.extend_from_slice(&u16::try_from(text.len()).unwrap_or(u16::MAX).to_le_bytes());
            out.extend_from_slice(text.as_bytes());
        }
        out
    }

    fn read_back(&mut self, bytes: &[u8]) -> Result<(), DecodeError> {
        let bad = DecodeError::NonCanonical;
        let mut r = Reader::framed(bytes, &MAGIC_INTAKE)?;
        let n = r.count(MAX_COUNTERS, 40)?;
        for _ in 0..n {
            let counter = r.device_id()?;
            if self.applied.insert(counter, r.u64_le()?).is_some() {
                return Err(bad("a counter is listed twice"));
            }
        }
        if r.flag()? {
            self.pending = Some(Pending {
                counter: r.device_id()?,
                seq: r.u64_le()?,
                number: r.u64_le()?,
                sale: r.u32_le()?,
            });
        }
        let n = r.count(MAX_QUEUED, 44)?;
        for _ in 0..n {
            let key = (r.device_id()?, r.u64_le()?);
            if self.queued.insert(key, r.u32_le()?).is_some() {
                return Err(bad("a request is listed twice"));
            }
        }
        let n = r.count(MAX_NOTES, 50)?;
        for _ in 0..n {
            let counter = r.device_id()?;
            let seq = r.u64_le()?;
            let number = r.u64_le()?;
            let sale = r.u32_le()?;
            let kind = NoteKind::from_byte(r.u8()?).ok_or(bad("a line kind is not known"))?;
            let withdrawn = r.flag()?;
            let len = usize::from(r.u16_le()?);
            if len > MAX_TEXT {
                return Err(DecodeError::TooLarge);
            }
            let text = std::str::from_utf8(r.take(len)?)
                .map_err(|_| bad("a line is not text"))?
                .to_string();
            self.notes.push(RefundNote {
                counter,
                seq,
                number,
                sale,
                kind,
                text,
                withdrawn,
            });
        }
        r.finish()
    }
}

/// What a withdrawal did.
enum Withdrawal {
    /// The queued refund was removed.
    Removed,
    /// The refund stays, and a line says why.
    Held,
    /// There was nothing queued to remove.
    Nothing,
    /// The owner's store refused the new queue.
    StoreFailed(String),
}

fn len32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// A text cut to [`MAX_TEXT`] bytes at a character boundary.
fn truncated(text: &str) -> &str {
    if text.len() <= MAX_TEXT {
        return text;
    }
    let mut end = MAX_TEXT;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.get(..end).unwrap_or("")
}
