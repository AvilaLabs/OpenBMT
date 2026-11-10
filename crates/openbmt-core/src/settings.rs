//! Signed shop settings.
//!
//! Settings are signed by the owner root. A change that only makes things
//! stricter (tightening) needs that one signature. A change that lets more
//! money leave or be accepted at risk, or removes a check or an alert
//! (loosening), also needs the Key app's co-signature (§2.9). A change that
//! cannot be sorted is loosening, and a change that mixes both is loosening.
//!
//! Each money limit is kept in satoshis, which is what counters enforce, and
//! in whole display dollars "from the owner phone", which is only a label.
//! Moving either number up counts as loosening.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::batch::PinnedRoots;
use crate::identity::{tagged_hash, DeviceId, Identity, Sig, TAG_SETTINGS};
use crate::policy::{self, CheckMode};
use crate::wire::{self, DecodeError, Reader};

/// Largest underpayment tolerance `validate` accepts, in basis points (5%).
pub const MAX_TOLERANCE_BPS: u32 = 500;
/// Shortest heartbeat age limit `validate` accepts, in seconds.
pub const MIN_HEARTBEAT_MAX_AGE_SECS: u32 = 60;
/// Longest heartbeat age limit `validate` accepts, in seconds.
pub const MAX_HEARTBEAT_MAX_AGE_SECS: u32 = 3_600;
/// The whole of a budget, in basis points; counter shares may not sum above it.
pub const SHARE_TOTAL_BPS: u32 = 10_000;

/// Most devices a settings value may list. Counters must be devices, so this
/// bounds the counter list too.
pub const MAX_DEVICES: usize = 256;

/// Most Boxes a settings value may name (BX-20).
pub const MAX_BOXES: usize = 8;

/// Largest smallest-sale floor `validate` accepts, in satoshis (all the
/// bitcoin there is).
pub const MAX_MIN_SALE_SATS: u64 = 2_100_000_000_000_000;

/// The first value of the smallest bitcoin sale, in satoshis: about the
/// amount at which spending its receipt at 5 sat/vB costs 2% of it
/// **[to tune]** (CR-75).
pub const DEFAULT_MIN_SALE_SATS: u64 = 26_000;

/// What a device is to the shop, as the signed settings give it (BX-24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceRole {
    /// The owner root's key.
    Owner,
    /// A key with a share (a key in `counters`).
    Counter,
    /// A key in `boxes`.
    Box,
}

/// Require `key` to sort strictly after the previous one in a list.
fn ascending(
    last: &mut Option<[u8; 32]>,
    key: [u8; 32],
    why: &'static str,
) -> Result<(), DecodeError> {
    if last.is_some_and(|prev| prev >= key) {
        return Err(DecodeError::NonCanonical(why));
    }
    *last = Some(key);
    Ok(())
}

/// A money limit: satoshis (enforced) and whole dollars (shown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    /// Enforced amount.
    pub sats: u64,
    /// Display amount, whole dollars.
    pub display_usd: u32,
}

impl Limit {
    /// A limit of `usd` whole dollars at `sats_per_usd` (0 turns it off).
    pub fn from_usd(usd: u32, sats_per_usd: u64) -> Self {
        Self {
            sats: u64::from(usd).saturating_mul(sats_per_usd),
            display_usd: usd,
        }
    }
}

/// The shop's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Must go up with every signed change.
    pub version: u32,
    /// Light check: largest instant sale.
    pub light_per_sale: Limit,
    /// Light check: shop-wide outstanding instant total.
    pub light_budget: Limit,
    /// Own node: largest instant sale.
    pub node_per_sale: Limit,
    /// Own node: shop-wide outstanding instant total.
    pub node_budget: Limit,
    /// Box reports only: largest instant sale.
    pub box_per_sale: Limit,
    /// Box reports only: shop-wide outstanding instant total.
    pub box_budget: Limit,
    /// How far under the sale amount a payment may fall and still count, in
    /// basis points (100 = 1%).
    pub underpayment_tolerance_bps: u32,
    /// Largest single refund.
    pub refund_cap: Limit,
    /// Largest refunds per day.
    pub refund_daily_cap: Limit,
    /// Cashier's daily override cap.
    pub cashier_override_daily_cap: Limit,
    /// Manager's override ceiling.
    pub manager_override_ceiling: Limit,
    /// Confirmations that settle a sale (T0 depth).
    pub min_confirmations: u8,
    /// Confirmations at which a sale is final and a refund may be paid (the
    /// refund depth).
    pub final_confirmations: u8,
    /// Longest age of the owner-phone heartbeat that still allows instant.
    pub heartbeat_max_age_secs: u32,
    /// Tor switch is on.
    pub tor_on: bool,
    /// Customers can join the Wi-Fi the counter is on (or the owner is
    /// unsure).
    pub customer_wifi: bool,
    /// Smallest bitcoin sale the counter opens, in satoshis at the quote's
    /// rate (CR-75). 0 means no floor.
    pub min_sale_sats: u64,
    /// Devices allowed to write records.
    pub devices: BTreeSet<DeviceId>,
    /// Each counter's share of every budget, in basis points (10,000 = the
    /// whole budget).
    pub counters: BTreeMap<DeviceId, u16>,
    /// Devices that are Boxes (BX-20). Each is also on the device list, and
    /// none is also a counter.
    pub boxes: BTreeSet<DeviceId>,
}

impl Settings {
    /// Safe starting values at `sats_per_usd`. Instant limits are the §6
    /// defaults. The refund, override, tolerance and heartbeat values are
    /// placeholders **[to tune]**. Customer Wi-Fi starts as "yes" and Tor as
    /// off, so light-check instant stays off until the owner answers.
    pub fn defaults(sats_per_usd: u64) -> Self {
        let usd = |d| Limit::from_usd(d, sats_per_usd);
        Self {
            version: 1,
            light_per_sale: usd(policy::LIGHT_PER_SALE_USD),
            light_budget: usd(policy::LIGHT_SLICE_USD),
            node_per_sale: usd(policy::NODE_PER_SALE_USD),
            node_budget: usd(policy::NODE_SLICE_USD),
            box_per_sale: usd(policy::BOX_PER_SALE_USD),
            box_budget: usd(policy::BOX_SLICE_USD),
            underpayment_tolerance_bps: 50,
            refund_cap: usd(100),
            refund_daily_cap: usd(300),
            cashier_override_daily_cap: usd(20),
            manager_override_ceiling: usd(100),
            min_confirmations: 1,
            final_confirmations: 6,
            heartbeat_max_age_secs: policy::DEFAULT_HEARTBEAT_MAX_AGE_SECS,
            tor_on: false,
            customer_wifi: true,
            min_sale_sats: DEFAULT_MIN_SALE_SATS,
            devices: BTreeSet::new(),
            counters: BTreeMap::new(),
            boxes: BTreeSet::new(),
        }
    }

    /// The shop-wide outstanding instant total for a check mode.
    pub fn budget(&self, mode: CheckMode) -> &Limit {
        match mode {
            CheckMode::Light => &self.light_budget,
            CheckMode::NodeAlsoSeenByLight => &self.node_budget,
            CheckMode::BoxReportsOnly => &self.box_budget,
        }
    }

    /// `counter`'s slice of the budget for `mode`, in satoshis:
    /// `budget * share / 10_000`, rounded down. Zero when the device has no
    /// entry in `counters`.
    pub fn slice_sats(&self, counter: &DeviceId, mode: CheckMode) -> u64 {
        let Some(share) = self.counters.get(counter) else {
            return 0;
        };
        let sats = u128::from(self.budget(mode).sats) * u128::from(*share) / 10_000;
        // `share` can exceed 10_000 only in unvalidated settings.
        u64::try_from(sats).unwrap_or(u64::MAX)
    }

