//! A Redirect change of a shop with its own wallet through `dispatch` (ops
//! 243 and 244, `docs/APPROVAL-RULES.md` AP-14, AP-15,
//! `docs/OWNER-APP-RULES.md` OA-127): the preview gives the message and the
//! address, a signature that does not check is refused with the rule and
//! stores nothing, and a good one puts the settings in force.

use openbmt_core::approval::test_signing::{sign_bip322_text, sign_compact_text, signing_wallet};
use openbmt_core::approval::wallet_message_for;
use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::OwnCounterPairing;
use openbmt_core::settings::SignedSettings;
use openbmt_owner_node::OwnNodeError;

use super::kit::Scratch;
use super::settings::Edit;
use super::wire::{frame, Rd, W};
use crate::{dispatch, Registry};

const T0: u64 = 1_000_000;
const KEY: &str = "0279BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798";

fn call(op: u16, fields: &[u8]) -> Vec<u8> {
    dispatch(&Registry::new(), &frame(op, 0, fields))
}

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

fn shop(dir: &Scratch, wallet: &[u8]) -> W {
    W::new()
        .string(dir.path().to_str().unwrap())
        .bytes(&[1u8; 32])
        .bytes(wallet)
}

/// A shop with its own wallet and one counter in force: its folder and the
/// wallet's file, and the key that signs for the wallet.
fn paired(seed: u8) -> (Scratch, Vec<u8>, bitcoin::secp256k1::SecretKey) {
    let dir = Scratch::new();
    let (wallet, key) = signing_wallet(seed);
    let file = wallet.to_bytes();
    let response = call(150, &shop(&dir, &file).u64(T0).0);
    let mut r = Rd::ok(&response);
    let code = r.bytes();
    let offer = openbmt_core::pairing_own::OwnOffer::from_bytes(&code).unwrap();
    let (_, request) = OwnCounterPairing::begin(offer, &id(2), "Front", [2u8; 32], T0 + 5).unwrap();
    let response = call(
        152,
        &shop(&dir, &file)
            .bytes(&code)
            .u64(T0)
            .u64(T0 + 30)
            .bytes(&request.to_bytes())
            .u64(100_000)
            .opt_bytes(None)
            .0,
    );
    Rd::ok(&response);
    (dir, file, key)
}

fn node_edit() -> Vec<u8> {
    Edit::node(KEY).bytes()
}

/// The answer of op 243: the settings of the preview, whether a signature is
/// asked for, the message and the address.
fn ask(
    dir: &Scratch,
    file: &[u8],
    starter: bool,
) -> Result<(Vec<u8>, bool, String, String), String> {
    let response = call(
        243,
        &W::new()
            .string(dir.path().to_str().unwrap())
            .bytes(&[1u8; 32])
            .bytes(file)
            .0
            .into_iter()
            .chain(node_edit())
            .chain([u8::from(starter)])
            .collect::<Vec<u8>>(),
    );
    if let Some(f) = Rd::error(&response) {
        return Err(f.rule);
    }
    let mut r = Rd::ok(&response);
    let _version = r.u32();
    let _changes = r.list(|r| {
        (
            r.string(),
            r.string(),
            r.opt(Rd::bytes),
            r.string(),
            r.string(),
            r.bool(),
        )
    });
    let (_, _, _) = (r.bool(), r.bool(), r.u32());
    let (_, _) = (r.opt(Rd::u64), r.opt(Rd::u64));
    let settings = r.bytes();
    let _currency = r.u8();
    let (needs, message, address) = (r.bool(), r.string(), r.string());
    r.done();
    Ok((settings, needs, message, address))
}

fn attach(dir: &Scratch, file: &[u8], settings: &[u8], signature: &str) -> Result<u32, String> {
    let response = call(244, &shop(dir, file).bytes(settings).string(signature).0);
    if let Some(f) = Rd::error(&response) {
        return Err(f.rule);
    }
    let mut r = Rd::ok(&response);
    let version = r.u32();
    r.done();
    Ok(version)
}

#[test]
fn ap_14_the_preview_gives_the_message_and_the_address_through_dispatch() {
    let (dir, file, _) = paired(61);
    let (settings, needs, message, address) = ask(&dir, &file, false).unwrap();
    assert!(needs);
    let parsed = SignedSettings::from_bytes(&settings).unwrap();
    assert_eq!(message, wallet_message_for(&parsed.settings));
    let wallet = openbmt_core::pinned_wallet::PinnedWallet::from_bytes(&file).unwrap();
    assert_eq!(address, wallet.receive_address(0).unwrap().to_string());
}

#[test]
fn ap_15_a_shop_on_its_starter_wallet_is_refused_through_dispatch() {
    let (dir, file, _) = paired(62);
    assert_eq!(ask(&dir, &file, true).unwrap_err(), "AP-15");
}

