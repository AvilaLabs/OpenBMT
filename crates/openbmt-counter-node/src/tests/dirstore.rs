//! `DirStore`: slots, records, failed writes and a counter that is saved and
//! loaded again.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::atomic::Ordering;

use openbmt_core::policy::Heartbeat;
use openbmt_core::records::{Chain, Record, RecordKind};
use openbmt_counter::{Counter, Facts, OpenFacts, Slot, State, Store};
use openbmt_watch::{HeaderChain, Source};

use super::kit::*;
use crate::dirstore::INDEX_STRIDE;
use crate::{DirStore, DirStoreError};

/// Records made and signed by the test counter, as the store is given them.
fn records(n: usize) -> Vec<Vec<u8>> {
    let mut chain = Chain::new();
    (0..n)
        .map(|i| {
            chain
                .append(&id(3), RecordKind::Sale, vec![i as u8; 10 + i])
                .unwrap()
                .to_bytes()
        })
        .collect()
}

fn append_raw(dir: &std::path::Path, bytes: &[u8]) {
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .open(dir.join("records.bin"))
        .unwrap();
    f.write_all(bytes).unwrap();
}

fn framed(record: &[u8]) -> Vec<u8> {
    let mut out = (record.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(record);
    out
}

#[test]
fn every_slot_and_the_checked_height_read_back() {
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    assert!(store.load().unwrap().batches.is_none());
    store.put(Slot::Batches, b"batches").unwrap();
    store.put(Slot::Exposure, b"exposure").unwrap();
    store.put(Slot::Pause, b"pause").unwrap();
    store.put(Slot::Desk, b"desk").unwrap();
    store.put(Slot::Bills, b"bills").unwrap();
    for index in [12, 0, 7] {
        store
            .put(Slot::Sale(index), format!("sale {index}").as_bytes())
            .unwrap();
    }
    store.put_checked(321, &[]).unwrap();
    let loaded = DirStore::open(dir.path()).unwrap().load().unwrap();
    assert_eq!(loaded.batches.as_deref(), Some(&b"batches"[..]));
    assert_eq!(loaded.exposure.as_deref(), Some(&b"exposure"[..]));
    assert_eq!(loaded.pause.as_deref(), Some(&b"pause"[..]));
    assert_eq!(loaded.desk.as_deref(), Some(&b"desk"[..]));
    assert_eq!(loaded.bills.as_deref(), Some(&b"bills"[..]));
    assert_eq!(
        loaded.sales,
        vec![
            (0, b"sale 0".to_vec()),
            (7, b"sale 7".to_vec()),
            (12, b"sale 12".to_vec())
        ]
    );
    assert_eq!(loaded.checked, Some(321));
    assert!(loaded.record_count == 0 && loaded.torn.is_none());
    // A slot is replaced whole.
    store.put(Slot::Desk, b"a longer desk than before").unwrap();
    store.put(Slot::Desk, b"d").unwrap();
    assert_eq!(store.load().unwrap().desk.as_deref(), Some(&b"d"[..]));
}

#[test]
fn files_that_are_not_slots_are_left_alone() {
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    store.put(Slot::Sale(3), b"three").unwrap();
    for name in [
        "sale-4.bin.tmp",
        "sale-05.bin",
        "sale-x.bin",
        "sale-.bin",
        "headers.bin",
        "book.bin",
    ] {
        fs::write(dir.path().join(name), b"other").unwrap();
    }
    let loaded = store.load().unwrap();
    assert_eq!(loaded.sales, vec![(3, b"three".to_vec())]);
}

#[test]
fn cr_77_a_slot_is_deleted_whole_and_deleting_what_is_not_there_is_no_error() {
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    store.put(Slot::Sale(1), b"one").unwrap();
    store.put(Slot::Sale(2), b"two").unwrap();
    // A temporary file that a crash left behind goes with it.
    fs::write(dir.path().join("sale-1.bin.tmp"), b"half").unwrap();
    store.delete(Slot::Sale(1)).unwrap();
    store.delete(Slot::Sale(1)).unwrap();
    store.delete(Slot::Sale(77)).unwrap();
    assert!(!dir.path().join("sale-1.bin").exists());
    assert!(!dir.path().join("sale-1.bin.tmp").exists());
    let loaded = store.load().unwrap();
    assert_eq!(loaded.sales, vec![(2, b"two".to_vec())]);
    // The other slots are not touched.
    store.put(Slot::Desk, b"desk").unwrap();
    store.delete(Slot::Sale(2)).unwrap();
    let loaded = store.load().unwrap();
    assert!(loaded.sales.is_empty());
    assert_eq!(loaded.desk.as_deref(), Some(&b"desk"[..]));
}

#[test]
fn a_failed_write_leaves_the_old_value() {
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    store.put(Slot::Desk, b"old").unwrap();
    store.put(Slot::Sale(1), b"old sale").unwrap();
    store.put_checked(5, &[]).unwrap();
    // A directory where the temporary file belongs: the file cannot be made.
    for name in ["desk.bin.tmp", "sale-1.bin.tmp", "checked.bin.tmp"] {
        fs::create_dir(dir.path().join(name)).unwrap();
    }
    let err = store.put(Slot::Desk, b"new").unwrap_err();
    assert!(
        matches!(err, DirStoreError::Io { op: "write", .. }),
        "{err}"
    );
    assert!(store.put(Slot::Sale(1), b"new sale").is_err());
    assert!(store.put_checked(9, &[]).is_err());
    let loaded = store.load().unwrap();
    assert_eq!(loaded.desk.as_deref(), Some(&b"old"[..]));
    assert_eq!(loaded.sales, vec![(1, b"old sale".to_vec())]);
    assert_eq!(loaded.checked, Some(5));
    // The error names the file in plain words.
    assert_eq!(
        err.to_string(),
        format!(
            "could not write desk.bin: {}",
            std::io::ErrorKind::IsADirectory
        )
    );
}

#[test]
fn a_checked_file_of_the_wrong_size_is_an_error() {
    let dir = Scratch::new();
    let store = DirStore::open(dir.path()).unwrap();
    fs::write(dir.path().join("checked.bin"), [1, 2, 3]).unwrap();
    assert!(matches!(
        store.load().unwrap_err(),
        DirStoreError::Corrupt {
            file,
            ..
        } if file == "checked.bin"
    ));
}

#[test]
fn records_are_appended_in_order_and_read_back() {
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    let made = records(5);
    for r in &made[..3] {
        store.append_record(r).unwrap();
    }
    // A new store on the same directory goes on after the third.
    let mut again = DirStore::open(dir.path()).unwrap();
    for r in &made[3..] {
        again.append_record(r).unwrap();
    }
    let loaded = again.load().unwrap();
    assert!(loaded.torn.is_none());
    assert_eq!(loaded.record_count, 5);
    let back = again.read_records(0, 100).unwrap();
    assert_eq!(back, made);
    Chain::from_records(
        back.iter()
            .map(|b| Record::from_bytes(b).unwrap())
            .collect(),
    )
    .verify(&openbmt_core::records::DeviceList::new([id(3).device_id()]))
    .unwrap();
}

#[test]
fn cr_81_records_read_back_by_number_across_the_marks_of_the_index() {
    let n = 3 * INDEX_STRIDE as usize + 10;
    let made = records(n);
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    for r in &made {
        store.append_record(r).unwrap();
    }
    // The index kept as records were appended, and the one a load makes.
    let fresh = DirStore::open(dir.path()).unwrap();
    assert_eq!(fresh.load().unwrap().record_count, n as u64);
    for s in [&store, &fresh] {
        for (from, max) in [
            (0usize, 1usize),
            (0, 5),
            (63, 3),
            (64, 1),
            (65, 70),
            (127, 128),
            (n - 1, 5),
            (n - 3, 10),
            (0, n + 50),
        ] {
            let got = s.read_records(from as u64, max).unwrap();
            let end = (from + max).min(n);
            assert_eq!(got, made[from..end].to_vec(), "from {from} max {max}");
        }
        assert!(s.read_records(n as u64, 5).unwrap().is_empty());
        assert!(s.read_records(u64::MAX, 5).unwrap().is_empty());
        assert!(s.read_records(0, 0).unwrap().is_empty());
    }
    // A store that never loaded finds the records by a scan of its own.
    let cold = DirStore::open(dir.path()).unwrap();
    assert_eq!(cold.read_records(70, 2).unwrap(), made[70..72].to_vec());
}

#[test]
fn cr_81_a_record_file_cut_after_the_index_was_made_is_an_error_on_read() {
    let made = records(10);
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    for r in &made {
        store.append_record(r).unwrap();
    }
    let path = dir.path().join("records.bin");
    let size = fs::metadata(&path).unwrap().len();
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(size - 20)
        .unwrap();
    assert!(matches!(
        store.read_records(5, 10).unwrap_err(),
        DirStoreError::Corrupt { .. }
    ));
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        store.read_records(0, 1).unwrap_err(),
        DirStoreError::Io { .. }
    ));
}

