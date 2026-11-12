use std::str::FromStr;

use bitcoin::bip32::{Fingerprint, Xpub};
use bitcoin::Network;

use super::*;
use crate::approval::ChangeClass;
use crate::identity::TAG_PAIR_SESSION;
use crate::pairing::{PairOffer, PairRequest};
use crate::pinned_wallet::MAX_RANGE_LEN;
use crate::relay::{RelayHint, RelayRoute};
use crate::settings::{Authority, Settings, SettingsError, SettingsStore, LIGHTNING_NODE_BYTES};
use crate::testutil::{assert_strict_decoding, identity, wallet};
use crate::wallet_single::{ImportedWallet, SingleKeyWallet};

/// The Key Maker's test seed (fingerprint `818bc9b6`), account 0 (WL-06).
fn single() -> PinnedWallet {
    let xpub = Xpub::from_str(
        "xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8",
    )
    .unwrap();
    let w = SingleKeyWallet::new(Fingerprint::from_str("818bc9b6").unwrap(), 0, xpub).unwrap();
    PinnedWallet::new(ImportedWallet::Single(w))
}

fn multi() -> PinnedWallet {
    PinnedWallet::new(ImportedWallet::Multisig(wallet(Network::Bitcoin)))
}

fn owner() -> Identity {
    identity(1)
}

fn counter() -> Identity {
    identity(3)
}

fn offer(wallet: PinnedWallet) -> OwnOffer {
    OwnOffer::new(Network::Bitcoin, owner().device_id(), 4, wallet).unwrap()
}

fn begin(offer: &OwnOffer) -> (OwnCounterPairing, OwnRequest) {
    OwnCounterPairing::begin(offer.clone(), &counter(), "Front", [9; 32], 100).unwrap()
}

fn range(start: u32, end: u32) -> SignedRange {
    SignedRange::sign(
        &owner(),
        IndexRange::new(counter().device_id(), start, end).unwrap(),
    )
    .unwrap()
}

fn settings() -> SignedSettings {
    let mut s = Settings::defaults(1_000);
    s.version = 5;
    let id = counter().device_id();
    s.devices.insert(id);
    s.counters.insert(id, 10_000);
    SignedSettings {
        owner_sig: s.sign(&owner()),
        approver_sig: None,
        settings: s,
    }
}

fn resign(mut s: Settings) -> SignedSettings {
    s.version += 1;
    SignedSettings {
        owner_sig: s.sign(&owner()),
        approver_sig: None,
        settings: s,
    }
}

fn bundle_for(offer: &OwnOffer, request: &OwnRequest) -> OwnBundle {
    OwnBundle::build(
        &owner(),
        offer,
        request,
        settings(),
        range(0, 1_000),
        vec!["10.0.0.2:8333".to_string(), "[::1]:48333".to_string()],
        None,
    )
    .unwrap()
}

fn relay() -> SignedRelayHint {
    let hint = RelayHint::new(
        RelayRoute::Direct("192.168.1.20:48333".parse().unwrap()),
        identity(40).device_id().to_bytes(),
    )
    .unwrap();
    SignedRelayHint::sign(hint, &owner())
}

#[test]
fn cr_227_the_offer_round_trips_strictly_for_both_kinds() {
    for wallet in [single(), multi()] {
        let bytes = offer(wallet).to_bytes();
        assert_eq!(&bytes[..5], b"OBM-\x01");
        assert_strict_decoding(&bytes, OwnOffer::from_bytes, OwnOffer::to_bytes);
    }
}

#[test]
fn cr_227_the_network_is_the_main_network() {
    assert_eq!(
        OwnOffer::new(Network::Testnet, owner().device_id(), 1, single()).err(),
        Some(OwnPairError::NotMainNetwork)
    );
    let mut bytes = offer(single()).to_bytes();
    bytes[5] = 1;
    assert!(OwnOffer::from_bytes(&bytes).is_err());
    bytes[5] = 9;
    assert!(OwnOffer::from_bytes(&bytes).is_err());
}

