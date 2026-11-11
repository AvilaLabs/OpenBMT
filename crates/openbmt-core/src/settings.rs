//! Signed shop settings.
//!
//! Settings are signed by the owner root. A change that only makes things
//! stricter (tightening) needs that one signature. A change that lets more
//! money leave or be accepted at risk, or removes a check or an alert
//! (loosening), also needs the Key app's co-signature (§2.9). A change that
//! cannot be sorted is loosening, and a change that mixes both is loosening.
//!
//! Each money limit is kept in satoshis, which is what counters enforce, and
//! in whole display units of the shop's currency "from the owner phone",
//! which is only a label. Moving either number up counts as loosening. The
//! shop's currency is part of the settings (CR-193); changing it loosens and
//! every limit is given again in the new one (CR-195).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::batch::PinnedRoots;
use crate::currency::ShopCurrency;
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

/// The first value of the smallest bitcoin sale, in satoshis: 0, no floor
/// (CR-75). The owner sets one when small receipts cost too much to spend.
pub const DEFAULT_MIN_SALE_SATS: u64 = 0;

/// Most suggested tips a settings value may carry (CR-187).
pub const MAX_TIP_PERCENTS: usize = 3;
/// Smallest suggested tip, in whole percent (CR-187).
pub const MIN_TIP_PERCENT: u8 = 1;
/// Largest suggested tip, in whole percent (CR-187).
pub const MAX_TIP_PERCENT: u8 = 50;
/// The first suggested tips, in whole percent (CR-187).
pub const DEFAULT_TIP_PERCENTS: [u8; 3] = [10, 15, 20];

/// The version byte of a settings message that carries the extension block
/// (CR-187, CR-193): the shop's currency and then the tips. A dollar shop with
/// tips off and the first percents is written as version 1, byte for byte what
/// earlier builds wrote.
const EXTENSION_VERSION: u8 = 2;

/// Length of a Lightning node's public key: compressed, as in BOLT 11 and in
/// the node's own `getinfo` (LN-03).
pub const LIGHTNING_NODE_BYTES: usize = 33;

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

/// A money limit: satoshis (enforced) and whole units of the shop's
/// currency (shown). For Bitcoin Standard the two are the same number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    /// Enforced amount.
    pub sats: u64,
    /// Display amount, whole units of the shop's currency (whole dollars
    /// for a dollar shop; the name is kept from when that was all there
    /// was).
    pub display_usd: u32,
}

impl Limit {
    /// A limit of `usd` whole units of the shop's currency at `sats_per_usd`
    /// satoshis to a unit (0 turns it off).
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
    /// The public key of the shop's own Lightning node, if it has one
    /// (LN-03). The node decides where Lightning payments go, so the key
    /// that every invoice must be signed by is part of the signed settings
    /// and not of a counter's own configuration.
    pub lightning_node: Option<[u8; LIGHTNING_NODE_BYTES]>,
    /// The public key of the watch key the owner phone's background watcher
    /// signs heartbeats with (CR-50, CR-55), if the owner turned that on. A
    /// counter takes a heartbeat signed by it, under its own tag, as it
    /// takes one signed by the owner root. It is never a device of the
    /// shop: it is not on the device list, is not a counter or a Box, and
    /// signs nothing but heartbeats.
    pub watch_key: Option<DeviceId>,
    /// The counters ask the customer for a tip (CR-187). Off at first.
    pub tips_on: bool,
    /// The tips the counters suggest, in whole percent, 0 to
    /// [`MAX_TIP_PERCENTS`] of them, each from [`MIN_TIP_PERCENT`] to
    /// [`MAX_TIP_PERCENT`] (CR-187).
    pub tip_percents: Vec<u8>,
    /// What the shop prices its sales in, and what every limit above is in
    /// whole units of (CR-193).
    pub currency: ShopCurrency,
}

