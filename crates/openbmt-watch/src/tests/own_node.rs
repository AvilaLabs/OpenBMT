//! Section 7: the own node as a source apart from the peers.

use bitcoin::{ScriptBuf, Transaction};
use openbmt_core::policy::CheckMode;
use openbmt_core::say::check;

use super::kit::*;
use crate::{
    check_health, check_own_node, HealthInput, OwnNodeHealth, OwnNodeInput, OwnNodeLink,
    OwnNodeProblem, OwnNodeTracker, Peer, PeerAddr, PeerTransport, SaleStatus, SaleWatch, Source,
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

/// Report the transaction from this many different groups.
fn seen_by(w: &mut SaleWatch, tx: &Transaction, groups: u8) {
    for n in 20..20 + groups {
        w.sighting(&bytes(tx), peer(n), NOW).unwrap();
    }
}

fn connected(best: u32, heard: u64) -> OwnNodeInput {
    OwnNodeInput {
        link: OwnNodeLink::Connected,
        best_height: Some(best),
        last_heard: Some(heard),
    }
}

fn healthy() -> OwnNodeHealth {
    let h = check_own_node(Some(&connected(TIP, NOW)), TIP, NOW, None);
    assert!(h.usable);
    h
}

fn down() -> OwnNodeHealth {
    let input = OwnNodeInput {
        link: OwnNodeLink::Down { since: NOW - 500 },
        best_height: None,
        last_heard: None,
    };
    let h = check_own_node(Some(&input), TIP, NOW, None);
    assert!(!h.usable);
    h
}

fn status(w: &mut SaleWatch, own: &OwnNodeHealth) -> SaleStatus {
    let chain = TestChain::new(1);
    w.status_with_own_node(&chain, NOW, own)
}

// WR-60.

#[test]
fn wr_60_an_own_node_report_is_no_peer_and_no_sighting() {
    let tx = pays();
    // Alone, the report gives no mode, no group, no clock and no amount.
    let mut w = watch();
    assert!(!w.own_node_saw(tx.compute_txid()));
    w.sighting(&bytes(&tx), peer(20), NOW).unwrap();
    assert!(w.own_node_saw(tx.compute_txid()));
    let s = status(&mut w, &healthy());
    assert_eq!(s.mode, None);
    assert_eq!(s.peer_groups, 1);
    assert_eq!(s.secs_since_first_seen, 0);
    assert_eq!(s.received_sats, 10_000);
    // The report does not add a group to the ones that reported it.
    seen_by(&mut w, &tx, 2);
    let s = status(&mut w, &healthy());
    assert_eq!(s.peer_groups, 2);
    assert_eq!(s.mode, None);
}

#[test]
fn wr_60_the_own_node_is_not_among_the_peers_of_the_health_check() {
    // `HealthInput` has no place for it: the same peers give the same
    // answer whatever the own node's state.
    let peers: Vec<Peer> = ["8.8.1.1", "9.9.1.1", "1.1.1.1"]
        .iter()
        .map(|a| Peer {
            addr: PeerAddr::Ip(a.parse().unwrap()),
            serves_filters: *a != "1.1.1.1",
            best_height: TIP,
            transport: PeerTransport::V1,
        })
        .collect();
    let input = HealthInput {
        peers: &peers,
        tip_height: TIP,
        tip_time: NOW - 60,
        now: NOW,
        behind_since: None,
    };
    assert!(check_health(&input).healthy);
    let _ = down();
    assert!(check_health(&input).healthy);
}

// WR-61.

#[test]
fn wr_61_a_report_for_a_transaction_the_sale_does_not_know_is_not_kept() {
    let tx = pays();
    let mut w = watch();
    assert!(!w.own_node_saw(tx.compute_txid()));
    seen_by(&mut w, &tx, 3);
    // Reported before the light client sighted it: nothing was recorded, so
    // the caller has to ask again (WR-65).
    let s = status(&mut w, &healthy());
    assert!(!s.own_node_saw);
    assert_eq!(s.mode, Some(CheckMode::Light));
    assert!(w.own_node_saw(tx.compute_txid()));
    assert!(status(&mut w, &healthy()).own_node_saw);
}

#[test]
fn wr_61_the_report_changes_no_amount_and_names_one_transaction() {
    let mut w = watch();
    let one = spend(1, &[(&shop(), 6_000)]);
    let two = spend(2, &[(&shop(), 4_000)]);
    seen_by(&mut w, &one, 4);
    seen_by(&mut w, &two, 3);
    assert!(w.own_node_saw(one.compute_txid()));
    let s = status(&mut w, &healthy());
    assert_eq!(s.received_sats, 10_000);
    // The weakest part is the one the own node did not report.
    assert!(!s.own_node_saw);
    assert!(w.own_node_saw(two.compute_txid()));
    assert!(status(&mut w, &healthy()).own_node_saw);
}

// WR-62.

#[test]
fn wr_62_the_own_node_and_the_light_client_make_the_own_node_mode() {
    let tx = pays();
    let mut w = watch();
    seen_by(&mut w, &tx, 3);
    assert!(w.own_node_saw(tx.compute_txid()));
    let s = status(&mut w, &healthy());
    assert_eq!(s.mode, Some(CheckMode::NodeAlsoSeenByLight));
    assert!(s.own_node_saw && s.own_node_counted);
}

#[test]
fn wr_62_without_the_light_client_the_own_node_gives_no_mode() {
    let tx = pays();
    let mut w = watch();
    seen_by(&mut w, &tx, 2);
    assert!(w.own_node_saw(tx.compute_txid()));
    let s = status(&mut w, &healthy());
    assert_eq!(s.mode, None);
    assert!(s.own_node_saw && !s.own_node_counted);
}

#[test]
fn wr_62_the_light_check_alone_and_the_box_are_as_before() {
    let tx = pays();
    // No own node: the plain status is what it always was.
    let mut w = watch();
    seen_by(&mut w, &tx, 3);
    assert!(w.own_node_saw(tx.compute_txid()));
    let chain = TestChain::new(1);
    let s = w.status(&chain, NOW);
    assert_eq!(s.mode, Some(CheckMode::Light));
    assert!(s.own_node_saw && !s.own_node_counted);
    // The Box is still the Box.
    let mut w = watch();
    seen_by(&mut w, &tx, 3);
    w.sighting(&bytes(&tx), Source::Box, NOW).unwrap();
    assert_eq!(
        status(&mut w, &OwnNodeHealth::not_configured()).mode,
        Some(CheckMode::NodeAlsoSeenByLight)
    );
    let mut w = watch();
    seen_by(&mut w, &tx, 2);
    w.sighting(&bytes(&tx), Source::Box, NOW).unwrap();
    assert!(w.own_node_saw(tx.compute_txid()));
    assert_eq!(
        status(&mut w, &healthy()).mode,
        Some(CheckMode::BoxReportsOnly)
    );
}

// WR-63.

#[test]
fn wr_63_each_rule_at_its_edge() {
    let at = |input: OwnNodeInput| check_own_node(Some(&input), TIP, NOW, None);
    // Connected, current and heard a moment ago.
    assert_eq!(at(connected(TIP, NOW)).problem, None);
    // Ahead of the counter is no problem; one block behind is none either.
    assert_eq!(at(connected(TIP + 5, NOW)).problem, None);
    assert_eq!(at(connected(TIP - 1, NOW)).problem, None);
    assert_eq!(
        at(connected(TIP - 2, NOW)).problem,
        Some(OwnNodeProblem::Behind { blocks: 2 })
    );
    // Silent for exactly 10 minutes is fine; one second more is not.
    assert_eq!(at(connected(TIP, NOW - 600)).problem, None);
    assert_eq!(
        at(connected(TIP, NOW - 601)).problem,
        Some(OwnNodeProblem::Silent { since: NOW - 601 })
    );
    // A connection that has shown nothing yet.
    let bare = OwnNodeInput {
        link: OwnNodeLink::Connected,
        best_height: None,
        last_heard: Some(NOW),
    };
    assert_eq!(at(bare).problem, Some(OwnNodeProblem::TipUnknown));
    let mute = OwnNodeInput {
        link: OwnNodeLink::Connected,
        best_height: Some(TIP),
        last_heard: None,
    };
    assert_eq!(at(mute).problem, Some(OwnNodeProblem::TipUnknown));
}

#[test]
fn wr_63_no_connection_and_a_refused_node_are_unusable() {
    let h = down();
    assert_eq!(
        h.problem,
        Some(OwnNodeProblem::Unreachable { since: NOW - 500 })
    );
    assert!(h.configured && !h.usable);
    let refused = OwnNodeInput {
        link: OwnNodeLink::Refused { since: NOW - 5 },
        best_height: Some(TIP),
        last_heard: Some(NOW),
    };
    let h = check_own_node(Some(&refused), TIP, NOW, None);
    assert_eq!(h.problem, Some(OwnNodeProblem::Refused { since: NOW - 5 }));
    assert!(!h.usable);
    assert_eq!(h.own_height, Some(TIP));
    assert_eq!(h.last_heard, Some(NOW));
    assert_eq!(h.our_height, TIP);
}

#[test]
fn wr_63_no_own_node_set_is_not_a_problem() {
    let h = check_own_node(None, TIP, NOW, None);
    assert_eq!(h, OwnNodeHealth::not_configured());
    assert!(!h.configured && !h.usable && h.problem.is_none());
    assert!(h.words().is_none());
}

// WR-64.

#[test]
fn wr_64_a_report_while_the_node_is_unusable_falls_back_to_the_light_check() {
    let tx = pays();
    for (own, why) in [
        (down(), "unreachable"),
        (
            check_own_node(Some(&connected(TIP - 4, NOW)), TIP, NOW, None),
            "behind",
        ),
        (
            check_own_node(Some(&connected(TIP, NOW - 700)), TIP, NOW, None),
            "silent",
        ),
    ] {
        let mut w = watch();
        seen_by(&mut w, &tx, 3);
        assert!(w.own_node_saw(tx.compute_txid()));
        let s = status(&mut w, &own);
        assert_eq!(s.mode, Some(CheckMode::Light), "{why}");
        // Never silent: the node saw it, was not counted, and the health
        // line says why.
        assert!(s.own_node_saw && !s.own_node_counted, "{why}");
        let words = own.words().expect("the health line has words");
        assert!(words.what.contains("light check"), "{why}: {}", words.what);
        assert_eq!(words.code, Some("WR-63"));
    }
    // The same sale with the node back is the own-node mode again.
    let mut w = watch();
    seen_by(&mut w, &tx, 3);
    assert!(w.own_node_saw(tx.compute_txid()));
    assert_eq!(
        status(&mut w, &healthy()).mode,
        Some(CheckMode::NodeAlsoSeenByLight)
    );
}

fn link_down(since: u64, last_heard: Option<u64>) -> OwnNodeInput {
    OwnNodeInput {
        link: OwnNodeLink::Down { since },
        best_height: None,
        last_heard,
    }
}

#[test]
fn wr_63_a_first_connection_is_connecting_for_120_seconds() {
    let mut t = OwnNodeTracker::new();
    let start = link_down(NOW, None);
    let h = t.check(Some(&start), TIP, NOW + 1);
    assert_eq!(h.problem, Some(OwnNodeProblem::Connecting { since: NOW }));
    assert!(h.configured && !h.usable);
    assert_eq!(
        t.check(Some(&start), TIP, NOW + 119).problem,
        Some(OwnNodeProblem::Connecting { since: NOW })
    );
    let w = h.words().expect("words");
    assert_eq!(check(&w), Ok(()));
    assert_eq!(w.heading, "Connecting to your node");
    assert_eq!(w.code, Some("WR-63"));
}

#[test]
fn wr_63_after_120_seconds_with_no_connection_it_is_not_reachable() {
    let mut t = OwnNodeTracker::new();
    let start = link_down(NOW, None);
    assert!(t.check(Some(&start), TIP, NOW).problem.is_some());
    assert_eq!(
        t.check(Some(&start), TIP, NOW + 120).problem,
        Some(OwnNodeProblem::Unreachable { since: NOW })
    );
    assert_eq!(
        t.check(Some(&start), TIP, NOW + 600).problem,
        Some(OwnNodeProblem::Unreachable { since: NOW })
    );
}

#[test]
fn wr_63_a_drop_after_a_connection_is_not_reachable_at_once() {
    let mut t = OwnNodeTracker::new();
    assert!(t.check(Some(&connected(TIP, NOW)), TIP, NOW).usable);
    let dropped = link_down(NOW + 5, Some(NOW + 4));
    assert_eq!(
        t.check(Some(&dropped), TIP, NOW + 5).problem,
        Some(OwnNodeProblem::Unreachable { since: NOW + 5 })
    );
}

#[test]
fn wr_63_a_retry_starts_a_new_120_second_grace() {
    let mut t = OwnNodeTracker::new();
    let start = link_down(NOW, None);
    assert!(matches!(
        t.check(Some(&start), TIP, NOW + 200).problem,
        Some(OwnNodeProblem::Unreachable { .. })
    ));
    t.retry(NOW + 300);
    assert_eq!(
        t.check(Some(&start), TIP, NOW + 301).problem,
        Some(OwnNodeProblem::Connecting { since: NOW + 300 })
    );
    assert!(matches!(
        t.check(Some(&start), TIP, NOW + 420).problem,
        Some(OwnNodeProblem::Unreachable { .. })
    ));
}

#[test]
fn wr_63_a_refused_node_has_no_grace() {
    let mut t = OwnNodeTracker::new();
    let refused = OwnNodeInput {
        link: OwnNodeLink::Refused { since: NOW },
        best_height: Some(TIP),
        last_heard: Some(NOW),
    };
    assert_eq!(
        t.check(Some(&refused), TIP, NOW + 1).problem,
        Some(OwnNodeProblem::Refused { since: NOW })
    );
}

#[test]
fn wr_63_one_block_of_lag_is_allowed_for_five_minutes() {
    let one = connected(TIP - 1, NOW);
    let mut t = OwnNodeTracker::new();
    // One block behind for 299 s: still the own node's tier.
    assert!(t.check(Some(&one), TIP, NOW - 299).usable);
    assert!(t.check(Some(&one), TIP, NOW - 100).usable);
    assert!(t.check(Some(&one), TIP, NOW).usable);
    // Exactly 300 s is not more than 300 s; 301 s falls back, and says so.
    let mut t = OwnNodeTracker::new();
    assert!(
        t.check(Some(&connected(TIP - 1, NOW - 300)), TIP, NOW - 300)
            .usable
    );
    assert!(t.check(Some(&connected(TIP - 1, NOW - 1)), TIP, NOW).usable);
    let h = t.check(Some(&connected(TIP - 1, NOW)), TIP, NOW + 1);
    assert_eq!(h.problem, Some(OwnNodeProblem::Behind { blocks: 1 }));
    assert!(!h.usable);
    let w = h.words().expect("words");
    assert_eq!(check(&w), Ok(()));
    assert!(w.what.contains("a block behind"), "{}", w.what);
    // Two blocks fall back at once, with no wait.
    let mut t = OwnNodeTracker::new();
    let h = t.check(Some(&connected(TIP - 2, NOW)), TIP, NOW);
    assert_eq!(h.problem, Some(OwnNodeProblem::Behind { blocks: 2 }));
    // Catching up returns to the own-node tier, and the count starts over.
    let h = t.check(Some(&connected(TIP, NOW)), TIP, NOW + 10);
    assert!(h.usable);
    let h = t.check(Some(&connected(TIP - 1, NOW + 20)), TIP, NOW + 20);
    assert!(h.usable);
    let h = t.check(Some(&connected(TIP - 1, NOW + 320)), TIP, NOW + 320);
    assert!(h.usable, "300 s from the new start is not more than 300 s");
    // A drop of the link also starts the count over.
    let down = OwnNodeInput {
        link: OwnNodeLink::Down { since: NOW + 330 },
        best_height: Some(TIP - 1),
        last_heard: Some(NOW),
    };
    assert!(!t.check(Some(&down), TIP, NOW + 330).usable);
    assert!(
        t.check(Some(&connected(TIP - 1, NOW + 340)), TIP, NOW + 340)
            .usable
    );
}

#[test]
fn wr_64_every_problem_has_words_with_its_time_and_rule() {
    let down = OwnNodeProblem::Unreachable { since: NOW }.words();
    assert_eq!(down.at, Some(NOW));
    assert!(down.what.contains("cannot reach"));
    let behind = OwnNodeProblem::Behind { blocks: 7 }.words();
    assert!(behind.what.contains("7 blocks behind"));
    assert_eq!(behind.at, None);
    for problem in [
        OwnNodeProblem::Connecting { since: 1 },
        OwnNodeProblem::Unreachable { since: 1 },
        OwnNodeProblem::Refused { since: 1 },
        OwnNodeProblem::Silent { since: 1 },
        OwnNodeProblem::Behind { blocks: 2 },
        OwnNodeProblem::TipUnknown,
    ] {
        let w = problem.words();
        assert_eq!(check(&w), Ok(()), "{problem:?}");
        assert_eq!(w.code, Some(problem.rule()));
        assert!(!w.what.contains("WR-"));
    }
    let ok = healthy().words().expect("a configured node has a line");
    assert_eq!(check(&ok), Ok(()));
    assert_eq!(ok.at, Some(NOW));
}

// WR-65.

#[test]
fn wr_65_a_report_is_not_stored_and_a_restart_asks_again() {
    let tx = pays();
    let mut with = watch();
    seen_by(&mut with, &tx, 3);
    let mut without = with.clone();
    assert!(with.own_node_saw(tx.compute_txid()));
    // The snapshot is the same bytes with or without the report.
    assert_eq!(with.to_bytes(), without.to_bytes());
    let mut restored = SaleWatch::from_bytes(&with.to_bytes()).unwrap();
    assert!(!status(&mut restored, &healthy()).own_node_saw);
    assert_eq!(
        status(&mut restored, &healthy()).mode,
        Some(CheckMode::Light)
    );
    // Asked again, the node's answer is taken again.
    assert!(restored.own_node_saw(tx.compute_txid()));
    assert_eq!(
        status(&mut restored, &healthy()).mode,
        Some(CheckMode::NodeAlsoSeenByLight)
    );
    assert!(!status(&mut without, &healthy()).own_node_saw);
}