#[test]
fn cr_227_the_pinned_wallet_passes_wl_21_and_wl_22() {
    let bytes = offer(single()).to_bytes();
    // The hash inside the pinned wallet is the last 32 bytes of the frame.
    let mut bad_hash = bytes.clone();
    *bad_hash.last_mut().unwrap() ^= 1;
    assert!(OwnOffer::from_bytes(&bad_hash).is_err());
    // A wallet frame of another magic.
    let mut bad_magic = bytes.clone();
    bad_magic[42] = b'X';
    assert!(OwnOffer::from_bytes(&bad_magic).is_err());
    // A Key app offer is not an own-wallet offer.
    let key_app = PairOffer::new(
        owner().device_id(),
        identity(2).device_id(),
        single().checksum32(),
        Network::Bitcoin,
    )
    .unwrap();
    assert_eq!(
        OwnOffer::from_bytes(&key_app.to_bytes()).err(),
        Some(DecodeError::BadMagic)
    );
    assert_eq!(
        PairOffer::from_bytes(&bytes).err(),
        Some(DecodeError::BadMagic)
    );
}

#[test]
fn cr_227_the_offer_carries_the_floor_and_the_owner_root() {
    let o = offer(multi());
    let back = OwnOffer::from_bytes(&o.to_bytes()).unwrap();
    assert_eq!(back.floor, 4);
    assert_eq!(back.owner, owner().device_id());
    assert_eq!(back.network(), Network::Bitcoin);
}

#[test]
fn cr_228_the_first_address_is_index_zero_and_the_kind_is_named() {
    let o = offer(single());
    assert_eq!(
        o.first_address().unwrap().to_string(),
        "bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj"
    );
    assert_eq!(o.kind(), WalletKind::SingleKey);
    let m = offer(multi());
    assert_eq!(m.kind(), WalletKind::TwoOfThree);
    assert_eq!(
        m.first_address().unwrap(),
        m.wallet.receive_address(0).unwrap()
    );
    assert_ne!(m.first_address().unwrap(), o.first_address().unwrap());
}

#[test]
fn cr_229_the_request_is_strict_and_signed_by_the_counter() {
    let o = offer(single());
    let (_, request) = begin(&o);
    let bytes = request.to_bytes();
    assert_eq!(&bytes[..5], b"OBM.\x01");
    assert_strict_decoding(&bytes, OwnRequest::from_bytes, OwnRequest::to_bytes);
    assert!(check_own_request(&o, &request).is_ok());
    // A request of the Key app is not read as this one.
    assert_eq!(
        PairRequest::from_bytes(&bytes).err(),
        Some(DecodeError::BadMagic)
    );
    let mut forged = request.clone();
    forged.sig.0[3] ^= 1;
    assert_eq!(
        check_own_request(&o, &forged).err(),
        Some(OwnPairError::RequestSignatureInvalid)
    );
}

#[test]
fn cr_229_the_binding_covers_the_whole_offer() {
    let a = offer(single());
    let (_, request) = begin(&a);
    for other in [
        offer(multi()),
        OwnOffer::new(Network::Bitcoin, owner().device_id(), 5, single()).unwrap(),
        OwnOffer::new(Network::Bitcoin, identity(2).device_id(), 4, single()).unwrap(),
    ] {
        assert_ne!(other.binding(), a.binding());
        assert_eq!(
            check_own_request(&other, &request).err(),
            Some(OwnPairError::BindingMismatch)
        );
    }
}

#[test]
fn cr_229_the_words_are_made_under_their_own_tag() {
    let o = offer(single());
    let (c, _) = begin(&o);
    let words = c.session_words();
    assert_eq!(
        words,
        own_session_words(&o, &counter().device_id(), &[9; 32])
    );
    // The same fields under the Key app's tag give other words.
    let mut msg = o.body();
    msg.extend_from_slice(&counter().device_id().to_bytes());
    msg.extend_from_slice(&[9; 32]);
    assert_ne!(
        crate::words::fingerprint_words(&tagged_hash(TAG_PAIR_SESSION, &msg)),
        words
    );
    assert_ne!(
        tagged_hash(TAG_PAIR_OWN_BINDING, &o.body()),
        tagged_hash(crate::identity::TAG_PAIR_OFFER, &o.body())
    );
    // The number, the counter and the offer each move the digest.
    assert_ne!(
        own_session_digest(&o, &counter().device_id(), &[9; 32]),
        own_session_digest(&o, &counter().device_id(), &[8; 32])
    );
    assert_ne!(
        own_session_digest(&o, &counter().device_id(), &[9; 32]),
        own_session_digest(&o, &identity(4).device_id(), &[9; 32])
    );
    assert_ne!(
        own_session_digest(&o, &counter().device_id(), &[9; 32]),
        own_session_digest(&offer(multi()), &counter().device_id(), &[9; 32])
    );
    // The owner phone's side shows the same four words.
    let (_, request) = begin(&o);
    assert_eq!(
        check_own_request(&o, &request).unwrap().session_words(),
        words
    );
}

