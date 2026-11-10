//! The state the Key app keeps (section 1 of the rules) and the calls the
//! owner makes on the Key app's own screen.

use std::collections::{BTreeMap, BTreeSet};

use bitcoin::bip32::Xpub;
use bitcoin::hashes::Hash;
use bitcoin::{Network, OutPoint, Script, ScriptBuf, Txid};
use openbmt_core::batch::{AddressBatch, Approver, CosignError, HighWater};
use openbmt_core::identity::{DeviceId, Sig};
use openbmt_core::wallet::WalletDescription;

use crate::refusal::{ChangeError, ChangeRefusal, RegisterError};
use crate::snapshot::network_byte;
use crate::{
    ALLOWLIST_WAIT_SECS, DAY_SECS, FIRST_ALLOWLISTED, FIRST_FEE, FIRST_OTHER, MAX_ALLOWLIST,
    MAX_FEE_RATE_SAT_PER_VB, MAX_FEE_SATS, MAX_LABEL_CHARS, MAX_MONEY_SATS, WEEK_SECS,
};

/// A set of ceilings in satoshis: per transaction, per day and per week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceilings {
    /// Most one transaction may send.
    pub per_tx: u64,
    /// Most that may be sent in any 24 hours.
    pub per_day: u64,
    /// Most that may be sent in any 7 days.
    pub per_week: u64,
}

/// The fee ceilings of SR-40 and SR-41.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeCeilings {
    /// Highest fee rate, in satoshis per virtual byte.
    pub sat_per_vb: u64,
    /// Highest fee of one transaction, in satoshis.
    pub absolute_sats: u64,
}

/// A cash-out destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowlistEntry {
    pub(crate) script: ScriptBuf,
    pub(crate) label: String,
    pub(crate) enrolled_at: u64,
}

impl AllowlistEntry {
    /// The destination's script.
    pub fn script(&self) -> &Script {
        &self.script
    }

    /// The owner's label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The time it was enrolled, in seconds.
    pub fn enrolled_at(&self) -> u64 {
        self.enrolled_at
    }

    /// The time from which it counts as allowlisted: 24 hours after it was
    /// enrolled. `None` if that time is past the end of the clock; such an
    /// entry is never active.
    pub fn active_at(&self) -> Option<u64> {
        self.enrolled_at.checked_add(ALLOWLIST_WAIT_SECS)
    }
}

/// One signed transaction in the spend log (SR-33).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub(crate) txid: Txid,
    pub(crate) time: u64,
    pub(crate) other_sats: u64,
    pub(crate) allowlisted_sats: u64,
    pub(crate) fee_sats: u64,
    pub(crate) inputs: Vec<OutPoint>,
}

impl LogEntry {
    /// The transaction ID.
    pub fn txid(&self) -> Txid {
        self.txid
    }

    /// When it was signed, in seconds.
    pub fn time(&self) -> u64 {
        self.time
    }

    /// Total sent to other destinations.
    pub fn other_sats(&self) -> u64 {
        self.other_sats
    }

    /// Total sent to allowlisted destinations.
    pub fn allowlisted_sats(&self) -> u64 {
        self.allowlisted_sats
    }

    /// The fee.
    pub fn fee_sats(&self) -> u64 {
        self.fee_sats
    }

    /// The outpoints it spends, in ascending order.
    pub fn inputs(&self) -> &[OutPoint] {
        &self.inputs
    }
}

/// The day and week totals of SR-32. "Other" includes fees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    /// Other destinations plus fees in the last 24 hours.
    pub other_day: u64,
    /// Other destinations plus fees in the last 7 days.
    pub other_week: u64,
    /// Allowlisted destinations in the last 24 hours.
    pub allowlisted_day: u64,
    /// Allowlisted destinations in the last 7 days.
    pub allowlisted_week: u64,
}

/// Window sums before they are held to 64 bits.
pub(crate) struct Sums {
    pub other_day: u128,
    pub other_week: u128,
    pub allowlisted_day: u128,
    pub allowlisted_week: u128,
}

pub(crate) type LogMap = BTreeMap<[u8; 32], LogEntry>;

pub(crate) fn in_day(effective: u64, time: u64) -> bool {
    effective.saturating_sub(time) < DAY_SECS
}

pub(crate) fn in_week(effective: u64, time: u64) -> bool {
    effective.saturating_sub(time) < WEEK_SECS
}

/// Sort key of an outpoint in the spend log and its snapshot.
pub(crate) fn outpoint_key(o: &OutPoint) -> ([u8; 32], u32) {
    (o.txid.to_byte_array(), o.vout)
}

