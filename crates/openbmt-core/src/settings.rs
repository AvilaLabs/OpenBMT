//! Signed shop settings.
//!
//! Settings are signed by the owner root. A change that only makes things
//! stricter (tightening) needs that one signature. A change that lets more
//! money leave or be accepted at risk, or removes a check or an alert
//! (loosening), also needs the Key app's co-signature (§2.9). A change we
//! cannot sort is loosening, and a change that mixes both is loosening.
//!
//! Each money limit is kept in satoshis, which is what counters enforce, and
//! in whole display dollars "from the owner phone", which is only a label.
//! Moving either number up counts as loosening.

use std::collections::BTreeSet;
use std::fmt;

use crate::batch::PinnedRoots;
use crate::identity::{tagged_hash, DeviceId, Identity, Sig, TAG_SETTINGS};
use crate::policy;

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
    /// Light check: this counter's slice of the outstanding total.
    pub light_slice: Limit,
    /// Own node: largest instant sale.
    pub node_per_sale: Limit,
    /// Own node: this counter's slice.
    pub node_slice: Limit,
    /// Box reports only: largest instant sale.
    pub box_per_sale: Limit,
    /// Box reports only: this counter's slice.
    pub box_slice: Limit,
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
    /// Longest age of the owner-phone heartbeat that still allows instant.
    pub heartbeat_max_age_secs: u32,
    /// Tor switch is on.
    pub tor_on: bool,
    /// Customers can join the Wi-Fi the counter is on (or the owner is
    /// unsure).
    pub customer_wifi: bool,
    /// Devices allowed to write records.
    pub devices: BTreeSet<DeviceId>,
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
            light_slice: usd(policy::LIGHT_SLICE_USD),
            node_per_sale: usd(policy::NODE_PER_SALE_USD),
            node_slice: usd(policy::NODE_SLICE_USD),
            box_per_sale: usd(policy::BOX_PER_SALE_USD),
            box_slice: usd(policy::BOX_SLICE_USD),
            underpayment_tolerance_bps: 100,
            refund_cap: usd(100),
            refund_daily_cap: usd(300),
            cashier_override_daily_cap: usd(20),
            manager_override_ceiling: usd(100),
            min_confirmations: 1,
            heartbeat_max_age_secs: policy::DEFAULT_HEARTBEAT_MAX_AGE_SECS,
            tor_on: false,
            customer_wifi: true,
            devices: BTreeSet::new(),
        }
    }

    /// Canonical bytes: fixed field order, little-endian numbers, devices
    /// sorted.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256 + self.devices.len() * 32);
        out.extend_from_slice(&self.version.to_le_bytes());
        for l in self.limits() {
            out.extend_from_slice(&l.sats.to_le_bytes());
            out.extend_from_slice(&l.display_usd.to_le_bytes());
        }
        out.extend_from_slice(&self.underpayment_tolerance_bps.to_le_bytes());
        out.push(self.min_confirmations);
        out.extend_from_slice(&self.heartbeat_max_age_secs.to_le_bytes());
        out.push(u8::from(self.tor_on));
        out.push(u8::from(self.customer_wifi));
        out.extend_from_slice(&(self.devices.len() as u32).to_le_bytes());
        for d in &self.devices {
            out.extend_from_slice(&d.to_bytes());
        }
        out
    }

    fn limits(&self) -> [&Limit; 10] {
        [
            &self.light_per_sale,
            &self.light_slice,
            &self.node_per_sale,
            &self.node_slice,
            &self.box_per_sale,
            &self.box_slice,
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
    limit(o, "light_slice", &old.light_slice, &new.light_slice);
    limit(o, "node_per_sale", &old.node_per_sale, &new.node_per_sale);
    limit(o, "node_slice", &old.node_slice, &new.node_slice);
    limit(o, "box_per_sale", &old.box_per_sale, &new.box_per_sale);
    limit(o, "box_slice", &old.box_slice, &new.box_slice);
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
    if old.devices != new.devices {
        let added = new.devices.difference(&old.devices).next().is_some();
        o.push(FieldChange {
            field: "devices",
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
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OwnerSignatureInvalid => write!(f, "owner signature missing or wrong"),
            Self::ApproverRequired => write!(f, "this change needs the Key app's co-signature"),
            Self::ApproverSignatureInvalid => write!(f, "Key app signature is wrong"),
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

/// The settings a counter has accepted so far.
#[derive(Debug)]
pub struct SettingsStore {
    roots: PinnedRoots,
    current: Option<SignedSettings>,
}

impl SettingsStore {
    /// An empty store for these pinned roots.
    pub fn new(roots: PinnedRoots) -> Self {
        Self {
            roots,
            current: None,
        }
    }

    /// The settings in force, if any.
    pub fn current(&self) -> Option<&Settings> {
        self.current.as_ref().map(|s| &s.settings)
    }

    /// Verify and take new settings. Nothing changes on failure.
    pub fn apply(&mut self, signed: SignedSettings) -> Result<Change, SettingsError> {
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
        assert_eq!(s.light_slice.display_usd, 50);
        assert_eq!(s.node_per_sale.sats, 25 * RATE);
        assert_eq!(s.node_slice.sats, 100 * RATE);
        assert_eq!(s.box_per_sale.display_usd, 10);
        assert_eq!(s.box_slice.display_usd, 50);
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
            ("light_slice", |s| s.light_slice.sats += 1),
            ("node_per_sale", |s| s.node_per_sale.sats += 1),
            ("node_slice", |s| s.node_slice.sats += 1),
            ("box_per_sale", |s| s.box_per_sale.sats += 1),
            ("box_slice", |s| s.box_slice.sats += 1),
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
            ("heartbeat_max_age_secs", |s| s.heartbeat_max_age_secs += 1),
            ("tor_on", |s| s.tor_on = false),
            ("customer_wifi", |s| s.customer_wifi = false),
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
        new.light_slice.sats -= 100; // tightening
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
        new.tor_on = true;
        new.customer_wifi = true;
        new.heartbeat_max_age_secs -= 1;
        assert_eq!(classify_change(&old, &new), Change::Tightening);
    }

    #[test]
    fn encoding_is_stable_and_covers_every_field() {
        let s = base();
        assert_eq!(s.hash(), s.clone().hash());
        let mut t = s.clone();
        t.version += 1;
        assert_ne!(t.hash(), s.hash());
        let mut t = s.clone();
        t.tor_on = !t.tor_on;
        assert_ne!(t.hash(), s.hash());
        let mut t = s.clone();
        t.devices.insert(identity(43).device_id());
        assert_ne!(t.hash(), s.hash());
        let mut t = s.clone();
        t.manager_override_ceiling.display_usd += 1;
        assert_ne!(t.hash(), s.hash());
        // device order does not change the bytes
        let mut a = s.clone();
        a.devices.insert(identity(50).device_id());
        a.devices.insert(identity(51).device_id());
        let mut b = s;
        b.devices.insert(identity(51).device_id());
        b.devices.insert(identity(50).device_id());
        assert_eq!(a.encode(), b.encode());
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
        let cur = base();
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
}
