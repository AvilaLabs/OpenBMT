//! Section 7, with `node_only` on: the own node as the only source.

use bitcoin::{ScriptBuf, Transaction};
use openbmt_core::policy::{
    evaluate, CheckMode, Decision, Heartbeat, Inputs, InstantLabel, InstantPolicy, PauseStatus,
};
use openbmt_core::say::Tone;

use super::kit::*;
use crate::{
    NodeOnlyFault, NodeOnlyHealth, NodeOnlyInput, NodeOnlyLine, NodeOnlyProblem, NodeOnlyTracker,
    OwnNodeInput, OwnNodeLink, OwnNodeProblem, PeerAddr, SaleStatus, SaleWatch, Source,
};

const NOW: u64 = 1_000_000;
const TIP: u32 = 800_000;

fn shop() -> ScriptBuf {
    script(1)
}

fn watch() -> SaleWatch {
    SaleWatch::new(shop(), 10_000, 0)
}

fn pays() -> Transaction {
    spend(1, &[(&shop(), 10_000)])
}

fn peer(n: u8) -> Source {
    Source::Peer(PeerAddr::Ip(format!("{n}.{n}.1.1").parse().unwrap()))
}

fn input(link: OwnNodeLink, best: Option<u32>, heard: Option<u64>) -> NodeOnlyInput {
    NodeOnlyInput {
        node: OwnNodeInput {
            link,
            best_height: best,
            last_heard: heard,
        },
        fault: None,
        light_fallback: false,
        fallback_running: false,
    }
}

fn connected() -> NodeOnlyInput {
    input(OwnNodeLink::Connected, Some(TIP), Some(NOW))
}

fn check(i: &NodeOnlyInput) -> NodeOnlyHealth {
    NodeOnlyTracker::new().check(Some(i), TIP, NOW)
}

fn usable() -> NodeOnlyHealth {
    let h = check(&connected());
    assert!(h.own.usable && h.problem.is_none());
    h
}

fn status(w: &mut SaleWatch, h: &NodeOnlyHealth) -> SaleStatus {
    let chain = TestChain::new(1);
    w.status_node_only(&chain, NOW, h)
}

// WR-66.

#[test]
fn wr_66_a_payment_the_node_holds_has_the_own_node_only_mode_with_no_light_client() {
    let tx = pays();
    let mut w = watch();
    // One sighting, from the node alone: no three groups are needed.
    w.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    assert!(w.own_node_saw(tx.compute_txid()));
    let s = status(&mut w, &usable());
    assert_eq!(s.mode, Some(CheckMode::OwnNodeOnly));
    assert!(s.own_node_saw && s.own_node_counted);
    // The sale reads the same with no sighting but the node's.
    assert_eq!(s.received_sats, 10_000);
}

#[test]
fn wr_66_the_mode_takes_the_instant_limits_and_label_of_the_node_mode() {
    let policy = InstantPolicy::defaults(1_000);
    assert_eq!(
        policy.limits(CheckMode::OwnNodeOnly),
        policy.limits(CheckMode::NodeAlsoSeenByLight)
    );
    assert_ne!(
        policy.limits(CheckMode::OwnNodeOnly),
        policy.limits(CheckMode::Light)
    );
    // The label that goes with it is "your node checked it".
    let i = Inputs {
        mode: CheckMode::OwnNodeOnly,
        sale_sats: 1_000,
        received_sats: 1_000,
        outstanding_sats: 0,
        check_healthy: true,
        heartbeat: Heartbeat {
            verified: true,
            age_secs: 1,
        },
        remote_order: false,
        customer_wifi: false,
        tor_on: true,
        pause: PauseStatus::Running,
        conflict_seen: false,
        secs_since_first_seen: 60,
    };
    assert_eq!(
        evaluate(&policy, &i),
        Decision::Instant {
            label: InstantLabel::YourNode,
            mode: CheckMode::OwnNodeOnly
        }
    );
}