    /// Check the values make sense. `SignedSettings::verify` runs this after
    /// the owner signature check.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.version == 0 {
            return Err(SettingsError::Invalid("version must be at least 1"));
        }
        if self.devices.len() > MAX_DEVICES {
            return Err(SettingsError::Invalid("more than 256 devices"));
        }
        if self.min_sale_sats > MAX_MIN_SALE_SATS {
            return Err(SettingsError::Invalid(
                "smallest bitcoin sale is more than all the bitcoin there is",
            ));
        }
        if self.underpayment_tolerance_bps > MAX_TOLERANCE_BPS {
            return Err(SettingsError::Invalid(
                "underpayment tolerance is above 500 basis points",
            ));
        }
        if self.min_confirmations < 1 {
            return Err(SettingsError::Invalid(
                "at least one confirmation must settle a sale",
            ));
        }
        if self.final_confirmations < self.min_confirmations {
            return Err(SettingsError::Invalid(
                "final depth must be at least the confirmed depth",
            ));
        }
        if !(MIN_HEARTBEAT_MAX_AGE_SECS..=MAX_HEARTBEAT_MAX_AGE_SECS)
            .contains(&self.heartbeat_max_age_secs)
        {
            return Err(SettingsError::Invalid(
                "heartbeat age limit is outside 60 to 3600 seconds",
            ));
        }
        if self.counters.keys().any(|c| !self.devices.contains(c)) {
            return Err(SettingsError::Invalid(
                "a counter with a share is not on the device list",
            ));
        }
        let total: u32 = self.counters.values().map(|s| u32::from(*s)).sum();
        if total > SHARE_TOTAL_BPS {
            return Err(SettingsError::Invalid(
                "counter shares add up to more than 10000 basis points",
            ));
        }
        if self.boxes.len() > MAX_BOXES {
            return Err(SettingsError::Invalid("BX-20: more than 8 Boxes"));
        }
        if self.boxes.iter().any(|b| !self.devices.contains(b)) {
            return Err(SettingsError::Invalid(
                "BX-20: a Box is not on the device list",
            ));
        }
        if self.boxes.iter().any(|b| self.counters.contains_key(b)) {
            return Err(SettingsError::Invalid(
                "BX-20: a key is both a counter and a Box",
            ));
        }
        Ok(())
    }

    /// What `key` is to the shop (BX-24): the owner root's key is the
    /// owner, a key with a share is a counter, a key in `boxes` is a Box.
    /// Any other key has no role. The owner root is checked first, so it is
    /// the owner even if it is also listed as something else.
    pub fn role_of(&self, owner_root: &DeviceId, key: &DeviceId) -> Option<DeviceRole> {
        if key == owner_root {
            Some(DeviceRole::Owner)
        } else if self.counters.contains_key(key) {
            Some(DeviceRole::Counter)
        } else if self.boxes.contains(key) {
            Some(DeviceRole::Box)
        } else {
            None
        }
    }

    /// The counters, as the set the Key app passes to
    /// [`Approver::check_and_cosign`](crate::batch::Approver::check_and_cosign)
    /// as `approved_counters`: the keys of `counters`.
    ///
    /// A counter may hold a share of 0. It is in this set, so it gets
    /// address batches, but its slice is 0 satoshis, so it never accepts a
    /// payment as instant.
    pub fn counter_set(&self) -> BTreeSet<DeviceId> {
        self.counters.keys().copied().collect()
    }

    /// Canonical bytes: fixed field order, little-endian numbers, devices
    /// and counters sorted by key.
    ///
    /// The two list lengths are written as `u32`. `validate` limits the
    /// device list to [`MAX_DEVICES`] entries, and counters must be devices,
    /// so for validated settings the conversion cannot fail. Calling this
    /// directly on hand-built settings with 2^32 or more entries in a list
    /// panics rather than truncating the length.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            256 + self.devices.len() * 32 + self.counters.len() * 34 + self.boxes.len() * 32,
        );
        out.extend_from_slice(&self.version.to_le_bytes());
        for l in self.limits() {
            out.extend_from_slice(&l.sats.to_le_bytes());
            out.extend_from_slice(&l.display_usd.to_le_bytes());
        }
        out.extend_from_slice(&self.underpayment_tolerance_bps.to_le_bytes());
        out.push(self.min_confirmations);
        out.push(self.final_confirmations);
        out.extend_from_slice(&self.heartbeat_max_age_secs.to_le_bytes());
        out.push(u8::from(self.tor_on));
        out.push(u8::from(self.customer_wifi));
        out.extend_from_slice(&self.min_sale_sats.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(self.devices.len()).to_le_bytes());
        for d in &self.devices {
            out.extend_from_slice(&d.to_bytes());
        }
        out.extend_from_slice(&wire::len_u32(self.counters.len()).to_le_bytes());
        for (d, share) in &self.counters {
            out.extend_from_slice(&d.to_bytes());
            out.extend_from_slice(&share.to_le_bytes());
        }
        out.extend_from_slice(&wire::len_u32(self.boxes.len()).to_le_bytes());
        for b in &self.boxes {
            out.extend_from_slice(&b.to_bytes());
        }
        out
    }

    /// Read the bytes written by [`Settings::encode`], field by field. This
    /// only parses: `validate` is still the caller's to run.
    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let version = r.u32_le()?;
        let mut limits = [Limit {
            sats: 0,
            display_usd: 0,
        }; 10];
        for l in &mut limits {
            l.sats = r.u64_le()?;
            l.display_usd = r.u32_le()?;
        }
        let [light_per_sale, light_budget, node_per_sale, node_budget, box_per_sale, box_budget, refund_cap, refund_daily_cap, cashier_override_daily_cap, manager_override_ceiling] =
            limits;
        let underpayment_tolerance_bps = r.u32_le()?;
        let min_confirmations = r.u8()?;
        let final_confirmations = r.u8()?;
        let heartbeat_max_age_secs = r.u32_le()?;
        let tor_on = r.flag()?;
        let customer_wifi = r.flag()?;
        let min_sale_sats = r.u64_le()?;

        let n = r.count(MAX_DEVICES, 32)?;
        let mut devices = BTreeSet::new();
        let mut last: Option<[u8; 32]> = None;
        for _ in 0..n {
            let key = r.device_id()?;
            ascending(
                &mut last,
                key.to_bytes(),
                "devices are not in ascending order",
            )?;
            devices.insert(key);
        }

        let n = r.count(MAX_DEVICES, 34)?;
        let mut counters = BTreeMap::new();
        let mut last: Option<[u8; 32]> = None;
        for _ in 0..n {
            let key = r.device_id()?;
            ascending(
                &mut last,
                key.to_bytes(),
                "counters are not in ascending order",
            )?;
            counters.insert(key, r.u16_le()?);
        }

        let n = r.count(MAX_BOXES, 32)?;
        let mut boxes = BTreeSet::new();
        let mut last: Option<[u8; 32]> = None;
        for _ in 0..n {
            let key = r.device_id()?;
            ascending(
                &mut last,
                key.to_bytes(),
                "boxes are not in ascending order",
            )?;
            boxes.insert(key);
        }

        Ok(Self {
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
        })
    }

    fn limits(&self) -> [&Limit; 10] {
        [
            &self.light_per_sale,
            &self.light_budget,
            &self.node_per_sale,
            &self.node_budget,
            &self.box_per_sale,
            &self.box_budget,
            &self.refund_cap,
            &self.refund_daily_cap,
            &self.cashier_override_daily_cap,
            &self.manager_override_ceiling,
        ]
    }

    /// Tagged hash (`OpenBMT/settings/v1`) of the canonical bytes.
    pub fn hash(&self) -> [u8; 32] {
        tagged_hash(TAG_SETTINGS, &self.encode())
    }

    /// Sign with an identity (owner root, or Key app root for co-signing).
    pub fn sign(&self, identity: &Identity) -> Sig {
        identity.sign_digest(&self.hash())
    }
}

/// Which way a change moves risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Stricter only: owner signature is enough. Also the answer for "no
    /// field changed".
    Tightening,
    /// Looser or mixed: needs the Key app too.
    Loosening,
}

/// One changed field, for the approval screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldChange {
    /// Field name.
    pub field: &'static str,
    /// Direction of this field alone.
    pub direction: Change,
}

fn dir(loosens: bool) -> Change {
    if loosens {
        Change::Loosening
    } else {
        Change::Tightening
    }
}

fn num<T: PartialOrd>(
    out: &mut Vec<FieldChange>,
    field: &'static str,
    old: T,
    new: T,
    higher_loosens: bool,
) {
    if old != new {
        out.push(FieldChange {
            field,
            direction: dir((new > old) == higher_loosens),
        });
    }
}

/// A flag whose value `loosening_value` is the looser one.
fn flag(
    out: &mut Vec<FieldChange>,
    field: &'static str,
    old: bool,
    new: bool,
    loosening_value: bool,
) {
    if old != new {
        out.push(FieldChange {
            field,
            direction: dir(new == loosening_value),
        });
    }
}

