//! The owner phone's state and the calls that keep it (AS-50 to AS-66).

use std::collections::BTreeMap;

use bitcoin::{OutPoint, ScriptBuf, Txid};
use openbmt_core::batch::{AddressBatch, BatchBuilder, BatchPolicy, PinnedRoots};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::pairing::{check_request, PairOffer, PairRequest};
use openbmt_core::settings::{classify_change, Change, Settings, SignedSettings};
use openbmt_core::wallet::WalletDescription;
use openbmt_session::{label_text_ok, ShopValue};
use openbmt_spend::dust_limit;

use crate::codec;
use crate::error::{OwnerError, Refusal, RestoreError, SetupError};
use crate::types::{
    CashAmount, CashOut, Deliverable, Delivery, Options, QueuedBatch, QueuedPairing,
    QueuedSettings, Refund, SaleFacts, SaleKey, SettingsQueued, Stage,
};
use crate::{
    MAX_BATCHES, MAX_CASH_OUTS, MAX_PAIRINGS, MAX_REFUNDS, MAX_SALES, MAX_SETTINGS,
    MAX_SNAPSHOT_BYTES,
};

/// 21,000,000 BTC in satoshis.
const MAX_MONEY: u64 = 2_100_000_000_000_000;

/// A sale's facts and what was archived as paid against it (AS-52).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SaleEntry {
    pub facts: SaleFacts,
    pub archived_paid: u64,
}

/// What was frozen for one item of an open session (AS-61, AS-67).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Frozen {
    Spend {
        txid: Txid,
        refunds: Vec<u32>,
        cash_out: Option<u32>,
        payments: Vec<(ScriptBuf, u64)>,
        change_index: u32,
        /// The change output's position among the outputs, if the plan has
        /// change (AS-61, AS-67).
        change_position: Option<u32>,
    },
    Batch(u32),
    Settings(u32),
    Pairing(u32),
    Health,
}

/// AS-67: whether `outputs` are the frozen payments, in order, with one
/// more output, the change, at `change_position` when there is one. The
/// change's script is checked by the caller, who has the wallet.
pub(crate) fn outputs_fit(
    outputs: &[bitcoin::TxOut],
    payments: &[(ScriptBuf, u64)],
    change_position: Option<u32>,
) -> bool {
    let change = change_position.and_then(|p| usize::try_from(p).ok());
    if outputs.len() != payments.len() + usize::from(change_position.is_some()) {
        return false;
    }
    if change.is_some_and(|c| c >= outputs.len()) {
        return false;
    }
    let mut paid = payments.iter();
    for (at, out) in outputs.iter().enumerate() {
        if change == Some(at) {
            continue;
        }
        match paid.next() {
            Some((script, sats)) if out.script_pubkey == *script && out.value.to_sat() == *sats => {
            }
            _ => return false,
        }
    }
    true
}

/// A session that was built and has not been answered (AS-61, AS-65).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenSession {
    pub id: [u8; 32],
    pub created_at: u64,
    pub request: Vec<u8>,
    /// One entry for each item of the request, with the item's number.
    pub frozen: Vec<(u32, Frozen)>,
}

/// Everything the owner phone keeps. Every call works on a copy and stores
/// it before it takes effect (AS-50).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct State {
    pub next_id: u32,
    pub options: Options,
    pub sales: BTreeMap<SaleKey, SaleEntry>,
    pub refunds: Vec<Refund>,
    pub cash_outs: Vec<CashOut>,
    pub batches: Vec<QueuedBatch>,
    pub settings: Vec<QueuedSettings>,
    pub pairings: Vec<QueuedPairing>,
    pub known: Option<SignedSettings>,
    pub sessions: Vec<OpenSession>,
    pub outbox: Vec<Deliverable>,
}

impl State {
    pub fn new(options: Options) -> Self {
        Self {
            next_id: 1,
            options,
            sales: BTreeMap::new(),
            refunds: Vec::new(),
            cash_outs: Vec::new(),
            batches: Vec::new(),
            settings: Vec::new(),
            pairings: Vec::new(),
            known: None,
            sessions: Vec::new(),
            outbox: Vec::new(),
        }
    }

