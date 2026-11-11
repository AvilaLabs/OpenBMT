//! OC-30 to OC-37: the owner's own matching of the till's orders, fed with
//! the fixtures of `openbmt-pos-readers` and with the sales of a counter's
//! records.

use std::fs;

use openbmt_core::identity::Identity;
use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::records::{Chain, DeviceList};
use openbmt_core::wire::DecodeError;
use openbmt_counter::{Amount, Currency, Event, RateSource};
use openbmt_courier::{Courier, FirstStart, MemoryPost, MemoryRelay};
use openbmt_link::Role;
use openbmt_pos::{
    AlertKey, AlertKind, FactsError, Ledger, MatchHow, RefCode, SaleFact, SettingsError,
};
use openbmt_pos_readers::{ReadRefused, RequestRefused};

use super::*;
use crate::pos::{
    counter_facts, OwnerPos, PosMethod, PosRefused, PosSetup, PosVendor, CLOVER_HOST,
    MAGIC_POS_SETUP, POS_LEDGER_FILE, POS_MIN_POLL_SECS, POS_SETUP_FILE, SQUARE_HOST,
};
use crate::{OwnerLink, OwnerLinkConfig};

const SQUARE_PAGE1: &[u8] =
    include_bytes!("../../../openbmt-pos-readers/tests/fixtures/square_orders_page1.json");
const SQUARE_LAST: &[u8] =
    include_bytes!("../../../openbmt-pos-readers/tests/fixtures/square_orders_last.json");
const CLOVER: &[u8] =
    include_bytes!("../../../openbmt-pos-readers/tests/fixtures/clover_orders.json");

/// Long after the last tender of the fixtures.
const NOW: u64 = 1_794_324_000;

fn usd() -> Currency {
    Currency::new(*b"USD").unwrap()
}

fn setup(vendor: PosVendor, location: &str) -> PosSetup {
    PosSetup::from_text(
        vendor,
        location,
        &["Bitcoin".to_string()],
        &["USD".to_string()],
    )
    .unwrap()
}

fn square() -> PosSetup {
    setup(PosVendor::Square, "LOCSQ1")
}

fn clover() -> PosSetup {
    setup(PosVendor::Clover, "MERCHANT1")
}

fn pos_in(dir: &Dir, setup: PosSetup) -> OwnerPos {
    let mut pos = OwnerPos::open(dir.path());
    pos.set_setup(setup).unwrap();
    pos
}

fn no_facts() -> Result<Vec<SaleFact>, PosRefused> {
    Ok(Vec::new())
}

fn sale(index: u32, cents: u64, opened_at: u64, paid_at: Option<u64>) -> SaleFact {
    let mut s = SaleFact::opened(counter(), index, Amount(cents), usd(), opened_at);
    s.paid_at = paid_at;
    s
}

/// One whole read of a single-page reply.
fn poll(
    pos: &mut OwnerPos,
    now: u64,
    body: &[u8],
    sales: Vec<SaleFact>,
) -> Result<crate::pos::PosPolled, PosRefused> {
    pos.request(now, &[counter()])?;
    pos.reply(200, body, now, || Ok(sales))
}

fn kinds(pos: &OwnerPos) -> Vec<AlertKind> {
    let mut v: Vec<AlertKind> = pos
        .view()
        .unwrap()
        .open
        .iter()
        .map(|a| a.key.kind)
        .collect();
    v.sort();
    v
}

/// The Clover fixture as it was before the refund of the second order.
fn without_refund(body: &[u8]) -> Vec<u8> {
    replace(
        body,
        r#"{ "id": "CLREFUND01", "payment": { "id": "CLPAY00004" }, "amount": 500, "createdTime": 1794319900000 }"#,
        "",
    )
}

fn replace(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains(from), "{from}");
    text.replace(from, to).into_bytes()
}

// ---- the counters' sales ---------------------------------------------------------

fn opened(index: u32, at: u64, cents: u64) -> Event {
    Event::Opened {
        index,
        at,
        amount: Amount(cents),
        currency: usd(),
        needs_sats: 1,
        minor_per_btc: 1,
        source: RateSource::new("test-fixed", at).unwrap(),
    }
}

fn at_once(index: u32, at: u64) -> Event {
    Event::PaidAtOnce {
        index,
        at,
        at_risk_sats: 1,
        label: InstantLabel::LightCheck,
        mode: CheckMode::Light,
    }
}

