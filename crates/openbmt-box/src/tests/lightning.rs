//! BX-90 to BX-95: the Box's store of Shop Lightning's state.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use openbmt_core::hkdf::sha256_of;
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::{Settings, ShopLightning};
use openbmt_link::numbers::STORE_INBOX_FILE;
use openbmt_link::shopln::{
    Action, Part, StoreBody, StoreCause, StoreMessage, StoredBody, StoredMessage, MAX_PART_LEN,
    MAX_STORED_NAME_LEN,
};
use openbmt_link::{open, seal, Inbox, Kind, Role, StoreAnswer};

use super::kit::*;
use super::runner::{connect, eventually, sealed_to, start};
use crate::files::{BoxFiles, MemFiles};
use crate::lightning::{
    ShopStore, GRACE_SECS, LIGHTNING_DIR, MAX_NAMES, MAX_STORE_BYTES, MAX_WRITES,
    PART_TIMEOUT_SECS, TENANTS_FILE,
};
use crate::status_page::{render_home, store_full_words, PageView};

const T0: u64 = 5_000_000;

#[derive(Default)]
struct Rng(u8);

impl Rng {
    fn fill(&mut self, buf: &mut [u8; 32]) -> Result<(), ()> {
        self.0 = self.0.wrapping_add(1);
        *buf = [(self.0 & 0x3f) | 0x40; 32];
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A counter, as the Box's store sees it, and the store.
struct Rig {
    dir: TempDir,
    store: ShopStore<MemFiles>,
    next: u64,
    rng: Rng,
}

impl Rig {
    fn new(name: &str) -> Self {
        let dir = TempDir::new(name);
        let store = ShopStore::open(MemFiles::new(), dir.path(), the_box()).unwrap();
        let mut rig = Self {
            dir,
            store,
            next: 1,
            rng: Rng::default(),
        };
        rig.store.sync(Some(counter1().device_id()), T0).unwrap();
        rig
    }

    fn counter_dir(&self) -> PathBuf {
        self.dir
            .path()
            .join(LIGHTNING_DIR)
            .join(hex(&counter1().device_id().to_bytes()))
    }

    fn blob(&self, name: &str) -> PathBuf {
        self.counter_dir().join(hex(name.as_bytes()))
    }

    fn seal_for(&mut self, from: &Identity, content: &[u8], n: u64, kind: Kind) -> Vec<u8> {
        let rng = &mut self.rng;
        seal(
            from,
            Role::Counter,
            &the_box().device_id(),
            Role::Box,
            n,
            kind,
            content,
            &mut |b| rng.fill(b),
        )
        .unwrap()
    }

    /// Send a Store as `from` with the given number and kind; the answer,
    /// opened as the counter would.
    fn send_as(
        &mut self,
        from: &Identity,
        n: u64,
        id: u8,
        body: StoreBody,
        now: u64,
    ) -> Option<StoredMessage> {
        let message = StoreMessage {
            request_id: [id; 16],
            body,
        };
        let sealed = self.seal_for(from, &message.to_bytes(), n, Kind::Store);
        self.answer_to(&sealed, from, now)
    }

    fn answer_to(&mut self, sealed: &[u8], who: &Identity, now: u64) -> Option<StoredMessage> {
        let rng = &mut self.rng;
        let answer = self.store.answer(sealed, now, &mut |b| rng.fill(b));
        let StoreAnswer::Answer(bytes) = answer else {
            return None;
        };
        let opened = open(
            who,
            Role::Counter,
            &bytes,
            |d| (*d == the_box().device_id()).then_some(Role::Box),
            |_| 0,
        )
        .unwrap();
        assert_eq!(opened.kind, Kind::Stored);
        Some(StoredMessage::from_bytes(&opened.content).unwrap())
    }

    fn send(&mut self, body: StoreBody, now: u64) -> StoredMessage {
        let n = self.next;
        self.next += 1;
        let c = counter1();
        self.send_as(&c, n, n as u8, body, now).expect("an answer")
    }

    fn part(
        &mut self,
        name: &str,
        number: u32,
        parts: u32,
        sha: [u8; 32],
        bytes: &[u8],
        now: u64,
    ) -> StoredBody {
        self.send(
            StoreBody::Write {
                name: name.to_string(),
                part: Part {
                    number,
                    parts,
                    sha256: sha,
                    bytes: bytes.to_vec(),
                },
            },
            now,
        )
        .body
    }

    /// Write a whole blob in parts of `chunk` bytes at time `now`; the
    /// answer to the last part.
    fn write(&mut self, name: &str, blob: &[u8], chunk: usize, now: u64) -> StoredBody {
        let sha = sha256_of(blob);
        let pieces: Vec<&[u8]> = if blob.is_empty() {
            vec![&[][..]]
        } else {
            blob.chunks(chunk).collect()
        };
        let parts = pieces.len() as u32;
        let mut last = StoredBody::Removed;
        for (i, piece) in pieces.iter().enumerate() {
            last = self.part(name, i as u32, parts, sha, piece, now);
        }
        last
    }

    fn read_part(&mut self, name: &str, part: u32, now: u64) -> StoredBody {
        self.send(
            StoreBody::Read {
                name: name.to_string(),
                part,
            },
            now,
        )
        .body
    }
}

fn blob_of(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}

// ---- BX-90 ---------------------------------------------------------------

#[test]
fn bx_90_the_named_counter_is_served_and_the_answer_names_its_request_and_action() {
    let mut rig = Rig::new("b90a");
    let c = counter1();
    let message = StoreMessage {
        request_id: [0xab; 16],
        body: StoreBody::List {
            after: String::new(),
        },
    };
    let sealed = rig.seal_for(&c, &message.to_bytes(), 1, Kind::Store);
    let answer = rig.answer_to(&sealed, &c, T0).expect("served");
    assert_eq!(answer.request_id, [0xab; 16]);
    assert_eq!(answer.action, Action::List);
    assert_eq!(
        answer.body,
        StoredBody::Names {
            names: vec![],
            more: false
        }
    );
    assert_eq!(rig.store.served(T0), vec![c.device_id()]);
}

#[test]
fn bx_90_no_other_device_is_served_and_nothing_of_theirs_is_kept() {
    let mut rig = Rig::new("b90b");
    // Another counter of the shop, the owner, a stranger.
    for who in [counter2(), owner(), stranger()] {
        let m = StoreMessage {
            request_id: [1; 16],
            body: StoreBody::Write {
                name: "mine".into(),
                part: Part {
                    number: 0,
                    parts: 1,
                    sha256: sha256_of(b"x"),
                    bytes: b"x".to_vec(),
                },
            },
        };
        let sealed = rig.seal_for(&who, &m.to_bytes(), 1, Kind::Store);
        assert_eq!(rig.answer_to(&sealed, &who, T0), None);
    }
    assert!(rig.store.blobs(&counter1().device_id()).is_empty());
    assert!(!rig.counter_dir().join(hex(b"mine")).exists());
    assert!(!rig
        .dir
        .path()
        .join(LIGHTNING_DIR)
        .join(hex(&counter2().device_id().to_bytes()))
        .exists());
    // Their numbers were not stored either: the named counter's number 1
    // is still free.
    let c = counter1();
    let ok = rig.send_as(
        &c,
        1,
        9,
        StoreBody::List {
            after: String::new(),
        },
        T0,
    );
    assert!(ok.is_some());
}

#[test]
fn bx_90_a_message_that_is_not_a_store_for_this_box_is_refused() {
    let mut rig = Rig::new("b90c");
    let c = counter1();
    // A kind of the main count.
    let paid = rig.seal_for(&c, b"x", 1, Kind::PaidNotice);
    assert_eq!(rig.answer_to(&paid, &c, T0), None);
    // A Store whose content is not one.
    let junk = rig.seal_for(&c, b"junk", 1, Kind::Store);
    assert_eq!(rig.answer_to(&junk, &c, T0), None);
    // A Store sealed for another reader.
    let rng = &mut rig.rng;
    let other = seal(
        &c,
        Role::Counter,
        &counter2().device_id(),
        Role::Box,
        5,
        Kind::Store,
        b"x",
        &mut |b| rng.fill(b),
    )
    .unwrap();
    assert_eq!(rig.answer_to(&other, &c, T0), None);
}

#[test]
fn bx_90_a_store_cannot_be_taken_twice_and_has_numbers_of_its_own() {
    let mut rig = Rig::new("b90d");
    let c = counter1();
    let m = StoreMessage {
        request_id: [3; 16],
        body: StoreBody::List {
            after: String::new(),
        },
    };
    let sealed = rig.seal_for(&c, &m.to_bytes(), 4, Kind::Store);
    assert!(rig.answer_to(&sealed, &c, T0).is_some());
    // The same bytes again, and a lower number.
    assert_eq!(rig.answer_to(&sealed, &c, T0), None);
    let lower = rig.seal_for(&c, &m.to_bytes(), 3, Kind::Store);
    assert_eq!(rig.answer_to(&lower, &c, T0), None);
    // The numbers are in a file of their own (LK-81).
    let stored = rig.store_files_inbox();
    assert_eq!(stored.last(&c.device_id()), 4);
}

impl Rig {
    fn store_files_inbox(&self) -> Inbox {
        // The store keeps its files in `MemFiles`, which it owns; reopening
        // from a copy shows what was written.
        let files = self.store_files();
        Inbox::from_bytes(&files.read(STORE_INBOX_FILE).unwrap().unwrap()).unwrap()
    }

    fn store_files(&self) -> MemFiles {
        self.store.files_for_test().clone()
    }
}

#[test]
fn bx_90_read_list_and_remove_answer_as_lk_83_says() {
    let mut rig = Rig::new("b90e");
    let big = blob_of(150_000, 1);
    assert!(matches!(
        rig.write("big", &big, MAX_PART_LEN, T0),
        StoredBody::Written { .. }
    ));
    assert_eq!(
        rig.write("small", b"s", 10, T0),
        StoredBody::Written {
            sha256: sha256_of(b"s")
        }
    );
    // Read: three parts of 60,000, 60,000 and 30,000, each naming the SHA-256.
    let mut got = Vec::new();
    for i in 0..3 {
        let StoredBody::Part(p) = rig.read_part("big", i, T0) else {
            panic!("a part");
        };
        assert_eq!((p.number, p.parts, p.sha256), (i, 3, sha256_of(&big)));
        got.extend_from_slice(&p.bytes);
    }
    assert_eq!(got, big);
    assert_eq!(
        rig.read_part("big", 3, T0),
        StoredBody::Refused(StoreCause::PartOutOfOrder)
    );
    assert_eq!(rig.read_part("missing", 0, T0), StoredBody::None);
    // List: names in byte order, after a name.
    let list = |rig: &mut Rig, after: &str| {
        rig.send(
            StoreBody::List {
                after: after.into(),
            },
            T0,
        )
        .body
    };
    assert_eq!(
        list(&mut rig, ""),
        StoredBody::Names {
            names: vec!["big".into(), "small".into()],
            more: false
        }
    );
    assert_eq!(
        list(&mut rig, "big"),
        StoredBody::Names {
            names: vec!["small".into()],
            more: false
        }
    );
    // Remove, and again: both say removed.
    for _ in 0..2 {
        assert_eq!(
            rig.send(StoreBody::Remove { name: "big".into() }, T0).body,
            StoredBody::Removed
        );
    }
    assert!(!rig.blob("big").exists());
    assert_eq!(rig.read_part("big", 0, T0), StoredBody::None);
}

#[test]
fn bx_90_a_long_list_comes_in_turns() {
    let mut rig = Rig::new("b90f");
    let names: Vec<String> = (0..600)
        .map(|i| format!("{i:03}{}", "n".repeat(MAX_STORED_NAME_LEN - 3)))
        .collect();
    for n in &names {
        fs::create_dir_all(rig.counter_dir()).unwrap();
        fs::write(rig.blob(n), b"").unwrap();
    }
    // Reopen so the store reads the folder.
    rig.store = ShopStore::open(MemFiles::new(), rig.dir.path(), the_box()).unwrap();
    rig.store.sync(Some(counter1().device_id()), T0).unwrap();
    let mut got: Vec<String> = Vec::new();
    let mut after = String::new();
    let mut turns = 0;
    loop {
        let StoredBody::Names { names, more } = rig
            .send(
                StoreBody::List {
                    after: after.clone(),
                },
                T0,
            )
            .body
        else {
            panic!("names");
        };
        assert!(!names.is_empty());
        after = names.last().unwrap().clone();
        got.extend(names);
        turns += 1;
        if !more {
            break;
        }
    }
    assert!(
        turns >= 2,
        "a list of 75,600 bytes needs more than one turn"
    );
    assert_eq!(got, names);
}

#[test]
fn bx_90_over_the_wire_a_paired_box_serves_the_named_counter_and_an_unpaired_one_no_one() {
    let dir = TempDir::new("b90wire");
    let (r, _) = start(&dir);
    // Unpaired: the hook refuses.
    let c = counter1();
    let m = StoreMessage {
        request_id: [1; 16],
        body: StoreBody::List {
            after: String::new(),
        },
    };
    let s = sealed_to(
        &c,
        Role::Counter,
        &r.device_id(),
        Role::Box,
        1,
        Kind::Store,
        &m.to_bytes(),
    );
    assert!(connect(&r).store(&s).is_err());
    // Paired with settings that name counter 1 and this Box (SL-71).
    pair_with_lightning(&r);
    eventually("the Box serves the counter", || {
        let n = 2 + r.notes().len() as u64;
        let s = sealed_to(
            &c,
            Role::Counter,
            &r.device_id(),
            Role::Box,
            n,
            Kind::Store,
            &m.to_bytes(),
        );
        match connect(&r).store(&s) {
            Ok(answer) => {
                let o = open(
                    &c,
                    Role::Counter,
                    &answer,
                    |d| (*d == r.device_id()).then_some(Role::Box),
                    |_| 0,
                )
                .unwrap();
                StoredMessage::from_bytes(&o.content).unwrap().request_id == [1; 16]
            }
            Err(_) => false,
        }
    });
    // Counter 2 is not served.
    let s = sealed_to(
        &counter2(),
        Role::Counter,
        &r.device_id(),
        Role::Box,
        1,
        Kind::Store,
        &m.to_bytes(),
    );
    assert!(connect(&r).store(&s).is_err());
    r.stop();
}

fn node_id(n: u8) -> [u8; 33] {
    openbmt_core::shop_lightning::node_id(&[n; 64])
}

fn lightning_settings(version: u32, box_key: &DeviceId) -> Settings {
    let mut s = settings_with(version, box_key);
    s.shop_lightning = Some(ShopLightning {
        counter: counter1().device_id(),
        node_id: node_id(1),
        second_device: *box_key,
        provider_node_id: node_id(2),
        provider_addr: "203.0.113.5:9735".parse().unwrap(),
        cap_sats: 1_000_000,
        max_opening_fee_sats: 10_000,
    });
    s
}

fn pair_with_lightning(r: &crate::runner::BoxRunner) {
    use openbmt_core::boxpair::{Claim, Welcome};
    let inv = invitation(5);
    r.enter_invitation(&inv.to_text()).unwrap();
    let mut c = connect(r);
    let claim = Claim::from_bytes(&c.claim().unwrap()).unwrap();
    let welcome = Welcome::make(&inv, &claim, roots());
    let settings = sign_both(lightning_settings(1, &r.device_id()));
    c.welcome(&welcome.to_bytes(), &settings.to_bytes())
        .unwrap();
}

// ---- BX-91 ---------------------------------------------------------------

#[test]
fn bx_91_a_write_says_written_only_after_the_blob_is_in_place() {
    let mut rig = Rig::new("b91a");
    let blob = blob_of(25, 3);
    let sha = sha256_of(&blob);
    // Parts of 10: taken, taken, written.
    assert_eq!(
        rig.part("n", 0, 3, sha, &blob[..10], T0),
        StoredBody::Taken { part: 0 }
    );
    assert!(!rig.blob("n").exists(), "no blob before the last part");
    assert_eq!(
        rig.part("n", 1, 3, sha, &blob[10..20], T0),
        StoredBody::Taken { part: 1 }
    );
    assert!(!rig.blob("n").exists());
    assert_eq!(
        rig.part("n", 2, 3, sha, &blob[20..], T0),
        StoredBody::Written { sha256: sha }
    );
    // At the moment of the answer the file holds the whole blob.
    assert_eq!(fs::read(rig.blob("n")).unwrap(), blob);
    // No temporary file is left.
    let left: Vec<_> = fs::read_dir(rig.counter_dir()).unwrap().flatten().collect();
    assert_eq!(left.len(), 1);
}

#[test]
fn bx_91_a_write_cut_off_or_wrong_leaves_the_old_blob() {
    let mut rig = Rig::new("b91b");
    let old = blob_of(30, 1);
    assert!(matches!(
        rig.write("n", &old, 10, T0),
        StoredBody::Written { .. }
    ));
    // New bytes, one part sent and the rest never: the old blob stays.
    let new = blob_of(30, 9);
    let sha = sha256_of(&new);
    rig.part("n", 0, 3, sha, &new[..10], T0);
    assert_eq!(fs::read(rig.blob("n")).unwrap(), old);
    // The Box restarts: the half is gone and the old blob is what is held.
    rig.store = ShopStore::open(MemFiles::new(), rig.dir.path(), the_box()).unwrap();
    rig.store.sync(Some(counter1().device_id()), T0).unwrap();
    rig.send(
        StoreBody::List {
            after: String::new(),
        },
        T0,
    );
    assert_eq!(fs::read(rig.blob("n")).unwrap(), old);
    let names: Vec<_> = fs::read_dir(rig.counter_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec![hex(b"n")]);
    // Parts with the wrong SHA-256: refused, the old blob stays, no
    // temporary file is left.
    let wrong = [7u8; 32];
    rig.part("n", 0, 2, wrong, &new[..10], T0);
    assert_eq!(
        rig.part("n", 1, 2, wrong, &new[10..20], T0),
        StoredBody::Refused(StoreCause::WrongSha)
    );
    assert_eq!(fs::read(rig.blob("n")).unwrap(), old);
    assert_eq!(fs::read_dir(rig.counter_dir()).unwrap().count(), 1);
}

#[test]
fn bx_91_a_blob_that_cannot_be_put_in_place_is_not_written() {
    let mut rig = Rig::new("b91c");
    // A folder where the blob goes: the rename cannot succeed.
    fs::create_dir_all(rig.blob("n")).unwrap();
    rig.store = ShopStore::open(MemFiles::new(), rig.dir.path(), the_box()).unwrap();
    rig.store.sync(Some(counter1().device_id()), T0).unwrap();
    let blob = blob_of(12, 4);
    assert_eq!(
        rig.write("n", &blob, 6, T0),
        StoredBody::Refused(StoreCause::Storage)
    );
    // Nothing is left of the attempt.
    let names: Vec<_> = fs::read_dir(rig.counter_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec![hex(b"n")]);
}

#[test]
fn bx_91_parts_must_come_in_order_and_name_the_same_blob() {
    let mut rig = Rig::new("b91d");
    let blob = blob_of(30, 2);
    let sha = sha256_of(&blob);
    // A first part that is not part 0.
    assert_eq!(
        rig.part("n", 1, 3, sha, &blob[10..20], T0),
        StoredBody::Refused(StoreCause::PartOutOfOrder)
    );
    // A part skipped.
    rig.part("n", 0, 3, sha, &blob[..10], T0);
    assert_eq!(
        rig.part("n", 2, 3, sha, &blob[20..], T0),
        StoredBody::Refused(StoreCause::PartOutOfOrder)
    );
    // The write was dropped with the refusal: part 1 now has no write.
    assert_eq!(
        rig.part("n", 1, 3, sha, &blob[10..20], T0),
        StoredBody::Refused(StoreCause::PartOutOfOrder)
    );
    // Parts that name another count or another SHA-256.
    rig.part("n", 0, 3, sha, &blob[..10], T0);
    assert_eq!(
        rig.part("n", 1, 4, sha, &blob[10..20], T0),
        StoredBody::Refused(StoreCause::PartOutOfOrder)
    );
    rig.part("n", 0, 3, sha, &blob[..10], T0);
    assert_eq!(
        rig.part("n", 1, 3, [1; 32], &blob[10..20], T0),
        StoredBody::Refused(StoreCause::PartOutOfOrder)
    );
    assert!(!rig.blob("n").exists());
}

#[test]
fn bx_91_a_write_whose_parts_stop_for_60_seconds_is_dropped() {
    let mut rig = Rig::new("b91e");
    let blob = blob_of(20, 5);
    let sha = sha256_of(&blob);
    rig.part("n", 0, 2, sha, &blob[..10], T0);
    // 59 seconds: still waiting.
    assert_eq!(
        rig.part("n", 1, 2, sha, &blob[10..], T0 + PART_TIMEOUT_SECS - 1),
        StoredBody::Written { sha256: sha }
    );
    let other = blob_of(20, 6);
    let sha = sha256_of(&other);
    rig.part("m", 0, 2, sha, &other[..10], T0 + 100);
    // 60 seconds: dropped, the temporary file is gone.
    assert_eq!(
        rig.part("m", 1, 2, sha, &other[10..], T0 + 100 + PART_TIMEOUT_SECS),
        StoredBody::Refused(StoreCause::PartOutOfOrder)
    );
    assert!(!rig.blob("m").exists());
    assert_eq!(fs::read_dir(rig.counter_dir()).unwrap().count(), 1);
    // Each part moves the time on: parts 50 seconds apart go on.
    let long = blob_of(30, 7);
    let sha = sha256_of(&long);
    rig.part("l", 0, 3, sha, &long[..10], T0 + 200);
    rig.part("l", 1, 3, sha, &long[10..20], T0 + 250);
    assert!(matches!(
        rig.part("l", 2, 3, sha, &long[20..], T0 + 300),
        StoredBody::Written { .. }
    ));
}

#[test]
fn bx_91_at_most_eight_writes_are_in_progress_and_the_ninth_is_refused_with_its_cause() {
    let mut rig = Rig::new("b91f");
    let blob = blob_of(20, 8);
    let sha = sha256_of(&blob);
    for i in 0..MAX_WRITES {
        assert_eq!(
            rig.part(&format!("w{i}"), 0, 2, sha, &blob[..10], T0),
            StoredBody::Taken { part: 0 }
        );
    }
    assert_eq!(
        rig.part("w8", 0, 2, sha, &blob[..10], T0),
        StoredBody::Refused(StoreCause::TooManyWrites)
    );
    // A write goes on while the limit holds, and finishing it makes room.
    assert!(matches!(
        rig.part("w0", 1, 2, sha, &blob[10..], T0),
        StoredBody::Written { .. }
    ));
    assert_eq!(
        rig.part("w8", 0, 2, sha, &blob[..10], T0),
        StoredBody::Taken { part: 0 }
    );
    // A new first part of a name already in progress restarts it and takes
    // no extra place.
    assert_eq!(
        rig.part("w1", 0, 2, sha, &blob[..10], T0),
        StoredBody::Taken { part: 0 }
    );
}

// ---- BX-92 ---------------------------------------------------------------

#[test]
fn bx_92_a_write_past_10000_names_is_refused_and_the_page_says_the_store_is_full() {
    let mut rig = Rig::new("b92a");
    fs::create_dir_all(rig.counter_dir()).unwrap();
    for i in 0..MAX_NAMES - 1 {
        fs::write(rig.blob(&format!("n{i}")), b"").unwrap();
    }
    rig.store = ShopStore::open(MemFiles::new(), rig.dir.path(), the_box()).unwrap();
    rig.store.sync(Some(counter1().device_id()), T0).unwrap();
    // The 10,000th name fits; the 10,001st does not; a name that is held
    // can be written again.
    assert!(matches!(
        rig.write("last", b"x", 10, T0),
        StoredBody::Written { .. }
    ));
    assert_eq!(
        rig.write("one-more", b"x", 10, T0),
        StoredBody::Refused(StoreCause::TooManyNames)
    );
    assert!(matches!(
        rig.write("last", b"yy", 10, T0),
        StoredBody::Written { .. }
    ));
    assert!(rig.store.is_full());
    // Removing a name makes room again.
    rig.send(StoreBody::Remove { name: "n0".into() }, T0);
    assert!(!rig.store.is_full());
    assert!(matches!(
        rig.write("one-more", b"x", 10, T0),
        StoredBody::Written { .. }
    ));
}

#[test]
fn bx_92_a_write_past_1_gib_is_refused_and_a_smaller_one_is_not() {
    let mut rig = Rig::new("b92b");
    fs::create_dir_all(rig.counter_dir()).unwrap();
    // A sparse file that makes the store hold 1 GiB less 10 bytes.
    let big = fs::File::create(rig.blob("big")).unwrap();
    big.set_len(MAX_STORE_BYTES - 10).unwrap();
    drop(big);
    rig.store = ShopStore::open(MemFiles::new(), rig.dir.path(), the_box()).unwrap();
    rig.store.sync(Some(counter1().device_id()), T0).unwrap();
    assert_eq!(
        rig.write("n", &blob_of(11, 1), 100, T0),
        StoredBody::Refused(StoreCause::StoreFull)
    );
    assert!(!rig.blob("n").exists());
    assert!(rig.store.is_full());
    assert!(matches!(
        rig.write("n", &blob_of(10, 1), 100, T0),
        StoredBody::Written { .. }
    ));
    // Writing a name again counts the new blob, not both.
    assert!(matches!(
        rig.write("n", &blob_of(10, 2), 100, T0),
        StoredBody::Written { .. }
    ));
    // Removing the big blob frees the store.
    rig.send(StoreBody::Remove { name: "big".into() }, T0);
    assert!(!rig.store.is_full());
}

#[test]
fn bx_92_a_blob_past_16_mib_is_refused_part_by_part() {
    let mut rig = Rig::new("b92c");
    let chunk = vec![1u8; MAX_PART_LEN];
    let sha = [3u8; 32];
    let mut answer = StoredBody::Removed;
    for i in 0..280u32 {
        answer = rig.part("huge", i, 280, sha, &chunk, T0);
        if matches!(answer, StoredBody::Refused(_)) {
            break;
        }
    }
    assert_eq!(answer, StoredBody::Refused(StoreCause::BlobTooLarge));
    assert!(!rig.blob("huge").exists());
    assert_eq!(fs::read_dir(rig.counter_dir()).unwrap().count(), 0);
}

#[test]
fn bx_92_the_status_page_shows_the_full_store_and_otherwise_nothing() {
    let view = |store_full| PageView {
        status: crate::state::BoxStatus::Paired,
        version: "1.0.0",
        relay_port: 8986,
        devices: 3,
        shop: Some(roots().owner.fingerprint_words()),
        last_note: None,
        store_full,
    };
    let words = store_full_words();
    assert_eq!(openbmt_core::say::check(&words), Ok(()));
    let full = render_home(&view(true), "box.local:8987", None);
    let shown = |t: &str| t.replace('\'', "&#39;");
    assert!(full.contains(&shown(&words.heading)) && full.contains(&shown(&words.what)));
    let plain = render_home(&view(false), "box.local:8987", None);
    assert!(!plain.contains(&shown(&words.heading)));
}

// ---- BX-93 ---------------------------------------------------------------

#[test]
fn bx_93_a_blob_is_a_file_named_by_the_hex_of_its_name_in_the_counters_folder() {
    let mut rig = Rig::new("b93a");
    for name in ["a", "Mon_itor-1~x", &"z".repeat(MAX_STORED_NAME_LEN)] {
        assert!(matches!(
            rig.write(name, b"data", 10, T0),
            StoredBody::Written { .. }
        ));
        let path = rig.blob(name);
        assert_eq!(fs::read(&path).unwrap(), b"data");
        let want = rig
            .dir
            .path()
            .join("lightning")
            .join(hex(&counter1().device_id().to_bytes()))
            .join(hex(name.as_bytes()));
        assert_eq!(path, want);
    }
}

#[test]
fn bx_93_a_name_whose_hexadecimal_is_no_file_name_is_refused_with_its_cause() {
    let mut rig = Rig::new("b93d");
    let long = "y".repeat(MAX_STORED_NAME_LEN + 1);
    assert_eq!(
        rig.write(&long, b"x", 10, T0),
        StoredBody::Refused(StoreCause::NameTooLong)
    );
    assert_eq!(rig.read_part(&long, 0, T0), StoredBody::None);
    assert!(
        rig.write(&"y".repeat(MAX_STORED_NAME_LEN), b"x", 10, T0)
            != StoredBody::Refused(StoreCause::NameTooLong)
    );
}

#[test]
fn bx_93_every_file_is_mode_0600_and_the_folder_is_the_boxs_alone() {
    let mut rig = Rig::new("b93b");
    assert!(matches!(
        rig.write("n", &blob_of(30, 1), 10, T0),
        StoredBody::Written { .. }
    ));
    let mode = |p: &PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&rig.blob("n")), 0o600);
    // The temporary file of a write in progress too.
    let sha = sha256_of(b"0123456789ab");
    rig.part("t", 0, 2, sha, b"0123456789", T0);
    let tmp = rig.counter_dir().join(format!("{}.tmp", hex(b"t")));
    assert_eq!(mode(&tmp), 0o600);
    assert_eq!(mode(&rig.counter_dir()) & 0o077, 0);
}

