//! CR-239 to CR-243: a change to the settings that waits for a counter's
//! confirmation. The node of a counter paired with the shop's own wallet is
//! started on a fake driver; the owner phone's steps are the functions of
//! `openbmt_core`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use openbmt_core::approval::{confirm_words, ConfirmError, CounterConfirmation};
use openbmt_core::confirmed::ConfirmedSettings;
use openbmt_core::records::RecordKind;
use openbmt_core::settings::{Change, Settings, SettingsError, SignedSettings};
use openbmt_counter::ConfirmHeld;

use super::kit::*;
use crate::confirm::{ChangeNotice, ConfirmRefused, FieldValue, PENDING_SECS};
use crate::dirstore::{CONFIRM_FILE, SETTINGS_FILE};
use crate::paired::PairingRecord;
use crate::settings::{decode_frame, effective_of, SettingsFault, SettingsFrame, SettingsRefused};
use crate::{DirStore, NodeConfig, PairedRecord};

const DAY: u64 = 86_400;

fn own_config(dir: &std::path::Path, time: &Arc<AtomicU64>) -> NodeConfig {
    let record = super::paired_own::record();
    let mut config = NodeConfig::new_own(
        dir,
        id(3),
        record.own_pairing(super::paired_own::POLICY),
        settings(),
        openbmt_net_io::DriverConfig::new(bitcoin::Network::Bitcoin, 0),
    );
    config.clock = Box::new(TestClock(time.clone()));
    config.health = Box::new(|_| healthy());
    config
}

fn own_rig(dir: &std::path::Path) -> Rig {
    let time = Arc::new(AtomicU64::new(T0));
    start_with(own_config(dir, &time), time)
}

/// The test settings at `version`, changed by `f`, signed by the owner root.
fn signed(version: u32, f: impl FnOnce(&mut Settings)) -> SignedSettings {
    let mut s = settings();
    s.version = version;
    f(&mut s);
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s,
    }
}

fn looser(version: u32) -> SignedSettings {
    signed(version, |s| s.light_per_sale.sats += 10)
}

/// The counter confirmed `signed` at its own screen; what the owner phone
/// does with it.
fn confirmed_by(who: u8, signed: &SignedSettings) -> ConfirmedSettings {
    let c = CounterConfirmation::sign(&id(who), &signed.settings.hash());
    ConfirmedSettings::new(signed.clone(), c)
}

fn confirm_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join(CONFIRM_FILE)
}

#[test]
fn cr_239_a_loosening_is_kept_and_not_put_in_force() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let before = rig.node.records_len();
    let change = looser(2);
    assert_eq!(rig.node.take_settings(&change), Ok(None));
    // Not in force, not in the settings file, no record of it.
    assert_eq!(rig.node.settings().version, 1);
    assert_eq!(rig.node.counter().settings().version, 1);
    assert!(DirStore::open(dir.path())
        .unwrap()
        .settings_file()
        .unwrap()
        .is_none());
    assert_eq!(rig.node.records_len(), before);
    // It is kept: the OBMS frame and nothing else.
    let kept = std::fs::read(confirm_path(dir.path())).unwrap();
    assert_eq!(kept, change.to_bytes());
    assert_eq!(&kept[..4], b"OBMS");
    let waiting = rig.node.pending_change().unwrap();
    assert_eq!(waiting.version, 2);
    assert_eq!(waiting.hash, change.settings.hash());
    assert_eq!(waiting.words, confirm_words(&change.settings.hash()));
    assert!(!waiting.confirmed);
    assert_eq!(waiting.arrived, T0);
    assert_eq!(waiting.expires_at, T0 + PENDING_SECS);
    assert!(rig
        .node
        .take_notes()
        .iter()
        .any(|n| n.starts_with("CR-239")));
}

#[test]
fn cr_239_a_tightening_is_taken_as_before_and_nothing_waits() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    assert_eq!(
        rig.node
            .take_settings(&signed(2, |s| s.light_per_sale.sats -= 1)),
        Ok(Some(Change::Tightening))
    );
    assert_eq!(rig.node.settings().version, 2);
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_path(dir.path()).exists());
}