fn owner_id() -> DeviceId {
    id(1).device_id()
}

fn courier_for(dir: &Dir) -> Courier<MemoryPost> {
    let relay = MemoryRelay::new(id(99), &[owner_id(), counter()]).unwrap();
    Courier::start_with(
        relay.post(),
        id(1),
        Role::Owner,
        [(counter(), Role::Counter)].into_iter().collect(),
        dir.path(),
        FirstStart::Yes,
    )
    .unwrap()
}

/// A link whose copy of the counter's chain holds `events`, written to the
/// copy's file as the link writes it and read back (verified) by the link.
fn link_with(events: &[Event]) -> (OwnerLink<MemoryPost>, Dir, Dir) {
    let (root, courier_dir) = (Dir::new(), Dir::new());
    let mut chain = Chain::new();
    for e in events {
        chain.append(&id(3), e.kind(), e.encode()).unwrap();
    }
    let counter_dir = root.path().join("counters").join(
        counter()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    );
    fs::create_dir_all(&counter_dir).unwrap();
    let mut file = Vec::new();
    for r in chain.records() {
        let bytes: Vec<u8> = r.to_bytes();
        file.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        file.extend_from_slice(&bytes);
    }
    fs::write(counter_dir.join("records"), file).unwrap();
    let link = OwnerLink::new(
        courier_for(&courier_dir),
        root.path(),
        DeviceList::new([counter(), owner_id()]),
        vec![counter()],
        OwnerLinkConfig {
            poll_secs: 0,
            retry_secs: 0,
            ..OwnerLinkConfig::default()
        },
    )
    .unwrap();
    (link, root, courier_dir)
}

#[test]
fn oc_30_the_sales_are_folded_from_the_verified_copies_of_the_counters_records() {
    let (link, _a, _b) = link_with(&[
        opened(0, 1_794_319_300, 950),
        at_once(0, 1_794_319_380),
        opened(1, 1_794_319_500, 4_200),
    ]);
    let facts = counter_facts(&link).unwrap();
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].amount, Amount(950));
    assert_eq!(facts[0].paid_at, Some(1_794_319_380));
    assert_eq!(facts[1].paid_at, None);
    assert_eq!(facts[1].reference, Some(RefCode::for_sale(&counter(), 1)));
}

#[test]
fn oc_30_a_counter_whose_records_give_no_sales_gives_none_and_nothing_is_matched() {
    // A signed record for a sale that was never opened: the chain verifies
    // and the events do not fold.
    let (link, _a, _b) = link_with(&[opened(0, 1_794_319_300, 950), at_once(9, 1_794_319_380)]);
    let err = counter_facts(&link).unwrap_err();
    assert!(
        matches!(
            err,
            PosRefused::Facts {
                error: FactsError::UnknownSale { index: 9 },
                ..
            }
        ),
        "{err:?}"
    );
    assert!(err.to_string().starts_with("OC-30:"));
    // The last page of a read cannot be matched: the page stays stored,
    // nothing is matched and no alert is raised.
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    pos.request(NOW, &[counter()]).unwrap();
    let got = pos
        .reply(200, SQUARE_LAST, NOW, || counter_facts(&link))
        .unwrap_err();
    assert_eq!(got, err);
    assert_eq!(pos.ledger().orders().len(), 1);
    assert!(pos.ledger().matches().is_empty());
    assert!(pos.ledger().open_alerts().is_empty());
}

// ---- the settings ----------------------------------------------------------------

#[test]
fn oc_31_the_settings_are_stored_framed_and_read_back_strictly() {
    let s = PosSetup::from_text(
        PosVendor::Square,
        " LOCSQ1 ",
        &["Bitcoin".to_string(), " On-chain ".to_string()],
        &["USD".to_string(), "EUR".to_string()],
    )
    .unwrap();
    assert_eq!(s.location.as_str(), "LOCSQ1");
    let bytes = s.to_bytes();
    assert_eq!(&bytes[..4], &MAGIC_POS_SETUP);
    assert_eq!(bytes[4], 1);
    assert_eq!(PosSetup::from_bytes(&bytes), Ok(s));
    // Every cut is refused.
    for n in 0..bytes.len() {
        assert!(PosSetup::from_bytes(&bytes[..n]).is_err(), "cut at {n}");
    }
    // A byte left over, another magic, another version, an unknown vendor.
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(
        PosSetup::from_bytes(&longer),
        Err(DecodeError::TrailingBytes)
    );
    let mut other = bytes.clone();
    other[3] = b'p';
    assert_eq!(PosSetup::from_bytes(&other), Err(DecodeError::BadMagic));
    let mut version = bytes.clone();
    version[4] = 2;
    assert_eq!(
        PosSetup::from_bytes(&version),
        Err(DecodeError::UnsupportedVersion(2))
    );
    let mut vendor = bytes.clone();
    vendor[5] = 3;
    assert!(PosSetup::from_bytes(&vendor).is_err());
    // Whatever one changed byte gives, it is refused or it encodes back to
    // itself.
    for i in 0..bytes.len() {
        for flip in [1u8, 0x20, 0x80, 0xff] {
            let mut b = bytes.clone();
            b[i] ^= flip;
            if let Ok(read) = PosSetup::from_bytes(&b) {
                assert_eq!(read.to_bytes(), b, "byte {i}");
            }
        }
    }
}

