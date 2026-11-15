//! CR-227 to CR-233: the counter's side of pairing with the shop's own
//! wallet, with the owner phone's steps done by the functions of
//! `openbmt_core::pairing_own`.

use bitcoin::Network;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::DeviceId;
use openbmt_core::pairing_own::{OwnBundle, OwnOffer, OwnPairError, OwnRequest};
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
use openbmt_core::relay::{RelayHint, RelayRoute, SignedRelayHint};
use openbmt_core::settings::{Authority, Settings, SettingsError, SignedSettings};
use openbmt_core::wire::{DecodeError, MAGIC_OWN_PAIRING_RECORD};
use openbmt_core::words::WORD_COUNT;

use super::kit::id;
use crate::paired::{PairRefused, PairingRecord};
use crate::paired_own::{
    OwnCeremony, OwnPairRefused, OwnPairingRecord, PairedRecord, RecordRefused,
};

const T0: u64 = 5_000;
const FLOOR: u32 = 4;
/// The Key Maker's test seed, account 0 (WL-06).
const SINGLE: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
const FIRST_ADDRESS: &str = "bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj";
pub(super) const POLICY: BatchPolicy = BatchPolicy { min_count: 1_000 };

fn offer() -> OwnOffer {
    OwnOffer::new(
        Network::Bitcoin,
        id(1).device_id(),
        FLOOR,
        PinnedWallet::from_text(SINGLE).unwrap(),
    )
    .unwrap()
}

fn settings_for(counter: &DeviceId, version: u32) -> SignedSettings {
    let mut s = Settings::defaults(1_000);
    s.version = version;
    s.devices.insert(*counter);
    s.counters.insert(*counter, 10_000);
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s,
        wallet_sig: None,
    }
}

fn range(counter: &DeviceId, count: u32) -> SignedRange {
    SignedRange::sign(&id(1), IndexRange::new(*counter, 0, count).unwrap()).unwrap()
}

/// A ceremony that said "Same address" and "They match" for counter 3, and
/// the bundle the owner phone makes for it from `settings` and a first range
/// of `count` indexes.
fn ready(settings: impl Fn(&DeviceId) -> SignedSettings, count: u32) -> (OwnCeremony, Vec<u8>) {
    ready_with(settings, count, None)
}

/// [`ready`] with a relay in the bundle.
fn ready_with(
    settings: impl Fn(&DeviceId) -> SignedSettings,
    count: u32,
    relay: Option<SignedRelayHint>,
) -> (OwnCeremony, Vec<u8>) {
    let (mut c, shown) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    assert_eq!(shown.address, FIRST_ADDRESS);
    let started = c.address_same("Front", [3; 32], [7; 32], T0 + 5).unwrap();
    c.confirm(T0 + 10).unwrap();
    let request = OwnRequest::from_bytes(&started.request).unwrap();
    let counter = request.counter;
    let bundle = OwnBundle::build(
        &id(1),
        &offer(),
        &request,
        settings(&counter),
        range(&counter, count),
        vec!["10.0.0.2:8333".to_string()],
        relay,
    )
    .unwrap();
    (c, bundle.to_bytes())
}

pub(super) fn record() -> OwnPairingRecord {
    let (mut c, bytes) = ready(|c| settings_for(c, FLOOR + 1), 1_000);
    c.finish(&bytes, &POLICY, T0 + 20).unwrap()
}

#[test]
fn cr_228_begin_makes_no_key_and_no_number() {
    let (c, shown) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    assert_eq!(shown.address, FIRST_ADDRESS);
    assert!(c.is_open() && !c.has_request() && c.words().is_none());
    let mut c = c;
    // Nothing goes on before "Same address".
    assert_eq!(c.confirm(T0 + 1), Err(OwnPairRefused::NoCeremony));
    assert_eq!(
        c.finish(&[], &POLICY, T0 + 1).unwrap_err(),
        OwnPairRefused::NoCeremony
    );
    assert!(c.is_open());
}

#[test]
fn cr_229_the_ten_minutes_run_from_the_offer() {
    let (mut c, _) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    assert!(matches!(
        c.address_same("Front", [3; 32], [7; 32], T0 + 601),
        Err(OwnPairRefused::Pair(OwnPairError::Expired))
    ));
    assert!(!c.is_open());
    let (mut c, _) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    assert!(matches!(
        c.address_same("Front", [3; 32], [7; 32], T0 - 1),
        Err(OwnPairRefused::Pair(OwnPairError::ClockWentBack))
    ));
    let (mut c, _) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    c.address_same("Front", [3; 32], [7; 32], T0 + 600).unwrap();
    assert!(c.confirm(T0 + 601).is_err());
}

