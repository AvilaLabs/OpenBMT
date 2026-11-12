//! The node and the mode come from the settings in force (NR-105), and the
//! sales are checked against the own node alone in node-only mode (NR-106,
//! WR-66 to WR-68). The driver is the one tests feed by hand.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bitcoin::hashes::Hash;
use bitcoin::Transaction;
use openbmt_core::approval::CounterConfirmation;
use openbmt_core::confirmed::ConfirmedSettings;
use openbmt_core::heartbeat::WatcherHeartbeat;
use openbmt_core::onion::onion_name;
use openbmt_core::policy::{CheckMode, InstantLabel};
use openbmt_core::records::RecordKind;
use openbmt_core::settings::{Change, OwnNode as SettingsOwnNode, Settings, SignedSettings};
use openbmt_counter::say::CheckLine;
use openbmt_counter::{Amount, ConfirmHeld, State, Why};
use openbmt_net_io::{AppEvent, NetMode, OwnNodeEvent};
use openbmt_watch::{NodeOnlyFault, NodeOnlyInput, NodeOnlyLine, OwnNodeInput, OwnNodeLink};

use super::kit::*;
use crate::NodeConfig;

const TWENTY: Amount = Amount(2_000);

/// The own node the settings name: not the one of the app's configuration.
fn settings_node() -> SettingsOwnNode {
    SettingsOwnNode::new(&onion_name(&[9; 32]), 8_333).unwrap()
}

/// The test settings, with the own node and the mode as given.
fn node_settings(version: u32, node_only: bool, light_fallback: bool) -> Settings {
    let mut s = settings();
    s.version = version;
    s.own_node = Some(settings_node());
    s.node_only = node_only;
    s.light_fallback = light_fallback;
    s
}

fn config_with(dir: &std::path::Path, time: &Arc<AtomicU64>, s: Settings) -> NodeConfig {
    let mut config = config(dir, time);
    config.driver.proxy = Some("127.0.0.1:9050".parse().unwrap());
    config.settings = s;
    config
}

fn rig_with(dir: &std::path::Path, s: Settings) -> Rig {
    let time = Arc::new(AtomicU64::new(T0));
    start_with(config_with(dir, &time, s), time)
}

fn hb(seq: u64, time: u64) -> Vec<u8> {
    WatcherHeartbeat {
        counter: id(3).device_id(),
        seq,
        time,
        tip_height: 0,
        tip_hash: genesis().to_byte_array(),
        healthy: true,
    }
    .sign(&id(1))
    .to_bytes()
}

fn payment() -> Transaction {
    spend(1, &[(&script_of(0), 20_000)])
}

fn input(link: OwnNodeLink, fault: Option<NodeOnlyFault>, fallback: (bool, bool)) -> OwnNodeEvent {
    OwnNodeEvent::NodeOnly(NodeOnlyInput {
        node: OwnNodeInput {
            link,
            best_height: matches!(link, OwnNodeLink::Connected).then_some(0),
            last_heard: matches!(link, OwnNodeLink::Connected).then_some(T0 + 10),
        },
        fault,
        light_fallback: fallback.0,
        fallback_running: fallback.1,
    })
}

fn up() -> OwnNodeEvent {
    input(OwnNodeLink::Connected, None, (false, false))
}

fn down() -> OwnNodeEvent {
    input(OwnNodeLink::Down { since: T0 }, None, (false, false))
}

/// A started node-only rig with a $20 sale open and a heartbeat taken.
fn rig_with_sale(dir: &std::path::Path, fallback: bool) -> Rig {
    let mut rig = rig_with(dir, node_settings(1, true, fallback));
    rig.feed(vec![Rig::peers()]);
    rig.node
        .open(TWENTY, usd(), rate(), &source_at(&rig.time))
        .unwrap();
    rig.time.store(T0 + 10, Ordering::SeqCst);
    rig.node.heartbeat(&hb(1, T0 + 10)).unwrap();
    rig
}

fn sighted_by(tx: &Transaction, peers: &[u8]) -> AppEvent {
    AppEvent::Sighting {
        tx: bytes(tx),
        from: peers.iter().map(|n| peer(*n)).collect(),
    }
}

