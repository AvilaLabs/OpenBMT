//! Every op through `dispatch`, on a node with drivers the test feeds by
//! hand.

use openbmt_core::settings::{Settings, SignedSettings};

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{Fault, Rd, W};
use crate::{LinkConfig, RouteView};

/// A rig with a started node that has heard from the network.
fn ready() -> (Rig, u64) {
    let rig = Rig::new();
    let handle = rig.start();
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    (rig, handle)
}

fn fault(response: &[u8]) -> Fault {
    let f = Rd::error(response).expect("an error response");
    check_wire_words(&f.words).unwrap_or_else(|why| panic!("{why}: {f:?}"));
    f
}

fn unit(response: &[u8]) {
    Rd::ok(response).done();
}

fn queue(rig: &Rig, handle: u64) -> super::wire::Queue {
    let response = rig.call(40, handle, &[]);
    let mut r = Rd::ok(&response);
    let q = r.queue();
    r.done();
    q
}

fn dispatch_start(rig: &Rig, dir: &std::path::Path) -> Vec<u8> {
    crate::dispatch(rig.registry(), &start_request(dir))
}

#[test]
fn start_and_stop_hold_the_handles_and_the_limit() {
    let rig = Rig::new();
    let first = rig.start();
    assert_eq!(first, 1);
    // A second node on another folder; a third is over the limit.
    let other = Scratch::new();
    rig.arm();
    let second = dispatch_start(&rig, other.path());
    assert_eq!(Rd::ok(&second).u64(), 2);
    let third_dir = Scratch::new();
    rig.arm();
    let third = fault(&dispatch_start(&rig, third_dir.path()));
    assert_eq!((third.status, third.rule.as_str()), (3, "node-limit"));
    // Stop makes room, and the handle is gone.
    Rd::ok(&rig.call(2, first, &[])).pump();
    assert_eq!(fault(&rig.call(2, first, &[])).rule, "handle-unknown");
    rig.arm();
    let again = dispatch_start(&rig, third_dir.path());
    assert_eq!(Rd::ok(&again).u64(), 3);
}

#[test]
fn the_same_folder_twice_is_refused_by_the_folders_lock() {
    let rig = Rig::new();
    let first = rig.start();
    rig.arm();
    let same = fault(&dispatch_start(&rig, &rig.dir()));
    assert_eq!((same.status, same.rule.as_str()), (1, "NR-79"), "{same:?}");
    // Stopping lets go of the lock.
    Rd::ok(&rig.call(2, first, &[])).pump();
    rig.arm();
    assert_eq!(Rd::ok(&dispatch_start(&rig, &rig.dir())).u64(), 2);
}

#[test]
fn start_refuses_what_the_setup_does_not_hold() {
    let rig = Rig::new();
    let dir = rig.dir();
    let base = start_config(dir.to_str().expect("UTF-8"));
    let mut cases: Vec<(&str, crate::StartConfig)> = Vec::new();
    let mut c = base.clone();
    c.dir = String::new();
    cases.push(("start-dir", c));
    let mut c = base.clone();
    c.owner_secret = vec![0x11; 31];
    cases.push(("start-secret", c));
    let mut c = base.clone();
    c.owner_secret = vec![0; 32];
    cases.push(("start-secret", c));
    let mut c = base.clone();
    c.approver_root = vec![1; 5];
    cases.push(("start-roots", c));
    let mut c = base.clone();
    c.wallet = "not a wallet".to_string();
    cases.push(("start-wallet", c));
    let mut c = base.clone();
    c.key1_account = vec![1; 77];
    cases.push(("start-key1", c));
    let mut c = base.clone();
    c.key3_xpub = Some("not a key".to_string());
    cases.push(("start-key1", c));
    let mut c = base.clone();
    c.proxy = Some("not an address".to_string());
    cases.push(("start-proxy", c));
    let mut c = base.clone();
    c.broadcast_proxy = Some("not an address".to_string());
    cases.push(("start-proxy", c));
    let mut c = base.clone();
    c.fixed_peers = vec!["127.0.0.1:8333".to_string(), "nope".to_string()];
    cases.push(("start-peer", c));
    let link = |route: RouteView, relay: Vec<u8>, counters: Vec<Vec<u8>>| LinkConfig {
        route,
        relay_key: relay,
        counters,
        heartbeat_every_secs: 0,
        check_in_grace_secs: 0,
    };
    let direct = RouteView::Direct {
        addr: "127.0.0.1:9".to_string(),
    };
    let relay = id(0x41).device_id().to_bytes().to_vec();
    for l in [
        link(
            RouteView::Direct {
                addr: "nope".to_string(),
            },
            relay.clone(),
            vec![counter_bytes()],
        ),
        link(direct.clone(), vec![1; 5], vec![counter_bytes()]),
        link(direct, relay, vec![vec![1; 5]]),
    ] {
        let mut c = base.clone();
        c.link = Some(l);
        cases.push(("start-link", c));
    }
    for (rule, config) in cases {
        rig.arm();
        let response = rig.call(1, 0, &W::new().start(&config).0);
        let f = fault(&response);
        assert_eq!((f.status, f.rule.as_str()), (1, rule), "{f:?}");
    }
    // None of that left a node or a lock behind.
    rig.arm();
    assert_eq!(rig.start(), 1);
}