#[test]
fn cr_230_a_bundle_is_not_taken_before_they_match_and_a_bad_one_changes_nothing() {
    let (mut c, bytes) = ready(|c| settings_for(c, FLOOR + 1), 1_000);
    // A fresh ceremony for the same counter that has not matched.
    let (mut fresh, _) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    fresh
        .address_same("Front", [3; 32], [7; 32], T0 + 5)
        .unwrap();
    assert_eq!(
        fresh.finish(&bytes, &POLICY, T0 + 20).unwrap_err(),
        OwnPairRefused::Pair(OwnPairError::NotConfirmed)
    );
    // A code that is not a bundle leaves the ceremony as it was.
    assert!(matches!(
        c.finish(&[1, 2, 3], &POLICY, T0 + 20),
        Err(OwnPairRefused::Bundle(_))
    ));
    assert!(c.is_open());
    c.finish(&bytes, &POLICY, T0 + 20)
        .expect("the honest bundle");
    // Used up.
    assert_eq!(
        c.finish(&bytes, &POLICY, T0 + 21).unwrap_err(),
        OwnPairRefused::NoCeremony
    );
}

#[test]
fn cr_230_a_bundle_for_another_number_is_refused_and_ends_the_ceremony() {
    let (mut c, _) = ready(|c| settings_for(c, FLOOR + 1), 1_000);
    // The owner phone answers a different request of the same counter.
    let (mut other, _) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    let started = other
        .address_same("Front", [3; 32], [8; 32], T0 + 5)
        .unwrap();
    let request = OwnRequest::from_bytes(&started.request).unwrap();
    let bundle = OwnBundle::build(
        &id(1),
        &offer(),
        &request,
        settings_for(&request.counter, FLOOR + 1),
        range(&request.counter, 1_000),
        Vec::new(),
        None,
    )
    .unwrap()
    .to_bytes();
    assert!(matches!(
        c.finish(&bundle, &POLICY, T0 + 20),
        Err(OwnPairRefused::Pair(OwnPairError::WrongNonce))
    ));
    assert!(!c.is_open());
}

#[test]
fn cr_231_the_first_settings_are_owner_signed_newer_and_list_this_counter() {
    // The owner root alone: an approver signature is refused.
    let (mut c, bytes) = ready(
        |c| {
            let mut s = settings_for(c, FLOOR + 1);
            s.approver_sig = Some(s.settings.sign(&id(2)));
            s
        },
        1_000,
    );
    assert!(matches!(
        c.finish(&bytes, &POLICY, T0 + 20),
        Err(OwnPairRefused::Settings(SettingsError::Invalid(_)))
    ));
    // A version that does not pass the offer's.
    let (mut c, bytes) = ready(|c| settings_for(c, FLOOR), 1_000);
    assert_eq!(
        c.finish(&bytes, &POLICY, T0 + 20).unwrap_err(),
        OwnPairRefused::SettingsOld {
            floor: FLOOR,
            got: FLOOR
        }
    );
    // Not on the device list, or not among the counters.
    let (mut c, bytes) = ready(|_| settings_for(&id(9).device_id(), FLOOR + 1), 1_000);
    assert_eq!(
        c.finish(&bytes, &POLICY, T0 + 20).unwrap_err(),
        OwnPairRefused::NotOnDevices
    );
    let (mut c, bytes) = ready(
        |c| {
            let mut s = settings_for(c, FLOOR + 1).settings;
            s.counters.clear();
            SignedSettings {
                owner_sig: s.sign(&id(1)),
                approver_sig: None,
                settings: s,
                wallet_sig: None,
            }
        },
        1_000,
    );
    assert_eq!(
        c.finish(&bytes, &POLICY, T0 + 20).unwrap_err(),
        OwnPairRefused::NotOnCounters
    );
}