#[test]
fn cr_239_a_redirect_is_still_refused() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let redirect = signed(2, |s| {
        s.lightning_node = Some([2u8; openbmt_core::settings::LIGHTNING_NODE_BYTES]);
    });
    let err = rig.node.take_settings(&redirect).unwrap_err();
    assert_eq!(
        err,
        SettingsRefused::Verify(SettingsError::ConfirmationRequired(
            openbmt_core::approval::ChangeClass::Redirect
        ))
    );
    assert!(err.to_string().starts_with("CR-239"));
    assert_eq!(rig.node.settings().version, 1);
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_path(dir.path()).exists());
    // The manager is told it needs the wallet.
    assert_eq!(
        rig.node.change_notice(),
        Some(ChangeNotice::NeedsWallet { version: 2 })
    );
    assert_eq!(
        err.words().heading,
        ChangeNotice::NeedsWallet { version: 2 }.words().heading
    );
    // A change that redirects and loosens is a Redirect too.
    let both = signed(3, |s| {
        s.lightning_node = Some([2u8; openbmt_core::settings::LIGHTNING_NODE_BYTES]);
        s.light_per_sale.sats += 10;
    });
    assert!(rig.node.take_settings(&both).is_err());
    assert!(rig.node.pending_change().is_none());
}

#[test]
fn cr_239_settings_that_fail_a_check_are_refused_not_kept() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let mut bad = looser(2);
    bad.owner_sig = looser(3).owner_sig;
    assert!(matches!(
        rig.node.take_settings(&bad),
        Err(SettingsRefused::Verify(
            SettingsError::OwnerSignatureInvalid
        ))
    ));
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_path(dir.path()).exists());
    // A Key app signature has no place here.
    let mut with_key = looser(2);
    with_key.approver_sig = Some(with_key.settings.sign(&id(2)));
    assert!(rig.node.take_settings(&with_key).is_err());
    assert!(rig.node.pending_change().is_none());
}

#[test]
fn cr_239_newer_settings_replace_older_and_older_ones_do_not() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    assert_eq!(rig.node.take_settings(&looser(2)), Ok(None));
    // Another loosening, at version 4.
    let newer = signed(4, |s| s.light_per_sale.sats += 99);
    assert_ne!(newer, looser(2));
    assert_eq!(rig.node.take_settings(&newer), Ok(None));
    assert_eq!(rig.node.pending_change().unwrap().version, 4);
    assert_eq!(
        std::fs::read(confirm_path(dir.path())).unwrap(),
        newer.to_bytes()
    );
    // An older one, and the same one again, change nothing.
    assert_eq!(rig.node.take_settings(&looser(3)), Ok(None));
    assert_eq!(rig.node.take_settings(&newer), Ok(None));
    assert_eq!(rig.node.pending_change().unwrap().version, 4);
    assert_eq!(
        std::fs::read(confirm_path(dir.path())).unwrap(),
        newer.to_bytes()
    );
    // The same one again does not start its seven days over.
    rig.time.store(T0 + DAY, Ordering::SeqCst);
    assert_eq!(rig.node.take_settings(&newer), Ok(None));
    assert_eq!(rig.node.pending_change().unwrap().arrived, T0);
}

#[test]
fn cr_239_a_change_is_dropped_seven_days_after_it_arrived() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    rig.node.take_settings(&looser(2)).unwrap();
    rig.time.store(T0 + PENDING_SECS - 1, Ordering::SeqCst);
    assert!(rig.node.pending_change().is_some());
    assert_eq!(rig.node.change_notice(), None);
    rig.time.store(T0 + PENDING_SECS, Ordering::SeqCst);
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_path(dir.path()).exists());
    let notice = rig.node.change_notice();
    assert_eq!(notice, Some(ChangeNotice::Dropped { version: 2 }));
    assert_eq!(rig.node.settings().version, 1);
    // A confirmation of it is no longer possible.
    assert_eq!(
        rig.node.confirm_pending(2),
        Err(ConfirmRefused::NothingWaits)
    );
    // Pumping drops one too.
    rig.time.store(T0 + 20 * DAY, Ordering::SeqCst);
    rig.node.take_settings(&looser(5)).unwrap();
    assert_eq!(rig.node.change_notice(), None);
    rig.time
        .store(T0 + 20 * DAY + PENDING_SECS, Ordering::SeqCst);
    rig.feed(vec![]);
    assert!(!confirm_path(dir.path()).exists());
    assert_eq!(
        rig.node.change_notice(),
        Some(ChangeNotice::Dropped { version: 5 })
    );
}