#[test]
fn oc_31_settings_that_break_a_bound_are_refused_with_the_rule_that_says_so() {
    let ok = |names: &[&str], currencies: &[&str]| {
        PosSetup::from_text(
            PosVendor::Square,
            "L1",
            &names.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &currencies.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        ok(&[], &["USD"]),
        Err(PosRefused::Setup(SettingsError::TenderNames))
    );
    assert_eq!(
        ok(&["a"; 9], &["USD"]),
        Err(PosRefused::Setup(SettingsError::TenderNames))
    );
    assert_eq!(
        ok(&["  "], &["USD"]),
        Err(PosRefused::Model(openbmt_pos::ModelError::BadText))
    );
    for bad in [&[][..], &["usd"], &["US"], &["USD", "USD"], &["USDX"]] {
        assert_eq!(
            ok(&["Bitcoin"], bad),
            Err(PosRefused::Setup(SettingsError::Currencies)),
            "{bad:?}"
        );
    }
    assert!(PosSetup::from_text(PosVendor::Square, "", &["B".into()], &["USD".into()]).is_err());
    // Clover's merchant id is 1 to 32 letters and digits.
    let long = "A".repeat(33);
    for bad in ["has space", "under_score", long.as_str()] {
        assert_eq!(
            PosSetup::from_text(PosVendor::Clover, bad, &["B".into()], &["USD".into()]),
            Err(PosRefused::Request(RequestRefused::BadMerchant)),
            "{bad}"
        );
    }
    assert!(
        PosSetup::from_text(PosVendor::Clover, "ABC123", &["B".into()], &["USD".into()]).is_ok()
    );
}

#[test]
fn oc_31_the_settings_are_kept_between_runs_and_a_new_till_starts_an_empty_ledger() {
    let dir = Dir::new();
    let mut pos = OwnerPos::open(dir.path());
    assert!(pos.setup().is_none());
    assert_eq!(pos.request(NOW, &[counter()]), Err(PosRefused::NotSetUp));
    assert_eq!(
        PosRefused::NotSetUp.words().who,
        openbmt_core::say::Who::Owner
    );
    pos.set_setup(square()).unwrap();
    poll(&mut pos, NOW, SQUARE_LAST, no_facts().unwrap()).unwrap();
    assert_eq!(pos.ledger().orders().len(), 1);
    drop(pos);
    // Read back: the same settings and the same ledger.
    let mut pos = OwnerPos::open(dir.path());
    assert_eq!(pos.setup(), Some(&square()));
    assert_eq!(pos.ledger().orders().len(), 1);
    // Another name for the same till keeps the ledger (PS-15).
    let more = PosSetup::from_text(
        PosVendor::Square,
        "LOCSQ1",
        &["Bitcoin".into(), "BTC".into()],
        &["USD".into()],
    )
    .unwrap();
    pos.set_setup(more).unwrap();
    assert_eq!(pos.ledger().orders().len(), 1);
    // Another location, or another vendor, starts over.
    pos.set_setup(setup(PosVendor::Square, "LOCSQ2")).unwrap();
    assert_eq!(pos.ledger(), &Ledger::new());
    assert_eq!(
        Ledger::from_bytes(&fs::read(dir.path().join(POS_LEDGER_FILE)).unwrap()),
        Ok(Ledger::new())
    );
    // Remove: nothing is left.
    pos.remove().unwrap();
    assert!(pos.setup().is_none());
    assert!(!dir.path().join(POS_SETUP_FILE).exists());
    assert!(!dir.path().join(POS_LEDGER_FILE).exists());
    assert!(OwnerPos::open(dir.path()).setup().is_none());
}

// ---- what is stored --------------------------------------------------------------

#[test]
fn oc_32_a_file_that_does_not_read_back_leaves_the_till_unread_until_it_is_removed() {
    for (file, bytes) in [
        (POS_LEDGER_FILE, b"not a ledger".to_vec()),
        (POS_SETUP_FILE, b"OBMr\x01".to_vec()),
    ] {
        let dir = Dir::new();
        drop(pos_in(&dir, square()));
        fs::write(dir.path().join(file), bytes).unwrap();
        let mut pos = OwnerPos::open(dir.path());
        let err = pos.request(NOW, &[counter()]).unwrap_err();
        assert!(
            matches!(&err, PosRefused::Corrupt { file: f, .. } if *f == file),
            "{err:?}"
        );
        assert!(err.to_string().starts_with("OC-32:"));
        assert_eq!(pos.view(), Err(err.clone()));
        assert_eq!(pos.reply(200, b"", NOW, no_facts), Err(err.clone()));
        assert_eq!(pos.set_setup(square()), Err(err.clone()));
        assert_eq!(pos.acknowledge(&any_key(), NOW), Err(err));
        // The file was not replaced behind the owner's back.
        assert!(dir.path().join(file).exists());
        pos.remove().unwrap();
        pos.set_setup(square()).unwrap();
        assert!(pos.view().unwrap().open.is_empty());
    }
}

fn any_key() -> AlertKey {
    AlertKey {
        kind: AlertKind::BitcoinTip,
        subject: openbmt_pos::Subject::Tender(openbmt_pos::TenderKey {
            order: openbmt_pos::Id::new("o").unwrap(),
            tender: openbmt_pos::Id::new("t").unwrap(),
        }),
    }
}

#[test]
fn oc_32_a_ledger_that_cannot_be_stored_takes_no_effect() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    let before = pos.ledger().clone();
    pos.request(NOW, &[counter()]).unwrap();
    fs::remove_dir_all(dir.path()).unwrap();
    let err = pos.reply(200, SQUARE_LAST, NOW, no_facts).unwrap_err();
    assert!(matches!(err, PosRefused::Store(_)), "{err:?}");
    assert_eq!(pos.ledger(), &before);
    assert!(err.words().detail.is_some());
    // The settings too.
    let err = pos
        .set_setup(setup(PosVendor::Square, "LOCSQ2"))
        .unwrap_err();
    assert!(matches!(err, PosRefused::Store(_)), "{err:?}");
    assert_eq!(pos.setup(), Some(&square()));
}