#[test]
fn cr_231_first_settings_that_name_a_lightning_node_are_refused_at_pairing_and_at_start() {
    let naming = |c: &DeviceId| {
        let mut s = settings_for(c, FLOOR + 1).settings;
        s.lightning_node = Some([2u8; 33]);
        SignedSettings {
            owner_sig: s.sign(&id(1)),
            approver_sig: None,
            settings: s,
            wallet_sig: None,
        }
    };
    let (mut c, bytes) = ready(naming, 1_000);
    assert_eq!(
        c.finish(&bytes, &POLICY, T0 + 20).unwrap_err(),
        OwnPairRefused::Settings(SettingsError::RedirectAtPairing)
    );
    assert!(!c.is_open());
    // The same settings in a saved record are refused at start.
    let mut saved = record();
    saved.settings = naming(&saved.identity().unwrap().device_id());
    assert_eq!(
        saved.check(&POLICY).unwrap_err(),
        OwnPairRefused::Settings(SettingsError::RedirectAtPairing)
    );
    let words = OwnPairRefused::Settings(SettingsError::RedirectAtPairing).words();
    assert_eq!(words.heading, "Code not taken");
}

#[test]
fn cr_232_the_first_range_holds_at_least_the_minimum() {
    let (mut c, bytes) = ready(|c| settings_for(c, FLOOR + 1), 999);
    let refused = c.finish(&bytes, &POLICY, T0 + 20).unwrap_err();
    assert!(matches!(
        refused,
        OwnPairRefused::Pair(OwnPairError::RangeTooSmall {
            got: 999,
            need: 1_000
        })
    ));
    assert_eq!(refused.rule(), "CR-232");
    // 1,000 is enough.
    let (mut c, bytes) = ready(|c| settings_for(c, FLOOR + 1), 1_000);
    c.finish(&bytes, &POLICY, T0 + 20).expect("a thousand");
}

