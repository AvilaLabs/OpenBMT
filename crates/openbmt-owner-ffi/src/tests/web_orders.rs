//! The ops of the owner phone's web orders through `dispatch` (ops 227 to
//! 233, `docs/WEB-ORDER-RULES.md`): the pairing code, the store's state, the
//! visits' steps and the answers handed back.

use openbmt_owner::web_order::PairingCode;

use super::kit::Rig;
use super::requests::{ask, settle, wallet_file, T0};
use super::wire::{Rd, W};

const URL: &str = "https://shop.example";

fn pair_args(url: &str, name: &str) -> Vec<u8> {
    ask(T0).string(url).string(name).0
}

/// The state view's first fields, and its counts.
struct State {
    off: Option<String>,
    pairing: u8,
    code: Option<String>,
    fingerprint: String,
    failure: Option<String>,
    sent: u32,
    left: Option<u64>,
}

fn state(r: &mut Rd<'_>) -> State {
    let off = r.opt(|r| r.words().heading);
    let pairing = r.u8();
    let (_shop, _url, fingerprint) = (r.string(), r.string(), r.string());
    let code = r.opt(|r| r.string());
    let _last_visit = r.opt(|r| r.u64());
    let failure = r.opt(|r| r.words().heading);
    let _retry = r.opt(|r| r.u64());
    let left = r.opt(|r| r.u64());
    let sent = r.u32();
    let _trouble = r.opt(|r| r.words().heading);
    let _counts = (r.u32(), r.u32(), r.u32(), r.u32());
    let _refused = r.list(|r| (r.u64(), r.string(), r.u64(), r.u64(), r.words().heading));
    State {
        off,
        pairing,
        code,
        fingerprint,
        failure,
        sent,
        left,
    }
}

/// What a step gives: its kind, and the call for a call.
struct Step {
    kind: u8,
    call: Option<(u8, String, Vec<(String, String)>, String, Vec<u8>)>,
}

fn step(r: &mut Rd<'_>) -> Step {
    let kind = r.u8();
    let call = r.opt(|r| {
        (
            r.u8(),
            r.string(),
            r.list(|r| (r.string(), r.string())),
            r.string(),
            r.bytes(),
        )
    });
    Step { kind, call }
}

#[test]
fn wo_05_a_pairing_code_is_read_back_with_what_the_owner_compares() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let response = rig.call(227, handle, &pair_args(URL, "Corner Shop"));
    let mut r = Rd::ok(&response);
    let (code, fingerprint, shop, url) = (r.string(), r.string(), r.string(), r.string());
    r.done();
    assert_eq!((shop.as_str(), url.as_str()), ("Corner Shop", URL));
    // The code is the one the plugin reads, and op 233 shows what to compare.
    assert_eq!(
        PairingCode::from_text(&code).expect("a code").fingerprint(),
        fingerprint
    );
    let response = rig.call(233, 0, &W::new().string(&code).0);
    let mut r = Rd::ok(&response);
    assert_eq!((r.string(), r.string()), (fingerprint.clone(), shop));
    r.done();
    // A code that is not one is refused with the rule's placeholder words.
    let mut bent = code.clone();
    bent.replace_range(10..11, if &code[10..11] == "A" { "B" } else { "A" });
    let fault = Rd::error(&rig.call(233, 0, &W::new().string(&bent).0)).expect("refused");
    assert_eq!(fault.rule, "WO-05");
    assert_eq!(fault.words.heading, "WORDS_NEEDED_WO_05_HEADING");
    // The state waits for the owner's word, and shows the code again.
    let response = rig.call(230, handle, &ask(T0).0);
    let state = state(&mut Rd::ok(&response));
    assert_eq!(
        (state.pairing, state.code.as_deref()),
        (1, Some(code.as_str()))
    );
    assert_eq!(state.fingerprint, fingerprint);
}

