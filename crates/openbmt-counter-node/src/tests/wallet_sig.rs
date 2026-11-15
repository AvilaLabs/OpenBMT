//! CR-236, AP-14, AP-15: a Redirect change of a shop with its own wallet
//! comes with the wallet's signature, and the counter checks it against the
//! wallet it pinned. The node of a counter paired with the shop's own wallet
//! is started on a fake driver.

use bitcoin::secp256k1::SecretKey;
use openbmt_core::approval::test_signing::{
    cosigning_wallet, sign_bip322_text, sign_compact_text, signing_wallet,
};
use openbmt_core::approval::{join_cosigner_sigs, wallet_message_for, WalletSigError};
use openbmt_core::settings::{
    Change, Settings, SettingsError, ShopLightning, SignedSettings, LIGHTNING_NODE_BYTES,
};
use openbmt_core::shop_lightning::node_id;

use super::kit::*;
use super::paired_own::record;
use super::switch::rig_for;
use crate::dirstore::CONFIRM_FILE;
use crate::settings::{effective_of, SettingsRefused};
use crate::{DirStore, PairedRecord};

fn node_key() -> [u8; LIGHTNING_NODE_BYTES] {
    node_id(&std::array::from_fn::<u8, 64, _>(|i| {
        9u8.wrapping_add(i as u8)
    }))
}

fn redirect(version: u32) -> Settings {
    let mut s = settings();
    s.version = version;
    s.lightning_node = Some(node_key());
    s
}

fn shop_lightning(version: u32) -> Settings {
    let mut s = settings();
    s.version = version;
    let counter = id(60).device_id();
    let boxed = id(61).device_id();
    s.devices.insert(counter);
    s.devices.insert(boxed);
    s.counters.insert(id(3).device_id(), 9_500);
    s.counters.insert(counter, 500);
    s.boxes.insert(boxed);
    s.shop_lightning = Some(ShopLightning {
        counter,
        node_id: node_id(&std::array::from_fn::<u8, 64, _>(|i| {
            3u8.wrapping_add(i as u8)
        })),
        second_device: boxed,
        provider_node_id: node_key(),
        provider_addr: "127.0.0.1:9".parse().unwrap(),
        cap_sats: 100_000,
        max_opening_fee_sats: 3_000,
    });
    s
}

fn signed(s: &Settings, wallet_sig: Option<String>) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s.clone(),
        wallet_sig,
    }
}

/// A record paired with the wallet of `seed` (a starter wallet when
/// `starter`) and the key of its first address.
fn paired(seed: u8, starter: bool) -> (crate::OwnPairingRecord, SecretKey) {
    let (wallet, key) = signing_wallet(seed);
    let mut r = record();
    r.wallet = wallet;
    r.starter = starter;
    (r, key)
}

fn confirm_exists(dir: &std::path::Path) -> bool {
    dir.join(CONFIRM_FILE).exists()
}

#[test]
fn cr_236_a_redirect_with_the_wallets_signature_comes_into_force() {
    for bip322 in [false, true] {
        let dir = Scratch::new();
        let (record, key) = paired(31, false);
        let mut rig = rig_for(dir.path(), &record);
        let s = redirect(2);
        let message = wallet_message_for(&s);
        let sig = if bip322 {
            sign_bip322_text(&key, &message)
        } else {
            sign_compact_text(&key, &message)
        };
        let change = signed(&s, Some(sig));
        assert_eq!(rig.node.take_settings(&change), Ok(Some(Change::Loosening)));
        assert_eq!(rig.node.settings().version, 2);
        assert_eq!(rig.node.settings().lightning_node, Some(node_key()));
        assert_eq!(
            rig.node.counter().settings().lightning_node,
            Some(node_key())
        );
        // Never kept to be confirmed (CR-239), and nothing waits.
        assert!(!confirm_exists(dir.path()));
        assert!(rig.node.pending_change().is_none());
        // The settings file holds the frame with the signature, and a start
        // reads it back under the wallet the record pins.
        let kept = DirStore::open(dir.path())
            .unwrap()
            .settings_file()
            .unwrap()
            .unwrap();
        assert_eq!(kept, change.to_bytes());
        // A start reads the file back under the wallet the record pins; the
        // node's own settings stand in for the record's, which are older.
        let mut base = record;
        base.settings = signed(&settings(), None);
        let effective = effective_of(
            &DirStore::open(dir.path()).unwrap(),
            &PairedRecord::OwnWallet(Box::new(base)),
        )
        .unwrap();
        assert!(effective.from_file);
        assert_eq!(effective.signed, change);
    }
}