#[test]
fn cr_233_the_record_round_trips_strictly_and_is_told_apart_by_its_magic() {
    let record = record();
    let bytes = record.to_bytes();
    assert_eq!(bytes.get(..4), Some(&MAGIC_OWN_PAIRING_RECORD[..]));
    let again = OwnPairingRecord::from_bytes(&bytes).unwrap();
    assert_eq!(again.to_bytes(), bytes);
    for n in 0..bytes.len() {
        assert!(OwnPairingRecord::from_bytes(&bytes[..n]).is_err(), "{n}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(
        OwnPairingRecord::from_bytes(&longer).err(),
        Some(DecodeError::TrailingBytes)
    );
    match PairedRecord::from_bytes(&bytes).unwrap() {
        PairedRecord::OwnWallet(r) => assert_eq!(r.owner, id(1).device_id()),
        PairedRecord::KeyApp(_) => panic!("the magic says own wallet"),
    }
    // Anything that is not an own-wallet record is read as the Key app's,
    // which refuses what is not its own.
    assert!(matches!(
        PairedRecord::from_bytes(&[0; 40]),
        Err(crate::paired_own::RecordRefused::KeyApp(
            PairRefused::Record(_)
        ))
    ));
    let _ = PairingRecord::from_bytes;
}

#[test]
fn cr_233_a_saved_record_is_checked_again_at_start() {
    let record = record();
    record.check(&POLICY).expect("what was just made");
    // A smaller minimum than the range holds passes; a larger one does not.
    assert!(record.check(&BatchPolicy { min_count: 1_001 }).is_err());
    // Settings that lost the owner's signature.
    let mut bad = record.clone();
    bad.settings.owner_sig = bad.settings.settings.sign(&id(2));
    assert!(matches!(
        bad.check(&POLICY),
        Err(OwnPairRefused::Settings(_))
    ));
    // The wallet of another offer: the pairing names the first one's hash.
    let mut other = record.clone();
    other.wallet = PinnedWallet::from_text(MULTI).unwrap();
    assert!(other.check(&POLICY).is_err());
    // Not the main network.
    let mut net = record.clone();
    net.network = Network::Regtest;
    assert!(matches!(
        net.check(&POLICY),
        Err(OwnPairRefused::Pair(OwnPairError::NotMainNetwork))
    ));
}

#[test]
fn cr_233_the_secret_is_not_in_what_a_ceremony_or_a_record_prints() {
    let record = record();
    let text = format!(
        "{record:?} {:?}",
        PairedRecord::OwnWallet(Box::new(record.clone()))
    );
    assert!(
        !text.contains("3, 3, 3") && !text.contains("0x03"),
        "{text}"
    );
    let (c, _) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    assert!(!format!("{c:?}").contains("secret"));
}

#[test]
fn cr_236_the_authority_of_an_own_wallet_record_is_the_owner_root_alone() {
    let own = record();
    let expected = Authority::pinned_own_wallet(id(1).device_id(), &own.wallet, own.starter);
    let record = PairedRecord::OwnWallet(Box::new(own));
    assert_eq!(record.authority(), expected);
    assert!(matches!(
        record.authority(),
        Authority::OwnWallet { owner, signers: Some(_), .. } if owner == id(1).device_id()
    ));
    assert!(record.roots().is_none() && record.key_app_pairing().is_none());
    assert_eq!(record.owner(), id(1).device_id());
    assert_eq!(record.network(), Network::Bitcoin);
}

fn relay_hint() -> RelayHint {
    RelayHint::new(
        RelayRoute::Direct("192.168.1.20:48333".parse().unwrap()),
        id(40).device_id().to_bytes(),
    )
    .unwrap()
}

fn other_hint() -> RelayHint {
    RelayHint::new(
        RelayRoute::Direct("192.168.1.21:48333".parse().unwrap()),
        id(41).device_id().to_bytes(),
    )
    .unwrap()
}

/// Byte offsets of the length fields of a record in version 1.
struct Offsets {
    pairing_len: usize,
    settings_len: usize,
}

fn offsets(record: &OwnPairingRecord) -> Offsets {
    // Magic and version (5), secret (32), owner (32), network (1), floor (4),
    // wallet length (2), wallet; then the pairing length (2), the pairing.
    let pairing_len = 76 + record.wallet.to_bytes().len();
    let settings_len = pairing_len + 2 + record.pairing.to_bytes().len();
    Offsets {
        pairing_len,
        settings_len,
    }
}

#[test]
fn cr_229_the_clock_bounds_of_same_address_and_they_match_are_exact() {
    let begun = || OwnCeremony::begin(&offer().to_bytes(), T0).unwrap().0;
    // At the second the offer was read, and at the last second of the ten
    // minutes, both steps are taken.
    for now in [T0, T0 + 600] {
        let mut c = begun();
        c.address_same("Front", [3; 32], [7; 32], now).unwrap();
        let mut c = begun();
        c.address_same("Front", [3; 32], [7; 32], T0).unwrap();
        c.confirm(now).unwrap();
        assert!(c.confirmed());
    }
    // One second before and one second after are not.
    let mut c = begun();
    assert_eq!(
        c.address_same("Front", [3; 32], [7; 32], T0 - 1),
        Err(OwnPairRefused::Pair(OwnPairError::ClockWentBack))
    );
    let mut c = begun();
    assert_eq!(
        c.address_same("Front", [3; 32], [7; 32], T0 + 601),
        Err(OwnPairRefused::Pair(OwnPairError::Expired))
    );
    let mut c = begun();
    c.address_same("Front", [3; 32], [7; 32], T0).unwrap();
    assert_eq!(
        c.confirm(T0 - 1),
        Err(OwnPairRefused::Pair(OwnPairError::ClockWentBack))
    );
    assert!(!c.confirmed());
    assert_eq!(
        c.confirm(T0 + 601),
        Err(OwnPairRefused::Pair(OwnPairError::Expired))
    );
    assert!(!c.confirmed());
}

#[test]
fn cr_229_the_words_and_the_confirmation_are_shown_only_when_they_exist() {
    let (mut c, _) = OwnCeremony::begin(&offer().to_bytes(), T0).unwrap();
    assert!(!c.confirmed() && c.words().is_none());
    let started = c.address_same("Front", [3; 32], [7; 32], T0 + 5).unwrap();
    assert!(!c.confirmed());
    let words: [&str; WORD_COUNT] = started.words;
    assert_eq!(c.words(), Some(words));
    c.confirm(T0 + 10).unwrap();
    assert!(c.confirmed());
    assert_eq!(c.words(), Some(words));
    // Once the bundle is taken the ceremony is used up and shows no words.
    let (mut c, bytes) = ready(|c| settings_for(c, FLOOR + 1), 1_000);
    assert!(c.words().is_some());
    c.finish(&bytes, &POLICY, T0 + 20).unwrap();
    assert!(c.words().is_none());
    assert!(format!("{c:?}").contains("OwnCeremony"));
}

#[test]
fn cr_130_the_bundles_relay_is_kept_only_with_the_owner_roots_signature() {
    let signed = SignedRelayHint::sign(relay_hint(), &id(1));
    let (mut c, bytes) = ready_with(|c| settings_for(c, FLOOR + 1), 1_000, Some(signed));
    let kept = c.finish(&bytes, &POLICY, T0 + 20).unwrap();
    assert_eq!(kept.relay, Some(relay_hint()));
    assert_eq!(kept.to_bytes()[4], 2);
    // Another key's signature is refused, and the ceremony is used up.
    let forged = SignedRelayHint::sign(relay_hint(), &id(2));
    let (mut c, bytes) = ready_with(|c| settings_for(c, FLOOR + 1), 1_000, Some(forged));
    let refused = c.finish(&bytes, &POLICY, T0 + 20).unwrap_err();
    assert_eq!(refused, OwnPairRefused::RelayNotSigned);
    assert_eq!(refused.rule(), "CR-230");
    assert!(!c.is_open());
}

#[test]
fn cr_132_a_relay_for_counters_is_taken_only_from_the_pinned_owner() {
    let record = record();
    assert_eq!(record.relay, None);
    let signed = SignedRelayHint::sign(relay_hint(), &id(1));
    let next = record.with_relay(&signed).unwrap();
    assert_eq!(next.relay, Some(relay_hint()));
    assert_eq!(record.relay, None);
    // Nothing else changed.
    assert_eq!(
        OwnPairingRecord {
            relay: None,
            ..next.clone()
        },
        record
    );
    let again = next
        .with_relay(&SignedRelayHint::sign(other_hint(), &id(1)))
        .unwrap();
    assert_eq!(again.relay, Some(other_hint()));
    for n in [2u8, 3, 77] {
        assert_eq!(
            record
                .with_relay(&SignedRelayHint::sign(relay_hint(), &id(n)))
                .unwrap_err(),
            crate::paired::RelayRefused::NotFromOwner,
            "{n}"
        );
    }
    // The same through the record of either kind.
    let either = PairedRecord::OwnWallet(Box::new(record));
    let next = either.with_relay(&signed).unwrap();
    assert_eq!(next.relay(), Some(&relay_hint()));
    assert_eq!(either.relay(), None);
    assert!(either
        .with_relay(&SignedRelayHint::sign(relay_hint(), &id(2)))
        .is_err());
}

#[test]
fn cr_233_every_field_of_the_record_comes_back_in_either_version() {
    let plain = record();
    for (starter, relay, version) in [
        (false, None, 1),
        (false, Some(relay_hint()), 2),
        (true, None, 3),
        (true, Some(relay_hint()), 3),
    ] {
        let mut r = plain.clone();
        r.starter = starter;
        r.relay = relay;
        let bytes = r.to_bytes();
        assert_eq!(bytes[4], version);
        let again = OwnPairingRecord::from_bytes(&bytes).unwrap();
        assert_eq!(again, r);
        assert_eq!(again.identity_secret, [3; 32]);
        assert_eq!(again.owner, id(1).device_id());
        assert_eq!(again.settings_floor, FLOOR);
        assert_eq!(again.peers, vec!["10.0.0.2:8333".to_string()]);
        assert_eq!(again.relay, r.relay);
        assert_eq!(again.starter, starter);
    }
    // The record of either kind hands out the same fields.
    let mut with_relay = plain.clone();
    with_relay.relay = Some(relay_hint());
    let either = PairedRecord::OwnWallet(Box::new(with_relay.clone()));
    assert_eq!(either.peers(), ["10.0.0.2:8333".to_string()]);
    assert_eq!(either.relay(), Some(&relay_hint()));
    assert_eq!(either.to_bytes(), with_relay.to_bytes());
    assert_eq!(either.settings(), &with_relay.settings);
    assert_eq!(
        either.identity().unwrap().device_id(),
        plain.identity().unwrap().device_id()
    );
    assert_eq!(
        PairedRecord::from_bytes(&either.to_bytes()).unwrap(),
        either
    );
}

#[test]
fn cr_233_the_key_apps_record_answers_for_its_own_kind() {
    let (mut c, bytes) = super::paired::ready();
    let key_app = super::paired::finish(&mut c, &bytes).unwrap();
    let either = PairedRecord::KeyApp(Box::new(key_app.clone()));
    assert!(either.key_app_pairing().is_some());
    assert!(PairedRecord::OwnWallet(Box::new(record()))
        .key_app_pairing()
        .is_none());
    assert_eq!(either.roots(), Some(key_app.roots));
    assert_eq!(either.peers(), key_app.peers.as_slice());
    assert_eq!(either.relay(), None);
    assert_eq!(either.to_bytes(), key_app.to_bytes());
    assert_eq!(
        PairedRecord::from_bytes(&either.to_bytes()).unwrap(),
        either
    );
    assert!(format!("{either:?}").contains("PairingRecord"));
}

#[test]
fn cr_233_a_saved_record_is_checked_with_a_range_book_only_when_there_is_one() {
    let mut other = record();
    other.wallet = PinnedWallet::from_text(MULTI).unwrap();
    let either = PairedRecord::OwnWallet(Box::new(other));
    let wrong = RecordRefused::OwnWallet(OwnPairRefused::Pair(OwnPairError::WrongWallet));
    assert_eq!(either.check(&POLICY), Err(wrong.clone()));
    assert_eq!(either.check_with_ranges(&POLICY, None), Err(wrong.clone()));
    // A book that does not prove the switch changes nothing.
    assert_eq!(either.check_with_ranges(&POLICY, Some(&[])), Err(wrong));
    // The record of an honest pairing passes with or without a book.
    let honest = PairedRecord::OwnWallet(Box::new(record()));
    honest.check(&POLICY).unwrap();
    honest.check_with_ranges(&POLICY, Some(&[])).unwrap();
}

#[test]
fn cr_233_every_length_of_a_record_is_checked_at_its_limit() {
    let record = record();
    let bytes = record.to_bytes();
    let at = offsets(&record);
    let too_large = Err(DecodeError::TooLarge);
    // The pairing: 512 bytes is the most that is read, 513 is refused first.
    let mut m = bytes.clone();
    m[at.pairing_len..at.pairing_len + 2].copy_from_slice(&512u16.to_le_bytes());
    assert_ne!(OwnPairingRecord::from_bytes(&m).map(|_| ()), too_large);
    m[at.pairing_len..at.pairing_len + 2].copy_from_slice(&513u16.to_le_bytes());
    assert_eq!(OwnPairingRecord::from_bytes(&m).map(|_| ()), too_large);
    // The settings: 64 KiB.
    let mut m = bytes.clone();
    m[at.settings_len..at.settings_len + 4].copy_from_slice(&65_536u32.to_le_bytes());
    assert_ne!(OwnPairingRecord::from_bytes(&m).map(|_| ()), too_large);
    m[at.settings_len..at.settings_len + 4].copy_from_slice(&65_537u32.to_le_bytes());
    assert_eq!(OwnPairingRecord::from_bytes(&m).map(|_| ()), too_large);
    // Settings well over a kilobyte, which a shop with many counters has,
    // are read back whole.
    let mut big = record.clone();
    let mut s = big.settings.settings.clone();
    for n in 100..160u8 {
        s.counters.insert(id(n).device_id(), 10_000);
    }
    big.settings = SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s,
        wallet_sig: None,
    };
    assert!(big.settings.to_bytes().len() > 2_000);
    assert_eq!(OwnPairingRecord::from_bytes(&big.to_bytes()).unwrap(), big);
}