#[test]
fn nr_105_the_node_the_settings_name_is_the_one_the_driver_gets() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut config = config_with(dir.path(), &time, node_settings(1, false, false));
    // The counter app has an own node of its own, which is not used.
    config.driver.own_node = Some(own_node());
    let mut rig = start_with(config, time);
    let got = rig.launch.driver.own_node.clone().unwrap();
    assert_eq!(got.port(), 8_333);
    assert_ne!(got, own_node());
    assert_eq!(
        got,
        openbmt_net_io::OwnNode::onion(&onion_name(&[9; 32]), 8_333).unwrap()
    );
    let notes = rig.node.take_notes();
    assert!(notes.iter().any(|n| n.starts_with("NR-105: ")), "{notes:?}");
    assert!(rig.node.own_node_configured());
    assert_eq!(rig.launch.driver.mode, NetMode::Mixed);
}

#[test]
fn nr_105_settings_without_an_own_node_leave_the_apps_node_and_say_nothing() {
    let dir = Scratch::new();
    let mut rig = start_with_own_node(dir.path());
    assert_eq!(rig.launch.driver.own_node, Some(own_node()));
    assert_eq!(rig.launch.driver.mode, NetMode::Mixed);
    assert!(rig.node.notes().all(|n| !n.contains("NR-105")));
    assert!(!rig.node.node_only());
    assert!(rig.node.node_only_line().is_none());
}

#[test]
fn nr_105_node_only_gives_the_driver_the_mode_and_the_fallback() {
    for (fallback, want) in [(false, false), (true, true)] {
        let dir = Scratch::new();
        let rig = rig_with(dir.path(), node_settings(1, true, fallback));
        assert_eq!(
            rig.launch.driver.mode,
            NetMode::NodeOnly {
                light_fallback: want
            }
        );
        assert!(rig.node.node_only());
    }
}

#[test]
fn nr_105_a_change_of_the_node_or_the_mode_needs_a_restart() {
    // A counter paired with the shop's own wallet: the owner signs alone.
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_with(config_with(dir.path(), &time, settings()), time.clone());
    assert!(!rig.node.restart_needed());
    // The node named while node-only is off: a tightening, in force at once.
    let named = SignedSettings {
        owner_sig: node_settings(2, false, false).sign(&id(1)),
        approver_sig: None,
        settings: node_settings(2, false, false),
    };
    // The Key app's counter takes a tightening on the owner's signature.
    assert_eq!(rig.node.take_settings(&named), Ok(Some(Change::Tightening)));
    assert!(rig.node.restart_needed());
    assert!(rig.node.notes().any(|n| n.contains("NR-105")));
    assert!(rig.node.notes().all(|n| !n.contains("CR-164")));
    // The driver still runs as it was started: nothing changed there.
    assert_eq!(rig.launch.driver.own_node, None);
    drop(rig);
    // A start with those settings has nothing left to do, and the driver
    // gets the node.
    let rig = start_with(
        config_with(dir.path(), &time, node_settings(2, false, false)),
        time,
    );
    assert!(!rig.node.restart_needed());
    assert!(rig.launch.driver.own_node.is_some());
}

#[test]
fn nr_105_a_change_of_the_mode_back_needs_a_restart_too() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_with(
        config_with(dir.path(), &time, node_settings(1, true, true)),
        time,
    );
    assert!(!rig.node.restart_needed());
    // Turning the fallback off and then node-only off are tightenings.
    for (version, node_only, fallback) in [(2, true, false), (3, false, false)] {
        let s = node_settings(version, node_only, fallback);
        let signed = SignedSettings {
            owner_sig: s.sign(&id(1)),
            approver_sig: None,
            settings: s,
        };
        // The Key app's counter needs no co-signature for a tightening.
        assert_eq!(
            rig.node.take_settings(&signed),
            Ok(Some(Change::Tightening))
        );
        assert!(rig.node.restart_needed());
    }
    // Back to what it was started with: nothing to restart for.
    let s = node_settings(4, true, true);
    let signed = SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: Some(s.sign(&id(2))),
        settings: s,
    };
    assert_eq!(rig.node.take_settings(&signed), Ok(Some(Change::Loosening)));
    assert!(!rig.node.restart_needed());
}

