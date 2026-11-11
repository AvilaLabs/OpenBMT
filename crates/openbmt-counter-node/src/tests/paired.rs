//! CR-91 to CR-99: the counter's side of pairing, with the Key app's and the
//! owner phone's steps done by the functions of `openbmt_core`.

use bitcoin::Network;
use openbmt_core::batch::{BatchError, BatchPolicy};
use openbmt_core::bundle::CounterBundle;
use openbmt_core::identity::Identity;
use openbmt_core::pairing::{
    check_request, owner_cosign, PairError, PairOffer, PairRequest, SignedPairing,
    PAIRING_WINDOW_SECS,
};
use openbmt_core::settings::{Settings, SettingsError, SignedSettings};
use openbmt_core::wire::DecodeError;

use super::kit::{batch, id};
use crate::paired::{
    batch_policy, Ceremony, PairRefused, PairingRecord, BATCH_MIN_COUNT, MAGIC_PAIRING_RECORD,
};

const T0: u64 = 5_000;
const NET: Network = Network::Regtest;
/// What the counter's batches are made for (see `kit::batch`).
const WALLET: [u8; 32] = [9; 32];
/// The floor the Key app signs into the pairing.
const FLOOR: u32 = 4;
/// A policy small enough for the three-address batches of the tests.
const SMALL: BatchPolicy = BatchPolicy { min_count: 1 };

fn offer_on(network: Network) -> PairOffer {
    PairOffer::new(id(1).device_id(), id(2).device_id(), WALLET, network).unwrap()
}

fn offer() -> PairOffer {
    offer_on(NET)
}

/// A counter with identity `n` that began a ceremony on `offer`.
fn begun(offer: &PairOffer, n: u8, nonce: u8) -> (Ceremony, PairRequest) {
    let (ceremony, started) =
        Ceremony::begin(&offer.to_bytes(), "Front", [n; 32], [nonce; 32], T0).unwrap();
    (ceremony, PairRequest::from_bytes(&started.request).unwrap())
}

/// What the Key app and the owner phone do for `request`: the pairing both
/// roots sign, with `floor` as the settings version.
fn signed_pairing(offer: &PairOffer, request: &PairRequest, floor: u32) -> SignedPairing {
    let pending = check_request(offer, request).unwrap();
    let (entry, sig) = pending.approve(&id(2), floor).unwrap();
    owner_cosign(offer, entry, sig, &id(1)).unwrap()
}

/// Settings of `version` that list counter `n`, signed by both roots.
fn settings_for(n: u8, version: u32) -> SignedSettings {
    let mut s = Settings::defaults(1_000);
    s.version = version;
    s.devices.insert(id(n).device_id());
    s.counters.insert(id(n).device_id(), 10_000);
    sign_both(s)
}