#[test]
fn status_and_notes_answer_before_and_after_the_network() {
    let rig = Rig::new();
    let handle = rig.start();
    let status = |rig: &Rig| {
        let response = rig.call(20, handle, &[]);
        let mut r = Rd::ok(&response);
        let s = r.status();
        r.done();
        s
    };
    let before = status(&rig);
    assert!(!before.health.0, "no answer before the first peers event");
    assert!(!before.stopped);
    rig.send(vec![peers_event()]);
    assert_eq!(rig.pump(handle), (vec![], false));
    let after = status(&rig);
    assert!(after.health.0, "an answer");
    check_wire_words(&after.health.3).expect("words");

    let response = rig.call(60, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.list(Rd::string).is_empty());
    r.done();
    // A line the driver logs is shown by the pump, once.
    rig.send(vec![openbmt_net_io::AppEvent::Note("a line".to_string())]);
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    assert_eq!(r.pump(), (vec!["a line".to_string()], false));
    r.done();
    assert_eq!(rig.pump(handle), (vec![], false));
    // Every op that needs a node refuses a handle that is not one.
    for op in [
        2u16, 20, 21, 22, 23, 24, 26, 27, 33, 40, 50, 52, 60, 61, 62, 63,
    ] {
        assert_eq!(
            fault(&rig.call(op, 99, &[])).rule,
            "handle-unknown",
            "op {op}"
        );
    }
}

#[test]
fn a_batch_a_cash_out_and_the_queue() {
    let (rig, handle) = ready();
    // The wallet derives the batch; fewer addresses than the policy asks is
    // refused, and so is a counter that is not 32 bytes.
    let f = fault(&rig.call(
        45,
        handle,
        &W::new().u64(T0).bytes(&counter_bytes()).u32(0).u32(1).0,
    ));
    assert_eq!((f.status, f.rule.as_str()), (1, "batch-not-derived"));
    let f = fault(&rig.call(45, handle, &W::new().u64(T0).bytes(&[1; 5]).u32(0).u32(3).0));
    assert_eq!(f.rule, "id-length");
    let batch = rig.queue_batch(handle);
    let q = queue(&rig, handle);
    assert_eq!(
        (q.others.len(), q.others[0].0, q.others[0].1),
        (1, batch, 0)
    );

    // A cash-out: the address is checked here, for the network of the node.
    let cash = |address: &str, amount: &[u8]| {
        let mut w = W::new().string(address);
        w.0.extend_from_slice(amount);
        rig.call(44, handle, &w.string("till").u8(0).0)
    };
    let sats = [&[0u8][..], &5_000u64.to_le_bytes()].concat();
    for bad in [
        "not an address",
        "",
        // A mainnet address on a regtest node.
        "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
    ] {
        let f = fault(&cash(bad, &sats));
        assert_eq!(
            (f.status, f.rule.as_str()),
            (1, "address-invalid"),
            "{bad:?}"
        );
    }
    let id = Rd::ok(&cash(&external_address(8), &sats)).u32();
    let all = Rd::ok(&cash(&external_address(7), &[1])).u32();
    assert_ne!(id, all);
    let q = queue(&rig, handle);
    assert_eq!(q.cash_outs.len(), 2);
    assert_eq!(q.cash_outs[0].address, external_address(8));
    assert_eq!(q.cash_outs[0].amount, Some(5_000));
    assert_eq!(q.cash_outs[0].label, "till");
    assert_eq!(q.cash_outs[1].amount, None, "all");
    for row in &q.cash_outs {
        check_wire_words(&row.stage.words).expect("words");
    }

    // Cancel one; the other stays. A number nobody has is refused.
    unit(&rig.call(42, handle, &W::new().u32(id).0));
    assert_eq!(queue(&rig, handle).cash_outs.len(), 1);
    let f = fault(&rig.call(42, handle, &W::new().u32(id).0));
    assert_eq!(f.status, 1);
    // Only what was sent back is put in a session again.
    let f = fault(&rig.call(43, handle, &W::new().u32(all).0));
    assert_eq!(f.status, 1);
}