// ---- the request -----------------------------------------------------------------

#[test]
fn oc_33_a_square_request_is_a_post_of_the_readers_body_to_squares_host() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    let r = pos.request(NOW, &[counter()]).unwrap();
    assert_eq!(r.method, PosMethod::Post);
    assert_eq!(r.url, format!("{SQUARE_HOST}/v2/orders/search"));
    let body = String::from_utf8(r.body).unwrap();
    assert!(body.contains("\"location_ids\":[\"LOCSQ1\"]"), "{body}");
    assert!(body.contains("\"limit\":100"), "{body}");
    assert!(body.contains("1970-01-01T00:00:00Z"), "{body}");
    assert!(!body.contains("cursor"), "{body}");
}

#[test]
fn oc_33_a_clover_request_is_a_get_with_the_merchant_and_no_body() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, clover());
    let r = pos.request(NOW, &[counter()]).unwrap();
    assert_eq!(r.method, PosMethod::Get);
    assert!(r.body.is_empty());
    assert!(
        r.url.starts_with(&format!(
            "{CLOVER_HOST}/v3/merchants/MERCHANT1/orders?filter="
        )),
        "{}",
        r.url
    );
    assert!(r.url.contains("&limit=100&offset=0&"), "{}", r.url);
}

#[test]
fn oc_33_the_request_never_carries_a_sign_in_and_needs_a_counter() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    assert_eq!(pos.request(NOW, &[]), Err(PosRefused::NoCounters));
    let too_many: Vec<DeviceId> = (1..=17).map(|n| id(n).device_id()).collect();
    assert_eq!(
        pos.request(NOW, &too_many),
        Err(PosRefused::Setup(SettingsError::Locations))
    );
    let r = pos.request(NOW, &[counter()]).unwrap();
    let text = format!("{} {}", r.url, String::from_utf8_lossy(&r.body)).to_lowercase();
    for word in ["bearer", "token", "authorization", "access"] {
        assert!(!text.contains(word), "{word}");
    }
}

