//! LK-80 to LK-84: Shop Lightning's store.

use std::sync::{Arc, Mutex};

use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{Settings, ShopLightning};

use super::kit::*;
use super::relay::Rig;
use crate::numbers::{Inbox, Outbox, TakeError, STORE_INBOX_FILE, STORE_OUTBOX_FILE};
use crate::proto::{Refusal, Request, Response};
use crate::relay::StoreAnswer;
use crate::seal::{open, seal, Kind, OpenError, Role, StoreParties};
use crate::shopln::*;

fn node_key(n: u8) -> [u8; 33] {
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let sk = bitcoin::secp256k1::SecretKey::from_slice(&[n; 32]).unwrap();
    bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk).serialize()
}

fn lightning_settings() -> Settings {
    let mut s = Settings::defaults(1_000);
    s.shop_lightning = Some(ShopLightning {
        counter: counter().device_id(),
        node_id: node_key(7),
        second_device: the_box().device_id(),
        provider_node_id: node_key(8),
        provider_addr: "203.0.113.5:9735".parse().unwrap(),
        cap_sats: 1_000_000,
        max_opening_fee_sats: 10_000,
    });
    s
}

fn parties() -> StoreParties {
    StoreParties::from_settings(&lightning_settings()).unwrap()
}

fn sealed(
    from: &openbmt_core::identity::Identity,
    fr: Role,
    to: DeviceId,
    tr: Role,
    n: u64,
    kind: Kind,
    c: &[u8],
) -> Vec<u8> {
    let mut rng = Counting(0);
    seal(from, fr, &to, tr, n, kind, c, &mut |b| rng.fill(b)).unwrap()
}

fn write_message() -> StoreMessage {
    StoreMessage {
        request_id: [7; 16],
        body: StoreBody::Write {
            name: "monitor_update-1~a".to_string(),
            part: Part {
                number: 2,
                parts: 3,
                sha256: [9; 32],
                bytes: vec![5; 1000],
            },
        },
    }
}

fn store_messages() -> Vec<StoreMessage> {
    let id = [3; 16];
    vec![
        write_message(),
        StoreMessage {
            request_id: id,
            body: StoreBody::Write {
                name: "a".to_string(),
                part: Part {
                    number: 0,
                    parts: 1,
                    sha256: [1; 32],
                    bytes: vec![],
                },
            },
        },
        StoreMessage {
            request_id: id,
            body: StoreBody::Write {
                name: "b".repeat(255),
                part: Part {
                    number: 279,
                    parts: 280,
                    sha256: [2; 32],
                    bytes: vec![8; MAX_PART_LEN],
                },
            },
        },
        StoreMessage {
            request_id: id,
            body: StoreBody::Read {
                name: "state".to_string(),
                part: 4,
            },
        },
        StoreMessage {
            request_id: id,
            body: StoreBody::List {
                after: String::new(),
            },
        },
        StoreMessage {
            request_id: id,
            body: StoreBody::List {
                after: "m".to_string(),
            },
        },
        StoreMessage {
            request_id: id,
            body: StoreBody::Remove {
                name: "gone".to_string(),
            },
        },
    ]
}

fn stored_messages() -> Vec<StoredMessage> {
    let id = [4; 16];
    let part = Part {
        number: 1,
        parts: 2,
        sha256: [6; 32],
        bytes: vec![1, 2, 3],
    };
    let m = |action, body| StoredMessage {
        request_id: id,
        action,
        body,
    };
    let mut v = vec![
        m(Action::Write, StoredBody::Taken { part: 3 }),
        m(Action::Write, StoredBody::Written { sha256: [5; 32] }),
        m(Action::Read, StoredBody::Part(part)),
        m(Action::Read, StoredBody::None),
        m(
            Action::List,
            StoredBody::Names {
                names: vec!["a".into(), "b".into(), "ba".into()],
                more: true,
            },
        ),
        m(
            Action::List,
            StoredBody::Names {
                names: vec![],
                more: false,
            },
        ),
        m(Action::Remove, StoredBody::Removed),
    ];
    for cause in StoreCause::ALL {
        for action in [Action::Write, Action::Read, Action::List, Action::Remove] {
            v.push(m(action, StoredBody::Refused(cause)));
        }
    }
    v
}

