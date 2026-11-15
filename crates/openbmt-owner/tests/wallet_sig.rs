//! A Redirect change of a shop with its own wallet carries the wallet's
//! signature (AP-14, AP-15, OA-127): the preview says what to sign and which
//! address signs, the signature is checked on the phone before anything is
//! stored, and a shop on its starter wallet is not offered the change.

use std::cell::{Cell, RefCell};

use openbmt_core::approval::test_signing::{sign_bip322_text, sign_compact_text, signing_wallet};
use openbmt_core::approval::{classify, wallet_message_for, ChangeClass};
use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::OwnCounterPairing;
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_owner::own_shop::{OwnError, OwnRefusal, OwnShop};
use openbmt_owner::{
    LightningEdit, Refusal, SettingsEdit, ShopLightningEdit, ShopLightningRefusal, ShopLightningSet,
};

const NODE: &str = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const PROVIDER: &str = "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";
const T0: u64 = 1_000_000;
const USD: u64 = 100_000;

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

/// A store that keeps the last bytes written and counts the writes.
#[derive(Default)]
struct Disk(RefCell<Vec<u8>>, Cell<u32>);

impl Disk {
    fn put(&self, bytes: &[u8]) -> Result<(), String> {
        *self.0.borrow_mut() = bytes.to_vec();
        self.1.set(self.1.get() + 1);
        Ok(())
    }
}

/// A shop on the wallet of `seed`, with a counter and a Box in force, and the
/// secret key of the wallet's first address.
fn shop(seed: u8, disk: &Disk) -> (OwnShop, bitcoin::secp256k1::SecretKey) {
    let (wallet, key): (PinnedWallet, _) = signing_wallet(seed);
    let mut s = OwnShop::new(id(1), wallet);
    let made = s.make_offer(T0).unwrap();
    let (_, request) =
        OwnCounterPairing::begin(made.offer.clone(), &id(3), "Front", [3; 32], T0 + 5).unwrap();
    s.confirm(
        &made,
        T0 + 30,
        &request.to_bytes(),
        USD,
        Vec::new(),
        None,
        |b| disk.put(b),
    )
    .unwrap();
    s.add_box(id(9).device_id(), 0, |b| disk.put(b)).unwrap();
    (s, key)
}

fn node_edit() -> SettingsEdit {
    SettingsEdit {
        lightning_node: Some(LightningEdit::Set(NODE.to_string())),
        ..SettingsEdit::default()
    }
}

fn shop_lightning_edit() -> SettingsEdit {
    SettingsEdit {
        shop_lightning: Some(ShopLightningEdit::Set(ShopLightningSet {
            counter: id(3).device_id(),
            node_id: NODE.to_string(),
            second_device: None,
            provider_node_id: PROVIDER.to_string(),
            provider_addr: "203.0.113.7:9735".to_string(),
            cap_sats: 5_000_000,
            max_opening_fee_sats: 20_000,
        })),
        ..SettingsEdit::default()
    }
}

