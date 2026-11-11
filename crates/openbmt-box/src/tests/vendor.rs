//! BX-49: the vendor's reader behind the connector, the Box's own POS
//! config, the pace of the reading, and the POS side over a connector that
//! reads a vendor's replies. The replies are the fixtures of
//! `openbmt-pos-readers`; the runner is a fake, so there is no network.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use openbmt_core::identity::DeviceId;
use openbmt_core::settings::DeviceRole;
use openbmt_net_io::SystemClock;
use openbmt_pos::{Connector, Cursor, NoticeDraft, NoticeState, PosError, PosReader, SaleFact};
use openbmt_pos_readers::{clover, square};

use super::https::Dir;
use super::kit::*;
use super::pos_kit::{item, roles, token, usd};
use crate::files::{BoxFiles, DirFiles, MemFiles};
use crate::https::{
    HttpsRefusal, HttpsReply, HttpsRequest, HttpsRunner, Method, Transport, RATE_LIMIT_WAIT_SECS,
};
use crate::pos::{BoxPos, PosSetup, Reading};
use crate::pos_config::{PosConfig, PosConfigError, Vendor, POS_CONFIG_FILE};
use crate::runner::{BoxRunner, PosView};
use crate::say::note_words;
use crate::vendor::{VendorConnector, PAGE_LIMIT, VENDOR_POLL_SECS};

const SQUARE_PAGE1: &str =
    include_str!("../../../openbmt-pos-readers/tests/fixtures/square_orders_page1.json");
const SQUARE_LAST: &str =
    include_str!("../../../openbmt-pos-readers/tests/fixtures/square_orders_last.json");
const SQUARE_EMPTY: &str =
    include_str!("../../../openbmt-pos-readers/tests/fixtures/square_orders_empty.json");
const SQUARE_PAYMENTS: &str =
    include_str!("../../../openbmt-pos-readers/tests/fixtures/square_payments.json");
const SQUARE_PAYMENTS_PAGE1: &str =
    include_str!("../../../openbmt-pos-readers/tests/fixtures/square_payments_page1.json");
const CLOVER_ORDERS: &str =
    include_str!("../../../openbmt-pos-readers/tests/fixtures/clover_orders.json");

/// The time of the Bitcoin tender in `square_orders_last.json`:
/// 2026-11-10T15:01:30Z.
const TENDER_AT: u64 = 1_794_322_890;

#[derive(Default)]
struct Fake {
    replies: VecDeque<Result<HttpsReply, HttpsRefusal>>,
    seen: Vec<HttpsRequest>,
}

/// A runner the test holds a handle to.
#[derive(Clone, Default)]
struct FakeRunner(Arc<Mutex<Fake>>);

impl FakeRunner {
    fn say(&self, status: u16, body: &str) {
        self.0.lock().unwrap().replies.push_back(Ok(HttpsReply {
            status,
            body: body.as_bytes().to_vec(),
        }));
    }

    fn fail(&self, why: HttpsRefusal) {
        self.0.lock().unwrap().replies.push_back(Err(why));
    }

    fn seen(&self) -> Vec<HttpsRequest> {
        self.0.lock().unwrap().seen.clone()
    }
}

impl HttpsRunner for FakeRunner {
    fn run(&mut self, request: &HttpsRequest) -> Result<HttpsReply, HttpsRefusal> {
        let mut f = self.0.lock().unwrap();
        f.seen.push(request.clone());
        f.replies.pop_front().unwrap_or(Ok(HttpsReply {
            status: 200,
            body: SQUARE_EMPTY.as_bytes().to_vec(),
        }))
    }
}

fn square_config() -> PosConfig {
    PosConfig::parse("vendor=square|location=LOCSQ1|currency=USD|tenders=Bitcoin").unwrap()
}

fn clover_config() -> PosConfig {
    PosConfig::parse("vendor=clover|location=MERCH12345|currency=USD|tenders=Bitcoin;BTC").unwrap()
}

fn connector(config: &PosConfig, runner: &FakeRunner) -> (VendorConnector, Arc<Transport>) {
    let transport = Arc::new(Transport::new());
    let mut c =
        VendorConnector::new(config, Box::new(runner.clone()), Arc::clone(&transport)).unwrap();
    c.set_token(Some("vendor-token-abc"));
    (c, transport)
}