impl Settings {
    /// Safe starting values in dollars at `sats_per_usd`. Instant limits are the §6
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
            lightning_node: None,
            watch_key: None,
            tips_on: false,
            tip_percents: DEFAULT_TIP_PERCENTS.to_vec(),
            currency: ShopCurrency::Usd,
        }
    }

    /// Whether the extension block is part of the canonical bytes (CR-187,
    /// CR-193): it is when the currency is not USD, tips are on or the
    /// percents are not the first ones.
    fn has_extension_block(&self) -> bool {
        self.currency != ShopCurrency::Usd
            || self.tips_on
            || self.tip_percents != DEFAULT_TIP_PERCENTS
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
        if let Some(key) = &self.lightning_node {
            if bitcoin::secp256k1::PublicKey::from_slice(key).is_err() {
                return Err(SettingsError::Invalid(
                    "LN-03: the Lightning node key is not a compressed public key",
                ));
            }
        }
        if self.tip_percents.len() > MAX_TIP_PERCENTS
            || self
                .tip_percents
                .iter()
                .any(|p| !(MIN_TIP_PERCENT..=MAX_TIP_PERCENT).contains(p))
        {
            return Err(SettingsError::Invalid(
                "CR-187: a suggested tip is not a whole percent from 1 to 50, or there are more than three",
            ));
        }
        if self.currency.is_sats()
            && self
                .limits()
                .iter()
                .any(|l| l.sats != u64::from(l.display_usd))
        {
            return Err(SettingsError::Invalid(
                "CR-194: a limit in Bitcoin Standard is not its own satoshis",
            ));
        }
        if let Some(key) = &self.watch_key {
            if self.devices.contains(key)
                || self.counters.contains_key(key)
                || self.boxes.contains(key)
            {
                return Err(SettingsError::Invalid(
                    "CR-50: the watch key is also a device of the shop",
                ));
            }
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
    /// and counters sorted by key, and last a flag byte and, when it is 1,
    /// the 33 bytes of the Lightning node's key (LN-03).
    ///
    /// The two list lengths are written as `u32`. `validate` limits the
    /// device list to [`MAX_DEVICES`] entries, and counters must be devices,
    /// so for validated settings the conversion cannot fail. Calling this
    /// directly on hand-built settings with 2^32 or more entries in a list
    /// panics rather than truncating the length.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            329 + self.devices.len() * 32 + self.counters.len() * 34 + self.boxes.len() * 32,
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
        match &self.lightning_node {
            Some(key) => {
                out.push(1);
                out.extend_from_slice(key);
            }
            None => out.push(0),
        }
        match &self.watch_key {
            Some(key) => {
                out.push(1);
                out.extend_from_slice(&key.to_bytes());
            }
            None => out.push(0),
        }
        // CR-193, CR-187: the extension block is the currency byte and then
        // the tips block. It is left out, and the message is version 1, when
        // the shop is a dollar shop with tips off and the first percents.
        if self.has_extension_block() {
            out.push(self.currency.to_byte());
            out.push(u8::from(self.tips_on));
            out.push(u8::try_from(self.tip_percents.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(&self.tip_percents);
        }
        out
    }

    /// Read the bytes written by [`Settings::encode`], field by field. This
    /// only parses: `validate` is still the caller's to run. `version_byte`
    /// is the version of the frame: from 2 on the extension block follows
    /// the watch key, and a version 1 message reads as US dollars with tips
    /// off and the first percents (CR-187, CR-193).
    fn read(r: &mut Reader<'_>, version_byte: u8) -> Result<Self, DecodeError> {
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

        let lightning_node = if r.flag()? {
            Some(r.array::<LIGHTNING_NODE_BYTES>()?)
        } else {
            None
        };
        let watch_key = if r.flag()? {
            Some(r.device_id()?)
        } else {
            None
        };
        let (currency, tips_on, tip_percents) = if version_byte >= EXTENSION_VERSION {
            let currency = ShopCurrency::from_byte(r.u8()?).ok_or(DecodeError::NonCanonical(
                "the currency byte is not a choice",
            ))?;
            let on = r.flag()?;
            let n = usize::from(r.u8()?);
            if n > MAX_TIP_PERCENTS {
                return Err(DecodeError::NonCanonical("more than three suggested tips"));
            }
            let percents = r.take(n)?.to_vec();
            if currency == ShopCurrency::Usd && !on && percents == DEFAULT_TIP_PERCENTS {
                return Err(DecodeError::NonCanonical(
                    "an extension block holds nothing but the first values",
                ));
            }
            (currency, on, percents)
        } else {
            (ShopCurrency::Usd, false, DEFAULT_TIP_PERCENTS.to_vec())
        };

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
            lightning_node,
            watch_key,
            tips_on,
            tip_percents,
            currency,
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
    // CR-195: another currency means every limit is another number, which
    // cannot be sorted, so it loosens.
    if old.currency != new.currency {
        o.push(FieldChange {
            field: "currency",
            direction: Change::Loosening,
        });
    }
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
    // The node decides where Lightning payments go (LN-03). Naming a node, or
    // another one, sends money somewhere new, like a changed destination, so
    // it loosens; taking the node away only stops Lightning payments.
    if old.lightning_node != new.lightning_node {
        o.push(FieldChange {
            field: "lightning_node",
            direction: dir(new.lightning_node.is_some()),
        });
    }
    // Naming a watch key, or another one, lets instant sales go on on the
    // word of a key that is not the owner root, so it loosens; taking the key
    // away only stops that.
    if old.watch_key != new.watch_key {
        o.push(FieldChange {
            field: "watch_key",
            direction: dir(new.watch_key.is_some()),
        });
    }
    // Neither asking for a tip nor changing the suggested percents changes
    // what a counter accepts or how much is put at risk (CR-187): the tip is
    // part of the sale's total and every check is made on the total. Both
    // directions tighten, so the owner phone signs alone.
    if old.tips_on != new.tips_on {
        o.push(FieldChange {
            field: "tips_on",
            direction: Change::Tightening,
        });
    }
    if old.tip_percents != new.tip_percents {
        o.push(FieldChange {
            field: "tip_percents",
            direction: Change::Tightening,
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

/// CR-195: the fields of a limit that is on and the same, in satoshis and in
/// whole units, in settings of another currency than `old`. A change of
/// currency gives every limit again; one that carries a value over from the
/// old currency is not made.
pub fn carried_limit_fields(old: &Settings, new: &Settings) -> Vec<&'static str> {
    if old.currency == new.currency {
        return Vec::new();
    }
    const NAMES: [&str; 10] = [
        "light_per_sale",
        "light_budget",
        "node_per_sale",
        "node_budget",
        "box_per_sale",
        "box_budget",
        "refund_cap",
        "refund_daily_cap",
        "cashier_override_daily_cap",
        "manager_override_ceiling",
    ];
    old.limits()
        .iter()
        .zip(new.limits())
        .zip(NAMES)
        .filter(|((before, after), _)| before.display_usd != 0 && *before == after)
        .map(|(_, name)| name)
        .collect()
}

fn carried_limits(old: &Settings, new: &Settings) -> Result<(), SettingsError> {
    if carried_limit_fields(old, new).is_empty() {
        Ok(())
    } else {
        Err(SettingsError::Invalid(
            "CR-195: a change of currency keeps a limit of the old currency",
        ))
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
        if let Some(cur) = current {
            carried_limits(cur, &self.settings)?;
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
    /// Check the signatures that are there and the values, without a change
    /// from other settings: the owner's signature, the Key app's if present,
    /// and `validate`. For settings kept after they were checked as a change
    /// (a counter's settings file).
    pub fn verify_signatures(&self, roots: &PinnedRoots) -> Result<(), SettingsError> {
        let h = self.settings.hash();
        if !roots.owner.verify_digest(&h, &self.owner_sig) {
            return Err(SettingsError::OwnerSignatureInvalid);
        }
        if let Some(sig) = &self.approver_sig {
            if !roots.approver.verify_digest(&h, sig) {
                return Err(SettingsError::ApproverSignatureInvalid);
            }
        }
        self.settings.validate()
    }
}

impl SignedSettings {
    /// Wire form: `OBMS`, version 1 (2 when the settings carry the extension
    /// block, the currency and the tips, CR-187 and CR-193), the bytes of
    /// [`Settings::encode`] (the
    /// bytes that are hashed and signed), the owner signature, a flag byte
    /// (1 if a Key app signature follows, else 0) and, only when the flag is
    /// 1, the Key app signature. Call it on settings that passed `validate`;
    /// `encode` panics when a list holds more than `u32::MAX` entries.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = self.settings.encode();
        let mut out = wire::start(&wire::MAGIC_SIGNED_SETTINGS, body.len() + 129);
        if self.settings.has_extension_block() {
            if let Some(byte) = out.get_mut(4) {
                *byte = EXTENSION_VERSION;
            }
        }
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
        let (mut r, version) =
            Reader::framed_up_to(bytes, &wire::MAGIC_SIGNED_SETTINGS, EXTENSION_VERSION)?;
        let settings = Settings::read(&mut r, version)?;
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

/// Length of the body of a `Setting` record: the version, the kind of change
/// and the hash of the settings.
pub const SETTING_NOTE_LEN: usize = 37;

/// What a counter writes in a `Setting` record when it takes settings
/// (`docs/COUNTER-RULES.md`, CR-163): the version, whether the change
/// loosened or tightened, and the hash of the settings, so that the owner's
/// copy of the counter's chain shows which settings the counter holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingNote {
    /// The version of the settings taken.
    pub version: u32,
    /// How the change moved risk. The first settings a counter holds, and a
    /// change that cannot be sorted, are loosening.
    pub change: Change,
    /// [`Settings::hash`] of the settings taken.
    pub hash: [u8; 32],
}

impl SettingNote {
    /// The record body: the version (4 bytes little endian), 0 for a
    /// tightening or 1 for a loosening, and the 32-byte hash.
    pub fn to_body(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SETTING_NOTE_LEN);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.push(match self.change {
            Change::Tightening => 0,
            Change::Loosening => 1,
        });
        out.extend_from_slice(&self.hash);
        out
    }

    /// Parse a record body strictly: exactly [`SETTING_NOTE_LEN`] bytes and a
    /// kind of change that is 0 or 1.
    pub fn from_body(body: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(body);
        let version = r.u32_le()?;
        let change = match r.u8()? {
            0 => Change::Tightening,
            1 => Change::Loosening,
            _ => {
                return Err(DecodeError::NonCanonical(
                    "the kind of change is not 0 or 1",
                ))
            }
        };
        let hash = r.array()?;
        r.finish()?;
        Ok(Self {
            version,
            change,
            hash,
        })
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
        // A floor is set, so that lowering it can be tested.
        s.min_sale_sats = 26_000;
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
    fn the_first_smallest_sale_is_no_floor() {
        assert_eq!(Settings::defaults(RATE).min_sale_sats, 0);
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
        // the empty Box list (a zero count) and the flags of no Lightning node
        // and no watch key
        let enc = s.encode();
        let end = enc.len() - 2;
        assert_eq!(enc[end], 0);
        assert_eq!(&enc[end - 4..end], &0u32.to_le_bytes());
        let tail = &enc[end - 42..end - 4];
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
        assert_eq!(edit(4, 3), DecodeError::UnsupportedVersion(3));
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
        // The Box list comes last but for the flags of the Lightning node and
        // the watch key: a count, then the keys.
        let bytes = s.encode();
        let end = bytes.len() - 2;
        assert_eq!(&bytes[end - 36..end - 32], &[1, 0, 0, 0]);
        assert_eq!(&bytes[end - 32..end], &boxed.to_bytes());
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
        // After the Boxes come the flags of the Lightning node and the watch
        // key (2), the owner signature (64) and the flag and Key app
        // signature (65).
        let first = good.len() - 129 - 2 - 64;
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

    /// A valid compressed key for a test node, from a one-byte seed.
    fn node_key(seed: u8) -> [u8; LIGHTNING_NODE_BYTES] {
        let secp = bitcoin::secp256k1::Secp256k1::new();
        let sk = bitcoin::secp256k1::SecretKey::from_slice(&[seed; 32]).expect("a key");
        bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk).serialize()
    }

    #[test]
    fn cr_185_the_first_settings_name_no_watch_key() {
        assert_eq!(Settings::defaults(RATE).watch_key, None);
    }

    #[test]
    fn cr_185_naming_or_changing_the_watch_key_loosens_and_removing_it_tightens() {
        let old = base();
        let mut named = old.clone();
        named.watch_key = Some(identity(90).device_id());
        let fields = changed_fields(&old, &named);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field, "watch_key");
        assert_eq!(classify_change(&old, &named), Change::Loosening);
        assert_eq!(classify_change(&named, &old), Change::Tightening);
        let mut other = named.clone();
        other.watch_key = Some(identity(91).device_id());
        assert_eq!(classify_change(&named, &other), Change::Loosening);
        assert_eq!(classify_change(&named, &named), Change::Tightening);
    }

    #[test]
    fn cr_185_naming_the_watch_key_needs_the_key_app_and_removing_it_does_not() {
        let (o, a, roots) = roots();
        let old = base();
        let mut named = next(&old);
        named.watch_key = Some(identity(90).device_id());
        let only_owner = SignedSettings {
            owner_sig: named.sign(&o),
            approver_sig: None,
            settings: named.clone(),
        };
        assert_eq!(
            only_owner.verify(&roots, Some(&old)),
            Err(SettingsError::ApproverRequired)
        );
        let both = SignedSettings {
            approver_sig: Some(named.sign(&a)),
            ..only_owner
        };
        assert_eq!(both.verify(&roots, Some(&old)), Ok(Change::Loosening));
        let mut gone = next(&named);
        gone.watch_key = None;
        let removed = SignedSettings {
            owner_sig: gone.sign(&o),
            approver_sig: None,
            settings: gone,
        };
        assert_eq!(removed.verify(&roots, Some(&named)), Ok(Change::Tightening));
    }

    #[test]
    fn cr_185_the_watch_key_is_signed_reads_back_and_is_never_a_device() {
        let s = base();
        let mut t = s.clone();
        t.watch_key = Some(identity(90).device_id());
        assert_ne!(t.hash(), s.hash());
        let (o, _a, _roots) = roots();
        let signed = SignedSettings {
            owner_sig: t.sign(&o),
            approver_sig: None,
            settings: t.clone(),
        };
        let bytes = signed.to_bytes();
        let back = SignedSettings::from_bytes(&bytes).expect("reads back");
        assert_eq!(back, signed);
        assert_eq!(back.to_bytes(), bytes);
        let none = SignedSettings {
            settings: s,
            ..signed
        };
        assert_eq!(bytes.len(), none.to_bytes().len() + 32);
        // A watch key that is also a device, a counter or a Box is refused.
        let key = identity(90).device_id();
        let mut as_device = t.clone();
        as_device.devices.insert(key);
        assert!(as_device.validate().is_err());
        let mut as_box = t.clone();
        as_box.devices.insert(key);
        as_box.boxes.insert(key);
        assert!(as_box.validate().is_err());
        assert!(t.validate().is_ok());
    }

    #[test]
    fn cr_185_the_watch_key_flag_is_strict() {
        let mut t = base();
        t.watch_key = Some(identity(90).device_id());
        let (o, _a, _roots) = roots();
        let signed = SignedSettings {
            owner_sig: t.sign(&o),
            approver_sig: None,
            settings: t,
        };
        let bytes = signed.to_bytes();
        // The flag byte sits before the key, the owner signature and the
        // approver flag byte.
        let at = bytes.len() - 1 - 64 - 32 - 1;
        let mut bad = bytes.clone();
        bad[at] = 2;
        assert_eq!(
            SignedSettings::from_bytes(&bad),
            Err(DecodeError::NonCanonical("flag is not 0 or 1"))
        );
        let mut cut = bytes.clone();
        cut.truncate(at + 1 + 31);
        assert!(SignedSettings::from_bytes(&cut).is_err());
    }

    #[test]
    fn ln_03_the_first_settings_name_no_lightning_node() {
        assert_eq!(Settings::defaults(RATE).lightning_node, None);
    }

    #[test]
    fn ln_03_naming_or_changing_the_node_loosens_and_removing_it_tightens() {
        let old = base();
        let mut named = old.clone();
        named.lightning_node = Some(node_key(7));
        let fields = changed_fields(&old, &named);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].field, "lightning_node");
        assert_eq!(classify_change(&old, &named), Change::Loosening);
        assert_eq!(classify_change(&named, &old), Change::Tightening);
        let mut other = named.clone();
        other.lightning_node = Some(node_key(8));
        assert_eq!(classify_change(&named, &other), Change::Loosening);
        assert_eq!(classify_change(&named, &named), Change::Tightening);
    }

    #[test]
    fn ln_03_naming_the_node_needs_the_key_app() {
        let (o, a, roots) = roots();
        let old = base();
        let mut named = next(&old);
        named.lightning_node = Some(node_key(7));
        let only_owner = SignedSettings {
            owner_sig: named.sign(&o),
            approver_sig: None,
            settings: named.clone(),
        };
        assert_eq!(
            only_owner.verify(&roots, Some(&old)),
            Err(SettingsError::ApproverRequired)
        );
        let both = SignedSettings {
            approver_sig: Some(named.sign(&a)),
            ..only_owner
        };
        assert_eq!(both.verify(&roots, Some(&old)), Ok(Change::Loosening));
        // Taking the node away needs the owner alone.
        let mut gone = next(&named);
        gone.lightning_node = None;
        let removed = SignedSettings {
            owner_sig: gone.sign(&o),
            approver_sig: None,
            settings: gone,
        };
        assert_eq!(removed.verify(&roots, Some(&named)), Ok(Change::Tightening));
    }

    #[test]
    fn ln_03_the_node_key_is_part_of_the_signed_bytes_and_reads_back() {
        let s = base();
        let mut t = s.clone();
        t.lightning_node = Some(node_key(7));
        assert_ne!(t.hash(), s.hash());
        let (o, _a, _roots) = roots();
        let signed = SignedSettings {
            owner_sig: t.sign(&o),
            approver_sig: None,
            settings: t,
        };
        let bytes = signed.to_bytes();
        let back = SignedSettings::from_bytes(&bytes).expect("reads back");
        assert_eq!(back, signed);
        assert_eq!(back.to_bytes(), bytes);
        // The same settings with no node are 33 bytes shorter.
        let none = SignedSettings {
            settings: s,
            ..signed
        };
        assert_eq!(bytes.len(), none.to_bytes().len() + LIGHTNING_NODE_BYTES);
    }

    #[test]
    fn ln_03_the_node_key_flag_and_length_are_strict() {
        let mut t = base();
        t.lightning_node = Some(node_key(7));
        let (o, _a, _roots) = roots();
        let signed = SignedSettings {
            owner_sig: t.sign(&o),
            approver_sig: None,
            settings: t,
        };
        let bytes = signed.to_bytes();
        // The flag byte sits before its 33 bytes, the watch key flag, the
        // owner signature and the approver flag byte.
        let at = bytes.len() - 1 - 64 - 1 - LIGHTNING_NODE_BYTES - 1;
        let mut bad = bytes.clone();
        bad[at] = 2;
        assert_eq!(
            SignedSettings::from_bytes(&bad),
            Err(DecodeError::NonCanonical("flag is not 0 or 1"))
        );
        // A cut-off key is refused, not read short.
        assert!(SignedSettings::from_bytes(&bytes[..bytes.len() - 71]).is_err());
    }

    #[test]
    fn ln_03_a_node_key_that_is_not_a_compressed_point_is_refused() {
        let mut s = base();
        s.lightning_node = Some(node_key(7));
        assert_eq!(s.validate(), Ok(()));
        let mut bad_prefix = node_key(7);
        bad_prefix[0] = 4;
        let mut off_field = [0xffu8; LIGHTNING_NODE_BYTES];
        off_field[0] = 3;
        for key in [bad_prefix, off_field, [0u8; LIGHTNING_NODE_BYTES]] {
            let mut s = base();
            s.lightning_node = Some(key);
            assert!(
                matches!(s.validate(), Err(SettingsError::Invalid(_))),
                "{key:?}"
            );
        }
    }

    #[test]
    fn cr_187_tips_start_off_with_ten_fifteen_and_twenty_percent() {
        let s = Settings::defaults(RATE);
        assert!(!s.tips_on);
        assert_eq!(s.tip_percents, vec![10, 15, 20]);
        assert_eq!(s.tip_percents, DEFAULT_TIP_PERCENTS);
        assert_eq!(s.validate(), Ok(()));
    }

    #[test]
    fn cr_187_tips_on_and_the_percents_tighten_in_both_directions() {
        let old = base();
        type Edit = (&'static str, fn(&mut Settings));
        let edits: [Edit; 4] = [
            ("tips_on", |s| s.tips_on = true),
            ("tip_percents", |s| s.tip_percents = vec![5, 10]),
            ("tip_percents", |s| s.tip_percents = Vec::new()),
            ("tip_percents", |s| s.tip_percents = vec![25, 30, 50]),
        ];
        for (name, edit) in edits {
            let mut new = old.clone();
            edit(&mut new);
            let fields = changed_fields(&old, &new);
            assert_eq!(fields.len(), 1, "{name}");
            assert_eq!(fields[0].field, name);
            assert_eq!(classify_change(&old, &new), Change::Tightening, "{name}");
            assert_eq!(
                classify_change(&new, &old),
                Change::Tightening,
                "{name} back"
            );
        }
        // Both together are still a tightening, and the owner alone signs.
        let (o, _, roots) = roots();
        let mut both = next(&old);
        both.tips_on = true;
        both.tip_percents = vec![12];
        let signed = SignedSettings {
            owner_sig: both.sign(&o),
            approver_sig: None,
            settings: both,
        };
        assert_eq!(signed.verify(&roots, Some(&old)), Ok(Change::Tightening));
        // Mixed with a loosening field, the whole change loosens.
        let mut mixed = old.clone();
        mixed.tips_on = true;
        mixed.min_sale_sats -= 1;
        assert_eq!(classify_change(&old, &mixed), Change::Loosening);
    }

    #[test]
    fn cr_187_the_percents_are_whole_percents_from_one_to_fifty_and_at_most_three() {
        let ok: [Vec<u8>; 4] = [vec![], vec![1], vec![50, 1, 7], vec![10, 15, 20]];
        for p in ok {
            let mut s = base();
            s.tip_percents = p.clone();
            assert_eq!(s.validate(), Ok(()), "{p:?}");
        }
        let bad: [Vec<u8>; 4] = [vec![0], vec![51], vec![10, 15, 20, 25], vec![5, 0, 5]];
        for p in bad {
            let mut s = base();
            s.tip_percents = p.clone();
            assert!(
                matches!(s.validate(), Err(SettingsError::Invalid(_))),
                "{p:?}"
            );
        }
    }

    #[test]
    fn cr_187_old_settings_read_as_tips_off_and_keep_their_signed_bytes() {
        // Tips off with the first percents write no block: the message is
        // version 1 and its hash is the hash of the bytes before the block.
        let s = wire_settings();
        let signed = signed_both(s.clone());
        let bytes = signed.to_bytes();
        assert_eq!(bytes[4], 1);
        let body = s.encode();
        assert_eq!(&bytes[5..5 + body.len()], &body[..]);
        let back = SignedSettings::from_bytes(&bytes).unwrap();
        assert!(!back.settings.tips_on);
        assert_eq!(back.settings.tip_percents, vec![10, 15, 20]);
        assert_eq!(back.settings, s);
        let (_, _, roots) = roots();
        assert_eq!(back.verify(&roots, None), Ok(Change::Loosening));
    }

    #[test]
    fn cr_187_the_tips_block_is_written_only_when_it_says_something() {
        let plain = wire_settings();
        let mut on = plain.clone();
        on.tips_on = true;
        let mut own = plain.clone();
        own.tip_percents = vec![5, 12];
        for s in [on, own] {
            assert_ne!(s.hash(), plain.hash());
            let bytes = signed_both(s.clone()).to_bytes();
            assert_eq!(bytes[4], 2);
            let back = SignedSettings::from_bytes(&bytes).unwrap();
            assert_eq!(back.settings, s);
            assert_eq!(back.to_bytes(), bytes);
            let (_, _, roots) = roots();
            assert!(back.verify_signatures(&roots).is_ok());
        }
        let mut on = plain.clone();
        on.tips_on = true;
        let bytes = signed_both(on).to_bytes();
        let body_end = bytes.len() - 64 - 1 - 64;
        // The block is a flag, a count and the percents, last in the body.
        assert_eq!(&bytes[body_end - 5..body_end], &[1, 3, 10, 15, 20][..]);
        // Every cut inside the block fails.
        for cut in 1..=5 {
            let mut short = bytes[..body_end - cut].to_vec();
            short.extend_from_slice(&bytes[body_end..]);
            assert!(SignedSettings::from_bytes(&short).is_err(), "{cut}");
        }
    }

    #[test]
    fn cr_187_a_tips_block_that_adds_nothing_or_overruns_is_refused() {
        let mut s = wire_settings();
        s.tips_on = true;
        let bytes = signed_both(s).to_bytes();
        let body_end = bytes.len() - 64 - 1 - 64;
        // Tips off and the first percents would have been written as version 1.
        let mut canon = bytes.clone();
        canon[body_end - 5] = 0;
        assert!(SignedSettings::from_bytes(&canon).is_err());
        // A flag that is not 0 or 1.
        let mut flag = bytes.clone();
        flag[body_end - 5] = 2;
        assert!(SignedSettings::from_bytes(&flag).is_err());
        // More than three percents.
        let mut many = bytes.clone();
        many[body_end - 4] = 4;
        assert!(SignedSettings::from_bytes(&many).is_err());
        // A version 1 frame that carries a block has bytes left over.
        let mut v1 = bytes.clone();
        v1[4] = 1;
        assert!(SignedSettings::from_bytes(&v1).is_err());
        // A version 3 frame is not known.
        let mut v3 = bytes;
        v3[4] = 3;
        assert!(SignedSettings::from_bytes(&v3).is_err());
    }

    #[test]
    fn a_setting_note_reads_back_and_refuses_anything_else() {
        for change in [Change::Tightening, Change::Loosening] {
            let note = SettingNote {
                version: 7,
                change,
                hash: base().hash(),
            };
            let body = note.to_body();
            assert_eq!(body.len(), SETTING_NOTE_LEN);
            assert_eq!(SettingNote::from_body(&body), Ok(note));
            // Cut short, a byte too many and a kind of change that is not 0
            // or 1 are all refused.
            assert!(SettingNote::from_body(&body[..body.len() - 1]).is_err());
            let mut long = body.clone();
            long.push(0);
            assert!(SettingNote::from_body(&long).is_err());
            let mut odd = body;
            odd[4] = 2;
            assert!(SettingNote::from_body(&odd).is_err());
        }
        assert!(SettingNote::from_body(&[]).is_err());
    }

    // ---- the shop's currency (CR-193 to CR-195) ----------------------------

    fn signed_by_both(s: &Settings, o: &Identity, a: &Identity) -> SignedSettings {
        SignedSettings {
            owner_sig: s.sign(o),
            approver_sig: Some(s.sign(a)),
            settings: s.clone(),
        }
    }

    /// Every limit given again in `currency`, at `per_unit` satoshis to a
    /// unit, with different numbers from `base()`.
    fn retyped(s: &Settings, currency: ShopCurrency, per_unit: u64) -> Settings {
        let mut n = next(s);
        n.currency = currency;
        let mut v = 7;
        for l in [
            &mut n.light_per_sale,
            &mut n.light_budget,
            &mut n.node_per_sale,
            &mut n.node_budget,
            &mut n.box_per_sale,
            &mut n.box_budget,
            &mut n.refund_cap,
            &mut n.refund_daily_cap,
            &mut n.cashier_override_daily_cap,
            &mut n.manager_override_ceiling,
        ] {
            v += 3;
            *l = Limit::from_usd(v, per_unit);
        }
        n
    }

    #[test]
    fn cr_193_a_dollar_shop_is_written_as_it_always_was() {
        let s = base();
        assert_eq!(s.currency, ShopCurrency::Usd);
        let bytes = s.encode();
        // The currency is the absence of a byte: the last byte is still the
        // watch key's flag.
        assert_eq!(bytes.last(), Some(&0));
        let (o, a, _) = roots();
        let wire = signed_by_both(&s, &o, &a).to_bytes();
        assert_eq!(wire.get(4), Some(&1));
        let back = SignedSettings::from_bytes(&wire).unwrap();
        assert_eq!(back.settings, s);
        assert_eq!(back.to_bytes(), wire);
    }

    #[test]
    fn cr_193_another_currency_is_wire_version_two_with_its_byte() {
        let (o, a, _) = roots();
        for c in ShopCurrency::ALL {
            let mut s = base();
            s.currency = c;
            if c.is_sats() {
                for l in [&mut s.light_per_sale, &mut s.light_budget] {
                    *l = Limit {
                        sats: 5,
                        display_usd: 5,
                    };
                }
                s.node_per_sale = Limit {
                    sats: 0,
                    display_usd: 0,
                };
                s.node_budget = s.node_per_sale;
                s.box_per_sale = s.node_per_sale;
                s.box_budget = s.node_per_sale;
                s.refund_cap = s.node_per_sale;
                s.refund_daily_cap = s.node_per_sale;
                s.cashier_override_daily_cap = s.node_per_sale;
                s.manager_override_ceiling = s.node_per_sale;
            }
            let signed = signed_by_both(&s, &o, &a);
            let wire = signed.to_bytes();
            let want = if c == ShopCurrency::Usd { 1 } else { 2 };
            assert_eq!(wire.get(4), Some(&want), "{}", c.code());
            assert_eq!(SignedSettings::from_bytes(&wire).unwrap(), signed);
            // The currency is in what is signed.
            let mut other = s.clone();
            other.currency = if c == ShopCurrency::Eur {
                ShopCurrency::Gbp
            } else {
                ShopCurrency::Eur
            };
            assert_ne!(s.hash(), other.hash());
        }
    }

    #[test]
    fn cr_193_version_one_reads_as_usd_and_other_versions_are_refused() {
        let (o, a, _) = roots();
        let wire = signed_by_both(&base(), &o, &a).to_bytes();
        assert_eq!(
            SignedSettings::from_bytes(&wire).unwrap().settings.currency,
            ShopCurrency::Usd
        );
        let mut v3 = wire.clone();
        v3[4] = 3;
        assert_eq!(
            SignedSettings::from_bytes(&v3),
            Err(DecodeError::UnsupportedVersion(3))
        );
        let mut v0 = wire;
        v0[4] = 0;
        assert_eq!(
            SignedSettings::from_bytes(&v0),
            Err(DecodeError::UnsupportedVersion(0))
        );
    }

    /// The bytes of the signed form: where the body ends, before the owner
    /// signature, the flag and the Key app signature.
    fn body_end(wire: &[u8]) -> usize {
        wire.len() - (64 + 1 + 64)
    }

    #[test]
    fn cr_193_a_currency_byte_that_is_not_canonical_is_refused() {
        let (o, a, _) = roots();
        let mut s = base();
        s.currency = ShopCurrency::Eur;
        let wire = signed_by_both(&s, &o, &a).to_bytes();
        // The byte opens the extension block: it sits before the tips block
        // (a flag, a count and three percents) that ends the body.
        let at = body_end(&wire) - 5 - 1;
        assert_eq!(wire.get(at), Some(&ShopCurrency::Eur.to_byte()));
        // USD with the first tips would have been written as version 1.
        for bad in [0u8, 1, 11, 255] {
            let mut w = wire.clone();
            w[at] = bad;
            assert!(
                matches!(
                    SignedSettings::from_bytes(&w),
                    Err(DecodeError::NonCanonical(_))
                ),
                "{bad}"
            );
        }
        // The block left off, and bytes after the end.
        let mut short = wire.clone();
        short.drain(at..at + 6);
        assert!(SignedSettings::from_bytes(&short).is_err());
        let mut long = wire.clone();
        long.push(0);
        assert!(SignedSettings::from_bytes(&long).is_err());
        assert!(SignedSettings::from_bytes(&wire[..wire.len() - 1]).is_err());
    }

    #[test]
    fn cr_187_every_pairing_of_a_currency_and_tips_round_trips_in_its_own_version() {
        let (o, a, roots) = roots();
        let mut on = wire_settings();
        on.tips_on = true;
        let mut own = wire_settings();
        own.tip_percents = vec![5, 12];
        let mut none = wire_settings();
        none.tip_percents = Vec::new();
        let tips = [wire_settings(), on, own, none];
        for currency in [ShopCurrency::Usd, ShopCurrency::Eur, ShopCurrency::Jpy] {
            for (i, base) in tips.iter().enumerate() {
                let mut s = base.clone();
                s.currency = currency;
                let signed = signed_by_both(&s, &o, &a);
                let wire = signed.to_bytes();
                // Only a dollar shop with tips off and the first percents is
                // version 1.
                let plain = currency == ShopCurrency::Usd && i == 0;
                assert_eq!(
                    wire.get(4),
                    Some(&if plain { 1 } else { 2 }),
                    "{currency:?} {i}"
                );
                let body = s.encode();
                if plain {
                    assert_eq!(body.last(), Some(&0), "the watch key's flag");
                } else {
                    // The block: the currency byte, then flag, count, percents.
                    let n = s.tip_percents.len();
                    let block = &body[body.len() - (3 + n)..];
                    assert_eq!(block[0], currency.to_byte(), "{currency:?} {i}");
                    assert_eq!(block[1], u8::from(s.tips_on));
                    assert_eq!(usize::from(block[2]), n);
                    assert_eq!(&block[3..], &s.tip_percents[..]);
                }
                let back = SignedSettings::from_bytes(&wire).unwrap();
                assert_eq!(back, signed, "{currency:?} {i}");
                assert_eq!(back.to_bytes(), wire);
                assert!(back.verify_signatures(&roots).is_ok());
            }
        }
        // Tips and the currency are both in what is signed.
        let mut eur = wire_settings();
        eur.currency = ShopCurrency::Eur;
        let mut tips = wire_settings();
        tips.tips_on = true;
        assert_ne!(eur.hash(), tips.hash());
        assert_ne!(eur.hash(), wire_settings().hash());
        assert_ne!(tips.hash(), wire_settings().hash());
    }

    #[test]
    fn cr_187_an_extension_block_that_adds_nothing_or_is_not_the_layout_is_refused() {
        let (o, a, _) = roots();
        // A dollar shop with tips off and the first percents in version 2.
        let mut eur = wire_settings();
        eur.currency = ShopCurrency::Eur;
        let wire = signed_by_both(&eur, &o, &a).to_bytes();
        let at = body_end(&wire) - 6;
        let mut nothing = wire.clone();
        nothing[at] = ShopCurrency::Usd.to_byte();
        assert!(matches!(
            SignedSettings::from_bytes(&nothing),
            Err(DecodeError::NonCanonical(_))
        ));
        // The same block with tips on is a message of version 2 as it should.
        let mut usd_tips = wire_settings();
        usd_tips.tips_on = true;
        let wire = signed_by_both(&usd_tips, &o, &a).to_bytes();
        assert_eq!(wire.get(4), Some(&2));
        assert_eq!(
            wire.get(body_end(&wire) - 6),
            Some(&ShopCurrency::Usd.to_byte())
        );
        assert!(SignedSettings::from_bytes(&wire).is_ok());
        // The tips flag that is not 0 or 1, a fourth percent, a count that
        // overruns the body.
        let at = body_end(&wire) - 5;
        let mut flag = wire.clone();
        flag[at] = 2;
        assert!(SignedSettings::from_bytes(&flag).is_err());
        let mut many = wire.clone();
        many[at + 1] = 4;
        assert!(SignedSettings::from_bytes(&many).is_err());
        let mut over = wire.clone();
        over[at + 1] = 200;
        assert!(SignedSettings::from_bytes(&over).is_err());
        // A version 1 message that carries a block has bytes left over, and
        // version 3 is not known.
        let mut v1 = wire.clone();
        v1[4] = 1;
        assert!(SignedSettings::from_bytes(&v1).is_err());
        let mut v3 = wire;
        v3[4] = 3;
        assert_eq!(
            SignedSettings::from_bytes(&v3),
            Err(DecodeError::UnsupportedVersion(3))
        );
    }

    #[test]
    fn cr_194_a_limit_in_bitcoin_standard_is_its_own_satoshis() {
        let mut s = base();
        s.currency = ShopCurrency::Sats;
        assert!(s.validate().is_err(), "dollar limits are not satoshis");
        let sat = |n| Limit {
            sats: u64::from(n),
            display_usd: n,
        };
        s.light_per_sale = sat(1_000);
        s.light_budget = sat(5_000);
        s.node_per_sale = sat(2_000);
        s.node_budget = sat(10_000);
        s.box_per_sale = sat(1_000);
        s.box_budget = sat(5_000);
        s.refund_cap = sat(0);
        s.refund_daily_cap = sat(0);
        s.cashier_override_daily_cap = sat(0);
        s.manager_override_ceiling = sat(0);
        assert_eq!(s.validate(), Ok(()));
        s.light_budget.sats += 1;
        assert!(
            matches!(s.validate(), Err(SettingsError::Invalid(why)) if why.starts_with("CR-194"))
        );
    }

    #[test]
    fn cr_195_a_change_of_currency_loosens_whatever_else_it_does() {
        let s = base();
        let mut n = next(&s);
        n.currency = ShopCurrency::Eur;
        let fields = changed_fields(&s, &n);
        assert_eq!(
            fields,
            vec![FieldChange {
                field: "currency",
                direction: Change::Loosening
            }]
        );
        assert_eq!(classify_change(&s, &n), Change::Loosening);
        // Even when every limit goes down.
        let mut down = retyped(&s, ShopCurrency::Eur, 1);
        down.light_per_sale = Limit {
            sats: 1,
            display_usd: 1,
        };
        assert_eq!(classify_change(&s, &down), Change::Loosening);
    }

    #[test]
    fn cr_195_a_currency_change_needs_the_key_app() {
        let (o, a, roots) = roots();
        let s = base();
        let n = retyped(&s, ShopCurrency::Eur, 1_100);
        let owner_only = SignedSettings {
            owner_sig: n.sign(&o),
            approver_sig: None,
            settings: n.clone(),
        };
        assert_eq!(
            owner_only.verify(&roots, Some(&s)),
            Err(SettingsError::ApproverRequired)
        );
        assert_eq!(
            signed_by_both(&n, &o, &a).verify(&roots, Some(&s)),
            Ok(Change::Loosening)
        );
    }

    #[test]
    fn cr_195_a_change_of_currency_that_carries_a_limit_is_refused() {
        let (o, a, roots) = roots();
        let s = base();
        // Only the currency changes: every limit is carried over.
        let mut n = next(&s);
        n.currency = ShopCurrency::Eur;
        assert_eq!(carried_limit_fields(&s, &n).len(), 10);
        assert!(matches!(
            signed_by_both(&n, &o, &a).verify(&roots, Some(&s)),
            Err(SettingsError::Invalid(why)) if why.starts_with("CR-195")
        ));
        // One limit carried, the others given again.
        let mut one = retyped(&s, ShopCurrency::Eur, 1_100);
        one.refund_cap = s.refund_cap;
        assert_eq!(carried_limit_fields(&s, &one), vec!["refund_cap"]);
        assert!(signed_by_both(&one, &o, &a)
            .verify(&roots, Some(&s))
            .is_err());
        // Every limit given again: taken. A limit turned off is not a carried
        // value, and neither is one that is off in both.
        let mut again = retyped(&s, ShopCurrency::Eur, 1_100);
        again.refund_daily_cap = Limit {
            sats: 0,
            display_usd: 0,
        };
        assert!(carried_limit_fields(&s, &again).is_empty());
        assert!(signed_by_both(&again, &o, &a)
            .verify(&roots, Some(&s))
            .is_ok());
        let mut off_both = s.clone();
        off_both.refund_cap = Limit {
            sats: 0,
            display_usd: 0,
        };
        let mut moved = retyped(&off_both, ShopCurrency::Eur, 1_100);
        moved.refund_cap = off_both.refund_cap;
        assert!(carried_limit_fields(&off_both, &moved).is_empty());
        // The same currency never counts anything as carried.
        assert!(carried_limit_fields(&s, &next(&s)).is_empty());
    }

    #[test]
    fn cr_195_the_store_takes_a_currency_change_only_with_every_limit_again() {
        let (o, a, roots) = roots();
        let mut store = SettingsStore::new(roots);
        let s1 = base();
        store.apply(signed_by_both(&s1, &o, &a)).unwrap();
        let mut carried = next(&s1);
        carried.currency = ShopCurrency::Jpy;
        assert!(store.apply(signed_by_both(&carried, &o, &a)).is_err());
        assert_eq!(store.current(), Some(&s1));
        let again = retyped(&s1, ShopCurrency::Jpy, 7);
        assert_eq!(
            store.apply(signed_by_both(&again, &o, &a)),
            Ok(Change::Loosening)
        );
        assert_eq!(store.current().map(|s| s.currency), Some(ShopCurrency::Jpy));
    }
}