#[test]
fn cr_239_a_change_is_dropped_when_the_same_or_a_higher_version_is_in_force() {
    // A tightening of a higher version comes into force.
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    rig.node.take_settings(&looser(2)).unwrap();
    assert!(rig.node.pending_change().is_some());
    assert!(rig
        .node
        .take_settings(&signed(3, |s| s.light_per_sale.sats -= 1))
        .unwrap()
        .is_some());
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_path(dir.path()).exists());
    assert_eq!(rig.node.change_notice(), None);

    // The same version comes into force, with a confirmation.
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let change = looser(2);
    rig.node.take_settings(&change).unwrap();
    rig.node.confirm_pending(2).unwrap();
    let held = confirmation_record(&rig);
    let c = ConfirmedSettings::new(change, held);
    assert_eq!(
        rig.node.take_confirmed_settings(&c),
        Ok(Some(Change::Loosening))
    );
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_path(dir.path()).exists());

    // One that waits and is higher than the version that came into force
    // stays.
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    rig.node.take_settings(&looser(5)).unwrap();
    rig.node
        .take_settings(&signed(2, |s| s.light_per_sale.sats -= 1))
        .unwrap();
    assert_eq!(rig.node.pending_change().unwrap().version, 5);
}

/// The confirmation the counter wrote last, read from its own records as the
/// owner phone reads it.
fn confirmation_record(rig: &Rig) -> CounterConfirmation {
    let records = rig.node.records_from(0, usize::MAX).unwrap();
    let held = records
        .iter()
        .filter(|r| r.kind == RecordKind::Setting)
        .filter_map(|r| ConfirmHeld::decode(&r.body).ok())
        .next_back()
        .expect("a confirmation record");
    held.confirmation(id(3).device_id())
}

#[test]
fn cr_239_a_waiting_change_is_read_again_at_start_and_its_file_time_is_its_arrival() {
    let dir = Scratch::new();
    {
        let mut rig = own_rig(dir.path());
        rig.node.take_settings(&looser(2)).unwrap();
    }
    // The file was written two days before this start.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(confirm_path(dir.path()))
        .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(T0 - 2 * DAY))
        .unwrap();
    drop(file);
    let mut rig = own_rig(dir.path());
    let waiting = rig.node.pending_change().unwrap();
    assert_eq!(waiting.version, 2);
    assert_eq!(waiting.arrived, T0 - 2 * DAY);
    assert_eq!(waiting.expires_at, T0 + 5 * DAY);
    drop(rig);
    // Eight days before: it is dropped at once.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(confirm_path(dir.path()))
        .unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(T0 - 8 * DAY))
        .unwrap();
    drop(file);
    let mut rig = own_rig(dir.path());
    assert!(rig.node.pending_change().is_none());
    assert_eq!(
        rig.node.change_notice(),
        Some(ChangeNotice::Dropped { version: 2 })
    );
    assert!(!confirm_path(dir.path()).exists());
}

#[test]
fn cr_239_a_file_that_is_of_no_use_is_removed_with_a_note() {
    for bytes in [
        b"not settings".to_vec(),
        // Not above the version in force.
        looser(1).to_bytes(),
        // Not signed by the owner root.
        {
            let mut forged = looser(2);
            forged.owner_sig = looser(3).owner_sig;
            forged.to_bytes()
        },
    ] {
        let dir = Scratch::new();
        std::fs::write(confirm_path(dir.path()), bytes).unwrap();
        let mut rig = own_rig(dir.path());
        assert!(rig.node.pending_change().is_none());
        assert!(!confirm_path(dir.path()).exists());
        assert!(rig
            .node
            .take_notes()
            .iter()
            .any(|n| n.starts_with("CR-239")));
    }
}

