//! Changing the shop's settings from the owner phone (AS-74 to AS-77): an
//! edit with every field optional is applied to the settings held, checked,
//! and sorted as the queue sorts it, and nothing is stored. The settings
//! that come out go to [`Owner::queue_settings`] when the owner agrees.

use bitcoin::secp256k1::PublicKey;
use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{
    changed_fields, classify_change, Change, FieldChange, Limit, Settings, LIGHTNING_NODE_BYTES,
};

use crate::error::{NodeKeyFault, Refusal};
use crate::state::Owner;

/// Most confirmations an edit may ask for (AS-74).
pub const MAX_EDIT_CONFIRMATIONS: u8 = 100;

/// What an edit does to the Lightning node (AS-76).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LightningEdit {
    /// Name no node: Lightning payments stop.
    Remove,
    /// Name this node, as typed or scanned: a 66-digit key, or an address
    /// `key@host:port` of which only the key is taken.
    Set(String),
}

/// What an edit does to the watch key (CR-185).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEdit {
    /// Name no watch key: heartbeats signed by one stop counting.
    Remove,
    /// Name this watch key.
    Set(DeviceId),
}

/// A change of the settings: each field that is `None` (or empty) is kept
/// as it is (AS-74). Money is in whole dollars, 0 meaning off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsEdit {
    /// Today's price of one bitcoin in whole dollars, for the limits this
    /// edit changes. A changed limit without it is refused (AS-75).
    pub usd_per_btc: Option<u64>,
    /// Light check: largest instant sale.
    pub light_per_sale: Option<u32>,
    /// Light check: shop-wide outstanding instant total.
    pub light_budget: Option<u32>,
    /// Own node: largest instant sale.
    pub node_per_sale: Option<u32>,
    /// Own node: shop-wide outstanding instant total.
    pub node_budget: Option<u32>,
    /// Box reports only: largest instant sale.
    pub box_per_sale: Option<u32>,
    /// Box reports only: shop-wide outstanding instant total.
    pub box_budget: Option<u32>,
    /// Underpayment tolerance, in basis points.
    pub underpayment_tolerance_bps: Option<u32>,
    /// Largest single refund.
    pub refund_cap: Option<u32>,
    /// Largest refunds per day.
    pub refund_daily_cap: Option<u32>,
    /// Cashier's daily override cap.
    pub cashier_override_daily_cap: Option<u32>,
    /// Manager's override ceiling.
    pub manager_override_ceiling: Option<u32>,
    /// Confirmations that settle a sale.
    pub min_confirmations: Option<u8>,
    /// Confirmations at which a sale is final.
    pub final_confirmations: Option<u8>,
    /// Longest heartbeat age that still allows instant, in seconds.
    pub heartbeat_max_age_secs: Option<u32>,
    /// The Tor switch.
    pub tor_on: Option<bool>,
    /// Customers can join the counter's Wi-Fi.
    pub customer_wifi: Option<bool>,
    /// Smallest bitcoin sale, in satoshis; 0 means none.
    pub min_sale_sats: Option<u64>,
    /// The counters ask the customer for a tip (CR-187).
    pub tips_on: Option<bool>,
    /// The suggested tips, in whole percent, at most three (CR-187): the
    /// whole list replaces the one held, and an empty list suggests none.
    pub tip_percents: Option<Vec<u8>>,
    /// New shares for counters, in basis points. A counter that is not
    /// named keeps its share; a key that is not a counter is refused.
    pub shares: Vec<(DeviceId, u16)>,
    /// The Lightning node: `None` keeps it.
    pub lightning_node: Option<LightningEdit>,
    /// The watch key: `None` keeps it.
    pub watch_key: Option<WatchEdit>,
}

/// What an edit would do, before anything is stored (AS-77).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPreview {
    /// The settings the edit makes, at the next version, validated. They
    /// are what [`Owner::queue_settings`] is given.
    pub settings: Settings,
    /// What they are compared with: the settings in force, or, with none in
    /// force yet, the ones queued.
    pub reference: Settings,
    /// Each field that differs from the reference, once, with the direction
    /// of that field. A limit whose satoshis and dollars pull in two
    /// directions is loosening.
    pub fields: Vec<FieldChange>,
    /// The whole change as the queue sorts it: loosening needs the Key
    /// app's co-signature.
    pub change: Change,
    /// The price of a bitcoin in dollars and the satoshis to a dollar that
    /// the changed limits were made at, when any was (AS-75).
    pub price: Option<(u64, u64)>,
}

/// Lowest price of a bitcoin in dollars that is taken (AS-75).
pub const MIN_USD_PER_BTC: u64 = 1_000;
/// Highest price of a bitcoin in dollars that is taken (AS-75).
pub const MAX_USD_PER_BTC: u64 = 10_000_000;