/// Every prefix is refused, one more byte is refused, and a flipped bit
/// either fails or decodes to a value that writes back the same bytes.
fn strict<T>(
    valid: &[u8],
    decode: impl Fn(&[u8]) -> Result<T, openbmt_core::wire::DecodeError>,
    encode: impl Fn(&T) -> Vec<u8>,
) {
    let value = decode(valid).expect("valid bytes decode");
    assert_eq!(encode(&value), valid);
    // A long part would make this loop slow; the prefixes of the head and
    // of the tail are enough.
    let cuts: Vec<usize> = (0..valid.len().min(200))
        .chain(valid.len().saturating_sub(200)..valid.len())
        .collect();
    for n in cuts {
        assert!(decode(&valid[..n]).is_err(), "prefix of {n} bytes decoded");
    }
    let mut longer = valid.to_vec();
    longer.push(0);
    assert!(decode(&longer).is_err());
    let mut m = valid.to_vec();
    let flips: Vec<usize> = (0..m.len().min(120)).collect();
    for i in flips {
        for bit in 0..8 {
            m[i] ^= 1 << bit;
            if let Ok(v) = decode(&m) {
                assert_eq!(encode(&v), m, "byte {i} bit {bit} decoded non-canonically");
            }
            m[i] ^= 1 << bit;
        }
    }
}

// ---- LK-80 ---------------------------------------------------------------

#[test]
fn lk_80_the_bytes_of_the_kinds_are_14_15_and_16_after_the_starter_line() {
    assert_eq!(Kind::Store.to_byte(), 14);
    assert_eq!(Kind::Stored.to_byte(), 15);
    assert_eq!(Kind::LightningReport.to_byte(), 16);
    assert_eq!(Kind::from_byte(13), Some(Kind::Starter));
    assert_eq!(Kind::from_byte(17), None);
    for k in [Kind::Store, Kind::Stored, Kind::LightningReport] {
        assert_eq!(Kind::from_byte(k.to_byte()), Some(k));
        assert!(k.is_shop_lightning());
    }
    assert!(!Kind::Command.is_shop_lightning());
}

#[test]
fn lk_80_store_goes_counter_to_box_and_stored_back_and_no_other_way() {
    let counter_id = counter().device_id();
    let box_id = the_box().device_id();
    let store = sealed(
        &counter(),
        Role::Counter,
        box_id,
        Role::Box,
        1,
        Kind::Store,
        b"s",
    );
    let opened = open(&the_box(), Role::Box, &store, allowed, none_taken).unwrap();
    assert_eq!(opened.kind, Kind::Store);
    let stored = sealed(
        &the_box(),
        Role::Box,
        counter_id,
        Role::Counter,
        1,
        Kind::Stored,
        b"s",
    );
    assert!(open(&counter(), Role::Counter, &stored, allowed, none_taken).is_ok());
    let report = sealed(
        &counter(),
        Role::Counter,
        owner().device_id(),
        Role::Owner,
        1,
        Kind::LightningReport,
        b"r",
    );
    assert!(open(&owner(), Role::Owner, &report, allowed, none_taken).is_ok());
    // The owner may not send a Store, and the Box may not send one to the owner.
    let mut rng = Counting(0);
    for (from, fr, to, tr, kind) in [
        (owner(), Role::Owner, box_id, Role::Box, Kind::Store),
        (
            the_box(),
            Role::Box,
            owner().device_id(),
            Role::Owner,
            Kind::Stored,
        ),
        (counter(), Role::Counter, box_id, Role::Box, Kind::Stored),
        (the_box(), Role::Box, counter_id, Role::Counter, Kind::Store),
        (
            owner(),
            Role::Owner,
            counter_id,
            Role::Counter,
            Kind::LightningReport,
        ),
        (
            counter(),
            Role::Counter,
            box_id,
            Role::Box,
            Kind::LightningReport,
        ),
    ] {
        let r = seal(&from, fr, &to, tr, 1, kind, b"x", &mut |b| rng.fill(b));
        assert!(r.is_err(), "{kind:?} {fr:?} to {tr:?}");
    }
}

