//! Records that leave the counter's memory (CR-81), on a real directory.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

use openbmt_core::records::{Chain, DeviceList, Record, RecordKind, GENESIS};
use openbmt_counter::{Counter, LoadError, KEEP_RECORDS, MAX_UNSENT_IN_MEMORY};

use super::kit::*;
use crate::{DirStore, DirStoreError};

/// Write a chain of `n` check-ins signed by the test counter to the record
/// file, in the file's framing, without a flush for each: the way a counter
/// that ran for a long time would have left it. Returns the chain's head.
fn write_log(dir: &Path, n: u64) -> [u8; 32] {
    let mut out = BufWriter::new(File::create(dir.join("records.bin")).unwrap());
    let identity = id(3);
    let mut prev = GENESIS;
    for seq in 0..n {
        let mut body = seq.to_le_bytes().to_vec();
        body.extend_from_slice(&prev);
        let record = Record::build(&identity, seq, prev, RecordKind::CheckIn, body).unwrap();
        let bytes = record.to_bytes();
        out.write_all(&(bytes.len() as u32).to_le_bytes()).unwrap();
        out.write_all(&bytes).unwrap();
        prev = record.hash();
    }
    out.flush().unwrap();
    prev
}

fn load(dir: &Path) -> Result<(DirStore, Counter), LoadError> {
    let store = DirStore::open(dir).unwrap();
    let loaded = store.load().unwrap();
    let counter = Counter::load(id(3), pairing(), settings(), loaded.stored(), &store)?;
    Ok((store, counter))
}

#[test]
fn cr_81_a_restart_with_5000_records_streams_the_chain_and_holds_a_bounded_tail() {
    let dir = Scratch::new();
    let head = write_log(dir.path(), 5_000);
    let (store, mut counter) = load(dir.path()).unwrap();
    assert_eq!(counter.records_len(), 5_000);
    assert_eq!(counter.head(), head);
    // No mark yet: what is held is under the cap on records the owner may
    // not hold.
    assert!(counter.records_in_memory() as u64 <= MAX_UNSENT_IN_MEMORY);
    counter.set_owner_mark(5_000);
    assert_eq!(counter.records_in_memory() as u64, KEEP_RECORDS);
    assert_eq!(counter.records_base(), 5_000 - KEEP_RECORDS);
    // The whole chain reads back from the file and verifies.
    let mut all: Vec<Record> = Vec::new();
    loop {
        let page = counter.records_from(all.len() as u64, 500, &store).unwrap();
        if page.is_empty() {
            break;
        }
        all.extend(page);
    }
    assert_eq!(all.len(), 5_000);
    assert_eq!(all.last().map(Record::hash), Some(head));
    Chain::from_records(all)
        .verify(&DeviceList::new([id(3).device_id()]))
        .unwrap();
}

#[test]
fn cr_81_a_damaged_record_below_the_base_stops_the_start() {
    let dir = Scratch::new();
    write_log(dir.path(), 3_000);
    let path = dir.path().join("records.bin");
    let whole = fs::read(&path).unwrap();
    // A changed byte in a record's body: the signature no longer holds.
    let frame = 4 + u32::from_le_bytes(whole[..4].try_into().unwrap()) as usize;
    let mut changed = whole.clone();
    let at = 40 * frame + 4 + 100;
    changed[at] ^= 1;
    fs::write(&path, &changed).unwrap();
    let err = load(dir.path()).err().unwrap();
    assert!(matches!(err, LoadError::Records(_)), "{err:?}");
    assert_eq!(err.rule(), "CR-42");
    // A record that does not parse is found by the file's own load.
    let mut garbled = whole.clone();
    garbled[40 * frame + 4] ^= 0xff;
    fs::write(&path, &garbled).unwrap();
    let err = DirStore::open(dir.path()).unwrap().load().unwrap_err();
    assert!(matches!(err, DirStoreError::Corrupt { .. }), "{err:?}");
    // The file as it was starts.
    fs::write(&path, &whole).unwrap();
    assert!(load(dir.path()).is_ok());
}

/// How long a start takes with 100,000 records. Run it once, by hand:
/// `cargo test -p openbmt-counter-node --release cr_81_start_time -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run by hand"]
fn cr_81_start_time_with_100000_records() {
    let dir = Scratch::new();
    let made = Instant::now();
    let head = write_log(dir.path(), 100_000);
    eprintln!("made 100000 records in {:?}", made.elapsed());
    let started = Instant::now();
    let store = DirStore::open(dir.path()).unwrap();
    let loaded = store.load().unwrap();
    let scanned = started.elapsed();
    let counter = Counter::load(id(3), pairing(), settings(), loaded.stored(), &store).unwrap();
    eprintln!(
        "start with 100000 records: file scan {scanned:?}, whole start {:?}, \
         {} records held",
        started.elapsed(),
        counter.records_in_memory()
    );
    assert_eq!(counter.records_len(), 100_000);
    assert_eq!(counter.head(), head);
}