#[test]
fn a_torn_last_record_is_cut_off_and_reported() {
    let made = records(4);
    // Cut in the data, and cut inside the length.
    for cut in [framed(&made[3]).len() - 7, 2] {
        let dir = Scratch::new();
        let mut store = DirStore::open(dir.path()).unwrap();
        for r in &made[..3] {
            store.append_record(r).unwrap();
        }
        let whole = fs::metadata(dir.path().join("records.bin")).unwrap().len();
        append_raw(dir.path(), &framed(&made[3])[..cut]);
        let loaded = store.load().unwrap();
        assert_eq!(loaded.record_count, 3);
        assert_eq!(
            loaded.torn.map(|t| (t.kept_records, t.cut_bytes)),
            Some((3, cut as u64))
        );
        // The file is whole again, and the next record follows the third.
        assert_eq!(
            fs::metadata(dir.path().join("records.bin")).unwrap().len(),
            whole
        );
        store.append_record(&made[3]).unwrap();
        let loaded = store.load().unwrap();
        assert!(loaded.torn.is_none());
        assert_eq!(loaded.record_count, 4);
    }
}

#[test]
fn garbage_in_the_middle_of_the_records_is_an_error_and_nothing_is_cut() {
    let made = records(3);
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    store.append_record(&made[0]).unwrap();
    // A whole frame whose content is not a record.
    append_raw(dir.path(), &framed(&[7u8; 40]));
    store.append_record(&made[2]).unwrap();
    let size = fs::metadata(dir.path().join("records.bin")).unwrap().len();
    assert!(matches!(
        store.load().unwrap_err(),
        DirStoreError::Corrupt { .. }
    ));
    assert_eq!(
        fs::metadata(dir.path().join("records.bin")).unwrap().len(),
        size
    );

    // A length no record has is an error too, not a torn tail.
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    store.append_record(&made[0]).unwrap();
    append_raw(dir.path(), &u32::MAX.to_le_bytes());
    append_raw(dir.path(), &[0; 30]);
    assert!(matches!(
        store.load().unwrap_err(),
        DirStoreError::Corrupt { .. }
    ));

    // A record that is too long to append is refused before anything is
    // written.
    let err = store
        .append_record(&vec![0; crate::dirstore::MAX_RECORD_BYTES + 1])
        .unwrap_err();
    assert!(matches!(err, DirStoreError::RecordTooLarge(_)));
}

