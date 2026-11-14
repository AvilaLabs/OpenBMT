//! The owner phone's Shop Lightning page (`docs/SHOP-LIGHTNING-RULES.md`
//! SL-45, SL-76 to SL-78, `docs/APPROVAL-RULES.md` AP-13): what the settings
//! in force name, the newest report of the Lightning counter, and the
//! preview of a change of the field. The rules are the owner library's and
//! the link's; this file turns the app's arguments into the library's edit
//! and gives back facts, never sentences: the words stand in the app.

use openbmt_core::approval::{classify, ChangeClass};
use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{ShopLightning, SignedSettings};
use openbmt_link::shopln::NotOffered;
use openbmt_owner::{
    SettingsEdit, SettingsPreview, ShopLightningChange, ShopLightningEdit, ShopLightningSet,
};
use openbmt_owner_node::TakenReport;

use crate::codec::{wire_record, wire_record_get, wire_unit_enum};
use crate::error::{refused_by, AppError};

/// Why Lightning is not offered, as the report says it (LK-84, SL-41).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotOfferedView {
    /// It is offered.
    Offered,
    /// The second device is away.
    SecondDeviceAway,
    /// The chain data is behind.
    ChainBehind,
    /// No channel can be used.
    NoUsableChannel,
    /// The provider does not answer.
    ProviderSilent,
}

wire_unit_enum!(NotOfferedView {
    Offered = 0,
    SecondDeviceAway = 1,
    ChainBehind = 2,
    NoUsableChannel = 3,
    ProviderSilent = 4
});

impl NotOfferedView {
    fn of(n: NotOffered) -> Self {
        match n {
            NotOffered::Offered => Self::Offered,
            NotOffered::SecondDeviceAway => Self::SecondDeviceAway,
            NotOffered::ChainBehind => Self::ChainBehind,
            NotOffered::NoUsableChannel => Self::NoUsableChannel,
            NotOffered::ProviderSilent => Self::ProviderSilent,
        }
    }
}

/// The Shop Lightning the settings in force name (SL-71): the cap and the
/// provider the page shows are these (SL-78).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopLightningHeldView {
    /// The Lightning counter, 32 bytes.
    pub counter: Vec<u8>,
    /// The node's ID, 33 bytes.
    pub node_id: Vec<u8>,
    /// The Box that keeps the second copy, 32 bytes.
    pub second_device: Vec<u8>,
    /// The provider's node ID, 33 bytes.
    pub provider_node_id: Vec<u8>,
    /// The provider's IP address and port as text.
    pub provider_addr: String,
    /// The cap, in satoshis.
    pub cap_sats: u64,
    /// The most for one opening, in satoshis.
    pub max_opening_fee_sats: u64,
}

wire_record!(ShopLightningHeldView {
    counter,
    node_id,
    second_device,
    provider_node_id,
    provider_addr,
    cap_sats,
    max_opening_fee_sats
});

impl ShopLightningHeldView {
    fn of(sl: &ShopLightning) -> Self {
        Self {
            counter: sl.counter.to_bytes().to_vec(),
            node_id: sl.node_id.to_vec(),
            second_device: sl.second_device.to_bytes().to_vec(),
            provider_node_id: sl.provider_node_id.to_vec(),
            provider_addr: sl.provider_addr.to_string(),
            cap_sats: sl.cap_sats,
            max_opening_fee_sats: sl.max_opening_fee_sats,
        }
    }
}

/// The newest report of the Lightning counter and when it came (SL-78,
/// LK-84).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightningReportView {
    /// When the link took it, in seconds since 1970.
    pub taken_at: u64,
    /// The Lightning balance, in satoshis.
    pub balance_sats: u64,
    /// The room to receive, in satoshis.
    pub receive_room_sats: u64,
    /// The opening fees paid so far, in satoshis.
    pub opening_fees_paid_sats: u64,
    /// When the Box last stored the state; 0 if it never has.
    pub last_stored_at: u64,
    /// The time of the last sweep; 0 if there was none.
    pub last_sweep_at: u64,
    /// Why Lightning is not offered, or that it is.
    pub not_offered: NotOfferedView,
}

wire_record!(LightningReportView {
    taken_at,
    balance_sats,
    receive_room_sats,
    opening_fees_paid_sats,
    last_stored_at,
    last_sweep_at,
    not_offered
});