#[test]
fn a_refund_needs_the_wallets_own_facts() {
    let (rig, handle) = ready();
    let ask = |sale: u32, sats: u64| {
        rig.call(
            41,
            handle,
            &W::new()
                .refund(
                    T0,
                    &counter_bytes(),
                    sale,
                    "Mia",
                    &external_address(9),
                    sats,
                )
                .0,
        )
    };
    // No batch was signed, so no address is the counter's.
    let f = fault(&ask(0, 10_000));
    assert_eq!(f.rule, "OC-14", "{f:?}");
    rig.queue_batch(handle);
    // The address is the counter's but nothing has been checked yet, or
    // nothing was received: the wallet's view says so, the counter cannot.
    let f = fault(&ask(0, 10_000));
    assert!(f.status == 1 || f.status == 3, "{f:?}");
    rig.fund(handle, 300_000);
    // An address that is not an address, a counter that is not 32 bytes and
    // a sale beyond the batch are each refused.
    let f = fault(
        &rig.call(
            41,
            handle,
            &W::new()
                .refund(T0, &counter_bytes(), 0, "Mia", "nope", 10_000)
                .0,
        ),
    );
    assert_eq!(f.rule, "address-invalid");
    let f = fault(
        &rig.call(
            41,
            handle,
            &W::new()
                .refund(T0, &[1; 3], 0, "Mia", &external_address(9), 10_000)
                .0,
        ),
    );
    assert_eq!(f.rule, "id-length");
    let f = fault(&ask(3, 10_000));
    assert_eq!(f.rule, "OC-14");
    // More than the sale received is refused by the queue's own rule.
    let f = fault(&ask(0, 400_000));
    assert_eq!(f.status, 1, "{f:?}");
    assert!(queue(&rig, handle).refunds.is_empty());
    // And a refund that is asked rightly is queued.
    let response = ask(0, 10_000);
    let mut r = Rd::ok(&response);
    let (_, received, deep) = (r.u32(), r.u64(), r.bool());
    r.done();
    assert_eq!((received, deep), (300_000, true));
}

#[test]
fn the_receipts_line_the_merge_offer_and_the_sends_answer() {
    let (rig, handle) = ready();
    let response = rig.call(21, handle, &[]);
    let mut r = Rd::ok(&response);
    let (kind, payments, _, _, words) = r.receipts();
    r.done();
    assert_eq!(payments, 0);
    assert!(kind <= 3);
    check_wire_words(&words).expect("words");
    // No coins, no offer to merge them.
    let response = rig.call(22, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.opt(|_| ()).is_none());
    r.done();
    let response = rig.call(23, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.list(Rd::send).is_empty());
    r.done();
    // Nothing to approve yet: a refusal with words, not a fault.
    let f = fault(&rig.call(31, handle, &W::new().u64(T0).0));
    assert!(f.status == 1 || f.status == 3, "{f:?}");
    // An approval with nothing queued is either refused with words or has
    // no lines to carry.
    if let Some(f) = Rd::error(&rig.call(30, handle, &W::new().u64(T0).u64(1_000).0)) {
        check_wire_words(&f.words).expect("words");
    }
}