fn limit(out: &mut Vec<FieldChange>, field: &'static str, old: &Limit, new: &Limit) {
    // A limit with a conflicting pair (sats up, dollars down) is mixed.
    num(out, field, old.sats, new.sats, true);
    num(out, field, old.display_usd, new.display_usd, true);
}

/// Every field that differs, with its own direction.
pub fn changed_fields(old: &Settings, new: &Settings) -> Vec<FieldChange> {
    let mut out = Vec::new();
    let o = &mut out;
    limit(
        o,
        "light_per_sale",
        &old.light_per_sale,
        &new.light_per_sale,
    );
    limit(o, "light_budget", &old.light_budget, &new.light_budget);
    limit(o, "node_per_sale", &old.node_per_sale, &new.node_per_sale);
    limit(o, "node_budget", &old.node_budget, &new.node_budget);
    limit(o, "box_per_sale", &old.box_per_sale, &new.box_per_sale);
    limit(o, "box_budget", &old.box_budget, &new.box_budget);
    num(
        o,
        "underpayment_tolerance_bps",
        old.underpayment_tolerance_bps,
        new.underpayment_tolerance_bps,
        true,
    );
    limit(o, "refund_cap", &old.refund_cap, &new.refund_cap);
    limit(
        o,
        "refund_daily_cap",
        &old.refund_daily_cap,
        &new.refund_daily_cap,
    );
    limit(
        o,
        "cashier_override_daily_cap",
        &old.cashier_override_daily_cap,
        &new.cashier_override_daily_cap,
    );
    limit(
        o,
        "manager_override_ceiling",
        &old.manager_override_ceiling,
        &new.manager_override_ceiling,
    );
    num(
        o,
        "min_confirmations",
        old.min_confirmations,
        new.min_confirmations,
        false,
    );
    num(
        o,
        "final_confirmations",
        old.final_confirmations,
        new.final_confirmations,
        false,
    );
    num(
        o,
        "heartbeat_max_age_secs",
        old.heartbeat_max_age_secs,
        new.heartbeat_max_age_secs,
        true,
    );
    // Turning Tor off removes a protection. Saying customers cannot join the
    // Wi-Fi removes the Tor requirement.
    flag(o, "tor_on", old.tor_on, new.tor_on, false);
    flag(
        o,
        "customer_wifi",
        old.customer_wifi,
        new.customer_wifi,
        false,
    );
    // A higher floor turns more sales away; a lower one, or none, lets
    // small receipts in.
    num(
        o,
        "min_sale_sats",
        old.min_sale_sats,
        new.min_sale_sats,
        false,
    );
    if old.devices != new.devices {
        let added = new.devices.difference(&old.devices).next().is_some();
        o.push(FieldChange {
            field: "devices",
            direction: dir(added),
        });
    }
    if old.counters != new.counters {
        // A new counter, or a bigger share for one, lets more through.
        let loosens = new
            .counters
            .iter()
            .any(|(d, share)| old.counters.get(d).is_none_or(|before| share > before));
        o.push(FieldChange {
            field: "counters",
            direction: dir(loosens),
        });
    }
    if old.boxes != new.boxes {
        // A new Box is a new device that reads the shop's orders (BX-20).
        let added = new.boxes.difference(&old.boxes).next().is_some();
        o.push(FieldChange {
            field: "boxes",
            direction: dir(added),
        });
    }
    out
}

/// Sort a change: any loosening field makes it loosening.
pub fn classify_change(old: &Settings, new: &Settings) -> Change {
    if changed_fields(old, new)
        .iter()
        .any(|c| c.direction == Change::Loosening)
    {
        Change::Loosening
    } else {
        Change::Tightening
    }
}

/// Why signed settings were refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// Owner signature missing or wrong.
    OwnerSignatureInvalid,
    /// The change is loosening and carries no Key app signature.
    ApproverRequired,
    /// A Key app signature is present but wrong.
    ApproverSignatureInvalid,
    /// The version did not go up.
    StaleVersion { current: u32, got: u32 },
    /// The values are not acceptable; the text says which rule failed.
    Invalid(&'static str),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerSignatureInvalid => write!(f, "owner signature missing or wrong"),
            Self::ApproverRequired => write!(f, "this change needs the Key app's co-signature"),
            Self::ApproverSignatureInvalid => write!(f, "Key app signature is wrong"),
            Self::Invalid(reason) => write!(f, "settings are not valid: {reason}"),
            Self::StaleVersion { current, got } => {
                write!(
                    f,
                    "settings version {got} does not replace version {current}"
                )
            }
        }
    }
}

impl std::error::Error for SettingsError {}

/// Settings with their signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedSettings {
    /// The settings.
    pub settings: Settings,
    /// Owner root signature over `settings.hash()`.
    pub owner_sig: Sig,
    /// Key app root signature, needed for loosening.
    pub approver_sig: Option<Sig>,
}

impl SignedSettings {
    /// Check against the pinned roots and the settings now in force. First
    /// settings (`current` is `None`) are treated as loosening. Returns the
    /// kind of change on success.
    pub fn verify(
        &self,
        roots: &PinnedRoots,
        current: Option<&Settings>,
    ) -> Result<Change, SettingsError> {
        let h = self.settings.hash();
        if !roots.owner.verify_digest(&h, &self.owner_sig) {
            return Err(SettingsError::OwnerSignatureInvalid);
        }
        self.settings.validate()?;
        if let Some(cur) = current {
            if self.settings.version <= cur.version {
                return Err(SettingsError::StaleVersion {
                    current: cur.version,
                    got: self.settings.version,
                });
            }
        }
        let change = current.map_or(Change::Loosening, |cur| {
            classify_change(cur, &self.settings)
        });
        match (&self.approver_sig, change) {
            (Some(sig), _) if !roots.approver.verify_digest(&h, sig) => {
                Err(SettingsError::ApproverSignatureInvalid)
            }
            (None, Change::Loosening) => Err(SettingsError::ApproverRequired),
            _ => Ok(change),
        }
    }
}

impl SignedSettings {
    /// Wire form: `OBMS`, version 1, the bytes of [`Settings::encode`] (the
    /// bytes that are hashed and signed), the owner signature, a flag byte
    /// (1 if a Key app signature follows, else 0) and, only when the flag is
    /// 1, the Key app signature. Call it on settings that passed `validate`;
    /// `encode` panics when a list holds more than `u32::MAX` entries.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.settings.encode();
        let mut out = wire::start(&wire::MAGIC_SIGNED_SETTINGS, body.len() + 129);
        out.extend_from_slice(&body);
        out.extend_from_slice(&self.owner_sig.0);
        match &self.approver_sig {
            Some(sig) => {
                out.push(1);
                out.extend_from_slice(&sig.0);
            }
            None => out.push(0),
        }
        out
    }

    /// Parse the wire form strictly (see [`crate::wire`]). Lists longer than
    /// [`MAX_DEVICES`] are refused before anything is allocated. This does
    /// not check signatures or run `validate`: call
    /// [`SignedSettings::verify`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &wire::MAGIC_SIGNED_SETTINGS)?;
        let settings = Settings::read(&mut r)?;
        let owner_sig = r.sig()?;
        let approver_sig = if r.flag()? { Some(r.sig()?) } else { None };
        r.finish()?;
        Ok(Self {
            settings,
            owner_sig,
            approver_sig,
        })
    }
}

/// The settings a counter has accepted so far.
#[derive(Debug)]
pub struct SettingsStore {
    roots: PinnedRoots,
    min_version: u32,
    current: Option<SignedSettings>,
}

impl SettingsStore {
    /// An empty store for these pinned roots.
    pub fn new(roots: PinnedRoots) -> Self {
        Self::with_floor(roots, 0)
    }

    /// An empty store that refuses any settings whose version is
    /// `min_version` or lower, even before it holds any. A restored device
    /// passes the newest version it knew of, so an old signed copy cannot be
    /// replayed into an empty store.
    pub fn with_floor(roots: PinnedRoots, min_version: u32) -> Self {
        Self {
            roots,
            min_version,
            current: None,
        }
    }

    /// The settings in force, if any.
    pub fn current(&self) -> Option<&Settings> {
        self.current.as_ref().map(|s| &s.settings)
    }