impl LightningReportView {
    pub(crate) fn of(t: &TakenReport) -> Self {
        Self {
            taken_at: t.taken_at,
            balance_sats: t.report.balance_sats,
            receive_room_sats: t.report.receive_room_sats,
            opening_fees_paid_sats: t.report.opening_fees_paid_sats,
            last_stored_at: t.report.last_stored_at,
            last_sweep_at: t.report.last_sweep_at,
            not_offered: NotOfferedView::of(t.report.not_offered),
        }
    }
}

/// What the Shop Lightning page shows (SL-45, SL-78).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopLightningView {
    /// Shop Lightning is named by the settings in force.
    pub on: bool,
    /// The shop uses its own wallet: its owner phone has no way to carry a
    /// change of Shop Lightning (SL-76).
    pub own_wallet: bool,
    /// The shop's counters, 32 bytes each, to pick the Lightning counter
    /// from.
    pub counters: Vec<Vec<u8>>,
    /// The shop's Boxes, 32 bytes each (SL-73). None means Shop Lightning
    /// cannot be turned on.
    pub boxes: Vec<Vec<u8>>,
    /// What the settings in force name, when Shop Lightning is on.
    pub held: Option<ShopLightningHeldView>,
    /// The newest report of the Lightning counter, if one came.
    pub report: Option<LightningReportView>,
}

wire_record!(ShopLightningView {
    on,
    own_wallet,
    counters,
    boxes,
    held,
    report
});

/// What an edit of Shop Lightning does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopLightningAction {
    /// Name the whole block: turn on, or move to another counter or node.
    Set,
    /// Change the cap, the most for one opening, the provider or the Box of
    /// a Shop Lightning that is on.
    Change,
    /// Turn off: the field is taken away.
    Remove,
}

wire_unit_enum!(ShopLightningAction {
    Set = 0,
    Change = 1,
    Remove = 2
});

/// The arguments of `shop_lightning_preview`. A field a step does not use is
/// left absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopLightningEditArgs {
    /// What the edit does.
    pub action: ShopLightningAction,
    /// Set: the Lightning counter, 32 bytes.
    pub counter: Option<Vec<u8>>,
    /// Set: the node's ID as the stick's code gives it, 66 lower-case
    /// hexadecimal digits.
    pub node_id: Option<String>,
    /// Set or Change: the Box, 32 bytes; absent takes the shop's Box (Set)
    /// or keeps it (Change).
    pub second_device: Option<Vec<u8>>,
    /// Set or Change: the provider's node ID as 66 hexadecimal digits.
    pub provider_node_id: Option<String>,
    /// Set or Change: the provider's IP address and port.
    pub provider_addr: Option<String>,
    /// Set: the cap in satoshis. Change: the new cap, absent keeps it.
    pub cap_sats: Option<u64>,
    /// Set: the most for one opening. Change: the new one, absent keeps it.
    pub max_opening_fee_sats: Option<u64>,
}

wire_record!(ShopLightningEditArgs {
    action,
    counter,
    node_id,
    second_device,
    provider_node_id,
    provider_addr,
    cap_sats,
    max_opening_fee_sats
});
wire_record_get!(ShopLightningEditArgs {
    action,
    counter,
    node_id,
    second_device,
    provider_node_id,
    provider_addr,
    cap_sats,
    max_opening_fee_sats
});

/// The class of a change of the settings (AP-01, AP-02, AP-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopLightningClass {
    /// The owner phone alone.
    Free,
    /// Loosens: a counter's confirmation, or the Key app.
    Loosen,
    /// Redirects: the wallet's signature, or the Key app.
    Redirect,
}

wire_unit_enum!(ShopLightningClass {
    Free = 0,
    Loosen = 1,
    Redirect = 2
});

impl ShopLightningClass {
    fn of(c: ChangeClass) -> Self {
        match c {
            ChangeClass::Free => Self::Free,
            ChangeClass::Loosen => Self::Loosen,
            ChangeClass::Redirect => Self::Redirect,
        }
    }
}

