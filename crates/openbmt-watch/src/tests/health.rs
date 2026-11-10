//! Section 6: health, each threshold at its exact edge.

use crate::{check_health, HealthInput, Peer, PeerAddr, PeerTransport, Problem};

const NOW: u64 = 1_700_000_000;
const TIP: u32 = 800_000;

fn peer(addr: &str, filters: bool, best_height: u32) -> Peer {
    Peer {
        addr: PeerAddr::Ip(addr.parse().unwrap()),
        serves_filters: filters,
        best_height,
        transport: PeerTransport::V1,
    }
}

/// Three groups, two of them with a peer that serves filters.
fn good_peers() -> Vec<Peer> {
    vec![
        peer("8.8.1.1", true, TIP),
        peer("9.9.1.1", true, TIP),
        peer("1.1.1.1", false, TIP),
    ]
}

fn input(peers: &[Peer]) -> HealthInput<'_> {
    HealthInput {
        peers,
        tip_height: TIP,
        tip_time: NOW - 60,
        now: NOW,
        behind_since: None,
    }
}

fn problem(i: &HealthInput<'_>) -> Option<Problem> {
    let h = check_health(i);
    assert_eq!(h.healthy, h.problem.is_none());
    h.problem
}

#[test]
fn wr_50_peers_in_three_groups_with_filters_in_two_are_healthy() {
    let peers = good_peers();
    let h = check_health(&input(&peers));
    assert!(h.healthy && h.problem.is_none() && !h.behind);
}

#[test]
fn wr_50_two_groups_are_too_few_however_many_peers() {
    let peers = vec![
        peer("8.8.1.1", true, TIP),
        peer("8.8.2.2", true, TIP),
        peer("9.9.1.1", true, TIP),
        peer("9.9.2.2", true, TIP),
    ];
    assert_eq!(problem(&input(&peers)), Some(Problem::TooFewPeerGroups));
    assert_eq!(problem(&input(&[])), Some(Problem::TooFewPeerGroups));
}

#[test]
fn wr_50_all_named_peers_are_one_group() {
    let mut peers: Vec<Peer> = (0..10)
        .map(|_| Peer {
            addr: PeerAddr::Named,
            serves_filters: true,
            best_height: TIP,
            transport: PeerTransport::V1,
        })
        .collect();
    assert_eq!(problem(&input(&peers)), Some(Problem::TooFewPeerGroups));
    peers.push(peer("8.8.1.1", true, TIP));
    peers.push(peer("9.9.1.1", true, TIP));
    assert_eq!(problem(&input(&peers)), None);
}

#[test]
fn wr_50_local_peers_are_one_group() {
    let peers = vec![
        peer("10.0.0.1", true, TIP),
        peer("192.168.0.1", true, TIP),
        peer("127.0.0.1", true, TIP),
        peer("8.8.1.1", true, TIP),
    ];
    assert_eq!(problem(&input(&peers)), Some(Problem::TooFewPeerGroups));
}

#[test]
fn wr_50_filters_in_one_group_only_are_too_few() {
    let peers = vec![
        peer("8.8.1.1", true, TIP),
        peer("8.8.2.2", true, TIP),
        peer("9.9.1.1", false, TIP),
        peer("1.1.1.1", false, TIP),
    ];
    assert_eq!(problem(&input(&peers)), Some(Problem::TooFewFilterPeers));
    let none = vec![
        peer("8.8.1.1", false, TIP),
        peer("9.9.1.1", false, TIP),
        peer("1.1.1.1", false, TIP),
    ];
    assert_eq!(problem(&input(&none)), Some(Problem::TooFewFilterPeers));
}

#[test]
fn wr_50_filter_peers_count_by_group_not_by_address() {
    let peers = vec![
        peer("8.8.1.1", true, TIP),
        peer("8.8.2.2", true, TIP),
        peer("8.8.3.3", true, TIP),
        peer("9.9.1.1", false, TIP),
        peer("1.1.1.1", false, TIP),
    ];
    assert_eq!(problem(&input(&peers)), Some(Problem::TooFewFilterPeers));
}

#[test]
fn wr_51_a_tip_two_hours_behind_is_still_healthy_and_one_second_more_is_not() {
    let peers = good_peers();
    let mut i = input(&peers);
    i.tip_time = NOW - 7_200;
    assert_eq!(problem(&i), None);
    i.tip_time = NOW - 7_201;
    assert_eq!(problem(&i), Some(Problem::TipIsOld));
}

#[test]
fn wr_51_a_tip_ahead_of_the_clock_is_not_old() {
    let peers = good_peers();
    let mut i = input(&peers);
    i.tip_time = NOW + 1;
    assert_eq!(problem(&i), None);
}

