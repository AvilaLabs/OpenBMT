//! Turning Shop Lightning on, changing it and turning it off from the owner
//! phone: its class, its checks and the way it is queued (SL-71, SL-73,
//! SL-76, SL-77, AP-13).

mod common;

use common::*;
use openbmt_core::approval::{classify, ChangeClass};
use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{Change, Settings};
use openbmt_owner::{
    Refusal, SettingsEdit, SettingsQueued, ShopLightningChange, ShopLightningEdit,
    ShopLightningRefusal, ShopLightningSet,
};

const NODE: &str = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const PROVIDER: &str = "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

fn boxed() -> DeviceId {
    identity([0x61; 32]).device_id()
}

/// A rig whose settings in force have two counters and, when `with_box`, a Box.
fn held(with_box: bool) -> Rig {
    let mut rig = Rig::new();
    let mut s = settings_with(1, &[1, 2]);
    if with_box {
        s.devices.insert(boxed());
        s.boxes.insert(boxed());
    }
    ok(rig.owner.queue_settings(s, nop));
    rig.round(&[], 0);
    rig
}

fn set() -> ShopLightningSet {
    ShopLightningSet {
        counter: counter(1).device_id(),
        node_id: NODE.to_string(),
        second_device: None,
        provider_node_id: PROVIDER.to_string(),
        provider_addr: "203.0.113.7:9735".to_string(),
        cap_sats: 5_000_000,
        max_opening_fee_sats: 20_000,
    }
}

fn edit(e: ShopLightningEdit) -> SettingsEdit {
    SettingsEdit {
        shop_lightning: Some(e),
        ..SettingsEdit::default()
    }
}

/// The rig after Shop Lightning was turned on and approved.
fn on() -> Rig {
    let mut rig = held(true);
    let preview = rig
        .owner
        .settings_preview(&edit(ShopLightningEdit::Set(set())))
        .unwrap();
    ok(rig.owner.queue_settings(preview.settings, nop));
    rig.round(&[], 1);
    assert!(rig.owner.known_settings().unwrap().shop_lightning.is_some());
    rig
}

fn class_of(rig: &Rig, e: ShopLightningEdit) -> (ChangeClass, Change, Settings) {
    let p = rig.owner.settings_preview(&edit(e)).unwrap();
    (classify(&p.reference, &p.settings), p.change, p.settings)
}

#[test]
fn sl_76_turning_it_on_is_a_redirect_that_the_key_app_approves() {
    let mut rig = held(true);
    let (class, change, settings) = class_of(&rig, ShopLightningEdit::Set(set()));
    assert_eq!(class, ChangeClass::Redirect);
    assert_eq!(change, Change::Loosening);
    let sl = settings.shop_lightning.unwrap();
    assert_eq!(sl.counter, counter(1).device_id());
    assert_eq!(sl.second_device, boxed());
    assert_eq!(sl.cap_sats, 5_000_000);
    assert_eq!(sl.max_opening_fee_sats, 20_000);
    assert_eq!(sl.provider_addr.to_string(), "203.0.113.7:9735");
    let queued = ok(rig.owner.queue_settings(settings, nop));
    assert!(matches!(queued, SettingsQueued::Queued(_)));
    rig.round(&[], 1);
    assert!(rig.owner.known_settings().unwrap().shop_lightning.is_some());
}

#[test]
fn sl_73_a_shop_without_a_box_cannot_turn_it_on() {
    let rig = held(false);
    assert_eq!(
        rig.owner
            .settings_preview(&edit(ShopLightningEdit::Set(set())))
            .unwrap_err(),
        Refusal::ShopLightning(ShopLightningRefusal::NoBox)
    );
}

#[test]
fn sl_73_a_second_device_that_is_not_a_box_of_the_shop_is_refused() {
    let rig = held(true);
    let mut s = set();
    s.second_device = Some(counter(2).device_id());
    assert_eq!(
        rig.owner
            .settings_preview(&edit(ShopLightningEdit::Set(s)))
            .unwrap_err(),
        Refusal::ShopLightning(ShopLightningRefusal::BoxNotInShop)
    );
}