#[test]
fn lk_80_only_the_named_counter_and_box_may_send_the_kinds() {
    let p = parties();
    let (c, b, o) = (
        counter().device_id(),
        the_box().device_id(),
        owner().device_id(),
    );
    let other_counter = stranger().device_id();
    assert!(StoreParties::allows(Some(&p), Kind::Store, &c, &b));
    assert!(StoreParties::allows(Some(&p), Kind::Stored, &b, &c));
    assert!(StoreParties::allows(
        Some(&p),
        Kind::LightningReport,
        &c,
        &o
    ));
    // Another counter, another Box, the wrong way round.
    assert!(!StoreParties::allows(
        Some(&p),
        Kind::Store,
        &other_counter,
        &b
    ));
    assert!(!StoreParties::allows(
        Some(&p),
        Kind::Store,
        &c,
        &other_counter
    ));
    assert!(!StoreParties::allows(
        Some(&p),
        Kind::Stored,
        &other_counter,
        &c
    ));
    assert!(!StoreParties::allows(
        Some(&p),
        Kind::Stored,
        &b,
        &other_counter
    ));
    assert!(!StoreParties::allows(Some(&p), Kind::Store, &b, &c));
    assert!(!StoreParties::allows(
        Some(&p),
        Kind::LightningReport,
        &other_counter,
        &o
    ));
    // With Shop Lightning off nobody may.
    for kind in [Kind::Store, Kind::Stored, Kind::LightningReport] {
        assert!(!StoreParties::allows(None, kind, &c, &b));
    }
    // A kind that is not Shop Lightning's is not this check's.
    assert!(StoreParties::allows(None, Kind::Command, &o, &c));
    // The settings without the field name no parties.
    assert_eq!(
        StoreParties::from_settings(&Settings::defaults(1_000)),
        None
    );
    assert_eq!(
        p,
        StoreParties {
            counter: c,
            second_device: b
        }
    );
}

// ---- LK-81 ---------------------------------------------------------------

fn counter_to_box(n: u64, kind: Kind) -> Vec<u8> {
    sealed(
        &counter(),
        Role::Counter,
        the_box().device_id(),
        Role::Box,
        n,
        kind,
        b"x",
    )
}

#[test]
fn lk_81_a_store_and_a_paid_notice_never_refuse_each_other() {
    let ok = |_: &[u8]| -> Result<(), ()> { Ok(()) };
    let mut main = Inbox::default();
    let mut apart = Inbox::default();
    // A Paid notice waiting with number 7, and Stores 1, 2, 3.
    let paid = counter_to_box(7, Kind::PaidNotice);
    assert!(main.open(&the_box(), Role::Box, &paid, allowed, ok).is_ok());
    for n in 1..=3 {
        let m = counter_to_box(n, Kind::Store);
        assert!(
            apart
                .open_store(&the_box(), Role::Box, &m, allowed, ok)
                .is_ok(),
            "{n}"
        );
    }
    assert_eq!(main.last(&counter().device_id()), 7);
    assert_eq!(apart.last(&counter().device_id()), 3);
    // A replayed Store is refused by its own count; a Paid notice of a
    // lower number is refused by the other one.
    let replay = counter_to_box(3, Kind::Store);
    assert!(matches!(
        apart.open_store(&the_box(), Role::Box, &replay, allowed, ok),
        Err(TakeError::Open(OpenError::Replay { last: 3, got: 3 }))
    ));
    let low_paid = counter_to_box(6, Kind::PaidNotice);
    assert!(main
        .open(&the_box(), Role::Box, &low_paid, allowed, ok)
        .is_err());
    // The Store count did not move the other.
    assert_eq!(main.last(&counter().device_id()), 7);
}

#[test]
fn lk_81_each_count_takes_only_its_own_kinds() {
    let ok = |_: &[u8]| -> Result<(), ()> { Ok(()) };
    let mut main = Inbox::default();
    let mut apart = Inbox::default();
    let store = counter_to_box(1, Kind::Store);
    let paid = counter_to_box(1, Kind::PaidNotice);
    let refused = main.open(&the_box(), Role::Box, &store, allowed, ok);
    assert!(matches!(
        refused,
        Err(TakeError::Open(OpenError::KindNotAllowed(Kind::Store)))
    ));
    let refused = apart.open_store(&the_box(), Role::Box, &paid, allowed, ok);
    assert!(matches!(
        refused,
        Err(TakeError::Open(OpenError::KindNotAllowed(Kind::PaidNotice)))
    ));
    // A refusal stores nothing.
    assert_eq!(main.last(&counter().device_id()), 0);
    assert_eq!(apart.last(&counter().device_id()), 0);
    // Stored comes back on the counter's own count.
    let stored = sealed(
        &the_box(),
        Role::Box,
        counter().device_id(),
        Role::Counter,
        1,
        Kind::Stored,
        b"x",
    );
    assert!(apart
        .open_store(&counter(), Role::Counter, &stored, allowed, ok)
        .is_ok());
}