    /// A new number for a queue entry or a finished object.
    pub fn take_id(&mut self) -> Result<u32, Refusal> {
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or(Refusal::Full("the numbering"))?;
        Ok(id)
    }

    /// AS-52: what the refunds of a sale add up to: every refund of the
    /// sale that is still in the queue, and what was archived as paid.
    pub fn counted(&self, sale: &SaleKey) -> u64 {
        let archived = self.sales.get(sale).map_or(0, |s| s.archived_paid);
        self.refunds
            .iter()
            .filter(|r| r.sale == *sale)
            .fold(archived, |sum, r| sum.saturating_add(r.sats))
    }

    /// AS-59: the highest settings version known, counting the queued ones.
    pub fn highest_version(&self) -> Option<u32> {
        let known = self.known.as_ref().map(|s| s.settings.version);
        let queued = self.settings.iter().map(|q| q.signed.settings.version);
        known.into_iter().chain(queued).max()
    }

    /// AS-62: the highest settings version this owner phone has signed: the
    /// one in force, the queued ones and the ones waiting to be delivered.
    pub fn highest_signed_version(&self) -> u32 {
        let waiting = self.outbox.iter().filter_map(|d| match &d.what {
            Delivery::Settings(bytes) => SignedSettings::from_bytes(bytes)
                .ok()
                .map(|s| s.settings.version),
            _ => None,
        });
        self.highest_version()
            .into_iter()
            .chain(waiting)
            .max()
            .unwrap_or(0)
    }

    /// The stage of any queue entry by number.
    pub fn stage_mut(&mut self, id: u32) -> Option<&mut Stage> {
        if let Some(r) = self.refunds.iter_mut().find(|r| r.id == id) {
            return Some(&mut r.stage);
        }
        if let Some(c) = self.cash_outs.iter_mut().find(|c| c.id == id) {
            return Some(&mut c.stage);
        }
        if let Some(b) = self.batches.iter_mut().find(|b| b.id == id) {
            return Some(&mut b.stage);
        }
        if let Some(s) = self.settings.iter_mut().find(|s| s.id == id) {
            return Some(&mut s.stage);
        }
        if let Some(p) = self.pairings.iter_mut().find(|p| p.id == id) {
            return Some(&mut p.stage);
        }
        None
    }

    /// AS-68: the outpoints kept with a refund or a cash-out.
    pub fn conflict_mut(&mut self, id: u32) -> Option<&mut Vec<OutPoint>> {
        if let Some(r) = self.refunds.iter_mut().find(|r| r.id == id) {
            return Some(&mut r.conflict);
        }
        self.cash_outs
            .iter_mut()
            .find(|c| c.id == id)
            .map(|c| &mut c.conflict)
    }

    /// Remove a queue entry by number.
    pub fn remove_entry(&mut self, id: u32) {
        self.refunds.retain(|r| r.id != id);
        self.cash_outs.retain(|c| c.id != id);
        self.batches.retain(|b| b.id != id);
        self.settings.retain(|s| s.id != id);
        self.pairings.retain(|p| p.id != id);
    }
}

/// The owner phone: its identity, the wallet, the pinned Key app root and
/// the queue.
pub struct Owner {
    pub(crate) identity: Identity,
    pub(crate) wallet: WalletDescription,
    pub(crate) wallet_id: [u8; 32],
    pub(crate) roots: PinnedRoots,
    pub(crate) policy: BatchPolicy,
    pub(crate) state: State,
}

impl std::fmt::Debug for Owner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Owner")
            .field("owner", &self.identity.device_id())
            .field("approver", &self.roots.approver)
            .finish_non_exhaustive()
    }
}

/// AS-07: a text as the labels hold it.
fn label_ok(s: &str, what: &'static str) -> Result<(), Refusal> {
    if label_text_ok(s) {
        Ok(())
    } else {
        Err(Refusal::BadText(what))
    }
}

