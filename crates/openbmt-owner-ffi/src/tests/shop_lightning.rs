//! The ops of Shop Lightning on the owner phone through `dispatch` (ops 236
//! and 237, `docs/SHOP-LIGHTNING-RULES.md` SL-45, SL-73, SL-76 to SL-78,
//! `docs/APPROVAL-RULES.md` AP-13): what the page shows, the preview of
//! turning it on, changing it and turning it off with its class, and that
//! the preview stores nothing.

// A refusal is a whole error response; the tests read it as it is.
#![allow(clippy::result_large_err)]

use std::collections::BTreeSet;

use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_link::shopln::{LightningReport, NotOffered};
use openbmt_owner_node::TakenReport;

use super::kit::*;
use super::wire::{Fault, Rd, W};
use crate::shop_lightning;

const NODE: &str = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const PROVIDER: &str = "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

const SET: u8 = 0;
const CHANGE: u8 = 1;
const REMOVE: u8 = 2;

fn boxed() -> openbmt_core::identity::DeviceId {
    id(0x61).device_id()
}

/// A node whose settings in force name the test counter and, when
/// `with_box`, a Box.
fn held(with_box: bool) -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    rig.queue_batch(handle);
    rig.fund(handle, 300_000);
    let mut settings = Settings::defaults(1_000);
    settings.devices.insert(counter());
    settings.counters.insert(counter(), 10_000);
    if with_box {
        settings.devices.insert(boxed());
        settings.boxes.insert(boxed());
    }
    let signed = SignedSettings {
        owner_sig: settings.sign(&id(0x11)),
        approver_sig: None,
        settings,
    };
    Rd::ok(&rig.call(46, handle, &W::new().bytes(&signed.to_bytes()).0));
    approve(&rig, handle, 1);
    (rig, handle)
}

/// The Key app approves what is queued.
fn approve(rig: &Rig, handle: u64, round: u64) {
    let now = T0 + round;
    let response = rig.call(30, handle, &W::new().u64(now).u64(1_000).0);
    let mut r = Rd::ok(&response);
    let (_, request, _) = r.built();
    r.done();
    let mut app = key_app();
    let answer = answer(&mut app, &request, now);
    Rd::ok(&rig.call(32, handle, &W::new().u64(now).bytes(&answer).0)).taken();
}

/// An edit as the app writes it.
struct Edit {
    action: u8,
    counter: Option<Vec<u8>>,
    node_id: Option<String>,
    second_device: Option<Vec<u8>>,
    provider: Option<(String, String)>,
    cap: Option<u64>,
    fee: Option<u64>,
}

impl Edit {
    fn none(action: u8) -> Self {
        Self {
            action,
            counter: None,
            node_id: None,
            second_device: None,
            provider: None,
            cap: None,
            fee: None,
        }
    }

    fn set() -> Self {
        Self {
            counter: Some(counter_bytes()),
            node_id: Some(NODE.to_string()),
            provider: Some((PROVIDER.to_string(), "203.0.113.7:9735".to_string())),
            cap: Some(5_000_000),
            fee: Some(20_000),
            ..Self::none(SET)
        }
    }

    fn bytes(&self) -> Vec<u8> {
        let w = W::new()
            .u8(self.action)
            .opt_bytes(self.counter.as_deref())
            .opt_string(self.node_id.as_deref())
            .opt_bytes(self.second_device.as_deref());
        let w = match &self.provider {
            Some((key, addr)) => w.opt_string(Some(key)).opt_string(Some(addr)),
            None => w.opt_string(None).opt_string(None),
        };
        let w = match self.cap {
            Some(v) => w.u8(1).u64(v),
            None => w.u8(0),
        };
        match self.fee {
            Some(v) => w.u8(1).u64(v),
            None => w.u8(0),
        }
        .0
    }
}

#[derive(Debug)]
struct Held {
    counter: Vec<u8>,
    node_id: Vec<u8>,
    second_device: Vec<u8>,
    provider_node_id: Vec<u8>,
    provider_addr: String,
    cap: u64,
    fee: u64,
}

fn read_held(r: &mut Rd) -> Held {
    Held {
        counter: r.bytes(),
        node_id: r.bytes(),
        second_device: r.bytes(),
        provider_node_id: r.bytes(),
        provider_addr: r.string(),
        cap: r.u64(),
        fee: r.u64(),
    }
}

#[derive(Debug)]
struct Page {
    on: bool,
    own_wallet: bool,
    counters: Vec<Vec<u8>>,
    boxes: Vec<Vec<u8>>,
    held: Option<Held>,
    report: Option<[u64; 6]>,
}

fn page(rig: &Rig, handle: u64) -> Page {
    let response = rig.call(236, handle, &[]);
    let mut r = Rd::ok(&response);
    let p = Page {
        on: r.bool(),
        own_wallet: r.bool(),
        counters: r.list(Rd::bytes),
        boxes: r.list(Rd::bytes),
        held: r.opt(read_held),
        report: r.opt(|r| {
            let out = [r.u64(), r.u64(), r.u64(), r.u64(), r.u64(), r.u64()];
            let _cause = r.u8();
            out
        }),
    };
    r.done();
    p
}