#[test]
fn lk_81_the_counts_are_stored_before_use_in_files_of_their_own() {
    assert_ne!(STORE_INBOX_FILE, "link-inbox");
    assert_ne!(STORE_OUTBOX_FILE, "link-outbox");
    let stored: std::cell::RefCell<Option<Vec<u8>>> = std::cell::RefCell::new(None);
    let mut apart = Inbox::default();
    let m = counter_to_box(4, Kind::Store);
    apart
        .open_store(&the_box(), Role::Box, &m, allowed, |bytes| {
            *stored.borrow_mut() = Some(bytes.to_vec());
            Ok::<(), ()>(())
        })
        .unwrap();
    let back = Inbox::from_bytes(&stored.into_inner().unwrap()).unwrap();
    assert_eq!(back, apart);
    // A failed store gives nothing out and changes nothing.
    let mut again = back.clone();
    let m5 = counter_to_box(5, Kind::Store);
    let r = again.open_store(&the_box(), Role::Box, &m5, allowed, |_| {
        Err::<(), _>("disk")
    });
    assert!(matches!(r, Err(TakeError::Persist("disk"))));
    assert_eq!(again, back);
    // Two outboxes count on their own.
    let (mut main, mut apart) = (Outbox::default(), Outbox::default());
    let reader = the_box().device_id();
    let ok = |_: &[u8]| -> Result<(), ()> { Ok(()) };
    assert_eq!(main.next(&reader, ok), Ok(1));
    assert_eq!(main.next(&reader, ok), Ok(2));
    assert_eq!(apart.next(&reader, ok), Ok(1));
    assert_eq!(main.last(&reader), 2);
    assert_eq!(apart.last(&reader), 1);
}

// ---- LK-82 ---------------------------------------------------------------

#[test]
fn lk_82_a_relay_with_no_hook_refuses_the_store_request() {
    let mut rig = Rig::new();
    let m = counter_to_box(1, Kind::Store);
    assert_eq!(
        rig.ask(Request::Store { sealed: m }),
        Response::Refused(Refusal::NoStore)
    );
    assert_eq!(Refusal::NoStore.rule(), "LK-82");
    assert_eq!(Refusal::StoreRefused.rule(), "LK-82");
}

#[test]
fn lk_82_the_request_goes_to_the_hook_and_its_answer_comes_back_with_no_challenge() {
    let mut rig = Rig::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    rig.relay.set_store_hook(Some(Box::new(move |m, now| {
        log.lock().unwrap().push((m.to_vec(), now));
        StoreAnswer::Answer(vec![1, 2, 3])
    })));
    let m = counter_to_box(1, Kind::Store);
    // No challenge was asked for on this connection.
    let answer = rig.ask(Request::Store { sealed: m.clone() });
    assert_eq!(
        answer,
        Response::StoreAnswer {
            sealed: vec![1, 2, 3]
        }
    );
    assert_eq!(*seen.lock().unwrap(), vec![(m, rig.now)]);
}

#[test]
fn lk_82_a_store_never_waits_in_a_mailbox() {
    let mut rig = Rig::new();
    rig.relay
        .set_store_hook(Some(Box::new(|_, _| StoreAnswer::Refused)));
    assert_eq!(rig.make(&the_box()), Response::Made);
    let before = rig.relay.mailbox(&the_box().device_id()).map(|m| m.len());
    let answer = rig.ask(Request::Store {
        sealed: counter_to_box(1, Kind::Store),
    });
    assert_eq!(answer, Response::Refused(Refusal::StoreRefused));
    assert_eq!(
        rig.relay.mailbox(&the_box().device_id()).map(|m| m.len()),
        before
    );
    // A Store sent as a plain put is no more taken than any other kind
    // that the relay would hold: the hook is not called for it.
    let called = Arc::new(Mutex::new(0));
    let c = Arc::clone(&called);
    rig.relay.set_store_hook(Some(Box::new(move |_, _| {
        *c.lock().unwrap() += 1;
        StoreAnswer::Refused
    })));
    let _ = rig.ask(Request::Challenge);
    assert_eq!(*called.lock().unwrap(), 0);
}