/// AS-75: satoshis to a dollar at a price of `usd_per_btc` dollars for a
/// bitcoin: 100,000,000 over the price, rounded to the nearest whole number
/// (a half rounds up). A price outside $1,000 to $10,000,000 is refused.
pub fn sats_per_usd_at(usd_per_btc: u64) -> Result<u64, Refusal> {
    if !(MIN_USD_PER_BTC..=MAX_USD_PER_BTC).contains(&usd_per_btc) {
        return Err(Refusal::PriceOutOfRange);
    }
    Ok((200_000_000 + usd_per_btc) / (2 * usd_per_btc))
}

fn hex_val(c: char) -> Option<u8> {
    c.to_digit(16).and_then(|d| u8::try_from(d).ok())
}

/// The host and port after the @ of a node address (AS-76): a host that is
/// not empty and has no space, a colon, and a port from 1 to 65535.
fn host_and_port(text: &str) -> bool {
    let Some((host, port)) = text.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && !host.chars().any(|c| c.is_whitespace() || c == '@')
        && port.parse::<u16>().is_ok_and(|p| p >= 1)
}

/// AS-76: the key of a Lightning node from the text a person typed, pasted
/// or scanned. Either the key alone, 66 hexadecimal digits, or an address
/// `key@host:port`, from which only the key is taken. Capitals are read.
pub fn parse_lightning_node(text: &str) -> Result<[u8; LIGHTNING_NODE_BYTES], Refusal> {
    let fault = Refusal::LightningNode;
    let text = text.trim();
    let (key, address) = match text.split_once('@') {
        Some((key, address)) => (key, Some(address)),
        None => (text, None),
    };
    let abbreviated = key.contains('.') || key.contains('\u{2026}');
    let digits: Vec<u8> = key
        .chars()
        .filter(|c| !matches!(c, '.' | '\u{2026}'))
        .map(|c| hex_val(c).ok_or(fault(NodeKeyFault::NotAKey)))
        .collect::<Result<_, _>>()?;
    if digits.is_empty() {
        return Err(fault(NodeKeyFault::NotAKey));
    }
    if abbreviated || digits.len() < 2 * LIGHTNING_NODE_BYTES {
        return Err(fault(NodeKeyFault::ShortForm));
    }
    if digits.len() > 2 * LIGHTNING_NODE_BYTES {
        return Err(fault(NodeKeyFault::NotAKey));
    }
    let mut bytes = [0u8; LIGHTNING_NODE_BYTES];
    for (byte, pair) in bytes.iter_mut().zip(digits.chunks(2)) {
        if let [high, low] = pair {
            *byte = (high << 4) | low;
        }
    }
    if !matches!(bytes.first(), Some(2 | 3)) {
        return Err(fault(NodeKeyFault::NotCompressed));
    }
    if PublicKey::from_slice(&bytes).is_err() {
        return Err(fault(NodeKeyFault::NotOnCurve));
    }
    if let Some(address) = address {
        if !host_and_port(address) {
            return Err(fault(NodeKeyFault::Address));
        }
    }
    Ok(bytes)
}

/// AS-74, AS-75: the limit after an edit of `usd` dollars. The same dollars
/// keep the limit as it is, satoshis too; 0 turns it off; any other amount
/// is made at `rate`, satoshis to a dollar from today's price.
fn edited_limit(
    old: &Limit,
    usd: Option<u32>,
    rate: Option<u64>,
    used: &mut bool,
) -> Result<Limit, Refusal> {
    match usd {
        None => Ok(*old),
        Some(u) if u == old.display_usd => Ok(*old),
        Some(0) => Ok(Limit {
            sats: 0,
            display_usd: 0,
        }),
        Some(u) => {
            let rate = rate.ok_or(Refusal::EditNeedsRate)?;
            let sats = u64::from(u).checked_mul(rate).ok_or_else(|| {
                Refusal::SettingsInvalid("a limit is more satoshis than a number can hold".into())
            })?;
            *used = true;
            Ok(Limit {
                sats,
                display_usd: u,
            })
        }
    }
}

/// Every field once: a field that appears twice keeps the looser direction.
fn once(fields: Vec<FieldChange>) -> Vec<FieldChange> {
    let mut out: Vec<FieldChange> = Vec::new();
    for f in fields {
        match out.iter_mut().find(|o| o.field == f.field) {
            Some(o) => {
                if f.direction == Change::Loosening {
                    o.direction = Change::Loosening;
                }
            }
            None => out.push(f),
        }
    }
    out
}