#[test]
fn ap_14_a_good_signature_is_attached_and_a_bad_one_stores_nothing_through_dispatch() {
    let (dir, file, key) = paired(63);
    let (settings, _, message, _) = ask(&dir, &file, false).unwrap();
    // A signature of another message is refused, with the rule, and the next
    // preview still finds the same settings to change.
    let wrong = sign_compact_text(&key, "some other message");
    assert_eq!(attach(&dir, &file, &settings, &wrong).unwrap_err(), "AP-10");
    assert_eq!(
        attach(&dir, &file, &settings, "  not a signature ").unwrap_err(),
        "AP-10"
    );
    let again = ask(&dir, &file, false).unwrap();
    assert!(
        again.1,
        "nothing was put in force, so the change is still a Redirect"
    );
    // The good one, padded as it comes from a clipboard, is taken.
    let good = sign_bip322_text(&key, &message);
    let version = attach(&dir, &file, &settings, &format!(" {good}\n")).unwrap();
    assert!(
        version
            > SignedSettings::from_bytes(&settings)
                .unwrap()
                .settings
                .version
                - 1
    );
    // Now the node is set and the same edit changes nothing, so no Redirect.
    let after = ask(&dir, &file, false).unwrap();
    assert!(!after.1);
}

const LN_NODE: &str = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const LN_PROVIDER: &str = "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

/// The shop has a Box in force, as Shop Lightning needs (SL-73) and as a
/// change that loosens needs to wait for a confirmation (OA-119).
fn add_a_box(dir: &Scratch, file: &[u8]) {
    let wallet = openbmt_core::pinned_wallet::PinnedWallet::from_bytes(file).unwrap();
    crate::own::turn_on_shop(dir.path(), id(1), wallet, |node| {
        let (shop, store) = node.shop_and_store();
        shop.add_box(id(9).device_id(), 0, |b| store(b))
            .map(|_| ())
            .map_err(|e| OwnNodeError::Io(format!("{e:?}")))
    })
    .unwrap_or_else(|e| panic!("a Box was not added: {e}"));
}

/// The request of op 245 for turning Shop Lightning on at the paired counter,
/// with the Box given.
fn shop_lightning_request(dir: &Scratch, file: &[u8], starter: bool) -> Vec<u8> {
    add_a_box(dir, file);
    let mut w = W::new()
        .string(dir.path().to_str().unwrap())
        .bytes(&[1u8; 32])
        .bytes(file)
        // Set, the counter, the node, the Box, the provider and its address.
        .u8(0)
        .opt_bytes(Some(&id(2).device_id().to_bytes()))
        .opt_string(Some(LN_NODE))
        .opt_bytes(None)
        .opt_string(Some(LN_PROVIDER))
        .opt_string(Some("203.0.113.7:9735"));
    // The cap and the most for one opening.
    w = w.u8(1).u64(5_000_000).u8(1).u64(20_000);
    w.u8(u8::from(starter)).0
}

#[test]
fn ap_14_shop_lightning_is_previewed_and_put_in_force_through_dispatch() {
    let (dir, file, key) = paired(64);
    let response = call(245, &shop_lightning_request(&dir, &file, false));
    if let Some(f) = Rd::error(&response) {
        panic!("refused: {f:?}");
    }
    let mut r = Rd::ok(&response);
    let _version = r.u32();
    let (class, _, nothing) = (r.u8(), r.bool(), r.bool());
    assert_eq!(class, 2, "a Redirect");
    assert!(!nothing);
    let after = r.opt(|r| {
        let counter = r.bytes();
        let _ = (
            r.bytes(),
            r.bytes(),
            r.bytes(),
            r.string(),
            r.u64(),
            r.u64(),
        );
        counter
    });
    assert_eq!(after.unwrap(), id(2).device_id().to_bytes());
    let settings = r.bytes();
    let (needs, message, address) = (r.bool(), r.string(), r.string());
    r.done();
    assert!(needs);
    let parsed = SignedSettings::from_bytes(&settings).unwrap();
    assert_eq!(message, wallet_message_for(&parsed.settings));
    let wallet = openbmt_core::pinned_wallet::PinnedWallet::from_bytes(&file).unwrap();
    assert_eq!(address, wallet.receive_address(0).unwrap().to_string());
    // A wrong signature stores nothing; the right one puts it in force.
    let wrong = sign_compact_text(&key, "another message");
    assert_eq!(attach(&dir, &file, &settings, &wrong).unwrap_err(), "AP-10");
    attach(&dir, &file, &settings, &sign_compact_text(&key, &message)).unwrap();
}

#[test]
fn ap_15_shop_lightning_on_a_starter_wallet_is_refused_through_dispatch() {
    let (dir, file, _) = paired(65);
    let response = call(245, &shop_lightning_request(&dir, &file, true));
    assert_eq!(Rd::error(&response).expect("refused").rule, "AP-15");
}