#[test]
fn sessions_are_listed_read_and_abandoned() {
    let (rig, handle) = ready();
    let response = rig.call(33, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.list(Rd::bytes).is_empty());
    r.done();
    let f = fault(&rig.call(34, handle, &W::new().bytes(&[7; 32]).0));
    assert_eq!((f.status, f.rule.as_str()), (1, "session-unknown"));
    let f = fault(&rig.call(35, handle, &W::new().bytes(&[7; 32]).0));
    assert_eq!(f.rule, "session-unknown");
    let f = fault(&rig.call(34, handle, &W::new().bytes(&[7; 5]).0));
    assert_eq!(f.rule, "id-length");
    // A response that is not the Key app's is refused with words.
    let f = fault(&rig.call(32, handle, &W::new().u64(T0).bytes(&[1, 2, 3]).0));
    assert!(f.status == 1 || f.status == 3, "{f:?}");
}

#[test]
fn settings_the_owner_signs_are_queued_and_a_repeat_is_refused() {
    let (rig, handle) = ready();
    let f = fault(&rig.call(46, handle, &W::new().bytes(&[1, 2, 3]).0));
    assert_eq!((f.status, f.rule.as_str()), (1, "settings-bytes"));
    let settings = Settings::defaults(1_000);
    let signed = SignedSettings {
        owner_sig: settings.sign(&id(0x11)),
        approver_sig: None,
        settings,
    };
    let response = rig.call(46, handle, &W::new().bytes(&signed.to_bytes()).0);
    let mut r = Rd::ok(&response);
    let (tightening, id, signed_bytes) = (r.bool(), r.u32(), r.opt(Rd::bytes));
    r.done();
    assert!(!tightening, "first settings are a loosening change");
    assert!(signed_bytes.is_none());
    let q = queue(&rig, handle);
    assert_eq!((q.others.len(), q.others[0].0, q.others[0].1), (1, id, 1));
    // The same version again is not newer.
    let f = fault(&rig.call(46, handle, &W::new().bytes(&signed.to_bytes()).0));
    assert_eq!(f.status, 1, "{f:?}");
}

/// What the test reads back of a `SettingsHeldView`.
struct Held {
    version: u32,
    /// The ten limits in the order the frame writes them, around the
    /// tolerance.
    limits: [(u64, u32); 10],
    tolerance: u32,
    min_conf: u8,
    final_conf: u8,
    tor: bool,
    counters: usize,
    boxes: usize,
    node: Option<Vec<u8>>,
    /// The tag of the shop's currency.
    currency: u8,
}

/// `SettingsHeldView`, field by field as the frame writes them.
fn read_settings(r: &mut Rd<'_>) -> Held {
    let version = r.u32();
    let mut limits = [(0, 0); 10];
    for slot in limits.iter_mut().take(6) {
        *slot = (r.u64(), r.u32());
    }
    let tolerance = r.u32();
    for slot in limits.iter_mut().skip(6) {
        *slot = (r.u64(), r.u32());
    }
    let (min_conf, final_conf) = (r.u8(), r.u8());
    let _heartbeat = r.u32();
    let tor = r.bool();
    let _wifi = r.bool();
    let _floor = r.u64();
    let _tips_on = r.bool();
    let _tip_percents = r.list(|r| r.u8());
    let devices = r.list(Rd::bytes);
    assert!(devices.iter().all(|d| d.len() == 32));
    let counters = r.list(|r| (r.bytes(), r.u32()));
    let boxes = r.list(Rd::bytes);
    let node = r.opt(Rd::bytes);
    let currency = r.u8();
    Held {
        version,
        limits,
        tolerance,
        min_conf,
        final_conf,
        tor,
        counters: counters.len(),
        boxes: boxes.len(),
        node,
        currency,
    }
}