#[test]
fn cr_233_the_peers_of_a_record_are_at_most_eight_valid_and_ascending() {
    let mut r = record();
    r.peers = (1..=8).map(|n| format!("10.0.0.{n}:8333")).collect();
    assert_eq!(OwnPairingRecord::from_bytes(&r.to_bytes()).unwrap(), r);
    r.peers = (1..=9).map(|n| format!("10.0.0.{n}:8333")).collect();
    assert_eq!(
        OwnPairingRecord::from_bytes(&r.to_bytes()).err(),
        Some(DecodeError::TooLarge)
    );
    // Ascending is strict: the same peer twice, or a lower one after a
    // higher, does not read.
    for peers in [
        vec!["10.0.0.1:8333", "10.0.0.1:8333"],
        vec!["10.0.0.2:8333", "10.0.0.1:8333"],
    ] {
        r.peers = peers.into_iter().map(str::to_owned).collect();
        assert_eq!(
            OwnPairingRecord::from_bytes(&r.to_bytes()).err(),
            Some(DecodeError::NonCanonical(
                "peers are not in ascending order"
            ))
        );
    }
    r.peers = vec!["10.0.0.1:8333".into(), "10.0.0.2:8333".into()];
    assert_eq!(OwnPairingRecord::from_bytes(&r.to_bytes()).unwrap(), r);
    r.peers = vec!["not a peer".into()];
    assert_eq!(
        OwnPairingRecord::from_bytes(&r.to_bytes()).err(),
        Some(DecodeError::NonCanonical("a peer is not ip:port"))
    );
}