#[test]
fn cr_239_a_counter_paired_with_the_key_app_behaves_as_before() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    let both = |s: Settings| SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s,
    };
    let mut s = settings();
    s.version = 2;
    s.light_per_sale.sats += 10;
    assert_eq!(
        rig.node.take_settings(&both(s)),
        Err(SettingsRefused::Verify(SettingsError::ApproverRequired))
    );
    assert!(rig.node.pending_change().is_none());
    assert!(!confirm_path(dir.path()).exists());
    // And a counter's confirmation is no way in.
    let change = signed(2, |s| s.light_per_sale.sats += 10);
    assert_eq!(
        rig.node.take_confirmed_settings(&confirmed_by(3, &change)),
        Err(SettingsRefused::NoConfirmationHere)
    );
    assert_eq!(rig.node.settings().version, 1);
}

#[test]
fn cr_240_the_fields_the_change_loosens_come_with_their_values_now_and_after() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let now = settings();
    let change = signed(2, |s| {
        s.light_per_sale.display_usd += 7;
        s.light_per_sale.sats += 7_000;
        s.node_budget.sats += 5;
        s.final_confirmations = 3;
        s.tor_on = false;
        s.heartbeat_max_age_secs += 60;
        s.underpayment_tolerance_bps += 100;
        // Not loosening: not listed.
        s.refund_cap.sats -= 1;
        s.tips_on = true;
        // Free by AP-01 though it names a new device: a Box.
        let b = id(60).device_id();
        s.devices.insert(b);
        s.boxes.insert(b);
        s.watch_key = Some(id(61).device_id());
    });
    rig.node.take_settings(&change).unwrap();
    let fields = rig.node.pending_change().unwrap().fields;
    let find = |name: &str| fields.iter().find(|f| f.field == name);
    let lp = find("light_per_sale").unwrap();
    // The satoshis differ, so they are shown (CR-240).
    assert_eq!(lp.now, FieldValue::Sats(now.light_per_sale.sats));
    assert_eq!(lp.after, FieldValue::Sats(now.light_per_sale.sats + 7_000));
    let nb = find("node_budget").unwrap();
    assert_eq!(nb.now, FieldValue::Sats(now.node_budget.sats));
    assert_eq!(nb.after, FieldValue::Sats(now.node_budget.sats + 5));
    let fc = find("final_confirmations").unwrap();
    assert_eq!(
        fc.now,
        FieldValue::Count(u32::from(now.final_confirmations))
    );
    assert_eq!(fc.after, FieldValue::Count(3));
    assert_eq!(
        find("heartbeat_max_age_secs").unwrap().after,
        FieldValue::Seconds(now.heartbeat_max_age_secs + 60)
    );
    assert_eq!(
        find("underpayment_tolerance_bps").unwrap().after,
        FieldValue::Bps(now.underpayment_tolerance_bps + 100)
    );
    // tor_on was off already in the test settings, so it is not a change.
    assert!(find("tor_on").is_none());
    for hidden in ["refund_cap", "tips_on", "boxes", "watch_key", "devices"] {
        assert!(find(hidden).is_none(), "{hidden}");
    }
    assert_eq!(fields.len(), 5, "{fields:?}");
}

#[test]
fn cr_241_the_counter_signs_the_hash_and_writes_a_record_of_its_own_chain() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let change = looser(2);
    rig.node.take_settings(&change).unwrap();
    let before = rig.node.records_len();
    assert_eq!(rig.node.confirm_pending(2), Ok(()));
    assert_eq!(rig.node.records_len(), before + 1);
    let conf = confirmation_record(&rig);
    // AP-04 and AP-05: it verifies for this hash and the counter in force.
    assert_eq!(
        conf.verify(&change.settings.hash(), &rig.node.settings().counters),
        Ok(())
    );
    assert_eq!(
        conf,
        CounterConfirmation::sign(&id(3), &change.settings.hash())
    );
    // The settings are not in force, and the change still waits, confirmed.
    assert_eq!(rig.node.settings().version, 1);
    assert!(rig.node.pending_change().unwrap().confirmed);
    assert!(!dir.path().join(SETTINGS_FILE).exists());
}

