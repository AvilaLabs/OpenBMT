//! Instant (T1) acceptance, as a pure state machine (§6).
//!
//! [`evaluate`] looks at one payment and the state around it and returns one
//! of three answers: accept at once, wait for a confirmation, or instant is
//! off right now. It reads no clock and no network: the caller passes ages
//! and flags in. The reasons are enums so the screen can turn each into plain
//! words.
//!
//! The pieces that keep state between payments are separate and small:
//! [`Exposure`] (what is outstanding), [`PauseTracker`] (pause after a
//! cancel) and [`classify_replacement`] (fee bump versus cancel).

use std::collections::BTreeMap;
use std::fmt;

use bitcoin::{Script, TxOut};

use crate::identity::DeviceId;
use crate::settings::Settings;
use crate::wire::{self, DecodeError, Reader};

/// Light check: largest instant sale, whole dollars.
pub const LIGHT_PER_SALE_USD: u32 = 10;
/// Light check: outstanding total for a counter's slice, whole dollars.
pub const LIGHT_SLICE_USD: u32 = 50;
/// Own node also seen by the light client: largest instant sale.
pub const NODE_PER_SALE_USD: u32 = 25;
/// Own node also seen by the light client: outstanding total.
pub const NODE_SLICE_USD: u32 = 100;
/// Box reports only: largest instant sale.
pub const BOX_PER_SALE_USD: u32 = 10;
/// Box reports only: outstanding total.
pub const BOX_SLICE_USD: u32 = 50;

/// Seconds with no conflicting spend seen before instant is allowed.
pub const CONFLICT_WAIT_SECS: u64 = 8;
/// Default longest age of the owner-phone heartbeat (10 minutes **[to tune]**).
pub const DEFAULT_HEARTBEAT_MAX_AGE_SECS: u32 = 600;
/// Pause after a cancel: 1 hour.
pub const PAUSE_AFTER_CANCEL_SECS: u64 = 3_600;
/// A second cancel inside this window pauses until the owner resets: 24 hours.
pub const SECOND_CANCEL_WINDOW_SECS: u64 = 86_400;

/// How the counter checked the payment, weakest to strongest of the
/// instant-eligible labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckMode {
    /// The counter's own light client.
    Light,
    /// The merchant's own node, and the counter's light client saw it too.
    NodeAlsoSeenByLight,
    /// Only the Box reports it.
    BoxReportsOnly,
}

/// What the screen says when a payment is accepted at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstantLabel {
    /// "Paid (light check)"
    LightCheck,
    /// "Paid (your node checked it)"
    YourNode,
    /// "Paid (your Box reports it)"
    YourBox,
}

impl InstantLabel {
    /// The words shown.
    pub fn text(&self) -> &'static str {
        match self {
            Self::LightCheck => "Paid (light check)",
            Self::YourNode => "Paid (your node checked it)",
            Self::YourBox => "Paid (your Box reports it)",
        }
    }
}

/// Limits for one check mode, in satoshis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeLimits {
    /// Largest `max(sale, received)` accepted at once. Zero turns instant off.
    pub per_sale_sats: u64,
    /// Largest outstanding instant total for this counter. Zero turns
    /// instant off.
    pub slice_sats: u64,
}

/// All the numbers the rules need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstantPolicy {
    /// Light check limits.
    pub light: ModeLimits,
    /// Own node limits.
    pub node: ModeLimits,
    /// Box-only limits.
    pub box_only: ModeLimits,
    /// Allowed shortfall against the sale amount, basis points.
    pub underpayment_tolerance_bps: u32,
    /// Longest age of the owner-phone heartbeat, seconds.
    pub heartbeat_max_age_secs: u64,
}

impl InstantPolicy {
    /// The §6 dollar defaults converted at `sats_per_usd`.
    pub fn defaults(sats_per_usd: u64) -> Self {
        let m = |per: u32, slice: u32| ModeLimits {
            per_sale_sats: u64::from(per).saturating_mul(sats_per_usd),
            slice_sats: u64::from(slice).saturating_mul(sats_per_usd),
        };
        Self {
            light: m(LIGHT_PER_SALE_USD, LIGHT_SLICE_USD),
            node: m(NODE_PER_SALE_USD, NODE_SLICE_USD),
            box_only: m(BOX_PER_SALE_USD, BOX_SLICE_USD),
            underpayment_tolerance_bps: 50,
            heartbeat_max_age_secs: u64::from(DEFAULT_HEARTBEAT_MAX_AGE_SECS),
        }
    }

    /// The numbers from the shop's signed settings, for one counter. Per-sale
    /// limits are shop-wide; each mode's slice is `counter`'s share of that
    /// mode's budget (zero, so instant off, if the counter has no share).
    pub fn for_counter(s: &Settings, counter: &DeviceId) -> Self {
        let m = |per: &crate::settings::Limit, mode: CheckMode| ModeLimits {
            per_sale_sats: per.sats,
            slice_sats: s.slice_sats(counter, mode),
        };
        Self {
            light: m(&s.light_per_sale, CheckMode::Light),
            node: m(&s.node_per_sale, CheckMode::NodeAlsoSeenByLight),
            box_only: m(&s.box_per_sale, CheckMode::BoxReportsOnly),
            underpayment_tolerance_bps: s.underpayment_tolerance_bps,
            heartbeat_max_age_secs: u64::from(s.heartbeat_max_age_secs),
        }
    }

    /// Limits for a mode.
    pub fn limits(&self, mode: CheckMode) -> ModeLimits {
        match mode {
            CheckMode::Light => self.light,
            CheckMode::NodeAlsoSeenByLight => self.node,
            CheckMode::BoxReportsOnly => self.box_only,
        }
    }
}

/// The owner-phone heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Heartbeat {
    /// The signature checked out against the pinned owner root.
    pub verified: bool,
    /// Seconds since it was sent.
    pub age_secs: u64,
}

/// Whether instant is paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseStatus {
    /// Not paused.
    Running,
    /// Paused for another `secs_left` seconds.
    PausedFor { secs_left: u64 },
    /// Paused until the owner resets it on the owner phone.
    UntilOwnerReset,
}

/// Everything one decision looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inputs {
    /// How the payment was checked.
    pub mode: CheckMode,
    /// The sale amount.
    pub sale_sats: u64,
    /// What actually arrived.
    pub received_sats: u64,
    /// Instant payments this counter accepted and not yet released.
    pub outstanding_sats: u64,
    /// The check is healthy (enough peers, current headers).
    pub check_healthy: bool,
    /// The owner-phone heartbeat.
    pub heartbeat: Heartbeat,
    /// Remote or online order.
    pub remote_order: bool,
    /// Customers can join the Wi-Fi the counter is on (or unsure).
    pub customer_wifi: bool,
    /// The Tor switch is on.
    pub tor_on: bool,
    /// Pause state.
    pub pause: PauseStatus,
    /// A conflicting spend of the payment's coins was seen.
    pub conflict_seen: bool,
    /// Seconds since the payment was first seen.
    pub secs_since_first_seen: u64,
}

