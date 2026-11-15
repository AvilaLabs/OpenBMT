//! The owner phone's Redirect change of a shop with its own wallet (ops 243
//! and 244, `docs/APPROVAL-RULES.md` AP-14, AP-15, `docs/OWNER-APP-RULES.md`
//! OA-127): the preview of an edit, which says whether the wallet has to sign
//! it, with the message and the address, and the call that attaches the
//! signature, checks it and puts the settings in force. Like the other ops of
//! a shop with its own wallet they work on no node: each is given the shop's
//! folder, the owner root secret and the wallet's file.

use openbmt_owner::own_shop::{OwnError, OwnRefusal};
use openbmt_owner::OwnerError;

use crate::codec::{wire_record, wire_record_get};
use crate::error::{refused_by, AppError};
use crate::host::queue_error;
use crate::own::{device, own_error, with_shop, OwnShopArgs};
use crate::settings_edit::{preview_view, to_edit, SettingsEditArgs, SettingsPreviewView};
use crate::shop_lightning::{ShopLightningEditArgs, ShopLightningPreviewView};

/// The arguments of `own_settings_preview`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSettingsPreviewArgs {
    /// The shop.
    pub shop: OwnShopArgs,
    /// The edit, as for `settings_preview`.
    pub edit: SettingsEditArgs,
    /// The shop is on its starter wallet (AP-15): the app knows.
    pub starter: bool,
}

wire_record!(OwnSettingsPreviewArgs {
    shop,
    edit,
    starter
});
wire_record_get!(OwnSettingsPreviewArgs {
    shop,
    edit,
    starter
});

/// What `own_settings_preview` found: nothing was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSettingsPreviewView {
    /// The preview as `settings_preview` gives it.
    pub preview: SettingsPreviewView,
    /// The change is a Redirect and needs the wallet's signature (AP-14).
    pub needs_wallet_sig: bool,
    /// The message to sign (AP-06); empty when no signature is needed.
    pub message: String,
    /// The address that has to sign (AP-09); empty when no signature is
    /// needed.
    pub address: String,
}

wire_record!(OwnSettingsPreviewView {
    preview,
    needs_wallet_sig,
    message,
    address
});

/// The arguments of `own_settings_attach`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSettingsAttachArgs {
    /// The shop.
    pub shop: OwnShopArgs,
    /// The settings of the preview, as its bytes.
    pub settings: Vec<u8>,
    /// The wallet's signature as the person pasted or scanned it.
    pub signature: String,
}

wire_record!(OwnSettingsAttachArgs {
    shop,
    settings,
    signature
});
wire_record_get!(OwnSettingsAttachArgs {
    shop,
    settings,
    signature
});

/// Op 243: what an edit does to the settings of a shop with its own wallet.
pub(crate) fn preview(a: OwnSettingsPreviewArgs) -> Result<OwnSettingsPreviewView, AppError> {
    let mut edit = to_edit(a.edit, |b| device("a counter's root", b))?;
    edit.starter_wallet = a.starter;
    with_shop(a.shop, |node| {
        let preview = node
            .shop()
            .settings_preview(&edit)
            .map_err(|r| queue_error(&OwnerError::Refused(r)))?;
        let (needs_wallet_sig, message, address) = match &preview.wallet_sig {
            Some(ask) => (true, ask.message.clone(), ask.address.clone()),
            None => (false, String::new(), String::new()),
        };
        Ok(OwnSettingsPreviewView {
            preview: preview_view(&preview),
            needs_wallet_sig,
            message,
            address,
        })
    })
}

/// Op 244: attach the wallet's signature to the settings of a preview, check
/// it, and put the settings in force. A signature that does not verify
/// refuses with the address that had to sign and stores nothing. The answer
/// is the version in force.
pub(crate) fn attach(a: OwnSettingsAttachArgs) -> Result<u32, AppError> {
    let signed = openbmt_core::settings::SignedSettings::from_bytes(&a.settings)
        .map_err(|e| refused_by("settings-bytes", format!("not settings: {e}")))?;
    with_shop(a.shop, |node| {
        let (shop, store) = node.shop_and_store();
        shop.change_settings_with_wallet_sig(signed.settings, &a.signature, &store)
            .map(|s| s.settings.version)
            .map_err(own_error)
    })
}

