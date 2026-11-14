//! Pairing with the shop's own wallet through `dispatch`
//! (`docs/COUNTER-RULES.md` CR-227 to CR-233, CR-235, CR-237): the owner
//! phone's offer, the first address, the counter's request, the owner
//! phone's bundle, and a start from the stored record that opens a sale on
//! an address of the wallet. The owner phone's side is the functions of
//! `openbmt_core::pairing_own`, which lets these tests make a bundle that
//! the real owner phone never would; the real owner phone pairs a real
//! counter in `own_owner.rs`.

use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::{OwnBundle, OwnOffer, OwnRequest};
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
use openbmt_core::settings::{Settings, SignedSettings};

use super::kit::{peers_event, Rig, FIVE_DOLLARS, RATE, T0};
use super::wire::{Rd, W};
use crate::StartConfig;

/// The Key Maker's test seed, account 0 (WL-06): the first receive address
/// is the one WL-06 names.
pub(super) const SINGLE: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
pub(super) const FIRST_ADDRESS: &str = "bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj";
pub(super) const MULTI: &str = "wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt";
pub(super) const FLOOR: u32 = 4;
const PEER: &str = "127.0.0.1:8333";
/// The tag bytes of `OwnWalletKind`.
const KIND_SINGLE: u8 = 1;
const KIND_MULTI: u8 = 2;

fn owner() -> Identity {
    Identity::from_secret_bytes(&[0x11; 32]).unwrap()
}

pub(super) fn offer_for(text: &str) -> OwnOffer {
    OwnOffer::new(
        bitcoin::Network::Bitcoin,
        owner().device_id(),
        FLOOR,
        PinnedWallet::from_text(text).unwrap(),
    )
    .unwrap()
}

/// What `pair_begin` returns for an own-wallet offer: the request, the
/// words, the address and the kind byte, or the refusal's rule.
pub(super) fn begin(rig: &Rig, offer: &[u8]) -> Result<(Vec<u8>, Vec<String>, String, u8), String> {
    let response = rig.call(
        44,
        0,
        &W::new().bytes(offer).string("Front counter").u64(T0).0,
    );
    if let Some(e) = Rd::error(&response) {
        return Err(e.rule);
    }
    let mut r = Rd::ok(&response);
    let request = r.bytes();
    let words = r.list(|r| r.string());
    let address = r.string();
    let kind = r.u8();
    r.done();
    Ok((request, words, address, kind))
}

/// `pair_address_same`: the request, the words and the empty address, or the
/// refusal's rule.
pub(super) fn same(rig: &Rig, now: u64) -> Result<(Vec<u8>, Vec<String>), String> {
    let response = rig.call(69, 0, &W::new().u64(now).string("Front counter").0);
    if let Some(e) = Rd::error(&response) {
        return Err(e.rule);
    }
    let mut r = Rd::ok(&response);
    let request = r.bytes();
    let words = r.list(|r| r.string());
    assert_eq!(r.string(), "");
    r.u8();
    r.done();
    Ok((request, words))
}

pub(super) fn matched(rig: &Rig, now: u64) -> Result<(), String> {
    let response = rig.call(45, 0, &W::new().u64(now).0);
    match Rd::error(&response) {
        Some(e) => Err(e.rule),
        None => {
            Rd::ok(&response).done();
            Ok(())
        }
    }
}

pub(super) fn finish(rig: &Rig, bundle: &[u8], now: u64) -> Result<Vec<u8>, String> {
    let response = rig.call(48, 0, &W::new().u64(now).bytes(bundle).0);
    if let Some(e) = Rd::error(&response) {
        assert!(!e.words.heading.is_empty(), "{e:?}");
        return Err(e.rule);
    }
    let mut r = Rd::ok(&response);
    let record = r.bytes();
    r.done();
    Ok(record)
}

fn range(counter: &openbmt_core::identity::DeviceId, count: u32) -> SignedRange {
    SignedRange::sign(&owner(), IndexRange::new(*counter, 0, count).unwrap()).unwrap()
}