#[test]
fn oc_33_only_a_200_is_read_and_each_failure_has_its_own_refusal() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    // No request, no answer.
    assert_eq!(
        pos.reply(200, SQUARE_LAST, NOW, no_facts),
        Err(PosRefused::NothingAsked)
    );
    for (status, want) in [
        (401, PosRefused::SignInRefused { status: 401 }),
        (403, PosRefused::SignInRefused { status: 403 }),
        (429, PosRefused::RateLimited),
        (500, PosRefused::Unavailable { status: 500 }),
        (204, PosRefused::Unavailable { status: 204 }),
        (302, PosRefused::Unavailable { status: 302 }),
    ] {
        pos.request(NOW, &[counter()]).unwrap();
        assert_eq!(pos.reply(status, SQUARE_LAST, NOW, no_facts), Err(want));
        assert!(pos.ledger().orders().is_empty(), "{status}");
        // The answer was used up: a second answer is not expected.
        assert_eq!(
            pos.reply(200, SQUARE_LAST, NOW, no_facts),
            Err(PosRefused::NothingAsked)
        );
        // A failed read does not stop the next one at once, but the minute
        // still counts from its start.
        assert!(matches!(
            pos.request(NOW, &[counter()]),
            Err(PosRefused::TooSoon { .. })
        ));
        // Forget the minute for the next case.
        let mut next = OwnerPos::open(dir.path());
        std::mem::swap(&mut pos, &mut next);
    }
    // The words of the refused sign-in tell how to make a new one.
    let w = PosRefused::SignInRefused { status: 401 }.words();
    assert!(w.next.contains("read-only sign-in"), "{}", w.next);
}

#[test]
fn oc_33_an_answer_that_cannot_be_read_changes_nothing() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    pos.request(NOW, &[counter()]).unwrap();
    let before = pos.ledger().clone();
    for (i, body) in [&b"not json"[..], b"{\"errors\":[{\"code\":\"X\"}]}", b"[]"]
        .into_iter()
        .enumerate()
    {
        let later = NOW + POS_MIN_POLL_SECS * 100 * (i as u64 + 1);
        pos.request(later, &[counter()]).unwrap();
        let got = pos.reply(200, body, NOW, no_facts);
        assert!(matches!(got, Err(PosRefused::Unreadable(_))), "{got:?}");
        assert_eq!(pos.ledger(), &before);
    }
    let e = PosRefused::Unreadable(ReadRefused::Malformed);
    assert!(e.to_string().starts_with("PS-90:"));
    assert_eq!(e.words(), ReadRefused::Malformed.words());
}

// ---- the polling limit ------------------------------------------------------------

#[test]
fn oc_34_a_read_starts_at_most_once_a_minute_and_its_pages_are_not_held_back() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    pos.request(NOW, &[counter()]).unwrap();
    let first = pos.reply(200, SQUARE_PAGE1, NOW, no_facts).unwrap();
    assert!(first.again && first.more_left);
    // The second page of the same read comes at once.
    pos.request(NOW, &[counter()]).unwrap();
    let last = pos.reply(200, SQUARE_LAST, NOW, no_facts).unwrap();
    assert!(!last.again);
    // A new read waits for the minute that began with the first.
    assert_eq!(
        pos.request(NOW + 59, &[counter()]),
        Err(PosRefused::TooSoon { wait_secs: 1 })
    );
    assert!(pos.request(NOW + POS_MIN_POLL_SECS, &[counter()]).is_ok());
    let w = PosRefused::TooSoon { wait_secs: 3 }.words();
    assert_eq!(w.who, openbmt_core::say::Who::Nobody);
}

#[test]
fn oc_34_a_read_takes_at_most_64_pages_and_the_next_read_goes_on() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    let mut sales_asked = 0;
    for n in 1..=64 {
        pos.request(NOW, &[counter()]).unwrap();
        let body = format!("{{\"orders\":[],\"cursor\":\"c{n}\"}}");
        let polled = pos
            .reply(200, body.as_bytes(), NOW, || {
                sales_asked += 1;
                Ok(Vec::new())
            })
            .unwrap();
        assert_eq!(polled.pages, n);
        assert_eq!(polled.again, n < 64, "page {n}");
        assert!(polled.more_left);
    }
    // The matching ran once, at the 64th page, and the cursor holds on.
    assert_eq!(sales_asked, 1);
    let r = pos.request(NOW + POS_MIN_POLL_SECS, &[counter()]).unwrap();
    assert!(String::from_utf8(r.body)
        .unwrap()
        .contains("\"cursor\":\"c64\""));
}