#[test]
fn cr_229_a_counter_that_is_the_owner_or_has_a_bad_name_is_refused() {
    let o = offer(single());
    assert_eq!(
        OwnCounterPairing::begin(o.clone(), &owner(), "Front", [9; 32], 0).err(),
        Some(OwnPairError::CounterIsOwner)
    );
    assert_eq!(
        OwnCounterPairing::begin(o, &counter(), "", [9; 32], 0).err(),
        Some(OwnPairError::InvalidName)
    );
}

#[test]
fn cr_230_the_bundle_is_strict_with_and_without_a_relay() {
    let o = offer(single());
    let (_, request) = begin(&o);
    let b = bundle_for(&o, &request);
    let bytes = b.to_bytes();
    assert_eq!(&bytes[..5], b"OBM/\x01");
    assert_strict_decoding(&bytes, OwnBundle::from_bytes, OwnBundle::to_bytes);
    let with = OwnBundle {
        relay: Some(relay()),
        ..b.clone()
    };
    let bytes = with.to_bytes();
    assert_eq!(&bytes[..5], b"OBM/\x02");
    assert_strict_decoding(&bytes, OwnBundle::from_bytes, OwnBundle::to_bytes);
    // A Key app bundle is not an own-wallet bundle.
    assert_eq!(
        OwnBundle::from_bytes(b"OBMs\x01").err(),
        Some(DecodeError::BadMagic)
    );
}

#[test]
fn cr_230_peers_are_sorted_checked_and_limited_as_cr_99_says() {
    let o = offer(single());
    let (_, request) = begin(&o);
    let make = |peers: Vec<String>| {
        OwnBundle::build(
            &owner(),
            &o,
            &request,
            settings(),
            range(0, 1_000),
            peers,
            None,
        )
    };
    let b = make(vec![
        "[::1]:48333".into(),
        "10.0.0.2:8333".into(),
        "10.0.0.2:8333".into(),
    ])
    .unwrap();
    assert_eq!(b.peers, vec!["10.0.0.2:8333", "[::1]:48333"]);
    assert_eq!(
        make(vec!["example.com:8333".into()]).err(),
        Some(OwnPairError::Bundle(BundleError::BadPeer))
    );
    let nine: Vec<String> = (1..=9).map(|n| format!("10.0.0.{n}:8333")).collect();
    assert_eq!(
        make(nine).err(),
        Some(OwnPairError::Bundle(BundleError::TooManyPeers))
    );
    // On the wire, peers out of order are refused.
    let b = make(vec!["10.0.0.2:8333".into(), "10.0.0.3:8333".into()]).unwrap();
    let mut bytes = b.to_bytes();
    let first = bytes
        .windows(13)
        .position(|w| w == b"10.0.0.2:8333")
        .unwrap();
    bytes[first + 7] = b'9';
    assert!(OwnBundle::from_bytes(&bytes).is_err());
}

#[test]
fn cr_230_the_pairing_checks_run_in_the_order_the_rule_gives() {
    let o = offer(single());
    let (c, request) = begin(&o);
    let b = bundle_for(&o, &request);
    // Not confirmed comes first, even for a pairing that is wrong.
    let (c2, _) = begin(&o);
    assert_eq!(
        c2.finish(&b.pairing, false, 100).err(),
        Some(OwnPairError::NotConfirmed)
    );
    // The time comes next.
    let (c3, _) = begin(&o);
    assert_eq!(
        c3.finish(&b.pairing, true, 99).err(),
        Some(OwnPairError::ClockWentBack)
    );
    let (c4, _) = begin(&o);
    assert_eq!(
        c4.finish(&b.pairing, true, 100 + PAIRING_WINDOW_SECS + 1)
            .err(),
        Some(OwnPairError::Expired)
    );
    // The window is ten minutes from the offer, inclusive.
    let (c5, _) = begin(&o);
    assert!(c5
        .finish(&b.pairing, true, 100 + PAIRING_WINDOW_SECS)
        .is_ok());
    let paired = c.finish(&b.pairing, true, 110).unwrap();
    assert_eq!(paired.offer, o);
    assert_eq!(paired.pairing, b.pairing);
}

