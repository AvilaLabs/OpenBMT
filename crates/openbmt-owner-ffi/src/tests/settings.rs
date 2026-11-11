//! The preview of a change of the shop's settings through `dispatch`
//! (`docs/SESSION-RULES.md`, AS-74 to AS-77): the changes in words, the
//! bytes `queue_settings` takes, the refusals with their words, and that
//! the preview stores nothing.

// A refusal is a whole error response; the tests read it as it is.
#![allow(clippy::result_large_err)]

use openbmt_core::settings::{Settings, SignedSettings};

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{Fault, Rd, W};
use crate::settings_edit::{dollars, percent};

const KEY: &str = "0279BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798";

const KEEP: u8 = 0;
const REMOVE: u8 = 1;
const SET: u8 = 2;

/// An edit as the app writes it: every field absent unless set.
#[derive(Default)]
struct Edit {
    rate: Option<u64>,
    limits: [Option<u32>; 10],
    tolerance: Option<u32>,
    min_conf: Option<u8>,
    final_conf: Option<u8>,
    heartbeat: Option<u32>,
    tor: Option<bool>,
    wifi: Option<bool>,
    floor: Option<u64>,
    shares: Vec<(Vec<u8>, u32)>,
    lightning: u8,
    text: String,
}

// The ten limits in the order of the frame, with the tolerance after six.
const LIGHT_PER_SALE: usize = 0;
const REFUND_CAP: usize = 6;
const MANAGER: usize = 9;

fn opt32(w: W, v: Option<u32>) -> W {
    match v {
        None => w.u8(0),
        Some(v) => w.u8(1).u32(v),
    }
}

impl Edit {
    fn bytes(&self) -> Vec<u8> {
        let mut w = match self.rate {
            None => W::new().u8(0),
            Some(r) => W::new().u8(1).u64(r),
        };
        for l in &self.limits[..6] {
            w = opt32(w, *l);
        }
        w = opt32(w, self.tolerance);
        for l in &self.limits[6..] {
            w = opt32(w, *l);
        }
        w = match self.min_conf {
            None => w.u8(0),
            Some(v) => w.u8(1).u8(v),
        };
        w = match self.final_conf {
            None => w.u8(0),
            Some(v) => w.u8(1).u8(v),
        };
        w = opt32(w, self.heartbeat);
        for flag in [self.tor, self.wifi] {
            w = match flag {
                None => w.u8(0),
                Some(v) => w.u8(1).u8(u8::from(v)),
            };
        }
        w = match self.floor {
            None => w.u8(0),
            Some(v) => w.u8(1).u64(v),
        };
        w = w.u32(u32::try_from(self.shares.len()).expect("fits"));
        for (counter, bps) in &self.shares {
            w = w.bytes(counter).u32(*bps);
        }
        w.u8(self.lightning).string(&self.text).0
    }
}

#[derive(Debug)]
struct Change {
    field: String,
    label: String,
    counter: Option<Vec<u8>>,
    old: String,
    new: String,
    loosening: bool,
}

#[derive(Debug)]
struct Preview {
    version: u32,
    changes: Vec<Change>,
    needs_key_app: bool,
    nothing_changed: bool,
    total: u32,
    rate: Option<u64>,
    settings: Vec<u8>,
}

fn preview(rig: &Rig, handle: u64, edit: &Edit) -> Result<Preview, Fault> {
    let response = rig.call(47, handle, &edit.bytes());
    if let Some(f) = Rd::error(&response) {
        check_wire_words(&f.words).unwrap_or_else(|why| panic!("{why}: {f:?}"));
        return Err(f);
    }
    let mut r = Rd::ok(&response);
    let p = Preview {
        version: r.u32(),
        changes: r.list(|r| Change {
            field: r.string(),
            label: r.string(),
            counter: r.opt(Rd::bytes),
            old: r.string(),
            new: r.string(),
            loosening: r.bool(),
        }),
        needs_key_app: r.bool(),
        nothing_changed: r.bool(),
        total: r.u32(),
        rate: r.opt(Rd::u64),
        settings: r.bytes(),
    };
    r.done();
    Ok(p)
}

fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    (rig, handle)
}

/// A node whose settings in force name the test counter, at 1,000 satoshis
/// to a dollar.
fn held() -> (Rig, u64) {
    let (rig, handle) = ready();
    rig.queue_batch(handle);
    rig.fund(handle, 300_000);
    let mut settings = Settings::defaults(1_000);
    settings.devices.insert(counter());
    settings.counters.insert(counter(), 10_000);
    let signed = SignedSettings {
        owner_sig: settings.sign(&id(0x11)),
        approver_sig: None,
        settings,
    };
    Rd::ok(&rig.call(46, handle, &W::new().bytes(&signed.to_bytes()).0));
    let response = rig.call(30, handle, &W::new().u64(T0).u64(1_000).0);
    let mut r = Rd::ok(&response);
    let (_, request, _) = r.built();
    r.done();
    let mut app = key_app();
    let answer = answer(&mut app, &request, T0);
    Rd::ok(&rig.call(32, handle, &W::new().u64(T0).bytes(&answer).0)).taken();
    (rig, handle)
}

/// What `queue_settings` did: tightening, the entry and the signed bytes.
fn queue_settings(rig: &Rig, handle: u64, bytes: &[u8]) -> (bool, u32, Option<Vec<u8>>) {
    let response = rig.call(46, handle, &W::new().bytes(bytes).0);
    let mut r = Rd::ok(&response);
    let out = (r.bool(), r.u32(), r.opt(Rd::bytes));
    r.done();
    out
}

fn entries(rig: &Rig, handle: u64) -> usize {
    let response = rig.call(40, handle, &[]);
    let mut r = Rd::ok(&response);
    let q = r.queue();
    r.done();
    q.others.len()
}