#[test]
fn rq_01_a_shop_with_a_key_app_has_no_web_orders() {
    let rig = Rig::new();
    let handle = rig.start();
    let response = rig.call(230, handle, &ask(T0).0);
    let state = state(&mut Rd::ok(&response));
    assert_eq!(state.off.as_deref(), Some("Requests need own wallet"));
    assert_eq!(state.pairing, 0);
    let fault = Rd::error(&rig.call(227, handle, &pair_args(URL, "Corner Shop"))).expect("refused");
    assert_eq!(fault.status, 3, "a state, not a rule");
    assert_eq!(fault.words.heading, "Requests need own wallet");
}

#[test]
fn wo_06_a_store_without_https_is_refused_and_nothing_is_kept() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let fault = Rd::error(&rig.call(227, handle, &pair_args("http://shop.example", "Shop")))
        .expect("refused");
    assert_eq!(fault.rule, "WO-06");
    assert_eq!(fault.words.heading, "WORDS_NEEDED_WO_06_HEADING");
    let response = rig.call(230, handle, &ask(T0).0);
    assert_eq!(state(&mut Rd::ok(&response)).pairing, 0);
}

#[test]
fn wo_20_wo_10_a_visit_asks_for_orders_then_sends_a_range_from_the_desk() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    Rd::ok(&rig.call(227, handle, &pair_args(URL, "Corner Shop")));
    let force = ask(T0 + 5).u8(1).0;
    // Nothing is visited before the owner says the fingerprints match.
    let response = rig.call(231, handle, &force);
    assert_eq!(step(&mut Rd::ok(&response)).kind, 0);
    let response = rig.call(228, handle, &ask(T0 + 6).0);
    assert_eq!(state(&mut Rd::ok(&response)).pairing, 2);
    // The first call asks for the orders, with the three headers.
    let response = rig.call(231, handle, &ask(T0 + 7).u8(0).0);
    let first = step(&mut Rd::ok(&response));
    assert_eq!(first.kind, 2);
    let (kind, url, headers, content_type, body) = first.call.expect("a call");
    assert_eq!(kind, 0);
    assert_eq!(url, "https://shop.example/wp-json/openbmt/v1/orders");
    let names: Vec<&str> = headers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["X-OBM-Time", "X-OBM-Nonce", "X-OBM-Mac"]);
    assert_eq!(headers[0].1, (T0 + 7).to_string());
    assert_eq!(content_type, "application/json");
    assert_eq!(body, b"{\"since\":0}");
    // A second step while the call is in the air waits.
    let response = rig.call(231, handle, &force);
    assert_eq!(step(&mut Rd::ok(&response)).kind, 1);
    // The answer: no orders, no addresses left. The next call is a range, made
    // from 100 indexes of the desk, which the node took for it.
    let reply = ask(T0 + 8)
        .u8(0)
        .u32(200)
        .bytes(b"{\"orders\":[],\"left\":0}")
        .0;
    let response = rig.call(232, handle, &reply);
    assert_eq!(state(&mut Rd::ok(&response)).failure, None);
    let response = rig.call(231, handle, &ask(T0 + 9).u8(0).0);
    let second = step(&mut Rd::ok(&response));
    let (kind, url, _, content_type, body) = second.call.expect("a range");
    assert_eq!(kind, 1);
    assert!(url.ends_with("/openbmt/v1/range"));
    assert_eq!(content_type, "text/plain");
    // The range is hex, of the magic, the version, the id, the start, 100
    // addresses and the signature.
    let text = String::from_utf8(body).expect("hex");
    assert!(text.starts_with(
        &"OBM{"
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ));
    assert!(text.bytes().all(|b| b.is_ascii_hexdigit()));
    let response = rig.call(230, handle, &ask(T0 + 9).0);
    let now = state(&mut Rd::ok(&response));
    assert_eq!(now.sent, 100);
    // The store takes it.
    let reply = ask(T0 + 10)
        .u8(0)
        .u32(200)
        .bytes(b"{\"result\":\"accepted\",\"left\":100}")
        .0;
    let response = rig.call(232, handle, &reply);
    let now = state(&mut Rd::ok(&response));
    assert_eq!((now.left, now.failure), (Some(100), None));
    let response = rig.call(231, handle, &ask(T0 + 11).u8(0).0);
    assert_eq!(step(&mut Rd::ok(&response)).kind, 0, "nothing more to do");
}