#[test]
fn cr_230_a_pairing_for_another_counter_number_wallet_or_name_is_refused() {
    let o = offer(single());
    let (_, request) = begin(&o);
    let good = bundle_for(&o, &request).pairing;
    let resigned = |f: &dyn Fn(&mut PairingEntry)| {
        let mut entry = good.entry.clone();
        f(&mut entry);
        OwnSignedPairing::sign(&owner(), entry)
    };
    let cases: [(OwnSignedPairing, OwnPairError); 4] = [
        (
            resigned(&|e| e.counter = identity(4).device_id()),
            OwnPairError::WrongCounter,
        ),
        (resigned(&|e| e.nonce = [1; 32]), OwnPairError::WrongNonce),
        (
            resigned(&|e| e.wallet = multi().checksum32()),
            OwnPairError::WrongWallet,
        ),
        (
            resigned(&|e| e.name = "Back".to_string()),
            OwnPairError::WrongName,
        ),
    ];
    for (pairing, want) in cases {
        let (c, _) = begin(&o);
        assert_eq!(c.finish(&pairing, true, 110).err(), Some(want));
    }
    // Signed by a key that is not the offer's owner root, and signed under
    // the Key app's tag.
    let (c, _) = begin(&o);
    let stranger = OwnSignedPairing::sign(&identity(2), good.entry.clone());
    assert_eq!(
        c.finish(&stranger, true, 110).err(),
        Some(OwnPairError::OwnerSignatureInvalid)
    );
    let (c, _) = begin(&o);
    let mut other_tag = good.clone();
    other_tag.owner_sig = owner().sign_tagged("OpenBMT/pairing/v1", &good.entry.encode());
    assert_eq!(
        c.finish(&other_tag, true, 110).err(),
        Some(OwnPairError::OwnerSignatureInvalid)
    );
}

#[test]
fn cr_230_the_owner_phone_signs_only_as_the_offers_owner_root() {
    let o = offer(single());
    let (_, request) = begin(&o);
    let pending = check_own_request(&o, &request).unwrap();
    assert_eq!(pending.counter(), counter().device_id());
    assert_eq!(pending.name(), "Front");
    assert_eq!(
        pending.sign(&identity(2)).err(),
        Some(OwnPairError::NotTheOwner)
    );
    let signed = bundle_for(&o, &request).pairing;
    assert_eq!(signed.entry.wallet, single().checksum32());
    assert_eq!(signed.entry.settings_version, 4);
    assert_strict_decoding(
        &signed.to_bytes(),
        OwnSignedPairing::from_bytes,
        OwnSignedPairing::to_bytes,
    );
}

#[test]
fn cr_231_the_first_settings_need_the_owner_signature_only() {
    let first = settings();
    let authority = Authority::OwnWallet {
        owner: owner().device_id(),
    };
    assert!(first.verify_with(&authority, None).is_ok());
    // Not the owner root's signature.
    let mut wrong = first.clone();
    wrong.owner_sig = identity(2).sign_digest(&wrong.settings.hash());
    assert_eq!(
        wrong.verify_with(&authority, None).err(),
        Some(SettingsError::OwnerSignatureInvalid)
    );
    // A Key app signature is refused, even a good one.
    let mut with_key_app = first.clone();
    with_key_app.approver_sig = Some(identity(2).sign_digest(&first.settings.hash()));
    assert!(matches!(
        with_key_app.verify_with(&authority, None),
        Err(SettingsError::Invalid(_))
    ));
    // Invalid values are refused.
    let mut bad = first.settings.clone();
    bad.final_confirmations = 0;
    bad.min_confirmations = 9;
    let bad = SignedSettings {
        owner_sig: bad.sign(&owner()),
        approver_sig: None,
        settings: bad,
    };
    assert!(bad.verify_with(&authority, None).is_err());
    // The floor is the store's: a version at or below it is stale.
    let mut store = SettingsStore::with_authority(authority, 5);
    assert_eq!(
        store.apply(first.clone()).err(),
        Some(SettingsError::StaleVersion { current: 5, got: 5 })
    );
    let mut store = SettingsStore::with_authority(authority, 4);
    assert!(store.apply(first).is_ok());
}

