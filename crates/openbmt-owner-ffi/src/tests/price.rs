//! The price of a bitcoin through `dispatch` (OA-101): ops 140 and 141 work on
//! no node and give the rates library's own answers and words.

use super::kit::*;
use super::screen::check_wire_words;
use super::wire::{Fault, Rd, W};

const NOW: u64 = 1_700_000_000;

fn reply(source: &str, body: &str, at: u64) -> W {
    W::new().string(source).bytes(body.as_bytes()).u64(at)
}

fn last(source: &str, price: &str) -> W {
    reply(source, &format!(r#"{{"last":"{price}"}}"#), NOW)
}

fn request(replies: Vec<W>, now: u64) -> Vec<u8> {
    let mut w = W::new().u32(u32::try_from(replies.len()).expect("few"));
    for r in replies {
        w.0.extend_from_slice(&r.0);
    }
    w.u64(now).0
}

fn fault(response: &[u8]) -> Fault {
    let f = Rd::error(response).expect("an error response");
    check_wire_words(&f.words).unwrap_or_else(|why| panic!("{why}: {f:?}"));
    f
}

#[test]
fn oa_101_the_table_of_sources_is_the_librarys() {
    let rig = Rig::new();
    let response = rig.call(140, 0, &[]);
    let mut r = Rd::ok(&response);
    let rows = r.list(|r| (r.string(), r.string()));
    r.done();
    let ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["kraken", "coinbase", "bitstamp", "gemini"]);
    assert!(rows.iter().all(|(_, url)| url.starts_with("https://")));
}

#[test]
fn oa_101_the_replies_that_agree_give_the_price_in_whole_dollars() {
    let rig = Rig::new();
    let response = rig.call(
        141,
        0,
        &request(
            vec![last("bitstamp", "98500.50"), last("gemini", "98500.50")],
            NOW,
        ),
    );
    let mut r = Rd::ok(&response);
    assert_eq!(r.u64(), 98_501);
    assert_eq!(r.u64(), 9_850_050);
    assert_eq!(r.string(), "med.bg");
    r.done();
}

#[test]
fn oa_101_a_refusal_of_the_library_carries_its_words() {
    let rig = Rig::new();
    let response = rig.call(
        141,
        0,
        &request(
            vec![last("bitstamp", "98000"), last("gemini", "110000")],
            NOW,
        ),
    );
    let f = fault(&response);
    assert_eq!((f.status, f.rule.as_str()), (1, "RT-33"));
    assert!(f.words.what.contains("sources"), "{:?}", f.words);
    let response = rig.call(141, 0, &request(vec![last("bitstamp", "98000")], NOW));
    let f = fault(&response);
    assert_eq!((f.status, f.rule.as_str()), (1, "RT-31"));
    let response = rig.call(141, 0, &request(vec![], NOW));
    assert_eq!(fault(&response).rule, "RT-31");
}

#[test]
fn oa_101_a_source_not_in_the_table_is_a_missing_reply_and_a_bad_request_is_refused() {
    let rig = Rig::new();
    let response = rig.call(141, 0, &request(vec![reply("nowhere", "{}", NOW)], NOW));
    let f = fault(&response);
    assert_eq!((f.status, f.rule.as_str()), (1, "RT-31"));
    // The request is read completely: bytes left over are refused.
    let mut extra = request(vec![], NOW);
    extra.push(0);
    assert_eq!(
        Rd::error(&rig.call(141, 0, &extra))
            .expect("refused")
            .status,
        1
    );
    // These ops name no node: a handle is refused.
    assert_eq!(
        Rd::error(&rig.call(140, 7, &[])).expect("refused").rule,
        "handle-unexpected"
    );
}

fn request_for(currency: &str, replies: Vec<W>, now: u64) -> Vec<u8> {
    let mut w = W::new()
        .string(currency)
        .u32(u32::try_from(replies.len()).expect("few"));
    for r in replies {
        w.0.extend_from_slice(&r.0);
    }
    w.u64(now).0
}

#[test]
fn oa_102_the_sources_of_the_shops_currency_are_the_librarys_and_sats_has_none() {
    let rig = Rig::new();
    let ids = |code: &str| {
        let response = rig.call(142, 0, &W::new().string(code).0);
        let mut r = Rd::ok(&response);
        let rows = r.list(|r| (r.string(), r.string()));
        r.done();
        rows.into_iter().map(|(id, _)| id).collect::<Vec<_>>()
    };
    assert_eq!(ids("USD"), ["kraken", "coinbase", "bitstamp", "gemini"]);
    let eur = ids("EUR");
    assert!(!eur.is_empty() && eur.iter().all(|id| !ids("USD").contains(id)));
    for bad in ["SAT", "usd", "XXX", ""] {
        let f = Rd::error(&rig.call(142, 0, &W::new().string(bad).0)).expect("refused");
        assert_eq!(
            (f.status, f.rule.as_str()),
            (1, "request-malformed"),
            "{bad:?}"
        );
    }
    // The call names no node.
    assert_eq!(
        Rd::error(&rig.call(142, 7, &W::new().string("USD").0))
            .expect("refused")
            .rule,
        "handle-unexpected"
    );
}

#[test]
fn oa_102_the_price_is_given_in_whole_units_and_in_minor_units_of_the_currency() {
    let rig = Rig::new();
    let ask =
        |currency: &str, replies: Vec<W>| rig.call(143, 0, &request_for(currency, replies, NOW));
    // Dollars: the same agreement as the dollar op.
    let response = ask(
        "USD",
        vec![last("bitstamp", "98500.50"), last("gemini", "98500.50")],
    );
    let mut r = Rd::ok(&response);
    assert_eq!((r.u64(), r.u64()), (98_501, 9_850_050));
    assert_eq!(r.string(), "med.bg");
    r.done();
    // A reply of a source of another currency is not a sample of this one.
    let f = Rd::error(&ask(
        "EUR",
        vec![last("bitstamp", "98500.50"), last("gemini", "98500.50")],
    ))
    .expect("refused");
    assert_eq!((f.status, f.rule.as_str()), (1, "RT-31"));
    // Bitcoin Standard asks nothing.
    let f = Rd::error(&ask("SAT", vec![])).expect("refused");
    assert_eq!(f.rule, "request-malformed");
}
