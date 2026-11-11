//! CR-160 to CR-166: the settings the owner sends after pairing. The node on
//! a fake driver takes them, keeps them in its folder, records them and
//! starts from them; the link carries them in.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use bitcoin::Network;
use openbmt_core::records::{Record, RecordKind};
use openbmt_core::settings::{Change, SettingNote, Settings, SettingsError, SignedSettings};
use openbmt_core::wire::DecodeError;
use openbmt_counter::OpenRefused;

use super::kit::*;
use crate::dirstore::SETTINGS_FILE;
use crate::node::StartError;
use crate::paired::PairingRecord;
use crate::settings::{
    check_not_rolled, decode_file, effective, SettingsFault, SettingsLine, SettingsRefused,
    MAX_SETTINGS_FILE_BYTES,
};
use crate::DirStore;

/// The test counter's settings at `version`, changed by `f`.
fn next(version: u32, f: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = settings();
    s.version = version;
    f(&mut s);
    s
}

fn by_owner(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s,
    }
}

fn by_both(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    }
}

/// The record the counter was paired with, holding settings of version 1.
fn record() -> PairingRecord {
    PairingRecord {
        identity_secret: [3; 32],
        roots: roots(),
        wallet: [9; 32],
        network: Network::Regtest,
        settings_floor: 0,
        settings: by_both(settings()),
        first_batch: None,
        peers: Vec::new(),
        relay: None,
    }
}

fn setting_records(rig: &Rig) -> Vec<(Record, SettingNote)> {
    rig.node
        .records_from(0, usize::MAX)
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == RecordKind::Setting)
        .map(|r| {
            let note = SettingNote::from_body(&r.body).unwrap();
            (r, note)
        })
        .collect()
}

#[test]
fn cr_160_newer_settings_signed_as_they_must_be_are_taken_and_used() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    // A tightening needs the owner alone.
    let tighter = next(2, |s| s.light_per_sale.sats -= 1);
    assert_eq!(
        rig.node.take_settings(&by_owner(tighter.clone())),
        Ok(Some(Change::Tightening))
    );
    assert_eq!(rig.node.settings(), &tighter);
    assert_eq!(rig.node.counter().settings(), &tighter);
    // A loosening needs the Key app too.
    let looser = next(3, |s| s.light_per_sale.sats += 10);
    assert_eq!(
        rig.node.take_settings(&by_owner(looser.clone())),
        Err(SettingsRefused::Verify(SettingsError::ApproverRequired))
    );
    assert_eq!(rig.node.settings().version, 2);
    assert_eq!(
        rig.node.take_settings(&by_both(looser.clone())),
        Ok(Some(Change::Loosening))
    );
    assert_eq!(rig.node.counter().settings(), &looser);
}

#[test]
fn cr_160_an_older_or_equal_version_is_dropped_without_a_refusal() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    let before = rig.node.records_len();
    for version in [0, 1] {
        let s = by_both(next(version, |s| s.light_per_sale.sats += 1));
        assert_eq!(rig.node.take_settings(&s), Ok(None));
    }
    assert_eq!(rig.node.settings().version, 1);
    assert_eq!(rig.node.records_len(), before);
    assert!(DirStore::open(dir.path())
        .unwrap()
        .settings_file()
        .unwrap()
        .is_none());
}

#[test]
fn cr_160_settings_that_do_not_check_change_nothing() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    let mut wrong = by_both(next(2, |s| s.light_per_sale.sats -= 1));
    wrong.owner_sig = wrong.settings.sign(&id(9));
    let err = rig.node.take_settings(&wrong).unwrap_err();
    assert_eq!(
        err,
        SettingsRefused::Verify(SettingsError::OwnerSignatureInvalid)
    );
    assert_eq!(err.rule(), "CR-160");
    assert!(err.to_string().starts_with("CR-160: "));
    // Signed by the wrong Key app.
    let mut other = by_both(next(2, |s| s.light_per_sale.sats -= 1));
    other.approver_sig = Some(other.settings.sign(&id(9)));
    assert_eq!(
        rig.node.take_settings(&other),
        Err(SettingsRefused::Verify(
            SettingsError::ApproverSignatureInvalid
        ))
    );
    // Values that are not valid.
    let bad = by_both(next(2, |s| s.underpayment_tolerance_bps = 9_000));
    assert!(matches!(
        rig.node.take_settings(&bad),
        Err(SettingsRefused::Verify(SettingsError::Invalid(_)))
    ));
    assert_eq!(rig.node.settings().version, 1);
    assert_eq!(setting_records(&rig).len(), 0);
}

#[test]
fn cr_160_the_frame_is_read_strictly() {
    let good = by_both(next(2, |s| s.light_per_sale.sats -= 1)).to_bytes();
    assert!(decode_file(&good).is_ok());
    assert!(decode_file(&good[..good.len() - 1]).is_err());
    let mut long = good.clone();
    long.push(0);
    assert!(decode_file(&long).is_err());
    assert_eq!(
        decode_file(&vec![0u8; MAX_SETTINGS_FILE_BYTES + 1]),
        Err(DecodeError::TooLarge)
    );
    assert!(decode_file(b"").is_err());
}