#[test]
fn cr_233_a_wallet_that_does_not_decode_reports_why() {
    let record = record();
    let mut bytes = record.to_bytes();
    // The wallet starts after the magic, version, secret, owner, network,
    // floor and its length.
    bytes[76] ^= 0xff;
    assert_eq!(
        OwnPairingRecord::from_bytes(&bytes).err(),
        Some(DecodeError::BadMagic)
    );
    // A wallet that decodes but is refused for another reason.
    let wallet = record.wallet.to_bytes();
    let mut bytes = record.to_bytes();
    let last = 76 + wallet.len() - 1;
    bytes[last] ^= 0xff;
    assert_eq!(
        OwnPairingRecord::from_bytes(&bytes).err(),
        Some(DecodeError::NonCanonical("the wallet is refused"))
    );
}

#[test]
fn cr_233_what_the_record_prints_names_it_and_not_the_secret() {
    let record = record();
    let text = format!("{record:?}");
    assert!(text.starts_with("OwnPairingRecord"), "{text}");
    assert!(
        text.contains("settings_floor: 4") && text.contains("Bitcoin"),
        "{text}"
    );
    let either = format!("{:?}", PairedRecord::OwnWallet(Box::new(record)));
    assert!(either.starts_with("OwnPairingRecord"), "{either}");
}