#[test]
fn wo_21_a_call_that_got_no_answer_is_shown_as_a_failed_visit_and_waits() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    Rd::ok(&rig.call(227, handle, &pair_args(URL, "Corner Shop")));
    Rd::ok(&rig.call(228, handle, &ask(T0 + 1).0));
    let response = rig.call(231, handle, &ask(T0 + 2).u8(0).0);
    assert_eq!(step(&mut Rd::ok(&response)).kind, 2);
    // The app could not reach the store (outcome 1).
    let response = rig.call(232, handle, &ask(T0 + 3).u8(1).u32(0).bytes(&[]).0);
    let now = state(&mut Rd::ok(&response));
    assert_eq!(now.failure.as_deref(), Some("WORDS_NEEDED_WO_06_HEADING"));
    // The next visit waits 30 seconds; the owner's own does not.
    let response = rig.call(231, handle, &ask(T0 + 10).u8(0).0);
    assert_eq!(step(&mut Rd::ok(&response)).kind, 0);
    let response = rig.call(231, handle, &ask(T0 + 11).u8(1).0);
    assert_eq!(step(&mut Rd::ok(&response)).kind, 2);
    // An answer with no call in the air is refused, and so is a bad status.
    let reply = ask(T0 + 12).u8(0).u32(200).bytes(b"{}").0;
    Rd::ok(&rig.call(232, handle, &reply));
    let fault = Rd::error(&rig.call(232, handle, &reply)).expect("refused");
    assert_eq!(fault.rule, "WO-20");
    let reply = ask(T0 + 13).u8(0).u32(70_000).bytes(b"{}").0;
    assert!(Rd::error(&rig.call(232, handle, &reply)).is_some());
}

#[test]
fn wo_07_unpairing_forgets_the_pairing_and_keeps_the_addresses() {
    let rig = Rig::new();
    let file = wallet_file(&rig);
    settle(&rig, &file);
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    Rd::ok(&rig.call(227, handle, &pair_args(URL, "Corner Shop")));
    Rd::ok(&rig.call(228, handle, &ask(T0 + 1).0));
    // Take the range of the first visit.
    for (n, reply) in [
        &b"{\"orders\":[],\"left\":0}"[..],
        &b"{\"result\":\"accepted\",\"left\":100}"[..],
    ]
    .iter()
    .enumerate()
    {
        let response = rig.call(231, handle, &ask(T0 + 2).u8(0).0);
        assert_eq!(step(&mut Rd::ok(&response)).kind, 2, "step {n}");
        let reply = ask(T0 + 3).u8(0).u32(200).bytes(reply).0;
        Rd::ok(&rig.call(232, handle, &reply));
    }
    let response = rig.call(229, handle, &ask(T0 + 4).0);
    let now = state(&mut Rd::ok(&response));
    assert_eq!((now.pairing, now.sent, now.left), (0, 100, None));
    assert_eq!(now.code, None);
    let fault = Rd::error(&rig.call(229, handle, &ask(T0 + 5).0)).expect("refused");
    assert_eq!(fault.rule, "WO-05");
    // A restart reads the file back: still unpaired, still 100 addresses.
    Rd::ok(&rig.call(2, handle, &[]));
    let handle = Rd::ok(&rig.start_own(&file)).u64();
    let response = rig.call(230, handle, &ask(T0 + 6).0);
    let now = state(&mut Rd::ok(&response));
    assert_eq!((now.pairing, now.sent, now.off), (0, 100, None));
}