#[test]
fn cr_161_the_settings_are_in_the_file_before_they_are_used() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    let signed = by_owner(next(2, |s| s.light_per_sale.sats -= 1));
    rig.node.take_settings(&signed).unwrap();
    let bytes = std::fs::read(dir.path().join(SETTINGS_FILE)).unwrap();
    assert_eq!(bytes, signed.to_bytes());
    assert_eq!(decode_file(&bytes), Ok(signed));
}

#[test]
fn cr_161_settings_that_cannot_be_written_are_not_used() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    // The temporary file cannot be made: a folder is in its place.
    std::fs::create_dir(dir.path().join(format!("{SETTINGS_FILE}.tmp"))).unwrap();
    let signed = by_owner(next(2, |s| s.light_per_sale.sats -= 1));
    let err = rig.node.take_settings(&signed).unwrap_err();
    assert!(matches!(err, SettingsRefused::Store(_)), "{err}");
    assert_eq!(err.rule(), "CR-161");
    assert_eq!(rig.node.settings().version, 1);
    assert_eq!(rig.node.counter().settings().version, 1);
    assert_eq!(setting_records(&rig).len(), 0);
    assert!(!dir.path().join(SETTINGS_FILE).exists());
}

#[test]
fn cr_162_taken_settings_are_recorded_with_the_version_and_the_change() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    let tighter = by_owner(next(2, |s| s.light_per_sale.sats -= 1));
    rig.node.take_settings(&tighter).unwrap();
    let looser = by_both(next(3, |s| s.light_per_sale.sats += 10));
    rig.node.take_settings(&looser).unwrap();
    let notes: Vec<SettingNote> = setting_records(&rig).into_iter().map(|(_, n)| n).collect();
    assert_eq!(
        notes,
        vec![
            SettingNote {
                version: 2,
                change: Change::Tightening,
                hash: tighter.settings.hash()
            },
            SettingNote {
                version: 3,
                change: Change::Loosening,
                hash: looser.settings.hash()
            },
        ]
    );
    assert_eq!(rig.node.counter().settings_recorded(), Some(3));
}

#[test]
fn cr_162_a_counter_with_a_link_records_the_settings_of_its_pairing_once() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut config = config(dir.path(), &time);
    config.announce_settings = true;
    let rig = start_with(config, time.clone());
    let first = setting_records(&rig);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].1.version, 1);
    assert_eq!(first[0].1.change, Change::Loosening);
    assert_eq!(first[0].1.hash, settings().hash());
    drop(rig);
    // The next start finds the record and writes no other.
    let mut config = super::kit::config(dir.path(), &time);
    config.announce_settings = true;
    let rig = start_with(config, time);
    assert_eq!(setting_records(&rig).len(), 1);
    // Without a link nothing is recorded.
    let other = Scratch::new();
    assert_eq!(setting_records(&start(other.path())).len(), 0);
}

#[test]
fn cr_162_a_record_that_failed_is_written_at_the_next_call() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    // Make the record file unwritable: a folder where the file must be.
    let records = dir.path().join("records.bin");
    let held = std::fs::read(&records).ok();
    std::fs::remove_file(&records).ok();
    std::fs::create_dir(&records).unwrap();
    let signed = by_owner(next(2, |s| s.light_per_sale.sats -= 1));
    // The settings are taken (they are in force and in the file); only the
    // record is waiting.
    assert_eq!(
        rig.node.take_settings(&signed),
        Ok(Some(Change::Tightening))
    );
    assert_eq!(rig.node.counter().settings_recorded(), None);
    assert!(rig
        .node
        .notes()
        .any(|n| n.contains("CR-162") && n.contains("not written")));
    std::fs::remove_dir(&records).unwrap();
    if let Some(bytes) = held {
        std::fs::write(&records, bytes).unwrap();
    }
    rig.node.retry_setting_record();
    assert_eq!(rig.node.counter().settings_recorded(), Some(2));
}

#[test]
fn cr_163_a_start_uses_the_newer_of_the_record_and_the_file() {
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    // No file: the record's.
    let got = effective(&store, &record()).unwrap();
    assert!(!got.from_file);
    assert_eq!(got.settings().version, 1);
    // A newer file.
    let newer = by_owner(next(4, |s| s.light_per_sale.sats -= 1));
    store.put_settings(&newer.to_bytes()).unwrap();
    let got = effective(&store, &record()).unwrap();
    assert!(got.from_file);
    assert_eq!(got.signed, newer);
    // A file with the record's version is not newer: the record's, noted.
    store.put_settings(&by_both(settings()).to_bytes()).unwrap();
    let got = effective(&store, &record()).unwrap();
    assert!(!got.from_file);
    assert!(got.note.unwrap().contains("CR-163"));
}

