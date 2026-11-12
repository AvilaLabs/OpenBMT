//! The op that previews a change of the shop's settings
//! (`docs/SESSION-RULES.md`, AS-74 to AS-77): what the owner app sends as an
//! edit, and what it is shown before it queues the change. The rules are
//! the owner library's; this file turns the edit into the library's, and
//! the library's preview into the words of a screen: dollars, percent,
//! blocks, minutes, on and off, and the node's key in full. The values are
//! written and read like those of [`views`](crate::views).

use openbmt_core::currency::{grouped, ShopCurrency};
use openbmt_core::settings::{
    Change, Limit, Settings, SignedSettings, SwapProvider, LIGHTNING_NODE_BYTES,
};
use openbmt_owner::{LightningEdit, SettingsEdit, SettingsPreview, SwapEdit};

use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::error::{refused_by, AppError};
use crate::views::{CounterShareView, ShopCurrencyView};

/// What an edit does to the Lightning node (AS-76).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightningAction {
    /// Keep the node as it is.
    Keep,
    /// Name no node.
    Remove,
    /// Name the node of the text.
    Set,
}

wire_unit_enum!(LightningAction {
    Keep = 0,
    Remove = 1,
    Set = 2
});

/// What an edit does to the swap provider (CR-218).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapAction {
    /// Keep the provider as it is.
    Keep,
    /// Name no provider.
    Remove,
    /// Name the provider of the four fields.
    Set,
}

wire_unit_enum!(SwapAction {
    Keep = 0,
    Remove = 1,
    Set = 2
});

/// What an edit does to the shop's currency (CR-198): keep it, or name one.
/// The tags of the choices are those of [`ShopCurrencyView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrencyAction {
    /// Keep the currency as it is.
    Keep,
    /// US dollar.
    Usd,
    /// Euro.
    Eur,
    /// Pound sterling.
    Gbp,
    /// Japanese yen.
    Jpy,
    /// Australian dollar.
    Aud,
    /// Indian rupee.
    Inr,
    /// South Korean won.
    Krw,
    /// Indonesian rupiah.
    Idr,
    /// Singapore dollar.
    Sgd,
    /// Bitcoin Standard: sats, no price.
    Sats,
}

wire_unit_enum!(CurrencyAction {
    Keep = 0,
    Usd = 1,
    Eur = 2,
    Gbp = 3,
    Jpy = 4,
    Aud = 5,
    Inr = 6,
    Krw = 7,
    Idr = 8,
    Sgd = 9,
    Sats = 10
});

impl CurrencyAction {
    /// The currency named, `None` for keep.
    pub(crate) fn named(self) -> Option<ShopCurrency> {
        match self {
            Self::Keep => None,
            Self::Usd => Some(ShopCurrency::Usd),
            Self::Eur => Some(ShopCurrency::Eur),
            Self::Gbp => Some(ShopCurrency::Gbp),
            Self::Jpy => Some(ShopCurrency::Jpy),
            Self::Aud => Some(ShopCurrency::Aud),
            Self::Inr => Some(ShopCurrency::Inr),
            Self::Krw => Some(ShopCurrency::Krw),
            Self::Idr => Some(ShopCurrency::Idr),
            Self::Sgd => Some(ShopCurrency::Sgd),
            Self::Sats => Some(ShopCurrency::Sats),
        }
    }
}