#[test]
fn cr_233_each_refusal_says_its_rule_and_its_words() {
    use crate::paired::PairRefused;
    let texts = [
        (
            OwnPairRefused::NoCeremony,
            "CR-228: no pairing is under way at this step",
        ),
        (
            OwnPairRefused::NoRandomness,
            "CR-229: the system gave no random numbers",
        ),
        (
            OwnPairRefused::Bundle(DecodeError::Truncated),
            "CR-230: the owner phone's code is not a bundle: ",
        ),
        (
            OwnPairRefused::Settings(SettingsError::RedirectAtPairing),
            "CR-231: the bundle's settings are refused: ",
        ),
        (
            OwnPairRefused::SettingsOld { floor: 4, got: 3 },
            "CR-231: the bundle's settings are version 3, which does not pass the offer's \
             version 4",
        ),
        (
            OwnPairRefused::NotOnDevices,
            "CR-231: this counter is not on the settings' device list",
        ),
        (
            OwnPairRefused::NotOnCounters,
            "CR-231: this counter is not among the settings' counters",
        ),
        (
            OwnPairRefused::RelayNotSigned,
            "CR-230: the bundle's relay is not signed by the owner root this pairing pinned",
        ),
        (
            OwnPairRefused::Record(DecodeError::Truncated),
            "CR-233: the saved pairing does not read back: ",
        ),
        (
            OwnPairRefused::RecordKey,
            "CR-233: the saved pairing holds a secret that is not a key",
        ),
    ];
    for (refused, text) in &texts {
        assert!(refused.to_string().starts_with(text), "{refused:?}");
    }
    let pair = OwnPairRefused::Pair(OwnPairError::Expired);
    assert_eq!(
        pair.to_string(),
        format!("CR-230: {}", OwnPairError::Expired)
    );
    assert_eq!(
        OwnPairRefused::Pair(OwnPairError::NotMainNetwork).rule(),
        "CR-227"
    );
    // The record of either kind says what its own kind says.
    let own = RecordRefused::OwnWallet(OwnPairRefused::NoCeremony);
    assert_eq!(own.rule(), "CR-228");
    assert_eq!(own.to_string(), OwnPairRefused::NoCeremony.to_string());
    let key_app = RecordRefused::KeyApp(PairRefused::NoCeremony);
    assert_eq!(key_app.rule(), "CR-91");
    assert_eq!(key_app.to_string(), PairRefused::NoCeremony.to_string());
}
/// A 2-of-3 description, for the wallet of another offer.
pub const MULTI: &str = "wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt";
