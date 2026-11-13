//! A Box for a shop with its own wallet, through `dispatch` (ops 112 and
//! 113 and the known check of op 104, `docs/BOX-RULES.md` BX-18,
//! `docs/COUNTER-RULES.md` CR-130 to CR-132): the owner phone makes the
//! invitation, a real Box on the loopback takes it, the phone hands it the
//! welcome of an own-wallet shop with the settings signed by the owner root
//! alone, and signs the relay a counter uses to reach the Box.

use std::net::SocketAddr;
use std::sync::Arc;

use openbmt_box::{BoxRunner, BoxStatus};
use openbmt_core::boxpair::BoxRoots;
use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::{OwnCounterPairing, OwnOffer};
use openbmt_core::relay::{RelayRoute, SignedRelayHint};
use openbmt_core::settings::SignedSettings;
use openbmt_link::{Client, ClientConfig, Route, ServerConfig};
use openbmt_net_io::SystemClock;

use super::import::descriptor;
use super::kit::{Rig, OWNER_SECRET};
use super::wire::{Fault, Rd, W};

const T0: u64 = 1_000_000;
const USD: u64 = 100_000;

fn owner() -> Identity {
    Identity::from_secret_bytes(&OWNER_SECRET).unwrap()
}

fn wallet_file(rig: &Rig) -> Vec<u8> {
    let response = rig.call(148, 0, &W::new().string(&descriptor()).0);
    let mut r = Rd::ok(&response);
    let (file, _, _) = (r.bytes(), r.string(), r.bool());
    r.done();
    file
}

fn shop(rig: &Rig, wallet: &[u8]) -> W {
    W::new()
        .string(rig.dir().to_str().unwrap())
        .bytes(&OWNER_SECRET)
        .bytes(wallet)
}

fn started_box(name: &str) -> (BoxRunner, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let runner = BoxRunner::start(
        &dir,
        "127.0.0.1:0".parse::<SocketAddr>().expect("an address"),
        Arc::new(SystemClock),
        ServerConfig::default(),
    )
    .expect("the Box starts");
    (runner, dir)
}

/// The invitation the owner phone made, typed into the Box, and the claim
/// the Box offers for it.
fn invite(rig: &Rig, runner: &BoxRunner) -> (String, Vec<u8>) {
    let text = Rd::ok(&rig.call(108, 0, &W::new().u64(T0).0)).string();
    runner.enter_invitation(&text).expect("taken");
    let claim = Client::connect(
        &Route::Direct(runner.local_addr()),
        None,
        &ClientConfig::default(),
    )
    .expect("connects")
    .claim()
    .expect("a claim");
    (text, claim)
}

fn welcome_fields(rig: &Rig, wallet: &[u8], address: &str, text: &str, claim: &[u8]) -> W {
    shop(rig, wallet)
        .string(address)
        .string("")
        .string(text)
        .bytes(claim)
        .u64(USD)
}

fn refused(rig: &Rig, op: u16, fields: &[u8]) -> Fault {
    Rd::error(&rig.call(op, 0, fields)).expect("refused")
}

/// A counter paired to the shop: the offer, the request and "They match".
fn pair_counter(rig: &Rig, wallet: &[u8], n: u8) -> Vec<u8> {
    let response = rig.call(150, 0, &shop(rig, wallet).u64(T0).0);
    let mut r = Rd::ok(&response);
    let code = r.bytes();
    let made_at = r.u64();
    let _ = (r.u64(), r.u32(), r.string());
    r.done();
    let identity = Identity::from_secret_bytes(&[n; 32]).unwrap();
    let (_, request) = OwnCounterPairing::begin(
        OwnOffer::from_bytes(&code).unwrap(),
        &identity,
        "Front",
        [n; 32],
        T0 + 5,
    )
    .unwrap();
    let response = rig.call(
        152,
        0,
        &shop(rig, wallet)
            .bytes(&code)
            .u64(made_at)
            .u64(T0 + 30)
            .bytes(&request.to_bytes())
            .u64(USD)
            .opt_bytes(None)
            .0,
    );
    Rd::ok(&response);
    identity.device_id().to_bytes().to_vec()
}