/// What `shop_lightning_preview` found: nothing was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopLightningPreviewView {
    /// The version the settings would have.
    pub version: u32,
    /// The class of the change, as the approval code gives it (AP-13).
    pub class: ShopLightningClass,
    /// The change needs the Key app's co-signature.
    pub needs_key_app: bool,
    /// The edit changes nothing.
    pub nothing_changed: bool,
    /// What Shop Lightning would be after the change: absent when it would
    /// be off.
    pub after: Option<ShopLightningHeldView>,
    /// The settings, as the bytes `queue_settings` takes.
    pub settings: Vec<u8>,
}

wire_record!(ShopLightningPreviewView {
    version,
    class,
    needs_key_app,
    nothing_changed,
    after,
    settings
});

fn device(what: &str, bytes: &[u8]) -> Result<DeviceId, AppError> {
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| refused_by("id-length", format!("{what} must be 32 bytes")))?;
    DeviceId::from_bytes(&array)
        .map_err(|_| refused_by("id-length", format!("{what} is not a key")))
}

fn missing(what: &str) -> AppError {
    refused_by(
        "request-malformed",
        format!("{what} is needed for this step"),
    )
}

/// The library's edit for the app's arguments.
pub(crate) fn to_edit(a: ShopLightningEditArgs) -> Result<SettingsEdit, AppError> {
    let second_device = a
        .second_device
        .as_deref()
        .map(|b| device("the Box", b))
        .transpose()?;
    let provider = match (a.provider_node_id, a.provider_addr) {
        (Some(key), Some(addr)) => Some((key, addr)),
        (None, None) => None,
        _ => return Err(missing("the provider's ID with its address")),
    };
    let edit = match a.action {
        ShopLightningAction::Remove => ShopLightningEdit::Remove,
        ShopLightningAction::Change => ShopLightningEdit::Change(ShopLightningChange {
            cap_sats: a.cap_sats,
            max_opening_fee_sats: a.max_opening_fee_sats,
            provider,
            second_device,
        }),
        ShopLightningAction::Set => {
            let (provider_node_id, provider_addr) =
                provider.ok_or_else(|| missing("the provider"))?;
            ShopLightningEdit::Set(ShopLightningSet {
                counter: device(
                    "the Lightning counter",
                    &a.counter.ok_or_else(|| missing("the Lightning counter"))?,
                )?,
                node_id: a.node_id.ok_or_else(|| missing("the node's ID"))?,
                second_device,
                provider_node_id,
                provider_addr,
                cap_sats: a.cap_sats.ok_or_else(|| missing("the cap"))?,
                max_opening_fee_sats: a
                    .max_opening_fee_sats
                    .ok_or_else(|| missing("the most for one opening"))?,
            })
        }
    };
    Ok(SettingsEdit {
        shop_lightning: Some(edit),
        ..SettingsEdit::default()
    })
}

/// The view of a preview of an edit of Shop Lightning.
pub(crate) fn preview_view(p: &SettingsPreview) -> ShopLightningPreviewView {
    let class = classify(&p.reference, &p.settings);
    let unsigned = SignedSettings {
        settings: p.settings.clone(),
        owner_sig: openbmt_core::identity::Sig([0; 64]),
        approver_sig: None,
    };
    ShopLightningPreviewView {
        version: p.settings.version,
        class: ShopLightningClass::of(class),
        needs_key_app: p.change == openbmt_core::settings::Change::Loosening,
        nothing_changed: p.fields.is_empty(),
        after: p
            .settings
            .shop_lightning
            .as_ref()
            .map(ShopLightningHeldView::of),
        settings: unsigned.to_bytes(),
    }
}

/// The Shop Lightning page's facts.
pub(crate) fn view(
    own_wallet: bool,
    settings: Option<&openbmt_core::settings::Settings>,
    report: Option<&TakenReport>,
) -> ShopLightningView {
    let held = settings.and_then(|s| s.shop_lightning.as_ref());
    ShopLightningView {
        on: held.is_some(),
        own_wallet,
        counters: settings
            .map(|s| s.counters.keys().map(|c| c.to_bytes().to_vec()).collect())
            .unwrap_or_default(),
        boxes: settings
            .map(|s| s.boxes.iter().map(|b| b.to_bytes().to_vec()).collect())
            .unwrap_or_default(),
        held: held.map(ShopLightningHeldView::of),
        report: report.map(LightningReportView::of),
    }
}