#[test]
fn wr_66_a_payment_the_node_does_not_hold_has_no_mode_and_waits() {
    let tx = pays();
    let mut w = watch();
    w.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    // Not reported by the node: no mode, however many groups the light
    // client has (it is not used), and a Box report gives none either.
    for n in 21..24 {
        w.sighting(&bytes(&tx), peer(n), NOW).unwrap();
    }
    w.sighting(&bytes(&tx), Source::Box, NOW).unwrap();
    let s = status(&mut w, &usable());
    assert_eq!(s.mode, None);
    assert!(!s.own_node_saw && !s.own_node_counted);
}

#[test]
fn wr_66_the_conflict_watch_counts_from_the_nodes_first_report() {
    let tx = pays();
    let mut w = watch();
    w.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    // Reported at NOW, read 30 seconds later; a second report does not
    // start the time over.
    assert!(w.own_node_saw_at(tx.compute_txid(), NOW));
    assert!(w.own_node_saw_at(tx.compute_txid(), NOW + 20));
    let chain = TestChain::new(1);
    let h = usable();
    assert_eq!(w.status_node_only(&chain, NOW, &h).secs_since_first_seen, 0);
    assert_eq!(
        w.status_node_only(&chain, NOW + 30, &h)
            .secs_since_first_seen,
        30
    );
    // Without a report there is no time at all; in the mixed mode the
    // report starts no clock (WR-46).
    let mut other = watch();
    other.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    assert!(other.own_node_saw(tx.compute_txid()));
    assert_eq!(
        other
            .status_node_only(&chain, NOW + 30, &h)
            .secs_since_first_seen,
        0
    );
    let mut mixed = watch();
    mixed.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    assert!(mixed.own_node_saw_at(tx.compute_txid(), NOW));
    let own = crate::check_own_node(Some(&connected().node), TIP, NOW, None);
    assert_eq!(
        mixed
            .status_with_own_node(&chain, NOW + 30, &own)
            .secs_since_first_seen,
        0
    );
}

#[test]
fn wr_66_a_node_that_cannot_be_used_gives_no_mode() {
    let tx = pays();
    let mut w = watch();
    w.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    assert!(w.own_node_saw(tx.compute_txid()));
    let mut i = connected();
    i.node.link = OwnNodeLink::Down { since: NOW - 500 };
    let h = check(&i);
    assert!(!h.own.usable);
    let s = status(&mut w, &h);
    assert_eq!(s.mode, None);
    assert!(s.own_node_saw && !s.own_node_counted);
    // A node whose services do not suit is the same.
    let mut i = connected();
    i.fault = Some(NodeOnlyFault::MissingFilters { since: NOW });
    let s = status(&mut w, &check(&i));
    assert_eq!(s.mode, None);
}

#[test]
fn wr_66_the_mode_leaves_the_mixed_mode_as_it_was() {
    // Without node_only the same sale is judged by WR-62 as before.
    let tx = pays();
    let mut w = watch();
    w.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    assert!(w.own_node_saw(tx.compute_txid()));
    let chain = TestChain::new(1);
    let health = crate::check_own_node(Some(&connected().node), TIP, NOW, None);
    let s = w.status_with_own_node(&chain, NOW, &health);
    assert_eq!(s.mode, None);
}

// WR-67.