/// Why instant is off for now (not a verdict on this payment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffReason {
    /// Paused after a cancel, for a while.
    PausedFor { secs_left: u64 },
    /// Paused after repeated cancels until the owner resets it.
    PausedUntilOwnerReset,
    /// The shop's limit for this check is zero.
    Disabled,
    /// The check is unhealthy.
    CheckUnhealthy,
    /// "Instant off: no independent watcher."
    NoWatcher,
    /// Customers can join the Wi-Fi and Tor is off.
    SharedWifiNeedsTor,
}

/// Why this payment must wait for a confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitReason {
    /// A conflicting spend was seen.
    ConflictSeen,
    /// Remote or online order.
    RemoteOrder,
    /// Less arrived than the sale needs.
    Underpaid,
    /// `max(sale, received)` is over the per-sale limit.
    OverSaleLimit,
    /// The counter's slice has no room.
    SliceFull,
    /// Still inside the 8-second conflict watch.
    ConflictWatch { secs_left: u64 },
}

/// The answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Accept now. The caller then records the exposure.
    Instant { label: InstantLabel },
    /// Wait for a confirmation.
    WaitForConfirmation { reason: WaitReason },
    /// Instant is off.
    InstantOff { reason: OffReason },
}

/// The amount instant is judged on: `max(sale, received)`.
pub fn amount_at_risk(sale_sats: u64, received_sats: u64) -> u64 {
    sale_sats.max(received_sats)
}

/// Decide. Checked in this order: whether instant is off at all (pause,
/// disabled, check health, watcher, network), then what is wrong with this
/// payment (conflict, remote, underpaid, limit, slice), then the 8-second
/// watch.
///
/// The mode used for limits and for the label is the effective mode. On a
/// network that customers can join without Tor, the light client's own
/// sighting is not independent evidence, so `NodeAlsoSeenByLight` with
/// `customer_wifi` set and Tor off counts as `BoxReportsOnly`. The light
/// check on such a network is off altogether (`SharedWifiNeedsTor`).
pub fn evaluate(policy: &InstantPolicy, i: &Inputs) -> Decision {
    use Decision::{Instant, InstantOff, WaitForConfirmation};
    let mode = if i.mode == CheckMode::NodeAlsoSeenByLight && i.customer_wifi && !i.tor_on {
        CheckMode::BoxReportsOnly
    } else {
        i.mode
    };
    let limits = policy.limits(mode);

    match i.pause {
        PauseStatus::PausedFor { secs_left } => {
            return InstantOff {
                reason: OffReason::PausedFor { secs_left },
            }
        }
        PauseStatus::UntilOwnerReset => {
            return InstantOff {
                reason: OffReason::PausedUntilOwnerReset,
            }
        }
        PauseStatus::Running => {}
    }
    if limits.per_sale_sats == 0 || limits.slice_sats == 0 {
        return InstantOff {
            reason: OffReason::Disabled,
        };
    }
    if !i.check_healthy {
        return InstantOff {
            reason: OffReason::CheckUnhealthy,
        };
    }
    if !i.heartbeat.verified || i.heartbeat.age_secs > policy.heartbeat_max_age_secs {
        return InstantOff {
            reason: OffReason::NoWatcher,
        };
    }
    if mode == CheckMode::Light && i.customer_wifi && !i.tor_on {
        return InstantOff {
            reason: OffReason::SharedWifiNeedsTor,
        };
    }

    let wait = |reason| WaitForConfirmation { reason };
    if i.conflict_seen {
        return wait(WaitReason::ConflictSeen);
    }
    if i.remote_order {
        return wait(WaitReason::RemoteOrder);
    }
    let tolerance =
        u128::from(i.sale_sats) * u128::from(policy.underpayment_tolerance_bps) / 10_000;
    if u128::from(i.received_sats) + tolerance < u128::from(i.sale_sats) {
        return wait(WaitReason::Underpaid);
    }
    let amount = amount_at_risk(i.sale_sats, i.received_sats);
    if amount > limits.per_sale_sats {
        return wait(WaitReason::OverSaleLimit);
    }
    if u128::from(i.outstanding_sats) + u128::from(amount) > u128::from(limits.slice_sats) {
        return wait(WaitReason::SliceFull);
    }
    if i.secs_since_first_seen < CONFLICT_WAIT_SECS {
        return wait(WaitReason::ConflictWatch {
            secs_left: CONFLICT_WAIT_SECS - i.secs_since_first_seen,
        });
    }
    Instant {
        label: match mode {
            CheckMode::Light => InstantLabel::LightCheck,
            CheckMode::NodeAlsoSeenByLight => InstantLabel::YourNode,
            CheckMode::BoxReportsOnly => InstantLabel::YourBox,
        },
    }
}

/// Identifies one accepted payment in the exposure ledger (a txid, say).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaymentId(pub [u8; 32]);

/// Why exposure could not be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposureError {
    /// The counter's slice has no room for this amount.
    SliceFull,
    /// This payment is already counted.
    AlreadyCounted,
}

impl fmt::Display for ExposureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SliceFull => write!(f, "this counter's exposure slice is full"),
            Self::AlreadyCounted => write!(f, "payment already counted"),
        }
    }
}

impl std::error::Error for ExposureError {}

/// Why an exposure entry was not added: the rules refused it, or the new
/// ledger could not be stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposureWriteError<E> {
    /// The slice is full or the payment is already counted. The ledger is
    /// unchanged.
    Refused(ExposureError),
    /// The caller's store refused the new ledger. The ledger is unchanged.
    Persist(E),
}

impl<E: fmt::Display> fmt::Display for ExposureWriteError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(e) => write!(f, "{e}"),
            Self::Persist(e) => write!(f, "could not store the new exposure: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for ExposureWriteError<E> {}

impl<E> From<ExposureError> for ExposureWriteError<E> {
    fn from(e: ExposureError) -> Self {
        Self::Refused(e)
    }
}

/// What a counter has accepted at once and not yet seen settle. A payment
/// enters on instant acceptance (or an override) and leaves only at its first
/// confirmation or by the owner's action. Nothing here knows the time, so
/// nothing can leave on a timeout. The limit it is held to is passed with each
/// addition (the counter's `slice_sats` for the mode in use), so a new signed
/// limit takes effect at once and never touches what is already counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Exposure {
    entries: BTreeMap<PaymentId, u64>,
}

impl Exposure {
    /// Nothing outstanding.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sum of everything outstanding.
    pub fn outstanding(&self) -> u64 {
        self.entries
            .values()
            .fold(0u64, |a, b| a.saturating_add(*b))
    }