/// The arguments of `settings_preview`: every field optional, an absent
/// field kept (AS-74). Money is in whole units of the shop's currency (the
/// new one, when `currency` names it), 0 is off; the tolerance is in basis
/// points and the heartbeat in seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsEditArgs {
    /// Today's price of one bitcoin in whole units of the shop's currency,
    /// for the limits changed; a changed limit without it is refused
    /// (AS-75). Bitcoin Standard takes none.
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
    /// The cashier's daily override cap.
    pub cashier_override_daily_cap: Option<u32>,
    /// The manager's override ceiling.
    pub manager_override_ceiling: Option<u32>,
    /// Confirmations that settle a sale.
    pub min_confirmations: Option<u8>,
    /// Confirmations at which a sale is final.
    pub final_confirmations: Option<u8>,
    /// The longest heartbeat age that still allows instant, in seconds.
    pub heartbeat_max_age_secs: Option<u32>,
    /// The Tor switch.
    pub tor_on: Option<bool>,
    /// Customers can join the counter's Wi-Fi.
    pub customer_wifi: Option<bool>,
    /// The smallest bitcoin sale, in satoshis; 0 is none.
    pub min_sale_sats: Option<u64>,
    /// The counters ask the customer for a tip (CR-187).
    pub tips_on: Option<bool>,
    /// The suggested tips in whole percent, at most three; the whole list
    /// replaces the one held, and an empty list suggests none (CR-187).
    pub tip_percents: Option<Vec<u8>>,
    /// New shares for some counters, in basis points; the others keep theirs.
    pub shares: Vec<CounterShareView>,
    /// What to do with the Lightning node.
    pub lightning: LightningAction,
    /// The node's key or address, read only when `lightning` is `Set`.
    pub lightning_text: String,
    /// The shop's currency (CR-198): `Keep`, or the one to change to, which
    /// needs every limit again.
    pub currency: CurrencyAction,
    /// What to do with the swap provider (CR-218).
    pub swap: SwapAction,
    /// The provider's name, read only when `swap` is `Set`.
    pub swap_name: String,
    /// The provider's onion name, read only when `swap` is `Set`.
    pub swap_onion: String,
    /// The path prefix of the provider's API, read only when `swap` is `Set`.
    pub swap_prefix: String,
    /// The most the shop takes over Lightning for one sale, in satoshis,
    /// read only when `swap` is `Set`.
    pub swap_max_sats: u64,
}

wire_record_get!(SettingsEditArgs {
    usd_per_btc,
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
    tips_on,
    tip_percents,
    shares,
    lightning,
    lightning_text,
    currency,
    swap,
    swap_name,
    swap_onion,
    swap_prefix,
    swap_max_sats
});

wire_record!(SettingsEditArgs {
    usd_per_btc,
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
    tips_on,
    tip_percents,
    shares,
    lightning,
    lightning_text,
    currency,
    swap,
    swap_name,
    swap_onion,
    swap_prefix,
    swap_max_sats
});

/// One changed field, in the words of a screen (AS-77).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingChangeView {
    /// The field's name in the settings, as the library names it.
    pub field: String,
    /// What the field is, for a line of the review.
    pub label: String,
    /// The counter this is the share of, 32 bytes; none for any other field.
    pub counter: Option<Vec<u8>>,
    /// The value now, in words.
    pub old: String,
    /// The value after the change, in words.
    pub new: String,
    /// The change loosens this field: it needs the Key app.
    pub loosening: bool,
}

wire_record!(SettingChangeView {
    field,
    label,
    counter,
    old,
    new,
    loosening
});

/// What `settings_preview` found: nothing was stored (AS-77).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsPreviewView {
    /// The version the settings would have.
    pub version: u32,
    /// Each changed field once; a counter's share is a line of its own.
    pub changes: Vec<SettingChangeView>,
    /// The whole change needs the Key app's co-signature: some field loosens.
    pub needs_key_app: bool,
    /// The edit changes nothing.
    pub nothing_changed: bool,
    /// What the counters' shares add up to after the change, in basis points.
    pub shares_total_bps: u32,
    /// The price of a bitcoin in the shop's currency the changed limits were
    /// made at, when any limit was changed (AS-75).
    pub usd_per_btc: Option<u64>,
    /// The satoshis to a unit of that currency that price gives, rounded
    /// (0 when a unit is worth under half a satoshi).
    pub sats_per_usd: Option<u64>,
    /// The settings, as the bytes `queue_settings` takes.
    pub settings: Vec<u8>,
    /// The shop's currency after the change.
    pub currency: ShopCurrencyView,
}

wire_record!(SettingsPreviewView {
    version,
    changes,
    needs_key_app,
    nothing_changed,
    shares_total_bps,
    usd_per_btc,
    sats_per_usd,
    settings,
    currency
});