#[test]
fn bx_93_the_state_survives_a_restart_and_a_file_that_is_not_a_blob_is_left_alone() {
    let mut rig = Rig::new("b93c");
    assert!(matches!(
        rig.write("keep", b"abc", 10, T0),
        StoredBody::Written { .. }
    ));
    fs::write(rig.counter_dir().join("README"), b"not a blob").unwrap();
    fs::write(rig.counter_dir().join("zz"), b"not hex").unwrap();
    fs::write(rig.counter_dir().join(hex(b"has space")), b"not a name").unwrap();
    let files = rig.store_files();
    rig.store = ShopStore::open(files, rig.dir.path(), the_box()).unwrap();
    rig.store.sync(Some(counter1().device_id()), T0).unwrap();
    assert_eq!(
        rig.store.blobs(&counter1().device_id()),
        Vec::<(String, u64)>::new()
    );
    // The folder is read when the counter is first served after a start.
    assert_eq!(
        rig.read_part("keep", 0, T0),
        StoredBody::Part(Part {
            number: 0,
            parts: 1,
            sha256: sha256_of(b"abc"),
            bytes: b"abc".to_vec(),
        })
    );
    assert_eq!(
        rig.store.blobs(&counter1().device_id()),
        vec![("keep".to_string(), 3)]
    );
    assert!(rig.counter_dir().join("README").exists());
}

