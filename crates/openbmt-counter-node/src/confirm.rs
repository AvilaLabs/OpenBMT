//! A change to the shop's settings that waits for a counter's confirmation
//! (`docs/COUNTER-RULES.md` §24, CR-239 to CR-244).
//!
//! A shop that uses its own wallet has no Key app to co-sign a loosening. The
//! owner phone sends the signed settings (`OBMS`); the counter checks all
//! but the confirmation, does not put them in force and keeps them as its one
//! change to confirm in [`crate::dirstore::CONFIRM_FILE`] (CR-239). A manager
//! reads what the change loosens, compares four words with the owner phone and
//! types the PIN (CR-240); the counter then signs the confirmation and writes
//! it as a record of its own chain (CR-241). The owner phone sends the
//! settings again in the frame `OBM<` (CR-242), and every counter takes them
//! as it takes any settings.
//!
//! This module holds what the node keeps and what it tells the app: the
//! waiting change, its fields with their values now and after, and the
//! notices that follow a change that was dropped or refused.

use openbmt_core::approval::{confirm_words, drops_last_counter, field_class, ChangeClass};
use openbmt_core::settings::{changed_fields, Change, Limit, Settings, SignedSettings};
use openbmt_core::words::WORD_COUNT;

/// CR-239: how long a change waits for its confirmation: seven days from the
/// time it arrived.
pub const PENDING_SECS: u64 = 7 * 86_400;

/// One value of a field of the settings, before or after a change (CR-240).
/// The app writes it, in the shop's currency where it is an amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    /// A limit in whole units of the shop's currency (0 is off).
    Units(u32),
    /// An amount in satoshis.
    Sats(u64),
    /// A share in basis points (100 is one percent).
    Bps(u32),
    /// A count: confirmations, devices, counters.
    Count(u32),
    /// A length of time in seconds.
    Seconds(u32),
    /// A switch.
    Flag(bool),
    /// A currency, by its three letters.
    Code(String),
}

/// One field a change loosens, with its value now and after (CR-240).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedField {
    /// The name of the field, as `settings::changed_fields` gives it.
    pub field: &'static str,
    /// The value in the settings in force.
    pub now: FieldValue,
    /// The value in the change.
    pub after: FieldValue,
}

fn count(n: usize) -> FieldValue {
    FieldValue::Count(u32::try_from(n).unwrap_or(u32::MAX))
}

/// The value of a limit (CR-240): the satoshis when they differ, since a
/// counter enforces satoshis and the shown units can move the other way; the
/// units only when the satoshis are the same.
fn limit(old: &Limit, new: &Limit) -> (FieldValue, FieldValue) {
    if old.sats == new.sats && old.display_usd != new.display_usd {
        (
            FieldValue::Units(old.display_usd),
            FieldValue::Units(new.display_usd),
        )
    } else {
        (FieldValue::Sats(old.sats), FieldValue::Sats(new.sats))
    }
}

/// The own node of `s` as text, or "Not set" (AP-11).
fn own_node_text(s: &Settings) -> String {
    s.own_node.as_ref().map_or_else(
        || "Not set".to_string(),
        |n| format!("{}:{}", n.name, n.port),
    )
}