#[test]
fn oc_34_a_page_that_says_more_and_does_not_move_on_ends_the_read() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    pos.request(NOW, &[counter()]).unwrap();
    pos.reply(200, br#"{"orders":[],"cursor":"X"}"#, NOW, no_facts)
        .unwrap();
    pos.request(NOW, &[counter()]).unwrap();
    assert_eq!(
        pos.reply(200, br#"{"orders":[],"cursor":"X"}"#, NOW, no_facts),
        Err(PosRefused::NoProgress)
    );
}

// ---- the matching -----------------------------------------------------------------

#[test]
fn oc_35_square_fixtures_and_the_counters_records_give_a_match_and_three_alerts() {
    let (link, _a, _b) = link_with(&[
        // 9.50 paid at once; the order's Bitcoin tender is 9.50 and a tip.
        opened(0, 1_794_319_300, 950),
        at_once(0, 1_794_319_380),
        // 42.00 paid and no order at the till.
        opened(1, 1_794_319_500, 4_200),
        at_once(1, 1_794_319_600),
    ]);
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    // Page 1: the order with the Bitcoin tender and its tip.
    pos.request(NOW, &[counter()]).unwrap();
    let first = pos
        .reply(200, SQUARE_PAGE1, NOW, || counter_facts(&link))
        .unwrap();
    assert!(first.again && first.delta.new_matches.is_empty());
    // The second request carries Square's marker.
    let second = pos.request(NOW, &[counter()]).unwrap();
    assert!(String::from_utf8(second.body)
        .unwrap()
        .contains("PAGE2OPAQUE-cursor_value.1"));
    let done = pos
        .reply(200, SQUARE_LAST, NOW, || counter_facts(&link))
        .unwrap();
    assert!(!done.again && !done.more_left);
    assert_eq!((done.pages, done.orders_read), (2, 4));
    // One match, by amount and time, to the first sale.
    assert_eq!(done.delta.new_matches.len(), 1);
    let (key, m) = &done.delta.new_matches[0];
    assert_eq!(key.tender.as_str(), "TENDERA02");
    assert_eq!(m.how, MatchHow::AmountAndTime);
    assert_eq!(m.sale.index, 0);
    // Three alerts: the tip, the bitcoin tender nobody paid and the payment
    // nobody rang.
    assert_eq!(
        kinds(&pos),
        vec![
            AlertKind::TenderWithoutPayment,
            AlertKind::PaymentWithoutTender,
            AlertKind::BitcoinTip
        ]
    );
    let view = pos.view().unwrap();
    assert_eq!(view.last_poll_at, Some(NOW));
    assert_eq!(view.summary.matches, 1);
    assert_eq!(view.summary.open_alerts, 3);
    for a in &view.open {
        let w = &a.words;
        assert!(!w.what.is_empty() && !w.next.is_empty());
        assert!(openbmt_core::say::check(w).is_ok(), "{w:?}");
    }
    // The ledger on disk is what the phone shows after a restart.
    let again = OwnerPos::open(dir.path());
    assert_eq!(again.ledger(), pos.ledger());
    assert_eq!(again.view().unwrap().open, view.open);
    // Reading the same pages again raises nothing again (PS-74).
    let mut pos = again;
    pos.request(NOW + POS_MIN_POLL_SECS, &[counter()]).unwrap();
    let repeat = pos
        .reply(200, SQUARE_LAST, NOW + POS_MIN_POLL_SECS, || {
            counter_facts(&link)
        })
        .unwrap();
    assert!(repeat.delta.new_alerts.is_empty() && repeat.delta.new_matches.is_empty());
    assert_eq!(pos.view().unwrap().open, view.open);
}

#[test]
fn oc_35_clover_fixtures_match_by_reference_and_by_amount_and_raise_a_tip_and_a_refund() {
    let sa = RefCode::for_sale(&counter(), 7);
    let body = replace(CLOVER, "K7M2QX", sa.as_str());
    let dir = Dir::new();
    let mut pos = pos_in(&dir, clover());
    // First the orders as the till has them before the refund.
    let sales = || {
        vec![
            sale(7, 1_100, 1_794_319_100, Some(1_794_319_300)),
            sale(8, 1_900, 1_794_319_700, Some(1_794_319_800)),
        ]
    };
    let polled = poll(&mut pos, 1_794_321_000, &without_refund(&body), sales()).unwrap();
    assert!(!polled.again);
    let mut hows: Vec<(String, MatchHow, u32)> = polled
        .delta
        .new_matches
        .iter()
        .map(|(k, m)| (k.tender.as_str().to_string(), m.how, m.sale.index))
        .collect();
    hows.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        hows,
        vec![
            ("CLPAY00002".to_string(), MatchHow::Reference, 7),
            ("CLPAY00004".to_string(), MatchHow::AmountAndTime, 8),
        ]
    );
    // The tip is an alert at once.
    assert_eq!(kinds(&pos), vec![AlertKind::BitcoinTip]);
    // Then the till shows the refund of the tender the paid sale rests on.
    poll(&mut pos, 1_794_321_000 + POS_MIN_POLL_SECS, &body, sales()).unwrap();
    // The refunded tender is no longer a candidate, so its tip alert goes
    // with it (PS-12); the match and the sale stay (PS-36).
    assert_eq!(kinds(&pos), vec![AlertKind::VoidedAfterPayment]);
    assert_eq!(pos.ledger().matches().len(), 2, "a refund keeps the match");
}