/// A POS side over a vendor's connector, set up as the runner does it.
fn vendor_pos(
    config: &PosConfig,
    runner: &FakeRunner,
    files: MemFiles,
) -> (BoxPos<MemFiles>, Arc<Transport>) {
    let (c, transport) = connector(config, runner);
    let mut setup = PosSetup::new(
        config.settings(vec![counter1().device_id()]).unwrap(),
        Box::new(c),
    );
    setup.interval_secs = VENDOR_POLL_SECS;
    setup.min_gap_secs = VENDOR_POLL_SECS;
    setup.counters_from_list = true;
    setup.transport = Some(Arc::clone(&transport));
    (BoxPos::open(files, the_box(), setup).unwrap(), transport)
}

fn paid_notices(amount: u64) -> Vec<(DeviceId, Vec<u8>)> {
    let draft = |state, at| NoticeDraft {
        index: 0,
        amount: openbmt_counter::Amount(amount),
        currency: usd(),
        opened_at: TENDER_AT - 120,
        state,
        state_at: at,
    };
    vec![
        item(&counter1(), 1, &draft(NoticeState::Opened, TENDER_AT - 120)),
        item(&counter1(), 2, &draft(NoticeState::Paid, TENDER_AT - 30)),
    ]
}

// ---- BX-49: what a poll sends ----

#[test]
fn bx_49_a_square_poll_is_the_post_the_reader_builds_with_the_token_in_a_header() {
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    runner.say(200, SQUARE_PAYMENTS);
    let (mut c, transport) = connector(&square_config(), &runner);
    transport.set_tor(false);
    let page = c.poll(&Cursor::start()).unwrap();
    assert_eq!(page.orders.len(), 1);
    assert!(!page.more);
    let seen = runner.seen();
    // The search, then the payments of its orders.
    assert_eq!(seen.len(), 2);
    let r = &seen[0];
    assert_eq!(r.method, Method::Post);
    assert_eq!(r.url, "https://connect.squareup.com/v2/orders/search");
    let want = square::Request::new(
        &square_config().settings(vec![]).unwrap(),
        PAGE_LIMIT,
        &Cursor::start(),
    )
    .unwrap();
    assert_eq!(r.body.as_deref(), Some(want.body().as_str()));
    assert!(r
        .headers
        .contains(&("Authorization".into(), "Bearer vendor-token-abc".into())));
    assert!(r.headers.iter().any(|(k, _)| k == "Content-Type"));
    assert_eq!(r.socks5, None);
    // Nothing but reads: Square is read-only.
    assert!(!r.url.contains("close") && !r.url.contains("pay"));
    let p = &seen[1];
    assert_eq!(p.method, Method::Get);
    assert_eq!(p.body, None);
    assert_eq!(p.socks5, None);
    assert_eq!(
        p.url,
        format!(
            "https://connect.squareup.com{}",
            want.payments_path(100, None).unwrap()
        )
    );
    assert!(p
        .headers
        .contains(&("Authorization".into(), "Bearer vendor-token-abc".into())));
    // The cancelled payment of the fixture is seen on its tender.
    assert!(page.orders[0]
        .tenders
        .iter()
        .any(|t| t.state == openbmt_pos::TenderState::Voided));
}

#[test]
fn bx_49_the_payments_of_a_square_page_are_read_page_by_page_through_the_same_runner() {
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    runner.say(200, SQUARE_PAYMENTS_PAGE1);
    runner.say(200, SQUARE_PAYMENTS);
    let (mut c, _) = connector(&square_config(), &runner);
    let page = c.poll(&Cursor::start()).unwrap();
    let seen = runner.seen();
    assert_eq!(seen.len(), 3);
    assert!(!seen[1].url.contains("cursor="));
    assert!(
        seen[2].url.ends_with("&cursor=PAYCURSOR1%3D%3D"),
        "{}",
        seen[2].url
    );
    assert!(page.orders[0]
        .tenders
        .iter()
        .any(|t| t.state == openbmt_pos::TenderState::Voided));
    // A page with no order asks for no payments.
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_EMPTY);
    let (mut c, _) = connector(&square_config(), &runner);
    c.poll(&Cursor::start()).unwrap();
    assert_eq!(runner.seen().len(), 1);
    // A payments reply that repeats its marker is not asked for again.
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    for _ in 0..3 {
        runner.say(200, SQUARE_PAYMENTS_PAGE1);
    }
    let (mut c, _) = connector(&square_config(), &runner);
    c.poll(&Cursor::start()).unwrap();
    assert_eq!(runner.seen().len(), 3);
    // Clover asks for no payments: they are in the orders.
    let runner = FakeRunner::default();
    runner.say(200, CLOVER_ORDERS);
    let (mut c, _) = connector(&clover_config(), &runner);
    c.poll(&Cursor::start()).unwrap();
    assert_eq!(runner.seen().len(), 1);
}