pub(crate) fn sums<'a>(entries: impl Iterator<Item = &'a LogEntry>, effective: u64) -> Sums {
    let mut s = Sums {
        other_day: 0,
        other_week: 0,
        allowlisted_day: 0,
        allowlisted_week: 0,
    };
    for e in entries {
        let other = u128::from(e.other_sats) + u128::from(e.fee_sats);
        let allow = u128::from(e.allowlisted_sats);
        if in_week(effective, e.time) {
            s.other_week += other;
            s.allowlisted_week += allow;
        }
        if in_day(effective, e.time) {
            s.other_day += other;
            s.allowlisted_day += allow;
        }
    }
    s
}

/// The outcome of adding a transaction to the spend log.
pub(crate) struct LogPlan {
    pub log: LogMap,
    pub already_counted: bool,
}

/// Whether `script` is one of the kinds SR-22 allows.
pub(crate) fn is_payment_script(script: &Script) -> bool {
    script.is_p2wpkh()
        || script.is_p2wsh()
        || script.is_p2tr()
        || script.is_p2pkh()
        || script.is_p2sh()
}

/// Whether `label` follows SR-30. The byte limit keeps it inside the
/// one-byte length of the snapshot.
pub(crate) fn label_is_valid(label: &str) -> bool {
    let chars = label.chars().count();
    (1..=MAX_LABEL_CHARS).contains(&chars)
        && label.len() <= usize::from(u8::MAX)
        && !label.chars().any(char::is_control)
}

/// Everything the Key app's signer remembers.
#[derive(Debug, Clone)]
pub struct SignerState {
    pub(crate) wallet: WalletDescription,
    pub(crate) own_slot: u8,
    pub(crate) key3_slot: Option<u8>,
    pub(crate) receive_mark: u32,
    pub(crate) change_mark: u32,
    pub(crate) latest_time_seen: u64,
    pub(crate) other: Ceilings,
    pub(crate) allowlisted: Ceilings,
    pub(crate) fee: FeeCeilings,
    /// Sorted by script bytes.
    pub(crate) allowlist: Vec<AllowlistEntry>,
    pub(crate) log: LogMap,
}

impl SignerState {
    /// Register the wallet (SR-60 to SR-62). This is also restore mode
    /// (SR-39): the state starts empty with the first values.
    ///
    /// `description` is the wallet description text. Exactly one key slot
    /// must hold `own_account_key` in full. If `key3` is given, exactly one
    /// other slot must hold it in full.
    pub fn register(
        description: &str,
        network: Network,
        own_account_key: &Xpub,
        key3: Option<&Xpub>,
    ) -> Result<Self, RegisterError> {
        if network_byte(network).is_none() {
            return Err(RegisterError::UnsupportedNetwork);
        }
        let wallet =
            WalletDescription::parse(description, network).map_err(RegisterError::Wallet)?;
        let own_slot = (1..=3u8)
            .find(|s| wallet.slot_matches(*s, own_account_key))
            .ok_or(RegisterError::OwnKeyNotInWallet)?;
        let key3_slot = match key3 {
            None => None,
            Some(k) => Some(
                (1..=3u8)
                    .find(|s| *s != own_slot && wallet.slot_matches(*s, k))
                    .ok_or(RegisterError::Key3NotInWallet)?,
            ),
        };
        Ok(Self {
            wallet,
            own_slot,
            key3_slot,
            receive_mark: 0,
            change_mark: 0,
            latest_time_seen: 0,
            other: FIRST_OTHER,
            allowlisted: FIRST_ALLOWLISTED,
            fee: FIRST_FEE,
            allowlist: Vec::new(),
            log: LogMap::new(),
        })
    }

    /// The registered wallet.
    pub fn wallet(&self) -> &WalletDescription {
        &self.wallet
    }

    /// This device's key slot: 1, 2 or 3.
    pub fn own_slot(&self) -> u8 {
        self.own_slot
    }

    /// The slot that holds key 3, if key 3 was given at setup.
    pub fn key3_slot(&self) -> Option<u8> {
        self.key3_slot
    }

    /// One past the highest receive index co-signed in an address batch,
    /// or the value the owner raised it to (SR-70).
    pub fn receive_mark(&self) -> u32 {
        self.receive_mark
    }

    /// One past the highest change index signed for.
    pub fn change_mark(&self) -> u32 {
        self.change_mark
    }

    /// The largest time at which this state signed or enrolled, in seconds
    /// (SR-31).
    pub fn latest_time_seen(&self) -> u64 {
        self.latest_time_seen
    }

    /// The allowlist, in script-byte order.
    pub fn allowlist(&self) -> impl Iterator<Item = &AllowlistEntry> {
        self.allowlist.iter()
    }