// ---- the edit -----------------------------------------------------------------------

/// The library's edit for the app's arguments. A share is for a counter of
/// 32 bytes and at most 65,535 basis points.
pub(crate) fn to_edit(
    a: SettingsEditArgs,
    device: impl Fn(&[u8]) -> Result<openbmt_core::identity::DeviceId, AppError>,
) -> Result<SettingsEdit, AppError> {
    let mut shares = Vec::with_capacity(a.shares.len());
    for s in &a.shares {
        let bps = u16::try_from(s.bps)
            .map_err(|_| refused_by("share-range", "a share is more than 65535 basis points"))?;
        shares.push((device(&s.counter)?, bps));
    }
    let lightning_node = match a.lightning {
        LightningAction::Keep => None,
        LightningAction::Remove => Some(LightningEdit::Remove),
        LightningAction::Set => Some(LightningEdit::Set(a.lightning_text)),
    };
    let swap = match a.swap {
        SwapAction::Keep => None,
        SwapAction::Remove => Some(SwapEdit::Remove),
        SwapAction::Set => Some(SwapEdit::Set(SwapProvider {
            name: a.swap_name,
            onion: a.swap_onion,
            path_prefix: a.swap_prefix,
            max_sale_sats: a.swap_max_sats,
        })),
    };
    Ok(SettingsEdit {
        currency: a.currency.named(),
        usd_per_btc: a.usd_per_btc,
        light_per_sale: a.light_per_sale,
        light_budget: a.light_budget,
        node_per_sale: a.node_per_sale,
        node_budget: a.node_budget,
        box_per_sale: a.box_per_sale,
        box_budget: a.box_budget,
        underpayment_tolerance_bps: a.underpayment_tolerance_bps,
        refund_cap: a.refund_cap,
        refund_daily_cap: a.refund_daily_cap,
        cashier_override_daily_cap: a.cashier_override_daily_cap,
        manager_override_ceiling: a.manager_override_ceiling,
        min_confirmations: a.min_confirmations,
        final_confirmations: a.final_confirmations,
        heartbeat_max_age_secs: a.heartbeat_max_age_secs,
        tor_on: a.tor_on,
        customer_wifi: a.customer_wifi,
        min_sale_sats: a.min_sale_sats,
        tips_on: a.tips_on,
        tip_percents: a.tip_percents,
        shares,
        lightning_node,
        swap,
        watch_key: None,
    })
}

// ---- the words of a value -------------------------------------------------------------

/// A limit changed at a price (AS-75): `$100 (101,500 sats at $98,500 a
/// bitcoin)`, the units and what they come to in satoshis. In another
/// currency the name stands where the dollar sign was: `100 euros (101,500
/// sats at 98,500 euros a bitcoin)`.
fn limit_at_price(currency: ShopCurrency, units: u32, sats: u64, price: u64) -> String {
    format!(
        "{} ({} sats at {} a bitcoin)",
        whole(currency, units),
        grouped(sats),
        currency.whole_text(price)
    )
}

/// Whole units of the currency: `$25`, `$1,250`, `25 euros`, or `off` for 0.
pub(crate) fn whole(currency: ShopCurrency, units: u32) -> String {
    if units == 0 {
        "off".to_string()
    } else {
        currency.whole_text(u64::from(units))
    }
}

/// Basis points as a percent with up to two decimals: 50 is `0.5%`, 1,000 is
/// `10%`, 3,333 is `33.33%`.
pub(crate) fn percent(bps: u32) -> String {
    let (whole, frac) = (bps / 100, bps % 100);
    match (frac, frac % 10) {
        (0, _) => format!("{whole}%"),
        (f, 0) => format!("{whole}.{}%", f / 10),
        (f, _) => format!("{whole}.{f:02}%"),
    }
}

/// A number of blocks: `1 block`, `6 blocks`.
fn blocks(n: u8) -> String {
    if n == 1 {
        "1 block".to_string()
    } else {
        format!("{n} blocks")
    }
}