/// AS-51: the destination is of an allowed kind and the amount is at least
/// its dust limit.
fn destination_ok(script: &ScriptBuf, sats: u64) -> Result<(), Refusal> {
    let dust = dust_limit(script).ok_or(Refusal::DestinationKind)?;
    if sats > MAX_MONEY {
        return Err(Refusal::AmountTooLarge);
    }
    if sats < dust {
        return Err(Refusal::BelowDust { sats, dust });
    }
    Ok(())
}

impl Owner {
    /// Set up the owner phone with an empty queue.
    pub fn new(
        owner_secret: &[u8; 32],
        wallet: WalletDescription,
        approver: DeviceId,
        policy: BatchPolicy,
        options: Options,
    ) -> Result<Self, SetupError> {
        let identity = Identity::from_secret_bytes(owner_secret).map_err(SetupError::Identity)?;
        let roots =
            PinnedRoots::new(identity.device_id(), approver).map_err(|_| SetupError::SameRoot)?;
        let wallet_id = wallet.checksum32();
        Ok(Self {
            identity,
            wallet,
            wallet_id,
            roots,
            policy,
            state: State::new(options),
        })
    }

    /// AS-65: the owner phone from its stored snapshot.
    pub fn restore(
        owner_secret: &[u8; 32],
        wallet: WalletDescription,
        approver: DeviceId,
        policy: BatchPolicy,
        bytes: &[u8],
    ) -> Result<Self, RestoreError> {
        let mut owner = Self::new(owner_secret, wallet, approver, policy, Options::default())
            .map_err(RestoreError::Setup)?;
        owner.state = codec::decode(&owner, bytes)?;
        Ok(owner)
    }

    /// AS-65: the queue's snapshot, `OBM4`.
    pub fn snapshot(&self) -> Result<Vec<u8>, Refusal> {
        codec::encode(self, &self.state)
    }

