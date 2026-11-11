//! Section 11: the merchant's own node as a second check (CR-104 to CR-106
//! and CR-108). The rules about where the address is kept and about the
//! driver (CR-100 to CR-103, CR-107) belong to the node, the library for the
//! app and the app, and are tested there.

use bitcoin::Transaction;
use openbmt_core::policy::{CheckMode, InstantLabel, WaitReason};
use openbmt_watch::{OwnNodeHealth, OwnNodeProblem};

use super::kit::*;
use crate::say::{check_line_name, CheckLine, ALL_CHECK_LINES};
use crate::{Event, Facts, Opened, Screen, State, Why};

/// An own node that is connected, level with the counter and heard now.
fn usable(w: &World) -> OwnNodeHealth {
    OwnNodeHealth {
        configured: true,
        usable: true,
        problem: None,
        own_height: Some(100),
        our_height: 100,
        last_heard: Some(w.now),
    }
}

/// An own node that is configured and cannot be used: not reachable.
fn unusable(w: &World) -> OwnNodeHealth {
    OwnNodeHealth {
        configured: true,
        usable: false,
        problem: Some(OwnNodeProblem::Unreachable { since: w.now }),
        own_height: None,
        our_height: 100,
        last_heard: None,
    }
}

impl World {
    fn judge_with(&mut self, index: u32, own: &OwnNodeHealth) -> Screen {
        let facts = Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c
            .judge_with_own_node(index, &facts, own, &mut self.m)
            .unwrap()
    }

    /// A sale of $20 (over the light limit of $10, under the own-node
    /// limit of $25), paid and seen by `groups` groups of the light client.
    fn twenty_seen_by(&mut self, groups: u8) -> (Opened, Transaction) {
        let o = self.open(2_000);
        let t = self.pay(&o, 20_000);
        self.tick(10);
        self.see(&t, groups);
        (o, t)
    }
}

#[test]
fn cr_104_the_own_node_tier_applies_when_the_own_node_counted() {
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(3);
    assert_eq!(w.c.own_node_saw(t.compute_txid()), vec![o.index]);
    let own = usable(&w);
    assert_eq!(w.judge_with(o.index, &own).state, State::Waiting);
    w.tick(8);
    let screen = w.judge_with(o.index, &own);
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(screen.why, Why::Instant(InstantLabel::YourNode));
    assert_eq!(screen.check_mode, Some(CheckMode::NodeAlsoSeenByLight));
    assert!(screen.own_node_saw && screen.own_node_counted);
    assert_eq!(
        w.c.sale(o.index).unwrap().accepted(),
        Some((InstantLabel::YourNode, CheckMode::NodeAlsoSeenByLight))
    );
}

#[test]
fn cr_104_the_own_node_tier_uses_its_own_slice() {
    // Three sales of $20 are $60 at risk: over the light slice of $50, inside
    // the own-node slice of $100. The exposure is counted against the slice of
    // the check that accepted the sale.
    let mut w = World::new();
    let mut sales = Vec::new();
    for _ in 0..3 {
        let (o, t) = w.twenty_seen_by(3);
        w.c.own_node_saw(t.compute_txid());
        sales.push(o.index);
    }
    let own = usable(&w);
    w.tick(8);
    for index in sales {
        assert_eq!(w.judge_with(index, &own).state, State::PaidInstant);
    }
    assert_eq!(w.c.exposure_outstanding(), 60_000);
}

#[test]
fn cr_104_an_own_node_that_cannot_be_used_leaves_the_light_tier() {
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(3);
    w.c.own_node_saw(t.compute_txid());
    w.tick(8);
    let screen = w.judge_with(o.index, &unusable(&w));
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::Wait(WaitReason::OverSaleLimit));
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    assert!(screen.own_node_saw && !screen.own_node_counted);
}

#[test]
fn cr_104_the_own_node_alone_gives_no_check() {
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(2);
    w.c.own_node_saw(t.compute_txid());
    w.tick(8);
    let screen = w.judge_with(o.index, &usable(&w));
    assert_eq!(screen.state, State::Waiting);
    assert_eq!(screen.why, Why::NotSeenEnough);
    assert_eq!(screen.check_mode, None);
    assert!(screen.own_node_saw && !screen.own_node_counted);
}

#[test]
fn cr_104_a_report_for_a_transaction_no_sale_knows_records_nothing() {
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(3);
    let stranger = w.pay_to(&p2wsh(0xaa), 20_000);
    assert!(w.c.own_node_saw(stranger.compute_txid()).is_empty());
    w.tick(8);
    let screen = w.judge_with(o.index, &usable(&w));
    assert!(!screen.own_node_saw);
    assert_eq!(screen.check_mode, Some(CheckMode::Light));
    // The report of the sale's own transaction, once it is known, is taken.
    assert_eq!(w.c.own_node_saw(t.compute_txid()), vec![o.index]);
}

#[test]
fn cr_104_a_report_is_not_kept_in_the_stored_sale() {
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(3);
    let before = w.c.sale(o.index).unwrap().to_bytes();
    w.c.own_node_saw(t.compute_txid());
    assert_eq!(w.c.sale(o.index).unwrap().to_bytes(), before);
    w.restart();
    w.tick(8);
    let screen = w.judge_with(o.index, &usable(&w));
    assert!(!screen.own_node_saw);
}