// ---- BX-94 ---------------------------------------------------------------

#[test]
fn bx_94_a_counter_no_longer_named_is_served_for_30_days_and_then_its_state_is_gone() {
    let mut rig = Rig::new("b94a");
    assert!(matches!(
        rig.write("n", b"state", 10, T0),
        StoredBody::Written { .. }
    ));
    // The settings stop naming this Box.
    rig.store.sync(None, T0 + 1_000).unwrap();
    let c = counter1().device_id();
    assert_eq!(rig.store.served(T0 + 1_000), vec![c]);
    // It still stores, so a node closing its channels stores twice.
    let last_day = T0 + 1_000 + GRACE_SECS - 1;
    assert!(matches!(
        rig.write("n", b"newer", 10, last_day),
        StoredBody::Written { .. }
    ));
    rig.store.sync(None, last_day).unwrap();
    assert!(rig.blob("n").exists());
    // At 30 days it is not served and its folder is deleted.
    let end = T0 + 1_000 + GRACE_SECS;
    rig.store.sync(None, end).unwrap();
    assert!(rig.store.served(end).is_empty());
    assert!(!rig.counter_dir().exists());
    let m = StoreMessage {
        request_id: [1; 16],
        body: StoreBody::List {
            after: String::new(),
        },
    };
    let sealed = rig.seal_for(&counter1(), &m.to_bytes(), 1_000, Kind::Store);
    assert_eq!(rig.answer_to(&sealed, &counter1(), end), None);
}