#[test]
fn the_settings_the_node_holds_and_the_outbox_follow_an_approval() {
    let (rig, handle) = ready();
    // Nothing held and nothing waiting before an approval.
    let response = rig.call(26, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.opt(read_settings).is_none());
    r.done();
    let response = rig.call(27, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.list(|r| r.bytes()).is_empty());
    r.done();

    // First settings that name the counter, and a batch for it: both go to
    // the Key app and come back signed.
    rig.queue_batch(handle);
    rig.fund(handle, 300_000);
    let mut settings = Settings::defaults(1_000);
    settings.devices.insert(counter());
    settings.counters.insert(counter(), 10_000);
    let signed = SignedSettings {
        owner_sig: settings.sign(&id(0x11)),
        approver_sig: None,
        settings: settings.clone(),
    };
    unit_ok_settings(&rig, handle, &signed);
    let mut app = key_app();
    approve(&rig, handle, &mut app);

    let response = rig.call(26, handle, &[]);
    let mut r = Rd::ok(&response);
    let held = r.opt(read_settings).expect("settings are held");
    r.done();
    assert_eq!(held.version, settings.version);
    assert_eq!(
        held.limits[0],
        (
            settings.light_per_sale.sats,
            settings.light_per_sale.display_usd
        )
    );
    assert_eq!(held.limits[9].0, settings.manager_override_ceiling.sats);
    assert_eq!(
        (held.min_conf, held.final_conf),
        (settings.min_confirmations, settings.final_confirmations)
    );
    assert_eq!(held.tolerance, settings.underpayment_tolerance_bps);
    assert_eq!(held.tor, settings.tor_on);
    assert_eq!(
        (held.counters, held.boxes),
        (1, 0),
        "one counter share, no Box"
    );
    // No Lightning node is named, and the shop prices in dollars (CR-193).
    assert_eq!((held.node), None);
    assert_eq!(held.currency, 1);
    assert!(queue(&rig, handle).others.is_empty(), "both left the queue");

    let response = rig.call(27, handle, &[]);
    let mut r = Rd::ok(&response);
    let rows = r.list(|r| {
        (
            r.u32(),
            r.u8(),
            r.opt(Rd::string),
            r.opt(Rd::bytes),
            r.opt(Rd::u32),
            r.opt(Rd::u64),
            r.u32(),
            r.words(),
        )
    });
    r.done();
    let kinds: Vec<u8> = rows.iter().map(|row| row.1).collect();
    assert!(kinds.contains(&1), "a batch waits: {kinds:?}");
    for row in &rows {
        check_wire_words(&row.7).unwrap_or_else(|why| panic!("{why}: {:?}", row.7));
        assert!(row.6 > 0, "a size");
        match row.1 {
            1 => {
                assert_eq!(row.3.as_deref(), Some(counter_bytes().as_slice()));
                assert_eq!(row.5, Some(T0), "the batch's signing time");
                assert!(row.7.detail.is_none());
            }
            2 => {
                assert_eq!(row.4, Some(settings.version));
                assert_eq!(row.5, None, "no time is kept for settings");
            }
            _ => {}
        }
    }
}

/// Build an approval, let the Key app approve every line it can, take the
/// answer.
fn approve(rig: &Rig, handle: u64, app: &mut openbmt_approver::Approver) {
    let response = rig.call(30, handle, &W::new().u64(T0).u64(1_000).0);
    let mut r = Rd::ok(&response);
    let (_, request, _) = r.built();
    r.done();
    let answer = answer(app, &request, T0);
    let response = rig.call(32, handle, &W::new().u64(T0).bytes(&answer).0);
    Rd::ok(&response).taken();
}

fn unit_ok_settings(rig: &Rig, handle: u64, signed: &SignedSettings) {
    let response = rig.call(46, handle, &W::new().bytes(&signed.to_bytes()).0);
    let mut r = Rd::ok(&response);
    let _ = (r.bool(), r.u32(), r.opt(Rd::bytes));
    r.done();
}

#[test]
fn the_old_addresses_line_and_the_deep_check_request() {
    let (rig, handle) = ready();
    let line = |rig: &Rig| {
        let response = rig.call(24, handle, &[]);
        let mut r = Rd::ok(&response);
        let out = (
            r.u8(),
            r.u64(),
            r.u64(),
            r.opt(Rd::u64),
            r.bool(),
            r.opt(Rd::words),
        );
        r.done();
        out
    };
    let (kind, count, _, _, running, words) = line(&rig);
    assert_eq!((kind, count, running), (0, 0, false), "none set aside");
    assert!(words.is_none());
    // Nothing is set aside, so there is nothing to check: a request that
    // asks for nothing, with words that say so.
    let response = rig.call(25, handle, &W::new().u64(T0).0);
    let mut r = Rd::ok(&response);
    let (asked, words) = (r.bool(), r.words());
    r.done();
    assert!(!asked);
    check_wire_words(&words).expect("words");
}

