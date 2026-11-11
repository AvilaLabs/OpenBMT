//! CR-112 to CR-116: the driver of the shop's own Lightning node, run
//! against the lightning crate's recorded replies of LND and Core Lightning.

use std::sync::atomic::Ordering;

use openbmt_core::say::{check, Who};
use openbmt_counter::{Rate, Report, State};
use openbmt_lightning::testing::{
    cln, cln_created, cln_denied, cln_duplicate_label, cln_listed, cln_lookup, der, hash, lnd,
    lnd_created, lnd_denied, lnd_lookup, node_key, reply, Make,
};
use openbmt_lightning::{Ask, Credential, Reply};

use super::kit::*;
use crate::lightning::{INVOICE_WINDOW_SECS, LOOKUP_EVERY_SECS};
use crate::{
    LightningOff, LightningRecord, LnTransport, SaleLightning, SetupRefused, SetupStep,
    MAGIC_LIGHTNING_RECORD,
};

const SATS: u64 = 5_000;

fn counter4() -> [u8; 4] {
    let id = id(3).device_id().to_bytes();
    [id[0], id[1], id[2], id[3]]
}

fn record_of(c: Credential) -> Vec<u8> {
    LightningRecord {
        credential: c,
        node: node_key(1),
    }
    .to_bytes()
}

/// A rig for the test counter whose settings name test node 1, holding a
/// saved connection when `saved` has one.
fn rig_with(dir: &Scratch, named: bool, saved: Option<Vec<u8>>) -> Rig {
    let time = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(T0));
    let mut config = config(dir.path(), &time);
    if named {
        config.settings.lightning_node = Some(node_key(1));
    }
    config.lightning = saved;
    let mut rig = start_with(config, time);
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    rig
}

fn rig(dir: &Scratch, c: Credential) -> Rig {
    rig_with(dir, true, Some(record_of(c)))
}

fn open(rig: &mut Rig) -> u32 {
    rig.node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap()
        .index
}

fn tick(rig: &Rig, secs: u64) {
    rig.time.fetch_add(secs, Ordering::SeqCst);
}

/// The invoice a node signs for sale `index` asking `sats`, for hash seed 1.
fn invoice(index: u32, sats: u64) -> (String, [u8; 32]) {
    let h = hash(1);
    let mut m = Make::good(h);
    m.msat = Some(sats * 1_000);
    m.description = Some(format!("Sale {index}"));
    (m.write(), h)
}

fn created_for(c: &Credential, index: u32, sats: u64) -> Reply {
    let (bolt11, h) = invoice(index, sats);
    match c {
        Credential::Lnd(_) => lnd_created(&h, &bolt11),
        Credential::Cln(_) => cln_created(&h, &bolt11),
    }
}

fn settled(c: &Credential, sats: u64) -> Reply {
    let h = hash(1);
    match c {
        Credential::Lnd(_) => lnd_lookup(
            &h,
            sats * 1_000,
            "SETTLED",
            Some(sats * 1_000),
            Some(T0 + 3),
        ),
        Credential::Cln(_) => {
            cln_lookup(&h, sats * 1_000, "paid", Some(sats * 1_000), Some(T0 + 3))
        }
    }
}

fn waiting(c: &Credential, sats: u64) -> Reply {
    let h = hash(1);
    match c {
        Credential::Lnd(_) => lnd_lookup(&h, sats * 1_000, "OPEN", None, None),
        Credential::Cln(_) => cln_lookup(&h, sats * 1_000, "unpaid", None, None),
    }
}

#[test]
fn cr_112_a_sale_asks_for_an_invoice_of_its_quote_one_request_at_a_time() {
    for c in [lnd(), cln()] {
        let dir = Scratch::new();
        let mut rig = rig(&dir, c.clone());
        let index = open(&mut rig);
        let req = rig.node.lightning_next().expect("a request");
        assert_eq!(req.host_port, "node.local:8080");
        let body = req.request.body.clone().unwrap();
        // 5,000 satoshis in millisatoshis, for as long as a quote lasts.
        assert!(body.contains("5000000") && body.contains("300"), "{body}");
        assert!(body.contains(&format!("Sale {index}")));
        // At most one request at a time.
        assert!(rig.node.lightning_next().is_none());
        // The certificate the node must show is the one that was saved.
        let pin = rig.node.lightning_pin(req.id).expect("a pin");
        assert!(pin.accepts(&der(300, 7)));
        assert!(!pin.accepts(&der(300, 8)));
        assert!(rig.node.lightning_pin(req.id + 1).is_none());
    }
}