#[test]
fn sl_71_each_broken_field_is_refused_with_its_own_reason() {
    let rig = held(true);
    type Break = Box<dyn Fn(&mut ShopLightningSet)>;
    let cases: Vec<(Break, ShopLightningRefusal)> = vec![
        (
            Box::new(|s| s.counter = counter(9).device_id()),
            ShopLightningRefusal::CounterNotInShop,
        ),
        (
            Box::new(|s| s.node_id = NODE.to_ascii_uppercase()),
            ShopLightningRefusal::NodeId,
        ),
        (
            Box::new(|s| s.node_id = NODE[..64].to_string()),
            ShopLightningRefusal::NodeId,
        ),
        (
            Box::new(|s| s.provider_node_id = format!("02{}", "00".repeat(32))),
            ShopLightningRefusal::ProviderKey,
        ),
        (
            Box::new(|s| s.provider_addr = "provider.example:9735".to_string()),
            ShopLightningRefusal::ProviderAddress,
        ),
        (
            Box::new(|s| s.provider_addr = "203.0.113.7:0".to_string()),
            ShopLightningRefusal::ProviderAddress,
        ),
        (Box::new(|s| s.cap_sats = 0), ShopLightningRefusal::CapZero),
    ];
    for (break_it, why) in cases {
        let mut s = set();
        break_it(&mut s);
        assert_eq!(
            rig.owner
                .settings_preview(&edit(ShopLightningEdit::Set(s)))
                .unwrap_err(),
            Refusal::ShopLightning(why),
            "{why:?}"
        );
    }
}

#[test]
fn sl_71_it_is_never_set_with_a_lightning_node_or_a_swap_provider() {
    let mut rig = Rig::new();
    let mut s = settings_with(1, &[1, 2]);
    s.devices.insert(boxed());
    s.boxes.insert(boxed());
    s.lightning_node = Some(openbmt_owner::parse_lightning_node(NODE).unwrap());
    ok(rig.owner.queue_settings(s, nop));
    rig.round(&[], 0);
    assert_eq!(
        rig.owner
            .settings_preview(&edit(ShopLightningEdit::Set(set())))
            .unwrap_err(),
        Refusal::ShopLightning(ShopLightningRefusal::OtherLightning)
    );
    // Taking the node away in the same edit is allowed.
    let both = SettingsEdit {
        lightning_node: Some(openbmt_owner::LightningEdit::Remove),
        shop_lightning: Some(ShopLightningEdit::Set(set())),
        ..SettingsEdit::default()
    };
    let p = rig.owner.settings_preview(&both).unwrap();
    assert!(p.settings.lightning_node.is_none() && p.settings.shop_lightning.is_some());
}

#[test]
fn ap_13_the_cap_and_the_fee_loosen_when_raised_and_are_free_when_lowered() {
    let rig = on();
    let change = |cap: Option<u64>, fee: Option<u64>| {
        class_of(
            &rig,
            ShopLightningEdit::Change(ShopLightningChange {
                cap_sats: cap,
                max_opening_fee_sats: fee,
                ..ShopLightningChange::default()
            }),
        )
    };
    for (cap, fee, class, direction) in [
        (
            Some(6_000_000),
            None,
            ChangeClass::Loosen,
            Change::Loosening,
        ),
        (None, Some(30_000), ChangeClass::Loosen, Change::Loosening),
        (Some(4_000_000), None, ChangeClass::Free, Change::Tightening),
        (None, Some(10_000), ChangeClass::Free, Change::Tightening),
        (
            Some(4_000_000),
            Some(30_000),
            ChangeClass::Loosen,
            Change::Loosening,
        ),
    ] {
        let (c, d, s) = change(cap, fee);
        assert_eq!((c, d), (class, direction), "{cap:?} {fee:?}");
        assert!(s.shop_lightning.is_some());
    }
    let (_, _, same) = change(Some(5_000_000), None);
    assert_eq!(
        same.shop_lightning,
        rig.owner.known_settings().unwrap().shop_lightning
    );
    assert_eq!(
        rig.owner
            .settings_preview(&edit(ShopLightningEdit::Change(ShopLightningChange {
                cap_sats: Some(0),
                ..ShopLightningChange::default()
            })))
            .unwrap_err(),
        Refusal::ShopLightning(ShopLightningRefusal::CapZero)
    );
}