#[test]
fn cr_236_shop_lightning_comes_into_force_with_the_wallets_signature() {
    let dir = Scratch::new();
    let (record, key) = paired(32, false);
    let mut rig = rig_for(dir.path(), &record);
    let s = shop_lightning(2);
    let change = signed(&s, Some(sign_compact_text(&key, &wallet_message_for(&s))));
    assert!(rig.node.take_settings(&change).is_ok());
    assert_eq!(rig.node.settings().version, 2);
    assert_eq!(rig.node.settings().shop_lightning, s.shop_lightning);
    assert!(!confirm_exists(dir.path()));
}

#[test]
fn cr_236_a_wrong_signature_is_refused_and_nothing_is_in_force() {
    let dir = Scratch::new();
    let (record, key) = paired(33, false);
    let other = signing_wallet(34).1;
    let mut rig = rig_for(dir.path(), &record);
    let s = redirect(2);
    let message = wallet_message_for(&s);
    let address = record.wallet.receive_address(0).unwrap().to_string();
    for sig in [
        sign_compact_text(&other, &message),
        sign_bip322_text(&other, &message),
        sign_compact_text(&key, "another message"),
        "notasignaturebutofthesaneshape".to_string(),
    ] {
        let err = rig.node.take_settings(&signed(&s, Some(sig))).unwrap_err();
        assert!(
            matches!(err, SettingsRefused::Verify(SettingsError::WalletSig(_))),
            "{err}"
        );
        assert!(err.to_string().contains(&address), "{err}");
        assert_eq!(err.words().heading, "Change not taken");
        assert_eq!(rig.node.settings().version, 1);
        assert_eq!(rig.node.settings().lightning_node, None);
        assert!(rig.node.pending_change().is_none());
        assert!(!confirm_exists(dir.path()));
        assert!(DirStore::open(dir.path())
            .unwrap()
            .settings_file()
            .unwrap()
            .is_none());
    }
    // A signature of other settings does not cover these.
    let mut elsewhere = redirect(2);
    elsewhere.tor_on = !elsewhere.tor_on;
    let sig = sign_compact_text(&key, &wallet_message_for(&elsewhere));
    assert!(rig.node.take_settings(&signed(&s, Some(sig))).is_err());
    assert_eq!(rig.node.settings().version, 1);
}

#[test]
fn ap_15_a_counter_paired_with_a_starter_wallet_does_not_take_a_redirect() {
    let dir = Scratch::new();
    let (record, key) = paired(35, true);
    let mut rig = rig_for(dir.path(), &record);
    let s = redirect(2);
    let sig = sign_compact_text(&key, &wallet_message_for(&s));
    let err = rig.node.take_settings(&signed(&s, Some(sig))).unwrap_err();
    assert_eq!(err, SettingsRefused::Verify(SettingsError::RedirectStarter));
    assert!(err.to_string().starts_with("CR-236"));
    let words = err.words();
    assert_eq!(words.heading, "Change not taken");
    assert!(words.what.contains("starter wallet"));
    assert_eq!(rig.node.settings().version, 1);
    assert!(!confirm_exists(dir.path()));
    // A Free change is still taken.
    let mut free = settings();
    free.version = 3;
    free.light_per_sale.sats -= 1;
    assert!(rig.node.take_settings(&signed(&free, None)).is_ok());
}

