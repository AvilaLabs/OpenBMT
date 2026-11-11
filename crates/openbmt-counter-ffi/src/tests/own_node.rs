//! The own node through `dispatch` (CR-100 to CR-102, CR-105, CR-107): the
//! address a manager typed is read, the start takes it or says why not, the
//! status carries the health line, and the sale screen says which check was
//! done.

use bitcoin::Transaction;
use openbmt_net_io::OwnNodeEvent;
use openbmt_watch::{OwnNodeInput, OwnNodeLink};

use super::kit::*;
use super::wire::{Rd, Words, W};
use crate::StartConfig;

const NAME: &str = "a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad.onion";
const REGTEST: u8 = 3;

fn check(rig: &Rig, text: &str, tor: bool) -> Result<String, Box<(String, Words)>> {
    let response = rig.call(
        49,
        0,
        &W::new().string(text).u8(REGTEST).u8(u8::from(tor)).0,
    );
    match Rd::error(&response) {
        None => {
            let mut r = Rd::ok(&response);
            let text = r.string();
            r.done();
            Ok(text)
        }
        Some(f) => {
            assert_eq!(f.status, 1);
            Err(Box::new((f.rule, f.words)))
        }
    }
}

fn start_with_own_node(rig: &Rig, own: Option<&str>, proxy: Option<&str>) -> u64 {
    let mut config: StartConfig = start_config(rig.dir().to_str().expect("UTF-8"));
    config.own_node = own.map(str::to_string);
    config.proxy = proxy.map(str::to_string);
    rig.start_with(&config)
}

/// The status as far as the own node's line: (usable, words).
fn own_line(rig: &Rig, handle: u64) -> Option<(bool, Words)> {
    let response = rig.call(30, handle, &[]);
    let mut r = Rd::ok(&response);
    let _ = (r.u32(), r.u32());
    let _ = r.opt(|r| (r.bool(), r.bool(), r.words()));
    let own = r.opt(|r| (r.bool(), r.words()));
    let _ = r.list(Rd::u32);
    let _ = (r.u64(), r.bool());
    r.done();
    own
}

fn pump(rig: &Rig, handle: u64) {
    let response = rig.call(10, handle, &W::new().u64(T0).u32(50).0);
    let mut r = Rd::ok(&response);
    let _ = r.pump();
    r.done();
}

fn judge(rig: &Rig, handle: u64, index: u32) -> super::wire::Screen {
    let response = rig.call(12, handle, &W::new().u64(T0).u32(index).0);
    let mut r = Rd::ok(&response);
    let screen = r.screen();
    r.done();
    screen
}

fn connected() -> OwnNodeEvent {
    OwnNodeEvent::State(OwnNodeInput {
        link: OwnNodeLink::Connected,
        best_height: Some(0),
        last_heard: Some(T0),
    })
}

#[test]
fn cr_100_the_check_makes_the_canonical_text() {
    let rig = Rig::new();
    assert_eq!(check(&rig, NAME, true), Ok(format!("{NAME}:18444")));
    assert_eq!(
        check(&rig, &format!(" {NAME}:9999 "), true),
        Ok(format!("{NAME}:9999"))
    );
    let (rule, words) = *check(&rig, "nonsense.onion", true).unwrap_err();
    assert_eq!(
        (rule.as_str(), words.heading.as_str()),
        ("CR-100", "Tor address not accepted")
    );
    let (rule, _) = *check(&rig, &format!("{NAME}:0"), true).unwrap_err();
    assert_eq!(rule, "CR-100");
}

#[test]
fn cr_101_a_home_network_address_is_refused_with_words() {
    let rig = Rig::new();
    for text in ["192.168.1.20:8333", "umbrel.local", "127.0.0.1:18444"] {
        let (rule, words) = *check(&rig, text, true).unwrap_err();
        assert_eq!(rule, "CR-101", "{text}");
        assert_eq!(words.heading, "Not a Tor address");
        assert!(words.what.contains("cannot prove which machine answers"));
        assert!(words.next.contains(".onion") && !words.next.contains("peer"));
    }
}

#[test]
fn cr_102_without_tor_the_check_says_to_turn_on_orbot() {
    let rig = Rig::new();
    let (rule, words) = *check(&rig, NAME, false).unwrap_err();
    assert_eq!(rule, "CR-102");
    assert_eq!(words.heading, "Turn on Orbot first");
}

