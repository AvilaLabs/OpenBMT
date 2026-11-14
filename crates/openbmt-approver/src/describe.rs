//! Settings values as text, for the old and new values of AS-27.

use std::fmt::Write;

use openbmt_core::currency::ShopCurrency;
use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{
    Limit, OwnNode, Settings, ShopLightning, SwapProvider, LIGHTNING_NODE_BYTES,
};

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
        "swap" => "Lightning through a swap provider",
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

/// The line for the swap provider (CR-218): its name, its onion name in
/// full and the largest sale over Lightning, so that the key holder can
/// compare them with what the owner's phone shows.
fn swap(provider: &Option<SwapProvider>) -> String {
    match provider {
        Some(p) => format!(
            "Lightning payments go through {} at {}{}, up to {} sats a sale",
            p.name, p.onion, p.path_prefix, p.max_sale_sats
        ),
        None => "no swap provider".to_string(),
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

/// The line for the own node (AP-11): `name:port`, or none.
fn own_node(node: &Option<OwnNode>) -> String {
    match node {
        Some(n) => format!("{}:{}", n.name, n.port),
        None => "none".to_string(),
    }
}

/// The line for Shop Lightning (SL-76): every part in full. The node's ID is
/// the one the key holder compares with the code the Stick shows.
fn shop_lightning(sl: &Option<ShopLightning>) -> String {
    match sl {
        Some(s) => format!(
            "node {}, Lightning counter {}, provider {} at {}, second copy on {}, \
             holds up to {} sats, opening costs up to {} sats",
            hex(&s.node_id),
            full(&s.counter),
            hex(&s.provider_node_id),
            s.provider_addr,
            full(&s.second_device),
            s.cap_sats,
            s.max_opening_fee_sats
        ),
        None => "none".to_string(),
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
        ("swap", swap(&s.swap)),
        ("watch_key", watch_key(&s.watch_key)),
        ("own_node", own_node(&s.own_node)),
        ("shop_lightning", shop_lightning(&s.shop_lightning)),
        ("node_only", flag(s.node_only)),
        ("light_fallback", flag(s.light_fallback)),
        ("tips_on", flag(s.tips_on)),
        ("tip_percents", tip_percents(&s.tip_percents)),
    ]
}

#[cfg(test)]
mod tests {
    //! Every field `changed_fields` can name has a value the Key holder is
    //! shown (AS-27, SL-76): a field without one would show as blank, or not
    //! at all.

    use super::*;
    use std::collections::BTreeSet;

    use openbmt_core::identity::Identity;
    use openbmt_core::onion::onion_name;
    use openbmt_core::settings::changed_fields;

    const SETTINGS_SOURCE: &str = include_str!("../../openbmt-core/src/settings.rs");

    fn dev(n: u8) -> DeviceId {
        Identity::from_secret_bytes(&[n; 32]).unwrap().device_id()
    }

    fn base() -> Settings {
        let mut s = Settings::defaults(1_000);
        s.min_confirmations = 3;
        s.final_confirmations = 6;
        s
    }

    fn lightning(node: u8) -> ShopLightning {
        ShopLightning {
            counter: dev(60),
            node_id: [node; LIGHTNING_NODE_BYTES],
            second_device: dev(61),
            provider_node_id: [8u8; LIGHTNING_NODE_BYTES],
            provider_addr: "203.0.113.5:9735".parse().unwrap(),
            cap_sats: 5_000_000,
            max_opening_fee_sats: 20_000,
        }
    }

    type Mutation = (&'static str, Box<dyn Fn(&mut Settings)>);

    /// One change for each field name, by the name.
    fn mutations() -> Vec<Mutation> {
        let m = |name, f: Box<dyn Fn(&mut Settings)>| -> Mutation { (name, f) };
        vec![
            m("currency", Box::new(|s| s.currency = ShopCurrency::Eur)),
            m("light_per_sale", Box::new(|s| s.light_per_sale.sats += 1)),
            m("light_budget", Box::new(|s| s.light_budget.sats += 1)),
            m("node_per_sale", Box::new(|s| s.node_per_sale.sats += 1)),
            m("node_budget", Box::new(|s| s.node_budget.sats += 1)),
            m("box_per_sale", Box::new(|s| s.box_per_sale.sats += 1)),
            m("box_budget", Box::new(|s| s.box_budget.sats += 1)),
            m(
                "underpayment_tolerance_bps",
                Box::new(|s| s.underpayment_tolerance_bps += 1),
            ),
            m("refund_cap", Box::new(|s| s.refund_cap.sats += 1)),
            m(
                "refund_daily_cap",
                Box::new(|s| s.refund_daily_cap.sats += 1),
            ),
            m(
                "cashier_override_daily_cap",
                Box::new(|s| s.cashier_override_daily_cap.sats += 1),
            ),
            m(
                "manager_override_ceiling",
                Box::new(|s| s.manager_override_ceiling.sats += 1),
            ),
            m("min_confirmations", Box::new(|s| s.min_confirmations += 1)),
            m(
                "final_confirmations",
                Box::new(|s| s.final_confirmations += 1),
            ),
            m(
                "heartbeat_max_age_secs",
                Box::new(|s| s.heartbeat_max_age_secs += 1),
            ),
            m("tor_on", Box::new(|s| s.tor_on = !s.tor_on)),
            m(
                "customer_wifi",
                Box::new(|s| s.customer_wifi = !s.customer_wifi),
            ),
            m("min_sale_sats", Box::new(|s| s.min_sale_sats += 1)),
            m(
                "devices",
                Box::new(|s| {
                    s.devices.insert(dev(40));
                }),
            ),
            m(
                "counters",
                Box::new(|s| {
                    s.counters.insert(dev(41), 100);
                }),
            ),
            m(
                "boxes",
                Box::new(|s| {
                    s.boxes.insert(dev(42));
                }),
            ),
            m(
                "lightning_node",
                Box::new(|s| s.lightning_node = Some([2u8; 33])),
            ),
            m(
                "swap",
                Box::new(|s| {
                    s.swap = Some(SwapProvider {
                        name: "Pretend".to_string(),
                        onion: onion_name(&[5u8; 32]),
                        path_prefix: "/api".to_string(),
                        max_sale_sats: 1_000_000,
                    })
                }),
            ),
            m(
                "own_node",
                Box::new(|s| {
                    s.own_node = Some(OwnNode::new(&onion_name(&[6u8; 32]), 8333).unwrap())
                }),
            ),
            m(
                "shop_lightning",
                Box::new(|s| s.shop_lightning = Some(lightning(7))),
            ),
            m("node_only", Box::new(|s| s.node_only = !s.node_only)),
            m(
                "light_fallback",
                Box::new(|s| s.light_fallback = !s.light_fallback),
            ),
            m("watch_key", Box::new(|s| s.watch_key = Some(dev(43)))),
            m("tips_on", Box::new(|s| s.tips_on = !s.tips_on)),
            m("tip_percents", Box::new(|s| s.tip_percents = vec![10, 15])),
        ]
    }

    /// Every name the body of `changed_fields` gives to a field.
    fn names_in_source() -> BTreeSet<String> {
        let start = SETTINGS_SOURCE.find("pub fn changed_fields").unwrap();
        let end = SETTINGS_SOURCE[start..]
            .find("pub fn classify_change")
            .unwrap();
        let body = &SETTINGS_SOURCE[start..start + end];
        body.split('"')
            .skip(1)
            .step_by(2)
            .filter(|t| {
                !t.is_empty()
                    && t.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            })
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn as_27_every_mutation_names_its_field_and_every_field_has_a_mutation() {
        let mut named = BTreeSet::new();
        for (name, f) in mutations() {
            let old = base();
            let mut new = old.clone();
            new.version += 1;
            f(&mut new);
            let fields: Vec<_> = changed_fields(&old, &new).iter().map(|c| c.field).collect();
            assert!(fields.contains(&name), "{name}: {fields:?}");
            named.insert(name.to_string());
        }
        assert_eq!(
            named,
            names_in_source(),
            "a field `changed_fields` names has no mutation here"
        );
    }

    #[test]
    fn as_27_sl_76_every_named_field_has_a_value_to_show() {
        for (name, f) in mutations() {
            let old = base();
            let mut new = old.clone();
            new.version += 1;
            f(&mut new);
            for c in changed_fields(&old, &new) {
                let shown = field_values(&new);
                let entry = shown.iter().find(|(n, _)| *n == c.field);
                assert!(
                    entry.is_some_and(|(_, v)| !v.is_empty()),
                    "{name}: no value for {}",
                    c.field
                );
            }
        }
    }

    #[test]
    fn sl_76_shop_lightning_is_shown_in_full() {
        let mut s = base();
        s.shop_lightning = Some(lightning(7));
        let shown = field_values(&s);
        let (_, v) = shown.iter().find(|(n, _)| *n == "shop_lightning").unwrap();
        assert!(v.contains(&"07".repeat(33)));
        assert!(v.contains(&"08".repeat(33)));
        assert!(v.contains("203.0.113.5:9735"));
        assert!(v.contains("5000000") && v.contains("20000"));
        let none = field_values(&base());
        let (_, v) = none.iter().find(|(n, _)| *n == "shop_lightning").unwrap();
        assert_eq!(v, "none");
    }
}