#[test]
fn cr_112_an_answer_for_any_other_number_is_ignored() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, lnd());
    let index = open(&mut rig);
    let req = rig.node.lightning_next().unwrap();
    rig.node
        .lightning_reply(req.id + 7, &created_for(&lnd(), index, SATS));
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Waiting);
    // The right number is taken once; the same answer again changes nothing.
    rig.node
        .lightning_reply(req.id, &created_for(&lnd(), index, SATS));
    assert!(matches!(
        rig.node.lightning_sale(index),
        SaleLightning::Invoice(_)
    ));
    rig.node
        .lightning_reply(req.id, &created_for(&lnd(), index, SATS));
    assert!(matches!(
        rig.node.lightning_sale(index),
        SaleLightning::Invoice(_)
    ));
}

#[test]
fn cr_112_a_payment_is_asked_about_every_two_seconds_while_the_invoice_lives() {
    for c in [lnd(), cln()] {
        let dir = Scratch::new();
        let mut rig = rig(&dir, c.clone());
        let index = open(&mut rig);
        let create = rig.node.lightning_next().unwrap();
        rig.node
            .lightning_reply(create.id, &created_for(&c, index, SATS));
        // The first lookup is due at once.
        let first = rig.node.lightning_next().expect("a lookup");
        assert_ne!(first.request.path, create.request.path);
        rig.node.lightning_reply(first.id, &waiting(&c, SATS));
        assert!(rig.node.lightning_next().is_none());
        tick(&rig, LOOKUP_EVERY_SECS - 1);
        assert!(rig.node.lightning_next().is_none());
        tick(&rig, 1);
        let second = rig.node.lightning_next().expect("a second lookup");
        assert_eq!(second.request.path, first.request.path);
        assert_ne!(second.id, first.id);
        rig.node.lightning_reply(second.id, &waiting(&c, SATS));
        // Not paid is not a state of the sale.
        assert_eq!(rig.node.judge(index).unwrap().state, State::NotPaidYet);
        // After the invoice has run out the driver stops asking.
        tick(&rig, 400);
        assert!(rig.node.lightning_next().is_none());
    }
}

#[test]
fn cr_109_a_settled_invoice_makes_the_sale_paid_over_lightning() {
    for c in [lnd(), cln()] {
        let dir = Scratch::new();
        let mut rig = rig(&dir, c.clone());
        let index = open(&mut rig);
        let create = rig.node.lightning_next().unwrap();
        rig.node
            .lightning_reply(create.id, &created_for(&c, index, SATS));
        let lookup = rig.node.lightning_next().unwrap();
        rig.node.lightning_reply(lookup.id, &settled(&c, SATS));
        // What the counter reports leaves with the next pump, once.
        let reports = rig.feed(vec![]);
        assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
        assert!(reports.iter().any(|r| matches!(
            r,
            Report::BooksFinal {
                received_sats: 5_000,
                ..
            }
        )));
        assert!(rig.feed(vec![]).is_empty());
        let screen = rig.node.judge(index).unwrap();
        assert_eq!(screen.state, State::PaidLightning);
        // Nothing more is asked, and the screen has no Lightning code.
        assert!(rig.node.lightning_next().is_none());
        assert_eq!(rig.node.lightning_sale(index), SaleLightning::Off);
        let sale = rig.node.counter().sale(index).unwrap();
        let ln = sale.lightning().expect("kept");
        assert_eq!(ln.payment_hash, hash(1));
        assert_eq!(ln.amount_msat, 5_000_000);
        assert_eq!(ln.settled_at, Some(T0 + 3));
    }
}

#[test]
fn cr_109_a_payment_for_less_than_the_sale_is_not_a_payment() {
    let dir = Scratch::new();
    let c = lnd();
    let mut rig = rig(&dir, c.clone());
    let index = open(&mut rig);
    let create = rig.node.lightning_next().unwrap();
    rig.node
        .lightning_reply(create.id, &created_for(&c, index, SATS));
    let lookup = rig.node.lightning_next().unwrap();
    let h = hash(1);
    rig.node.lightning_reply(
        lookup.id,
        &lnd_lookup(&h, 5_000_000, "SETTLED", Some(4_999_999), None),
    );
    assert!(rig.feed(vec![]).is_empty());
    assert_eq!(rig.node.judge(index).unwrap().state, State::NotPaidYet);
    assert!(rig.node.notes().any(|n| n.contains("settled")));
}