    /// Verify and take new settings. Nothing changes on failure.
    pub fn apply(&mut self, signed: SignedSettings) -> Result<Change, SettingsError> {
        if signed.settings.version <= self.min_version {
            return Err(SettingsError::StaleVersion {
                current: self.min_version,
                got: signed.settings.version,
            });
        }
        let change = signed.verify(&self.roots, self.current())?;
        self.current = Some(signed);
        Ok(change)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::identity;

    const RATE: u64 = 1_000;

    fn base() -> Settings {
        let mut s = Settings::defaults(RATE);
        s.devices.insert(identity(40).device_id());
        s.tor_on = true;
        s.customer_wifi = true;
        s
    }

    fn next(s: &Settings) -> Settings {
        Settings {
            version: s.version + 1,
            ..s.clone()
        }
    }

    fn roots() -> (Identity, Identity, PinnedRoots) {
        let (o, a) = (identity(20), identity(21));
        let r = PinnedRoots::new(o.device_id(), a.device_id()).unwrap();
        (o, a, r)
    }

    #[test]
    fn defaults_follow_the_policy_table() {
        let s = Settings::defaults(RATE);
        assert_eq!(s.light_per_sale.sats, 10 * RATE);
        assert_eq!(s.light_budget.display_usd, 50);
        assert_eq!(s.node_per_sale.sats, 25 * RATE);
        assert_eq!(s.node_budget.sats, 100 * RATE);
        assert_eq!(s.box_per_sale.display_usd, 10);
        assert_eq!(s.box_budget.display_usd, 50);
    }

    #[test]
    fn unchanged_settings_are_tightening_with_no_fields() {
        let s = base();
        assert!(changed_fields(&s, &s).is_empty());
        assert_eq!(classify_change(&s, &s), Change::Tightening);
    }

    /// Each case: a closure that makes the setting looser.
    #[test]
    fn every_loosening_edit_is_loosening_and_its_reverse_is_tightening() {
        type Edit = fn(&mut Settings);
        let cases: Vec<(&str, Edit)> = vec![
            ("light_per_sale", |s| s.light_per_sale.sats += 1),
            ("light_budget", |s| s.light_budget.sats += 1),
            ("node_per_sale", |s| s.node_per_sale.sats += 1),
            ("node_budget", |s| s.node_budget.sats += 1),
            ("box_per_sale", |s| s.box_per_sale.sats += 1),
            ("box_budget", |s| s.box_budget.sats += 1),
            ("underpayment_tolerance_bps", |s| {
                s.underpayment_tolerance_bps += 1
            }),
            ("refund_cap", |s| s.refund_cap.sats += 1),
            ("refund_daily_cap", |s| s.refund_daily_cap.sats += 1),
            ("cashier_override_daily_cap", |s| {
                s.cashier_override_daily_cap.sats += 1
            }),
            ("manager_override_ceiling", |s| {
                s.manager_override_ceiling.sats += 1
            }),
            ("min_confirmations", |s| s.min_confirmations -= 1),
            ("final_confirmations", |s| s.final_confirmations -= 1),
            ("heartbeat_max_age_secs", |s| s.heartbeat_max_age_secs += 1),
            ("tor_on", |s| s.tor_on = false),
            ("customer_wifi", |s| s.customer_wifi = false),
            ("min_sale_sats", |s| s.min_sale_sats -= 1),
            ("devices", |s| {
                s.devices.insert(identity(41).device_id());
            }),
        ];
        for (name, edit) in cases {
            let old = base();
            let mut loose = old.clone();
            edit(&mut loose);
            let fields = changed_fields(&old, &loose);
            assert_eq!(fields.len(), 1, "{name}");
            assert_eq!(fields[0].field, name);
            assert_eq!(classify_change(&old, &loose), Change::Loosening, "{name}");
            assert_eq!(
                classify_change(&loose, &old),
                Change::Tightening,
                "{name} reverse"
            );
        }
    }

    #[test]
    fn display_dollars_alone_count_as_a_limit_change() {
        let old = base();
        let mut new = old.clone();
        new.light_per_sale.display_usd += 1;
        assert_eq!(classify_change(&old, &new), Change::Loosening);
        assert_eq!(classify_change(&new, &old), Change::Tightening);
        // sats down but dollars up is mixed, so loosening
        let mut mixed = old.clone();
        mixed.light_per_sale.sats -= 1;
        mixed.light_per_sale.display_usd += 1;
        assert_eq!(classify_change(&old, &mixed), Change::Loosening);
    }

    #[test]
    fn mixed_changes_are_loosening() {
        let old = base();
        let mut new = old.clone();
        new.light_budget.sats -= 100; // tightening
        new.refund_cap.sats += 1; // loosening
        assert_eq!(classify_change(&old, &new), Change::Loosening);
        let fields = changed_fields(&old, &new);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].direction, Change::Tightening);
        assert_eq!(fields[1].direction, Change::Loosening);
    }

    #[test]
    fn device_changes_by_direction() {
        let old = base();
        let mut removed = old.clone();
        removed.devices.clear();
        assert_eq!(classify_change(&old, &removed), Change::Tightening);
        let mut swapped = old.clone();
        swapped.devices.clear();
        swapped.devices.insert(identity(42).device_id());
        assert_eq!(classify_change(&old, &swapped), Change::Loosening);
    }

    #[test]
    fn tightening_edits_are_tightening() {
        let old = base();
        let mut new = old.clone();
        new.light_per_sale.sats /= 2;
        new.min_confirmations += 2;
        new.final_confirmations += 2;
        new.tor_on = true;
        new.customer_wifi = true;
        new.heartbeat_max_age_secs -= 1;
        new.min_sale_sats += 1;
        assert_eq!(classify_change(&old, &new), Change::Tightening);
    }

    #[test]
    fn the_first_smallest_sale_is_26000_sats() {
        assert_eq!(Settings::defaults(RATE).min_sale_sats, 26_000);
        assert_eq!(
            Settings::defaults(RATE).min_sale_sats,
            DEFAULT_MIN_SALE_SATS
        );
    }

    #[test]
    fn raising_the_smallest_sale_tightens_and_lowering_or_removing_it_loosens() {
        let old = base();
        let mut up = old.clone();
        up.min_sale_sats = 50_000;
        let fields = changed_fields(&old, &up);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field, "min_sale_sats");
        assert_eq!(classify_change(&old, &up), Change::Tightening);
        assert_eq!(classify_change(&up, &old), Change::Loosening);
        let mut off = old.clone();
        off.min_sale_sats = 0;
        assert_eq!(classify_change(&old, &off), Change::Loosening);
        assert_eq!(classify_change(&off, &old), Change::Tightening);
    }

    #[test]
    fn lowering_the_smallest_sale_needs_the_key_app() {
        let (o, a, roots) = roots();
        let old = base();
        let mut off = next(&old);
        off.min_sale_sats = 0;
        let only_owner = SignedSettings {
            owner_sig: off.sign(&o),
            approver_sig: None,
            settings: off.clone(),
        };
        assert_eq!(
            only_owner.verify(&roots, Some(&old)),
            Err(SettingsError::ApproverRequired)
        );
        let both = SignedSettings {
            approver_sig: Some(off.sign(&a)),
            ..only_owner
        };
        assert_eq!(both.verify(&roots, Some(&old)), Ok(Change::Loosening));
        let mut up = next(&old);
        up.min_sale_sats = 100_000;
        let tight = SignedSettings {
            owner_sig: up.sign(&o),
            approver_sig: None,
            settings: up,
        };
        assert_eq!(tight.verify(&roots, Some(&old)), Ok(Change::Tightening));
    }

    #[test]
    fn the_smallest_sale_is_bounded_by_all_the_bitcoin_there_is() {
        let mut s = base();
        s.min_sale_sats = MAX_MIN_SALE_SATS;
        assert_eq!(s.validate(), Ok(()));
        s.min_sale_sats = 0;
        assert_eq!(s.validate(), Ok(()));
        s.min_sale_sats = MAX_MIN_SALE_SATS + 1;
        assert!(matches!(s.validate(), Err(SettingsError::Invalid(_))));
    }

    #[test]
    fn the_smallest_sale_is_part_of_the_signed_bytes() {
        let s = base();
        let mut t = s.clone();
        t.min_sale_sats += 1;
        assert_ne!(t.hash(), s.hash());
        assert_ne!(t.encode(), s.encode());
    }

    #[test]
    fn tightening_needs_only_the_owner() {
        let (o, _a, roots) = roots();
        let cur = base();
        let mut new = next(&cur);
        new.light_per_sale.sats -= 1;
        let signed = SignedSettings {
            owner_sig: new.sign(&o),
            approver_sig: None,
            settings: new,
        };
        assert_eq!(signed.verify(&roots, Some(&cur)), Ok(Change::Tightening));
    }

    #[test]
    fn loosening_needs_both_roots() {
        let (o, a, roots) = roots();
        let cur = base();
        let mut new = next(&cur);
        new.light_per_sale.sats += 1;
        let owner_only = SignedSettings {
            owner_sig: new.sign(&o),
            approver_sig: None,
            settings: new.clone(),
        };
        assert_eq!(
            owner_only.verify(&roots, Some(&cur)),
            Err(SettingsError::ApproverRequired)
        );
        let both = SignedSettings {
            approver_sig: Some(new.sign(&a)),
            ..owner_only.clone()
        };
        assert_eq!(both.verify(&roots, Some(&cur)), Ok(Change::Loosening));
        // approver alone is not enough either
        let approver_only = SignedSettings {
            owner_sig: new.sign(&a),
            approver_sig: Some(new.sign(&a)),
            settings: new.clone(),
        };
        assert_eq!(
            approver_only.verify(&roots, Some(&cur)),
            Err(SettingsError::OwnerSignatureInvalid)
        );
        // owner signing in both places is not a co-signature
        let owner_twice = SignedSettings {
            approver_sig: Some(new.sign(&o)),
            ..owner_only
        };
        assert_eq!(
            owner_twice.verify(&roots, Some(&cur)),
            Err(SettingsError::ApproverSignatureInvalid)
        );
    }

    #[test]
    fn present_but_wrong_approver_signature_fails_even_for_tightening() {
        let (o, _a, roots) = roots();
        let cur = base();
        let mut new = next(&cur);
        new.light_per_sale.sats -= 1;
        let signed = SignedSettings {
            owner_sig: new.sign(&o),
            approver_sig: Some(Sig([7; 64])),
            settings: new,
        };
        assert_eq!(
            signed.verify(&roots, Some(&cur)),
            Err(SettingsError::ApproverSignatureInvalid)
        );
    }

    #[test]
    fn signatures_do_not_transfer_to_changed_settings() {
        let (o, a, roots) = roots();
        let cur = base();
        let mut tight = next(&cur);
        tight.light_per_sale.sats -= 1;
        let sig = tight.sign(&o);
        // swap in a loosened body under the tightening signature
        let mut loose = next(&cur);
        loose.light_per_sale.sats += 1000;
        let forged = SignedSettings {
            owner_sig: sig,
            approver_sig: Some(loose.sign(&a)),
            settings: loose,
        };
        assert_eq!(
            forged.verify(&roots, Some(&cur)),
            Err(SettingsError::OwnerSignatureInvalid)
        );
    }

    #[test]
    fn version_must_increase() {
        let (o, a, roots) = roots();
        let mut cur = base();
        cur.version = 5;
        for v in [cur.version, cur.version - 1] {
            let mut new = cur.clone();
            new.version = v;
            new.light_per_sale.sats -= 1;
            let s = SignedSettings {
                owner_sig: new.sign(&o),
                approver_sig: Some(new.sign(&a)),
                settings: new,
            };
            assert_eq!(
                s.verify(&roots, Some(&cur)),
                Err(SettingsError::StaleVersion {
                    current: cur.version,
                    got: v
                })
            );
        }
    }

    #[test]
    fn first_settings_need_both_roots() {
        let (o, a, roots) = roots();
        let s = base();
        let one = SignedSettings {
            owner_sig: s.sign(&o),
            approver_sig: None,
            settings: s.clone(),
        };
        assert_eq!(
            one.verify(&roots, None),
            Err(SettingsError::ApproverRequired)
        );
        let two = SignedSettings {
            approver_sig: Some(s.sign(&a)),
            ..one
        };
        assert_eq!(two.verify(&roots, None), Ok(Change::Loosening));
    }

    #[test]
    fn store_applies_in_order_and_rolls_nothing_back_on_failure() {
        let (o, a, roots) = roots();
        let mut store = SettingsStore::new(roots);
        assert!(store.current().is_none());
        let s1 = base();
        store
            .apply(SignedSettings {
                owner_sig: s1.sign(&o),
                approver_sig: Some(s1.sign(&a)),
                settings: s1.clone(),
            })
            .unwrap();
        assert_eq!(store.current(), Some(&s1));
        // tightening applies
        let mut s2 = next(&s1);
        s2.light_per_sale.sats -= 1;
        let signed2 = SignedSettings {
            owner_sig: s2.sign(&o),
            approver_sig: None,
            settings: s2.clone(),
        };
        assert_eq!(store.apply(signed2.clone()), Ok(Change::Tightening));
        // replay of version 2 is stale
        assert!(matches!(
            store.apply(signed2),
            Err(SettingsError::StaleVersion { .. })
        ));
        // a failed loosening leaves s2 in force
        let mut s3 = next(&s2);
        s3.refund_cap.sats += 1;
        let bad = SignedSettings {
            owner_sig: s3.sign(&o),
            approver_sig: None,
            settings: s3,
        };
        assert_eq!(store.apply(bad), Err(SettingsError::ApproverRequired));
        assert_eq!(store.current(), Some(&s2));
    }

    /// `base()` with a counter that is on the device list and holds a share.
    fn with_counter(share: u16) -> (Settings, DeviceId) {
        let mut s = base();
        let c = identity(60).device_id();
        s.devices.insert(c);
        s.counters.insert(c, share);
        (s, c)
    }

    #[test]
    fn default_tolerance_is_half_a_percent_and_defaults_validate() {
        let s = Settings::defaults(RATE);
        assert_eq!(s.underpayment_tolerance_bps, 50);
        assert_eq!(s.validate(), Ok(()));
        assert!(s.counters.is_empty());
        assert_eq!(base().validate(), Ok(()));
    }

    #[test]
    fn budget_fields_are_named_in_changed_fields() {
        let old = base();
        for (name, edit) in [
            (
                "light_budget",
                (|s: &mut Settings| s.light_budget.sats += 1) as fn(&mut Settings),
            ),
            ("node_budget", |s| s.node_budget.sats += 1),
            ("box_budget", |s| s.box_budget.sats += 1),
        ] {
            let mut new = old.clone();
            edit(&mut new);
            assert_eq!(changed_fields(&old, &new)[0].field, name);
        }
    }

    #[test]
    fn slice_arithmetic_rounds_down_in_wide_integers() {
        let (mut s, c) = with_counter(3_333);
        s.light_budget = Limit {
            sats: 5_000_000,
            display_usd: 50,
        };
        assert_eq!(s.slice_sats(&c, CheckMode::Light), 1_666_500);
        s.light_budget.sats = 10;
        assert_eq!(s.slice_sats(&c, CheckMode::Light), 3); // 3.333 rounds down
                                                           // a budget near u64::MAX does not overflow
        s.node_budget.sats = u64::MAX;
        s.counters.insert(c, 10_000);
        assert_eq!(s.slice_sats(&c, CheckMode::NodeAlsoSeenByLight), u64::MAX);
        s.box_budget.sats = 1_000;
        s.counters.insert(c, 2_500);
        assert_eq!(s.slice_sats(&c, CheckMode::BoxReportsOnly), 250);
    }

    #[test]
    fn slice_is_zero_for_a_device_without_a_share() {
        let (s, _) = with_counter(5_000);
        let stranger = identity(61).device_id();
        for mode in [
            CheckMode::Light,
            CheckMode::NodeAlsoSeenByLight,
            CheckMode::BoxReportsOnly,
        ] {
            assert_eq!(s.slice_sats(&stranger, mode), 0);
        }
    }

    #[test]
    fn counters_are_part_of_the_signed_bytes_in_key_order() {
        let (s, c) = with_counter(1_000);
        let mut t = s.clone();
        t.counters.insert(c, 1_001);
        assert_ne!(s.hash(), t.hash());
        let mut u = s.clone();
        u.counters.clear();
        assert_ne!(s.hash(), u.hash());
        // tail of the encoding: count u32 LE, then id | share u16 LE, then
        // the empty Box list (a zero count)
        let enc = s.encode();
        assert_eq!(&enc[enc.len() - 4..], &0u32.to_le_bytes());
        let tail = &enc[enc.len() - 42..enc.len() - 4];
        assert_eq!(&tail[..4], &1u32.to_le_bytes());
        assert_eq!(&tail[4..36], &c.to_bytes());
        assert_eq!(&tail[36..], &1_000u16.to_le_bytes());
        // insertion order does not change the bytes
        let (a, b) = (identity(62).device_id(), identity(63).device_id());
        let mut x = base();
        let mut y = base();
        x.counters.insert(a, 1);
        x.counters.insert(b, 2);
        y.counters.insert(b, 2);
        y.counters.insert(a, 1);
        assert_eq!(x.encode(), y.encode());
    }

    #[test]
    fn counter_change_directions() {
        let (old, c) = with_counter(2_000);
        let other = identity(64).device_id();
        let dir_of = |new: &Settings| {
            let f = changed_fields(&old, new);
            assert_eq!(f.len(), 1);
            assert_eq!(f[0].field, "counters");
            f[0].direction
        };
        let mut raised = old.clone();
        raised.counters.insert(c, 2_001);
        assert_eq!(dir_of(&raised), Change::Loosening);
        let mut lowered = old.clone();
        lowered.counters.insert(c, 1_999);
        assert_eq!(dir_of(&lowered), Change::Tightening);
        let mut removed = old.clone();
        removed.counters.remove(&c);
        assert_eq!(dir_of(&removed), Change::Tightening);
        let mut added = old.clone();
        added.counters.insert(other, 1);
        assert_eq!(dir_of(&added), Change::Loosening);
        // one counter up and another down is mixed, so loosening
        let mut mixed = old.clone();
        mixed.counters.insert(c, 100);
        mixed.counters.insert(other, 500);
        assert_eq!(dir_of(&mixed), Change::Loosening);
    }

    #[test]
    fn raised_share_needs_the_key_app_and_removal_needs_only_the_owner() {
        let (o, a, roots) = roots();
        let (cur, c) = with_counter(2_000);
        let mut up = next(&cur);
        up.counters.insert(c, 3_000);
        let owner_only = SignedSettings {
            owner_sig: up.sign(&o),
            approver_sig: None,
            settings: up.clone(),
        };
        assert_eq!(
            owner_only.verify(&roots, Some(&cur)),
            Err(SettingsError::ApproverRequired)
        );
        let both = SignedSettings {
            approver_sig: Some(up.sign(&a)),
            ..owner_only
        };
        assert_eq!(both.verify(&roots, Some(&cur)), Ok(Change::Loosening));

        let mut gone = next(&cur);
        gone.counters.remove(&c);
        let signed = SignedSettings {
            owner_sig: gone.sign(&o),
            approver_sig: None,
            settings: gone,
        };
        assert_eq!(signed.verify(&roots, Some(&cur)), Ok(Change::Tightening));
    }

    #[test]
    fn validate_rules_each_have_their_own_reason() {
        let reason = |s: &Settings| match s.validate() {
            Err(SettingsError::Invalid(r)) => r,
            other => panic!("expected Invalid, got {other:?}"),
        };
        let mut reasons = Vec::new();

        let mut s = base();
        s.underpayment_tolerance_bps = MAX_TOLERANCE_BPS;
        assert_eq!(s.validate(), Ok(()));
        s.underpayment_tolerance_bps = MAX_TOLERANCE_BPS + 1;
        reasons.push(reason(&s));

        let mut s = base();
        s.min_confirmations = 0;
        reasons.push(reason(&s));

        let mut s = base();
        s.min_confirmations = 3;
        s.final_confirmations = 3;
        assert_eq!(s.validate(), Ok(()));
        s.final_confirmations = 2;
        reasons.push(reason(&s));

        for ok in [60, 3_600] {
            let mut s = base();
            s.heartbeat_max_age_secs = ok;
            assert_eq!(s.validate(), Ok(()));
        }
        for bad in [59, 3_601] {
            let mut s = base();
            s.heartbeat_max_age_secs = bad;
            reasons.push(reason(&s));
        }

        let mut s = base();
        s.counters.insert(identity(65).device_id(), 100);
        reasons.push(reason(&s));

        let (mut s, c) = with_counter(10_000);
        assert_eq!(s.validate(), Ok(()));
        s.counters.insert(c, 9_999);
        s.devices.insert(identity(66).device_id());
        s.counters.insert(identity(66).device_id(), 2);
        reasons.push(reason(&s));

        // the six rules give six different texts (heartbeat appears twice)
        reasons.sort_unstable();
        reasons.dedup();
        assert_eq!(reasons.len(), 6);
    }

    #[test]
    fn invalid_settings_fail_verification_after_the_owner_signature() {
        let (o, a, roots) = roots();
        let mut bad = base();
        bad.counters.insert(identity(67).device_id(), 10_001);
        let signed = SignedSettings {
            owner_sig: bad.sign(&o),
            approver_sig: Some(bad.sign(&a)),
            settings: bad.clone(),
        };
        assert!(matches!(
            signed.verify(&roots, None),
            Err(SettingsError::Invalid(_))
        ));
        // a bad owner signature is reported first
        let forged = SignedSettings {
            owner_sig: bad.sign(&a),
            ..signed
        };
        assert_eq!(
            forged.verify(&roots, None),
            Err(SettingsError::OwnerSignatureInvalid)
        );
        assert_eq!(
            SettingsError::Invalid("x").to_string(),
            "settings are not valid: x"
        );
    }

    #[test]
    fn shares_summing_past_the_whole_budget_are_refused() {
        let (mut s, _) = with_counter(5_000);
        let d = identity(68).device_id();
        s.devices.insert(d);
        s.counters.insert(d, 5_000);
        assert_eq!(s.validate(), Ok(()));
        s.counters.insert(d, 5_001);
        assert!(matches!(s.validate(), Err(SettingsError::Invalid(_))));
    }

    #[test]
    fn counter_must_be_on_the_device_list() {
        let mut s = base();
        s.counters.insert(identity(69).device_id(), 1);
        assert!(matches!(s.validate(), Err(SettingsError::Invalid(_))));
    }

    #[test]
    fn store_floor_refuses_old_versions_even_when_empty() {
        let (o, a, roots) = roots();
        let mut store = SettingsStore::with_floor(roots, 5);
        for v in [4, 5] {
            let mut s = base();
            s.version = v;
            let signed = SignedSettings {
                owner_sig: s.sign(&o),
                approver_sig: Some(s.sign(&a)),
                settings: s,
            };
            assert_eq!(
                store.apply(signed),
                Err(SettingsError::StaleVersion { current: 5, got: v })
            );
            assert!(store.current().is_none());
        }
        let mut s = base();
        s.version = 6;
        let signed = SignedSettings {
            owner_sig: s.sign(&o),
            approver_sig: Some(s.sign(&a)),
            settings: s,
        };
        assert_eq!(store.apply(signed), Ok(Change::Loosening));
    }

    #[test]
    fn new_store_is_a_floor_of_zero() {
        let (o, a, roots) = roots();
        let mut store = SettingsStore::new(roots);
        let mut s = base();
        s.version = 0;
        let signed = SignedSettings {
            owner_sig: s.sign(&o),
            approver_sig: Some(s.sign(&a)),
            settings: s,
        };
        assert_eq!(
            store.apply(signed),
            Err(SettingsError::StaleVersion { current: 0, got: 0 })
        );
    }

    #[test]
    fn validate_rejects_version_zero() {
        let mut s = base();
        s.version = 0;
        assert_eq!(
            s.validate(),
            Err(SettingsError::Invalid("version must be at least 1"))
        );
        // Settings with version 0 do not verify even for a first install.
        let (o, a, roots) = roots();
        let signed = SignedSettings {
            owner_sig: s.sign(&o),
            approver_sig: Some(s.sign(&a)),
            settings: s,
        };
        assert_eq!(
            signed.verify(&roots, None),
            Err(SettingsError::Invalid("version must be at least 1"))
        );
    }

    #[test]
    fn validate_limits_the_device_list() {
        let device = |i: u16| {
            let mut secret = [0u8; 32];
            secret[..2].copy_from_slice(&(i + 1).to_le_bytes());
            Identity::from_secret_bytes(&secret).unwrap().device_id()
        };
        let mut s = base();
        s.devices = (0..MAX_DEVICES as u16).map(device).collect();
        assert_eq!(s.devices.len(), MAX_DEVICES);
        assert_eq!(s.validate(), Ok(()));
        s.devices.insert(device(MAX_DEVICES as u16));
        assert_eq!(
            s.validate(),
            Err(SettingsError::Invalid("more than 256 devices"))
        );
        // Counters must be devices, so the counter list is bounded too.
        s.counters = s.devices.iter().copied().map(|d| (d, 1)).collect();
        assert!(s.validate().is_err());
    }

    #[test]
    fn counter_set_holds_every_counter_including_a_zero_share() {
        let (full, none, stranger) = (identity(40), identity(41), identity(42));
        let mut s = base();
        s.devices = [&full, &none, &stranger]
            .iter()
            .map(|i| i.device_id())
            .collect();
        s.counters = BTreeMap::from([(full.device_id(), 10_000), (none.device_id(), 0)]);
        assert_eq!(s.validate(), Ok(()));
        let set = s.counter_set();
        assert_eq!(set, BTreeSet::from([full.device_id(), none.device_id()]));
        assert!(!set.contains(&stranger.device_id()));
        // The zero-share counter gets addresses but no instant acceptance.
        for mode in [
            CheckMode::Light,
            CheckMode::NodeAlsoSeenByLight,
            CheckMode::BoxReportsOnly,
        ] {
            assert_eq!(s.slice_sats(&none.device_id(), mode), 0);
            assert_eq!(s.slice_sats(&full.device_id(), mode), s.budget(mode).sats);
        }
        assert!(Settings::defaults(RATE).counter_set().is_empty());
    }

    // Offsets in a settings message: magic 0..4, version 4, settings
    // version 5..9, ten limits of 12 bytes 9..129, tolerance 129..133,
    // confirmed depth 133, final depth 134, heartbeat 135..139, tor 139,
    // wifi 140, smallest sale 141..149, device count 149..153, then the
    // devices.
    const TOR_AT: usize = 139;
    const MIN_SALE_AT: usize = 141;
    const DEVICE_COUNT_AT: usize = 149;
    const DEVICES_AT: usize = 153;

    /// Two devices, both counters, one with a share of zero.
    fn wire_settings() -> Settings {
        let mut s = base();
        s.devices = [identity(40), identity(41)]
            .iter()
            .map(Identity::device_id)
            .collect();
        s.counters = s.devices.iter().copied().zip([2_500u16, 0]).collect();
        s
    }

    fn signed_both(s: Settings) -> SignedSettings {
        let (o, a, _) = roots();
        SignedSettings {
            owner_sig: s.sign(&o),
            approver_sig: Some(s.sign(&a)),
            settings: s,
        }
    }

    fn owner_only(s: Settings) -> SignedSettings {
        SignedSettings {
            approver_sig: None,
            ..signed_both(s)
        }
    }

    #[test]
    fn signed_settings_round_trip_and_still_verify() {
        let (_, _, roots) = roots();
        for signed in [signed_both(wire_settings()), owner_only(wire_settings())] {
            let bytes = signed.to_bytes();
            assert_eq!(&bytes[..5], b"OBMS\x01");
            assert_eq!(
                &bytes[5..5 + signed.settings.encode().len()],
                &signed.settings.encode()[..]
            );
            let back = SignedSettings::from_bytes(&bytes).unwrap();
            assert_eq!(back, signed);
            assert_eq!(back.settings.hash(), signed.settings.hash());
            assert_eq!(back.to_bytes(), bytes);
            assert_eq!(
                back.verify(&roots, None).is_ok(),
                signed.verify(&roots, None).is_ok()
            );
        }
        let back = SignedSettings::from_bytes(&signed_both(wire_settings()).to_bytes()).unwrap();
        assert_eq!(back.verify(&roots, None), Ok(Change::Loosening));
    }

    #[test]
    fn signed_settings_decoding_is_strict() {
        for signed in [signed_both(wire_settings()), owner_only(wire_settings())] {
            crate::testutil::assert_strict_decoding(
                &signed.to_bytes(),
                SignedSettings::from_bytes,
                SignedSettings::to_bytes,
            );
        }
    }

    #[test]
    fn many_devices_round_trip_in_ascending_key_order() {
        // BTreeSet order must be the byte order the decoder requires.
        let mut s = base();
        s.devices = (1..=MAX_DEVICES.min(100) as u8)
            .map(|n| identity(n).device_id())
            .collect();
        s.counters = s.devices.iter().copied().map(|d| (d, 1u16)).collect();
        let signed = signed_both(s);
        let back = SignedSettings::from_bytes(&signed.to_bytes()).unwrap();
        assert_eq!(back, signed);
    }

    #[test]
    fn signed_settings_rejects_bad_framing_and_fields() {
        let good = signed_both(wire_settings()).to_bytes();
        let edit = |at: usize, v: u8| {
            let mut b = good.clone();
            b[at] = v;
            SignedSettings::from_bytes(&b).unwrap_err()
        };
        assert_eq!(edit(3, b'X'), DecodeError::BadMagic);
        assert_eq!(edit(4, 0), DecodeError::UnsupportedVersion(0));
        assert_eq!(edit(4, 2), DecodeError::UnsupportedVersion(2));
        assert!(matches!(edit(TOR_AT, 2), DecodeError::NonCanonical(_)));
        assert!(matches!(edit(TOR_AT + 1, 2), DecodeError::NonCanonical(_)));
        // The Key app signature flag is the last byte but 64.
        let flag_at = good.len() - 65;
        assert!(matches!(edit(flag_at, 2), DecodeError::NonCanonical(_)));
        // A device key that is not on the curve.
        let mut b = good.clone();
        b[DEVICES_AT..DEVICES_AT + 32].fill(0);
        assert_eq!(
            SignedSettings::from_bytes(&b).unwrap_err(),
            DecodeError::InvalidKey
        );
    }

    #[test]
    fn the_smallest_sale_sits_after_the_flags_as_eight_little_endian_bytes() {
        let mut s = wire_settings();
        s.min_sale_sats = 0x0102_0304_0506_0708;
        let bytes = signed_both(s.clone()).to_bytes();
        assert_eq!(
            &bytes[MIN_SALE_AT..MIN_SALE_AT + 8],
            &0x0102_0304_0506_0708u64.to_le_bytes()
        );
        let back = SignedSettings::from_bytes(&bytes).unwrap();
        assert_eq!(back.settings.min_sale_sats, 0x0102_0304_0506_0708);
        // Zero is a value like any other and reads back as zero.
        s.min_sale_sats = 0;
        let back = SignedSettings::from_bytes(&signed_both(s).to_bytes()).unwrap();
        assert_eq!(back.settings.min_sale_sats, 0);
        // A message cut inside the field does not decode.
        let cut = &bytes[..MIN_SALE_AT + 4];
        assert!(SignedSettings::from_bytes(cut).is_err());
    }

    #[test]
    fn signed_settings_rejects_unsorted_and_repeated_lists() {
        let good = signed_both(wire_settings()).to_bytes();
        let (d0, d1) = (DEVICES_AT, DEVICES_AT + 32);
        let counters_at = DEVICES_AT + 64 + 4;
        let (c0, c1) = (counters_at, counters_at + 34);

        let swap = |a: usize, b: usize, len: usize| {
            let mut m = good.clone();
            let (x, y) = (good[a..a + len].to_vec(), good[b..b + len].to_vec());
            m[a..a + len].copy_from_slice(&y);
            m[b..b + len].copy_from_slice(&x);
            m
        };
        let repeat = |from: usize, to: usize, len: usize| {
            let mut m = good.clone();
            m.copy_within(from..from + len, to);
            m
        };
        let unsorted_devices = swap(d0, d1, 32);
        let repeated_devices = repeat(d0, d1, 32);
        let unsorted_counters = swap(c0, c1, 34);
        let repeated_counters = repeat(c0, c1, 34);
        for bad in [
            unsorted_devices,
            repeated_devices,
            unsorted_counters,
            repeated_counters,
        ] {
            assert!(matches!(
                SignedSettings::from_bytes(&bad),
                Err(DecodeError::NonCanonical(_))
            ));
        }
        assert!(SignedSettings::from_bytes(&good).is_ok());
    }

    #[test]
    fn signed_settings_list_counts_are_checked_before_allocation() {
        let good = signed_both(wire_settings()).to_bytes();
        let mut b = good[..DEVICES_AT].to_vec();
        for n in [MAX_DEVICES as u32 + 1, u32::MAX] {
            b[DEVICE_COUNT_AT..].copy_from_slice(&n.to_le_bytes());
            assert_eq!(
                SignedSettings::from_bytes(&b).unwrap_err(),
                DecodeError::TooLarge
            );
        }
        // At the limit but without the bytes to back it.
        b[DEVICE_COUNT_AT..].copy_from_slice(&(MAX_DEVICES as u32).to_le_bytes());
        assert_eq!(
            SignedSettings::from_bytes(&b).unwrap_err(),
            DecodeError::Truncated
        );
        // The same rule for the counter list.
        let counters_at = DEVICES_AT + 64;
        let mut b = good[..counters_at + 4].to_vec();
        b[counters_at..].copy_from_slice(&(MAX_DEVICES as u32 + 1).to_le_bytes());
        assert_eq!(
            SignedSettings::from_bytes(&b).unwrap_err(),
            DecodeError::TooLarge
        );
    }

    /// Two devices, one counter and one Box.
    fn with_box() -> (Settings, DeviceId, DeviceId) {
        let (counter, boxed) = (identity(80).device_id(), identity(81).device_id());
        let mut s = base();
        s.devices = [counter, boxed].into_iter().collect();
        s.counters.insert(counter, 2_000);
        s.boxes.insert(boxed);
        (s, counter, boxed)
    }

    #[test]
    fn bx_20_a_box_is_a_device_and_not_a_counter() {
        let (s, counter, boxed) = with_box();
        assert_eq!(s.validate(), Ok(()));
        let mut off_list = s.clone();
        off_list.devices.remove(&boxed);
        assert!(matches!(
            off_list.validate(),
            Err(SettingsError::Invalid(why)) if why.contains("not on the device list")
        ));
        let mut both = s.clone();
        both.boxes.insert(counter);
        assert!(matches!(
            both.validate(),
            Err(SettingsError::Invalid(why)) if why.contains("both a counter and a Box")
        ));
        let mut counted = s;
        counted.counters.insert(boxed, 1);
        assert!(counted.validate().is_err());
    }

    #[test]
    fn bx_20_at_most_eight_boxes() {
        let mut s = base();
        for n in 100..108u8 {
            let d = identity(n).device_id();
            s.devices.insert(d);
            s.boxes.insert(d);
        }
        assert_eq!(s.boxes.len(), MAX_BOXES);
        assert_eq!(s.validate(), Ok(()));
        let ninth = identity(108).device_id();
        s.devices.insert(ninth);
        s.boxes.insert(ninth);
        assert!(matches!(
            s.validate(),
            Err(SettingsError::Invalid(why)) if why.contains("more than 8")
        ));
    }

    #[test]
    fn bx_20_boxes_are_part_of_the_signed_bytes_in_key_order() {
        let (s, _, boxed) = with_box();
        let mut t = s.clone();
        t.boxes.clear();
        assert_ne!(s.encode(), t.encode());
        assert_ne!(s.hash(), t.hash());
        // The Box list is the last field: a count, then the keys.
        let bytes = s.encode();
        assert_eq!(&bytes[bytes.len() - 36..bytes.len() - 32], &[1, 0, 0, 0]);
        assert_eq!(&bytes[bytes.len() - 32..], &boxed.to_bytes());
        let mut x = base();
        let (a, b) = (identity(82).device_id(), identity(83).device_id());
        x.boxes.extend([a, b]);
        let mut y = base();
        y.boxes.extend([b, a]);
        assert_eq!(x.encode(), y.encode());
    }

    #[test]
    fn bx_20_signed_settings_with_boxes_round_trip_and_read_strictly() {
        let (s, _, _) = with_box();
        let signed = signed_both(s);
        assert_eq!(
            SignedSettings::from_bytes(&signed.to_bytes()),
            Ok(signed.clone())
        );
        crate::testutil::assert_strict_decoding(
            &signed.to_bytes(),
            SignedSettings::from_bytes,
            SignedSettings::to_bytes,
        );
        // Two Boxes out of order, and the same one twice, are refused.
        let (a, b) = (identity(82).device_id(), identity(83).device_id());
        let mut two = base();
        two.devices.extend([a, b]);
        two.boxes.extend([a, b]);
        let good = signed_both(two).to_bytes();
        // After the Boxes come the owner signature (64) and the flag and
        // Key app signature (65).
        let first = good.len() - 129 - 64;
        let mut swapped = good.clone();
        let (x, y) = (
            good[first..first + 32].to_vec(),
            good[first + 32..first + 64].to_vec(),
        );
        swapped[first..first + 32].copy_from_slice(&y);
        swapped[first + 32..first + 64].copy_from_slice(&x);
        let mut repeated = good.clone();
        repeated.copy_within(first..first + 32, first + 32);
        for bad in [swapped, repeated] {
            assert!(matches!(
                SignedSettings::from_bytes(&bad),
                Err(DecodeError::NonCanonical(_))
            ));
        }
        assert!(SignedSettings::from_bytes(&good).is_ok());
        // A count above the limit is refused before anything is read.
        let count_at = first - 4;
        let mut big = good.clone();
        big[count_at..count_at + 4].copy_from_slice(&(MAX_BOXES as u32 + 1).to_le_bytes());
        assert_eq!(
            SignedSettings::from_bytes(&big).unwrap_err(),
            DecodeError::TooLarge
        );
    }

    #[test]
    fn bx_20_box_change_directions() {
        let (old, _, boxed) = with_box();
        let other = identity(84).device_id();
        let dir_of = |new: &Settings| {
            let f = changed_fields(&old, new);
            assert_eq!(f.len(), 1);
            assert_eq!(f[0].field, "boxes");
            f[0].direction
        };
        let mut added = old.clone();
        added.boxes.insert(other);
        assert_eq!(dir_of(&added), Change::Loosening);
        let mut removed = old.clone();
        removed.boxes.remove(&boxed);
        assert_eq!(dir_of(&removed), Change::Tightening);
        // One out and another in is mixed, so loosening.
        let mut swapped = old.clone();
        swapped.boxes.remove(&boxed);
        swapped.boxes.insert(other);
        assert_eq!(dir_of(&swapped), Change::Loosening);
        assert_eq!(classify_change(&old, &added), Change::Loosening);
        assert_eq!(classify_change(&old, &removed), Change::Tightening);
    }

    #[test]
    fn bx_20_adding_a_box_needs_the_key_app_and_removing_one_does_not() {
        let (o, a, roots) = roots();
        let (cur, _, boxed) = with_box();
        let mut up = next(&cur);
        let other = identity(85).device_id();
        up.devices.insert(other);
        up.boxes.insert(other);
        let owner_only = SignedSettings {
            owner_sig: up.sign(&o),
            approver_sig: None,
            settings: up.clone(),
        };
        assert_eq!(
            owner_only.verify(&roots, Some(&cur)),
            Err(SettingsError::ApproverRequired)
        );
        let both = SignedSettings {
            approver_sig: Some(up.sign(&a)),
            ..owner_only
        };
        assert_eq!(both.verify(&roots, Some(&cur)), Ok(Change::Loosening));
        let mut gone = next(&cur);
        gone.boxes.remove(&boxed);
        let signed = SignedSettings {
            owner_sig: gone.sign(&o),
            approver_sig: None,
            settings: gone,
        };
        assert_eq!(signed.verify(&roots, Some(&cur)), Ok(Change::Tightening));
    }

    #[test]
    fn bx_24_role_of_gives_owner_counter_box_or_nothing() {
        let (s, counter, boxed) = with_box();
        let owner = identity(86).device_id();
        let stranger = identity(87).device_id();
        assert_eq!(s.role_of(&owner, &owner), Some(DeviceRole::Owner));
        assert_eq!(s.role_of(&owner, &counter), Some(DeviceRole::Counter));
        assert_eq!(s.role_of(&owner, &boxed), Some(DeviceRole::Box));
        assert_eq!(s.role_of(&owner, &stranger), None);
        // A device on the list with neither a share nor a Box entry has no role.
        let mut plain = s.clone();
        plain.devices.insert(stranger);
        assert_eq!(plain.role_of(&owner, &stranger), None);
        // The owner root wins over any other entry.
        assert_eq!(s.role_of(&counter, &counter), Some(DeviceRole::Owner));
    }
}