#[test]
fn cr_104_an_override_is_limited_by_the_check_judging_found() {
    // A cashier may accept up to the per-sale limit of the check: $10 for the
    // light check, $25 when the own node counted.
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(3);
    w.c.own_node_saw(t.compute_txid());
    w.tick(1);
    for (own, ok) in [(unusable(&w), false), (usable(&w), true)] {
        let facts = Facts {
            chain: &w.chain,
            health: w.health,
            heartbeat: heartbeat(),
            remote_order: false,
            now: w.now,
        };
        let done = w.c.override_sale_with_own_node(
            o.index,
            crate::Actor {
                role: crate::Role::Cashier,
                name: "Ana".to_string(),
            },
            &facts,
            &own,
            &mut w.m,
        );
        assert_eq!(done.result.is_ok(), ok);
    }
}

#[test]
fn cr_105_the_waiting_screen_says_which_check_was_done() {
    let mut w = World::new();
    // Light alone.
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 3);
    w.remote_order = true;
    let light = w.judge_with(o.index, &OwnNodeHealth::not_configured());
    assert_eq!(light.state, State::Waiting);
    assert_eq!(light.check_line(), Some(CheckLine::Light));
    // The own node reported, and cannot be used: the fallback is a line.
    w.c.own_node_saw(t.compute_txid());
    let own = unusable(&w);
    let fallback = w.judge_with(o.index, &own);
    assert_eq!(fallback.check_line(), Some(CheckLine::LightOwnNodeUnusable));
    let words = fallback.check_words().unwrap();
    assert!(words.what.contains("cannot be used"), "{}", words.what);
    // The own node counted.
    let own = usable(&w);
    let both = w.judge_with(o.index, &own);
    assert_eq!(both.check_line(), Some(CheckLine::YourNodeAndLight));
    assert!(both.check_words().unwrap().heading.contains("Your node"));
    assert_eq!(both.why, Why::Wait(WaitReason::RemoteOrder));
}

#[test]
fn cr_105_the_own_node_alone_and_a_sale_with_no_check_are_told() {
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.see(&t, 1);
    let none = w.judge_with(o.index, &usable(&w));
    assert_eq!(none.check_line(), None);
    w.c.own_node_saw(t.compute_txid());
    let alone = w.judge_with(o.index, &usable(&w));
    assert_eq!(alone.check_line(), Some(CheckLine::OwnNodeWaitingForLight));
}

#[test]
fn cr_105_every_check_line_has_words_and_a_name() {
    let mut names = std::collections::BTreeSet::new();
    for line in ALL_CHECK_LINES {
        let words = line.words();
        assert_eq!(words.code, Some("CR-105"));
        assert_eq!(openbmt_core::say::check(&words), Ok(()));
        assert!(names.insert(check_line_name(line)));
    }
    assert_eq!(names.len(), 6);
}

#[test]
fn cr_105_an_accepted_sale_says_its_check_in_its_own_words() {
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(3);
    w.c.own_node_saw(t.compute_txid());
    w.tick(8);
    let screen = w.judge_with(o.index, &usable(&w));
    assert_eq!(screen.state, State::PaidInstant);
    assert_eq!(screen.check_line(), None);
    assert!(screen.why_text().contains("your node checked it"));
}

#[test]
fn cr_106_the_record_of_an_instant_acceptance_says_which_check() {
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(3);
    w.c.own_node_saw(t.compute_txid());
    w.tick(8);
    assert_eq!(w.judge_with(o.index, &usable(&w)).state, State::PaidInstant);
    let paid =
        w.m.events()
            .into_iter()
            .find(|e| matches!(e, Event::PaidAtOnce { .. }));
    match paid {
        Some(Event::PaidAtOnce { label, mode, .. }) => {
            assert_eq!(label, InstantLabel::YourNode);
            assert_eq!(mode, CheckMode::NodeAlsoSeenByLight);
        }
        other => panic!("no record of the acceptance: {other:?}"),
    }
    // A light acceptance is recorded as the light check.
    let mut w = World::new();
    let o = w.open_five();
    let t = w.pay(&o, 5_000);
    w.accept_at_once(&o, &t);
    let light =
        w.m.events()
            .into_iter()
            .find(|e| matches!(e, Event::PaidAtOnce { .. }));
    assert!(matches!(
        light,
        Some(Event::PaidAtOnce {
            label: InstantLabel::LightCheck,
            mode: CheckMode::Light,
            ..
        })
    ));
}

#[test]
fn cr_108_without_an_own_node_nothing_changes() {
    let mut w = World::new();
    let (o, t) = w.twenty_seen_by(3);
    // A report with no own node set cannot make the own-node tier.
    w.c.own_node_saw(t.compute_txid());
    w.tick(8);
    let none = OwnNodeHealth::not_configured();
    let with = w.judge_with(o.index, &none);
    assert_eq!(with.why, Why::Wait(WaitReason::OverSaleLimit));
    assert_eq!(with.check_mode, Some(CheckMode::Light));
    assert!(!with.own_node_counted);
    // `judge` is the same call with no own node.
    let plain = w.judge(o.index);
    assert_eq!(plain.why, with.why);
    assert_eq!(plain.check_mode, with.check_mode);
    // And a small sale is accepted by the light check as before.
    let small = w.open_five();
    let ts = w.pay(&small, 5_000);
    w.accept_at_once(&small, &ts);
    assert_eq!(
        w.c.sale(small.index).unwrap().accepted().map(|(l, _)| l),
        Some(InstantLabel::LightCheck)
    );
}