#[test]
fn cr_241_a_confirmation_of_another_version_or_none_signs_nothing() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    assert_eq!(
        rig.node.confirm_pending(2),
        Err(ConfirmRefused::NothingWaits)
    );
    rig.node.take_settings(&looser(3)).unwrap();
    let before = rig.node.records_len();
    for version in [2, 4] {
        let e = rig.node.confirm_pending(version).unwrap_err();
        assert_eq!(e, ConfirmRefused::NothingWaits);
        assert_eq!(e.rule(), "CR-241");
        assert!(e.to_string().starts_with("CR-241"));
    }
    assert_eq!(rig.node.records_len(), before);
    let w = ConfirmRefused::NothingWaits.words();
    assert_eq!(w.heading, "Nothing to confirm");
    assert_eq!(openbmt_core::say::check(&w), Ok(()));
}

#[test]
fn cr_241_a_record_that_cannot_be_written_signs_nothing() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    rig.node.take_settings(&looser(2)).unwrap();
    let before = rig.node.records_len();
    // The record file cannot be appended to: a directory is in its place.
    let path = dir.path().join("records.bin");
    std::fs::create_dir(&path).unwrap();
    let result = rig.node.confirm_pending(2);
    std::fs::remove_dir(&path).unwrap();
    let e = result.unwrap_err();
    assert!(matches!(e, ConfirmRefused::NotWritten(_)));
    assert_eq!(e.rule(), "CR-40");
    assert_eq!(rig.node.records_len(), before);
    assert!(!rig.node.pending_change().unwrap().confirmed);
}

#[test]
fn cr_242_a_confirmed_loosening_is_taken_and_the_file_keeps_the_frame_whole() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let change = looser(2);
    rig.node.take_settings(&change).unwrap();
    rig.node.confirm_pending(2).unwrap();
    let c = ConfirmedSettings::new(change.clone(), confirmation_record(&rig));
    assert_eq!(
        rig.node.take_confirmed_settings(&c),
        Ok(Some(Change::Loosening))
    );
    assert_eq!(rig.node.settings(), &change.settings);
    assert_eq!(rig.node.counter().settings(), &change.settings);
    // The file is the `OBM<` frame, whole.
    let kept = std::fs::read(dir.path().join(SETTINGS_FILE)).unwrap();
    assert_eq!(&kept[..4], b"OBM<");
    assert_eq!(kept, c.to_bytes());
    assert_eq!(decode_frame(&kept), Ok(SettingsFrame::Confirmed(c)));
    // The record of CR-162 is written, as a loosening.
    let notes: Vec<_> = rig
        .node
        .records_from(0, usize::MAX)
        .unwrap()
        .into_iter()
        .filter_map(|r| openbmt_core::settings::SettingNote::from_body(&r.body).ok())
        .collect();
    assert_eq!(
        notes.last().map(|n| (n.version, n.change)),
        Some((2, Change::Loosening))
    );
    // Again is a version that does not go past the one in force: dropped.
    let again = ConfirmedSettings::new(change, confirmation_record(&rig));
    assert_eq!(rig.node.take_confirmed_settings(&again), Ok(None));
}

#[test]
fn cr_242_another_counter_than_the_one_in_force_is_refused_and_nothing_changes() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let change = looser(2);
    // Counter 4 is not in the settings in force.
    let outsider = confirmed_by(4, &change);
    let e = rig.node.take_confirmed_settings(&outsider).unwrap_err();
    assert_eq!(e, SettingsRefused::Confirmation(ConfirmError::NotACounter));
    assert_eq!(e.rule(), "CR-242");
    assert_eq!(rig.node.settings().version, 1);
    assert!(!dir.path().join(SETTINGS_FILE).exists());
    assert_eq!(
        rig.node.change_notice(),
        Some(ChangeNotice::BadConfirmation { version: 2 })
    );
    assert_eq!(
        ChangeNotice::BadConfirmation { version: 2 }.words().heading,
        "Confirmation refused"
    );
}