#[test]
fn as_77_a_loosening_edit_is_shown_in_words_and_queued_with_the_bytes_of_the_preview() {
    let (rig, handle) = held();
    let mut edit = Edit::default();
    edit.limits[REFUND_CAP] = Some(150);
    edit.tor = Some(true);
    let p = preview(&rig, handle, &edit).expect("a preview");
    assert_eq!(p.version, 2);
    assert!(p.needs_key_app && !p.nothing_changed);
    assert_eq!(p.rate, Some(1_000));
    let lines: Vec<_> = p
        .changes
        .iter()
        .map(|c| {
            (
                c.field.as_str(),
                c.label.as_str(),
                c.old.as_str(),
                c.new.as_str(),
                c.loosening,
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            ("refund_cap", "Largest single refund", "$100", "$150", true),
            ("tor_on", "Tor", "off", "on", false),
        ]
    );
    // A preview stores nothing: the queue is as it was.
    assert_eq!(entries(&rig, handle), 0);
    // The bytes of the preview are what the queue takes.
    let (tightening, id, signed) = queue_settings(&rig, handle, &p.settings);
    assert!(!tightening && signed.is_none());
    assert_eq!(entries(&rig, handle), 1);
    assert!(id > 0);
}

#[test]
fn as_77_a_tightening_edit_is_signed_by_the_owner_alone_and_handed_back() {
    let (rig, handle) = held();
    let edit = Edit {
        tor: Some(true),
        floor: Some(5_000),
        heartbeat: Some(300),
        ..Edit::default()
    };
    let p = preview(&rig, handle, &edit).expect("a preview");
    assert!(!p.needs_key_app);
    let lines: Vec<_> = p
        .changes
        .iter()
        .map(|c| {
            (
                c.label.as_str(),
                c.old.as_str(),
                c.new.as_str(),
                c.loosening,
            )
        })
        .collect();
    assert_eq!(
        lines,
        [
            (
                "Longest wait for this phone's heartbeat",
                "10 minutes",
                "5 minutes",
                false
            ),
            ("Tor", "off", "on", false),
            ("Smallest bitcoin sale", "none", "5,000 sats", false),
        ]
    );
    assert_eq!(p.rate, None, "no limit changed, so no rate was used");
    let (tightening, _, signed) = queue_settings(&rig, handle, &p.settings);
    assert!(tightening && signed.is_some());
    // The next preview starts from the settings now in force.
    let again = preview(&rig, handle, &Edit::default()).expect("a preview");
    assert_eq!(again.version, 3);
    assert!(again.nothing_changed && again.changes.is_empty());
}

#[test]
fn as_74_a_counters_share_is_a_line_of_its_own_with_the_total() {
    let (rig, handle) = held();
    let edit = Edit {
        shares: vec![(counter_bytes(), 3_333)],
        ..Edit::default()
    };
    let p = preview(&rig, handle, &edit).expect("a preview");
    assert_eq!(p.total, 3_333);
    assert_eq!(p.changes.len(), 1);
    let c = &p.changes[0];
    assert_eq!(c.counter.as_deref(), Some(counter_bytes().as_slice()));
    assert_eq!((c.old.as_str(), c.new.as_str()), ("100%", "33.33%"));
    assert!(!c.loosening && !p.needs_key_app);
    // More than the whole is refused by the settings rules.
    let over = Edit {
        shares: vec![(counter_bytes(), 10_001)],
        ..Edit::default()
    };
    let f = preview(&rig, handle, &over).expect_err("refused");
    assert_eq!(f.status, 1, "{f:?}");
    assert_eq!(f.words.code.as_deref(), Some("AS-59"));
    // A key that is not a counter, a key of the wrong length and a share no
    // counter can hold are refused, each with its own rule.
    let stranger = Edit {
        shares: vec![(id(0x44).device_id().to_bytes().to_vec(), 100)],
        ..Edit::default()
    };
    let f = preview(&rig, handle, &stranger).expect_err("refused");
    assert_eq!(f.words.code.as_deref(), Some("AS-59"));
    let short = Edit {
        shares: vec![(vec![1; 5], 100)],
        ..Edit::default()
    };
    assert_eq!(preview(&rig, handle, &short).unwrap_err().rule, "id-length");
    let huge = Edit {
        shares: vec![(counter_bytes(), 65_536)],
        ..Edit::default()
    };
    assert_eq!(
        preview(&rig, handle, &huge).unwrap_err().rule,
        "share-range"
    );
}

#[test]
fn as_76_the_node_is_named_by_key_or_address_and_shown_in_full() {
    let (rig, handle) = held();
    let edit = Edit {
        lightning: SET,
        text: format!("  {}@203.0.113.7:9735", KEY.to_lowercase()),
        ..Edit::default()
    };
    let p = preview(&rig, handle, &edit).expect("a preview");
    assert!(p.needs_key_app);
    let c = &p.changes[0];
    assert_eq!(
        (c.field.as_str(), c.label.as_str()),
        ("lightning_node", "Lightning node")
    );
    assert_eq!(c.old, "none");
    assert_eq!(c.new, KEY.to_lowercase());
    assert_eq!(c.new.len(), 66);
    assert!(c.loosening);
    // Nothing to remove in the settings held.
    let remove = Edit {
        lightning: REMOVE,
        ..Edit::default()
    };
    assert!(preview(&rig, handle, &remove).unwrap().nothing_changed);
}

#[test]
fn as_76_each_refusal_of_the_node_text_has_its_own_words() {
    let (rig, handle) = held();
    let mut headings = Vec::new();
    for text in [
        "my node".to_string(),
        KEY[..20].to_string(),
        format!("04{}", &KEY[2..]),
        format!("02{}", "00".repeat(32)),
        format!("{KEY}@host"),
    ] {
        let edit = Edit {
            lightning: SET,
            text: text.clone(),
            ..Edit::default()
        };
        let f = preview(&rig, handle, &edit).expect_err(&text);
        assert_eq!(
            (f.status, f.words.code.as_deref()),
            (1, Some("AS-76")),
            "{text}"
        );
        assert_eq!(f.rule, "AS-76");
        headings.push(f.words.heading);
    }
    headings.sort();
    headings.dedup();
    assert_eq!(headings.len(), 5, "{headings:?}");
    // The text is read only when the action is Set.
    let keep = Edit {
        lightning: KEEP,
        text: "not read".to_string(),
        ..Edit::default()
    };
    assert!(preview(&rig, handle, &keep).unwrap().nothing_changed);
}

#[test]
fn as_74_a_node_with_no_settings_has_nothing_to_change() {
    let (rig, handle) = ready();
    let edit = Edit {
        tor: Some(true),
        ..Edit::default()
    };
    let f = preview(&rig, handle, &edit).expect_err("refused");
    assert_eq!((f.status, f.rule.as_str()), (1, "AS-74"));
    assert_eq!(f.words.code.as_deref(), Some("AS-74"));
}

#[test]
fn as_74_values_the_settings_rules_refuse_come_back_with_their_words() {
    let (rig, handle) = held();
    for edit in [
        Edit {
            min_conf: Some(0),
            ..Edit::default()
        },
        Edit {
            final_conf: Some(101),
            ..Edit::default()
        },
        Edit {
            tolerance: Some(501),
            ..Edit::default()
        },
        Edit {
            heartbeat: Some(30),
            ..Edit::default()
        },
        Edit {
            min_conf: Some(9),
            ..Edit::default()
        },
    ] {
        let f = preview(&rig, handle, &edit).expect_err("refused");
        assert_eq!((f.status, f.rule.as_str()), (1, "AS-59"), "{f:?}");
    }
    // A limit at an explicit rate and a flag: the other limits stay as they
    // are.
    let mut edit = Edit {
        rate: Some(2_000),
        wifi: Some(false),
        ..Edit::default()
    };
    edit.limits[LIGHT_PER_SALE] = Some(12);
    edit.limits[MANAGER] = Some(100);
    let p = preview(&rig, handle, &edit).expect("a preview");
    assert_eq!(p.rate, Some(2_000));
    let fields: Vec<_> = p.changes.iter().map(|c| c.field.as_str()).collect();
    assert_eq!(fields, ["light_per_sale", "customer_wifi"]);
    assert_eq!(p.changes[1].new, "no");
}

#[test]
fn as_77_a_bad_frame_is_refused_and_nothing_is_stored() {
    let (rig, handle) = held();
    // A flag that is not 0 or 1, and bytes left over.
    let f = Rd::error(&rig.call(47, handle, &[2])).expect("an error");
    assert_eq!(f.status, 1);
    let mut bytes = Edit::default().bytes();
    bytes.push(0);
    assert!(Rd::error(&rig.call(47, handle, &bytes)).is_some());
    let bad_action = Edit {
        lightning: 3,
        ..Edit::default()
    };
    assert!(Rd::error(&rig.call(47, handle, &bad_action.bytes())).is_some());
    assert_eq!(entries(&rig, handle), 0);
}

#[test]
fn the_words_of_a_value_are_the_ones_a_person_reads() {
    assert_eq!(dollars(0), "off");
    assert_eq!(dollars(25), "$25");
    assert_eq!(dollars(1_250), "$1,250");
    assert_eq!(dollars(1_000_000), "$1,000,000");
    for (bps, text) in [
        (0, "0%"),
        (1, "0.01%"),
        (10, "0.1%"),
        (50, "0.5%"),
        (100, "1%"),
        (125, "1.25%"),
        (3_333, "33.33%"),
        (10_000, "100%"),
    ] {
        assert_eq!(percent(bps), text);
    }
}