#[test]
fn cr_232_the_first_range_is_checked_and_not_below_the_minimum() {
    let owner_id = owner().device_id();
    let me = counter().device_id();
    let min = FIRST_RANGE_MIN_COUNT;
    assert_eq!(min, 1_000);
    assert!(check_first_range(&range(0, min), &owner_id, &me, min).is_ok());
    assert!(check_first_range(&range(50, 50 + MAX_RANGE_LEN), &owner_id, &me, min).is_ok());
    assert_eq!(
        check_first_range(&range(0, min - 1), &owner_id, &me, min).err(),
        Some(OwnPairError::RangeTooSmall {
            got: min - 1,
            need: min
        })
    );
    // Another counter's range, and another root's signature.
    let other = SignedRange::sign(
        &owner(),
        IndexRange::new(identity(4).device_id(), 0, min).unwrap(),
    )
    .unwrap();
    assert_eq!(
        check_first_range(&other, &owner_id, &me, min).err(),
        Some(OwnPairError::Range(RangeError::WrongCounter))
    );
    let forged = SignedRange::sign(&identity(2), IndexRange::new(me, 0, min).unwrap()).unwrap();
    assert_eq!(
        check_first_range(&forged, &owner_id, &me, min).err(),
        Some(OwnPairError::Range(RangeError::BadSignature))
    );
}

#[test]
fn cr_234_the_ledger_round_trips_and_restarts_at_the_right_index() {
    let owner_id = owner().device_id();
    let me = counter().device_id();
    let mut ledger = RangeLedger::new(owner_id, me);
    assert_eq!(ledger.next().err(), Some(RangeError::UsedUp));
    ledger.accept(&range(0, 1_000)).unwrap();
    assert_eq!(ledger.next(), Ok(0));
    assert_eq!(ledger.next(), Ok(1));
    ledger.accept(&range(5_000, 5_010)).unwrap();
    let bytes = ledger.to_bytes();
    assert_eq!(&bytes[..5], b"OBM;\x01");
    assert_strict_decoding(
        &bytes,
        |b| RangeLedger::from_bytes(b, owner_id, me),
        RangeLedger::to_bytes,
    );
    let mut back = RangeLedger::from_bytes(&bytes, owner_id, me).unwrap();
    assert_eq!(back, ledger);
    assert_eq!(back.next(), Ok(2));
    assert_eq!(back.remaining(), 997 + 10);
    // Used-up ranges stay in the ledger, in order.
    let mut small = RangeLedger::new(owner_id, me);
    small.accept(&range(0, 2)).unwrap();
    small.next().unwrap();
    small.next().unwrap();
    assert_eq!(small.remaining(), 0);
    let mut kept = RangeLedger::from_bytes(&small.to_bytes(), owner_id, me).unwrap();
    assert_eq!(kept.signed().len(), 1);
    assert_eq!(kept.next(), Err(RangeError::UsedUp));
}