#[test]
fn cr_100_a_start_with_an_own_node_hands_it_to_the_driver() {
    let rig = Rig::new();
    let handle = start_with_own_node(&rig, Some(&format!("{NAME}:18444")), Some("127.0.0.1:9050"));
    let launch = rig.launch();
    let own = launch.driver.own_node.expect("an own node");
    assert_eq!(own.port(), 18_444);
    assert!(own.is_authenticated());
    assert!(launch.driver.proxy.is_some());
    // Before the driver says anything the own node is connecting, not
    // "not reachable" (WR-63).
    let (usable, words) = own_line(&rig, handle).expect("a line");
    assert!(!usable);
    assert_eq!(words.heading, "Connecting to your node");
    assert_eq!(words.code.as_deref(), Some("WR-63"));
    // The driver says it is connected: the line says it is checking too.
    rig.send_own(vec![connected()]);
    pump(&rig, handle);
    let (usable, words) = own_line(&rig, handle).expect("a line");
    assert!(usable);
    assert_eq!(words.heading, "Your node is checking too");
}

#[test]
fn cr_102_a_start_with_an_own_node_and_no_proxy_starts_without_it_and_says_why() {
    let rig = Rig::new();
    let handle = start_with_own_node(&rig, Some(&format!("{NAME}:18444")), None);
    assert!(rig.launch().driver.own_node.is_none());
    let (usable, words) = own_line(&rig, handle).expect("a line");
    assert!(!usable);
    assert_eq!(words.heading, "Turn on Orbot first");
    assert_eq!(words.code.as_deref(), Some("CR-102"));
}

#[test]
fn cr_101_a_start_with_a_home_network_address_starts_without_it_and_says_why() {
    let rig = Rig::new();
    let handle = start_with_own_node(&rig, Some("192.168.1.20:8333"), Some("127.0.0.1:9050"));
    assert!(rig.launch().driver.own_node.is_none());
    let (_, words) = own_line(&rig, handle).expect("a line");
    assert_eq!(words.heading, "Not a Tor address");
}

#[test]
fn cr_108_a_start_with_no_own_node_has_no_line() {
    let rig = Rig::new();
    let handle = start_with_own_node(&rig, None, Some("127.0.0.1:9050"));
    assert!(rig.launch().driver.own_node.is_none());
    assert!(own_line(&rig, handle).is_none());
}

#[test]
fn cr_107_try_again_reaches_the_driver_only_when_there_is_an_own_node() {
    let rig = Rig::new();
    let handle = start_with_own_node(&rig, Some(&format!("{NAME}:18444")), Some("127.0.0.1:9050"));
    let response = rig.call(32, handle, &[]);
    Rd::ok(&response).done();
    assert_eq!(rig.calls.lock().expect("lock").retried, 1);
    let f = Rd::error(&rig.call(32, 99, &[])).expect("no such node");
    assert_eq!(f.rule, "handle-unknown");

    let rig = Rig::new();
    let handle = start_with_own_node(&rig, None, None);
    Rd::ok(&rig.call(32, handle, &[])).done();
    assert_eq!(rig.calls.lock().expect("lock").retried, 0);
}

/// Open a sale of $5, pay it, and have three groups see it: the check is
/// the light check, and no heartbeat has come, so the sale waits.
fn waiting_sale(rig: &Rig, handle: u64) -> Transaction {
    rig.send(vec![peers_event()]);
    pump(rig, handle);
    let response = rig.call(
        11,
        handle,
        &W::new().open(T0, FIVE_DOLLARS, "USD", RATE, "test-fixed").0,
    );
    Rd::ok(&response);
    let tx = spend(1, &[(&script_of(0), 5_000)]);
    rig.send(vec![sighting(&tx, vec![peer(20), peer(21), peer(22)])]);
    pump(rig, handle);
    tx
}

#[test]
fn cr_105_the_sale_screen_says_which_check_was_done() {
    let rig = Rig::new();
    let handle = start_with_own_node(&rig, None, None);
    let tx = waiting_sale(&rig, handle);
    let screen = judge(&rig, handle, 0);
    assert_eq!(screen.state, 2, "Waiting");
    assert_eq!(screen.check.expect("a check line").heading, "Light check");
    drop(tx);

    // With an own node that reports and is connected: the node and the light
    // check; when it goes down: the fallback, named as such.
    let rig = Rig::new();
    let handle = start_with_own_node(&rig, Some(&format!("{NAME}:18444")), Some("127.0.0.1:9050"));
    let tx = waiting_sale(&rig, handle);
    rig.send_own(vec![OwnNodeEvent::Has(tx.compute_txid()), connected()]);
    pump(&rig, handle);
    let screen = judge(&rig, handle, 0);
    assert_eq!(
        screen.check.expect("a check line").heading,
        "Your node and light check"
    );
    rig.send_own(vec![OwnNodeEvent::State(OwnNodeInput {
        link: OwnNodeLink::Down { since: T0 },
        best_height: None,
        last_heard: None,
    })]);
    pump(&rig, handle);
    let screen = judge(&rig, handle, 0);
    let line = screen.check.expect("a check line");
    assert_eq!(line.heading, "Light check, not your node");
    assert_eq!(line.code.as_deref(), Some("CR-105"));
}