#[test]
fn cr_163_a_file_that_does_not_read_or_check_is_a_fault() {
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    store.put_settings(b"not settings").unwrap();
    let err = effective(&store, &record()).unwrap_err();
    assert!(matches!(err, SettingsFault::Decode(_)), "{err}");
    assert_eq!(err.rule(), "CR-163");
    assert!(err.to_string().starts_with("CR-163: "));
    let mut forged = by_both(next(4, |s| s.light_per_sale.sats -= 1));
    forged.owner_sig = forged.settings.sign(&id(9));
    store.put_settings(&forged.to_bytes()).unwrap();
    assert_eq!(
        effective(&store, &record()),
        Err(SettingsFault::Verify(SettingsError::OwnerSignatureInvalid))
    );
    // A file that cannot be read at all.
    std::fs::remove_file(dir.path().join(SETTINGS_FILE)).unwrap();
    std::fs::create_dir(dir.path().join(SETTINGS_FILE)).unwrap();
    assert!(matches!(
        effective(&store, &record()),
        Err(SettingsFault::Read(_))
    ));
}

#[test]
fn cr_163_the_start_refuses_settings_older_than_the_records_name() {
    assert_eq!(check_not_rolled(3, Some(3)), Ok(()));
    assert_eq!(check_not_rolled(3, None), Ok(()));
    assert_eq!(
        check_not_rolled(3, Some(5)),
        Err(SettingsFault::Rolled {
            in_force: 3,
            recorded: 5
        })
    );
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_at(dir.path(), time.clone());
    rig.node
        .take_settings(&by_owner(next(2, |s| s.light_per_sale.sats -= 1)))
        .unwrap();
    drop(rig);
    // Started again from the pairing's settings, as if the file were gone.
    let err = match try_start_with(config(dir.path(), &time), time.clone()) {
        Err(e) => e,
        Ok(_) => panic!("the start was not refused"),
    };
    assert_eq!(
        err,
        StartError::Settings(SettingsFault::Rolled {
            in_force: 1,
            recorded: 2
        })
    );
    // Started from the settings the file holds, it goes on.
    let mut config = config(dir.path(), &time);
    config.settings = next(2, |s| s.light_per_sale.sats -= 1);
    assert!(try_start_with(config, time).is_ok());
}

#[test]
fn cr_164_settings_that_change_tor_need_a_restart() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    assert!(!rig.node.restart_needed());
    // Same Tor switch: nothing to restart for.
    rig.node
        .take_settings(&by_owner(next(2, |s| s.light_per_sale.sats -= 1)))
        .unwrap();
    assert!(!rig.node.restart_needed());
    // Tor on is a tightening.
    rig.node
        .take_settings(&by_owner(next(3, |s| {
            s.light_per_sale.sats -= 1;
            s.tor_on = true;
        })))
        .unwrap();
    assert!(rig.node.restart_needed());
    assert_eq!(
        SettingsLine::of(rig.node.removed(), rig.node.restart_needed()),
        Some(SettingsLine::RestartNeeded)
    );
    assert!(rig.node.notes().any(|n| n.contains("CR-164")));
    // A start with those settings has nothing left to do.
    let time = Arc::new(AtomicU64::new(T0));
    drop(rig);
    let mut config = config(dir.path(), &time);
    config.settings = next(3, |s| {
        s.light_per_sale.sats -= 1;
        s.tor_on = true;
    });
    let rig = start_with(config, time);
    assert!(!rig.node.restart_needed());
}

#[test]
fn cr_165_a_counter_the_settings_remove_opens_no_sale_but_settles_the_open_ones() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    let open = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    let removed = by_owner(next(2, |s| {
        s.counters.clear();
        s.devices.remove(&id(3).device_id());
    }));
    assert_eq!(
        rig.node.take_settings(&removed),
        Ok(Some(Change::Tightening))
    );
    assert!(rig.node.removed());
    assert_eq!(
        SettingsLine::of(true, true),
        Some(SettingsLine::Removed),
        "removal is said before a restart"
    );
    let err = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap_err();
    assert_eq!(err, OpenRefused::Removed);
    // The sale that was open is still judged and the records still go on.
    assert!(rig.node.judge(open.index).is_ok());
    assert!(rig.node.check_in().is_ok());
    // And it stays so after a start from the file.
    drop(rig);
    let time = Arc::new(AtomicU64::new(T0));
    let store = DirStore::open(dir.path()).unwrap();
    let got = effective(&store, &record()).unwrap();
    assert!(got.from_file);
    let mut config = config(dir.path(), &time);
    config.settings = got.settings().clone();
    let rig = start_with(config, time);
    assert!(rig.node.removed());
}

#[test]
fn cr_166_nothing_builds_a_second_path_for_the_device_list() {
    // The device list is a field of the settings: removing a device is a
    // change of settings that the counter takes as any other.
    let before = settings();
    let after = next(2, |s| {
        s.devices.insert(id(40).device_id());
    });
    assert!(before.devices.len() < after.devices.len());
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    assert_eq!(
        rig.node.take_settings(&by_both(after.clone())),
        Ok(Some(Change::Loosening))
    );
    assert_eq!(rig.node.settings().devices, after.devices);
}