#[test]
fn cr_242_a_confirmation_for_another_hash_or_with_a_bad_signature_is_refused() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let change = looser(2);
    // The confirmation of other settings.
    let other = signed(2, |s| s.light_per_sale.sats += 11);
    let wrong_hash = ConfirmedSettings::new(
        change.clone(),
        CounterConfirmation::sign(&id(3), &other.settings.hash()),
    );
    assert_eq!(
        rig.node.take_confirmed_settings(&wrong_hash),
        Err(SettingsRefused::Confirmation(ConfirmError::BadSignature))
    );
    // A changed signature.
    let mut bad = confirmed_by(3, &change);
    bad.confirmation.sig.0[9] ^= 1;
    assert_eq!(
        rig.node.take_confirmed_settings(&bad),
        Err(SettingsRefused::Confirmation(ConfirmError::BadSignature))
    );
    // The signature of another counter under this counter's id.
    let mut forged = confirmed_by(3, &change);
    forged.confirmation.sig = confirmed_by(4, &change).confirmation.sig;
    assert!(rig.node.take_confirmed_settings(&forged).is_err());
    assert_eq!(rig.node.settings().version, 1);
    assert!(!dir.path().join(SETTINGS_FILE).exists());
    // The checks of the `OBMS` frame still apply: the owner's signature.
    let mut unsigned = confirmed_by(3, &change);
    unsigned.signed.owner_sig = looser(7).owner_sig;
    assert!(matches!(
        rig.node.take_confirmed_settings(&unsigned),
        Err(SettingsRefused::Verify(
            SettingsError::OwnerSignatureInvalid
        ))
    ));
    // A Redirect is no way in either.
    let redirect = signed(2, |s| {
        s.lightning_node = Some([2u8; openbmt_core::settings::LIGHTNING_NODE_BYTES]);
    });
    assert!(rig
        .node
        .take_confirmed_settings(&confirmed_by(3, &redirect))
        .is_err());
    assert_eq!(rig.node.settings().version, 1);
}

#[test]
fn cr_163_a_settings_file_holding_the_confirmed_frame_is_read_at_start() {
    let dir = Scratch::new();
    let change = looser(2);
    {
        let mut rig = own_rig(dir.path());
        rig.node.take_settings(&change).unwrap();
        rig.node.confirm_pending(2).unwrap();
        let c = ConfirmedSettings::new(change.clone(), confirmation_record(&rig));
        rig.node.take_confirmed_settings(&c).unwrap();
    }
    let store = DirStore::open(dir.path()).unwrap();
    let record = PairedRecord::OwnWallet(Box::new(super::paired_own::record()));
    // The record's settings are version 5 in this fixture, so the file's
    // version 2 is not above it: the record's are used and the file is
    // noted. Under a record below the file's version, the file's are used.
    let got = effective_of(&store, &record).unwrap();
    assert!(!got.from_file && got.note.is_some());
    let mut low = super::paired_own::record();
    low.settings = signed(1, |_| {});
    let low = PairedRecord::OwnWallet(Box::new(low));
    let got = effective_of(&store, &low).unwrap();
    assert!(got.from_file);
    assert_eq!(got.settings(), &change.settings);
    assert!(got.note.is_none());
}

#[test]
fn cr_163_a_confirmed_frame_with_a_bad_signature_or_for_a_key_app_counter_is_a_fault() {
    let dir = Scratch::new();
    let change = looser(2);
    let mut bad = confirmed_by(3, &change);
    bad.confirmation.sig.0[1] ^= 1;
    let mut store = DirStore::open(dir.path()).unwrap();
    store.put_settings(&bad.to_bytes()).unwrap();
    let mut low = super::paired_own::record();
    low.settings = signed(1, |_| {});
    let low = PairedRecord::OwnWallet(Box::new(low));
    let e = effective_of(&store, &low).unwrap_err();
    assert!(matches!(e, SettingsFault::Verify(_)), "{e}");
    // A counter paired with the Key app never wrote one.
    store
        .put_settings(&confirmed_by(3, &change).to_bytes())
        .unwrap();
    let key_app = PairedRecord::KeyApp(Box::new(PairingRecord {
        identity_secret: [3; 32],
        roots: roots(),
        wallet: [9; 32],
        network: bitcoin::Network::Regtest,
        settings_floor: 0,
        settings: SignedSettings {
            owner_sig: settings().sign(&id(1)),
            approver_sig: Some(settings().sign(&id(2))),
            settings: settings(),
        },
        first_batch: None,
        peers: Vec::new(),
        relay: None,
    }));
    let e = crate::settings::effective_of(&store, &key_app).unwrap_err();
    assert!(matches!(e, SettingsFault::Verify(_)), "{e}");
}