#[derive(Debug)]
struct Preview {
    version: u32,
    class: u8,
    needs_key_app: bool,
    nothing_changed: bool,
    after: Option<Held>,
    settings: Vec<u8>,
}

fn preview(rig: &Rig, handle: u64, edit: &Edit) -> Result<Preview, Fault> {
    let response = rig.call(237, handle, &edit.bytes());
    if let Some(f) = Rd::error(&response) {
        return Err(f);
    }
    let mut r = Rd::ok(&response);
    let p = Preview {
        version: r.u32(),
        class: r.u8(),
        needs_key_app: r.bool(),
        nothing_changed: r.bool(),
        after: r.opt(read_held),
        settings: r.bytes(),
    };
    r.done();
    Ok(p)
}

fn queue(rig: &Rig, handle: u64, bytes: &[u8]) -> bool {
    let response = rig.call(46, handle, &W::new().bytes(bytes).0);
    let mut r = Rd::ok(&response);
    let tightening = r.bool();
    let _ = (r.u32(), r.opt(Rd::bytes));
    r.done();
    tightening
}

const FREE: u8 = 0;
const LOOSEN: u8 = 1;
const REDIRECT: u8 = 2;

#[test]
fn sl_45_the_page_of_a_shop_with_it_off_names_the_counters_and_the_boxes() {
    let (rig, handle) = held(false);
    let p = page(&rig, handle);
    assert!(!p.on && !p.own_wallet);
    assert_eq!(p.counters, vec![counter_bytes()]);
    assert!(p.boxes.is_empty() && p.held.is_none() && p.report.is_none());
    let (rig, handle) = held(true);
    let p = page(&rig, handle);
    assert_eq!(p.boxes, vec![boxed().to_bytes().to_vec()]);
}

#[test]
fn sl_73_a_shop_without_a_box_is_refused_with_the_placeholder_of_its_rule() {
    let (rig, handle) = held(false);
    let f = preview(&rig, handle, &Edit::set()).unwrap_err();
    assert!(f.rule.starts_with("SL-73"), "{f:?}");
    assert_eq!(f.words.heading, "WORDS_NEEDED_SL73_NO_BOX_HEADING");
    assert_eq!(f.words.what, "WORDS_NEEDED_SL73_NO_BOX_WHAT");
    assert_eq!(f.words.next, "WORDS_NEEDED_SL73_NO_BOX_NEXT");
}

#[test]
fn sl_76_turning_it_on_is_a_redirect_that_is_queued_for_the_key_app_and_then_in_force() {
    let (rig, handle) = held(true);
    let stores = rig.calls().len();
    let p = preview(&rig, handle, &Edit::set()).expect("a preview");
    assert_eq!((p.class, p.needs_key_app), (REDIRECT, true));
    assert!(!p.nothing_changed);
    assert_eq!(p.version, 2);
    let after = p.after.as_ref().expect("on after");
    assert_eq!(after.counter, counter_bytes());
    assert_eq!(after.second_device, boxed().to_bytes().to_vec());
    assert_eq!(after.provider_addr, "203.0.113.7:9735");
    assert_eq!((after.cap, after.fee), (5_000_000, 20_000));
    // The preview stored nothing.
    assert!(!page(&rig, handle).on);
    assert_eq!(rig.calls().len(), stores);
    assert!(!queue(&rig, handle, &p.settings));
    approve(&rig, handle, 2);
    let on = page(&rig, handle);
    assert!(on.on);
    let held = on.held.expect("held");
    assert_eq!(held.node_id.len(), 33);
    assert_eq!(held.provider_node_id.len(), 33);
    assert_eq!(held.cap, 5_000_000);
}

#[test]
fn ap_13_the_cap_and_the_fee_loosen_when_raised_and_are_free_when_lowered() {
    let (rig, handle) = held(true);
    let p = preview(&rig, handle, &Edit::set()).unwrap();
    queue(&rig, handle, &p.settings);
    approve(&rig, handle, 2);
    for (cap, fee, class) in [
        (Some(6_000_000), None, LOOSEN),
        (None, Some(30_000), LOOSEN),
        (Some(4_000_000), None, FREE),
        (None, Some(10_000), FREE),
    ] {
        let edit = Edit {
            cap,
            fee,
            ..Edit::none(CHANGE)
        };
        let p = preview(&rig, handle, &edit).unwrap();
        assert_eq!(p.class, class, "{cap:?} {fee:?}");
        assert_eq!(p.needs_key_app, class != FREE);
    }
    // The provider changes the settings and loosens.
    let edit = Edit {
        provider: Some((NODE.to_string(), "198.51.100.2:9735".to_string())),
        ..Edit::none(CHANGE)
    };
    let p = preview(&rig, handle, &edit).unwrap();
    assert_eq!(p.class, LOOSEN);
    assert_eq!(p.after.unwrap().provider_addr, "198.51.100.2:9735");
}