// ---- the confirmation of a loosening of AP-11 -----------------------------

fn own_wallet_config(dir: &std::path::Path, time: &Arc<AtomicU64>) -> NodeConfig {
    let record = super::paired_own::record();
    let mut config = NodeConfig::new_own(
        dir,
        id(3),
        record.own_pairing(super::paired_own::POLICY),
        settings(),
        openbmt_net_io::DriverConfig::new(bitcoin::Network::Bitcoin, 0),
    );
    config.clock = Box::new(TestClock(time.clone()));
    config.health = Box::new(|_| healthy());
    config
}

fn signed_by_owner(s: Settings) -> SignedSettings {
    SignedSettings {
        owner_sig: s.sign(&id(1)),
        approver_sig: None,
        settings: s,
    }
}

#[test]
fn ap_11_node_only_turned_on_is_kept_confirmed_applied_and_then_the_driver_says_node_only() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_with(own_wallet_config(dir.path(), &time), time.clone());
    assert_eq!(rig.launch.driver.mode, NetMode::Mixed);
    // The own node with node-only on is a loosening: kept, not in force.
    let change = signed_by_owner(node_settings(2, true, false));
    assert_eq!(rig.node.take_settings(&change), Ok(None));
    assert_eq!(rig.node.settings().version, 1);
    assert!(!rig.node.settings().node_only);
    let waiting = rig.node.pending_change().unwrap();
    // The page lists what loosens, with the node's name and the switch.
    let fields: Vec<_> = waiting.fields.iter().map(|f| f.field).collect();
    assert_eq!(fields, ["own_node", "node_only"]);
    assert_eq!(
        waiting.fields[0].after,
        crate::FieldValue::Code(format!("{}:8333", onion_name(&[9; 32])))
    );
    assert_eq!(
        waiting.fields[0].now,
        crate::FieldValue::Code("none".to_string())
    );
    assert_eq!(waiting.fields[1].now, crate::FieldValue::Flag(false));
    assert_eq!(waiting.fields[1].after, crate::FieldValue::Flag(true));
    // A counter confirms it; the owner sends it again as `OBM<`.
    rig.node.confirm_pending(2).unwrap();
    let held = rig
        .node
        .records_from(0, usize::MAX)
        .unwrap()
        .iter()
        .filter(|r| r.kind == RecordKind::Setting)
        .filter_map(|r| ConfirmHeld::decode(&r.body).ok())
        .next_back()
        .unwrap()
        .confirmation(id(3).device_id());
    assert_eq!(
        held,
        CounterConfirmation::sign(&id(3), &change.settings.hash())
    );
    let confirmed = ConfirmedSettings::new(change.clone(), held);
    assert_eq!(
        rig.node.take_confirmed_settings(&confirmed),
        Ok(Some(Change::Loosening))
    );
    assert!(rig.node.settings().node_only);
    assert!(rig.node.restart_needed());
    assert!(rig.node.notes().any(|n| n.contains("NR-105")));
    drop(rig);
    // The next start: the driver config says node-only.
    let mut config = own_wallet_config(dir.path(), &time);
    config.settings = change.settings.clone();
    let rig = start_with(config, time);
    assert_eq!(
        rig.launch.driver.mode,
        NetMode::NodeOnly {
            light_fallback: false
        }
    );
    assert!(rig.launch.driver.own_node.is_some());
    assert!(!rig.node.restart_needed());
}

#[test]
fn ap_11_a_free_change_of_the_node_is_taken_at_once_on_the_wallets_counter() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut rig = start_with(own_wallet_config(dir.path(), &time), time);
    let change = signed_by_owner(node_settings(2, false, false));
    assert_eq!(
        rig.node.take_settings(&change),
        Ok(Some(Change::Tightening))
    );
    assert!(rig.node.pending_change().is_none());
    assert!(rig.node.restart_needed());
    // The same version again changes nothing.
    assert_eq!(rig.node.take_settings(&change), Ok(None));
}