#[test]
fn cr_112_a_new_quote_asks_for_a_new_invoice_and_lets_the_old_one_go() {
    let dir = Scratch::new();
    let c = cln();
    let mut rig = rig(&dir, c.clone());
    let index = open(&mut rig);
    let create = rig.node.lightning_next().unwrap();
    let label = |n: u32| {
        Ask::sale(counter4(), 0, n, 5_000_000, 300)
            .unwrap()
            .label()
            .to_string()
    };
    assert!(create.request.body.as_ref().unwrap().contains(&label(0)));
    rig.node
        .lightning_reply(create.id, &created_for(&c, index, SATS));
    assert!(matches!(
        rig.node.lightning_sale(index),
        SaleLightning::Invoice(_)
    ));
    // The price moves: 2,500 satoshis now.
    tick(&rig, 30);
    let rate = Rate::new(20_000_000).unwrap();
    rig.node.requote(index, rate, &fresh(T0 + 30)).unwrap();
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Waiting);
    let again = rig.node.lightning_next().expect("a new request");
    let body = again.request.body.unwrap();
    assert!(body.contains("2500000"), "{body}");
    assert!(body.contains(&label(1)), "{body}");
}

#[test]
fn cr_113_no_invoice_in_six_seconds_leaves_the_on_chain_code_and_one_line() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, lnd());
    let index = open(&mut rig);
    let req = rig.node.lightning_next().unwrap();
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Waiting);
    tick(&rig, INVOICE_WINDOW_SECS - 1);
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Waiting);
    tick(&rig, 1);
    let SaleLightning::Unavailable(words) = rig.node.lightning_sale(index) else {
        panic!("unavailable");
    };
    assert_eq!(words.heading, "Lightning not available");
    assert_eq!(words.who, Who::Cashier);
    assert_eq!(check(&words), Ok(()));
    assert!(words.detail.is_some());
    // A late answer is not used.
    rig.node
        .lightning_reply(req.id, &created_for(&lnd(), index, SATS));
    assert!(matches!(
        rig.node.lightning_sale(index),
        SaleLightning::Unavailable(_)
    ));
    // The sale itself is not touched.
    assert_eq!(rig.node.judge(index).unwrap().state, State::NotPaidYet);
}

#[test]
fn cr_113_an_invoice_that_fails_a_check_is_not_shown() {
    let dir = Scratch::new();
    let c = lnd();
    let mut rig = rig(&dir, c.clone());
    let index = open(&mut rig);
    let req = rig.node.lightning_next().unwrap();
    // Signed by another node than the settings name (LN-17).
    let h = hash(1);
    let mut m = Make::good(h);
    m.msat = Some(5_000_000);
    m.description = Some("Sale 0".to_string());
    m.key = openbmt_lightning::testing::secret(2);
    rig.node
        .lightning_reply(req.id, &lnd_created(&h, &m.write()));
    let SaleLightning::Unavailable(words) = rig.node.lightning_sale(index) else {
        panic!("unavailable");
    };
    assert_eq!(words.code, Some("LN-17"));
    assert_eq!(check(&words), Ok(()));
    assert!(rig.node.lightning_next().is_none());
}

#[test]
fn cr_113_a_node_that_cannot_be_reached_is_tried_again_inside_the_window() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, lnd());
    let index = open(&mut rig);
    let req = rig.node.lightning_next().unwrap();
    rig.node.lightning_failed(req.id, LnTransport::TimedOut);
    // The same request, after a second.
    assert!(rig.node.lightning_next().is_none());
    tick(&rig, 1);
    let again = rig.node.lightning_next().expect("again");
    assert_eq!(again.request, req.request);
    assert_ne!(again.id, req.id);
    rig.node
        .lightning_reply(again.id, &created_for(&lnd(), index, SATS));
    assert!(matches!(
        rig.node.lightning_sale(index),
        SaleLightning::Invoice(_)
    ));
    // The failure is a line in the log and a word on the status, never a
    // state of the sale.
    assert!(rig.node.notes().any(|n| n.contains("TimedOut")));
    assert!(rig.node.lightning_status().unwrap().ready);
}