#[test]
fn ap_13_the_provider_and_the_box_loosen() {
    let mut rig = Rig::new();
    let other_box = identity([0x62; 32]).device_id();
    let mut s = settings_with(1, &[1, 2]);
    for b in [boxed(), other_box] {
        s.devices.insert(b);
        s.boxes.insert(b);
    }
    ok(rig.owner.queue_settings(s, nop));
    rig.round(&[], 0);
    let p = rig
        .owner
        .settings_preview(&edit(ShopLightningEdit::Set(ShopLightningSet {
            second_device: Some(boxed()),
            ..set()
        })))
        .unwrap();
    ok(rig.owner.queue_settings(p.settings, nop));
    rig.round(&[], 1);
    let (class, _, _) = class_of(
        &rig,
        ShopLightningEdit::Change(ShopLightningChange {
            provider: Some((NODE.to_string(), "198.51.100.2:9735".to_string())),
            ..ShopLightningChange::default()
        }),
    );
    assert_eq!(class, ChangeClass::Loosen);
    let (class, _, s) = class_of(
        &rig,
        ShopLightningEdit::Change(ShopLightningChange {
            second_device: Some(other_box),
            ..ShopLightningChange::default()
        }),
    );
    assert_eq!(class, ChangeClass::Loosen);
    assert_eq!(s.shop_lightning.unwrap().second_device, other_box);
    // Moving to another counter is a redirect again.
    let (class, _, _) = class_of(
        &rig,
        ShopLightningEdit::Set(ShopLightningSet {
            counter: counter(2).device_id(),
            second_device: Some(boxed()),
            ..set()
        }),
    );
    assert_eq!(class, ChangeClass::Redirect);
}

#[test]
fn sl_77_turning_it_off_is_free_and_signed_by_the_owner_alone() {
    let mut rig = on();
    let (class, change, settings) = class_of(&rig, ShopLightningEdit::Remove);
    assert_eq!((class, change), (ChangeClass::Free, Change::Tightening));
    assert!(settings.shop_lightning.is_none());
    let queued = ok(rig.owner.queue_settings(settings, nop));
    assert!(matches!(queued, SettingsQueued::Tightening { .. }));
    assert!(rig.owner.known_settings().unwrap().shop_lightning.is_none());
}

#[test]
fn sl_71_changing_it_while_it_is_off_is_refused_and_turning_it_off_again_changes_nothing() {
    let rig = held(true);
    assert_eq!(
        rig.owner
            .settings_preview(&edit(ShopLightningEdit::Change(
                ShopLightningChange::default()
            )))
            .unwrap_err(),
        Refusal::ShopLightning(ShopLightningRefusal::NotOn)
    );
    let p = rig
        .owner
        .settings_preview(&edit(ShopLightningEdit::Remove))
        .unwrap();
    assert!(p.fields.is_empty());
}

#[test]
fn sl_76_a_preview_stores_nothing() {
    let rig = held(true);
    let before = rig.owner.snapshot().unwrap();
    let calls = rig.owner_store.calls();
    let _ = rig
        .owner
        .settings_preview(&edit(ShopLightningEdit::Set(set())));
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert_eq!(rig.owner_store.calls(), calls);
}

#[test]
fn sl_76_a_shop_with_its_own_wallet_has_no_way_to_carry_it_and_is_refused() {
    use bitcoin::bip32::{Xpriv, Xpub};
    use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
    use openbmt_owner::own_shop::own_approver;
    let master = Xpriv::new_master(bitcoin::Network::Bitcoin, &[0x77; 32]).unwrap();
    let path: bitcoin::bip32::DerivationPath = "m/84'/0'/0'".parse().unwrap();
    let account = master.derive_priv(secp(), &path).unwrap();
    let wallet = SingleKeyWallet::new(
        master.fingerprint(secp()),
        0,
        Xpub::from_priv(secp(), &account),
    )
    .unwrap();
    let owner = openbmt_owner::Owner::new(
        &OWNER_SECRET,
        ImportedWallet::Single(wallet),
        own_approver(&OWNER_SECRET).unwrap(),
        policy(),
        openbmt_owner::Options::default(),
    )
    .unwrap();
    for e in [
        ShopLightningEdit::Set(set()),
        ShopLightningEdit::Remove,
        ShopLightningEdit::Change(ShopLightningChange::default()),
    ] {
        assert_eq!(
            owner.settings_preview(&edit(e)).unwrap_err(),
            Refusal::ShopLightning(ShopLightningRefusal::OwnWallet)
        );
    }
}
