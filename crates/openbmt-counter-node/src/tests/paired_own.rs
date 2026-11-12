//! CR-227 to CR-233: the counter's side of pairing with the shop's own
//! wallet, with the owner phone's steps done by the functions of
//! `openbmt_core::pairing_own`.

use bitcoin::Network;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::DeviceId;
use openbmt_core::pairing_own::{OwnBundle, OwnOffer, OwnPairError, OwnRequest};
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
use openbmt_core::settings::{Authority, Settings, SettingsError, SignedSettings};
use openbmt_core::wire::{DecodeError, MAGIC_OWN_PAIRING_RECORD};

use super::kit::id;
use crate::paired::{PairRefused, PairingRecord};
use crate::paired_own::{OwnCeremony, OwnPairRefused, OwnPairingRecord, PairedRecord};

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
    }
}

fn range(counter: &DeviceId, count: u32) -> SignedRange {
    SignedRange::sign(&id(1), IndexRange::new(*counter, 0, count).unwrap()).unwrap()
}

/// A ceremony that said "Same address" and "They match" for counter 3, and
/// the bundle the owner phone makes for it from `settings` and a first range
/// of `count` indexes.
fn ready(settings: impl Fn(&DeviceId) -> SignedSettings, count: u32) -> (OwnCeremony, Vec<u8>) {
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
        None,
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
    let record = PairedRecord::OwnWallet(Box::new(record()));
    assert_eq!(
        record.authority(),
        Authority::OwnWallet {
            owner: id(1).device_id()
        }
    );
    assert!(record.roots().is_none() && record.key_app_pairing().is_none());
    assert_eq!(record.owner(), id(1).device_id());
    assert_eq!(record.network(), Network::Bitcoin);
}

/// A 2-of-3 description, for the wallet of another offer.
pub const MULTI: &str = "wsh(sortedmulti(2,[73c5da0a/48h/0h/0h/2h]xpub6DkFAXWQ2dHxq2vatrt9qyA3bXYU4ToWQwCHbf5XB2mSTexcHZCeKS1VZYcPoBd5X8yVcbXFHJR9R8UCVpt82VX1VhR28mCyxUFL4r6KFrf/<0;1>/*,[b8688df1/48h/0h/0h/2h]xpub6FQya7zGhR92kacYsNnjreouvnHJMpXYsUXnW6NJJAJRCKsa26TzDy4LdnGhEurr3d6y1J8PJ7EEMKQp74XTqYvmGJNogYXSKDszYHtF8mX/<0;1>/*,[28645006/48h/0h/0h/2h]xpub6DnEBNkSJKBYQmsbhS1sP9cNdtU5c9PLFGCjTJmxicxc13WB8zNNGQazabQpyFAGW5bV9tMko4uBxDxjUKL6dSAcx1tEbgEHtgSqyRsekh6/<0;1>/*))#34zcp4yt";