// ---- LK-83 ---------------------------------------------------------------

#[test]
fn lk_83_store_messages_round_trip_and_decode_strictly() {
    for m in store_messages() {
        let bytes = m.to_bytes();
        assert_eq!(StoreMessage::from_bytes(&bytes).unwrap(), m);
        strict(&bytes, StoreMessage::from_bytes, StoreMessage::to_bytes);
        assert!(bytes.len() <= crate::seal::MAX_CONTENT_LEN);
    }
}

#[test]
fn lk_83_stored_messages_round_trip_and_decode_strictly() {
    for m in stored_messages() {
        let bytes = m.to_bytes();
        assert_eq!(StoredMessage::from_bytes(&bytes).unwrap(), m);
        strict(&bytes, StoredMessage::from_bytes, StoredMessage::to_bytes);
    }
}

#[test]
fn lk_83_the_limits_are_the_rules() {
    assert_eq!(MAX_PART_LEN, 60_000);
    assert_eq!(MAX_BLOB_LEN, 16 * 1024 * 1024);
    assert_eq!(MAX_PARTS, 280);
    assert_eq!(MAX_NAME_LEN, 255);
    for ok in ["a", "Z", "0", "_", "-", "~", "ab_C-d~9", &"x".repeat(255)] {
        assert!(name_ok(ok), "{ok}");
    }
    for bad in ["", "a b", "a/b", "a.b", "é", "a\n", &"x".repeat(256)] {
        assert!(!name_ok(bad), "{bad:?}");
    }
}

#[test]
fn lk_83_a_store_with_anything_malformed_is_refused() {
    let good = write_message().to_bytes();
    let name_at = 4 + 1 + 16 + 1;
    // Header: magic, version, action.
    let mut b = good.clone();
    b[0] ^= 1;
    assert!(StoreMessage::from_bytes(&b).is_err());
    let mut b = good.clone();
    b[4] = 2;
    assert!(StoreMessage::from_bytes(&b).is_err());
    for action in [0u8, 5, 255] {
        let mut b = good.clone();
        b[4 + 1 + 16] = action;
        assert!(StoreMessage::from_bytes(&b).is_err(), "action {action}");
    }
    // Names: a character outside the set, an empty name, a length that
    // runs past the bytes.
    let mut b = good.clone();
    b[name_at + 1] = b' ';
    assert!(StoreMessage::from_bytes(&b).is_err());
    let mut b = good.clone();
    b[name_at] = 0;
    assert!(StoreMessage::from_bytes(&b).is_err());
    let mut b = good.clone();
    b[name_at] = 255;
    assert!(StoreMessage::from_bytes(&b).is_err());
    // Part numbers, in the message and as built.
    let with = |f: &dyn Fn(&mut Part)| {
        let mut m = write_message();
        if let StoreBody::Write { part, .. } = &mut m.body {
            f(part);
        }
        StoreMessage::from_bytes(&m.to_bytes())
    };
    assert!(with(&|_| {}).is_ok());
    assert!(with(&|p| p.parts = 0).is_err());
    assert!(with(&|p| p.parts = 281).is_err());
    assert!(with(&|p| p.number = 3).is_err());
    assert!(with(&|p| p.bytes = vec![0; MAX_PART_LEN + 1]).is_err());
    assert!(with(&|p| p.bytes = vec![0; MAX_PART_LEN]).is_ok());
    assert!(with(&|p| p.bytes.clear()).is_err(), "an empty part of 3");
    assert!(with(&|p| {
        p.bytes.clear();
        p.parts = 1;
        p.number = 0;
    })
    .is_ok());
    // A read past the last possible part, and a list or read name.
    let read = |part: u32, name: &str| {
        StoreMessage::from_bytes(
            &StoreMessage {
                request_id: [0; 16],
                body: StoreBody::Read {
                    name: name.to_string(),
                    part,
                },
            }
            .to_bytes(),
        )
    };
    assert!(read(279, "n").is_ok());
    assert!(read(280, "n").is_err());
    assert!(read(0, "").is_err());
    assert!(read(0, "n n").is_err());
    // A part longer than the bytes that follow.
    let mut b = good.clone();
    b.truncate(b.len() - 1);
    assert!(StoreMessage::from_bytes(&b).is_err());
}