#[test]
fn cr_113_the_reason_of_a_node_that_never_answers_is_in_the_details() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, lnd());
    let index = open(&mut rig);
    for _ in 0..3 {
        let req = rig.node.lightning_next().expect("a request");
        rig.node.lightning_failed(req.id, LnTransport::Unreachable);
        tick(&rig, 2);
    }
    let SaleLightning::Unavailable(words) = rig.node.lightning_sale(index) else {
        panic!("unavailable");
    };
    assert!(words.detail.unwrap().contains("connection"));
    let status = rig.node.lightning_status().unwrap();
    assert!(!status.ready);
    assert_eq!(check(&status.words), Ok(()));
}

#[test]
fn cr_112_core_lightning_takes_up_the_invoice_an_earlier_try_made() {
    let dir = Scratch::new();
    let c = cln();
    let mut rig = rig(&dir, c.clone());
    let index = open(&mut rig);
    let create = rig.node.lightning_next().unwrap();
    let ask = Ask::sale(counter4(), 0, 0, 5_000_000, 300).unwrap();
    rig.node
        .lightning_reply(create.id, &cln_duplicate_label(ask.label()));
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Waiting);
    let by_label = rig.node.lightning_next().expect("a request by label");
    assert!(by_label
        .request
        .body
        .as_ref()
        .unwrap()
        .contains(ask.label()));
    let (bolt11, h) = invoice(index, SATS);
    rig.node.lightning_reply(
        by_label.id,
        &cln_listed(ask.label(), 5_000_000, "unpaid", "Sale 0", &h, &bolt11),
    );
    assert!(matches!(
        rig.node.lightning_sale(index),
        SaleLightning::Invoice(_)
    ));
}

#[test]
fn cr_112_an_invoice_that_is_not_this_sales_makes_the_next_attempt() {
    let dir = Scratch::new();
    let c = cln();
    let mut rig = rig(&dir, c.clone());
    let index = open(&mut rig);
    let create = rig.node.lightning_next().unwrap();
    let first = Ask::sale(counter4(), 0, 0, 5_000_000, 300).unwrap();
    rig.node
        .lightning_reply(create.id, &cln_duplicate_label(first.label()));
    let by_label = rig.node.lightning_next().unwrap();
    let (bolt11, h) = invoice(index, SATS);
    // The amount differs: the label belongs to another sale's invoice.
    rig.node.lightning_reply(
        by_label.id,
        &cln_listed(first.label(), 9_000_000, "unpaid", "Sale 0", &h, &bolt11),
    );
    let next = rig.node.lightning_next().expect("the next attempt");
    let second = Ask::sale(counter4(), 0, 1, 5_000_000, 300).unwrap();
    assert!(next.request.body.unwrap().contains(second.label()));
}