#[test]
fn bx_94_a_counter_named_again_within_30_days_finds_its_state_as_it_was() {
    let mut rig = Rig::new("b94b");
    assert!(matches!(
        rig.write("n", b"state", 10, T0),
        StoredBody::Written { .. }
    ));
    rig.store.sync(None, T0 + 10).unwrap();
    rig.store
        .sync(Some(counter1().device_id()), T0 + 10 + GRACE_SECS / 2)
        .unwrap();
    // Named: the 30 days no longer run, even long after.
    let much_later = T0 + 10 + 3 * GRACE_SECS;
    rig.store
        .sync(Some(counter1().device_id()), much_later)
        .unwrap();
    assert_eq!(rig.store.served(much_later), vec![counter1().device_id()]);
    assert_eq!(fs::read(rig.blob("n")).unwrap(), b"state");
    assert_eq!(
        rig.read_part("n", 0, much_later),
        StoredBody::Part(Part {
            number: 0,
            parts: 1,
            sha256: sha256_of(b"state"),
            bytes: b"state".to_vec(),
        })
    );
}

#[test]
fn bx_94_the_30_days_survive_a_restart_and_another_counter_is_not_touched() {
    let mut rig = Rig::new("b94c");
    assert!(matches!(
        rig.write("n", b"one", 10, T0),
        StoredBody::Written { .. }
    ));
    // Another counter takes over; the first is in its 30 days.
    rig.store
        .sync(Some(counter2().device_id()), T0 + 5)
        .unwrap();
    let mut served = rig.store.served(T0 + 6);
    served.sort();
    let mut want = vec![counter1().device_id(), counter2().device_id()];
    want.sort();
    assert_eq!(served, want);
    // A restart keeps the end of the 30 days.
    let files = rig.store_files();
    assert!(files.read(TENANTS_FILE).unwrap().is_some());
    let mut reopened = ShopStore::open(files, rig.dir.path(), the_box()).unwrap();
    assert_eq!(reopened.served(T0 + 6).len(), 2);
    reopened
        .sync(Some(counter2().device_id()), T0 + 5 + GRACE_SECS)
        .unwrap();
    assert_eq!(
        reopened.served(T0 + 5 + GRACE_SECS),
        vec![counter2().device_id()]
    );
    assert!(!rig.counter_dir().exists());
}

