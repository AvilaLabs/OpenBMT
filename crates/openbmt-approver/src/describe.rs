//! Settings values as text, for the old and new values of AS-27.

use std::fmt::Write;

use openbmt_core::currency::ShopCurrency;
use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{Limit, Settings, LIGHTNING_NODE_BYTES};

/// Bytes as lower-case hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

fn limit(l: &Limit, currency: ShopCurrency) -> String {
    format!(
        "{} sats ({} from the owner phone)",
        l.sats,
        currency.whole_text(u64::from(l.display_usd))
    )
}

/// An identity in full: 64 lower-case hex digits in groups of four, so that
/// two identities that share their first bytes still read differently (AS-39).
pub(crate) fn full(d: &DeviceId) -> String {
    let digits = hex(&d.to_bytes());
    let groups: Vec<&str> = digits
        .as_bytes()
        .chunks(4)
        .filter_map(|c| std::str::from_utf8(c).ok())
        .collect();
    groups.join(" ")
}

fn flag(v: bool) -> String {
    if v { "on" } else { "off" }.to_string()
}

/// The name the key holder is shown for a field. Most fields keep the name
/// `changed_fields` gives; the smallest bitcoin sale is named in plain words.
pub(crate) fn shown_name(field: &'static str) -> &'static str {
    match field {
        "currency" => "Currency",
        "min_sale_sats" => "Smallest bitcoin sale",
        "lightning_node" => "Lightning node",
        "watch_key" => "Watch key",
        "tips_on" => "Ask for a tip",
        "tip_percents" => "Tips",
        other => other,
    }
}

/// The line for the shop's Lightning node (LN-03). The key is shown in full:
/// a short form of a public key can be ground to match, and the key holder
/// compares this with the node's own id before co-signing.
fn lightning_node(key: &Option<[u8; LIGHTNING_NODE_BYTES]>) -> String {
    match key {
        Some(k) => format!("Lightning payments go to node {}", hex(k)),
        None => "no Lightning node".to_string(),
    }
}

/// The line for the watch key (CR-185). The key is shown in full, as the
/// Lightning node's is: the key holder compares it with what the owner's
/// phone shows before co-signing.
fn watch_key(key: &Option<DeviceId>) -> String {
    match key {
        Some(k) => format!(
            "heartbeats from the owner's phone while its app is closed are \
             taken from key {}",
            hex(&k.to_bytes())
        ),
        None => "no watch key".to_string(),
    }
}

/// The line for the suggested tips (CR-187): the percents, or none.
fn tip_percents(percents: &[u8]) -> String {
    if percents.is_empty() {
        "none".to_string()
    } else {
        percents
            .iter()
            .map(|p| format!("{p}%"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Every field that `changed_fields` can name, with its value as text.
pub(crate) fn field_values(s: &Settings) -> Vec<(&'static str, String)> {
    let devices = if s.devices.is_empty() {
        "none".to_string()
    } else {
        s.devices.iter().map(full).collect::<Vec<_>>().join(", ")
    };
    let counters = if s.counters.is_empty() {
        "none".to_string()
    } else {
        s.counters
            .iter()
            .map(|(d, share)| format!("{} {} bp", full(d), share))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let boxes = if s.boxes.is_empty() {
        "none".to_string()
    } else {
        s.boxes.iter().map(full).collect::<Vec<_>>().join(", ")
    };
    vec![
        ("currency", s.currency.label().to_string()),
        ("light_per_sale", limit(&s.light_per_sale, s.currency)),
        ("light_budget", limit(&s.light_budget, s.currency)),
        ("node_per_sale", limit(&s.node_per_sale, s.currency)),
        ("node_budget", limit(&s.node_budget, s.currency)),
        ("box_per_sale", limit(&s.box_per_sale, s.currency)),
        ("box_budget", limit(&s.box_budget, s.currency)),
        (
            "underpayment_tolerance_bps",
            format!("{} bp", s.underpayment_tolerance_bps),
        ),
        ("refund_cap", limit(&s.refund_cap, s.currency)),
        ("refund_daily_cap", limit(&s.refund_daily_cap, s.currency)),
        (
            "cashier_override_daily_cap",
            limit(&s.cashier_override_daily_cap, s.currency),
        ),
        (
            "manager_override_ceiling",
            limit(&s.manager_override_ceiling, s.currency),
        ),
        (
            "min_confirmations",
            format!("{} blocks", s.min_confirmations),
        ),
        (
            "final_confirmations",
            format!("{} blocks", s.final_confirmations),
        ),
        (
            "heartbeat_max_age_secs",
            format!("{} s", s.heartbeat_max_age_secs),
        ),
        ("tor_on", flag(s.tor_on)),
        ("customer_wifi", flag(s.customer_wifi)),
        ("min_sale_sats", format!("{} sats", s.min_sale_sats)),
        ("devices", devices),
        ("counters", counters),
        ("boxes", boxes),
        ("lightning_node", lightning_node(&s.lightning_node)),
        ("watch_key", watch_key(&s.watch_key)),
        ("tips_on", flag(s.tips_on)),
        ("tip_percents", tip_percents(&s.tip_percents)),
    ]
}