#[test]
fn cr_115_the_saved_record_reads_back_and_is_read_strictly() {
    for c in [lnd(), cln()] {
        let record = LightningRecord {
            credential: c,
            node: node_key(1),
        };
        let bytes = record.to_bytes();
        assert_eq!(&bytes[..4], &MAGIC_LIGHTNING_RECORD);
        assert_eq!(LightningRecord::from_bytes(&bytes).unwrap(), record);
        for n in 0..bytes.len() {
            assert!(LightningRecord::from_bytes(&bytes[..n]).is_err(), "cut {n}");
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(LightningRecord::from_bytes(&longer).is_err());
        // The kind byte after the magic and the version.
        let mut bad = bytes.clone();
        bad[5] = 9;
        assert!(LightningRecord::from_bytes(&bad).is_err());
        // The secret is not in the debug text.
        let text = format!("{record:?}");
        assert!(text.contains("not shown"), "{text}");
    }
}

#[test]
fn cr_115_setup_with_no_node_named_tests_and_saves_nothing() {
    let dir = Scratch::new();
    let mut rig = rig_with(&dir, false, None);
    let refused = rig.node.lightning_setup(lnd()).unwrap_err();
    assert_eq!(refused, SetupRefused::NotNamed);
    let words = refused.words();
    assert!(words.what.contains("has not named"), "{}", words.what);
    assert_eq!(check(&words), Ok(()));
    // Nothing was sent and nothing is saved.
    assert!(rig.node.lightning_next().is_none());
    assert!(rig.node.lightning_status().is_none());
}

fn run_setup(rig: &mut Rig, c: Credential, denied: Reply, test_invoice: &Reply) -> Vec<u8> {
    let first = rig.node.lightning_setup(c.clone()).unwrap();
    assert!(rig.node.lightning_pin(first.id).is_some());
    let SetupStep::More(second) = rig.node.lightning_setup_answer(first.id, &denied).unwrap()
    else {
        panic!("more");
    };
    let SetupStep::More(third) = rig.node.lightning_setup_answer(second.id, &denied).unwrap()
    else {
        panic!("more");
    };
    // A stale number is not taken.
    assert_eq!(
        rig.node
            .lightning_setup_answer(first.id, test_invoice)
            .unwrap_err(),
        SetupRefused::NoSetup
    );
    let SetupStep::Done(bytes) = rig
        .node
        .lightning_setup_answer(third.id, test_invoice)
        .unwrap_or_else(|e| panic!("{e}"))
    else {
        panic!("done");
    };
    bytes
}

fn test_invoice_for(c: &Credential) -> Reply {
    // The test invoice: one satoshi, signed by the node of the settings.
    let h = hash(3);
    let mut m = Make::good(h);
    m.msat = Some(1_000);
    m.description = Some("OpenBMT test".to_string());
    match c {
        Credential::Lnd(_) => lnd_created(&h, &m.write()),
        Credential::Cln(_) => cln_created(&h, &m.write()),
    }
}

#[test]
fn cr_115_a_connection_that_passes_the_test_is_kept_and_used_at_once() {
    for (c, denied) in [(lnd(), lnd_denied()), (cln(), cln_denied())] {
        let dir = Scratch::new();
        let mut rig = rig_with(&dir, true, None);
        let index = open(&mut rig);
        assert!(rig.node.lightning_status().is_none());
        assert!(!rig.node.lightning_on());
        let bytes = run_setup(&mut rig, c.clone(), denied, &test_invoice_for(&c));
        // The bytes are the record to seal.
        let record = LightningRecord::from_bytes(&bytes).unwrap();
        assert_eq!(record.credential, c);
        assert_eq!(record.node, node_key(1));
        assert!(rig.node.lightning_on());
        let status = rig.node.lightning_status().unwrap();
        assert!(status.ready);
        assert_eq!(status.words.heading, "Lightning ready");
        // The sale that was open asks for an invoice now.
        assert!(rig.node.lightning_next().is_some());
        assert_eq!(rig.node.lightning_sale(index), SaleLightning::Waiting);
        // Removing it deletes it at once.
        rig.node.lightning_remove();
        assert!(!rig.node.lightning_on());
        assert!(rig.node.lightning_status().is_none());
        assert!(rig.node.lightning_next().is_none());
        assert_eq!(rig.node.lightning_sale(index), SaleLightning::Off);
    }
}

#[test]
fn cr_115_a_key_that_can_do_more_than_invoices_is_not_kept() {
    let dir = Scratch::new();
    let mut rig = rig_with(&dir, true, None);
    let first = rig.node.lightning_setup(lnd()).unwrap();
    let allowed = reply(200, "{\"identity_pubkey\":\"02\"}");
    let refused = rig
        .node
        .lightning_setup_answer(first.id, &allowed)
        .unwrap_err();
    assert!(matches!(refused, SetupRefused::Connection(_)));
    assert!(refused.rule().starts_with("LN-"));
    assert_eq!(check(&refused.words()), Ok(()));
    assert!(!rig.node.lightning_on());
    assert!(rig.node.lightning_status().is_none());
}

#[test]
fn cr_115_a_test_that_gets_no_answer_says_why_and_keeps_nothing() {
    for why in LnTransport::ALL {
        let dir = Scratch::new();
        let mut rig = rig_with(&dir, true, None);
        let first = rig.node.lightning_setup(lnd()).unwrap();
        let refused = rig.node.lightning_setup_failed(first.id, why);
        assert_eq!(refused, SetupRefused::Transport(why));
        assert_eq!(refused.rule(), "CR-114");
        assert_eq!(check(&refused.words()), Ok(()));
        assert!(!rig.node.lightning_on());
    }
}

#[test]
fn cr_116_the_status_has_a_lightning_line_only_with_a_saved_credential() {
    let dir = Scratch::new();
    let rig = rig_with(&dir, true, None);
    assert!(rig.node.lightning_status().is_none());
    let dir = Scratch::new();
    let rig = rig_with(&dir, true, Some(record_of(lnd())));
    let status = rig.node.lightning_status().unwrap();
    assert!(status.ready);
    assert_eq!(check(&status.words), Ok(()));
}

#[test]
fn cr_116_a_node_that_refused_the_key_or_was_not_reached_shows_why() {
    let dir = Scratch::new();
    let mut rig = rig(&dir, lnd());
    open(&mut rig);
    let req = rig.node.lightning_next().unwrap();
    rig.node
        .lightning_reply(req.id, &reply(401, "{\"code\":2,\"message\":\"no\"}"));
    let status = rig.node.lightning_status().unwrap();
    assert!(!status.ready);
    assert_eq!(status.words.heading, "Lightning node refused key");
    // A good answer afterwards makes it ready again.
    let dir = Scratch::new();
    let mut rig2 = rig_with(&dir, true, Some(record_of(lnd())));
    let index = open(&mut rig2);
    let req = rig2.node.lightning_next().unwrap();
    rig2.node.lightning_failed(req.id, LnTransport::Unreachable);
    assert_eq!(
        rig2.node.lightning_status().unwrap().words.heading,
        "Lightning node not reached"
    );
    tick(&rig2, 1);
    let req = rig2.node.lightning_next().unwrap();
    rig2.node
        .lightning_reply(req.id, &created_for(&lnd(), index, SATS));
    assert!(rig2.node.lightning_status().unwrap().ready);
}

#[test]
fn cr_116_another_node_in_the_settings_turns_lightning_off_and_says_so() {
    // The record was tested against node 1; the settings now name node 2.
    let dir = Scratch::new();
    let time = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(T0));
    let mut config = config(dir.path(), &time);
    config.settings.lightning_node = Some(node_key(2));
    config.lightning = Some(record_of(lnd()));
    let mut rig = start_with(config, time);
    rig.feed(vec![Rig::peers()]);
    let status = rig.node.lightning_status().unwrap();
    assert!(!status.ready);
    assert_eq!(
        status.words.heading,
        LightningOff::NodeChanged.words().heading
    );
    assert!(!rig.node.lightning_on());
    // A sale then has no Lightning line and no request is made.
    let index = open(&mut rig);
    assert!(rig.node.lightning_next().is_none());
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Off);
    // And it says so in the log.
    assert!(rig.node.notes().any(|n| n.contains("Lightning is off")));
}