#[test]
fn lk_83_a_stored_with_anything_malformed_is_refused() {
    use Action::{List, Read, Remove, Write};
    let id = [1; 16];
    let build = |action: Action, body: StoredBody| {
        StoredMessage {
            request_id: id,
            action,
            body,
        }
        .to_bytes()
    };
    // Only the combinations LK-83 names.
    let part = || Part {
        number: 0,
        parts: 1,
        sha256: [0; 32],
        bytes: vec![1],
    };
    let bodies: Vec<(StoredBody, Vec<Action>)> = vec![
        (StoredBody::Taken { part: 0 }, vec![Write]),
        (StoredBody::Written { sha256: [0; 32] }, vec![Write]),
        (StoredBody::Part(part()), vec![Read]),
        (StoredBody::None, vec![Read]),
        (
            StoredBody::Names {
                names: vec![],
                more: false,
            },
            vec![List],
        ),
        (StoredBody::Removed, vec![Remove]),
        (
            StoredBody::Refused(StoreCause::StoreFull),
            vec![Write, Read, List, Remove],
        ),
    ];
    for (body, ok_for) in bodies {
        for action in [Write, Read, List, Remove] {
            let r = StoredMessage::from_bytes(&build(action, body.clone()));
            assert_eq!(r.is_ok(), ok_for.contains(&action), "{body:?} {action:?}");
        }
    }
    // A result or cause that is not a choice.
    let good = build(Write, StoredBody::Taken { part: 1 });
    let mut b = good.clone();
    b[4 + 1 + 16 + 1] = 0;
    assert!(StoredMessage::from_bytes(&b).is_err());
    b[4 + 1 + 16 + 1] = 8;
    assert!(StoredMessage::from_bytes(&b).is_err());
    let mut b = build(Write, StoredBody::Refused(StoreCause::Storage));
    *b.last_mut().unwrap() = 0;
    assert!(StoredMessage::from_bytes(&b).is_err());
    *b.last_mut().unwrap() = 9;
    assert!(StoredMessage::from_bytes(&b).is_err());
    // Names out of order, repeated, not names, and past the byte limit.
    let names = |names: Vec<&str>| {
        StoredMessage::from_bytes(&build(
            List,
            StoredBody::Names {
                names: names.into_iter().map(String::from).collect(),
                more: false,
            },
        ))
    };
    assert!(names(vec!["a", "b"]).is_ok());
    assert!(names(vec!["b", "a"]).is_err());
    assert!(names(vec!["a", "a"]).is_err());
    assert!(names(vec!["a b"]).is_err());
    let long: Vec<String> = (0..300)
        .map(|i| format!("{i:03}{}", "n".repeat(250)))
        .collect();
    let refs: Vec<&str> = long.iter().map(String::as_str).collect();
    assert!(
        names(refs[..200].to_vec()).is_ok(),
        "50,000 bytes of names fit"
    );
    assert!(names(refs).is_err(), "75,000 bytes of names do not");
    // A part with the numbers LK-83 refuses.
    let bad_part = |p: Part| StoredMessage::from_bytes(&build(Read, StoredBody::Part(p)));
    let ok_part = Part {
        number: 1,
        parts: 2,
        sha256: [0; 32],
        bytes: vec![0; 5],
    };
    assert!(bad_part(ok_part.clone()).is_ok());
    assert!(bad_part(Part {
        number: 2,
        ..ok_part.clone()
    })
    .is_err());
    assert!(bad_part(Part {
        parts: 281,
        ..ok_part.clone()
    })
    .is_err());
    assert!(bad_part(Part {
        bytes: vec![0; MAX_PART_LEN + 1],
        ..ok_part
    })
    .is_err());
    // A flag that is not 0 or 1.
    let mut b = build(
        List,
        StoredBody::Names {
            names: vec![],
            more: true,
        },
    );
    *b.last_mut().unwrap() = 2;
    assert!(StoredMessage::from_bytes(&b).is_err());
}