    fn add<E>(
        &mut self,
        id: PaymentId,
        amount: u64,
        slice_limit_sats: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ExposureWriteError<E>> {
        if self.entries.contains_key(&id) {
            return Err(ExposureError::AlreadyCounted.into());
        }
        if u128::from(self.outstanding()) + u128::from(amount) > u128::from(slice_limit_sats) {
            return Err(ExposureError::SliceFull.into());
        }
        let mut next = self.clone();
        next.entries.insert(id, amount);
        persist(&next.to_bytes()).map_err(ExposureWriteError::Persist)?;
        *self = next;
        Ok(())
    }

    /// Count a payment accepted as instant (its `max(sale, received)`).
    /// Refused when it would take the total above `slice_limit_sats`.
    ///
    /// The ledger as it will be with the entry is serialized
    /// ([`Self::to_bytes`]) and given to `persist`; the entry is added only
    /// if `persist` returns `Ok`. The order matters: an acceptance that was
    /// not stored first could be forgotten in a crash, and the lost exposure
    /// would make room for more instant payments than the slice allows.
    pub fn accept_instant<E>(
        &mut self,
        id: PaymentId,
        amount: u64,
        slice_limit_sats: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ExposureWriteError<E>> {
        self.add(id, amount, slice_limit_sats, persist)
    }

    /// Count a cashier or manager override. Refused when it would take the
    /// total above `slice_limit_sats`. The `persist` rule is the same as for
    /// [`Self::accept_instant`], for the same reason.
    pub fn add_override<E>(
        &mut self,
        id: PaymentId,
        amount: u64,
        slice_limit_sats: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), ExposureWriteError<E>> {
        self.add(id, amount, slice_limit_sats, persist)
    }

    /// The payment reached its first confirmation. Returns what was released.
    pub fn release_on_first_confirmation(&mut self, id: &PaymentId) -> Option<u64> {
        self.entries.remove(id)
    }

    /// The owner released it (written off, or settled another way). Returns
    /// what was released.
    pub fn release_by_owner(&mut self, id: &PaymentId) -> Option<u64> {
        self.entries.remove(id)
    }

    /// Snapshot: `OBME`, version 1, the entry count as u32 LE, then each
    /// entry as the 32-byte payment id and its amount as u64 LE, in
    /// ascending id order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&wire::MAGIC_EXPOSURE, 4 + self.entries.len() * 40);
        out.extend_from_slice(&wire::len_u32(self.entries.len()).to_le_bytes());
        for (id, amount) in &self.entries {
            out.extend_from_slice(&id.0);
            out.extend_from_slice(&amount.to_le_bytes());
        }
        out
    }

    /// Restore a snapshot strictly (see [`crate::wire`]). Ids must be in
    /// strictly ascending order, and the amounts must not add up past
    /// `u64::MAX`; otherwise the result is `NonCanonical`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_EXPOSURE)?;
        let n = r.count(usize::MAX, 40)?;
        let mut entries = BTreeMap::new();
        let mut last: Option<[u8; 32]> = None;
        let mut total = 0u64;
        for _ in 0..n {
            let id = r.array::<32>()?;
            if last.is_some_and(|prev| prev >= id) {
                return Err(DecodeError::NonCanonical(
                    "payment ids are not in ascending order",
                ));
            }
            last = Some(id);
            let amount = r.u64_le()?;
            total = total
                .checked_add(amount)
                .ok_or(DecodeError::NonCanonical("outstanding total overflows"))?;
            entries.insert(PaymentId(id), amount);
        }
        r.finish()?;
        Ok(Self { entries })
    }
}

/// Pause state after cancels.
///
/// The time passed in is whatever clock the caller trusts. A clock moved
/// forward can end a pause early. The bound on what can be lost does not
/// depend on the pause: exposure stays counted until a confirmation or the
/// owner's release, so an early end to a pause adds no room.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PauseTracker {
    last_cancel: Option<u64>,
    paused_until: Option<u64>,
    locked: bool,
}

impl PauseTracker {
    /// Not paused, no cancels on record.
    pub fn new() -> Self {
        Self::default()
    }

    /// A cancel happened at `now` (seconds on the counter's own clock). It
    /// pauses instant for 1 hour. A second cancel within 24 hours of the
    /// last one pauses it until the owner resets.
    pub fn record_cancel(&mut self, now: u64) {
        if let Some(prev) = self.last_cancel {
            if now.saturating_sub(prev) <= SECOND_CANCEL_WINDOW_SECS {
                self.locked = true;
            }
        }
        if !self.locked {
            self.paused_until = Some(now.saturating_add(PAUSE_AFTER_CANCEL_SECS));
        }
        self.last_cancel = Some(now);
    }

    /// The pause state at `now`.
    pub fn status(&self, now: u64) -> PauseStatus {
        if self.locked {
            return PauseStatus::UntilOwnerReset;
        }
        match self.paused_until {
            Some(until) if now < until => PauseStatus::PausedFor {
                secs_left: until - now,
            },
            _ => PauseStatus::Running,
        }
    }

    /// The owner reset the pause on the owner phone. Clears the cancel
    /// history too.
    pub fn owner_reset(&mut self) {
        *self = Self::default();
    }

    /// Snapshot: `OBMP`, version 1, then `last_cancel` and `paused_until`
    /// (each a flag byte and, when the flag is 1, a u64 LE) and the `locked`
    /// flag.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&wire::MAGIC_PAUSE_TRACKER, 20);
        wire::put_opt_u64(&mut out, self.last_cancel);
        wire::put_opt_u64(&mut out, self.paused_until);
        out.push(u8::from(self.locked));
        out
    }

    /// Restore a snapshot strictly (see [`crate::wire`]). A pause or lock
    /// without a recorded cancel cannot arise from `record_cancel`, so it is
    /// `NonCanonical`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_PAUSE_TRACKER)?;
        let last_cancel = r.opt_u64()?;
        let paused_until = r.opt_u64()?;
        let locked = r.flag()?;
        r.finish()?;
        if last_cancel.is_none() && (paused_until.is_some() || locked) {
            return Err(DecodeError::NonCanonical("pause state without a cancel"));
        }
        Ok(Self {
            last_cancel,
            paused_until,
            locked,
        })
    }
}

/// What a replacement transaction did to the payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementKind {
    /// Still pays us at least what the original did: only the fee changed.
    FeeBump,
    /// Anything else.
    Cancel,
}

fn paid_to(outputs: &[TxOut], script: &Script) -> u64 {
    outputs
        .iter()
        .filter(|o| o.script_pubkey.as_script() == script)
        .fold(0u64, |a, o| a.saturating_add(o.value.to_sat()))
}