#[test]
fn the_link_ops_refuse_a_node_without_a_link() {
    let (rig, handle) = ready();
    for (op, fields) in [
        (11u16, W::new().u64(T0).0),
        (12, W::new().u64(T0).0),
        (50, Vec::new()),
        (51, W::new().u8(0).bytes(&counter_bytes()).u64(T0).0),
    ] {
        let f = fault(&rig.call(op, handle, &fields));
        assert_eq!((f.status, f.rule.as_str()), (3, "link-none"), "op {op}");
    }
}

#[test]
fn the_link_is_stepped_its_batches_delivered_and_its_state_read() {
    let rig = Rig::new();
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.link = Some(LinkConfig {
        route: RouteView::Direct {
            addr: "127.0.0.1:9".to_string(),
        },
        relay_key: id(0x41).device_id().to_bytes().to_vec(),
        counters: vec![counter_bytes()],
        heartbeat_every_secs: 60,
        check_in_grace_secs: 3_600,
    });
    let response = rig.call(1, 0, &W::new().start(&config).0);
    let handle = Rd::ok(&response).u64();
    rig.send(vec![peers_event()]);
    rig.pump(handle);

    // One turn of the link: nothing came from the counter, the relay is not
    // reached.
    let response = rig.call(11, handle, &W::new().u64(T0).0);
    let mut r = Rd::ok(&response);
    let _notes = r.list(Rd::string);
    let (queued, refused, withdrawn, held) = (r.u64(), r.u64(), r.u64(), r.u64());
    let waiting = r.opt(Rd::words);
    let stopped = r.opt(Rd::string);
    let _reached = r.bool();
    r.done();
    assert_eq!((queued, refused, withdrawn, held), (0, 0, 0, 0));
    assert!(waiting.is_none() && stopped.is_none());

    // A batch the Key app has not approved is not in the outbox yet.
    rig.queue_batch(handle);
    let deliver = |rig: &Rig| {
        let response = rig.call(12, handle, &W::new().u64(T0).0);
        let mut r = Rd::ok(&response);
        let out = (r.u32(), r.u32(), r.list(Rd::string));
        r.done();
        out
    };
    assert_eq!((deliver(&rig).0, deliver(&rig).1), (0, 0));
    rig.mine_empty(handle, 1);
    // Once it is approved, it goes to the link and is marked delivered.
    let response = rig.call(30, handle, &W::new().u64(T0).u64(1_000).0);
    let (_, request, _) = Rd::ok(&response).built();
    let answer = answer(&mut key_app(), &request, T0);
    let response = rig.call(32, handle, &W::new().u64(T0).bytes(&answer).0);
    let (_, outcomes, _) = Rd::ok(&response).taken();
    // The Key app of this kit does not know the counter, so what it
    // approves is what the link is handed, and nothing else.
    let approved = outcomes.iter().filter(|o| o.1 == 0).count() as u32;
    let (sent, waiting, notes) = deliver(&rig);
    assert_eq!((sent, waiting), (approved, 0), "{notes:?}");
    assert_eq!(
        (deliver(&rig).0, deliver(&rig).1),
        (0, 0),
        "nothing is sent twice"
    );

    // The link's state: one counter, no copy yet, no alert.
    let response = rig.call(50, handle, &[]);
    let mut r = Rd::ok(&response);
    let link = r.link();
    r.done();
    assert!(link.alerts.is_empty());
    assert_eq!(link.counters.len(), 1);
    let c = &link.counters[0];
    assert_eq!((c.0.clone(), c.1), (counter_bytes(), 0));
    assert_eq!(c.2.len(), 32);
    check_wire_words(&c.6).expect("words");
    // What was delivered waits for the counter to hold it (LK-70, LK-73).
    assert_eq!(link.batches.len(), usize::from(approved > 0));
    for b in &link.batches {
        assert_eq!((b.0.clone(), b.1), (counter_bytes(), approved));
        check_wire_words(&b.3).expect("words");
    }
    // An alert that is not raised cannot be cleared.
    let f = fault(&rig.call(51, handle, &W::new().u8(0).bytes(&counter_bytes()).u64(1).0));
    assert_eq!((f.status, f.rule.as_str()), (1, "alert-unknown"));
}