#[test]
fn cr_236_a_wallet_signature_on_a_change_of_another_class_is_refused() {
    let dir = Scratch::new();
    let (record, key) = paired(36, false);
    let mut rig = rig_for(dir.path(), &record);
    let mut free = settings();
    free.version = 2;
    free.light_per_sale.sats -= 1;
    let sig = sign_compact_text(&key, &wallet_message_for(&free));
    assert_eq!(
        rig.node.take_settings(&signed(&free, Some(sig))),
        Err(SettingsRefused::Verify(SettingsError::WalletSigNotWanted))
    );
    let mut loose = settings();
    loose.version = 3;
    loose.light_per_sale.sats += 10;
    let sig = sign_compact_text(&key, &wallet_message_for(&loose));
    assert_eq!(
        rig.node.take_settings(&signed(&loose, Some(sig))),
        Err(SettingsRefused::Verify(SettingsError::WalletSigNotWanted))
    );
    assert_eq!(rig.node.settings().version, 1);
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_exists(dir.path()));
}

#[test]
fn cr_239_a_redirect_without_a_signature_is_not_kept_and_a_signed_frame_in_confirm_is_dropped() {
    let dir = Scratch::new();
    let (record, key) = paired(37, false);
    let mut rig = rig_for(dir.path(), &record);
    let s = redirect(2);
    assert!(rig.node.take_settings(&signed(&s, None)).is_err());
    assert!(!confirm_exists(dir.path()));
    drop(rig);
    // A frame with a wallet signature left in the file of the change that
    // waits is of no use: a Redirect is never kept to be confirmed.
    let sig = sign_compact_text(&key, &wallet_message_for(&s));
    std::fs::write(
        dir.path().join(CONFIRM_FILE),
        signed(&s, Some(sig)).to_bytes(),
    )
    .unwrap();
    let mut rig = rig_for(dir.path(), &record);
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_exists(dir.path()));
}

/// A record paired with the 2-of-3 wallet and the key each cosigner signs
/// with (AP-16).
fn paired_multi(seed: u8) -> (crate::OwnPairingRecord, [SecretKey; 3]) {
    let (wallet, keys) = cosigning_wallet(seed);
    let mut r = record();
    r.wallet = wallet;
    (r, keys)
}

#[test]
fn ap_17_a_redirect_with_two_cosigners_signatures_comes_into_force() {
    let dir = Scratch::new();
    let (record, keys) = paired_multi(33);
    let mut rig = rig_for(dir.path(), &record);
    let s = redirect(2);
    let message = wallet_message_for(&s);
    let text = join_cosigner_sigs(&[
        (1, sign_compact_text(&keys[0], &message)),
        (3, sign_bip322_text(&keys[2], &message)),
    ]);
    let change = signed(&s, Some(text));
    assert_eq!(rig.node.take_settings(&change), Ok(Some(Change::Loosening)));
    assert_eq!(rig.node.settings().version, 2);
    assert!(!confirm_exists(dir.path()));
    // The frame that was kept reads back to the same bytes.
    let kept = DirStore::open(dir.path())
        .unwrap()
        .settings_file()
        .unwrap()
        .unwrap();
    assert_eq!(kept, change.to_bytes());
    assert_eq!(SignedSettings::from_bytes(&kept).unwrap(), change);
}

#[test]
fn ap_17_a_redirect_with_one_cosigner_or_a_bad_one_is_refused_and_changes_nothing() {
    let dir = Scratch::new();
    let (record, keys) = paired_multi(34);
    let mut rig = rig_for(dir.path(), &record);
    let s = redirect(2);
    let message = wallet_message_for(&s);
    let one = join_cosigner_sigs(&[(2, sign_compact_text(&keys[1], &message))]);
    let e = rig.node.take_settings(&signed(&s, Some(one))).unwrap_err();
    assert!(matches!(
        e,
        SettingsRefused::Verify(SettingsError::WalletSig(WalletSigError::NotEnough))
    ));
    // Two signatures, the second not by the cosigner of its place.
    let bad = join_cosigner_sigs(&[
        (1, sign_compact_text(&keys[0], &message)),
        (2, sign_compact_text(&keys[0], &message)),
    ]);
    let e = rig.node.take_settings(&signed(&s, Some(bad))).unwrap_err();
    let named = record.wallet.cosigners().unwrap()[1].address.to_string();
    match e {
        SettingsRefused::Verify(SettingsError::WalletSig(WalletSigError::WrongSigner {
            address,
        })) => assert_eq!(address, named),
        other => panic!("{other:?}"),
    }
    assert_eq!(rig.node.settings().version, settings().version);
    assert!(!confirm_exists(dir.path()));
}