#[test]
fn wr_67_the_line_has_a_state_for_each_thing_that_can_be_wrong() {
    let fresh = |i: &NodeOnlyInput| NodeOnlyTracker::new().check(Some(i), TIP, NOW);
    assert_eq!(fresh(&connected()).line(), NodeOnlyLine::Usable);

    let mut i = connected();
    i.node.link = OwnNodeLink::Down { since: NOW - 10 };
    let mut t = NodeOnlyTracker::new();
    t.retry(NOW - 10);
    assert_eq!(
        t.check(Some(&i), TIP, NOW).problem,
        Some(NodeOnlyProblem::Node(OwnNodeProblem::Connecting {
            since: NOW - 10
        }))
    );
    i.node.link = OwnNodeLink::Down { since: NOW - 500 };
    let mut t = NodeOnlyTracker::new();
    t.retry(NOW - 500);
    assert_eq!(
        t.check(Some(&i), TIP, NOW).problem,
        Some(NodeOnlyProblem::Node(OwnNodeProblem::Unreachable {
            since: NOW - 500
        }))
    );
    let mut i = connected();
    i.node.last_heard = Some(NOW - 700);
    assert_eq!(
        fresh(&i).problem,
        Some(NodeOnlyProblem::Node(OwnNodeProblem::Silent {
            since: NOW - 700
        }))
    );
    let mut i = connected();
    i.node.best_height = Some(TIP - 3);
    assert_eq!(
        fresh(&i).problem,
        Some(NodeOnlyProblem::Node(OwnNodeProblem::Behind { blocks: 3 }))
    );
    let mut i = connected();
    i.node.best_height = None;
    assert_eq!(
        fresh(&i).problem,
        Some(NodeOnlyProblem::Node(OwnNodeProblem::TipUnknown))
    );
    let mut i = connected();
    i.node.link = OwnNodeLink::Refused { since: NOW - 5 };
    assert_eq!(
        fresh(&i).problem,
        Some(NodeOnlyProblem::Node(OwnNodeProblem::Refused {
            since: NOW - 5
        }))
    );
    let mut i = connected();
    i.fault = Some(NodeOnlyFault::MissingFilters { since: NOW - 1 });
    assert_eq!(
        fresh(&i).problem,
        Some(NodeOnlyProblem::MissingFilters { since: NOW - 1 })
    );
    let mut i = connected();
    i.fault = Some(NodeOnlyFault::LacksBlock {
        height: 700_000,
        since: NOW - 1,
    });
    assert_eq!(
        fresh(&i).problem,
        Some(NodeOnlyProblem::LacksBlock {
            height: 700_000,
            since: NOW - 1
        })
    );
}

#[test]
fn wr_67_the_order_puts_a_broken_rule_first_then_the_services_then_the_link() {
    let mut i = connected();
    i.fault = Some(NodeOnlyFault::MissingFilters { since: 5 });
    i.node.link = OwnNodeLink::Refused { since: 9 };
    assert_eq!(
        check(&i).problem,
        Some(NodeOnlyProblem::Node(OwnNodeProblem::Refused { since: 9 }))
    );
    // A node that was dropped for its services is down, and the line goes on
    // saying why.
    i.node.link = OwnNodeLink::Down { since: NOW - 5 };
    assert_eq!(
        check(&i).problem,
        Some(NodeOnlyProblem::MissingFilters { since: 5 })
    );
    // A block it lacks is behind a missing connection.
    let mut i = connected();
    i.fault = Some(NodeOnlyFault::LacksBlock {
        height: 1,
        since: 5,
    });
    i.node.link = OwnNodeLink::Down { since: NOW - 500 };
    let mut t = NodeOnlyTracker::new();
    t.retry(NOW - 500);
    assert!(matches!(
        t.check(Some(&i), TIP, NOW).problem,
        Some(NodeOnlyProblem::Node(OwnNodeProblem::Unreachable { .. }))
    ));
}

#[test]
fn wr_67_the_line_says_whether_other_nodes_are_asked() {
    let mut i = connected();
    i.fault = Some(NodeOnlyFault::MissingFilters { since: 5 });
    let p = NodeOnlyProblem::MissingFilters { since: 5 };
    assert_eq!(check(&i).line(), NodeOnlyLine::NotUsable(p));
    i.light_fallback = true;
    i.fallback_running = true;
    assert_eq!(check(&i).line(), NodeOnlyLine::CheckingWithOthers(p));
    // Fallback allowed but not started: the node is not usable and nobody
    // else is asked yet.
    i.fallback_running = false;
    assert_eq!(check(&i).line(), NodeOnlyLine::NotUsable(p));
    assert_eq!(p.rule(), "WR-67");
}