// ---- the check of a sale in node-only mode (WR-66 to WR-68) ----------------

#[test]
fn wr_66_a_payment_the_node_holds_is_checked_by_the_node_alone() {
    let dir = Scratch::new();
    let mut rig = rig_with_sale(dir.path(), false);
    let tx = payment();
    // One sighting is not three groups; the node's report is what counts.
    rig.feed(vec![sighted_by(&tx, &[20])]);
    rig.feed_own(vec![up(), OwnNodeEvent::Has(tx.compute_txid())]);
    // Waiting for the conflict watch, which counts from the node's report.
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.check_mode, Some(CheckMode::OwnNodeOnly));
    assert_eq!(screen.check_line(), Some(CheckLine::YourNodeOnly));
    rig.time.store(T0 + 30, Ordering::SeqCst);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.check_mode, Some(CheckMode::OwnNodeOnly));
    assert_eq!(screen.state, State::PaidInstant, "{:?}", screen.why);
    assert_eq!(screen.why, Why::Instant(InstantLabel::YourNode));
    assert_eq!(rig.node.node_only_line(), Some(NodeOnlyLine::Usable));
}

#[test]
fn wr_66_the_light_groups_count_for_nothing_without_the_fallback() {
    let dir = Scratch::new();
    let mut rig = rig_with_sale(dir.path(), false);
    let tx = payment();
    rig.feed(vec![sighted_by(&tx, &[20, 21, 22])]);
    // The node is not connected: nothing is accepted at once, and the sale
    // has no check mode.
    rig.feed_own(vec![down()]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.check_mode, None);
    assert!(matches!(
        rig.node.node_only_line(),
        Some(NodeOnlyLine::NotUsable(_))
    ));
    // The node reports it but is cut off again: still the node's, unusable.
    rig.feed_own(vec![OwnNodeEvent::Has(tx.compute_txid())]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.check_mode, None);
}

#[test]
fn wr_68_with_the_fallback_the_light_groups_count_while_the_random_peers_run() {
    let dir = Scratch::new();
    let mut rig = rig_with_sale(dir.path(), true);
    let tx = payment();
    rig.feed(vec![sighted_by(&tx, &[20, 21, 22])]);
    rig.feed_own(vec![input(
        OwnNodeLink::Down { since: T0 },
        None,
        (true, true),
    )]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    assert!(matches!(
        rig.node.node_only_line(),
        Some(NodeOnlyLine::CheckingWithOthers(_))
    ));
}

#[test]
fn wr_67_a_node_that_lacks_the_filters_is_not_usable_even_when_connected() {
    let dir = Scratch::new();
    let mut rig = rig_with_sale(dir.path(), false);
    let tx = payment();
    rig.feed(vec![sighted_by(&tx, &[20])]);
    rig.feed_own(vec![
        OwnNodeEvent::Has(tx.compute_txid()),
        input(
            OwnNodeLink::Connected,
            Some(NodeOnlyFault::MissingFilters { since: T0 + 10 }),
            (false, false),
        ),
    ]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.check_mode, None);
    let line = rig.node.node_only_line();
    assert!(matches!(line, Some(NodeOnlyLine::NotUsable(_))), "{line:?}");
    // The node came up fine: the report counts.
    rig.feed_own(vec![up()]);
    let screen = rig.node.judge(0).unwrap();
    assert_eq!(screen.check_mode, Some(CheckMode::OwnNodeOnly));
}

#[test]
fn nr_105_the_event_of_the_other_mode_is_not_read() {
    // A node that was not started node-only ignores a node-only state, and
    // one that was ignores the state of the mode with the light client.
    let dir = Scratch::new();
    let mut rig = start_with_own_node(dir.path());
    rig.feed(vec![Rig::peers()]);
    rig.feed_own(vec![up()]);
    assert!(rig.node.node_only_line().is_none());
    let dir2 = Scratch::new();
    let mut rig = rig_with_sale(dir2.path(), false);
    rig.feed_own(vec![own_connected(0, T0 + 10)]);
    assert!(matches!(
        rig.node.node_only_line(),
        Some(NodeOnlyLine::NotUsable(_))
    ));
}