fn open_facts(now: u64) -> OpenFacts {
    OpenFacts {
        health: healthy(),
        rescan_pending: false,
        now,
    }
}

#[test]
fn a_counter_saved_through_the_store_loads_again_as_it_was() {
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    let mut c = Counter::new(id(3), pairing(), settings());
    c.add_batch(batch(0, 10), &mut store).unwrap();
    // A sale that is accepted at once and then has a conflict: the exposure
    // goes in and the alarm goes up.
    let opened = c
        .open(
            five_dollars(),
            usd(),
            rate(),
            &fresh(T0),
            open_facts(T0),
            &mut store,
        )
        .unwrap();
    let pay = spend(1, &[(&opened.script, opened.needs_sats)]);
    for n in [20, 21, 22] {
        c.sighting(&bytes(&pay), Source::Peer(peer(n)), T0, &mut store)
            .unwrap();
    }
    let chain = Chain0;
    let facts = |now| Facts {
        chain: &chain,
        health: healthy(),
        heartbeat: Heartbeat {
            verified: true,
            age_secs: 0,
        },
        remote_order: false,
        now,
    };
    let screen = c.judge(0, &facts(T0 + 60), &mut store).unwrap();
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(c.exposure_outstanding(), 5_000);
    let other = spend(1, &[(&p2wsh(0x77), 5_000)]);
    c.sighting(&bytes(&other), Source::Peer(peer(30)), T0 + 61, &mut store)
        .unwrap();
    let screen = c.judge(0, &facts(T0 + 62), &mut store).unwrap();
    assert_eq!(screen.state, State::DoNotHandOver);
    assert!(c.alarm());
    // A second sale that is still open, and the next address.
    let second = c
        .open(
            five_dollars(),
            usd(),
            rate(),
            &fresh(T0 + 70),
            open_facts(T0 + 70),
            &mut store,
        )
        .unwrap();
    assert_eq!(second.index, 1);

    let loaded = DirStore::open(dir.path()).unwrap().load().unwrap();
    let back = Counter::load(id(3), pairing(), settings(), loaded.stored(), &store).unwrap();
    assert_eq!(back.alarm(), c.alarm());
    assert_eq!(back.exposure_outstanding(), c.exposure_outstanding());
    assert_eq!(back.pause_status(T0 + 100), c.pause_status(T0 + 100));
    assert_eq!(back.records_len(), c.records_len());
    assert_eq!(back.batches().remaining(), c.batches().remaining());
    assert_eq!(back.watched_scripts(), c.watched_scripts());
    for index in [0, 1] {
        assert_eq!(
            back.sale(index).unwrap().to_bytes(),
            c.sale(index).unwrap().to_bytes()
        );
    }
    assert!(back.unreadable().next().is_none());
}

