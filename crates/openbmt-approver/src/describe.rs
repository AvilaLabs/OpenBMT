//! Settings values as text, for the old and new values of AS-27.

use std::fmt::Write;

use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{Limit, Settings};

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
        other => other,
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
    ]
}