#[test]
fn cr_240_dollars_down_and_satoshis_up_shows_the_satoshis() {
    let dir = Scratch::new();
    let mut rig = own_rig(dir.path());
    let now = settings();
    let change = signed(2, |s| {
        s.light_per_sale.display_usd = now.light_per_sale.display_usd.saturating_sub(1);
        s.light_per_sale.sats += 1_000_000;
    });
    rig.node.take_settings(&change).unwrap();
    let fields = rig.node.pending_change().unwrap().fields;
    let lp = fields.iter().find(|f| f.field == "light_per_sale").unwrap();
    assert_eq!(lp.now, FieldValue::Sats(now.light_per_sale.sats));
    assert_eq!(
        lp.after,
        FieldValue::Sats(now.light_per_sale.sats + 1_000_000)
    );
}

#[test]
fn cr_240_units_are_shown_only_when_the_satoshis_are_equal() {
    use crate::confirm::loosened_fields;
    let old = settings();
    let mut new = old.clone();
    new.version += 1;
    new.light_per_sale.display_usd += 3;
    let fields = loosened_fields(&old, &new);
    let lp = fields.iter().find(|f| f.field == "light_per_sale").unwrap();
    assert_eq!(lp.now, FieldValue::Units(old.light_per_sale.display_usd));
    assert_eq!(
        lp.after,
        FieldValue::Units(old.light_per_sale.display_usd + 3)
    );
}

#[test]
fn cr_240_a_changed_provider_lists_shop_lightning() {
    use crate::confirm::loosened_fields;
    let mut old = settings();
    let boxed = id(61).device_id();
    old.devices.insert(id(60).device_id());
    old.counters.insert(id(60).device_id(), 500);
    old.devices.insert(boxed);
    old.boxes.insert(boxed);
    old.shop_lightning = Some(openbmt_core::settings::ShopLightning {
        counter: id(60).device_id(),
        node_id: [7u8; 33],
        second_device: boxed,
        provider_node_id: [8u8; 33],
        provider_addr: "203.0.113.5:9735".parse().unwrap(),
        cap_sats: 5_000_000,
        max_opening_fee_sats: 20_000,
    });
    let mut new = old.clone();
    new.version += 1;
    let sl = new.shop_lightning.as_mut().unwrap();
    sl.provider_node_id = [9u8; 33];
    sl.provider_addr = "198.51.100.1:9735".parse().unwrap();
    sl.cap_sats += 1;
    let fields = loosened_fields(&old, &new);
    let f = fields.iter().find(|f| f.field == "shop_lightning").unwrap();
    let (FieldValue::Code(now), FieldValue::Code(after)) = (&f.now, &f.after) else {
        panic!("not text");
    };
    assert!(now.contains("08080808") && now.contains("203.0.113.5:9735"));
    assert!(now.contains("5000000") && now.contains("20000"));
    assert!(after.contains("09090909") && after.contains("198.51.100.1:9735"));
    assert!(after.contains("5000001"));
    assert!(now.contains(&format!("{:02x}", boxed.to_bytes()[0])));
}

#[test]
fn ap_12_the_last_counters_removal_is_listed_for_the_manager() {
    use crate::confirm::loosened_fields;
    let old = settings();
    assert!(!old.counters.is_empty());
    let mut new = old.clone();
    new.version += 1;
    for c in old.counters.keys() {
        new.devices.remove(c);
    }
    new.counters.clear();
    let fields = loosened_fields(&old, &new);
    let f = fields.iter().find(|f| f.field == "counters").unwrap();
    assert_eq!(f.now, FieldValue::Count(old.counters.len() as u32));
    assert_eq!(f.after, FieldValue::Count(0));
}
