//! Settings values as text, for the old and new values of AS-27.

use std::fmt::Write;

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

fn limit(l: &Limit) -> String {
    format!("{} sats (${} from the owner phone)", l.sats, l.display_usd)
}

fn short(d: &DeviceId) -> String {
    hex(d.to_bytes().get(..4).unwrap_or(&[]))
}

fn flag(v: bool) -> String {
    if v { "on" } else { "off" }.to_string()
}

/// The name the key holder is shown for a field. Most fields keep the name
/// `changed_fields` gives; the smallest bitcoin sale is named in plain words.
pub(crate) fn shown_name(field: &'static str) -> &'static str {
    match field {
        "min_sale_sats" => "Smallest bitcoin sale",
        "lightning_node" => "Lightning node",
        "watch_key" => "Watch key",
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

/// Every field that `changed_fields` can name, with its value as text.
pub(crate) fn field_values(s: &Settings) -> Vec<(&'static str, String)> {
    let devices = if s.devices.is_empty() {
        "none".to_string()
    } else {
        s.devices.iter().map(short).collect::<Vec<_>>().join(", ")
    };
    let counters = if s.counters.is_empty() {
        "none".to_string()
    } else {
        s.counters
            .iter()
            .map(|(d, share)| format!("{} {} bp", short(d), share))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let boxes = if s.boxes.is_empty() {
        "none".to_string()
    } else {
        s.boxes.iter().map(short).collect::<Vec<_>>().join(", ")
    };
    vec![
        ("light_per_sale", limit(&s.light_per_sale)),
        ("light_budget", limit(&s.light_budget)),
        ("node_per_sale", limit(&s.node_per_sale)),
        ("node_budget", limit(&s.node_budget)),
        ("box_per_sale", limit(&s.box_per_sale)),
        ("box_budget", limit(&s.box_budget)),
        (
            "underpayment_tolerance_bps",
            format!("{} bp", s.underpayment_tolerance_bps),
        ),
        ("refund_cap", limit(&s.refund_cap)),
        ("refund_daily_cap", limit(&s.refund_daily_cap)),
        (
            "cashier_override_daily_cap",
            limit(&s.cashier_override_daily_cap),
        ),
        (
            "manager_override_ceiling",
            limit(&s.manager_override_ceiling),
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
    ]
}
