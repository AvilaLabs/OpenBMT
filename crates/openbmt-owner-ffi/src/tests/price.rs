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