#[test]
fn cr_234_every_range_is_checked_again_when_the_slot_is_read() {
    let owner_id = owner().device_id();
    let me = counter().device_id();
    let mut ledger = RangeLedger::new(owner_id, me);
    ledger.accept(&range(0, 1_000)).unwrap();
    let bytes = ledger.to_bytes();
    // Another owner root, or another counter.
    assert!(RangeLedger::from_bytes(&bytes, identity(2).device_id(), me).is_err());
    assert!(RangeLedger::from_bytes(&bytes, owner_id, identity(4).device_id()).is_err());
    // A next index past the end of its range.
    let mut past = bytes.clone();
    let at = past.len() - 4;
    past[at..].copy_from_slice(&1_001u32.to_le_bytes());
    assert!(RangeLedger::from_bytes(&past, owner_id, me).is_err());
    // Two ranges that overlap.
    let mut two = RangeLedger::new(owner_id, me);
    two.accept(&range(0, 10)).unwrap();
    two.accept(&range(10, 20)).unwrap();
    let mut doubled = two.to_bytes();
    let overlapping = range(5, 15);
    let at = 5 + 4 + 113;
    doubled.splice(at..at + 109, overlapping.to_bytes());
    assert!(RangeLedger::from_bytes(&doubled, owner_id, me).is_err());
    // A count that cannot be present.
    let mut huge = bytes.clone();
    huge[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(RangeLedger::from_bytes(&huge, owner_id, me).is_err());
}

#[test]
fn cr_235_a_range_overlapping_what_is_handed_out_or_in_use_is_refused() {
    let owner_id = owner().device_id();
    let me = counter().device_id();
    let mut ledger = RangeLedger::new(owner_id, me);
    ledger.accept(&range(0, 100)).unwrap();
    ledger.next().unwrap();
    let before = ledger.clone();
    assert_eq!(
        ledger.accept(&range(0, 50)).err(),
        Some(RangeError::OverlapsUsed)
    );
    assert_eq!(
        ledger.accept(&range(50, 150)).err(),
        Some(RangeError::OverlapsTaken)
    );
    // Another counter's, and signed by another root.
    let other = SignedRange::sign(
        &owner(),
        IndexRange::new(identity(4).device_id(), 200, 300).unwrap(),
    )
    .unwrap();
    assert_eq!(ledger.accept(&other).err(), Some(RangeError::WrongCounter));
    let forged = SignedRange::sign(&identity(2), IndexRange::new(me, 200, 300).unwrap()).unwrap();
    assert_eq!(ledger.accept(&forged).err(), Some(RangeError::BadSignature));
    assert_eq!(ledger, before);
    ledger.accept(&range(100, 200)).unwrap();
    assert_eq!(ledger.signed().len(), 2);
}

#[test]
fn cr_236_only_a_free_change_is_taken_without_a_confirmation() {
    let authority = Authority::OwnWallet {
        owner: owner().device_id(),
    };
    let first = settings();
    let mut store = SettingsStore::with_authority(authority, 4);
    store.apply(first.clone()).unwrap();
    let base = first.settings;

    // Free: a limit lowered, tips turned on.
    let mut lower = base.clone();
    lower.light_per_sale.sats -= 1;
    lower.tips_on = true;
    assert!(store.apply(resign(lower)).is_ok());
    let base = store.current().unwrap().clone();

    // Loosen: a limit raised, a device added.
    let mut raised = base.clone();
    raised.light_per_sale.sats += 1;
    assert_eq!(
        store.apply(resign(raised)).err(),
        Some(SettingsError::ConfirmationRequired(ChangeClass::Loosen))
    );
    let mut added = base.clone();
    added.devices.insert(identity(9).device_id());
    assert_eq!(
        store.apply(resign(added)).err(),
        Some(SettingsError::ConfirmationRequired(ChangeClass::Loosen))
    );
    // Redirect: a Lightning node set.
    let mut redirect = base.clone();
    redirect.lightning_node = Some([2u8; LIGHTNING_NODE_BYTES]);
    let e = store.apply(resign(redirect)).unwrap_err();
    assert_eq!(
        e,
        SettingsError::ConfirmationRequired(ChangeClass::Redirect)
    );
    assert!(e.to_string().starts_with("CR-236"));
    // A refusal leaves the settings as they were.
    assert_eq!(store.current(), Some(&base));
}

#[test]
fn cr_236_the_key_app_authority_is_unchanged() {
    let roots =
        crate::batch::PinnedRoots::new(owner().device_id(), identity(2).device_id()).unwrap();
    let s = settings();
    // First settings are a loosening and need both signatures, as before.
    assert_eq!(s.verify(&roots, None), Err(SettingsError::ApproverRequired));
    assert_eq!(
        s.verify(&roots, None),
        s.verify_with(&Authority::KeyApp(roots), None)
    );
    let mut both = s.clone();
    both.approver_sig = Some(identity(2).sign_digest(&s.settings.hash()));
    assert_eq!(
        both.verify(&roots, None),
        Ok(crate::settings::Change::Loosening)
    );
    assert!(both.verify_signatures(&roots).is_ok());
    assert!(both
        .verify_signatures_with(&Authority::KeyApp(roots))
        .is_ok());
}