    /// The spend log, in transaction-ID byte order.
    pub fn log(&self) -> impl Iterator<Item = &LogEntry> {
        self.log.values()
    }

    /// Ceilings for other destinations.
    pub fn ceilings_other(&self) -> Ceilings {
        self.other
    }

    /// Ceilings for allowlisted destinations.
    pub fn ceilings_allowlisted(&self) -> Ceilings {
        self.allowlisted
    }

    /// The fee ceilings.
    pub fn fee_ceilings(&self) -> FeeCeilings {
        self.fee
    }

    /// The time the rules use: the larger of `now` and the latest time seen
    /// (SR-31).
    pub fn effective_time(&self, now: u64) -> u64 {
        now.max(self.latest_time_seen)
    }

    /// The day and week totals at the effective time for `now`. A total
    /// that does not fit in 64 bits is shown as the largest 64-bit number.
    pub fn totals(&self, now: u64) -> Totals {
        let s = sums(self.log.values(), self.effective_time(now));
        let cap = |v: u128| u64::try_from(v).unwrap_or(u64::MAX);
        Totals {
            other_day: cap(s.other_day),
            other_week: cap(s.other_week),
            allowlisted_day: cap(s.allowlisted_day),
            allowlisted_week: cap(s.allowlisted_week),
        }
    }

    /// The allowlist entry for `script` if it is active at `effective`.
    pub(crate) fn active_entry(&self, script: &Script, effective: u64) -> Option<&AllowlistEntry> {
        self.allowlist.iter().find(|e| {
            e.script.as_script() == script && e.active_at().is_some_and(|t| effective >= t)
        })
    }

    /// Add `new` (the transaction's own amounts) to the log as SR-33 says.
    /// The result is the log as it would be; nothing here changes `self`.
    pub(crate) fn plan_log(&self, new: LogEntry, effective: u64) -> LogPlan {
        let key = new.txid.to_byte_array();
        if self
            .log
            .get(&key)
            .is_some_and(|e| in_week(effective, e.time))
        {
            return LogPlan {
                log: self.log.clone(),
                already_counted: true,
            };
        }
        let mut log: LogMap = self
            .log
            .iter()
            .filter(|(_, e)| in_week(effective, e.time))
            .map(|(k, e)| (*k, e.clone()))
            .collect();
        let mut entry = new;
        let spent: BTreeSet<([u8; 32], u32)> = entry.inputs.iter().map(outpoint_key).collect();
        let joined = log
            .values()
            .filter(|e| e.inputs.iter().all(|i| spent.contains(&outpoint_key(i))))
            .max_by_key(|e| (e.time, e.txid.to_byte_array()))
            .map(|e| e.txid.to_byte_array());
        if let Some(k) = joined {
            if let Some(old) = log.remove(&k) {
                entry.other_sats = entry.other_sats.max(old.other_sats);
                entry.allowlisted_sats = entry.allowlisted_sats.max(old.allowlisted_sats);
                entry.fee_sats = entry.fee_sats.max(old.fee_sats);
            }
        }
        entry.time = effective;
        log.insert(key, entry);
        LogPlan {
            log,
            already_counted: false,
        }
    }