#[test]
fn sl_77_turning_it_off_is_free_and_signed_by_the_owner_alone() {
    let (rig, handle) = held(true);
    let on = preview(&rig, handle, &Edit::set()).unwrap();
    queue(&rig, handle, &on.settings);
    approve(&rig, handle, 2);
    let off = preview(&rig, handle, &Edit::none(REMOVE)).unwrap();
    assert_eq!((off.class, off.needs_key_app), (FREE, false));
    assert!(off.after.is_none() && !off.nothing_changed);
    assert!(queue(&rig, handle, &off.settings));
    assert!(!page(&rig, handle).on);
}

#[test]
fn sl_71_an_edit_that_is_missing_what_its_step_needs_is_refused() {
    let (rig, handle) = held(true);
    for edit in [
        Edit {
            counter: None,
            ..Edit::set()
        },
        Edit {
            node_id: None,
            ..Edit::set()
        },
        Edit {
            provider: None,
            ..Edit::set()
        },
        Edit {
            cap: None,
            ..Edit::set()
        },
        Edit {
            fee: None,
            ..Edit::set()
        },
        Edit {
            counter: Some(vec![1, 2, 3]),
            ..Edit::set()
        },
    ] {
        let f = preview(&rig, handle, &edit).unwrap_err();
        assert!(
            f.rule == "request-malformed" || f.rule == "id-length",
            "{f:?}"
        );
    }
    let f = preview(
        &rig,
        handle,
        &Edit {
            node_id: Some(NODE.to_ascii_uppercase()),
            ..Edit::set()
        },
    )
    .unwrap_err();
    assert!(f.rule.starts_with("SL-76"), "{f:?}");
    assert_eq!(f.words.heading, "WORDS_NEEDED_SL76_NODE_ID_HEADING");
}

#[test]
fn sl_78_the_view_gives_the_newest_report_with_its_time_and_cause() {
    let mut settings = Settings::defaults(1_000);
    settings.devices.insert(counter());
    settings.counters.insert(counter(), 10_000);
    let report = TakenReport {
        from: counter(),
        taken_at: 1_234,
        report: LightningReport {
            balance_sats: 11,
            receive_room_sats: 22,
            opening_fees_paid_sats: 33,
            last_stored_at: 44,
            last_sweep_at: 55,
            not_offered: NotOffered::ChainBehind,
        },
    };
    let view = shop_lightning::view(false, Some(&settings), Some(&report));
    let r = view.report.expect("a report");
    assert_eq!(
        [
            r.taken_at,
            r.balance_sats,
            r.receive_room_sats,
            r.opening_fees_paid_sats,
            r.last_stored_at,
            r.last_sweep_at
        ],
        [1_234, 11, 22, 33, 44, 55]
    );
    assert_eq!(r.not_offered, shop_lightning::NotOfferedView::ChainBehind);
    assert!(!view.on && view.held.is_none());
    let names: BTreeSet<_> = view.counters.iter().collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn the_request_of_a_turn_on_is_the_bytes_the_kotlin_harness_writes() {
    use crate::codec::Put;
    use crate::shop_lightning::{ShopLightningAction, ShopLightningEditArgs};
    let args = ShopLightningEditArgs {
        action: ShopLightningAction::Set,
        counter: Some(vec![7; 32]),
        node_id: Some("ab".to_string()),
        second_device: None,
        provider_node_id: Some("cd".to_string()),
        provider_addr: Some("1.2.3.4:5".to_string()),
        cap_sats: Some(5),
        max_opening_fee_sats: Some(6),
    };
    let mut out = Vec::new();
    args.put(&mut out);
    let hex: String = out.iter().map(|b| format!("{b:02x}")).collect();
    let expected = format!(
        "00{}{}00{}{}{}{}",
        format_args!("0120000000{}", "07".repeat(32)),
        "01020000006162",
        "01020000006364",
        "0109000000312e322e332e343a35",
        "010500000000000000",
        "010600000000000000",
    );
    assert_eq!(hex, expected);
    // The edit of Edit::bytes is the same layout.
    assert_eq!(
        Edit {
            counter: Some(vec![7; 32]),
            node_id: Some("ab".to_string()),
            provider: Some(("cd".to_string(), "1.2.3.4:5".to_string())),
            cap: Some(5),
            fee: Some(6),
            ..Edit::none(SET)
        }
        .bytes(),
        out
    );
    let mut remove = Vec::new();
    ShopLightningEditArgs {
        action: ShopLightningAction::Remove,
        counter: None,
        node_id: None,
        second_device: None,
        provider_node_id: None,
        provider_addr: None,
        cap_sats: None,
        max_opening_fee_sats: None,
    }
    .put(&mut remove);
    assert_eq!(remove, [vec![2], vec![0; 7]].concat());
}