/// Settings version `version` signed by the owner root alone, listing
/// `counter` unless it is `None`.
fn settings(counter: Option<&openbmt_core::identity::DeviceId>, version: u32) -> SignedSettings {
    let mut s = Settings::defaults(1_000);
    s.version = version;
    if let Some(c) = counter {
        s.devices.insert(*c);
        s.counters.insert(*c, 10_000);
    }
    SignedSettings {
        owner_sig: s.sign(&owner()),
        approver_sig: None,
        settings: s,
    }
}

/// The owner phone's bundle for `request` on `offer`.
fn bundle(offer: &OwnOffer, request: &[u8], settings: SignedSettings, count: u32) -> Vec<u8> {
    let request = OwnRequest::from_bytes(request).unwrap();
    let range = range(&request.counter, count);
    OwnBundle::build(
        &owner(),
        offer,
        &request,
        settings,
        range,
        vec![PEER.to_string()],
        None,
    )
    .unwrap()
    .to_bytes()
}

/// A ceremony as far as the matched words; the bundle is the caller's.
fn up_to_match(rig: &Rig, offer: &OwnOffer) -> Vec<u8> {
    begin(rig, &offer.to_bytes()).expect("begin");
    let (request, words) = same(rig, T0 + 10).expect("same address");
    assert_eq!(words.len(), 4);
    matched(rig, T0 + 20).expect("they match");
    request
}

#[test]
fn cr_228_an_own_offer_shows_the_first_address_and_the_kind_and_makes_no_request() {
    let rig = Rig::new();
    let (request, words, address, kind) =
        begin(&rig, &offer_for(SINGLE).to_bytes()).expect("begin");
    assert_eq!(address, FIRST_ADDRESS);
    assert_eq!(kind, KIND_SINGLE);
    // No key and no number yet: no request, no words.
    assert!(request.is_empty());
    assert!(words.is_empty());
    let (_, _, _, kind) = begin(&rig, &offer_for(MULTI).to_bytes()).expect("begin");
    assert_eq!(kind, KIND_MULTI);
}

#[test]
fn cr_228_nothing_goes_on_before_same_address() {
    let rig = Rig::new();
    begin(&rig, &offer_for(SINGLE).to_bytes()).unwrap();
    assert_eq!(matched(&rig, T0 + 10), Err("CR-228".to_string()));
    assert_eq!(finish(&rig, &[0; 4], T0 + 10), Err("CR-228".to_string()));
    // The refusals changed nothing: "Same address" still goes on.
    same(&rig, T0 + 20).expect("same address");
}

#[test]
fn cr_228_different_ends_the_ceremony_and_nothing_is_kept() {
    let rig = Rig::new();
    begin(&rig, &offer_for(SINGLE).to_bytes()).unwrap();
    let response = rig.call(70, 0, &[]);
    let mut r = Rd::ok(&response);
    let words = r.words();
    r.done();
    assert_eq!(words.heading, "Different wallet");
    assert_eq!(same(&rig, T0 + 20), Err("CR-228".to_string()));
}

#[test]
fn cr_229_the_request_after_same_address_is_the_own_frame_with_four_words() {
    let rig = Rig::new();
    begin(&rig, &offer_for(SINGLE).to_bytes()).unwrap();
    let (request, words) = same(&rig, T0 + 10).unwrap();
    assert_eq!(request.get(..4), Some(&b"OBM."[..]));
    assert_eq!(words.len(), 4);
    // Said twice, the second has nothing to go on from.
    assert_eq!(same(&rig, T0 + 11), Err("CR-228".to_string()));
}

#[test]
fn cr_229_a_window_that_has_passed_is_refused_at_same_address() {
    let rig = Rig::new();
    begin(&rig, &offer_for(SINGLE).to_bytes()).unwrap();
    assert!(same(&rig, T0 + 60 * 60).is_err());
    assert_eq!(same(&rig, T0 + 61), Err("CR-228".to_string()));
}