#[test]
fn refund_notes_notes_rescan_recovery_and_archive_answer() {
    let (rig, handle) = ready();
    let response = rig.call(52, handle, &[]);
    let mut r = Rd::ok(&response);
    assert!(r.list(|r| r.u32()).is_empty());
    r.done();
    let f = fault(&rig.call(53, handle, &W::new().u32(0).0));
    assert_eq!((f.status, f.rule.as_str()), (1, "note-unknown"));
    unit(&rig.call(61, handle, &[]));
    let response = rig.call(63, handle, &[]);
    let mut r = Rd::ok(&response);
    assert_eq!(r.u32(), 0);
    r.done();
    // No committed approval is waiting, so there is nothing to roll forward.
    unit(&rig.call(62, handle, &[]));
}

#[test]
fn the_proxies_reach_the_drivers() {
    let rig = Rig::new();
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.proxy = Some("127.0.0.1:9050".to_string());
    config.broadcast_proxy = Some("127.0.0.1:9051".to_string());
    config.without_tor = false;
    let response = rig.call(1, 0, &W::new().start(&config).0);
    Rd::ok(&response).u64();
    let spawned = rig.spawned();
    assert_eq!(spawned.len(), 2, "{spawned:?}");
    assert_eq!(
        spawned[0].proxy,
        Some("127.0.0.1:9050".parse().expect("addr"))
    );
    assert_eq!(
        spawned[1].proxy,
        Some("127.0.0.1:9051".parse().expect("addr"))
    );
    assert!(spawned.iter().all(|s| s.dir.starts_with(rig.dir())));
}

#[test]
fn lk_66_settings_wait_in_the_outbox_and_the_row_says_which_devices_lack_them() {
    let rig = Rig::new();
    let mut config = start_config(rig.dir().to_str().expect("UTF-8"));
    config.link = Some(LinkConfig {
        route: RouteView::Direct {
            addr: "127.0.0.1:9".to_string(),
        },
        relay_key: id(0x41).device_id().to_bytes().to_vec(),
        // LK-67: the counters come from the settings, not from here.
        counters: vec![],
        heartbeat_every_secs: 60,
        check_in_grace_secs: 3_600,
    });
    let response = rig.call(1, 0, &W::new().start(&config).0);
    let handle = Rd::ok(&response).u64();
    rig.send(vec![peers_event()]);
    rig.pump(handle);
    // First settings that name the counter, approved on the Key app.
    rig.queue_batch(handle);
    rig.fund(handle, 300_000);
    let mut settings = Settings::defaults(1_000);
    settings.devices.insert(counter());
    settings.counters.insert(counter(), 10_000);
    let first = SignedSettings {
        owner_sig: settings.sign(&id(0x11)),
        approver_sig: None,
        settings: settings.clone(),
    };
    unit_ok_settings(&rig, handle, &first);
    let mut app = key_app();
    approve(&rig, handle, &mut app);
    // A tightening the owner signs alone goes to the outbox next.
    settings.version = 2;
    settings.light_per_sale.sats -= 1;
    let second = SignedSettings {
        owner_sig: settings.sign(&id(0x11)),
        approver_sig: None,
        settings,
    };
    unit_ok_settings(&rig, handle, &second);

    let rows = |rig: &Rig| {
        let response = rig.call(27, handle, &[]);
        let mut r = Rd::ok(&response);
        let rows = r.list(|r| {
            (
                r.u32(),
                r.u8(),
                r.opt(Rd::string),
                r.opt(Rd::bytes),
                r.opt(Rd::u32),
                r.opt(Rd::u64),
                r.u32(),
                r.words(),
            )
        });
        r.done();
        rows
    };
    let deliver = |rig: &Rig| {
        let response = rig.call(12, handle, &W::new().u64(T0).0);
        let mut r = Rd::ok(&response);
        let out = (r.u32(), r.u32(), r.list(Rd::string));
        r.done();
        out
    };
    let settings_rows: Vec<_> = rows(&rig)
        .into_iter()
        .filter(|row| row.4.is_some())
        .collect();
    assert!(!settings_rows.is_empty(), "settings wait in the outbox");
    // The link has no counter before the settings are looked at, and the
    // counter the settings name after (LK-67).
    let counters = |rig: &Rig| {
        let response = rig.call(50, handle, &[]);
        let mut r = Rd::ok(&response);
        let link = r.link();
        r.done();
        link.counters.len()
    };
    assert_eq!(
        counters(&rig),
        0,
        "no counter before the settings are looked at"
    );
    // The counter has not shown it holds them, so they stay.
    let (sent, waiting, notes) = deliver(&rig);
    assert_eq!(sent, 0, "{notes:?}");
    assert!(waiting >= 1, "{notes:?}");
    assert_eq!(
        counters(&rig),
        1,
        "the counter the settings name is on the link"
    );
    let after: Vec<_> = rows(&rig)
        .into_iter()
        .filter(|row| row.4.is_some())
        .collect();
    assert_eq!(after.len(), settings_rows.len(), "none left the outbox");
    for row in &after {
        check_wire_words(&row.7).expect("words");
        assert!(row.7.what.contains("1 counter"), "{}", row.7.what);
    }
}