fn refused<T: std::fmt::Debug>(r: Result<T, OwnError<String>>) -> OwnRefusal {
    match r {
        Err(OwnError::Refused(e)) => e,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn ap_14_the_preview_of_a_redirect_gives_the_message_and_the_address() {
    let disk = Disk::default();
    let (s, _) = shop(41, &disk);
    for edit in [node_edit(), shop_lightning_edit()] {
        let preview = s.settings_preview(&edit).unwrap();
        let known = s.known_settings().unwrap();
        assert_eq!(
            classify(&known.settings, &preview.settings),
            ChangeClass::Redirect
        );
        let ask = preview
            .wallet_sig
            .expect("a Redirect asks for the signature");
        assert_eq!(ask.message, wallet_message_for(&preview.settings));
        assert_eq!(
            ask.address,
            s.wallet().receive_address(0).unwrap().to_string()
        );
    }
    // A change of another class asks for nothing.
    let free = SettingsEdit {
        tor_on: Some(false),
        ..SettingsEdit::default()
    };
    let preview = s.settings_preview(&free).unwrap();
    assert_eq!(preview.wallet_sig, None);
}

#[test]
fn ap_15_a_shop_on_its_starter_wallet_is_not_offered_a_redirect() {
    let disk = Disk::default();
    let (s, _) = shop(42, &disk);
    for edit in [node_edit(), shop_lightning_edit()] {
        let on_starter = SettingsEdit {
            starter_wallet: true,
            ..edit
        };
        assert_eq!(
            s.settings_preview(&on_starter).unwrap_err(),
            Refusal::ShopLightning(ShopLightningRefusal::Starter)
        );
    }
    // The words are the decided ones, and the rule is AP-15.
    let words = ShopLightningRefusal::Starter.words();
    assert_eq!(words.heading, "Move to your own wallet");
    assert_eq!(words.code, Some("AP-15"));
    // Another class of change is still offered.
    let free = SettingsEdit {
        starter_wallet: true,
        tor_on: Some(false),
        ..SettingsEdit::default()
    };
    assert!(s.settings_preview(&free).is_ok());
}

#[test]
fn ap_14_a_good_signature_puts_the_redirect_in_force_with_the_signature_attached() {
    for bip322 in [false, true] {
        let disk = Disk::default();
        let (mut s, key) = shop(43, &disk);
        let preview = s.settings_preview(&node_edit()).unwrap();
        let message = wallet_message_for(&preview.settings);
        let text = if bip322 {
            sign_bip322_text(&key, &message)
        } else {
            sign_compact_text(&key, &message)
        };
        // The text is trimmed of surrounding white space and nothing else.
        let padded = format!("  {text}\n");
        let signed = s
            .change_settings_with_wallet_sig(preview.settings.clone(), &padded, |b| disk.put(b))
            .unwrap();
        assert_eq!(signed.wallet_sig.as_deref(), Some(text.as_str()));
        assert_eq!(s.known_settings(), Some(&signed));
        assert!(s
            .known_settings()
            .unwrap()
            .settings
            .lightning_node
            .is_some());
        // It is stored, and what is stored reads back with the signature.
        let restored = OwnShop::restore(id(1), s.wallet().clone(), &s.to_bytes()).unwrap();
        assert_eq!(restored.known_settings(), Some(&signed));
        // What the link carries is the frame with the signature.
        let frame = signed.to_bytes();
        assert_eq!(frame[4], 6);
        assert_eq!(
            openbmt_core::settings::SignedSettings::from_bytes(&frame).unwrap(),
            signed
        );
    }
}

#[test]
fn ap_14_shop_lightning_is_put_in_force_the_same_way() {
    let disk = Disk::default();
    let (mut s, key) = shop(44, &disk);
    let preview = s.settings_preview(&shop_lightning_edit()).unwrap();
    let text = sign_compact_text(&key, &wallet_message_for(&preview.settings));
    s.change_settings_with_wallet_sig(preview.settings, &text, |b| disk.put(b))
        .unwrap();
    assert!(s
        .known_settings()
        .unwrap()
        .settings
        .shop_lightning
        .is_some());
}

#[test]
fn ap_10_a_wrong_signature_names_the_address_and_queues_nothing() {
    let disk = Disk::default();
    let (mut s, key) = shop(45, &disk);
    let other = signing_wallet(46).1;
    let preview = s.settings_preview(&node_edit()).unwrap();
    let message = wallet_message_for(&preview.settings);
    let address = s.wallet().receive_address(0).unwrap().to_string();
    let before = s.known_settings().cloned();
    let writes = disk.1.get();
    for text in [
        sign_compact_text(&other, &message),
        sign_bip322_text(&other, &message),
        sign_compact_text(&key, "another message"),
        "not a signature".to_string(),
        String::new(),
    ] {
        let e = refused(
            s.change_settings_with_wallet_sig(preview.settings.clone(), &text, |b| disk.put(b)),
        );
        assert_eq!(
            e,
            OwnRefusal::WalletSig {
                address: address.clone()
            }
        );
        assert!(e.to_string().contains(&address));
        let words = e.words();
        assert_eq!(words.heading, "Signature not accepted");
        assert!(words.what.contains(&address));
        assert_eq!(
            words.next,
            "Sign the message again with that address and paste the whole signature."
        );
        assert_eq!(s.known_settings().cloned(), before);
        assert_eq!(disk.1.get(), writes, "nothing was stored");
    }
}

#[test]
fn ap_14_a_signature_is_taken_only_with_a_redirect() {
    let disk = Disk::default();
    let (mut s, key) = shop(47, &disk);
    let free = s
        .settings_preview(&SettingsEdit {
            tor_on: Some(false),
            ..SettingsEdit::default()
        })
        .unwrap();
    let text = sign_compact_text(&key, &wallet_message_for(&free.settings));
    let e = refused(s.change_settings_with_wallet_sig(free.settings, &text, |b| disk.put(b)));
    assert!(matches!(e, OwnRefusal::Settings(_)));
    assert!(s.known_settings().unwrap().wallet_sig.is_none());
}