#[test]
fn bx_94_a_damaged_tenants_file_stops_the_store_and_says_which() {
    let dir = TempDir::new("b94d");
    let mut files = MemFiles::new();
    files.put(TENANTS_FILE, b"garbage");
    let err = ShopStore::open(files, dir.path(), the_box()).unwrap_err();
    assert_eq!(
        err,
        crate::lightning::StoreError::Damaged { file: TENANTS_FILE }
    );
    assert!(err.to_string().starts_with("BX-51"));
}

// ---- BX-95 ---------------------------------------------------------------

#[test]
fn bx_95_the_cases_of_the_rule_are_each_refused_or_served() {
    let mut rig = Rig::new("b95a");
    // A write counts only after the file is in place.
    let blob = blob_of(40, 3);
    let sha = sha256_of(&blob);
    rig.part("n", 0, 2, sha, &blob[..20], T0);
    assert!(!rig.blob("n").exists());
    assert_eq!(
        rig.part("n", 1, 2, sha, &blob[20..], T0),
        StoredBody::Written { sha256: sha }
    );
    assert!(rig.blob("n").exists());
    // A part with the wrong SHA-256 is refused.
    rig.part("m", 0, 1, [0; 32], b"x", T0);
    assert!(!rig.blob("m").exists());
    // A store request from any device other than the named counter.
    let m = StoreMessage {
        request_id: [1; 16],
        body: StoreBody::List {
            after: String::new(),
        },
    };
    let sealed = rig.seal_for(&counter2(), &m.to_bytes(), 1, Kind::Store);
    assert_eq!(rig.answer_to(&sealed, &counter2(), T0), None);
}

#[test]
fn bx_95_a_write_cut_off_leaves_the_old_blob_and_a_counter_no_longer_named_is_served_30_days() {
    let mut rig = Rig::new("b95b");
    assert!(matches!(
        rig.write("n", b"old", 10, T0),
        StoredBody::Written { .. }
    ));
    let sha = sha256_of(b"0123456789");
    rig.part("n", 0, 2, sha, b"01234", T0);
    // The part timeout passes with no more parts.
    rig.send(
        StoreBody::List {
            after: String::new(),
        },
        T0 + PART_TIMEOUT_SECS,
    );
    assert_eq!(fs::read(rig.blob("n")).unwrap(), b"old");
    rig.store.sync(None, T0 + 100).unwrap();
    assert!(matches!(
        rig.write("n", b"final", 10, T0 + 100 + GRACE_SECS - 1),
        StoredBody::Written { .. }
    ));
    rig.store.sync(None, T0 + 100 + GRACE_SECS).unwrap();
    assert!(!rig.counter_dir().exists());
}