#[test]
fn wr_67_every_state_has_words_with_its_rule_and_the_ones_not_written_say_so() {
    let problems = [
        NodeOnlyProblem::Node(OwnNodeProblem::Connecting { since: 1 }),
        NodeOnlyProblem::Node(OwnNodeProblem::Unreachable { since: 1 }),
        NodeOnlyProblem::Node(OwnNodeProblem::Refused { since: 1 }),
        NodeOnlyProblem::Node(OwnNodeProblem::Silent { since: 1 }),
        NodeOnlyProblem::Node(OwnNodeProblem::Behind { blocks: 2 }),
        NodeOnlyProblem::Node(OwnNodeProblem::TipUnknown),
        NodeOnlyProblem::MissingFilters { since: 1 },
        NodeOnlyProblem::LacksBlock {
            height: 5,
            since: 1,
        },
    ];
    let mut seen = vec![NodeOnlyLine::Usable.words().heading];
    for p in problems {
        for line in [
            NodeOnlyLine::NotUsable(p),
            NodeOnlyLine::CheckingWithOthers(p),
        ] {
            let w = line.words();
            assert_eq!(w.code, Some("WR-67"));
            assert_eq!(openbmt_core::say::check(&w), Ok(()), "{line:?}");
        }
        // With other nodes asked meanwhile, payments are still checked.
        assert_ne!(NodeOnlyLine::CheckingWithOthers(p).words().tone, Tone::Stop);
        // Without them, a node that is down stops the checks, and the line says so.
        let alone = NodeOnlyLine::NotUsable(p).words();
        let waiting = matches!(
            p,
            NodeOnlyProblem::Node(OwnNodeProblem::Connecting { .. })
                | NodeOnlyProblem::Node(OwnNodeProblem::TipUnknown)
        );
        assert_eq!(alone.tone == Tone::Stop, !waiting, "{p:?}");
        let heading = NodeOnlyLine::NotUsable(p).words().heading;
        assert!(!seen.contains(&heading), "{heading}");
        seen.push(heading);
    }
    assert_eq!(
        NodeOnlyLine::Usable.words().heading,
        "Your node is checking"
    );
    assert_eq!(seen.len(), 9);
}

// WR-68.

fn fallback_health(node: NodeOnlyInput) -> NodeOnlyHealth {
    let mut i = node;
    i.light_fallback = true;
    i.fallback_running = true;
    check(&i)
}

#[test]
fn wr_68_with_the_random_peers_running_what_they_report_has_the_light_mode() {
    let tx = pays();
    let mut w = watch();
    for n in 20..23 {
        w.sighting(&bytes(&tx), peer(n), NOW).unwrap();
    }
    let mut down = connected();
    down.node.link = OwnNodeLink::Down { since: NOW - 500 };
    let h = fallback_health(down);
    assert_eq!(
        h.line(),
        NodeOnlyLine::CheckingWithOthers(h.problem.unwrap())
    );
    let s = status(&mut w, &h);
    assert_eq!(s.mode, Some(CheckMode::Light));
    assert!(!s.own_node_saw && !s.own_node_counted);
    // Fewer than three groups is no light check (WR-44).
    let mut w = watch();
    w.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    assert_eq!(status(&mut w, &h).mode, None);
}

#[test]
fn wr_68_a_payment_first_seen_by_the_random_peers_keeps_the_light_mode_after_the_node_returns() {
    let tx = pays();
    let mut w = watch();
    for n in 20..23 {
        w.sighting(&bytes(&tx), peer(n), NOW).unwrap();
    }
    // The node is back and the fallback is over (light_fallback stays on).
    let mut back = connected();
    back.light_fallback = true;
    let h = check(&back);
    assert!(h.own.usable);
    assert_eq!(status(&mut w, &h).mode, Some(CheckMode::Light));
    // The node then reports it: its mode wins.
    assert!(w.own_node_saw(tx.compute_txid()));
    let s = status(&mut w, &h);
    assert_eq!(s.mode, Some(CheckMode::OwnNodeOnly));
    assert!(s.own_node_counted);
}

#[test]
fn wr_68_without_light_fallback_the_random_peers_count_for_nothing() {
    let tx = pays();
    let mut w = watch();
    for n in 20..23 {
        w.sighting(&bytes(&tx), peer(n), NOW).unwrap();
    }
    let h = check(&connected());
    assert!(!h.light_fallback);
    assert_eq!(status(&mut w, &h).mode, None);
}