// ---- a hardware signer as key 2 (AS-90 to AS-99) ------------------------------------

#[test]
fn as_97_the_wallet_is_written_for_a_signer_and_the_record_of_the_first_address_round_trips() {
    let (rig, handle) = ready();
    let response = rig.call(120, handle, &W::new().string("Corner Cafe").0);
    let mut r = Rd::ok(&response);
    let name = r.string();
    let text = r.string();
    let both = r.string();
    let receive = r.string();
    let change = r.string();
    let cbor = r.bytes();
    let first = r.string();
    let (text_fits, descriptor_fits) = (r.bool(), r.bool());
    r.done();
    assert_eq!(name, "Corner Cafe");
    assert!(text.starts_with("Name: Corner Cafe\nPolicy: 2 of 3\n"));
    assert!(both.contains("<0;1>") && receive.contains("/0/*") && change.contains("/1/*"));
    assert_eq!(cbor.get(..3), Some(&[0xd9, 0x01, 0x91][..]));
    assert!(first.starts_with("bcrt1q") || first.starts_with("tb1q") || first.starts_with("bc1q"));
    assert!(text_fits && descriptor_fits);
    // The record: 1 is checked, 2 is skipped.
    let response = rig.call(125, handle, &W::new().u8(2).u64(1_700_000_000).0);
    let mut r = Rd::ok(&response);
    let record = r.bytes();
    r.done();
    assert_eq!(record.get(..4), Some(&b"OBM0"[..]));
    let response = rig.call(126, handle, &W::new().bytes(&record).0);
    let mut r = Rd::ok(&response);
    assert_eq!((r.u8(), r.u64()), (2, 1_700_000_000));
    r.done();
    // A kind that is not defined, and a record that is not framed.
    assert_eq!(
        fault(&rig.call(125, handle, &W::new().u8(9).u64(1).0)).status,
        1
    );
    let f = fault(&rig.call(126, handle, &W::new().bytes(&record[..10]).0));
    assert_eq!((f.status, f.rule.as_str()), (1, "AS-99"));
}

#[test]
fn as_91_a_session_that_does_not_exist_is_refused_with_words_by_every_signer_op() {
    let (rig, handle) = ready();
    let session = [7u8; 32];
    let f = fault(&rig.call(122, handle, &W::new().u64(T0).bytes(&session).u32(0).0));
    assert_eq!(f.status, 1, "{f:?}");
    let f = fault(&rig.call(123, handle, &W::new().bytes(&session).u32(0).0));
    assert_eq!((f.status, f.rule.as_str()), (1, "AS-90"));
    let f = fault(&rig.call(
        124,
        handle,
        &W::new().u64(T0).bytes(&session).u32(0).bytes(b"psbt\xff").0,
    ));
    assert_eq!(f.status, 1, "{f:?}");
    let f = fault(&rig.call(123, handle, &W::new().bytes(&[7; 5]).u32(0).0));
    assert_eq!(f.rule, "id-length");
    // The build for a signer is either refused with words or has no spend.
    let response = rig.call(121, handle, &W::new().u64(T0).u64(1_000).0);
    if let Some(f) = Rd::error(&response) {
        check_wire_words(&f.words).expect("words");
    }
}
