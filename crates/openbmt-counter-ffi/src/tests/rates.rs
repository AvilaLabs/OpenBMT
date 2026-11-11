//! The price ops (40 to 42) through `dispatch`: they work on no node, take
//! the real replies of the sources and give refusals with the words of
//! `openbmt-rates`.

use openbmt_rates::SOURCES;

use super::kit::{record, record_with, signed_settings};
use super::screen::check_wire_words;
use super::wire::{frame, Rd, W};
use crate::{dispatch, Registry};

const KRAKEN: &[u8] = include_bytes!("../../../openbmt-rates/tests/fixtures/kraken.json");
const COINBASE: &[u8] = include_bytes!("../../../openbmt-rates/tests/fixtures/coinbase.json");
const NOW: u64 = 1_800_000_000;

fn call(op: u16, fields: &[u8]) -> Vec<u8> {
    dispatch(&Registry::new(), &frame(op, 0, fields))
}

fn parse_fields(source: &str, body: &[u8], at: u64) -> Vec<u8> {
    W::new().string(source).bytes(body).u64(at).0
}

fn sample_fields(list: &[(&str, u64, u64)], now: u64) -> Vec<u8> {
    let mut w = W::new().u32(u32::try_from(list.len()).expect("fits"));
    for (source, at, price) in list {
        w = w.string(source).u64(*at).u64(*price);
    }
    w.u64(now).0
}

fn sample(r: &mut Rd<'_>) -> (String, u64, u64) {
    (r.string(), r.u64(), r.u64())
}

#[test]
fn rt_01_the_table_of_sources_is_handed_to_the_app() {
    let response = call(40, &[]);
    let mut r = Rd::ok(&response);
    let rows = r.list(|r| (r.string(), r.string(), r.string(), r.string()));
    r.done();
    let table: Vec<(String, String, String, String)> = SOURCES
        .iter()
        .map(|s| {
            (
                s.id.to_string(),
                s.url.to_string(),
                s.currency.to_string(),
                s.initial.to_string(),
            )
        })
        .collect();
    assert_eq!(rows, table);
    assert!(rows
        .iter()
        .all(|(_, url, _, _)| url.starts_with("https://")));
}

#[test]
fn rt_45_a_real_reply_is_read_into_a_sample_with_the_apps_time() {
    let response = call(41, &parse_fields("kraken", KRAKEN, NOW));
    let mut r = Rd::ok(&response);
    assert_eq!(sample(&mut r), ("kraken".to_string(), NOW, 7_355_450));
    r.done();
    let response = call(41, &parse_fields("coinbase", COINBASE, NOW + 3));
    let mut r = Rd::ok(&response);
    assert_eq!(sample(&mut r), ("coinbase".to_string(), NOW + 3, 7_355_218));
    r.done();
}

#[test]
fn rt_44_a_reply_that_is_refused_comes_back_with_its_rule_and_words() {
    // Coinbase's reply read as Kraken's: the field it needs is missing.
    let response = call(41, &parse_fields("kraken", COINBASE, NOW));
    let e = Rd::error(&response).expect("a refusal");
    assert_eq!((e.status, e.rule.as_str()), (1, "RT-15"));
    assert!(e.text.starts_with("RT-15"));
    check_wire_words(&e.words).expect("the words pass the rules");
    let response = call(41, &parse_fields("kraken", b"<html>", NOW));
    let e = Rd::error(&response).expect("a refusal");
    assert_eq!(e.rule, "RT-13");
    // Over the limit of RT-10.
    let big = vec![b' '; 64 * 1024 + 1];
    let response = call(41, &parse_fields("kraken", &big, NOW));
    assert_eq!(Rd::error(&response).expect("a refusal").rule, "RT-10");
}

#[test]
fn rt_01_a_source_that_is_not_in_the_table_is_refused() {
    let response = call(41, &parse_fields("nobody", KRAKEN, NOW));
    let e = Rd::error(&response).expect("a refusal");
    assert_eq!(e.rule, "rate-source");
    check_wire_words(&e.words).expect("the words pass the rules");
    let response = call(42, &sample_fields(&[("nobody", NOW, 7_000_000)], NOW));
    assert_eq!(Rd::error(&response).expect("a refusal").rule, "rate-source");
}