/// Op 243 for `edit`: the settings of the preview.
fn preview_of(dir: &Scratch, file: &[u8], edit: Vec<u8>) -> Vec<u8> {
    let response = call(
        243,
        &W::new()
            .string(dir.path().to_str().unwrap())
            .bytes(&[1u8; 32])
            .bytes(file)
            .0
            .into_iter()
            .chain(edit)
            .chain([0u8])
            .collect::<Vec<u8>>(),
    );
    if let Some(f) = Rd::error(&response) {
        panic!("refused: {f:?}");
    }
    let mut r = Rd::ok(&response);
    let _ = r.u32();
    let _ = r.list(|r| {
        (
            r.string(),
            r.string(),
            r.opt(Rd::bytes),
            r.string(),
            r.string(),
            r.bool(),
        )
    });
    let _ = (r.bool(), r.bool(), r.u32(), r.opt(Rd::u64), r.opt(Rd::u64));
    r.bytes()
}

/// Op 246: (version, waits), or the rule that refused.
fn send(dir: &Scratch, file: &[u8], settings: &[u8], now: u64) -> Result<(u32, bool), String> {
    let response = call(246, &shop(dir, file).bytes(settings).u64(now).0);
    if let Some(f) = Rd::error(&response) {
        return Err(f.rule);
    }
    let mut r = Rd::ok(&response);
    let out = (r.u32(), r.bool());
    r.done();
    Ok(out)
}

#[test]
fn cr_236_a_free_change_is_put_in_force_by_op_246() {
    let (dir, file, _) = paired(66);
    let settings = preview_of(&dir, &file, Edit::tor(true).bytes());
    let (version, waits) = send(&dir, &file, &settings, T0 + 100).unwrap();
    assert!(!waits);
    assert_eq!(
        version,
        SignedSettings::from_bytes(&settings)
            .unwrap()
            .settings
            .version
    );
    // The same edit now changes nothing: the change is the settings in force.
    let again =
        SignedSettings::from_bytes(&preview_of(&dir, &file, Edit::tor(true).bytes())).unwrap();
    assert!(again.settings.tor_on);
}

#[test]
fn cr_239_a_change_that_loosens_waits_for_a_counters_confirmation_through_op_246() {
    let (dir, file, _) = paired(67);
    add_a_box(&dir, &file);
    let settings = preview_of(&dir, &file, Edit::raise_light_sale(900).bytes());
    let (version, waits) = send(&dir, &file, &settings, T0 + 100).unwrap();
    assert!(waits, "a counter has to confirm it");
    assert!(version > 0);
    // It is not in force: a new preview is made over the waiting change and
    // another change on top of the settings in force still goes.
    let free = preview_of(&dir, &file, Edit::tor(true).bytes());
    assert!(SignedSettings::from_bytes(&free).is_ok());
    // The waiting change is kept in its file.
    assert!(dir.path().join("own-waiting").exists());
}

#[test]
fn sl_77_shop_lightning_is_turned_off_for_a_shop_with_its_own_wallet_through_op_246() {
    let (dir, file, key) = paired(68);
    let response = call(245, &shop_lightning_request(&dir, &file, false));
    let mut r = Rd::ok(&response);
    let _ = (r.u32(), r.u8(), r.bool(), r.bool());
    let _ = r.opt(|r| {
        let _ = (
            r.bytes(),
            r.bytes(),
            r.bytes(),
            r.bytes(),
            r.string(),
            r.u64(),
            r.u64(),
        );
    });
    let on = r.bytes();
    let message = wallet_message_for(&SignedSettings::from_bytes(&on).unwrap().settings);
    attach(&dir, &file, &on, &sign_compact_text(&key, &message)).unwrap();
    // Off: action Remove, nothing else.
    let off = W::new()
        .string(dir.path().to_str().unwrap())
        .bytes(&[1u8; 32])
        .bytes(&file)
        .u8(2)
        .opt_bytes(None)
        .opt_string(None)
        .opt_bytes(None)
        .opt_string(None)
        .opt_string(None)
        .u8(0)
        .u8(0)
        .u8(0)
        .0;
    let response = call(245, &off);
    if let Some(f) = Rd::error(&response) {
        panic!("refused: {f:?}");
    }
    let mut r = Rd::ok(&response);
    let _ = (r.u32(), r.u8(), r.bool(), r.bool());
    let after = r.opt(|r| r.bytes());
    assert!(after.is_none(), "it would be off");
    let settings = r.bytes();
    let (needs, _, _) = (r.bool(), r.string(), r.string());
    assert!(!needs, "turning off asks for no signature");
    let (_, waits) = send(&dir, &file, &settings, T0 + 200).unwrap();
    assert!(!waits);
    assert!(SignedSettings::from_bytes(&settings)
        .unwrap()
        .settings
        .shop_lightning
        .is_none());
}

#[test]
fn ap_14_a_redirect_without_the_signature_is_refused_by_op_246_and_stores_nothing() {
    let (dir, file, _) = paired(69);
    let (settings, needs, _, _) = ask(&dir, &file, false).unwrap();
    assert!(needs);
    assert_eq!(
        send(&dir, &file, &settings, T0 + 100).unwrap_err(),
        "OA-110"
    );
    // Nothing is in force: the same Redirect is still a Redirect.
    assert!(ask(&dir, &file, false).unwrap().1);
}