impl Owner {
    /// AS-74 to AS-77: what `edit` would do to the settings. The edit is
    /// applied to the latest settings, held or queued, at the version one
    /// above every version signed so far; they are validated and sorted
    /// exactly as [`Owner::queue_settings`] sorts them. Nothing is stored
    /// and the owner is not changed.
    pub fn settings_preview(&self, edit: &SettingsEdit) -> Result<SettingsPreview, Refusal> {
        let base = self
            .state
            .latest_settings()
            .ok_or(Refusal::NoSettingsToChange)?;
        let rate = match edit.usd_per_btc {
            Some(price) => Some(sats_per_usd_at(price)?),
            None => None,
        };
        let mut used = false;
        let mut s = base.clone();
        s.version = self
            .state
            .highest_signed_version()
            .checked_add(1)
            .ok_or_else(|| Refusal::SettingsInvalid("no version is left".to_string()))?;
        let u = &mut used;
        s.light_per_sale = edited_limit(&base.light_per_sale, edit.light_per_sale, rate, u)?;
        s.light_budget = edited_limit(&base.light_budget, edit.light_budget, rate, u)?;
        s.node_per_sale = edited_limit(&base.node_per_sale, edit.node_per_sale, rate, u)?;
        s.node_budget = edited_limit(&base.node_budget, edit.node_budget, rate, u)?;
        s.box_per_sale = edited_limit(&base.box_per_sale, edit.box_per_sale, rate, u)?;
        s.box_budget = edited_limit(&base.box_budget, edit.box_budget, rate, u)?;
        s.refund_cap = edited_limit(&base.refund_cap, edit.refund_cap, rate, u)?;
        s.refund_daily_cap = edited_limit(&base.refund_daily_cap, edit.refund_daily_cap, rate, u)?;
        s.cashier_override_daily_cap = edited_limit(
            &base.cashier_override_daily_cap,
            edit.cashier_override_daily_cap,
            rate,
            u,
        )?;
        s.manager_override_ceiling = edited_limit(
            &base.manager_override_ceiling,
            edit.manager_override_ceiling,
            rate,
            u,
        )?;
        if let Some(v) = edit.underpayment_tolerance_bps {
            s.underpayment_tolerance_bps = v;
        }
        for depth in [edit.min_confirmations, edit.final_confirmations]
            .into_iter()
            .flatten()
        {
            if !(1..=MAX_EDIT_CONFIRMATIONS).contains(&depth) {
                return Err(Refusal::SettingsInvalid(
                    "a number of confirmations is outside 1 to 100".to_string(),
                ));
            }
        }
        if let Some(v) = edit.min_confirmations {
            s.min_confirmations = v;
        }
        if let Some(v) = edit.final_confirmations {
            s.final_confirmations = v;
        }
        if let Some(v) = edit.heartbeat_max_age_secs {
            s.heartbeat_max_age_secs = v;
        }
        if let Some(v) = edit.tor_on {
            s.tor_on = v;
        }
        if let Some(v) = edit.customer_wifi {
            s.customer_wifi = v;
        }
        if let Some(v) = edit.min_sale_sats {
            s.min_sale_sats = v;
        }
        if let Some(v) = edit.tips_on {
            s.tips_on = v;
        }
        if let Some(v) = &edit.tip_percents {
            s.tip_percents.clone_from(v);
        }
        for (counter, bps) in &edit.shares {
            match s.counters.get_mut(counter) {
                Some(share) => *share = *bps,
                None => {
                    return Err(Refusal::SettingsInvalid(
                        "a share is for a key that is not a counter".to_string(),
                    ))
                }
            }
        }
        match &edit.lightning_node {
            None => {}
            Some(LightningEdit::Remove) => s.lightning_node = None,
            Some(LightningEdit::Set(text)) => s.lightning_node = Some(parse_lightning_node(text)?),
        }
        match &edit.watch_key {
            None => {}
            Some(WatchEdit::Remove) => s.watch_key = None,
            Some(WatchEdit::Set(key)) => s.watch_key = Some(*key),
        }
        s.validate()
            .map_err(|e| Refusal::SettingsInvalid(e.to_string()))?;
        let known = self.known_settings();
        let reference = known.unwrap_or(base).clone();
        let change = match known {
            Some(k) => classify_change(k, &s),
            None => Change::Loosening,
        };
        let fields = once(changed_fields(&reference, &s));
        Ok(SettingsPreview {
            settings: s,
            reference,
            fields,
            change,
            price: match (used, edit.usd_per_btc, rate) {
                (true, Some(price), Some(rate)) => Some((price, rate)),
                _ => None,
            },
        })
    }
}