#[test]
fn bx_18_the_welcome_pairs_a_box_as_an_own_wallet_shops_and_lists_the_counters() {
    let rig = Rig::new();
    let wallet = wallet_file(&rig);
    let counter = pair_counter(&rig, &wallet, 3);
    let (runner, dir) = started_box("openbmt-owner-ffi-own-box");
    let (text, claim) = invite(&rig, &runner);
    let address = runner.local_addr().to_string();
    let response = rig.call(
        112,
        0,
        &welcome_fields(&rig, &wallet, &address, &text, &claim).0,
    );
    let mut r = Rd::ok(&response);
    let (key, counters, version) = (r.bytes(), r.list(Rd::bytes), r.u32());
    r.done();
    assert_eq!(key, runner.device_id().to_bytes().to_vec());
    assert_eq!(version, 2, "the counter's settings were version 1");
    assert_eq!(counters, vec![counter.clone()]);
    // The Box pinned the owner root alone and the kind with it.
    assert_eq!(runner.status(), BoxStatus::Paired);
    let pinned = runner.core().lock().unwrap().pinned();
    assert_eq!(
        pinned,
        Some(BoxRoots::OwnWallet {
            owner: owner().device_id()
        })
    );
    // The settings it holds list the Box and the counter, and carry no
    // Key app signature.
    let held = runner
        .core()
        .lock()
        .unwrap()
        .settings()
        .cloned()
        .expect("settings");
    assert!(held.boxes.contains(&runner.device_id()));
    assert!(held
        .counters
        .keys()
        .any(|c| c.to_bytes().to_vec() == counter));
    assert!(runner
        .members()
        .iter()
        .any(|m| m.to_bytes().to_vec() == counter));
    // The same call again is answered as paired and changes nothing (BX-16).
    let again = rig.call(
        112,
        0,
        &welcome_fields(&rig, &wallet, &address, &text, &claim).0,
    );
    Rd::ok(&again);
    assert_eq!(runner.status(), BoxStatus::Paired);
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bx_18_a_claim_made_with_another_invitation_or_a_box_that_is_not_there_gets_no_welcome() {
    let rig = Rig::new();
    let wallet = wallet_file(&rig);
    let (runner, dir) = started_box("openbmt-owner-ffi-own-box-refused");
    let (text, claim) = invite(&rig, &runner);
    let address = runner.local_addr().to_string();
    let other = Rd::ok(&rig.call(108, 0, &W::new().u64(T0).0)).string();
    let fault = refused(
        &rig,
        112,
        &welcome_fields(&rig, &wallet, &address, &other, &claim).0,
    );
    assert_eq!(fault.rule, "box-claim-mismatch");
    let fault = refused(
        &rig,
        112,
        &welcome_fields(&rig, &wallet, "not an address", &text, &claim).0,
    );
    assert_eq!(fault.rule, "box-address");
    // Nothing was stored for the shop and the Box is still unpaired.
    assert_eq!(runner.status(), BoxStatus::InvitationOpen);
    assert!(
        !rig.dir().join("own-shop").exists(),
        "the settings were not made for a claim that did not check"
    );
    // A Box that is not at the address is not reached, and the settings
    // that were made for it are kept for the next try.
    let gone = "127.0.0.1:9";
    let fault = refused(
        &rig,
        112,
        &welcome_fields(&rig, &wallet, gone, &text, &claim).0,
    );
    assert_eq!(fault.rule, "box-unreachable");
    // The retry to the right address carries the same settings.
    let response = rig.call(
        112,
        0,
        &welcome_fields(&rig, &wallet, &address, &text, &claim).0,
    );
    Rd::ok(&response);
    assert_eq!(runner.status(), BoxStatus::Paired);
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bx_18_a_running_own_shop_knows_the_box_it_welcomed() {
    let rig = Rig::new();
    let wallet = wallet_file(&rig);
    let handle = Rd::ok(&rig.start_own(&wallet)).u64();
    let (runner, dir) = started_box("openbmt-owner-ffi-own-box-known");
    let (text, claim) = invite(&rig, &runner);
    let address = runner.local_addr().to_string();
    // A new Box is found, and the shop holds no settings so it needs a rate.
    let response = rig.call(104, handle, &W::new().string(&address).string("").0);
    let mut r = Rd::ok(&response);
    let (found, key, needs_rate) = (r.bytes(), r.bytes(), r.bool());
    r.done();
    assert_eq!(found, claim);
    assert_eq!(key, runner.device_id().to_bytes().to_vec());
    assert!(needs_rate);
    Rd::ok(&rig.call(
        112,
        0,
        &welcome_fields(&rig, &wallet, &address, &text, &claim).0,
    ));
    // Paired, the Box offers no claim, and the shop says it knows it.
    let fault = Rd::error(&rig.call(104, handle, &W::new().string(&address).string("").0))
        .expect("refused");
    assert_eq!(fault.rule, "box-paired");
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cr_132_the_relay_is_signed_by_the_owner_root_for_the_stored_address_and_key() {
    let rig = Rig::new();
    let key = Identity::from_secret_bytes(&[9u8; 32])
        .unwrap()
        .device_id()
        .to_bytes();
    let sign = |address: &str, key: &[u8]| {
        rig.call(
            113,
            0,
            &W::new().bytes(&OWNER_SECRET).string(address).bytes(key).0,
        )
    };
    let bytes = Rd::ok(&sign("192.168.1.20:7000", &key)).bytes();
    let hint = SignedRelayHint::from_bytes(&bytes).expect("an OBMz frame");
    assert!(hint.verify(&owner().device_id()));
    assert_eq!(hint.hint.key, key);
    assert_eq!(
        hint.hint.route,
        RelayRoute::Direct("192.168.1.20:7000".parse().unwrap())
    );
    // The same inputs give the same code.
    assert_eq!(Rd::ok(&sign("192.168.1.20:7000", &key)).bytes(), bytes);
    // An onion name is written as the name without `.onion`.
    let name = "a".repeat(56);
    let bytes = Rd::ok(&sign(&format!("{name}.onion:80"), &key)).bytes();
    let hint = SignedRelayHint::from_bytes(&bytes).unwrap();
    assert_eq!(hint.hint.route, RelayRoute::Onion { name, port: 80 });
    // What cannot be a relay is refused with the rule.
    for (address, key) in [
        ("not an address", &key[..]),
        ("192.168.1.20:0", &key[..]),
        ("192.168.1.20:7000", &key[..31]),
        ("192.168.1.20:7000", &[0u8; 32][..]),
    ] {
        assert!(Rd::error(&sign(address, key)).is_some(), "{address}");
    }
    // A secret that is not 32 bytes.
    let response = rig.call(
        113,
        0,
        &W::new()
            .bytes(&[1u8; 5])
            .string("192.168.1.20:7000")
            .bytes(&key)
            .0,
    );
    assert_eq!(Rd::error(&response).unwrap().rule, "id-length");
}

#[test]
fn bx_18_the_settings_a_box_takes_read_back_as_signed_by_the_owner_alone() {
    let rig = Rig::new();
    let wallet = wallet_file(&rig);
    let (runner, dir) = started_box("openbmt-owner-ffi-own-box-sig");
    let (text, claim) = invite(&rig, &runner);
    let address = runner.local_addr().to_string();
    Rd::ok(&rig.call(
        112,
        0,
        &welcome_fields(&rig, &wallet, &address, &text, &claim).0,
    ));
    let stored = std::fs::read(dir.join("settings")).expect("the Box stored its settings");
    let signed = SignedSettings::from_bytes(&stored).unwrap();
    assert!(signed.approver_sig.is_none());
    signed
        .verify_signatures_with(&openbmt_core::settings::Authority::OwnWallet {
            owner: owner().device_id(),
        })
        .expect("signed by the owner root alone");
    runner.stop();
    let _ = std::fs::remove_dir_all(&dir);
}