#[test]
fn rt_35_the_samples_make_one_rate_with_its_label_and_oldest_time() {
    let samples = [
        ("kraken", NOW - 30, 7_355_450),
        ("coinbase", NOW - 10, 7_355_218),
        ("bitstamp", NOW - 90, 7_355_001),
    ];
    let response = call(42, &sample_fields(&samples, NOW));
    let mut r = Rd::ok(&response);
    let (rate, label, at) = (r.u64(), r.string(), r.u64());
    r.done();
    assert_eq!(rate, 7_355_218);
    assert_eq!(label, "med.kcb");
    assert_eq!(at, NOW - 90);
}

#[test]
fn rt_37_rt_31_rt_33_a_rate_that_cannot_be_made_is_refused_with_words() {
    let refused = |list: &[(&str, u64, u64)]| {
        let response = call(42, &sample_fields(list, NOW));
        let e = Rd::error(&response).expect("a refusal");
        check_wire_words(&e.words).expect("the words pass the rules");
        assert!(e.text.starts_with(&e.rule));
        e.rule
    };
    assert_eq!(refused(&[("kraken", NOW - 900, 7_355_450)]), "RT-37");
    assert_eq!(refused(&[("kraken", NOW, 7_355_450)]), "RT-31");
    assert_eq!(refused(&[]), "RT-31");
    assert_eq!(
        refused(&[("kraken", NOW, 7_000_000), ("coinbase", NOW, 7_500_000)]),
        "RT-33"
    );
    // A price outside the band is not a sample.
    assert_eq!(
        refused(&[("kraken", NOW, 5), ("coinbase", NOW, 5)]),
        "RT-24"
    );
}

#[test]
fn rt_30_no_more_samples_than_the_limit_are_taken() {
    let many = vec![("kraken", NOW, 7_355_450); crate::rates::MAX_SAMPLES + 1];
    let response = call(42, &sample_fields(&many, NOW));
    assert_eq!(
        Rd::error(&response).expect("a refusal").rule,
        "rate-samples"
    );
    let ok = vec![("kraken", NOW, 7_355_450); crate::rates::MAX_SAMPLES];
    // The same source many times is one source: not enough, but read.
    let response = call(42, &sample_fields(&ok, NOW));
    assert_eq!(Rd::error(&response).expect("a refusal").rule, "RT-31");
}

#[test]
fn the_price_ops_work_on_no_node() {
    let response = dispatch(&Registry::new(), &frame(40, 3, &[]));
    assert_eq!(
        Rd::error(&response).expect("a refusal").rule,
        "handle-unexpected"
    );
}

#[test]
fn nr_26_the_pairing_is_read_before_the_node_starts() {
    let mut signed = signed_settings();
    let response = call(
        43,
        &W::new()
            .bytes(&record_with(signed.clone()).to_bytes())
            .string("")
            .0,
    );
    let mut r = Rd::ok(&response);
    // The network tag of regtest, then the settings.
    assert_eq!(r.u8(), 3);
    assert_eq!((r.u32(), r.bool(), r.bool(), r.u64()), (1, false, false, 0));
    // The record holds no relay.
    assert_eq!(r.u8(), 0);
    r.done();
    // Tor on and a floor: read as they are. The signatures are not checked
    // here; `start` checks them on the same bytes.
    signed.settings.tor_on = true;
    signed.settings.min_sale_sats = 26_000;
    let response = call(
        43,
        &W::new().bytes(&record_with(signed).to_bytes()).string("").0,
    );
    let mut r = Rd::ok(&response);
    assert_eq!(r.u8(), 3);
    assert_eq!(
        (r.u32(), r.bool(), r.bool(), r.u64()),
        (1, true, false, 26_000)
    );
    assert_eq!(r.u8(), 0);
    r.done();
}

#[test]
fn a_record_that_is_not_a_record_is_refused_with_words() {
    let bytes = record().to_bytes();
    for cut in [0, 4, bytes.len() / 2, bytes.len() - 1] {
        let response = call(43, &W::new().bytes(&bytes[..cut]).string("").0);
        let e = Rd::error(&response).expect("a refusal");
        assert_eq!(e.rule, "CR-97", "cut at {cut}");
        check_wire_words(&e.words).expect("the words pass the rules");
    }
    let mut more = bytes;
    more.push(0);
    let response = call(43, &W::new().bytes(&more).string("").0);
    assert_eq!(Rd::error(&response).expect("a refusal").rule, "CR-97");
}