#[test]
fn wr_52_behind_means_more_than_two_blocks_above_the_tip() {
    let mut peers = good_peers();
    peers[0].best_height = TIP + 2;
    assert!(!check_health(&input(&peers)).behind);
    peers[0].best_height = TIP + 3;
    let h = check_health(&input(&peers));
    assert!(h.behind);
    // The clock has not started: still healthy.
    assert!(h.healthy);
}

#[test]
fn wr_52_ten_minutes_behind_is_still_healthy_and_one_second_more_is_not() {
    let mut peers = good_peers();
    peers[1].best_height = TIP + 3;
    let mut i = input(&peers);
    i.behind_since = Some(NOW - 600);
    let h = check_health(&i);
    assert!(h.behind && h.healthy);
    i.behind_since = Some(NOW - 601);
    let h = check_health(&i);
    assert!(h.behind && !h.healthy);
    assert_eq!(h.problem, Some(Problem::HeadersBehind));
}

#[test]
fn wr_52_a_stored_time_does_not_matter_when_no_peer_is_ahead() {
    let peers = good_peers();
    let mut i = input(&peers);
    i.behind_since = Some(NOW - 100_000);
    let h = check_health(&i);
    assert!(!h.behind && h.healthy);
}

#[test]
fn wr_52_a_clock_before_the_stored_time_is_not_behind_for_long() {
    let mut peers = good_peers();
    peers[0].best_height = TIP + 10;
    let mut i = input(&peers);
    i.behind_since = Some(NOW + 5_000);
    assert!(check_health(&i).healthy);
}

#[test]
fn wr_52_the_largest_height_does_not_overflow() {
    let mut peers = good_peers();
    peers[0].best_height = u32::MAX;
    let mut i = input(&peers);
    i.tip_height = u32::MAX;
    assert!(!check_health(&i).behind);
    i.tip_height = u32::MAX - 2;
    assert!(!check_health(&i).behind);
    i.tip_height = u32::MAX - 3;
    assert!(check_health(&i).behind);
}

#[test]
fn wr_53_a_clock_two_hours_behind_the_tip_is_healthy_and_one_second_more_is_not() {
    let peers = good_peers();
    let mut i = input(&peers);
    i.tip_time = NOW + 7_200;
    assert_eq!(problem(&i), None);
    i.tip_time = NOW + 7_201;
    assert_eq!(problem(&i), Some(Problem::ClockMismatch));
}

#[test]
fn wr_53_the_largest_times_do_not_overflow() {
    let peers = good_peers();
    let mut i = input(&peers);
    i.tip_time = u64::MAX;
    i.now = 0;
    assert_eq!(problem(&i), Some(Problem::ClockMismatch));
    i.tip_time = 0;
    i.now = u64::MAX;
    assert_eq!(problem(&i), Some(Problem::TipIsOld));
}

#[test]
fn wr_50_the_first_problem_in_the_order_of_the_rules_is_named() {
    // Everything is wrong at once; fix one rule at a time.
    let ahead = TIP + 50;
    let at = |peers: &[Peer], tip_time: u64, behind_since: Option<u64>| {
        problem(&HealthInput {
            peers,
            tip_height: TIP,
            tip_time,
            now: NOW,
            behind_since,
        })
    };
    let old = NOW - 100_000;
    let stale = Some(NOW - 10_000);
    let one = [peer("8.8.1.1", false, ahead)];
    assert_eq!(at(&one, old, stale), Some(Problem::TooFewPeerGroups));
    let no_filters = [
        peer("8.8.1.1", false, ahead),
        peer("9.9.1.1", false, ahead),
        peer("1.1.1.1", false, ahead),
    ];
    assert_eq!(
        at(&no_filters, old, stale),
        Some(Problem::TooFewFilterPeers)
    );
    let fine = [
        peer("8.8.1.1", true, ahead),
        peer("9.9.1.1", true, ahead),
        peer("1.1.1.1", false, ahead),
    ];
    assert_eq!(at(&fine, old, stale), Some(Problem::TipIsOld));
    assert_eq!(at(&fine, NOW, stale), Some(Problem::HeadersBehind));
    assert_eq!(at(&fine, NOW, None), None);
    assert_eq!(at(&fine, NOW + 100_000, None), Some(Problem::ClockMismatch));
}

#[test]
fn wr_51_problems_name_their_rules() {
    assert_eq!(Problem::TooFewPeerGroups.rule(), "WR-50");
    assert_eq!(Problem::TooFewFilterPeers.rule(), "WR-50");
    assert_eq!(Problem::TipIsOld.rule(), "WR-51");
    assert_eq!(Problem::HeadersBehind.rule(), "WR-52");
    assert_eq!(Problem::ClockMismatch.rule(), "WR-53");
}