/// A header chain with no block on it.
struct Chain0;

impl HeaderChain for Chain0 {
    fn height_of(&self, _: &bitcoin::BlockHash) -> Option<u32> {
        None
    }
    fn tip_height(&self) -> u32 {
        0
    }
    fn tip_time(&self) -> u64 {
        T0
    }
}

/// A counter on a directory, as a start makes it.
fn open_counter(dir: &std::path::Path) -> (DirStore, Counter) {
    let store = DirStore::open(dir).unwrap();
    let loaded = store.load().unwrap();
    let counter = Counter::load(id(3), pairing(), settings(), loaded.stored(), &store).unwrap();
    (store, counter)
}

fn setting_note(version: u32) -> openbmt_core::settings::SettingNote {
    openbmt_core::settings::SettingNote {
        version,
        change: openbmt_core::settings::Change::Loosening,
        hash: [version as u8; 32],
    }
}

/// The sequence numbers of the records on disk, in file order.
fn seqs_on_disk(dir: &std::path::Path) -> Vec<u64> {
    let store = DirStore::open(dir).unwrap();
    let count = store.load().unwrap().record_count;
    store
        .read_records(0, count as usize)
        .unwrap()
        .iter()
        .map(|b| Record::from_bytes(b).unwrap().seq)
        .collect()
}

#[test]
fn cr_81_a_failed_directory_flush_of_the_first_record_leaves_no_record_and_the_retry_loads() {
    let dir = Scratch::new();
    let (mut store, mut counter) = open_counter(dir.path());
    store.faults.sync_dir.store(1, Ordering::SeqCst);
    let first = setting_note(1);
    assert!(counter.record_setting(first, &mut store).is_err());
    assert_eq!(counter.records_len(), 0);
    assert!(!dir.path().join("records.bin").exists());
    // The automatic retry.
    counter.record_setting(first, &mut store).unwrap();
    assert_eq!(counter.records_len(), 1);
    drop((store, counter));
    let (_, counter) = open_counter(dir.path());
    assert_eq!(counter.records_len(), 1);
    assert_eq!(seqs_on_disk(dir.path()), vec![0]);
}

#[test]
fn cr_81_a_failed_data_flush_of_a_later_record_cuts_the_file_and_the_retry_appends() {
    let dir = Scratch::new();
    let (mut store, mut counter) = open_counter(dir.path());
    counter.record_setting(setting_note(1), &mut store).unwrap();
    counter.record_setting(setting_note(2), &mut store).unwrap();
    let size = fs::metadata(dir.path().join("records.bin")).unwrap().len();
    store.faults.sync_data.store(1, Ordering::SeqCst);
    assert!(counter.record_setting(setting_note(3), &mut store).is_err());
    assert_eq!(counter.records_len(), 2);
    assert_eq!(
        fs::metadata(dir.path().join("records.bin")).unwrap().len(),
        size
    );
    {
        let (_, restarted) = open_counter(dir.path());
        assert_eq!(restarted.records_len(), 2);
    }
    counter.record_setting(setting_note(3), &mut store).unwrap();
    drop((store, counter));
    let (_, counter) = open_counter(dir.path());
    assert_eq!(counter.records_len(), 3);
    assert_eq!(seqs_on_disk(dir.path()), vec![0, 1, 2]);
}

#[test]
fn cr_81_a_take_back_that_fails_refuses_every_later_append_until_the_store_is_reopened() {
    let dir = Scratch::new();
    let (mut store, mut counter) = open_counter(dir.path());
    counter.record_setting(setting_note(1), &mut store).unwrap();
    // The record is written whole, then the flush and the take back fail.
    store.faults.sync_data.store(1, Ordering::SeqCst);
    store.faults.take_back.store(1, Ordering::SeqCst);
    assert!(counter.record_setting(setting_note(2), &mut store).is_err());
    assert_eq!(counter.records_len(), 1);
    // Nothing is appended while the store is poisoned, retry or not.
    for _ in 0..3 {
        let err = counter
            .record_setting(setting_note(2), &mut store)
            .unwrap_err();
        assert!(err.to_string().contains("append to records.bin"), "{err}");
    }
    assert_eq!(counter.records_len(), 1);
    // Opened again, the store reads the file as it is: the whole record the
    // failed append left is a record, and the chain it makes verifies. The
    // counter that was running is dropped, as a restart does.
    drop((store, counter));
    let (mut store, mut counter) = open_counter(dir.path());
    assert_eq!(counter.records_len(), 2);
    counter.record_setting(setting_note(3), &mut store).unwrap();
    drop((store, counter));
    let (_, counter) = open_counter(dir.path());
    assert_eq!(counter.records_len(), 3);
    assert_eq!(seqs_on_disk(dir.path()), vec![0, 1, 2]);
}