/// The arguments of `own_shop_lightning_preview`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnShopLightningPreviewArgs {
    /// The shop.
    pub shop: OwnShopArgs,
    /// The Shop Lightning edit, as for `shop_lightning_preview`.
    pub edit: ShopLightningEditArgs,
    /// The shop is on its starter wallet (AP-15): the app knows.
    pub starter: bool,
}

wire_record!(OwnShopLightningPreviewArgs {
    shop,
    edit,
    starter
});
wire_record_get!(OwnShopLightningPreviewArgs {
    shop,
    edit,
    starter
});

/// What `own_shop_lightning_preview` found: nothing was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnShopLightningPreviewView {
    /// The preview as `shop_lightning_preview` gives it.
    pub preview: ShopLightningPreviewView,
    /// The change needs the wallet's signature (AP-14).
    pub needs_wallet_sig: bool,
    /// The message to sign (AP-06); empty when no signature is needed.
    pub message: String,
    /// The address that has to sign (AP-09); empty when none is needed.
    pub address: String,
}

wire_record!(OwnShopLightningPreviewView {
    preview,
    needs_wallet_sig,
    message,
    address
});

/// Op 245: what a Shop Lightning edit does to the settings of a shop with
/// its own wallet, with what the wallet has to sign (AP-14, AP-15).
pub(crate) fn shop_lightning_preview(
    a: OwnShopLightningPreviewArgs,
) -> Result<OwnShopLightningPreviewView, AppError> {
    let mut edit = crate::shop_lightning::to_edit(a.edit)?;
    edit.starter_wallet = a.starter;
    with_shop(a.shop, |node| {
        let preview = node
            .shop()
            .settings_preview(&edit)
            .map_err(|r| queue_error(&OwnerError::Refused(r)))?;
        let (needs_wallet_sig, message, address) = match &preview.wallet_sig {
            Some(ask) => (true, ask.message.clone(), ask.address.clone()),
            None => (false, String::new(), String::new()),
        };
        Ok(OwnShopLightningPreviewView {
            preview: crate::shop_lightning::preview_view(&preview),
            needs_wallet_sig,
            message,
            address,
        })
    })
}

/// The arguments of `own_settings_send`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSettingsSendArgs {
    /// The shop.
    pub shop: OwnShopArgs,
    /// The settings of the preview, as its bytes.
    pub settings: Vec<u8>,
    /// The time now, in Unix seconds.
    pub now: u64,
}

wire_record!(OwnSettingsSendArgs {
    shop,
    settings,
    now
});
wire_record_get!(OwnSettingsSendArgs {
    shop,
    settings,
    now
});

/// What `own_settings_send` did with the change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnSettingsSentView {
    /// The version of the change.
    pub version: u32,
    /// The change waits for a counter's confirmation (CR-239) and is not in
    /// force yet.
    pub waits: bool,
}

wire_record!(OwnSettingsSentView { version, waits });

/// Op 246: a change that needs no wallet signature (Free or Loosen) of a shop
/// with its own wallet, signed by the owner root alone: it is put in force, or
/// held as the waiting change when a counter must confirm it. A Redirect
/// change is refused here (AP-14): it goes through op 244 with the wallet's
/// signature.
pub(crate) fn send(a: OwnSettingsSendArgs) -> Result<OwnSettingsSentView, AppError> {
    let signed = openbmt_core::settings::SignedSettings::from_bytes(&a.settings)
        .map_err(|e| refused_by("settings-bytes", format!("not settings: {e}")))?;
    if signed.wallet_sig.is_some() {
        return Err(settings_refused(
            "AP-14: a signature of the wallet belongs only to a Redirect change",
        ));
    }
    with_shop(a.shop, |node| {
        if let Some(known) = node.shop().known_settings() {
            if openbmt_core::approval::classify(&known.settings, &signed.settings)
                == openbmt_core::approval::ChangeClass::Redirect
            {
                return Err(settings_refused(
                    "AP-14: a change of where payments go needs the signature of the wallet",
                ));
            }
        }
        let waiting = node.waiting_writer();
        let (shop, store) = node.shop_and_store();
        shop.change_settings_with(signed.settings, a.now, |b| store(b), waiting)
            .map(|(s, waits)| OwnSettingsSentView {
                version: s.settings.version,
                waits,
            })
            .map_err(own_error)
    })
}

/// A refusal of a change as the library words a refused change of settings.
fn settings_refused(text: &str) -> AppError {
    own_error(OwnError::Refused(OwnRefusal::Settings(
        openbmt_owner::Refusal::SettingsInvalid(text.to_string()),
    )))
}