#[test]
fn cr_116_settings_with_no_node_or_a_record_that_does_not_read_turn_it_off() {
    let dir = Scratch::new();
    let rig = rig_with(&dir, false, Some(record_of(cln())));
    let status = rig.node.lightning_status().unwrap();
    assert!(!status.ready);
    assert_eq!(status.words.heading, LightningOff::NotNamed.words().heading);

    let dir = Scratch::new();
    let mut bad = record_of(cln());
    bad.truncate(40);
    let rig = rig_with(&dir, true, Some(bad));
    let status = rig.node.lightning_status().unwrap();
    assert!(!status.ready);
    assert_eq!(
        status.words.heading,
        LightningOff::Unreadable.words().heading
    );
    assert!(!rig.node.lightning_on());
}

#[test]
fn cr_112_a_sale_open_at_the_last_stop_asks_again_after_a_restart() {
    let dir = Scratch::new();
    let index;
    {
        let mut first = rig(&dir, lnd());
        index = open(&mut first);
        first.node.shutdown();
    }
    let rig = rig(&dir, lnd());
    let mut rig = rig;
    let req = rig.node.lightning_next().expect("asks again");
    assert!(req.request.body.unwrap().contains(&format!("Sale {index}")));
}

#[test]
fn cr_113_a_sale_that_is_cancelled_is_not_followed_any_more() {
    let dir = Scratch::new();
    let c = lnd();
    let mut rig = rig(&dir, c.clone());
    let index = open(&mut rig);
    let create = rig.node.lightning_next().unwrap();
    rig.node
        .lightning_reply(create.id, &created_for(&c, index, SATS));
    rig.node.cancel(index).unwrap();
    assert_eq!(rig.node.lightning_sale(index), SaleLightning::Off);
    assert!(rig.node.lightning_next().is_none());
}
