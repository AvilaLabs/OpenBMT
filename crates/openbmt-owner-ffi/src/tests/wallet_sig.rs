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