    /// Store `next` and, only if that succeeds, make it the state.
    fn commit<E>(
        &mut self,
        next: SignerState,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ChangeError<E>> {
        persist(&next.to_bytes()).map_err(ChangeError::Persist)?;
        *self = next;
        Ok(())
    }

    /// Enroll a cash-out destination (SR-30). It counts as allowlisted 24
    /// hours from the effective time. The latest time seen moves to the
    /// effective time in the same store.
    pub fn enroll_destination<E>(
        &mut self,
        script: ScriptBuf,
        label: &str,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ChangeError<E>> {
        if !is_payment_script(&script) {
            return Err(ChangeRefusal::ScriptKind.into());
        }
        if !label_is_valid(label) {
            return Err(ChangeRefusal::InvalidLabel.into());
        }
        let pos = match self
            .allowlist
            .binary_search_by(|e| e.script.as_bytes().cmp(script.as_bytes()))
        {
            Ok(_) => return Err(ChangeRefusal::AlreadyEnrolled.into()),
            Err(pos) => pos,
        };
        if self.allowlist.len() >= MAX_ALLOWLIST {
            return Err(ChangeRefusal::AllowlistFull.into());
        }
        let effective = self.effective_time(now);
        let mut next = self.clone();
        next.latest_time_seen = effective;
        next.allowlist.insert(
            pos,
            AllowlistEntry {
                script,
                label: label.to_string(),
                enrolled_at: effective,
            },
        );
        self.commit(next, persist)
    }

    /// Remove a destination from the allowlist. This takes effect at once.
    pub fn remove_destination<E>(
        &mut self,
        script: &Script,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ChangeError<E>> {
        let pos = self
            .allowlist
            .binary_search_by(|e| e.script.as_bytes().cmp(script.as_bytes()))
            .map_err(|_| ChangeRefusal::NotEnrolled)?;
        let mut next = self.clone();
        next.allowlist.remove(pos);
        self.commit(next, persist)
    }

    /// Set the ceilings (SR-37). They take effect at once, up or down.
    pub fn set_ceilings<E>(
        &mut self,
        other: Ceilings,
        allowlisted: Ceilings,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ChangeError<E>> {
        let all = [
            other.per_tx,
            other.per_day,
            other.per_week,
            allowlisted.per_tx,
            allowlisted.per_day,
            allowlisted.per_week,
        ];
        if all.iter().any(|v| *v > MAX_MONEY_SATS) {
            return Err(ChangeRefusal::CeilingAboveMoney.into());
        }
        let mut next = self.clone();
        next.other = other;
        next.allowlisted = allowlisted;
        self.commit(next, persist)
    }

    /// Set the fee ceilings (SR-40 to SR-42), within the compiled-in bounds.
    pub fn set_fee_ceilings<E>(
        &mut self,
        fee: FeeCeilings,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ChangeError<E>> {
        if !(1..=MAX_FEE_RATE_SAT_PER_VB).contains(&fee.sat_per_vb) {
            return Err(ChangeRefusal::FeeRateOutOfBounds.into());
        }
        if !(1..=MAX_FEE_SATS).contains(&fee.absolute_sats) {
            return Err(ChangeRefusal::FeeOutOfBounds.into());
        }
        let mut next = self.clone();
        next.fee = fee;
        self.commit(next, persist)
    }

    /// Raise the change mark (SR-34). It only goes up, to at most 2^31.
    pub fn raise_change_mark<E>(
        &mut self,
        new_mark: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ChangeError<E>> {
        if new_mark > 1 << 31 {
            return Err(ChangeRefusal::MarkTooHigh.into());
        }
        if new_mark <= self.change_mark {
            return Err(ChangeRefusal::MarkNotHigher.into());
        }
        let mut next = self.clone();
        next.change_mark = new_mark;
        self.commit(next, persist)
    }

    /// Raise the receive mark (SR-70). It only goes up, to at most 2^31.
    /// A restored Key app starts at 0 and needs this before it can co-sign
    /// the next address batch.
    pub fn raise_receive_mark<E>(
        &mut self,
        new_mark: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ChangeError<E>> {
        if new_mark > 1 << 31 {
            return Err(ChangeRefusal::ReceiveMarkTooHigh.into());
        }
        if new_mark <= self.receive_mark {
            return Err(ChangeRefusal::ReceiveMarkNotHigher.into());
        }
        let mut next = self.clone();
        next.receive_mark = new_mark;
        self.commit(next, persist)
    }

    /// Confirm a clock correction (SR-31). If `now` is not behind the
    /// latest time seen, nothing happens and `persist` is not called.
    /// Otherwise the latest time seen becomes `now`, allowlist entries
    /// enrolled after `now` are enrolled at `now`, and log entries dated
    /// after `now` are dated `now`.
    pub fn correct_clock<E>(
        &mut self,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ChangeError<E>> {
        if now >= self.latest_time_seen {
            return Ok(());
        }
        let mut next = self.clone();
        next.latest_time_seen = now;
        for e in &mut next.allowlist {
            e.enrolled_at = e.enrolled_at.min(now);
        }
        for e in next.log.values_mut() {
            e.time = e.time.min(now);
        }
        self.commit(next, persist)
    }

    /// Co-sign an address batch (section 10). The checks and their order
    /// are those of [`Approver::check_and_cosign`]; the receive mark is the
    /// one held here. The whole new state, with the new mark, is handed to
    /// `persist` before the co-signature is released. This does not move
    /// the clock.
    pub fn cosign_batch<E>(
        &mut self,
        approver: &Approver,
        batch: &AddressBatch,
        approved_counters: &BTreeSet<DeviceId>,
        owner: &DeviceId,
        owner_sig: &Sig,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Sig, CosignError<E>> {
        let mut mark = HighWater::from_next_index(self.receive_mark);
        let current = &*self;
        let sig = approver.check_and_cosign(
            &current.wallet,
            batch,
            &mut mark,
            approved_counters,
            owner,
            owner_sig,
            |new_mark| {
                let mut next = current.clone();
                next.receive_mark = new_mark.next_index();
                persist(&next.to_bytes())
            },
        )?;
        self.receive_mark = mark.next_index();
        Ok(sig)
    }
}