#[test]
fn oc_35_a_reference_to_a_sale_of_another_amount_is_an_alert_and_claims_the_sale() {
    let sa = RefCode::for_sale(&counter(), 7);
    let body = replace(CLOVER, "K7M2QX", sa.as_str());
    let dir = Dir::new();
    let mut pos = pos_in(&dir, clover());
    poll(
        &mut pos,
        1_794_321_000,
        &body,
        vec![sale(7, 1_000, 1_794_319_100, Some(1_794_319_300))],
    )
    .unwrap();
    let k = kinds(&pos);
    assert!(k.contains(&AlertKind::AmountDiffers), "{k:?}");
    assert!(!k.contains(&AlertKind::PaymentWithoutTender), "{k:?}");
    assert!(pos.ledger().matches().is_empty());
}

#[test]
fn oc_35_two_tenders_naming_one_sale_raise_a_duplicate_on_the_later() {
    let sa = RefCode::for_sale(&counter(), 7);
    let body = replace(CLOVER, "K7M2QX", sa.as_str());
    let body = replace(
        &body,
        "\"tipAmount\": 100,",
        &format!("\"tipAmount\": 100, \"note\": \"{}\",", sa.as_str()),
    );
    let body = replace(&body, "\"amount\": 1900,", "\"amount\": 1100,");
    let body = without_refund(&body);
    let dir = Dir::new();
    let mut pos = pos_in(&dir, clover());
    poll(
        &mut pos,
        1_794_321_000,
        &body,
        vec![sale(7, 1_100, 1_794_319_100, Some(1_794_319_300))],
    )
    .unwrap();
    let dup: Vec<_> = pos
        .view()
        .unwrap()
        .open
        .into_iter()
        .filter(|a| a.key.kind == AlertKind::DuplicateReference)
        .collect();
    assert_eq!(dup.len(), 1);
    match &dup[0].key.subject {
        openbmt_pos::Subject::Tender(t) => assert_eq!(t.tender.as_str(), "CLPAY00004"),
        other => panic!("{other:?}"),
    }
    assert_eq!(pos.ledger().matches().len(), 1);
}

#[test]
fn oc_35_a_sale_paid_after_the_order_closed_and_a_sale_reported_unpaid_raise_alerts() {
    let sa = RefCode::for_sale(&counter(), 7);
    let body = replace(CLOVER, "K7M2QX", sa.as_str());
    // Paid 379 seconds after the tender was taken.
    let dir = Dir::new();
    let mut pos = pos_in(&dir, clover());
    poll(
        &mut pos,
        1_794_321_000,
        &body,
        vec![sale(7, 1_100, 1_794_319_100, Some(1_794_319_800))],
    )
    .unwrap();
    assert!(kinds(&pos).contains(&AlertKind::ClosedBeforePaid));
    // Reported paid and then unpaid again.
    let dir = Dir::new();
    let mut pos = pos_in(&dir, clover());
    let mut unpaid = sale(7, 1_100, 1_794_319_100, Some(1_794_319_300));
    unpaid.unpaid_after = true;
    poll(&mut pos, 1_794_321_000, &body, vec![unpaid]).unwrap();
    assert!(kinds(&pos).contains(&AlertKind::TenderOnUnpaidSale));
}