#[test]
fn bx_49_more_payment_pages_than_a_poll_reads_fail_the_poll_and_are_never_cut_in_silence() {
    assert!(SQUARE_PAYMENTS_PAGE1.contains("PAYCURSOR1=="));
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    // Every page names a new cursor, so the pages never end.
    for n in 0..crate::vendor::MAX_PAYMENT_PAGES {
        runner.say(
            200,
            &SQUARE_PAYMENTS_PAGE1.replace("PAYCURSOR1==", &format!("PAYCURSOR{n}X")),
        );
    }
    let (mut c, transport) = connector(&square_config(), &runner);
    assert!(c.poll(&Cursor::start()).is_err());
    assert_eq!(runner.seen().len(), 1 + crate::vendor::MAX_PAYMENT_PAGES);
    match transport.take() {
        Some(HttpsRefusal::Unreadable(why)) => assert!(why.starts_with("BX-49"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn bx_49_a_payments_request_that_fails_fails_the_poll_with_its_words() {
    for (status, heading) in [
        (401, "Till refused the token"),
        (429, "Till asks to wait"),
        (500, "Till system error"),
    ] {
        let runner = FakeRunner::default();
        runner.say(200, SQUARE_LAST);
        runner.say(status, "{}");
        let (mut c, transport) = connector(&square_config(), &runner);
        assert!(c.poll(&Cursor::start()).is_err());
        let why = transport.take().unwrap();
        assert_eq!(note_words(&why.note()).heading, heading);
    }
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    runner.fail(HttpsRefusal::Timeout);
    let (mut c, transport) = connector(&square_config(), &runner);
    assert_eq!(c.poll(&Cursor::start()), Err(PosError::Unavailable));
    assert_eq!(transport.take(), Some(HttpsRefusal::Timeout));
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    runner.say(200, "not json");
    let (mut c, transport) = connector(&square_config(), &runner);
    assert!(c.poll(&Cursor::start()).is_err());
    assert!(matches!(
        transport.take(),
        Some(HttpsRefusal::Unreadable(_))
    ));
}

#[test]
fn bx_49_a_clover_poll_is_the_get_the_reader_builds() {
    let runner = FakeRunner::default();
    runner.say(200, CLOVER_ORDERS);
    let (mut c, transport) = connector(&clover_config(), &runner);
    transport.set_tor(false);
    let page = c.poll(&Cursor::start()).unwrap();
    assert_eq!(page.orders.len(), 3);
    let seen = runner.seen();
    let r = &seen[0];
    assert_eq!(r.method, Method::Get);
    let want = clover::Request::new(
        "MERCH12345",
        &clover_config().settings(vec![]).unwrap(),
        PAGE_LIMIT,
        &Cursor::start(),
    )
    .unwrap();
    assert_eq!(
        r.url,
        format!("https://api.clover.com{}", want.path_and_query())
    );
    assert_eq!(r.body, None);
    assert!(r
        .headers
        .contains(&("Authorization".into(), "Bearer vendor-token-abc".into())));
    // The host can be named, for another region or a sandbox.
    let mut other = clover_config();
    other.host = Some("https://api.eu.clover.com".into());
    let runner = FakeRunner::default();
    runner.say(200, CLOVER_ORDERS);
    let (mut c, _) = connector(&other, &runner);
    c.poll(&Cursor::start()).unwrap();
    assert!(runner.seen()[0]
        .url
        .starts_with("https://api.eu.clover.com/v3/"));
}

#[test]
fn bx_49_a_connector_without_a_token_or_with_a_failure_gives_the_sync_a_reason() {
    let runner = FakeRunner::default();
    let (mut c, transport) = connector(&square_config(), &runner);
    c.set_token(None);
    assert_eq!(c.poll(&Cursor::start()), Err(PosError::SignInRefused));
    assert!(runner.seen().is_empty());
    assert!(matches!(
        transport.take(),
        Some(HttpsRefusal::TokenRefused { .. })
    ));
    c.set_token(Some("t"));
    runner.say(200, "not json");
    assert!(matches!(
        c.poll(&Cursor::start()),
        Err(PosError::BadResponse(_))
    ));
    assert!(matches!(
        transport.take(),
        Some(HttpsRefusal::Unreadable(_))
    ));
    runner.fail(HttpsRefusal::Timeout);
    assert_eq!(c.poll(&Cursor::start()), Err(PosError::Unavailable));
    assert_eq!(transport.take(), Some(HttpsRefusal::Timeout));
    // A poll that works clears what the last one left.
    runner.fail(HttpsRefusal::Timeout);
    let _ = c.poll(&Cursor::start());
    runner.say(200, SQUARE_EMPTY);
    c.poll(&Cursor::start()).unwrap();
    assert_eq!(transport.take(), None);
}

// ---- BX-49: reading, matching, paging ----

#[test]
fn bx_49_a_reading_matches_a_sale_the_counter_reported_to_the_vendors_order() {
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    let (mut pos, _) = vendor_pos(&square_config(), &runner, MemFiles::new());
    pos.take_notices(&paid_notices(700), &roles(), TENDER_AT);
    let notes = pos.tick(TENDER_AT + 120, token());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(pos.ledger().matches().len(), 1);
    assert_eq!(pos.pending_reports().len(), 1);
    assert_eq!(pos.state().reading, Reading::Reading);
    // The cursor is stored as the ledger.
    assert!(pos.files().read(crate::pos::LEDGER_FILE).unwrap().is_some());
    assert!(pos.ledger().cursor() != &Cursor::start());
}

#[test]
fn bx_49_pages_are_read_a_page_at_a_time_and_a_page_that_does_not_move_the_cursor_is_refused() {
    let runner = FakeRunner::default();
    // Two pages that say there is more, with the same marker.
    runner.say(200, SQUARE_PAGE1);
    runner.say(200, "{}");
    runner.say(200, SQUARE_PAGE1);
    runner.say(200, "{}");
    let (mut pos, _) = vendor_pos(&square_config(), &runner, MemFiles::new());
    let notes = pos.tick(TENDER_AT, token());
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].starts_with("the POS was not read: "), "{notes:?}");
    assert!(notes[0].contains("does not move on"), "{notes:?}");
    assert!(notes[0].contains("1 pages were kept"), "{notes:?}");
    // One search for each page (each followed by its payments), the second
    // from the cursor the first gave.
    let seen = runner.seen();
    assert_eq!(seen.len(), 4);
    assert!(!seen[0].body.as_deref().unwrap().contains("\"cursor\""));
    assert!(seen[2].body.as_deref().unwrap().contains("\"cursor\""));
    // What was kept stays: the cursor moved on from the start.
    assert!(pos.ledger().cursor() != &Cursor::start());
    assert!(pos.state().last_error.is_some());
}

#[test]
fn bx_49_a_failed_request_is_a_note_with_words_and_changes_nothing() {
    let runner = FakeRunner::default();
    runner.fail(HttpsRefusal::Timeout);
    runner.say(401, "{}");
    runner.say(500, "{}");
    runner.say(200, "{\"orders\":[{\"id\":1}]}");
    let (mut pos, _) = vendor_pos(&square_config(), &runner, MemFiles::new());
    let mut now = TENDER_AT;
    let mut lines = Vec::new();
    for _ in 0..4 {
        lines.extend(pos.tick(now, token()));
        now += VENDOR_POLL_SECS;
    }
    assert_eq!(lines.len(), 4, "{lines:?}");
    let headings: Vec<String> = lines.iter().map(|l| note_words(l).heading).collect();
    assert_eq!(
        headings,
        [
            "Till system too slow",
            "Till refused the token",
            "Till system error",
            "Till reply not readable"
        ]
    );
    assert_eq!(pos.ledger().cursor(), &Cursor::start());
    assert_eq!(pos.pending_reports().len(), 0);
    // The state the console shows has the same words.
    let words = pos.state().words(crate::state::BoxStatus::Paired);
    assert_eq!(words.heading, "Till reply not readable");
}

#[test]
fn bx_49_a_reading_is_made_once_a_minute_at_most() {
    let runner = FakeRunner::default();
    let (mut pos, _) = vendor_pos(&square_config(), &runner, MemFiles::new());
    pos.tick(TENDER_AT, token());
    assert_eq!(runner.seen().len(), 1);
    // A paid notice's wish for a reading at once waits for the minute.
    pos.take_notices(&paid_notices(700), &roles(), TENDER_AT + 5);
    pos.tick(TENDER_AT + 6, token());
    pos.tick(TENDER_AT + VENDOR_POLL_SECS - 1, token());
    assert_eq!(runner.seen().len(), 1);
    pos.tick(TENDER_AT + VENDOR_POLL_SECS, token());
    assert_eq!(runner.seen().len(), 2);
    // Without notices the next is a minute on, and not before.
    pos.tick(TENDER_AT + VENDOR_POLL_SECS + 59, token());
    assert_eq!(runner.seen().len(), 2);
    pos.tick(TENDER_AT + 2 * VENDOR_POLL_SECS, token());
    assert_eq!(runner.seen().len(), 3);
}

#[test]
fn bx_49_a_429_waits_the_longer_of_the_minute_and_what_the_vendor_asked() {
    let runner = FakeRunner::default();
    runner.say(429, "{}");
    let (mut pos, _) = vendor_pos(&square_config(), &runner, MemFiles::new());
    let notes = pos.tick(TENDER_AT, token());
    assert_eq!(note_words(&notes[0]).heading, "Till asks to wait");
    assert_eq!(RATE_LIMIT_WAIT_SECS, 60);
    pos.tick(TENDER_AT + RATE_LIMIT_WAIT_SECS - 1, token());
    assert_eq!(runner.seen().len(), 1);
    pos.tick(TENDER_AT + RATE_LIMIT_WAIT_SECS, token());
    assert_eq!(runner.seen().len(), 2);
}

#[test]
fn bx_49_no_order_is_ever_closed_through_the_vendor() {
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    let (mut c, _) = connector(&square_config(), &runner);
    assert!(!c.can_close());
    let sale = SaleFact::opened(
        counter1().device_id(),
        0,
        openbmt_counter::Amount(700),
        usd(),
        TENDER_AT,
    );
    assert_eq!(c.find_order(&sale), Ok(None));
    let request = openbmt_pos::CloseRequest::for_sale(
        &{
            let mut s = sale.clone();
            s.paid_at = Some(TENDER_AT);
            s
        },
        openbmt_pos::Id::new("SQORDER0004").unwrap(),
        openbmt_pos::Text::new("Bitcoin").unwrap(),
    )
    .unwrap();
    assert_eq!(c.close(&request), Err(PosError::Unsupported));
    assert!(runner.seen().is_empty());
    // And a Box with it sends only reads, for a sale reported paid.
    let runner = FakeRunner::default();
    runner.say(200, SQUARE_LAST);
    let (mut pos, _) = vendor_pos(&square_config(), &runner, MemFiles::new());
    pos.take_notices(&paid_notices(700), &roles(), TENDER_AT);
    pos.tick(TENDER_AT + 120, token());
    assert!(!pos.state().can_close);
    assert_eq!(pos.state().closes, 0);
    assert!(runner.seen().iter().all(|r| {
        (r.method == Method::Post && r.url.ends_with("/v2/orders/search"))
            || (r.method == Method::Get && r.url.contains("/v2/payments?"))
    }));
}

#[test]
fn bx_49_the_tor_setting_in_force_decides_whether_a_request_goes_through_the_proxy() {
    let runner = FakeRunner::default();
    let (mut pos, transport) = vendor_pos(&square_config(), &runner, MemFiles::new());
    // Taken as on until the settings say otherwise.
    pos.tick(TENDER_AT, token());
    assert_eq!(runner.seen()[0].socks5.as_deref(), Some("127.0.0.1:9050"));
    pos.set_tor(false);
    assert!(!transport.tor());
    pos.tick(TENDER_AT + VENDOR_POLL_SECS, token());
    assert_eq!(runner.seen()[1].socks5, None);
    pos.set_tor(true);
    pos.tick(TENDER_AT + 2 * VENDOR_POLL_SECS, token());
    assert_eq!(runner.seen()[2].socks5.as_deref(), Some("127.0.0.1:9050"));
    // The setup can name another proxy.
    let mut config = square_config();
    config.socks = Some("10.0.0.5:9150".into());
    let runner = FakeRunner::default();
    let (mut c, _) = connector(&config, &runner);
    c.poll(&Cursor::start()).unwrap();
    assert_eq!(runner.seen()[0].socks5.as_deref(), Some("10.0.0.5:9150"));
}

#[test]
fn bx_49_the_counters_on_the_device_list_serve_the_one_location() {
    let runner = FakeRunner::default();
    let (mut pos, _) = vendor_pos(&square_config(), &runner, MemFiles::new());
    // Counter 2 is on the list and was not in the settings the Box began with.
    let mut list: BTreeMap<DeviceId, DeviceRole> = roles();
    assert_eq!(pos.follow_counters(&list), None);
    runner.say(200, SQUARE_LAST);
    let draft = |state, at| NoticeDraft {
        index: 0,
        amount: openbmt_counter::Amount(700),
        currency: usd(),
        opened_at: TENDER_AT - 120,
        state,
        state_at: at,
    };
    pos.take_notices(
        &[
            item(&counter2(), 1, &draft(NoticeState::Opened, TENDER_AT - 120)),
            item(&counter2(), 2, &draft(NoticeState::Paid, TENDER_AT - 30)),
        ],
        &list,
        TENDER_AT,
    );
    pos.tick(TENDER_AT + 120, token());
    assert_eq!(pos.ledger().matches().len(), 1);
    // A list with too many counters is not taken, with one note.
    for n in 0..20u8 {
        let id = openbmt_core::identity::Identity::from_secret_bytes(&[n + 1; 32])
            .unwrap()
            .device_id();
        list.insert(id, DeviceRole::Counter);
    }
    let first = pos.follow_counters(&list);
    assert!(first.is_some_and(|n| n.starts_with("BX-49: ")));
    assert_eq!(pos.follow_counters(&list), None);
}

// ---- BX-49: the Box's own file ----

#[test]
fn bx_49_the_config_is_a_framed_file_read_strictly() {
    let mut config = clover_config();
    config.curl = Some("/usr/bin/curl".into());
    config.socks = Some("127.0.0.1:9150".into());
    config.host = Some("https://api.eu.clover.com".into());
    for c in [square_config(), clover_config(), config] {
        let bytes = c.to_bytes();
        assert_eq!(&bytes[..4], b"OBMq");
        assert_eq!(bytes[4], 1);
        assert_eq!(PosConfig::from_bytes(&bytes), Ok(c.clone()));
        // Every cut is refused, and so is a byte more.
        for n in 0..bytes.len() {
            assert!(
                matches!(
                    PosConfig::from_bytes(&bytes[..n]),
                    Err(PosConfigError::Damaged(_))
                ),
                "{n}"
            );
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(PosConfig::from_bytes(&longer).is_err());
        let mut wrong = bytes.clone();
        wrong[0] = b'X';
        assert!(PosConfig::from_bytes(&wrong).is_err());
        let mut version = bytes.clone();
        version[4] = 2;
        assert!(PosConfig::from_bytes(&version).is_err());
        let mut vendor = bytes.clone();
        vendor[5] = 3;
        assert!(PosConfig::from_bytes(&vendor).is_err());
    }
    // A value that is not valid does not read, though the frame is whole.
    let mut bad = square_config();
    bad.tenders = vec![" ".into()];
    assert!(PosConfig::from_bytes(&bad.to_bytes()).is_err());
    // Nothing of the token is in it.
    assert_eq!(Vendor::Square.word(), "square");
}

#[test]
fn bx_49_the_setup_line_makes_a_config_or_is_refused_with_words() {
    let c = PosConfig::parse(
        " vendor=square | location=LOCSQ1 | currency=USD | tenders=Bitcoin; BTC on-chain | curl=/usr/bin/curl | socks=127.0.0.1:9150 | host=https://connect.squareupsandbox.com ",
    )
    .unwrap();
    assert_eq!(c.vendor, Vendor::Square);
    assert_eq!(c.tenders, ["Bitcoin", "BTC on-chain"]);
    assert_eq!(c.curl_path(), "/usr/bin/curl");
    assert_eq!(c.socks_addr(), "127.0.0.1:9150");
    assert_eq!(c.host_url(), "https://connect.squareupsandbox.com");
    let d = square_config();
    assert_eq!(d.curl_path(), "curl");
    assert_eq!(d.socks_addr(), "127.0.0.1:9050");
    assert_eq!(d.host_url(), "https://connect.squareup.com");
    let line = "vendor=square|location=L1|currency=USD|tenders=Bitcoin";
    let cases: [(String, PosConfigError); 12] = [
        (
            line.replace("vendor=square|", ""),
            PosConfigError::Missing("vendor"),
        ),
        (
            line.replace("|tenders=Bitcoin", ""),
            PosConfigError::Missing("tenders"),
        ),
        (
            line.replace("square", "stripe"),
            PosConfigError::UnknownVendor("stripe".into()),
        ),
        (
            format!("{line}|colour=red"),
            PosConfigError::BadWord("colour=red".into()),
        ),
        (
            format!("{line}|currency=EUR"),
            PosConfigError::BadWord("currency=EUR".into()),
        ),
        (
            format!("{line}|nonsense"),
            PosConfigError::BadWord("nonsense".into()),
        ),
        (
            line.replace("USD", "usd"),
            PosConfigError::BadValue("currency"),
        ),
        (
            line.replace("Bitcoin", "a;b;c;d;e;f;g;h;i"),
            PosConfigError::BadValue("tenders"),
        ),
        (
            line.replace("Bitcoin", "Bitcoin; "),
            PosConfigError::BadValue("tenders"),
        ),
        (
            format!("{line}|host=http://x.example"),
            PosConfigError::BadValue("host"),
        ),
        (
            format!("{line}|socks=nocolon"),
            PosConfigError::BadValue("socks"),
        ),
        (
            "vendor=clover|location=not-alnum|currency=USD|tenders=Bitcoin".into(),
            PosConfigError::BadValue("location"),
        ),
    ];
    for (text, want) in cases {
        assert_eq!(PosConfig::parse(&text), Err(want), "{text}");
    }
    for e in [
        PosConfigError::Missing("vendor"),
        PosConfigError::BadWord("x".into()),
        PosConfigError::BadValue("curl"),
        PosConfigError::Damaged("cut".into()),
    ] {
        assert!(e.to_string().starts_with("BX-49: "));
        assert_eq!(e.words().code, Some("BX-49"));
    }
}

// ---- BX-49: the running Box ----

#[test]
fn bx_49_a_box_with_a_pos_config_reads_that_vendor_and_a_damaged_one_leaves_the_pos_off() {
    let dir = Dir::new("bx49-runner");
    let start = |dir: &Dir| {
        BoxRunner::start(
            &dir.0,
            "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
            Arc::new(SystemClock),
            openbmt_link::ServerConfig::default(),
        )
    };
    // No file: no POS.
    {
        let r = start(&dir).unwrap();
        assert_eq!(r.pos_view(), PosView::NotInstalled);
        // `pos-setup` stores the file, and says what it took.
        assert_eq!(
            r.store_pos_config("vendor=nope").unwrap_err(),
            PosConfigError::UnknownVendor("nope".into())
        );
        assert!(DirFiles::new(&dir.0)
            .read(POS_CONFIG_FILE)
            .unwrap()
            .is_none());
        let line = "vendor=square|location=LOCSQ1|currency=USD|tenders=Bitcoin";
        let stored = r.store_pos_config(line).unwrap();
        assert_eq!(stored, square_config());
        assert_eq!(
            DirFiles::new(&dir.0).mode(POS_CONFIG_FILE).unwrap(),
            Some(0o600)
        );
        r.stop();
    }
    // A file: the POS side is there, stopped because the Box is not paired.
    {
        let r = start(&dir).unwrap();
        match r.pos_view() {
            PosView::State(s) => assert_eq!(s.reading, Reading::Stopped),
            other => panic!("{other:?}"),
        }
        r.stop();
    }
    // A file that does not read: the POS is off, the relay runs on, and the
    // file is left as it is.
    {
        DirFiles::new(&dir.0)
            .write(POS_CONFIG_FILE, b"OBMq\x01garbage")
            .unwrap();
        let r = start(&dir).unwrap();
        assert!(matches!(
            r.pos_view(),
            PosView::Disabled(crate::pos::PosOpenError::Config(PosConfigError::Damaged(_)))
        ));
        assert!(r
            .notes()
            .iter()
            .any(|n| n.starts_with("the POS side is off: BX-49")));
        let words = r.pos_view().words(crate::state::BoxStatus::Unpaired);
        assert_eq!(words.heading, "Till link off: setup damaged");
        r.stop();
        assert_eq!(
            DirFiles::new(&dir.0)
                .read(POS_CONFIG_FILE)
                .unwrap()
                .unwrap(),
            b"OBMq\x01garbage"
        );
    }
}
