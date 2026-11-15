//! A Redirect change of a shop with the 2-of-3 wallet is signed by two of its
//! cosigners (AP-16, AP-17, AP-18): the preview lists the cosigners, each
//! signature is checked as it is taken, and the change is put in force once two
//! different cosigners have signed.

use std::cell::{Cell, RefCell};

use openbmt_core::approval::test_signing::{cosigning_wallet, sign_compact_text};
use openbmt_core::approval::{split_cosigner_sigs, wallet_message_for};
use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::OwnCounterPairing;
use openbmt_owner::own_shop::{OwnError, OwnRefusal, OwnShop};
use openbmt_owner::{LightningEdit, Refusal, SettingsEdit};

const NODE: &str = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const T0: u64 = 1_000_000;
const USD: u64 = 100_000;

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

#[derive(Default)]
struct Disk(RefCell<Vec<u8>>, Cell<u32>);

impl Disk {
    fn put(&self, bytes: &[u8]) -> Result<(), String> {
        *self.0.borrow_mut() = bytes.to_vec();
        self.1.set(self.1.get() + 1);
        Ok(())
    }
}

fn shop(disk: &Disk) -> (OwnShop, [bitcoin::secp256k1::SecretKey; 3]) {
    let (wallet, keys) = cosigning_wallet(51);
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
    (s, keys)
}

fn node_edit() -> SettingsEdit {
    SettingsEdit {
        lightning_node: Some(LightningEdit::Set(NODE.to_string())),
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
fn ap_18_the_preview_lists_the_three_cosigners_and_no_single_address() {
    let disk = Disk::default();
    let (s, _) = shop(&disk);
    let preview = s.settings_preview(&node_edit()).unwrap();
    let ask = preview.wallet_sig.expect("a Redirect asks for signatures");
    assert_eq!(ask.message, wallet_message_for(&preview.settings));
    assert!(ask.address.is_empty());
    assert_eq!(ask.cosigners.len(), 3);
    for (i, c) in ask.cosigners.iter().enumerate() {
        assert_eq!(usize::from(c.place), i + 1);
        assert_eq!(c.path, "m/48h/0h/0h/2h/0/0");
        assert_eq!(c.fingerprint.len(), 8);
        assert!(c.address.starts_with("bc1q"));
    }
    let shown: std::collections::BTreeSet<_> =
        ask.cosigners.iter().map(|c| c.address.clone()).collect();
    assert_eq!(shown.len(), 3);
}

#[test]
fn ap_17_two_cosigners_signing_one_at_a_time_put_the_change_in_force() {
    let disk = Disk::default();
    let (mut s, keys) = shop(&disk);
    let preview = s.settings_preview(&node_edit()).unwrap();
    let message = wallet_message_for(&preview.settings);
    let sig = |i: usize| sign_compact_text(&keys[i], &message);
    let held = s
        .add_cosigner_sig::<String>(&preview.settings, "", 3, &sig(2))
        .unwrap();
    assert_eq!(split_cosigner_sigs(&held).unwrap().len(), 1);
    // One signature is not enough to send.
    let writes = disk.1.get();
    assert!(matches!(
        refused(
            s.change_settings_with_wallet_sig(preview.settings.clone(), &held, |b| disk.put(b))
        ),
        OwnRefusal::Settings(Refusal::SettingsInvalid(_))
    ));
    assert_eq!(disk.1.get(), writes);
    let held = s
        .add_cosigner_sig::<String>(&preview.settings, &held, 1, &sig(0))
        .unwrap();
    // The places are kept in order.
    let places: Vec<u8> = split_cosigner_sigs(&held)
        .unwrap()
        .into_iter()
        .map(|(p, _)| p)
        .collect();
    assert_eq!(places, [1, 3]);
    let signed = s
        .change_settings_with_wallet_sig(preview.settings.clone(), &held, |b| disk.put(b))
        .unwrap();
    assert_eq!(signed.wallet_sig.as_deref(), Some(held.as_str()));
    assert_eq!(s.known_settings().unwrap().settings, preview.settings);
}

#[test]
fn ap_17_a_signature_by_the_wrong_key_is_refused_at_once_naming_that_cosigners_address() {
    let disk = Disk::default();
    let (s, keys) = shop(&disk);
    let preview = s.settings_preview(&node_edit()).unwrap();
    let message = wallet_message_for(&preview.settings);
    let ask = preview.wallet_sig.clone().unwrap();
    // Cosigner 1's signature offered for place 2.
    let wrong = sign_compact_text(&keys[0], &message);
    assert_eq!(
        refused(s.add_cosigner_sig::<String>(&preview.settings, "", 2, &wrong)),
        OwnRefusal::WalletSig {
            address: ask.cosigners[1].address.clone()
        }
    );
    // A text that is no signature.
    assert_eq!(
        refused(s.add_cosigner_sig::<String>(&preview.settings, "", 3, "AAAA")),
        OwnRefusal::WalletSig {
            address: ask.cosigners[2].address.clone()
        }
    );
    // A place that does not exist, and a held text that is not this form.
    let good = sign_compact_text(&keys[0], &message);
    for place in [0u8, 4] {
        assert!(matches!(
            refused(s.add_cosigner_sig::<String>(&preview.settings, "", place, &good)),
            OwnRefusal::Settings(_)
        ));
    }
    assert!(matches!(
        refused(s.add_cosigner_sig::<String>(&preview.settings, "AAAA", 1, &good)),
        OwnRefusal::Settings(_)
    ));
}

#[test]
fn ap_17_a_signature_taken_again_for_a_place_replaces_it_and_one_cosigner_twice_is_one() {
    let disk = Disk::default();
    let (mut s, keys) = shop(&disk);
    let preview = s.settings_preview(&node_edit()).unwrap();
    let message = wallet_message_for(&preview.settings);
    let first = sign_compact_text(&keys[0], &message);
    let held = s
        .add_cosigner_sig::<String>(&preview.settings, "", 1, &first)
        .unwrap();
    let again = s
        .add_cosigner_sig::<String>(&preview.settings, &held, 1, &first)
        .unwrap();
    assert_eq!(held, again);
    assert!(matches!(
        refused(s.change_settings_with_wallet_sig(preview.settings, &again, |b| disk.put(b))),
        OwnRefusal::Settings(Refusal::SettingsInvalid(_))
    ));
}

#[test]
fn ap_17_the_single_key_wallet_takes_no_cosigner_signature() {
    let disk = Disk::default();
    let (wallet, key) = openbmt_core::approval::test_signing::signing_wallet(41);
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
    let preview = s.settings_preview(&node_edit()).unwrap();
    let ask = preview.wallet_sig.clone().unwrap();
    assert!(ask.cosigners.is_empty());
    let sig = sign_compact_text(&key, &wallet_message_for(&preview.settings));
    assert!(matches!(
        refused(s.add_cosigner_sig::<String>(&preview.settings, "", 1, &sig)),
        OwnRefusal::WalletSig { .. }
    ));
}