/// A heartbeat age: whole minutes when it is, else seconds.
fn minutes(secs: u32) -> String {
    match (secs / 60, secs % 60) {
        (1, 0) => "1 minute".to_string(),
        (m, 0) => format!("{m} minutes"),
        _ => format!("{secs} seconds"),
    }
}

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
}

fn yes_no(yes: bool) -> String {
    if yes { "yes" } else { "no" }.to_string()
}

/// The smallest sale: `none` for 0, else satoshis.
fn sats_floor(sats: u64) -> String {
    if sats == 0 {
        "none".to_string()
    } else {
        format!("{} sats", grouped(sats))
    }
}

/// The suggested tips: `10%, 15%, 20%`, or `none` for no suggestion (CR-187).
pub(crate) fn tip_percents(percents: &[u8]) -> String {
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

/// The node's key in full, 66 lowercase digits, or `none`.
pub(crate) fn node_text(key: &Option<[u8; LIGHTNING_NODE_BYTES]>) -> String {
    match key {
        None => "none".to_string(),
        Some(key) => key.iter().map(|b| format!("{b:02x}")).collect(),
    }
}

/// The swap provider in words: its name, its onion name and the largest
/// sale over Lightning, or none.
pub(crate) fn swap_text(provider: &Option<SwapProvider>) -> String {
    match provider {
        None => "none".to_string(),
        Some(p) => format!(
            "{} at {}{}, sales up to {} sats",
            p.name,
            p.onion,
            p.path_prefix,
            grouped(p.max_sale_sats)
        ),
    }
}

/// What a field is, for a line of the review, and its old and new value in
/// words. `None` for a name that is not an editable field.
fn field_words(
    name: &str,
    old: &Settings,
    new: &Settings,
    price: Option<u64>,
) -> Option<(&'static str, String, String)> {
    // A limit that is on and changed was made at the price: say what it is
    // in satoshis; one that is off, or the same, is just its units. After a
    // change of currency every limit was made again, and so is said at the
    // price whatever its number.
    let usd = |f: fn(&Settings) -> &Limit| {
        let (before, after) = (f(old), f(new));
        let changed = after.display_usd != before.display_usd || old.currency != new.currency;
        let new_words = match price {
            Some(p) if after.display_usd != 0 && changed => {
                limit_at_price(new.currency, after.display_usd, after.sats, p)
            }
            _ => whole(new.currency, after.display_usd),
        };
        (whole(old.currency, before.display_usd), new_words)
    };
    let (label, (o, n)): (&'static str, (String, String)) = match name {
        "currency" => (
            "Currency",
            (
                old.currency.label().to_string(),
                new.currency.label().to_string(),
            ),
        ),
        "light_per_sale" => (
            "Largest instant sale, light check",
            usd(|s| &s.light_per_sale),
        ),
        "light_budget" => (
            "Instant sales at risk at once, light check",
            usd(|s| &s.light_budget),
        ),
        "node_per_sale" => ("Largest instant sale, own node", usd(|s| &s.node_per_sale)),
        "node_budget" => (
            "Instant sales at risk at once, own node",
            usd(|s| &s.node_budget),
        ),
        "box_per_sale" => ("Largest instant sale, Box only", usd(|s| &s.box_per_sale)),
        "box_budget" => (
            "Instant sales at risk at once, Box only",
            usd(|s| &s.box_budget),
        ),
        "underpayment_tolerance_bps" => (
            "Underpayment tolerance",
            (
                percent(old.underpayment_tolerance_bps),
                percent(new.underpayment_tolerance_bps),
            ),
        ),
        "refund_cap" => ("Largest single refund", usd(|s| &s.refund_cap)),
        "refund_daily_cap" => ("Most refunded in a day", usd(|s| &s.refund_daily_cap)),
        "cashier_override_daily_cap" => (
            "Cashier's override cap per day",
            usd(|s| &s.cashier_override_daily_cap),
        ),
        "manager_override_ceiling" => (
            "Manager's override ceiling",
            usd(|s| &s.manager_override_ceiling),
        ),
        "min_confirmations" => (
            "Blocks that settle a sale",
            (blocks(old.min_confirmations), blocks(new.min_confirmations)),
        ),
        "final_confirmations" => (
            "Blocks before a sale is final",
            (
                blocks(old.final_confirmations),
                blocks(new.final_confirmations),
            ),
        ),
        "heartbeat_max_age_secs" => (
            "Longest wait for this phone's heartbeat",
            (
                minutes(old.heartbeat_max_age_secs),
                minutes(new.heartbeat_max_age_secs),
            ),
        ),
        "tor_on" => ("Tor", (on_off(old.tor_on), on_off(new.tor_on))),
        "customer_wifi" => (
            "Customers can join the counter's Wi-Fi",
            (yes_no(old.customer_wifi), yes_no(new.customer_wifi)),
        ),
        "min_sale_sats" => (
            "Smallest bitcoin sale",
            (sats_floor(old.min_sale_sats), sats_floor(new.min_sale_sats)),
        ),
        "tips_on" => ("Ask for a tip", (on_off(old.tips_on), on_off(new.tips_on))),
        "tip_percents" => (
            "Tips",
            (
                tip_percents(&old.tip_percents),
                tip_percents(&new.tip_percents),
            ),
        ),
        "lightning_node" => (
            "Lightning node",
            (
                node_text(&old.lightning_node),
                node_text(&new.lightning_node),
            ),
        ),
        "swap" => (
            "Lightning through a swap provider",
            (swap_text(&old.swap), swap_text(&new.swap)),
        ),
        "watch_key" => (
            "Watch in the background",
            (
                on_off(old.watch_key.is_some()),
                on_off(new.watch_key.is_some()),
            ),
        ),
        _ => return None,
    };
    Some((label, o, n))
}

/// The shares that changed, one line for each counter, with the direction
/// of that counter's share alone.
fn share_lines(old: &Settings, new: &Settings) -> Vec<SettingChangeView> {
    new.counters
        .iter()
        .filter_map(|(counter, share)| {
            let before = old.counters.get(counter).copied().unwrap_or(0);
            (before != *share).then(|| SettingChangeView {
                field: "counters".to_string(),
                label: "Counter's share".to_string(),
                counter: Some(counter.to_bytes().to_vec()),
                old: percent(u32::from(before)),
                new: percent(u32::from(*share)),
                loosening: *share > before,
            })
        })
        .collect()
}

/// The view of a preview: each changed field in words, whether the Key app
/// must co-sign, and the settings as the bytes `queue_settings` takes (the
/// signatures in them are ignored there, AS-59).
pub(crate) fn preview_view(p: &SettingsPreview) -> SettingsPreviewView {
    let mut changes = Vec::with_capacity(p.fields.len());
    for f in &p.fields {
        if f.field == "counters" {
            changes.extend(share_lines(&p.reference, &p.settings));
        } else if let Some((label, old, new)) = field_words(
            f.field,
            &p.reference,
            &p.settings,
            p.price.map(|(price, _)| price),
        ) {
            changes.push(SettingChangeView {
                field: f.field.to_string(),
                label: label.to_string(),
                counter: None,
                old,
                new,
                loosening: f.direction == Change::Loosening,
            });
        }
    }
    let total: u32 = p.settings.counters.values().map(|s| u32::from(*s)).sum();
    let unsigned = SignedSettings {
        settings: p.settings.clone(),
        owner_sig: openbmt_core::identity::Sig([0; 64]),
        approver_sig: None,
    };
    SettingsPreviewView {
        version: p.settings.version,
        nothing_changed: changes.is_empty(),
        changes,
        needs_key_app: p.change == Change::Loosening,
        shares_total_bps: total,
        usd_per_btc: p.price.map(|(price, _)| price),
        sats_per_usd: p.price.map(|(_, rate)| rate),
        settings: unsigned.to_bytes(),
        currency: ShopCurrencyView::of(p.settings.currency),
    }
}