    /// Store `next`, and take it into memory only if the store worked
    /// (AS-50).
    pub(crate) fn commit<E>(
        &mut self,
        next: State,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let bytes = codec::encode(self, &next)?;
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(Refusal::SnapshotTooLarge.into());
        }
        persist(&bytes).map_err(OwnerError::Store)?;
        self.state = next;
        Ok(())
    }

    // ---- reading ------------------------------------------------------------

    /// The owner's identity.
    pub fn owner_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// The pinned Key app root.
    pub fn approver_id(&self) -> DeviceId {
        self.roots.approver
    }

    /// The wallet.
    pub fn wallet(&self) -> &WalletDescription {
        &self.wallet
    }

    /// The owner's choices.
    pub fn options(&self) -> Options {
        self.state.options
    }

    /// The refunds in the queue, in the order they were queued.
    pub fn refunds(&self) -> &[Refund] {
        &self.state.refunds
    }

    /// The cash-outs in the queue.
    pub fn cash_outs(&self) -> &[CashOut] {
        &self.state.cash_outs
    }

    /// The batches in the queue.
    pub fn batches(&self) -> &[QueuedBatch] {
        &self.state.batches
    }

    /// The settings in the queue.
    pub fn queued_settings(&self) -> &[QueuedSettings] {
        &self.state.settings
    }

    /// The pairings in the queue.
    pub fn pairings(&self) -> &[QueuedPairing] {
        &self.state.pairings
    }

    /// The finished objects waiting to be delivered.
    pub fn outbox(&self) -> &[Deliverable] {
        &self.state.outbox
    }

    /// The settings the owner phone knows are in force (AS-59).
    pub fn known_settings(&self) -> Option<&Settings> {
        self.state.known.as_ref().map(|s| &s.settings)
    }

    /// The IDs of the sessions that wait for an answer.
    pub fn open_sessions(&self) -> Vec<[u8; 32]> {
        self.state.sessions.iter().map(|s| s.id).collect()
    }

    /// The request of an open session, as it was handed out.
    pub fn session_request(&self, id: &[u8; 32]) -> Option<&[u8]> {
        self.state
            .sessions
            .iter()
            .find(|s| s.id == *id)
            .map(|s| s.request.as_slice())
    }

    /// The first receive index that no batch the owner phone holds has
    /// handed out: the highest `start + count` over the batches in the queue
    /// and over the signed batches waiting to be delivered. A batch that was
    /// delivered leaves both, so a caller that must not forget it keeps the
    /// highest value it has seen.
    pub fn receive_mark(&self) -> u32 {
        let queued = self
            .state
            .batches
            .iter()
            .map(|q| q.batch.start.saturating_add(q.batch.count));
        let waiting = self.state.outbox.iter().filter_map(|d| match &d.what {
            Delivery::Batch(bytes) => openbmt_core::batch::SignedBatch::from_bytes(bytes)
                .ok()
                .map(|b| b.batch.start.saturating_add(b.batch.count)),
            _ => None,
        });
        queued.chain(waiting).max().unwrap_or(0)
    }

    /// The first change index that no open session has frozen: one above the
    /// highest change index of a transaction in an open session (0 if none).
    pub fn change_mark(&self) -> u32 {
        self.state
            .sessions
            .iter()
            .flat_map(|s| s.frozen.iter())
            .filter_map(|(_, f)| match f {
                Frozen::Spend {
                    change_index,
                    change_position: Some(_),
                    ..
                } => Some(change_index.saturating_add(1)),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// A sale's facts, if any are kept.
    pub fn sale_facts(&self, sale: &SaleKey) -> Option<SaleFacts> {
        self.state.sales.get(sale).map(|s| s.facts)
    }

    /// AS-52: what the refunds of a sale add up to, paid ones archived
    /// included.
    pub fn refunded_sats(&self, sale: &SaleKey) -> u64 {
        self.state.counted(sale)
    }

    /// AS-53: whether a queued refund is still waiting for the sale to reach
    /// the refund depth, or for facts that cover the refunds counted. `None`
    /// if there is no such refund.
    pub fn waiting_for_depth(&self, id: u32) -> Option<bool> {
        let r = self.state.refunds.iter().find(|r| r.id == id)?;
        Some(!crate::build::is_ready(&self.state, r))
    }

    // ---- facts and options ----------------------------------------------------

    /// AS-52, AS-53: set what the counter's records say about a sale.
    pub fn set_sale_facts<E>(
        &mut self,
        sale: SaleKey,
        facts: SaleFacts,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        match next.sales.get_mut(&sale) {
            Some(entry) => entry.facts = facts,
            None => {
                if next.sales.len() >= MAX_SALES {
                    return Err(Refusal::Full("the list of sales' facts").into());
                }
                next.sales.insert(
                    sale,
                    SaleEntry {
                        facts,
                        archived_paid: 0,
                    },
                );
            }
        }
        self.commit(next, persist)
    }

    /// AS-50: forget a sale's facts, only when nothing is counted against it.
    pub fn forget_sale<E>(
        &mut self,
        sale: &SaleKey,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        if self.state.counted(sale) > 0 {
            return Err(Refusal::SaleInUse.into());
        }
        let mut next = self.state.clone();
        next.sales.remove(sale);
        self.commit(next, persist)
    }

    /// AS-55, AS-56: change the owner's choices. They apply to sessions
    /// built afterwards.
    pub fn set_options<E>(
        &mut self,
        options: Options,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        next.options = options;
        self.commit(next, persist)
    }

    // ---- queueing -------------------------------------------------------------

    /// AS-51, AS-52: queue a refund. The destination and the amount come
    /// from the person at the counter, never from the refund QR's own
    /// fields.
    pub fn queue_refund<E>(
        &mut self,
        sale: SaleKey,
        cashier: &str,
        script: ScriptBuf,
        sats: u64,
        shop_value: Option<ShopValue>,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<u32, OwnerError<E>> {
        label_ok(cashier, "cashier")?;
        if let Some(v) = &shop_value {
            if !v.currency.iter().all(u8::is_ascii_uppercase) {
                return Err(Refusal::BadText("currency code").into());
            }
        }
        destination_ok(&script, sats)?;
        let mut next = self.state.clone();
        if next.refunds.len() >= MAX_REFUNDS {
            return Err(Refusal::Full("the refund queue").into());
        }
        let received = next
            .sales
            .get(&sale)
            .map(|s| s.facts.received_sats)
            .ok_or(Refusal::UnknownSale)?;
        let wanted = next.counted(&sale).saturating_add(sats);
        if wanted > received {
            return Err(Refusal::MoreThanReceived { received, wanted }.into());
        }
        let id = next.take_id()?;
        next.refunds.push(Refund {
            id,
            sale,
            cashier: cashier.to_owned(),
            script,
            sats,
            shop_value,
            stage: Stage::Queued,
            conflict: Vec::new(),
        });
        self.commit(next, persist)?;
        Ok(id)
    }

    /// AS-57: queue a cash-out.
    pub fn queue_cash_out<E>(
        &mut self,
        script: ScriptBuf,
        amount: CashAmount,
        label: &str,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<u32, OwnerError<E>> {
        label_ok(label, "cash-out label")?;
        match amount {
            CashAmount::Sats(sats) => destination_ok(&script, sats)?,
            CashAmount::All => {
                dust_limit(&script).ok_or(Refusal::DestinationKind)?;
            }
        }
        let mut next = self.state.clone();
        if next.cash_outs.len() >= MAX_CASH_OUTS {
            return Err(Refusal::Full("the cash-out queue").into());
        }
        let id = next.take_id()?;
        next.cash_outs.push(CashOut {
            id,
            script,
            amount,
            label: label.to_owned(),
            conflict: Vec::new(),
            stage: Stage::Queued,
        });
        self.commit(next, persist)?;
        Ok(id)
    }

    /// AS-58: queue a batch. It is signed with the owner identity here and
    /// must be what the wallet derives for its counter and range.
    pub fn queue_batch<E>(
        &mut self,
        batch: AddressBatch,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<u32, OwnerError<E>> {
        let derived = BatchBuilder::new(self.policy)
            .derive(&self.wallet, batch.counter, batch.start, batch.count)
            .map_err(|_| Refusal::BatchNotDerived)?;
        if derived != batch {
            return Err(Refusal::BatchNotDerived.into());
        }
        let mut next = self.state.clone();
        if next.batches.len() >= MAX_BATCHES {
            return Err(Refusal::Full("the batch queue").into());
        }
        let id = next.take_id()?;
        let owner_sig = batch.sign(&self.identity);
        next.batches.push(QueuedBatch {
            id,
            batch,
            owner_sig,
            stage: Stage::Queued,
        });
        self.commit(next, persist)?;
        Ok(id)
    }

    /// AS-58: queue a counter's pairing request, decoded strictly and
    /// checked against the Key app's offer.
    pub fn queue_pairing<E>(
        &mut self,
        request_bytes: &[u8],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<u32, OwnerError<E>> {
        let request = PairRequest::from_bytes(request_bytes)
            .map_err(|e| Refusal::PairingRequest(e.to_string()))?;
        let offer = PairOffer::new(
            self.identity.device_id(),
            self.roots.approver,
            self.wallet_id,
        )
        .map_err(|e| Refusal::PairingRequest(e.to_string()))?;
        check_request(&offer, &request).map_err(|e| Refusal::PairingRequest(e.to_string()))?;
        let mut next = self.state.clone();
        if next.pairings.len() >= MAX_PAIRINGS {
            return Err(Refusal::Full("the pairing queue").into());
        }
        let id = next.take_id()?;
        next.pairings.push(QueuedPairing {
            id,
            request,
            stage: Stage::Queued,
        });
        self.commit(next, persist)?;
        Ok(id)
    }

    /// AS-59: sort a settings change. A loosening change is signed by the
    /// owner and queued. A tightening change is signed by the owner alone
    /// and handed back at once. Queued settings of a lower version are
    /// dropped as superseded.
    pub fn queue_settings<E>(
        &mut self,
        settings: Settings,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<SettingsQueued, OwnerError<E>> {
        settings
            .validate()
            .map_err(|e| Refusal::SettingsInvalid(e.to_string()))?;
        let mut next = self.state.clone();
        if let Some(known) = next.highest_version() {
            if settings.version <= known {
                return Err(Refusal::SettingsVersion {
                    known,
                    got: settings.version,
                }
                .into());
            }
        }
        let owner_sig = settings.sign(&self.identity);
        let signed = SignedSettings {
            settings,
            owner_sig,
            approver_sig: None,
        };
        let change = match &next.known {
            Some(k) => classify_change(&k.settings, &signed.settings),
            None => Change::Loosening,
        };
        // AS-59: what is queued and not in a session is superseded.
        next.settings
            .retain(|q| !matches!(q.stage, Stage::Queued | Stage::Returned { .. }));
        let outcome = match change {
            Change::Loosening => {
                if next.settings.len() >= MAX_SETTINGS {
                    return Err(Refusal::Full("the settings queue").into());
                }
                let id = next.take_id()?;
                next.settings.push(QueuedSettings {
                    id,
                    signed,
                    stage: Stage::Queued,
                });
                SettingsQueued::Queued(id)
            }
            Change::Tightening => {
                if next.outbox.len() >= crate::MAX_OUTBOX {
                    return Err(Refusal::OutboxFull.into());
                }
                let id = next.take_id()?;
                next.outbox.push(Deliverable {
                    id,
                    what: Delivery::Settings(signed.to_bytes()),
                });
                next.known = Some(signed.clone());
                SettingsQueued::Tightening {
                    id,
                    signed: Box::new(signed),
                }
            }
        };
        self.commit(next, persist)?;
        Ok(outcome)
    }

    // ---- changing a queue entry ---------------------------------------------------

    /// AS-54: cancel an entry that is queued or returned. A cancelled refund
    /// frees its amount.
    pub fn cancel<E>(
        &mut self,
        id: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        match next.stage_mut(id) {
            None => return Err(Refusal::UnknownEntry(id).into()),
            Some(Stage::Queued | Stage::Returned { .. }) => {}
            Some(Stage::InSession { .. }) => {
                return Err(
                    Refusal::WrongStage("it is in a session; abandon the session first").into(),
                )
            }
            Some(_) => {
                return Err(
                    Refusal::WrongStage("it was signed; its transaction decides its end").into(),
                )
            }
        }
        // AS-68: a dropped transaction may still confirm; the entry that was
        // in it cannot be cancelled while its outpoints are kept.
        if next.conflict_mut(id).is_some_and(|c| !c.is_empty()) {
            return Err(Refusal::WrongStage(
                "its dropped transaction may still confirm; ask for it again",
            )
            .into());
        }
        next.remove_entry(id);
        self.commit(next, persist)
    }

    /// AS-63: put a returned entry back in the queue.
    pub fn requeue<E>(
        &mut self,
        id: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        match next.stage_mut(id) {
            None => return Err(Refusal::UnknownEntry(id).into()),
            Some(stage @ Stage::Returned { .. }) => *stage = Stage::Queued,
            Some(_) => return Err(Refusal::WrongStage("it is not returned").into()),
        }
        self.commit(next, persist)
    }

    /// AS-66: give up on an open session. Its entries go back to queued.
    pub fn abandon_session<E>(
        &mut self,
        id: &[u8; 32],
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), OwnerError<E>> {
        let mut next = self.state.clone();
        let at = next
            .sessions
            .iter()
            .position(|s| s.id == *id)
            .ok_or(Refusal::UnknownSession)?;
        let session = next.sessions.remove(at);
        for (_, frozen) in &session.frozen {
            let ids: Vec<u32> = match frozen {
                Frozen::Spend {
                    refunds, cash_out, ..
                } => refunds.iter().copied().chain(*cash_out).collect(),
                Frozen::Batch(n) | Frozen::Settings(n) | Frozen::Pairing(n) => vec![*n],
                Frozen::Health => Vec::new(),
            };
            for n in ids {
                if let Some(stage) = next.stage_mut(n) {
                    if matches!(stage, Stage::InSession { session: s } if *s == session.id) {
                        *stage = Stage::Queued;
                    }
                }
            }
        }
        self.commit(next, persist)
    }
}