fn sign_both(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

fn bundle_bytes(
    pairing: SignedPairing,
    settings: SignedSettings,
    with_batch: bool,
    peers: &[&str],
) -> Vec<u8> {
    CounterBundle::new(
        pairing,
        settings,
        with_batch.then(|| batch(0, 3)),
        peers.iter().map(|p| p.to_string()).collect(),
    )
    .unwrap()
    .to_bytes()
}

/// A confirmed ceremony and the honest bundle for it.
fn ready() -> (Ceremony, Vec<u8>) {
    let (mut c, request) = begun(&offer(), 3, 7);
    c.confirm(T0 + 20).unwrap();
    let bytes = bundle_bytes(
        signed_pairing(&offer(), &request, FLOOR),
        settings_for(3, FLOOR + 1),
        true,
        &["10.0.0.2:8333"],
    );
    (c, bytes)
}

fn finish(c: &mut Ceremony, bytes: &[u8]) -> Result<PairingRecord, PairRefused> {
    c.finish(bytes, &SMALL, T0 + 30)
}

#[test]
fn cr_91_the_counter_fixes_the_offer_and_then_shows_a_request_the_key_app_accepts() {
    let o = offer();
    let (c, started) = Ceremony::begin(&o.to_bytes(), "Front", [3; 32], [7; 32], T0).unwrap();
    assert_eq!(c.network(), Some(NET));
    assert!(!c.confirmed());
    let request = PairRequest::from_bytes(&started.request).unwrap();
    // The request is for the counter's own key, binds to the offer and
    // carries the number the caller drew.
    assert_eq!(request.counter, id(3).device_id());
    assert_eq!(request.nonce, [7; 32]);
    assert_eq!(request.binding, o.binding());
    // The Key app takes it and shows the same four words.
    let pending = check_request(&o, &request).unwrap();
    assert_eq!(pending.session_words(), started.words);
    // Another number is another request and other words.
    let (_, other) = Ceremony::begin(&o.to_bytes(), "Front", [3; 32], [8; 32], T0).unwrap();
    assert_ne!(other.request, started.request);
    assert_ne!(other.words, started.words);
}

#[test]
fn cr_91_begin_refuses_what_the_ceremony_refuses() {
    let bytes = offer().to_bytes();
    assert_eq!(
        Ceremony::begin(&bytes[..40], "A", [3; 32], [7; 32], T0).unwrap_err(),
        PairRefused::Pair(PairError::Decode(DecodeError::Truncated))
    );
    // The counter's key is one of the roots.
    assert_eq!(
        Ceremony::begin(&bytes, "A", [1; 32], [7; 32], T0).unwrap_err(),
        PairRefused::Pair(PairError::CounterIsRoot)
    );
    assert_eq!(
        Ceremony::begin(&bytes, "", [3; 32], [7; 32], T0).unwrap_err(),
        PairRefused::Pair(PairError::InvalidName)
    );
    // A secret that is not a key is the random source's failure.
    assert_eq!(
        Ceremony::begin(&bytes, "A", [0; 32], [7; 32], T0).unwrap_err(),
        PairRefused::NoRandomness
    );
}

#[test]
fn cr_91_a_ceremony_is_used_once_and_a_second_finish_finds_none() {
    let (mut c, bytes) = ready();
    assert!(finish(&mut c, &bytes).is_ok());
    assert_eq!(finish(&mut c, &bytes).unwrap_err(), PairRefused::NoCeremony);
    assert_eq!(c.confirm(T0 + 40).unwrap_err(), PairRefused::NoCeremony);
    // A refused finish uses it up as well.
    let (mut c, request) = begun(&offer(), 3, 7);
    c.confirm(T0).unwrap();
    let bad = bundle_bytes(
        signed_pairing(&offer(), &request, FLOOR),
        settings_for(4, FLOOR + 1),
        false,
        &[],
    );
    assert_eq!(finish(&mut c, &bad).unwrap_err(), PairRefused::NotOnDevices);
    assert_eq!(finish(&mut c, &bad).unwrap_err(), PairRefused::NoCeremony);
}

#[test]
fn cr_91_the_secret_is_not_in_what_a_ceremony_or_a_record_prints() {
    let (c, bytes) = ready();
    assert!(!format!("{c:?}").contains("3, 3, 3"));
    let mut c = c;
    let record = finish(&mut c, &bytes).unwrap();
    let text = format!("{record:?}");
    assert!(!text.contains("identity_secret"));
    assert!(!text.contains("3, 3, 3, 3"));
}

#[test]
fn cr_92_the_bundle_is_not_taken_before_they_match() {
    let (mut c, request) = begun(&offer(), 3, 7);
    let bytes = bundle_bytes(
        signed_pairing(&offer(), &request, FLOOR),
        settings_for(3, FLOOR + 1),
        true,
        &[],
    );
    assert_eq!(
        finish(&mut c, &bytes).unwrap_err(),
        PairRefused::Pair(PairError::NotConfirmed)
    );
    // Nothing was used: after the tap the same bundle is taken.
    c.confirm(T0 + 5).unwrap();
    assert!(c.confirmed());
    assert!(finish(&mut c, &bytes).is_ok());
}

#[test]
fn cr_92_a_code_that_is_not_a_bundle_changes_nothing() {
    let (mut c, bytes) = ready();
    for junk in [
        Vec::new(),
        b"OBMs".to_vec(),
        bytes[..bytes.len() - 1].to_vec(),
        offer().to_bytes(),
    ] {
        assert!(matches!(
            finish(&mut c, &junk).unwrap_err(),
            PairRefused::Bundle(_)
        ));
    }
    assert!(finish(&mut c, &bytes).is_ok());
}

#[test]
fn cr_92_the_window_is_ten_minutes_from_the_start() {
    // At 600 seconds it is taken, at 601 it is not.
    let (mut c, bytes) = ready();
    assert!(c.finish(&bytes, &SMALL, T0 + PAIRING_WINDOW_SECS).is_ok());
    let (mut c, bytes) = ready();
    assert_eq!(
        c.finish(&bytes, &SMALL, T0 + PAIRING_WINDOW_SECS + 1)
            .unwrap_err(),
        PairRefused::Pair(PairError::Expired)
    );
    // The person is not asked to scan the owner phone after the window.
    let (mut c, _) = begun(&offer(), 3, 7);
    assert_eq!(
        c.confirm(T0 + PAIRING_WINDOW_SECS + 1).unwrap_err(),
        PairRefused::Pair(PairError::Expired)
    );
    assert_eq!(
        c.confirm(T0 - 1).unwrap_err(),
        PairRefused::Pair(PairError::ClockWentBack)
    );
    let (mut c, bytes) = ready();
    assert_eq!(
        c.finish(&bytes, &SMALL, T0 - 1).unwrap_err(),
        PairRefused::Pair(PairError::ClockWentBack)
    );
}

#[test]
fn cr_92_a_bundle_for_another_counter_or_another_try_is_refused() {
    // Another counter's ceremony, on the same offer.
    let (mut mine, _) = begun(&offer(), 3, 7);
    mine.confirm(T0).unwrap();
    let (_, theirs) = begun(&offer(), 4, 7);
    let bytes = bundle_bytes(
        signed_pairing(&offer(), &theirs, FLOOR),
        settings_for(4, FLOOR + 1),
        false,
        &[],
    );
    assert_eq!(
        finish(&mut mine, &bytes).unwrap_err(),
        PairRefused::Pair(PairError::WrongCounter)
    );
    // The same counter's earlier try, with another number.
    let (mut now, _) = begun(&offer(), 3, 8);
    now.confirm(T0).unwrap();
    let (_, earlier) = begun(&offer(), 3, 7);
    let bytes = bundle_bytes(
        signed_pairing(&offer(), &earlier, FLOOR),
        settings_for(3, FLOOR + 1),
        false,
        &[],
    );
    assert_eq!(
        finish(&mut now, &bytes).unwrap_err(),
        PairRefused::Pair(PairError::WrongNonce)
    );
}

#[test]
fn cr_92_the_entry_must_be_signed_by_both_roots_the_counter_fixed() {
    // Roots the counter did not scan: another Key app signs.
    let other = PairOffer::new(id(1).device_id(), id(5).device_id(), WALLET, NET).unwrap();
    let (mut c, request) = begun(&offer(), 3, 7);
    c.confirm(T0).unwrap();
    let pending = check_request(&offer(), &request).unwrap();
    let (entry, _) = pending.approve(&id(2), FLOOR).unwrap();
    let forged = Identity::from_secret_bytes(&[5; 32]).unwrap();
    let sig = forged.sign_tagged(openbmt_core::identity::TAG_PAIRING, &entry.encode());
    let signed = owner_cosign(&other, entry, sig, &id(1)).unwrap();
    let bytes = bundle_bytes(signed, settings_for(3, FLOOR + 1), false, &[]);
    assert_eq!(
        finish(&mut c, &bytes).unwrap_err(),
        PairRefused::Pair(PairError::ApproverSignatureInvalid)
    );
}

#[test]
fn cr_93_roots_wallet_and_network_are_the_offers() {
    for network in [
        Network::Bitcoin,
        Network::Testnet,
        Network::Signet,
        Network::Regtest,
    ] {
        let o = offer_on(network);
        let (mut c, request) = begun(&o, 3, 7);
        assert_eq!(c.network(), Some(network));
        c.confirm(T0).unwrap();
        let bytes = bundle_bytes(
            signed_pairing(&o, &request, FLOOR),
            settings_for(3, FLOOR + 1),
            true,
            &[],
        );
        let record = finish(&mut c, &bytes).unwrap();
        assert_eq!(record.network, network);
        assert_eq!(record.wallet, WALLET);
        assert_eq!(record.roots.owner, id(1).device_id());
        assert_eq!(record.roots.approver, id(2).device_id());
        assert_eq!(record.settings_floor, FLOOR);
    }
}

#[test]
fn cr_93_the_network_is_in_the_words_and_the_request_binding() {
    // The same key and number on two networks give other words and another
    // binding, so a counter shown the wrong network fails the comparison.
    let (_, main) = Ceremony::begin(
        &offer_on(Network::Bitcoin).to_bytes(),
        "Front",
        [3; 32],
        [7; 32],
        T0,
    )
    .unwrap();
    let (_, test) = Ceremony::begin(
        &offer_on(Network::Testnet).to_bytes(),
        "Front",
        [3; 32],
        [7; 32],
        T0,
    )
    .unwrap();
    assert_ne!(main.words, test.words);
    let a = PairRequest::from_bytes(&main.request).unwrap();
    let b = PairRequest::from_bytes(&test.request).unwrap();
    assert_ne!(a.binding, b.binding);
    // The Key app on the main network refuses the testnet counter's request.
    assert_eq!(
        check_request(&offer_on(Network::Bitcoin), &b).unwrap_err(),
        PairError::BindingMismatch
    );
}

#[test]
fn cr_94_the_settings_need_both_roots_signatures() {
    let try_settings = |signed: SignedSettings| {
        let (mut c, request) = begun(&offer(), 3, 7);
        c.confirm(T0).unwrap();
        let bytes = bundle_bytes(
            signed_pairing(&offer(), &request, FLOOR),
            signed,
            false,
            &[],
        );
        finish(&mut c, &bytes).unwrap_err()
    };
    // No Key app signature.
    let mut s = settings_for(3, FLOOR + 1);
    s.approver_sig = None;
    assert_eq!(
        try_settings(s),
        PairRefused::Settings(SettingsError::ApproverRequired)
    );
    // The Key app's signature from another key, and the owner's.
    let mut s = settings_for(3, FLOOR + 1);
    s.approver_sig = Some(s.settings.sign(&id(6)));
    assert_eq!(
        try_settings(s),
        PairRefused::Settings(SettingsError::ApproverSignatureInvalid)
    );
    let mut s = settings_for(3, FLOOR + 1);
    s.owner_sig = s.settings.sign(&id(6));
    assert_eq!(
        try_settings(s),
        PairRefused::Settings(SettingsError::OwnerSignatureInvalid)
    );
    // Signed settings changed after signing.
    let mut s = settings_for(3, FLOOR + 1);
    s.settings.tor_on = !s.settings.tor_on;
    assert_eq!(
        try_settings(s),
        PairRefused::Settings(SettingsError::OwnerSignatureInvalid)
    );
}

#[test]
fn cr_94_the_settings_must_be_newer_than_the_pairings_floor() {
    for (version, ok) in [(FLOOR - 1, false), (FLOOR, false), (FLOOR + 1, true)] {
        let (mut c, request) = begun(&offer(), 3, 7);
        c.confirm(T0).unwrap();
        let bytes = bundle_bytes(
            signed_pairing(&offer(), &request, FLOOR),
            settings_for(3, version),
            false,
            &[],
        );
        match finish(&mut c, &bytes) {
            Ok(_) => assert!(ok, "version {version}"),
            Err(e) => {
                assert!(!ok, "version {version}");
                assert_eq!(
                    e,
                    PairRefused::SettingsOld {
                        floor: FLOOR,
                        got: version
                    }
                );
            }
        }
    }
}

#[test]
fn cr_95_this_counter_must_be_on_the_device_and_counter_lists() {
    let run = |signed: SignedSettings| {
        let (mut c, request) = begun(&offer(), 3, 7);
        c.confirm(T0).unwrap();
        let bytes = bundle_bytes(
            signed_pairing(&offer(), &request, FLOOR),
            signed,
            false,
            &[],
        );
        finish(&mut c, &bytes)
    };
    // Settings that list another counter only.
    assert_eq!(
        run(settings_for(4, FLOOR + 1)).unwrap_err(),
        PairRefused::NotOnDevices
    );
    // Settings that list no counter at all.
    let mut s = Settings::defaults(1_000);
    s.version = FLOOR + 1;
    assert_eq!(run(sign_both(s)).unwrap_err(), PairRefused::NotOnDevices);
    // A device that is not among the counters.
    let mut s = Settings::defaults(1_000);
    s.version = FLOOR + 1;
    s.devices.insert(id(3).device_id());
    assert_eq!(run(sign_both(s)).unwrap_err(), PairRefused::NotOnCounters);
    // Listed in both, among others.
    let mut s = Settings::defaults(1_000);
    s.version = FLOOR + 1;
    for n in [3, 4] {
        s.devices.insert(id(n).device_id());
        s.counters.insert(id(n).device_id(), 5_000);
    }
    assert!(run(sign_both(s)).is_ok());
}

#[test]
fn cr_96_the_first_batch_goes_through_the_batch_checks() {
    let run = |first: Option<openbmt_core::batch::SignedBatch>, policy: BatchPolicy| {
        let (mut c, request) = begun(&offer(), 3, 7);
        c.confirm(T0).unwrap();
        let bundle = CounterBundle::new(
            signed_pairing(&offer(), &request, FLOOR),
            settings_for(3, FLOOR + 1),
            first,
            vec![],
        )
        .unwrap();
        c.finish(&bundle.to_bytes(), &policy, T0 + 1)
    };
    assert!(run(Some(batch(0, 3)), SMALL).is_ok());
    assert!(run(None, SMALL).is_ok());
    // For another wallet.
    let mut other_wallet = batch(0, 3);
    other_wallet.batch.wallet_checksum = [8; 32];
    assert_eq!(
        run(Some(other_wallet), SMALL).unwrap_err(),
        PairRefused::Batch(BatchError::WrongWallet)
    );
    // For another counter.
    let mut other_counter = batch(0, 3);
    other_counter.batch.counter = id(4).device_id();
    assert_eq!(
        run(Some(other_counter), SMALL).unwrap_err(),
        PairRefused::Batch(BatchError::WrongCounter)
    );
    // Signed by the wrong key.
    let mut forged = batch(0, 3);
    forged.approver_sig = forged.batch.sign(&id(6));
    assert_eq!(
        run(Some(forged), SMALL).unwrap_err(),
        PairRefused::Batch(BatchError::ApproverSignatureInvalid)
    );
}

#[test]
fn cr_96_the_smallest_batch_is_the_counters_own_number() {
    assert_eq!(BATCH_MIN_COUNT, 1_000);
    assert_eq!(batch_policy().min_count, 1_000);
    // A record cannot carry another: its pairing uses the constant.
    let (mut c, bytes) = ready();
    let record = finish(&mut c, &bytes).unwrap();
    assert_eq!(record.pairing().batch_policy, batch_policy());
    // Under the counter's own rule the three-address batch of the tests is
    // too small, whatever the bundle says.
    let (mut c, request) = begun(&offer(), 3, 7);
    c.confirm(T0).unwrap();
    let bytes = bundle_bytes(
        signed_pairing(&offer(), &request, FLOOR),
        settings_for(3, FLOOR + 1),
        true,
        &[],
    );
    assert_eq!(
        c.finish(&bytes, &batch_policy(), T0 + 1).unwrap_err(),
        PairRefused::Batch(BatchError::CountTooSmall { min: 1_000, got: 3 })
    );
}

#[test]
fn cr_97_the_record_round_trips_and_decodes_strictly() {
    let (mut c, bytes) = ready();
    let record = finish(&mut c, &bytes).unwrap();
    let enc = record.to_bytes();
    assert_eq!(&enc[..4], &MAGIC_PAIRING_RECORD);
    assert_eq!(&enc[..5], b"OBMt\x01");
    assert_eq!(PairingRecord::from_bytes(&enc).unwrap(), record);
    // Without a batch and peers too.
    let mut bare = record.clone();
    bare.first_batch = None;
    bare.peers.clear();
    assert_eq!(PairingRecord::from_bytes(&bare.to_bytes()).unwrap(), bare);
    for valid in [enc.clone(), bare.to_bytes()] {
        for n in 0..valid.len() {
            assert!(
                PairingRecord::from_bytes(&valid[..n]).is_err(),
                "prefix {n}"
            );
        }
        let mut longer = valid.clone();
        longer.push(0);
        assert_eq!(
            PairingRecord::from_bytes(&longer).unwrap_err(),
            DecodeError::TrailingBytes
        );
        let mut m = valid.clone();
        for i in 0..m.len() {
            for bit in 0..8 {
                m[i] ^= 1 << bit;
                if let Ok(v) = PairingRecord::from_bytes(&m) {
                    assert_eq!(v.to_bytes(), m, "byte {i} bit {bit}");
                }
                m[i] ^= 1 << bit;
            }
        }
    }
}

#[test]
fn cr_97_the_record_refuses_what_is_not_a_pairing() {
    let (mut c, bytes) = ready();
    let enc = finish(&mut c, &bytes).unwrap().to_bytes();
    let bad = |edit: &dyn Fn(&mut Vec<u8>)| {
        let mut e = enc.clone();
        edit(&mut e);
        PairingRecord::from_bytes(&e).unwrap_err()
    };
    assert_eq!(
        bad(&|e| e[..4].copy_from_slice(b"OBMs")),
        DecodeError::BadMagic
    );
    assert_eq!(bad(&|e| e[4] = 2), DecodeError::UnsupportedVersion(2));
    // The secret, the roots, the network.
    assert!(matches!(
        bad(&|e| e[5..37].fill(0)),
        DecodeError::NonCanonical(_)
    ));
    assert_eq!(bad(&|e| e[37..69].fill(0)), DecodeError::InvalidKey);
    assert!(matches!(
        bad(&|e| e.copy_within(37..69, 69)),
        DecodeError::NonCanonical(_)
    ));
    assert!(matches!(
        bad(&|e| e[5 + 32 + 32 + 32 + 32] = 9),
        DecodeError::NonCanonical(_)
    ));
    // The length of the settings.
    let at = 5 + 32 * 4 + 1 + 4;
    assert_eq!(
        bad(&|e| e[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes())),
        DecodeError::TooLarge
    );
}

#[test]
fn cr_97_a_saved_record_is_checked_again_when_the_counter_starts() {
    let (mut c, bytes) = ready();
    let record = finish(&mut c, &bytes).unwrap();
    let back = PairingRecord::from_bytes(&record.to_bytes()).unwrap();
    assert_eq!(back.check(&SMALL), Ok(()));
    assert_eq!(back.identity().unwrap().device_id(), id(3).device_id());
    // A record whose settings were swapped for ones of another owner.
    let mut tampered = back.clone();
    tampered.settings.owner_sig = tampered.settings.settings.sign(&id(6));
    assert_eq!(
        PairingRecord::from_bytes(&tampered.to_bytes())
            .unwrap()
            .check(&SMALL)
            .unwrap_err(),
        PairRefused::Settings(SettingsError::OwnerSignatureInvalid)
    );
    // A record for another counter than its own secret names.
    let mut other = back;
    other.identity_secret = [4; 32];
    assert_eq!(other.check(&SMALL).unwrap_err(), PairRefused::NotOnDevices);
}

#[test]
fn cr_99_peers_are_hints_kept_as_given_and_not_part_of_what_is_checked() {
    let run = |peers: &[&str]| {
        let (mut c, request) = begun(&offer(), 3, 7);
        c.confirm(T0).unwrap();
        let bytes = bundle_bytes(
            signed_pairing(&offer(), &request, FLOOR),
            settings_for(3, FLOOR + 1),
            false,
            peers,
        );
        finish(&mut c, &bytes).unwrap()
    };
    assert!(run(&[]).peers.is_empty());
    let record = run(&["10.0.0.9:8333", "[::1]:48333", "10.0.0.2:8333"]);
    // Sorted by the bundle, as the record holds them.
    assert_eq!(
        record.peers,
        vec!["10.0.0.2:8333", "10.0.0.9:8333", "[::1]:48333"]
    );
    // Other peers change nothing else.
    let other = run(&["10.9.9.9:1"]);
    assert_eq!(other.roots, record.roots);
    assert_eq!(other.settings, record.settings);
    assert_eq!(other.wallet, record.wallet);
}

#[test]
fn cr_99_a_bundle_with_a_name_or_too_many_peers_is_not_a_bundle() {
    let (mut c, request) = begun(&offer(), 3, 7);
    c.confirm(T0).unwrap();
    let ok = bundle_bytes(
        signed_pairing(&offer(), &request, FLOOR),
        settings_for(3, FLOOR + 1),
        false,
        &["10.0.0.2:8333"],
    );
    // The bytes of the one peer, replaced by a name of the same length.
    let at = ok.windows(13).position(|w| w == b"10.0.0.2:8333").unwrap();
    let mut named = ok.clone();
    named[at..at + 13].copy_from_slice(b"example.org:1");
    assert!(matches!(
        finish(&mut c, &named).unwrap_err(),
        PairRefused::Bundle(DecodeError::NonCanonical(_))
    ));
    // The ceremony was not used by that, and the honest bundle is taken.
    assert!(finish(&mut c, &ok).is_ok());
}

#[test]
fn every_refusal_names_its_rule() {
    for (name, e) in crate::say::every_pair_refused() {
        assert!(e.to_string().starts_with(e.rule()), "{name}");
        assert!(e.rule().starts_with("CR-"), "{name}");
    }
}