#[test]
fn oc_35_a_bitcoin_tender_in_a_currency_the_shop_does_not_use_raises_an_alert() {
    let dir = Dir::new();
    let eur = PosSetup::from_text(
        PosVendor::Clover,
        "MERCHANT1",
        &["Bitcoin".into()],
        &["EUR".into()],
    )
    .unwrap();
    let mut pos = pos_in(&dir, eur);
    poll(&mut pos, 1_794_321_000, CLOVER, no_facts().unwrap()).unwrap();
    assert!(kinds(&pos).contains(&AlertKind::UnknownCurrency));
    assert!(pos.ledger().matches().is_empty());
}

// ---- acknowledging ----------------------------------------------------------------

#[test]
fn oc_36_an_alert_is_acknowledged_on_this_phone_and_stays_acknowledged() {
    let dir = Dir::new();
    let mut pos = pos_in(&dir, square());
    let sales = vec![sale(1, 4_200, 1_794_319_500, Some(1_794_319_600))];
    poll(&mut pos, NOW, SQUARE_LAST, sales.clone()).unwrap();
    let open = pos.view().unwrap().open;
    let payment = open
        .iter()
        .find(|a| a.key.kind == AlertKind::PaymentWithoutTender)
        .unwrap()
        .key
        .clone();
    assert!(open.len() >= 2);
    pos.acknowledge(&payment, NOW + 5).unwrap();
    let view = pos.view().unwrap();
    assert_eq!(view.summary.acknowledged_alerts, 1);
    assert!(view.open.iter().all(|a| a.key != payment));
    // It is not raised again while the condition holds.
    pos.request(NOW + POS_MIN_POLL_SECS, &[counter()]).unwrap();
    let again = pos
        .reply(200, SQUARE_LAST, NOW + POS_MIN_POLL_SECS, || Ok(sales))
        .unwrap();
    assert!(again.delta.new_alerts.iter().all(|a| a.key != payment));
    assert_eq!(pos.view().unwrap().summary.acknowledged_alerts, 1);
    // Stored: it is still acknowledged after a restart.
    assert_eq!(
        OwnerPos::open(dir.path())
            .view()
            .unwrap()
            .summary
            .acknowledged_alerts,
        1
    );
    // An alert that is not open cannot be acknowledged.
    let err = pos.acknowledge(&payment, NOW + 9).unwrap_err();
    assert_eq!(err, PosRefused::Ledger(openbmt_pos::LedgerError::NotOpen));
    assert!(err.to_string().starts_with("PS-52:"));
}

// ---- the words --------------------------------------------------------------------

#[test]
fn oc_37_every_refusal_has_words_and_the_wrapped_ones_have_their_own() {
    use openbmt_core::say::check;
    let id = Identity::from_secret_bytes(&[3; 32]).unwrap().device_id();
    let all = [
        PosRefused::NotSetUp,
        PosRefused::NoCounters,
        PosRefused::TooSoon { wait_secs: 5 },
        PosRefused::NothingAsked,
        PosRefused::SignInRefused { status: 401 },
        PosRefused::RateLimited,
        PosRefused::Unavailable { status: 503 },
        PosRefused::Unreadable(ReadRefused::TooLong),
        PosRefused::Request(RequestRefused::BadMerchant),
        PosRefused::NoProgress,
        PosRefused::Setup(SettingsError::Currencies),
        PosRefused::Model(openbmt_pos::ModelError::BadText),
        PosRefused::Ledger(openbmt_pos::LedgerError::NotOpen),
        PosRefused::Facts {
            counter: id,
            error: FactsError::UnknownSale { index: 1 },
        },
        PosRefused::Store("disk\nfull".to_string()),
        PosRefused::Corrupt {
            file: POS_LEDGER_FILE,
            why: "bad".to_string(),
        },
    ];
    for e in &all {
        let w = e.words();
        assert_eq!(check(&w), Ok(()), "{e}: {w:?}");
        assert!(!e.to_string().is_empty());
    }
    assert_eq!(
        PosRefused::Facts {
            counter: id,
            error: FactsError::UnknownSale { index: 1 }
        }
        .words(),
        FactsError::UnknownSale { index: 1 }.words()
    );
}