/// Sort a replacement of a payment we counted. It is a fee bump only if the
/// original paid `our_script` more than zero and the replacement pays
/// `our_script` at least as much as the original did, summing every output to
/// that script. Otherwise it is a cancel.
pub fn classify_replacement(
    original_outputs: &[TxOut],
    replacement_outputs: &[TxOut],
    our_script: &Script,
) -> ReplacementKind {
    let original = paid_to(original_outputs, our_script);
    if original > 0 && paid_to(replacement_outputs, our_script) >= original {
        ReplacementKind::FeeBump
    } else {
        ReplacementKind::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{persist_fail, persist_ok};
    use bitcoin::{Amount, ScriptBuf};
    use std::convert::Infallible;

    const RATE: u64 = 1_000; // sats per dollar, so $10 = 10_000 sats

    /// The ledger's additions with a store that always succeeds, refusals
    /// flattened to `ExposureError`.
    trait Quick {
        fn accept_ok(
            &mut self,
            id: PaymentId,
            amount: u64,
            limit: u64,
        ) -> Result<(), ExposureError>;
        fn override_ok(
            &mut self,
            id: PaymentId,
            amount: u64,
            limit: u64,
        ) -> Result<(), ExposureError>;
    }

    fn flatten(r: Result<(), ExposureWriteError<Infallible>>) -> Result<(), ExposureError> {
        match r {
            Ok(()) => Ok(()),
            Err(ExposureWriteError::Refused(e)) => Err(e),
            Err(ExposureWriteError::Persist(never)) => match never {},
        }
    }

    impl Quick for Exposure {
        fn accept_ok(
            &mut self,
            id: PaymentId,
            amount: u64,
            limit: u64,
        ) -> Result<(), ExposureError> {
            flatten(self.accept_instant(id, amount, limit, persist_ok))
        }

        fn override_ok(
            &mut self,
            id: PaymentId,
            amount: u64,
            limit: u64,
        ) -> Result<(), ExposureError> {
            flatten(self.add_override(id, amount, limit, persist_ok))
        }
    }

    fn policy() -> InstantPolicy {
        InstantPolicy::defaults(RATE)
    }

    /// A payment that would be accepted at once under the light check.
    fn ok_light() -> Inputs {
        Inputs {
            mode: CheckMode::Light,
            sale_sats: 5_000,
            received_sats: 5_000,
            outstanding_sats: 0,
            check_healthy: true,
            heartbeat: Heartbeat {
                verified: true,
                age_secs: 30,
            },
            remote_order: false,
            customer_wifi: false,
            tor_on: false,
            pause: PauseStatus::Running,
            conflict_seen: false,
            secs_since_first_seen: 8,
        }
    }

    fn wait(r: WaitReason) -> Decision {
        Decision::WaitForConfirmation { reason: r }
    }
    fn off(r: OffReason) -> Decision {
        Decision::InstantOff { reason: r }
    }

    #[test]
    fn happy_path_gives_the_light_label() {
        assert_eq!(
            evaluate(&policy(), &ok_light()),
            Decision::Instant {
                label: InstantLabel::LightCheck
            }
        );
        assert_eq!(InstantLabel::LightCheck.text(), "Paid (light check)");
    }

    #[test]
    fn dollar_defaults_are_the_documented_ones() {
        let p = policy();
        assert_eq!(
            (p.light.per_sale_sats, p.light.slice_sats),
            (10_000, 50_000)
        );
        assert_eq!((p.node.per_sale_sats, p.node.slice_sats), (25_000, 100_000));
        assert_eq!(
            (p.box_only.per_sale_sats, p.box_only.slice_sats),
            (10_000, 50_000)
        );
        assert_eq!(
            (
                LIGHT_PER_SALE_USD,
                LIGHT_SLICE_USD,
                NODE_PER_SALE_USD,
                NODE_SLICE_USD,
                BOX_PER_SALE_USD,
                BOX_SLICE_USD
            ),
            (10, 50, 25, 100, 10, 50)
        );
    }

    fn settings_with_counter(share: u16) -> (Settings, DeviceId) {
        let mut s = Settings::defaults(RATE);
        let c = crate::testutil::identity(70).device_id();
        s.devices.insert(c);
        s.counters.insert(c, share);
        (s, c)
    }

    #[test]
    fn policy_follows_settings_for_one_counter() {
        let (s, c) = settings_with_counter(10_000);
        let p = InstantPolicy::for_counter(&s, &c);
        assert_eq!(p.light, policy().light);
        assert_eq!(p.node, policy().node);
        assert_eq!(p.box_only, policy().box_only);
        assert_eq!(p.underpayment_tolerance_bps, 50);
        assert_eq!(p.heartbeat_max_age_secs, 600);
        // half the budget: per-sale limits stay shop-wide, slices halve
        let (s, c) = settings_with_counter(5_000);
        let p = InstantPolicy::for_counter(&s, &c);
        assert_eq!(p.light.per_sale_sats, 10_000);
        assert_eq!(p.light.slice_sats, 25_000);
        assert_eq!(p.node.slice_sats, 50_000);
        assert_eq!(p.box_only.slice_sats, 25_000);
    }

    #[test]
    fn a_counter_without_a_share_has_instant_off() {
        let (s, _) = settings_with_counter(10_000);
        let stranger = crate::testutil::identity(71).device_id();
        let p = InstantPolicy::for_counter(&s, &stranger);
        assert_eq!(p.light.slice_sats, 0);
        assert_eq!(evaluate(&p, &ok_light()), off(OffReason::Disabled));
    }

    #[test]
    fn default_tolerance_is_half_a_percent() {
        assert_eq!(policy().underpayment_tolerance_bps, 50);
    }

    #[test]
    fn each_mode_uses_its_own_limit_and_label() {
        let p = policy();
        // $20 sale: over light and box, fine for node
        for (mode, want) in [
            (CheckMode::Light, wait(WaitReason::OverSaleLimit)),
            (CheckMode::BoxReportsOnly, wait(WaitReason::OverSaleLimit)),
            (
                CheckMode::NodeAlsoSeenByLight,
                Decision::Instant {
                    label: InstantLabel::YourNode,
                },
            ),
        ] {
            let i = Inputs {
                mode,
                sale_sats: 20_000,
                received_sats: 20_000,
                customer_wifi: false,
                ..ok_light()
            };
            assert_eq!(evaluate(&p, &i), want, "{mode:?}");
        }
        let i = Inputs {
            mode: CheckMode::BoxReportsOnly,
            ..ok_light()
        };
        assert_eq!(
            evaluate(&p, &i),
            Decision::Instant {
                label: InstantLabel::YourBox
            }
        );
        assert_eq!(InstantLabel::YourNode.text(), "Paid (your node checked it)");
        assert_eq!(InstantLabel::YourBox.text(), "Paid (your Box reports it)");
    }

    #[test]
    fn limit_is_inclusive_and_judged_on_max_of_sale_and_received() {
        let p = policy();
        let at = Inputs {
            sale_sats: 10_000,
            received_sats: 10_000,
            ..ok_light()
        };
        assert!(matches!(evaluate(&p, &at), Decision::Instant { .. }));
        let over = Inputs {
            sale_sats: 10_001,
            received_sats: 10_001,
            ..ok_light()
        };
        assert_eq!(evaluate(&p, &over), wait(WaitReason::OverSaleLimit));
        // a small sale paid with a huge amount counts as the huge amount
        let huge = Inputs {
            sale_sats: 1_000,
            received_sats: 5_000_000,
            ..ok_light()
        };
        assert_eq!(evaluate(&p, &huge), wait(WaitReason::OverSaleLimit));
        assert_eq!(amount_at_risk(1, 9), 9);
        assert_eq!(amount_at_risk(9, 1), 9);
    }

    #[test]
    fn zero_limit_turns_instant_off() {
        let mut p = policy();
        p.light.per_sale_sats = 0;
        assert_eq!(evaluate(&p, &ok_light()), off(OffReason::Disabled));
        let mut p = policy();
        p.light.slice_sats = 0;
        assert_eq!(evaluate(&p, &ok_light()), off(OffReason::Disabled));
        // other modes unaffected
        let i = Inputs {
            mode: CheckMode::BoxReportsOnly,
            ..ok_light()
        };
        assert!(matches!(evaluate(&p, &i), Decision::Instant { .. }));
    }

    #[test]
    fn remote_orders_never_go_instant() {
        let i = Inputs {
            remote_order: true,
            ..ok_light()
        };
        assert_eq!(evaluate(&policy(), &i), wait(WaitReason::RemoteOrder));
    }

    #[test]
    fn shared_wifi_needs_tor_for_light_check_only() {
        let p = policy();
        let wifi = Inputs {
            customer_wifi: true,
            tor_on: false,
            ..ok_light()
        };
        assert_eq!(evaluate(&p, &wifi), off(OffReason::SharedWifiNeedsTor));
        let tor = Inputs {
            tor_on: true,
            ..wifi
        };
        assert!(matches!(evaluate(&p, &tor), Decision::Instant { .. }));
        let node = Inputs {
            mode: CheckMode::NodeAlsoSeenByLight,
            ..wifi
        };
        assert!(matches!(evaluate(&p, &node), Decision::Instant { .. }));
    }

    #[test]
    fn unhealthy_check_turns_instant_off() {
        let i = Inputs {
            check_healthy: false,
            ..ok_light()
        };
        assert_eq!(evaluate(&policy(), &i), off(OffReason::CheckUnhealthy));
    }

    #[test]
    fn heartbeat_must_be_verified_and_fresh() {
        let p = policy();
        let unverified = Inputs {
            heartbeat: Heartbeat {
                verified: false,
                age_secs: 1,
            },
            ..ok_light()
        };
        assert_eq!(evaluate(&p, &unverified), off(OffReason::NoWatcher));
        let edge = Inputs {
            heartbeat: Heartbeat {
                verified: true,
                age_secs: 600,
            },
            ..ok_light()
        };
        assert!(matches!(evaluate(&p, &edge), Decision::Instant { .. }));
        let stale = Inputs {
            heartbeat: Heartbeat {
                verified: true,
                age_secs: 601,
            },
            ..ok_light()
        };
        assert_eq!(evaluate(&p, &stale), off(OffReason::NoWatcher));
    }

    #[test]
    fn conflict_seen_waits() {
        let i = Inputs {
            conflict_seen: true,
            secs_since_first_seen: 100,
            ..ok_light()
        };
        assert_eq!(evaluate(&policy(), &i), wait(WaitReason::ConflictSeen));
    }

    #[test]
    fn eight_seconds_without_conflict_before_instant() {
        let p = policy();
        for s in 0..8 {
            let i = Inputs {
                secs_since_first_seen: s,
                ..ok_light()
            };
            assert_eq!(
                evaluate(&p, &i),
                wait(WaitReason::ConflictWatch { secs_left: 8 - s }),
                "{s}"
            );
        }
        let i = Inputs {
            secs_since_first_seen: 8,
            ..ok_light()
        };
        assert!(matches!(evaluate(&p, &i), Decision::Instant { .. }));
        // a conflict at second 9 still wins
        let i = Inputs {
            secs_since_first_seen: 9,
            conflict_seen: true,
            ..ok_light()
        };
        assert_eq!(evaluate(&p, &i), wait(WaitReason::ConflictSeen));
    }

    #[test]
    fn full_slice_waits() {
        let p = policy();
        let room = Inputs {
            outstanding_sats: 45_000,
            ..ok_light()
        };
        assert!(matches!(evaluate(&p, &room), Decision::Instant { .. })); // 45k + 5k = 50k
        let full = Inputs {
            outstanding_sats: 45_001,
            ..ok_light()
        };
        assert_eq!(evaluate(&p, &full), wait(WaitReason::SliceFull));
    }

    #[test]
    fn the_counters_own_slice_binds() {
        let mut p = policy();
        p.light.slice_sats = 4_000;
        assert_eq!(evaluate(&p, &ok_light()), wait(WaitReason::SliceFull));
        p.light.slice_sats = 5_000;
        assert!(matches!(
            evaluate(&p, &ok_light()),
            Decision::Instant { .. }
        ));
    }

    #[test]
    fn exposure_uses_the_larger_of_sale_and_received() {
        // overpaid by a little, slice has room for the sale but not the payment
        let i = Inputs {
            sale_sats: 5_000,
            received_sats: 5_500,
            outstanding_sats: 45_000,
            ..ok_light()
        };
        assert_eq!(evaluate(&policy(), &i), wait(WaitReason::SliceFull));
    }

    #[test]
    fn underpayment_waits_unless_within_tolerance() {
        let p = policy(); // 0.5%
        let within = Inputs {
            sale_sats: 10_000,
            received_sats: 9_950,
            ..ok_light()
        };
        assert!(matches!(evaluate(&p, &within), Decision::Instant { .. }));
        let under = Inputs {
            received_sats: 9_949,
            ..within
        };
        assert_eq!(evaluate(&p, &under), wait(WaitReason::Underpaid));
        let nothing = Inputs {
            received_sats: 0,
            ..within
        };
        assert_eq!(evaluate(&p, &nothing), wait(WaitReason::Underpaid));
        let mut strict = policy();
        strict.underpayment_tolerance_bps = 0;
        assert_eq!(evaluate(&strict, &within), wait(WaitReason::Underpaid));
    }

    #[test]
    fn pause_beats_everything_else() {
        let p = policy();
        let paused = Inputs {
            pause: PauseStatus::PausedFor { secs_left: 120 },
            ..ok_light()
        };
        assert_eq!(
            evaluate(&p, &paused),
            off(OffReason::PausedFor { secs_left: 120 })
        );
        let locked = Inputs {
            pause: PauseStatus::UntilOwnerReset,
            check_healthy: false,
            ..ok_light()
        };
        assert_eq!(evaluate(&p, &locked), off(OffReason::PausedUntilOwnerReset));
    }

    #[test]
    fn off_reasons_come_before_wait_reasons() {
        let i = Inputs {
            check_healthy: false,
            remote_order: true,
            conflict_seen: true,
            sale_sats: 9_999_999,
            received_sats: 9_999_999,
            ..ok_light()
        };
        assert_eq!(evaluate(&policy(), &i), off(OffReason::CheckUnhealthy));
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        let i = Inputs {
            sale_sats: u64::MAX,
            received_sats: u64::MAX,
            outstanding_sats: u64::MAX,
            ..ok_light()
        };
        assert_eq!(evaluate(&policy(), &i), wait(WaitReason::OverSaleLimit));
        let mut p = policy();
        p.light.per_sale_sats = u64::MAX;
        p.light.slice_sats = u64::MAX;
        assert_eq!(evaluate(&p, &i), wait(WaitReason::SliceFull));
    }

    fn pid(n: u8) -> PaymentId {
        PaymentId([n; 32])
    }

    const LIMIT: u64 = 50_000;

    #[test]
    fn exposure_is_added_on_acceptance_and_released_on_first_confirmation() {
        let mut s = Exposure::new();
        s.accept_ok(pid(1), 10_000, LIMIT).unwrap();
        s.accept_ok(pid(2), 5_000, LIMIT).unwrap();
        assert_eq!(s.outstanding(), 15_000);
        assert_eq!(s.release_on_first_confirmation(&pid(1)), Some(10_000));
        assert_eq!(s.outstanding(), 5_000);
        assert_eq!(
            s.release_on_first_confirmation(&pid(1)),
            None,
            "released once only"
        );
    }

    #[test]
    fn exposure_is_released_by_the_owner_only_by_explicit_action() {
        let mut s = Exposure::new();
        s.accept_ok(pid(1), 10_000, LIMIT).unwrap();
        // nothing in this API takes a time, and evaluating does not release
        for _ in 0..100 {
            let _ = evaluate(
                &policy(),
                &Inputs {
                    outstanding_sats: s.outstanding(),
                    ..ok_light()
                },
            );
        }
        assert_eq!(s.outstanding(), 10_000);
        assert_eq!(s.release_by_owner(&pid(1)), Some(10_000));
        assert_eq!(s.outstanding(), 0);
        assert_eq!(s.release_by_owner(&pid(9)), None);
    }

    #[test]
    fn duplicate_payment_is_not_counted_twice() {
        let mut s = Exposure::new();
        s.accept_ok(pid(1), 10_000, LIMIT).unwrap();
        assert_eq!(
            s.accept_ok(pid(1), 10_000, LIMIT),
            Err(ExposureError::AlreadyCounted)
        );
        assert_eq!(
            s.override_ok(pid(1), 10, LIMIT),
            Err(ExposureError::AlreadyCounted)
        );
        assert_eq!(s.outstanding(), 10_000);
    }

    #[test]
    fn overrides_add_to_exposure_and_are_refused_when_full() {
        let mut s = Exposure::new();
        s.accept_ok(pid(1), 10_000, 20_000).unwrap();
        s.override_ok(pid(2), 10_000, 20_000).unwrap();
        assert_eq!(s.outstanding(), 20_000);
        assert_eq!(
            s.override_ok(pid(3), 1, 20_000),
            Err(ExposureError::SliceFull)
        );
        assert_eq!(
            s.accept_ok(pid(4), 1, 20_000),
            Err(ExposureError::SliceFull)
        );
        assert_eq!(s.outstanding(), 20_000);
        // room comes back only through a release
        s.release_on_first_confirmation(&pid(2));
        s.override_ok(pid(3), 10_000, 20_000).unwrap();
    }

    #[test]
    fn lowering_the_limit_blocks_new_payments_but_keeps_old_ones() {
        let mut s = Exposure::new();
        s.accept_ok(pid(1), 15_000, 20_000).unwrap();
        assert_eq!(
            s.override_ok(pid(2), 1, 10_000),
            Err(ExposureError::SliceFull)
        );
        assert_eq!(s.outstanding(), 15_000);
    }

    #[test]
    fn decision_and_ledger_work_together() {
        let p = policy();
        let mut ledger = Exposure::new();
        let mut accepted = 0;
        for n in 0..20u8 {
            let i = Inputs {
                sale_sats: 10_000,
                received_sats: 10_000,
                outstanding_sats: ledger.outstanding(),
                ..ok_light()
            };
            match evaluate(&p, &i) {
                Decision::Instant { .. } => {
                    ledger
                        .accept_ok(pid(n), 10_000, p.light.slice_sats)
                        .unwrap();
                    accepted += 1;
                }
                d => assert_eq!(d, wait(WaitReason::SliceFull)),
            }
        }
        assert_eq!(accepted, 5, "$50 slice holds five $10 sales");
    }

    fn shared_wifi(mode: CheckMode, tor_on: bool) -> Inputs {
        Inputs {
            mode,
            customer_wifi: true,
            tor_on,
            ..ok_light()
        }
    }

    #[test]
    fn node_mode_on_shared_wifi_without_tor_is_judged_as_box_only() {
        let p = policy();
        let i = shared_wifi(CheckMode::NodeAlsoSeenByLight, false);
        assert_eq!(
            evaluate(&p, &i),
            Decision::Instant {
                label: InstantLabel::YourBox
            }
        );
        // $20 fits the node per-sale limit but not the Box one
        let big = Inputs {
            sale_sats: 20_000,
            received_sats: 20_000,
            ..i
        };
        assert_eq!(evaluate(&p, &big), wait(WaitReason::OverSaleLimit));
        // the node slice is $100 and the Box slice $50
        let loaded = Inputs {
            outstanding_sats: 60_000,
            ..i
        };
        assert_eq!(evaluate(&p, &loaded), wait(WaitReason::SliceFull));
        // a Box limit of zero turns it off even though the node limit is set
        let mut off_box = policy();
        off_box.box_only.per_sale_sats = 0;
        assert_eq!(evaluate(&off_box, &i), off(OffReason::Disabled));
    }

    #[test]
    fn node_mode_with_tor_or_private_wifi_keeps_node_label_and_limits() {
        let p = policy();
        for i in [
            shared_wifi(CheckMode::NodeAlsoSeenByLight, true),
            Inputs {
                customer_wifi: false,
                ..shared_wifi(CheckMode::NodeAlsoSeenByLight, false)
            },
        ] {
            assert_eq!(
                evaluate(&p, &i),
                Decision::Instant {
                    label: InstantLabel::YourNode
                }
            );
            let big = Inputs {
                sale_sats: 20_000,
                received_sats: 20_000,
                ..i
            };
            assert!(matches!(evaluate(&p, &big), Decision::Instant { .. }));
            let loaded = Inputs {
                outstanding_sats: 60_000,
                ..i
            };
            assert!(matches!(evaluate(&p, &loaded), Decision::Instant { .. }));
        }
    }

    #[test]
    fn box_mode_is_unaffected_by_the_network() {
        let i = shared_wifi(CheckMode::BoxReportsOnly, false);
        assert_eq!(
            evaluate(&policy(), &i),
            Decision::Instant {
                label: InstantLabel::YourBox
            }
        );
    }

    #[test]
    fn first_cancel_pauses_for_one_hour() {
        let mut t = PauseTracker::new();
        assert_eq!(t.status(0), PauseStatus::Running);
        t.record_cancel(1_000);
        assert_eq!(t.status(1_000), PauseStatus::PausedFor { secs_left: 3_600 });
        assert_eq!(t.status(2_000), PauseStatus::PausedFor { secs_left: 2_600 });
        assert_eq!(t.status(4_599), PauseStatus::PausedFor { secs_left: 1 });
        assert_eq!(t.status(4_600), PauseStatus::Running);
    }

    #[test]
    fn second_cancel_within_24h_pauses_until_owner_reset() {
        let mut t = PauseTracker::new();
        t.record_cancel(0);
        t.record_cancel(10_000); // pause had ended, still inside 24 h
        assert_eq!(t.status(10_000), PauseStatus::UntilOwnerReset);
        assert_eq!(t.status(10_000_000), PauseStatus::UntilOwnerReset);
        t.owner_reset();
        assert_eq!(t.status(10_000_001), PauseStatus::Running);
        // history is cleared: the next cancel is a first cancel again
        t.record_cancel(10_000_100);
        assert_eq!(
            t.status(10_000_100),
            PauseStatus::PausedFor { secs_left: 3_600 }
        );
    }

    #[test]
    fn second_cancel_after_24h_is_a_new_first_cancel() {
        let mut t = PauseTracker::new();
        t.record_cancel(0);
        t.record_cancel(SECOND_CANCEL_WINDOW_SECS + 1);
        assert_eq!(
            t.status(SECOND_CANCEL_WINDOW_SECS + 1),
            PauseStatus::PausedFor { secs_left: 3_600 }
        );
        // exactly 24 h counts as within
        let mut t = PauseTracker::new();
        t.record_cancel(0);
        t.record_cancel(SECOND_CANCEL_WINDOW_SECS);
        assert_eq!(
            t.status(SECOND_CANCEL_WINDOW_SECS),
            PauseStatus::UntilOwnerReset
        );
    }

    #[test]
    fn cancel_while_already_locked_stays_locked_and_clock_going_back_is_safe() {
        let mut t = PauseTracker::new();
        t.record_cancel(5_000);
        t.record_cancel(4_000); // clock went backwards: treated as within the window
        assert_eq!(t.status(0), PauseStatus::UntilOwnerReset);
        t.record_cancel(9_000_000);
        assert_eq!(t.status(9_000_000), PauseStatus::UntilOwnerReset);
    }

    #[test]
    fn pause_status_feeds_the_decision() {
        let mut t = PauseTracker::new();
        t.record_cancel(100);
        let i = Inputs {
            pause: t.status(160),
            ..ok_light()
        };
        assert_eq!(
            evaluate(&policy(), &i),
            off(OffReason::PausedFor { secs_left: 3_540 })
        );
        let i = Inputs {
            pause: t.status(100 + 3_600),
            ..ok_light()
        };
        assert!(matches!(evaluate(&policy(), &i), Decision::Instant { .. }));
    }

    fn out(script: &ScriptBuf, sats: u64) -> TxOut {
        TxOut {
            value: Amount::from_sat(sats),
            script_pubkey: script.clone(),
        }
    }

    fn scripts() -> (ScriptBuf, ScriptBuf, ScriptBuf) {
        let w = crate::testutil::wallet(bitcoin::Network::Testnet);
        (
            w.receive_script(0).unwrap(),
            w.receive_script(1).unwrap(),
            w.change_script(0).unwrap(),
        )
    }

    #[test]
    fn same_payment_with_a_higher_fee_is_a_fee_bump() {
        let (ours, _, change) = scripts();
        let orig = [out(&ours, 10_000), out(&change, 89_000)];
        let repl = [out(&ours, 10_000), out(&change, 88_000)];
        assert_eq!(
            classify_replacement(&orig, &repl, &ours),
            ReplacementKind::FeeBump
        );
    }

    #[test]
    fn replacement_paying_more_or_with_extra_outputs_is_a_fee_bump() {
        let (ours, other, change) = scripts();
        let orig = [out(&ours, 10_000)];
        let repl = [out(&ours, 12_000), out(&other, 1), out(&change, 5)];
        assert_eq!(
            classify_replacement(&orig, &repl, &ours),
            ReplacementKind::FeeBump
        );
    }

    #[test]
    fn payment_split_over_two_outputs_to_our_script_is_a_fee_bump() {
        let (ours, _, _) = scripts();
        let orig = [out(&ours, 10_000)];
        let split = [out(&ours, 6_000), out(&ours, 4_000)];
        assert_eq!(
            classify_replacement(&orig, &split, &ours),
            ReplacementKind::FeeBump
        );
        // and an original split over two outputs, replaced by one
        assert_eq!(
            classify_replacement(&split, &orig, &ours),
            ReplacementKind::FeeBump
        );
    }

    #[test]
    fn replacement_that_pays_less_pays_elsewhere_or_pays_nothing_is_a_cancel() {
        let (ours, other, change) = scripts();
        let orig = [out(&ours, 10_000), out(&change, 50)];
        let less = [out(&ours, 9_999)];
        assert_eq!(
            classify_replacement(&orig, &less, &ours),
            ReplacementKind::Cancel
        );
        let elsewhere = [out(&other, 10_000)];
        assert_eq!(
            classify_replacement(&orig, &elsewhere, &ours),
            ReplacementKind::Cancel
        );
        assert_eq!(
            classify_replacement(&orig, &[], &ours),
            ReplacementKind::Cancel
        );
        // a zero-value output to our script does not make up the amount
        let sibling = [out(&ours.clone(), 0), out(&other, 20_000)];
        assert_eq!(
            classify_replacement(&orig, &sibling, &ours),
            ReplacementKind::Cancel
        );
    }

    #[test]
    fn original_that_paid_us_nothing_is_never_a_fee_bump() {
        let (ours, other, _) = scripts();
        let repl = [out(&ours, 10_000)];
        assert_eq!(
            classify_replacement(&[out(&other, 10_000)], &repl, &ours),
            ReplacementKind::Cancel
        );
        assert_eq!(
            classify_replacement(&[], &repl, &ours),
            ReplacementKind::Cancel
        );
        assert_eq!(
            classify_replacement(&[out(&ours, 0)], &repl, &ours),
            ReplacementKind::Cancel
        );
    }

    #[test]
    fn cancel_leads_to_a_pause() {
        let (ours, other, _) = scripts();
        let orig = [out(&ours, 10_000)];
        let mut t = PauseTracker::new();
        if classify_replacement(&orig, &[out(&other, 10_000)], &ours) == ReplacementKind::Cancel {
            t.record_cancel(500);
        }
        assert!(matches!(t.status(501), PauseStatus::PausedFor { .. }));
    }

    #[test]
    fn exposure_additions_with_a_failing_store_change_nothing() {
        let mut e = ids(&[1, 2]);
        let before = e.to_bytes();
        assert_eq!(
            e.accept_instant(pid(9), 10, LIMIT, persist_fail),
            Err(ExposureWriteError::Persist("disk full"))
        );
        assert_eq!(e.to_bytes(), before);
        assert_eq!(
            e.add_override(pid(9), 10, LIMIT, persist_fail),
            Err(ExposureWriteError::Persist("disk full"))
        );
        assert_eq!(e.to_bytes(), before);
        // Refusals stay refusals and never reach the store.
        assert_eq!(
            e.accept_instant(pid(1), 10, LIMIT, persist_fail),
            Err(ExposureWriteError::Refused(ExposureError::AlreadyCounted))
        );
        assert_eq!(
            e.add_override(pid(9), LIMIT, LIMIT, persist_fail),
            Err(ExposureWriteError::Refused(ExposureError::SliceFull))
        );
        assert_eq!(e.to_bytes(), before);
        // The payment is still addable once the store works.
        e.accept_ok(pid(9), 10, LIMIT).unwrap();
        assert_eq!(e.outstanding(), 3_010);
    }

    #[test]
    fn exposure_additions_hand_persist_the_state_they_then_hold() {
        let mut e = ids(&[1]);
        let mut handed = Vec::new();
        let mut keep = |b: &[u8]| {
            handed = b.to_vec();
            Ok::<(), Infallible>(())
        };
        e.accept_instant(pid(2), 5, LIMIT, &mut keep).unwrap();
        assert_eq!(handed, e.to_bytes());
        assert_eq!(Exposure::from_bytes(&handed).unwrap(), e);
        let mut handed = Vec::new();
        e.add_override(pid(3), 7, LIMIT, |b: &[u8]| {
            handed = b.to_vec();
            Ok::<(), Infallible>(())
        })
        .unwrap();
        assert_eq!(handed, e.to_bytes());
        assert_eq!(Exposure::from_bytes(&handed).unwrap(), e);
    }

    fn ids(ns: &[u8]) -> Exposure {
        let mut e = Exposure::new();
        for &n in ns {
            e.accept_ok(pid(n), u64::from(n) * 1_000, u64::MAX).unwrap();
        }
        e
    }

    #[test]
    fn exposure_snapshot_round_trips_and_behaves_the_same() {
        for e in [Exposure::new(), ids(&[3]), ids(&[9, 1, 5, 200])] {
            let bytes = e.to_bytes();
            assert_eq!(&bytes[..5], b"OBME\x01");
            let mut back = Exposure::from_bytes(&bytes).unwrap();
            assert_eq!(back, e);
            assert_eq!(back.to_bytes(), bytes);
            assert_eq!(back.outstanding(), e.outstanding());
            assert_eq!(
                back.accept_ok(pid(5), 1, u64::MAX).is_err(),
                e.clone().accept_ok(pid(5), 1, u64::MAX).is_err()
            );
        }
    }

    #[test]
    fn exposure_snapshot_decoding_is_strict() {
        crate::testutil::assert_strict_decoding(
            &ids(&[2, 4, 6]).to_bytes(),
            Exposure::from_bytes,
            Exposure::to_bytes,
        );
        crate::testutil::assert_strict_decoding(
            &Exposure::new().to_bytes(),
            Exposure::from_bytes,
            Exposure::to_bytes,
        );
    }

    #[test]
    fn exposure_snapshot_rejects_bad_framing_order_and_overflow() {
        let good = ids(&[1, 2]).to_bytes();
        let mut b = good.clone();
        b[2] = b'X';
        assert_eq!(Exposure::from_bytes(&b), Err(DecodeError::BadMagic));
        for v in [0, 2] {
            let mut b = good.clone();
            b[4] = v;
            assert_eq!(
                Exposure::from_bytes(&b),
                Err(DecodeError::UnsupportedVersion(v))
            );
        }
        // Entries start at 9 and take 40 bytes each.
        let (e0, e1) = (9, 49);
        let mut unsorted = good.clone();
        unsorted.copy_within(e1..e1 + 32, e0);
        unsorted[e1..e1 + 32].fill(1);
        assert!(matches!(
            Exposure::from_bytes(&unsorted),
            Err(DecodeError::NonCanonical(_))
        ));
        let mut repeated = good.clone();
        repeated.copy_within(e0..e0 + 32, e1);
        assert!(matches!(
            Exposure::from_bytes(&repeated),
            Err(DecodeError::NonCanonical(_))
        ));
        // Two amounts that add up past u64::MAX.
        let mut overflow = good.clone();
        overflow[e0 + 32..e0 + 40].fill(0xff);
        overflow[e1 + 32..e1 + 40].copy_from_slice(&1u64.to_le_bytes());
        assert_eq!(
            Exposure::from_bytes(&overflow),
            Err(DecodeError::NonCanonical("outstanding total overflows"))
        );
        // A count with no bytes behind it is refused without allocating.
        let mut huge = good[..9].to_vec();
        huge[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Exposure::from_bytes(&huge), Err(DecodeError::Truncated));
    }

    #[test]
    fn pause_snapshot_resumes_exactly() {
        let mut states = vec![PauseTracker::new()];
        let mut t = PauseTracker::new();
        t.record_cancel(1_000);
        states.push(t.clone());
        t.record_cancel(2_000);
        states.push(t.clone());
        t.owner_reset();
        t.record_cancel(90_000);
        states.push(t);
        for s in states {
            let bytes = s.to_bytes();
            assert_eq!(&bytes[..5], b"OBMP\x01");
            let mut back = PauseTracker::from_bytes(&bytes).unwrap();
            assert_eq!(back, s);
            assert_eq!(back.to_bytes(), bytes);
            for now in [0, 1_000, 4_599, 4_600, 90_000, 1_000_000] {
                assert_eq!(back.status(now), s.status(now));
            }
            let mut original = s.clone();
            back.record_cancel(95_000);
            original.record_cancel(95_000);
            assert_eq!(back, original);
        }
    }

    #[test]
    fn pause_snapshot_decoding_is_strict() {
        let mut t = PauseTracker::new();
        crate::testutil::assert_strict_decoding(
            &t.to_bytes(),
            PauseTracker::from_bytes,
            PauseTracker::to_bytes,
        );
        t.record_cancel(10);
        t.record_cancel(20);
        crate::testutil::assert_strict_decoding(
            &t.to_bytes(),
            PauseTracker::from_bytes,
            PauseTracker::to_bytes,
        );
    }

    #[test]
    fn pause_snapshot_rejects_bad_flags_and_impossible_state() {
        let mut t = PauseTracker::new();
        t.record_cancel(10);
        let good = t.to_bytes();
        // last_cancel flag 5, paused_until flag 14, locked flag 23.
        for at in [5, 14, 23] {
            let mut b = good.clone();
            b[at] = 2;
            assert!(matches!(
                PauseTracker::from_bytes(&b),
                Err(DecodeError::NonCanonical(_))
            ));
        }
        // A pause with no cancel on record.
        let mut b = good.clone();
        b[5] = 0;
        b.drain(6..14);
        assert_eq!(
            PauseTracker::from_bytes(&b),
            Err(DecodeError::NonCanonical("pause state without a cancel"))
        );
        let mut b = PauseTracker::new().to_bytes();
        *b.last_mut().unwrap() = 1;
        assert_eq!(
            PauseTracker::from_bytes(&b),
            Err(DecodeError::NonCanonical("pause state without a cancel"))
        );
        let mut b = good.clone();
        b[4] = 2;
        assert_eq!(
            PauseTracker::from_bytes(&b),
            Err(DecodeError::UnsupportedVersion(2))
        );
    }
}