#[test]
fn lk_83_the_content_of_a_full_part_fits_one_sealed_message() {
    let m = store_messages()[2].clone();
    let bytes = m.to_bytes();
    assert!(bytes.len() <= crate::seal::MAX_CONTENT_LEN);
    let s = sealed(
        &counter(),
        Role::Counter,
        the_box().device_id(),
        Role::Box,
        1,
        Kind::Store,
        &bytes,
    );
    let o = open(&the_box(), Role::Box, &s, allowed, none_taken).unwrap();
    assert_eq!(StoreMessage::from_bytes(&o.content).unwrap(), m);
}

// ---- LK-84 ---------------------------------------------------------------

fn report() -> LightningReport {
    LightningReport {
        balance_sats: 123_456,
        receive_room_sats: 4_000_000,
        opening_fees_paid_sats: 2_500,
        last_stored_at: 1_700_000_000,
        last_sweep_at: 0,
        not_offered: NotOffered::SecondDeviceAway,
    }
}

#[test]
fn lk_84_the_report_round_trips_and_decodes_strictly() {
    for cause in [
        NotOffered::Offered,
        NotOffered::SecondDeviceAway,
        NotOffered::ChainBehind,
        NotOffered::NoUsableChannel,
        NotOffered::ProviderSilent,
    ] {
        let r = LightningReport {
            not_offered: cause,
            ..report()
        };
        let bytes = r.to_bytes();
        assert_eq!(LightningReport::from_bytes(&bytes).unwrap(), r);
        strict(
            &bytes,
            LightningReport::from_bytes,
            LightningReport::to_bytes,
        );
        assert_eq!(NotOffered::from_code(cause.to_code()), Some(cause));
    }
}

#[test]
fn lk_84_the_report_is_five_u64_and_a_u16_after_the_frame() {
    let bytes = report().to_bytes();
    assert_eq!(bytes.len(), 4 + 1 + 5 * 8 + 2);
    assert_eq!(&bytes[..4], b"OBM\x1f");
    assert_eq!(&bytes[5..13], &123_456u64.to_le_bytes());
    assert_eq!(&bytes[13..21], &4_000_000u64.to_le_bytes());
    assert_eq!(&bytes[21..29], &2_500u64.to_le_bytes());
    assert_eq!(&bytes[29..37], &1_700_000_000u64.to_le_bytes());
    assert_eq!(&bytes[37..45], &0u64.to_le_bytes());
    assert_eq!(&bytes[45..], &1u16.to_le_bytes());
}

#[test]
fn lk_84_a_cause_that_is_not_a_choice_and_another_magic_are_refused() {
    let mut b = report().to_bytes();
    let n = b.len();
    b[n - 2..].copy_from_slice(&99u16.to_le_bytes());
    assert!(LightningReport::from_bytes(&b).is_err());
    assert_eq!(NotOffered::from_code(5), None);
    let mut b = report().to_bytes();
    b[3] = b'Z';
    assert!(LightningReport::from_bytes(&b).is_err());
    // A Store's bytes are no report and a report's are no Store.
    assert!(LightningReport::from_bytes(&write_message().to_bytes()).is_err());
    assert!(StoreMessage::from_bytes(&report().to_bytes()).is_err());
    assert!(StoredMessage::from_bytes(&report().to_bytes()).is_err());
}

#[test]
fn lk_84_a_report_is_no_record_and_travels_in_the_main_count() {
    // It is sent counter to owner like a check-in, on the ordinary numbers.
    let ok = |_: &[u8]| -> Result<(), ()> { Ok(()) };
    let mut main = Inbox::default();
    let s = sealed(
        &counter(),
        Role::Counter,
        owner().device_id(),
        Role::Owner,
        1,
        Kind::LightningReport,
        &report().to_bytes(),
    );
    let o = main.open(&owner(), Role::Owner, &s, allowed, ok).unwrap();
    assert_eq!(LightningReport::from_bytes(&o.content).unwrap(), report());
    assert!(!Kind::LightningReport.counts_apart());
    assert!(Kind::Store.counts_apart() && Kind::Stored.counts_apart());
}