/// The first four bytes of `bytes` as hex.
fn hex_prefix(bytes: &[u8]) -> String {
    bytes.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

/// Shop Lightning of `s` as text, or "Not set" (AP-13): the provider's key
/// prefix and address, the second device's id prefix, and the cap and the most
/// for one opening in satoshis.
fn shop_lightning_text(s: &Settings) -> String {
    s.shop_lightning.as_ref().map_or_else(
        || "Not set".to_string(),
        |sl| {
            format!(
                "provider {} at {}, second device {}, cap {} sats, opening {} sats",
                hex_prefix(&sl.provider_node_id),
                sl.provider_addr,
                hex_prefix(&sl.second_device.to_bytes()),
                sl.cap_sats,
                sl.max_opening_fee_sats
            )
        },
    )
}

/// The values of `field` in `old` and `new`, or `None` for a field that is not
/// shown (the Lightning node, the watch key and the Boxes, which no
/// loosening of a counter's concern names).
fn values(field: &str, old: &Settings, new: &Settings) -> Option<(FieldValue, FieldValue)> {
    use FieldValue::{Code, Count, Flag, Sats, Seconds};
    Some(match field {
        "light_per_sale" => limit(&old.light_per_sale, &new.light_per_sale),
        "light_budget" => limit(&old.light_budget, &new.light_budget),
        "node_per_sale" => limit(&old.node_per_sale, &new.node_per_sale),
        "node_budget" => limit(&old.node_budget, &new.node_budget),
        "box_per_sale" => limit(&old.box_per_sale, &new.box_per_sale),
        "box_budget" => limit(&old.box_budget, &new.box_budget),
        "refund_cap" => limit(&old.refund_cap, &new.refund_cap),
        "refund_daily_cap" => limit(&old.refund_daily_cap, &new.refund_daily_cap),
        "cashier_override_daily_cap" => limit(
            &old.cashier_override_daily_cap,
            &new.cashier_override_daily_cap,
        ),
        "manager_override_ceiling" => {
            limit(&old.manager_override_ceiling, &new.manager_override_ceiling)
        }
        "underpayment_tolerance_bps" => (
            FieldValue::Bps(old.underpayment_tolerance_bps),
            FieldValue::Bps(new.underpayment_tolerance_bps),
        ),
        "min_confirmations" => (
            Count(u32::from(old.min_confirmations)),
            Count(u32::from(new.min_confirmations)),
        ),
        "final_confirmations" => (
            Count(u32::from(old.final_confirmations)),
            Count(u32::from(new.final_confirmations)),
        ),
        "heartbeat_max_age_secs" => (
            Seconds(old.heartbeat_max_age_secs),
            Seconds(new.heartbeat_max_age_secs),
        ),
        "tor_on" => (Flag(old.tor_on), Flag(new.tor_on)),
        "customer_wifi" => (Flag(old.customer_wifi), Flag(new.customer_wifi)),
        "min_sale_sats" => (Sats(old.min_sale_sats), Sats(new.min_sale_sats)),
        "devices" => (count(old.devices.len()), count(new.devices.len())),
        "counters" => (count(old.counters.len()), count(new.counters.len())),
        // AP-11: the node every device trusts for its payments, as
        // `<name>:<port>`, or "Not set".
        "own_node" => (Code(own_node_text(old)), Code(own_node_text(new))),
        // AP-13: the provider, the second device and the limits.
        "shop_lightning" => (
            Code(shop_lightning_text(old)),
            Code(shop_lightning_text(new)),
        ),
        "node_only" => (Flag(old.node_only), Flag(new.node_only)),
        "light_fallback" => (Flag(old.light_fallback), Flag(new.light_fallback)),
        "currency" => (
            Code(old.currency.code().to_string()),
            Code(new.currency.code().to_string()),
        ),
        _ => return None,
    })
}

/// CR-240: every field `new` loosens from `old` that a counter's
/// confirmation answers for, with its value now and after. A field that is
/// Free by AP-01 (a Box added, the watch key) is not listed, and a Redirect
/// is never kept to be confirmed.
pub fn loosened_fields(old: &Settings, new: &Settings) -> Vec<ChangedField> {
    let mut out: Vec<ChangedField> = Vec::new();
    for c in changed_fields(old, new) {
        // A limit that changes in satoshis and in units is named twice.
        let last_counter = c.field == "counters" && drops_last_counter(old, new);
        if !(c.direction == Change::Loosening || last_counter)
            || field_class(old, new, c.field) != ChangeClass::Loosen
            || out.iter().any(|f| f.field == c.field)
        {
            continue;
        }
        if let Some((now, after)) = values(c.field, old, new) {
            out.push(ChangedField {
                field: c.field,
                now,
                after,
            });
        }
    }
    out
}

/// The one change that waits (CR-239).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The settings and the owner's signature, as they arrived.
    pub signed: SignedSettings,
    /// When they arrived, by the node's clock.
    pub arrived: u64,
    /// The counter signed its confirmation of these settings since it last
    /// started (CR-241).
    pub confirmed: bool,
}

impl Pending {
    /// The time after which the change is dropped (CR-239).
    pub fn expires_at(&self) -> u64 {
        self.arrived.saturating_add(PENDING_SECS)
    }
}

/// What the manager is shown of the waiting change (CR-240).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingChange {
    /// The settings version the change would put in force.
    pub version: u32,
    /// The hash of the settings, which the confirmation signs.
    pub hash: [u8; 32],
    /// The four words of AP-03 for the hash.
    pub words: [&'static str; WORD_COUNT],
    /// Each field the change loosens, against the settings in force.
    pub fields: Vec<ChangedField>,
    /// When the change arrived.
    pub arrived: u64,
    /// When the change is dropped if nobody confirmed it.
    pub expires_at: u64,
    /// The counter confirmed it since it last started.
    pub confirmed: bool,
}

impl PendingChange {
    /// The page's data for `pending` against the settings in force.
    pub fn of(pending: &Pending, in_force: &Settings) -> Self {
        let hash = pending.signed.settings.hash();
        Self {
            version: pending.signed.settings.version,
            hash,
            words: confirm_words(&hash),
            fields: loosened_fields(in_force, &pending.signed.settings),
            arrived: pending.arrived,
            expires_at: pending.expires_at(),
            confirmed: pending.confirmed,
        }
    }
}

/// Something the manager is told about a change that did not come to
/// anything (CR-244). It stays until a change is kept or settings come into
/// force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeNotice {
    /// A change waited seven days and was dropped (CR-239).
    Dropped {
        /// The version of the change.
        version: u32,
    },
    /// A change came with a confirmation that is not from a counter of the
    /// shop, or is not for those settings (CR-242).
    BadConfirmation {
        /// The version of the change.
        version: u32,
    },
    /// A change sends payments somewhere else, which only the shop's wallet
    /// can approve (CR-239).
    NeedsWallet {
        /// The version of the change.
        version: u32,
    },
}

/// Why the confirmation of the waiting change was not signed (CR-241).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmRefused {
    /// No change waits, or the one that waits is not the version the person
    /// was shown.
    NothingWaits,
    /// The record could not be written, so nothing was signed.
    NotWritten(String),
}

impl ConfirmRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            Self::NothingWaits => "CR-241",
            Self::NotWritten(_) => "CR-40",
        }
    }
}

impl std::fmt::Display for ConfirmRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::NothingWaits => write!(
                f,
                "no change waits to be confirmed, or it is not the one that was shown"
            ),
            Self::NotWritten(e) => write!(f, "the confirmation was not written: {e}"),
        }
    }
}

impl std::error::Error for ConfirmRefused {}