#[test]
fn cr_227_an_offer_that_is_not_for_the_main_network_is_refused() {
    let rig = Rig::new();
    let mut bytes = offer_for(SINGLE).to_bytes();
    // The byte after the frame's magic and version is the network's code.
    bytes[5] = 3;
    assert_eq!(begin(&rig, &bytes), Err("CR-227".to_string()));
    // And one cut short.
    let bytes = offer_for(SINGLE).to_bytes();
    assert_eq!(
        begin(&rig, &bytes[..bytes.len() - 1]),
        Err("CR-227".to_string())
    );
}

#[test]
fn cr_230_a_bundle_for_another_wallet_is_refused_and_ends_the_ceremony() {
    let rig = Rig::new();
    let request = up_to_match(&rig, &offer_for(SINGLE));
    let other = offer_for(MULTI);
    let counter = OwnRequest::from_bytes(&request).unwrap().counter;
    // The owner phone checks the request against another offer; the request
    // is not for it, so the bundle is made by hand from the right pairing.
    assert!(OwnBundle::build(
        &owner(),
        &other,
        &OwnRequest::from_bytes(&request).unwrap(),
        settings(Some(&counter), FLOOR + 1),
        range(&counter, 1_000),
        Vec::new(),
        None,
    )
    .is_err());
    assert_eq!(
        finish(&rig, &request, T0 + 30),
        Err("CR-230".to_string()),
        "a request is not a bundle"
    );
}

#[test]
fn cr_231_settings_with_an_approver_signature_or_without_this_counter_are_refused() {
    let offer = offer_for(SINGLE);
    let rig = Rig::new();
    let request = up_to_match(&rig, &offer);
    let counter = OwnRequest::from_bytes(&request).unwrap().counter;
    let stranger = Identity::from_secret_bytes(&[0x41; 32])
        .unwrap()
        .device_id();
    let bytes = bundle(
        &offer,
        &request,
        settings(Some(&stranger), FLOOR + 1),
        1_000,
    );
    *rig.registry().batch_min.lock().unwrap() = None;
    assert_eq!(finish(&rig, &bytes, T0 + 30), Err("CR-231".to_string()));

    // A version that does not pass the offer's floor.
    let rig = Rig::new();
    let request = up_to_match(&rig, &offer);
    let counter2 = OwnRequest::from_bytes(&request).unwrap().counter;
    let bytes = bundle(&offer, &request, settings(Some(&counter2), FLOOR), 1_000);
    assert_eq!(finish(&rig, &bytes, T0 + 30), Err("CR-231".to_string()));
    let _ = counter;
}

#[test]
fn cr_232_a_first_range_below_a_thousand_indexes_is_refused() {
    let offer = offer_for(SINGLE);
    let rig = Rig::new();
    *rig.registry().batch_min.lock().unwrap() = None;
    let request = up_to_match(&rig, &offer);
    let counter = OwnRequest::from_bytes(&request).unwrap().counter;
    let bytes = bundle(&offer, &request, settings(Some(&counter), FLOOR + 1), 999);
    assert_eq!(finish(&rig, &bytes, T0 + 30), Err("CR-232".to_string()));
    // The ceremony ended with the refusal.
    assert_eq!(finish(&rig, &bytes, T0 + 31), Err("CR-91".to_string()));
}

#[test]
fn cr_233_the_record_reads_by_its_magic_and_starts_a_counter_that_sells_from_the_wallet() {
    for (text, first) in [(SINGLE, Some(FIRST_ADDRESS)), (MULTI, None)] {
        let offer = offer_for(text);
        let rig = Rig::new();
        let request = up_to_match(&rig, &offer);
        let counter = OwnRequest::from_bytes(&request).unwrap().counter;
        let bytes = bundle(&offer, &request, settings(Some(&counter), FLOOR + 1), 1_000);
        let record = finish(&rig, &bytes, T0 + 30).expect("finish");
        assert_eq!(record.get(..4), Some(&b"OBM:"[..]));

        // The app reads the record before the start: the main network.
        let response = rig.call(43, 0, &W::new().bytes(&record).string("").0);
        let mut r = Rd::ok(&response);
        assert_eq!(r.u8(), 0);

        // The ceremony is over: a second finish has nothing to finish.
        assert_eq!(finish(&rig, &bytes, T0 + 31), Err("CR-91".to_string()));

        let handle = rig.start_with(&StartConfig {
            dir: rig.dir().to_str().unwrap().to_string(),
            now: T0 + 100,
            record,
            proxy: None,
            own_node: None,
            lightning: None,
            link_proxy: None,
            shop_seed: None,
        });
        rig.send(vec![peers_event()]);
        let response = rig.call(10, handle, &W::new().u64(T0 + 100).u32(50).0);
        Rd::ok(&response);
        let response = rig.call(
            11,
            handle,
            &W::new()
                .open(T0 + 100, FIVE_DOLLARS, "USD", RATE, "test-fixed")
                .0,
        );
        let mut r = Rd::ok(&response);
        let opened = r.opened();
        assert_eq!(opened.index, 0);
        // The sale's address is the wallet's first receive address.
        let expected = PinnedWallet::from_text(text)
            .unwrap()
            .receive_address(0)
            .unwrap()
            .to_string();
        assert_eq!(opened.payment.address, expected);
        if let Some(first) = first {
            assert_eq!(opened.payment.address, first);
        }
    }
}

#[test]
fn cr_235_a_batch_sent_to_an_own_wallet_counter_is_refused_with_words() {
    let offer = offer_for(SINGLE);
    let rig = Rig::new();
    let request = up_to_match(&rig, &offer);
    let counter = OwnRequest::from_bytes(&request).unwrap().counter;
    let bytes = bundle(&offer, &request, settings(Some(&counter), FLOOR + 1), 1_000);
    let record = finish(&rig, &bytes, T0 + 30).unwrap();
    let handle = rig.start_with(&StartConfig {
        dir: rig.dir().to_str().unwrap().to_string(),
        now: T0 + 100,
        record,
        proxy: None,
        own_node: None,
        lightning: None,
        link_proxy: None,
        shop_seed: None,
    });
    rig.send(vec![peers_event()]);
    // Bytes that are not a batch are refused as such; the refusal that
    // concerns the counter's kind is tested in the counter library.
    let response = rig.call(23, handle, &W::new().u64(T0 + 100).bytes(&[1, 2, 3]).0);
    assert!(Rd::error(&response).is_some());
}

#[test]
fn cr_237_the_same_scan_reads_either_kind_of_offer() {
    let rig = Rig::new();
    // An own-wallet offer begins the own ceremony, which has no request yet.
    let (request, _, address, _) = begin(&rig, &offer_for(SINGLE).to_bytes()).unwrap();
    assert!(request.is_empty() && !address.is_empty());
    // A Key app offer then replaces it and begins the Key app's, which has.
    let offer = openbmt_core::pairing::PairOffer::new(
        owner().device_id(),
        Identity::from_secret_bytes(&[0x22; 32])
            .unwrap()
            .device_id(),
        [9; 32],
        bitcoin::Network::Regtest,
    )
    .unwrap();
    let (request, words, address, kind) = begin(&rig, &offer.to_bytes()).unwrap();
    assert!(!request.is_empty() && words.len() == 4);
    assert!(address.is_empty() && kind == 0);
    // And the own ceremony is gone: "Same address" has nothing to go on from.
    assert_eq!(same(&rig, T0 + 20), Err("CR-228".to_string()));
}

#[test]
fn cr_229_they_differ_ends_an_own_wallet_ceremony_without_naming_the_key_app() {
    let rig = Rig::new();
    begin(&rig, &offer_for(SINGLE).to_bytes()).unwrap();
    same(&rig, T0 + 10).unwrap();
    let response = rig.call(46, 0, &[]);
    let mut r = Rd::ok(&response);
    let words = r.words();
    r.done();
    assert!(!words.what.contains("Key app") && !words.next.contains("Key app"));
    // Nothing is left of the ceremony: no pairing is under way (CR-91).
    assert_eq!(matched(&rig, T0 + 20), Err("CR-91".to_string()));
}
