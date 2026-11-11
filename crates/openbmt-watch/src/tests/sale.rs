//! Sections 4 and 5: what a sale holds, who saw it and conflicts.

use bitcoin::{Amount, Block, ScriptBuf, Transaction, TxOut, Witness};
use openbmt_core::policy::CheckMode;

use super::kit::*;
use crate::{
    spends_in_block, PeerAddr, Refusal, SaleStatus, SaleWatch, Sighted, Source,
    MAX_SALE_TRANSACTIONS, MAX_TX_BYTES,
};

const NOW: u64 = 1_000_000;

fn shop() -> ScriptBuf {
    script(1)
}

fn watch(sale: u64, bps: u32) -> SaleWatch {
    SaleWatch::new(shop(), sale, bps)
}

/// A peer in its own /16 for each `n` from 20 up.
fn peer(n: u8) -> Source {
    Source::Peer(PeerAddr::Ip(format!("{n}.{n}.1.1").parse().unwrap()))
}

fn pays(prev: u8, sats: u64) -> Transaction {
    spend(prev, &[(&shop(), sats)])
}

fn sight(w: &mut SaleWatch, tx: &Transaction, source: Source, now: u64) -> Sighted {
    w.sighting(&bytes(tx), source, now).unwrap()
}

/// Report the transaction from this many different groups.
fn seen_by(w: &mut SaleWatch, tx: &Transaction, groups: u8, now: u64) {
    for n in 20..20 + groups {
        sight(w, tx, peer(n), now);
    }
}

/// A block of a coinbase and these transactions; the sale is told that the
/// block holds them and the chain puts it at `height`.
fn confirm_in(w: &mut SaleWatch, chain: &mut TestChain, txs: &[Transaction], height: u32) -> Block {
    let mut all = vec![coinbase(height as u8)];
    all.extend(txs.iter().cloned());
    let b = block_at(all, height);
    chain.add(b.block_hash(), height);
    for position in 1..b.txdata.len() {
        w.confirmed(proven(&b, position as u32, height)).unwrap();
    }
    b
}

fn status(w: &mut SaleWatch, chain: &TestChain) -> SaleStatus {
    status_at(w, chain, NOW)
}

// WR-30, WR-31.

#[test]
fn wr_30_only_outputs_to_the_sales_script_count() {
    let mut w = watch(10_000, 0);
    let tx = spend(
        1,
        &[(&shop(), 6_000), (&script(0x55), 9_000), (&shop(), 1_500)],
    );
    assert_eq!(sight(&mut w, &tx, peer(20), NOW), Sighted::New);
    let chain = TestChain::new(100);
    assert_eq!(status(&mut w, &chain).received_sats, 7_500);
}

#[test]
fn wr_30_a_script_that_only_looks_like_the_sales_script_does_not_count() {
    let mut w = watch(10_000, 0);
    let mut near = shop().into_bytes();
    near.push(0x00);
    let near = ScriptBuf::from_bytes(near);
    let tx = spend(1, &[(&near, 10_000)]);
    assert_eq!(sight(&mut w, &tx, peer(20), NOW), Sighted::Ignored);
    assert_eq!(status(&mut w, &TestChain::new(1)).received_sats, 0);
}

#[test]
fn wr_31_a_transaction_counts_once_however_often_it_is_reported() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    assert_eq!(sight(&mut w, &tx, peer(20), NOW), Sighted::New);
    assert_eq!(sight(&mut w, &tx, peer(20), NOW), Sighted::Known);
    assert_eq!(sight(&mut w, &tx, peer(21), NOW), Sighted::Known);
    assert_eq!(sight(&mut w, &tx, Source::Box, NOW), Sighted::Known);
    assert_eq!(w.transactions().count(), 1);
    assert_eq!(status(&mut w, &TestChain::new(1)).received_sats, 10_000);
}

#[test]
fn wr_31_the_same_transaction_with_other_witness_bytes_is_the_same() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    let mut other = tx.clone();
    other.input[0].witness = Witness::from_slice(&[vec![1u8, 2, 3]]);
    assert_eq!(tx.compute_txid(), other.compute_txid());
    assert_ne!(bytes(&tx), bytes(&other));
    sight(&mut w, &tx, peer(20), NOW);
    assert_eq!(sight(&mut w, &other, peer(21), NOW), Sighted::Known);
    assert_eq!(status(&mut w, &TestChain::new(1)).received_sats, 10_000);
}

#[test]
fn wr_31_a_customer_who_pays_in_two_parts_adds_up() {
    let mut w = watch(10_000, 0);
    sight(&mut w, &pays(1, 6_000), peer(20), NOW);
    sight(&mut w, &pays(2, 4_000), peer(20), NOW);
    assert_eq!(status(&mut w, &TestChain::new(1)).received_sats, 10_000);
}

#[test]
fn wr_31_a_transaction_seen_and_then_confirmed_counts_once() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, peer(20), NOW);
    let mut chain = TestChain::new(100);
    confirm_in(&mut w, &mut chain, &[tx], 100);
    let s = status(&mut w, &chain);
    assert_eq!((s.received_sats, s.confirmed_sats), (10_000, 10_000));
    assert_eq!(w.transactions().count(), 1);
}

#[test]
fn wr_31_two_conflicting_transactions_count_once_at_the_smaller_amount() {
    // Only one of the two can confirm. Counting both would show a sale of
    // 13,000 as paid when at most 10,000 can ever arrive.
    let mut w = watch(13_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), NOW);
    assert_eq!(
        sight(&mut w, &pays(1, 3_000), peer(21), NOW),
        Sighted::Conflicting
    );
    let s = status(&mut w, &TestChain::new(1));
    assert_eq!(s.received_sats, 3_000);
    assert!(s.conflict_seen);
}

#[test]
fn wr_31_a_chain_of_conflicts_counts_once_and_others_still_add_up() {
    let mut w = watch(50_000, 0);
    // a and c do not conflict with each other; b conflicts with both and
    // pays less than either, so it is no fee bump. If b confirms, neither
    // a nor c can.
    let a = spend_coins(&[coin(1)], &[(&shop(), 10_000)]);
    let c = spend_coins(&[coin(2)], &[(&shop(), 7_000)]);
    let b = spend_coins(&[coin(1), coin(2)], &[(&shop(), 4_000)]);
    let apart = pays(9, 20_000);
    for tx in [&a, &c, &b, &apart] {
        sight(&mut w, tx, peer(20), NOW);
    }
    let s = status(&mut w, &TestChain::new(1));
    assert_eq!(s.received_sats, 4_000 + 20_000);
}

#[test]
fn wr_31_once_one_of_the_conflicting_transactions_confirms_its_amount_counts() {
    let mut w = watch(10_000, 0);
    let good = pays(1, 10_000);
    sight(&mut w, &good, peer(20), NOW);
    sight(&mut w, &pays(1, 3_000), peer(21), NOW);
    let mut chain = TestChain::new(5);
    confirm_in(&mut w, &mut chain, &[good], 5);
    let s = status(&mut w, &chain);
    assert_eq!((s.received_sats, s.confirmed_sats), (10_000, 10_000));
}

#[test]
fn wr_31_an_empty_watch_has_nothing() {
    let mut w = watch(10_000, 0);
    let s = status(&mut w, &TestChain::new(1));
    assert_eq!(
        s,
        SaleStatus {
            received_sats: 0,
            confirmed_sats: 0,
            min_confirmations: 0,
            conflict_seen: false,
            mode: None,
            peer_groups: 0,
            secs_since_first_seen: 0,
            unconfirmed_again: false,
            needs_a_person: false,
            own_node_saw: false,
            own_node_counted: false,
        }
    );
}

// WR-32.

#[test]
fn wr_32_the_confirmations_of_a_single_payment_are_the_tip_minus_height_plus_one() {
    let mut w = watch(10_000, 0);
    let mut chain = TestChain::new(100);
    confirm_in(&mut w, &mut chain, &[pays(1, 10_000)], 100);
    assert_eq!(status(&mut w, &chain).min_confirmations, 1);
    chain.tip = 105;
    assert_eq!(status(&mut w, &chain).min_confirmations, 6);
}

#[test]
fn wr_32_a_payment_that_is_only_seen_is_not_confirmed() {
    let mut w = watch(10_000, 0);
    seen_by(&mut w, &pays(1, 10_000), 5, NOW);
    let s = status(&mut w, &TestChain::new(100));
    assert_eq!((s.min_confirmations, s.confirmed_sats), (0, 0));
    assert_eq!(s.received_sats, 10_000);
}

#[test]
fn wr_32_the_tolerance_is_in_basis_points_rounded_down_at_the_exact_satoshi() {
    // 10,001 x 50 bps = 50.005, rounded down to 50: 9,951 is enough.
    let mut chain = TestChain::new(100);
    let mut enough = watch(10_001, 50);
    confirm_in(&mut enough, &mut chain, &[pays(1, 9_951)], 100);
    assert_eq!(status(&mut enough, &chain).min_confirmations, 1);
    let mut short = watch(10_001, 50);
    confirm_in(&mut short, &mut chain, &[pays(2, 9_950)], 99);
    assert_eq!(status(&mut short, &chain).min_confirmations, 0);
    // 199 x 50 bps = 0.995, rounded down to 0: nothing may be missing.
    let mut small = watch(199, 50);
    confirm_in(&mut small, &mut chain, &[pays(3, 198)], 98);
    assert_eq!(status(&mut small, &chain).min_confirmations, 0);
    let mut exact = watch(199, 50);
    confirm_in(&mut exact, &mut chain, &[pays(4, 199)], 97);
    assert!(status(&mut exact, &chain).min_confirmations >= 1);
}

#[test]
fn wr_32_a_tolerance_above_10000_basis_points_is_taken_as_10000() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 50_000);
    confirm_in(&mut w, &mut chain, &[pays(1, 1)], 100);
    // Everything may be missing, but something was paid.
    assert_eq!(status(&mut w, &chain).min_confirmations, 1);
    let mut nothing = watch(10_000, 50_000);
    assert_eq!(status(&mut nothing, &chain).min_confirmations, 0);
}

#[test]
fn wr_32_the_product_of_the_amount_and_the_tolerance_does_not_overflow() {
    let mut chain = TestChain::new(100);
    let mut w = watch(u64::MAX, 10_000);
    confirm_in(&mut w, &mut chain, &[pays(1, 1)], 100);
    assert_eq!(status(&mut w, &chain).min_confirmations, 1);
    let mut w = watch(u64::MAX, 1);
    confirm_in(
        &mut w,
        &mut chain,
        &[pays(2, u64::MAX / 10_000 * 9_999)],
        99,
    );
    assert_eq!(status(&mut w, &chain).min_confirmations, 0);
}

#[test]
fn wr_32_the_depth_is_the_largest_at_which_enough_is_confirmed() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    // 6,000 at depth 3 and 4,000 at depth 1.
    confirm_in(&mut w, &mut chain, &[pays(1, 6_000)], 98);
    confirm_in(&mut w, &mut chain, &[pays(2, 4_000)], 100);
    let s = status(&mut w, &chain);
    assert_eq!(s.min_confirmations, 1);
    assert_eq!(s.confirmed_sats, 10_000);
    // With a 40% tolerance the first part alone is enough at depth 3.
    let mut w = watch(10_000, 4_000);
    confirm_in(&mut w, &mut chain, &[pays(3, 6_000)], 98);
    confirm_in(&mut w, &mut chain, &[pays(4, 4_000)], 100);
    assert_eq!(status(&mut w, &chain).min_confirmations, 3);
}

#[test]
fn wr_32_a_confirmed_part_and_a_part_only_seen_is_not_confirmed() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    confirm_in(&mut w, &mut chain, &[pays(1, 6_000)], 100);
    sight(&mut w, &pays(2, 4_000), peer(20), NOW);
    let s = status(&mut w, &chain);
    assert_eq!(s.min_confirmations, 0);
    assert_eq!((s.confirmed_sats, s.received_sats), (6_000, 10_000));
}

// WR-33.

#[test]
fn wr_33_a_block_that_leaves_the_chain_takes_its_confirmations_back() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, peer(20), NOW);
    let b = confirm_in(&mut w, &mut chain, &[tx], 100);
    let s = status(&mut w, &chain);
    assert_eq!(s.min_confirmations, 1);
    assert!(!s.unconfirmed_again);

    chain.remove(&b.block_hash());
    let s = status(&mut w, &chain);
    assert_eq!((s.min_confirmations, s.confirmed_sats), (0, 0));
    assert!(s.unconfirmed_again);
    // The payment goes back to "seen": it still counts as received.
    assert_eq!(s.received_sats, 10_000);
}

#[test]
fn wr_33_unconfirmed_again_is_true_exactly_once() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    let b = confirm_in(&mut w, &mut chain, &[pays(1, 10_000)], 100);
    assert!(!status(&mut w, &chain).unconfirmed_again);
    chain.remove(&b.block_hash());
    assert!(status(&mut w, &chain).unconfirmed_again);
    assert!(!status(&mut w, &chain).unconfirmed_again);
    assert!(!status(&mut w, &chain).unconfirmed_again);
}

#[test]
fn wr_33_a_payment_confirmed_again_in_another_block_is_confirmed_again() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    let b = confirm_in(&mut w, &mut chain, std::slice::from_ref(&tx), 100);
    status(&mut w, &chain);
    chain.remove(&b.block_hash());
    assert!(status(&mut w, &chain).unconfirmed_again);
    chain.tip = 101;
    confirm_in(&mut w, &mut chain, &[tx], 101);
    let s = status(&mut w, &chain);
    assert_eq!(s.min_confirmations, 1);
    assert!(!s.unconfirmed_again);
}

#[test]
fn wr_33_a_reorg_that_keeps_the_payment_confirmed_is_not_unconfirmed_again() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    let a = confirm_in(&mut w, &mut chain, std::slice::from_ref(&tx), 100);
    status(&mut w, &chain);
    // The same transaction is in the block that replaces `a`.
    chain.remove(&a.block_hash());
    chain.tip = 101;
    confirm_in(&mut w, &mut chain, &[tx], 101);
    let s = status(&mut w, &chain);
    assert!(!s.unconfirmed_again);
    assert_eq!(s.min_confirmations, 1);
}

#[test]
fn wr_33_nothing_that_was_never_confirmed_is_unconfirmed_again() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), NOW);
    for _ in 0..3 {
        assert!(!status(&mut w, &chain).unconfirmed_again);
    }
    chain.tip = 200;
    assert!(!status(&mut w, &chain).unconfirmed_again);
}

// WR-34.

#[test]
fn wr_34_the_21st_transaction_is_refused_and_the_sale_needs_a_person() {
    let mut w = watch(1_000_000, 0);
    for n in 0..MAX_SALE_TRANSACTIONS as u8 {
        assert_eq!(
            sight(&mut w, &pays(n + 1, 100), peer(20), NOW),
            Sighted::New
        );
    }
    let chain = TestChain::new(1);
    assert!(!status(&mut w, &chain).needs_a_person);
    let twenty_first = pays(100, 100);
    let refused_ = w.sighting(&bytes(&twenty_first), peer(20), NOW);
    refused(refused_, Refusal::TooManyTransactions, "WR-34");
    let s = status(&mut w, &chain);
    assert!(s.needs_a_person);
    // The 20 kept transactions still count, the 21st does not.
    assert_eq!(s.received_sats, 2_000);
    assert_eq!(w.transactions().count(), 20);
    // A proven payment is refused the same way.
    let b = block(vec![coinbase(1), twenty_first]);
    refused(
        w.confirmed(proven(&b, 1, 5)),
        Refusal::TooManyTransactions,
        "WR-34",
    );
}

#[test]
fn wr_34_a_known_transaction_or_a_fee_bump_still_fits_in_a_full_sale() {
    let mut w = watch(1_000_000, 0);
    for n in 0..MAX_SALE_TRANSACTIONS as u8 {
        sight(&mut w, &pays(n + 1, 100), peer(20), NOW);
    }
    assert_eq!(sight(&mut w, &pays(1, 100), peer(21), NOW), Sighted::Known);
    assert_eq!(
        sight(&mut w, &pays(1, 150), peer(21), NOW),
        Sighted::Replaced
    );
    assert_eq!(w.transactions().count(), 20);
    assert!(!status(&mut w, &TestChain::new(1)).needs_a_person);
}

#[test]
fn wr_34_a_conflict_that_is_kept_counts_toward_the_limit() {
    let mut w = watch(1_000_000, 0);
    for n in 0..MAX_SALE_TRANSACTIONS as u8 {
        sight(&mut w, &pays(n + 1, 100), peer(20), NOW);
    }
    let cancel = spend(1, &[(&script(0x55), 100)]);
    refused(
        w.sighting(&bytes(&cancel), peer(20), NOW),
        Refusal::TooManyTransactions,
        "WR-34",
    );
    assert!(status(&mut w, &TestChain::new(1)).needs_a_person);
}

/// A transaction that pays the shop and is exactly this many bytes.
fn pays_with_size(prev: u8, total: usize) -> Transaction {
    let make = |padding: usize| {
        let mut tx = pays(prev, 1_000);
        tx.output.push(TxOut {
            value: Amount::from_sat(0),
            script_pubkey: ScriptBuf::from_bytes(vec![0x6a; padding]),
        });
        tx
    };
    let first = make(total - 1_000);
    let padding = total - 1_000 + total - first.total_size();
    let tx = make(padding);
    assert_eq!(tx.total_size(), total);
    tx
}

#[test]
fn wr_34_a_transaction_over_400000_bytes_is_refused() {
    let mut w = watch(10_000, 0);
    let at_limit = pays_with_size(1, MAX_TX_BYTES);
    assert_eq!(sight(&mut w, &at_limit, peer(20), NOW), Sighted::New);
    let over = pays_with_size(2, MAX_TX_BYTES + 1);
    refused(
        w.sighting(&bytes(&over), peer(20), NOW),
        Refusal::TransactionTooLarge {
            len: MAX_TX_BYTES + 1,
        },
        "WR-34",
    );
    let s = status(&mut w, &TestChain::new(1));
    assert!(s.needs_a_person);
    assert_eq!(s.received_sats, 1_000);
}

#[test]
fn wr_34_a_large_transaction_that_pays_nothing_is_ignored_not_a_reason_for_a_person() {
    let mut w = watch(10_000, 0);
    let mut tx = spend(1, &[(&script(0x55), 1_000)]);
    tx.output.push(TxOut {
        value: Amount::from_sat(0),
        script_pubkey: ScriptBuf::from_bytes(vec![0x6a; MAX_TX_BYTES + 10]),
    });
    assert_eq!(sight(&mut w, &tx, peer(20), NOW), Sighted::Ignored);
    assert!(!status(&mut w, &TestChain::new(1)).needs_a_person);
}

// WR-40.

#[test]
fn wr_40_a_transaction_that_pays_nothing_and_conflicts_with_nothing_is_ignored() {
    let mut w = watch(10_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), NOW);
    let other = spend(2, &[(&script(0x55), 10_000)]);
    assert_eq!(sight(&mut w, &other, peer(21), NOW), Sighted::Ignored);
    assert_eq!(w.transactions().count(), 1);
    assert_eq!(w.watched_outpoints(), vec![coin(1)]);
}

#[test]
fn wr_40_bytes_that_are_not_a_transaction_are_refused() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    let mut extra = bytes(&tx);
    extra.push(0);
    refused(
        w.sighting(&extra, peer(20), NOW),
        Refusal::NotATransaction,
        "WR-40",
    );
    refused(
        w.sighting(&[], Source::Box, NOW),
        Refusal::NotATransaction,
        "WR-40",
    );
    assert_eq!(w.transactions().count(), 0);
}

#[test]
fn wr_40_a_transaction_that_spends_a_known_coin_is_kept_even_if_it_pays_nothing() {
    let mut w = watch(10_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), NOW);
    let cancel = spend(1, &[(&script(0x55), 10_000)]);
    assert_eq!(sight(&mut w, &cancel, peer(21), NOW), Sighted::Conflicting);
    assert_eq!(w.transactions().count(), 2);
    assert!(status(&mut w, &TestChain::new(1)).conflict_seen);
}

#[test]
fn wr_40_a_proven_payment_that_pays_the_sale_nothing_is_refused() {
    let mut w = watch(10_000, 0);
    let b = block(vec![coinbase(1), spend(1, &[(&script(0x55), 10_000)])]);
    refused(w.confirmed(proven(&b, 1, 5)), Refusal::PaysNothing, "WR-30");
    assert_eq!(w.transactions().count(), 0);
}

// WR-41.

#[test]
fn wr_41_two_peers_in_one_group_count_once() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    for host in ["20.20.1.1", "20.20.2.2", "20.20.3.3", "20.20.255.1"] {
        let source = Source::Peer(PeerAddr::Ip(host.parse().unwrap()));
        sight(&mut w, &tx, source, NOW);
    }
    let s = status(&mut w, &TestChain::new(1));
    assert_eq!((s.peer_groups, s.mode), (1, None));
}

#[test]
fn wr_41_all_named_peers_count_once() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    for _ in 0..10 {
        sight(&mut w, &tx, Source::Peer(PeerAddr::Named), NOW);
    }
    assert_eq!(status(&mut w, &TestChain::new(1)).peer_groups, 1);
}

#[test]
fn wr_41_local_peers_count_once() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    for host in ["10.0.0.1", "192.168.1.1", "127.0.0.1"] {
        let source = Source::Peer(PeerAddr::Ip(host.parse().unwrap()));
        sight(&mut w, &tx, source, NOW);
    }
    assert_eq!(status(&mut w, &TestChain::new(1)).peer_groups, 1);
}

#[test]
fn wr_41_the_box_is_never_counted_as_a_peer() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, Source::Box, NOW);
    sight(&mut w, &tx, peer(20), NOW);
    sight(&mut w, &tx, peer(21), NOW);
    let s = status(&mut w, &TestChain::new(1));
    // Two groups and the Box: not three groups.
    assert_eq!(s.peer_groups, 2);
    assert_eq!(s.mode, Some(CheckMode::BoxReportsOnly));
}

// WR-42.

#[test]
fn wr_42_a_fee_bump_takes_the_place_of_the_earlier_transaction() {
    let mut w = watch(10_000, 0);
    let old = pays(1, 10_000);
    seen_by(&mut w, &old, 3, NOW);
    sight(&mut w, &old, Source::Box, NOW);
    let bump = spend(1, &[(&shop(), 10_000), (&script(0x55), 1)]);
    assert_eq!(sight(&mut w, &bump, peer(30), NOW + 100), Sighted::Replaced);
    assert_eq!(w.transactions().collect::<Vec<_>>(), vec![&bump]);
    let s = status_at(&mut w, &TestChain::new(1), NOW + 200);
    assert!(!s.conflict_seen);
    assert_eq!(s.received_sats, 10_000);
    // The groups and the Box start again from the new transaction.
    assert_eq!(
        (s.peer_groups, s.mode, s.secs_since_first_seen),
        (1, None, 0)
    );
}

#[test]
fn wr_42_a_fee_bump_restarts_first_seen_and_the_group_count() {
    let mut w = watch(10_000, 0);
    let old = pays(1, 10_000);
    seen_by(&mut w, &old, 3, 1_000);
    let chain = TestChain::new(1);
    let s = status_at(&mut w, &chain, 1_500);
    assert_eq!(
        (s.mode, s.peer_groups, s.secs_since_first_seen),
        (Some(CheckMode::Light), 3, 500)
    );
    let bump = pays(1, 12_000);
    sight(&mut w, &bump, peer(20), 1_600);
    sight(&mut w, &bump, peer(21), 1_700);
    let s = status_at(&mut w, &chain, 1_900);
    assert_eq!(
        (s.mode, s.peer_groups, s.secs_since_first_seen),
        (None, 2, 0)
    );
    sight(&mut w, &bump, peer(22), 1_800);
    let s = status_at(&mut w, &chain, 1_900);
    assert_eq!(
        (s.mode, s.peer_groups, s.secs_since_first_seen),
        (Some(CheckMode::Light), 3, 100)
    );
    assert_eq!(s.received_sats, 12_000);
}

#[test]
fn wr_42_a_late_sighting_of_a_replaced_transaction_is_not_a_conflict() {
    // A peer that has not heard of the fee bump still relays the old one.
    let mut w = watch(10_000, 0);
    let chain = TestChain::new(1);
    let old = pays(1, 10_000);
    sight(&mut w, &old, peer(20), NOW);
    let bump = pays(1, 12_000);
    assert_eq!(sight(&mut w, &bump, peer(20), NOW + 10), Sighted::Replaced);
    assert_eq!(sight(&mut w, &old, peer(21), NOW + 20), Sighted::Ignored);
    assert_eq!(sight(&mut w, &old, Source::Box, NOW + 20), Sighted::Ignored);
    assert_eq!(w.transactions().collect::<Vec<_>>(), vec![&bump]);
    let s = status_at(&mut w, &chain, NOW + 30);
    assert!(!s.conflict_seen);
    assert_eq!(s.received_sats, 12_000);
}

#[test]
fn wr_42_a_replaced_transaction_that_confirms_after_all_is_admitted() {
    // A miner can still mine the old one. A block is evidence; it is not
    // ignored, and the bump it conflicts with is displaced (WR-43).
    let mut w = watch(10_000, 0);
    let old = pays(1, 10_000);
    sight(&mut w, &old, peer(20), NOW);
    let bump = pays(1, 12_000);
    sight(&mut w, &bump, peer(20), NOW + 10);
    let mut chain = TestChain::new(5);
    confirm_in(&mut w, &mut chain, &[old], 5);
    let s = status(&mut w, &chain);
    assert_eq!((s.received_sats, s.confirmed_sats), (10_000, 10_000));
    assert_eq!(s.min_confirmations, 1);
    assert!(s.conflict_seen);
}

#[test]
fn wr_42_a_replacement_that_pays_the_same_is_a_fee_bump() {
    let mut w = watch(10_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), NOW);
    let same = spend(1, &[(&shop(), 10_000), (&script(0x55), 3)]);
    assert_eq!(sight(&mut w, &same, peer(20), NOW), Sighted::Replaced);
}

#[test]
fn wr_42_a_cancel_sets_the_conflict_flag_for_good_and_both_are_kept() {
    let mut w = watch(10_000, 0);
    let chain = TestChain::new(1);
    let original = pays(1, 10_000);
    seen_by(&mut w, &original, 3, NOW);
    assert!(!status_at(&mut w, &chain, NOW).conflict_seen);
    // The replacement pays the shop less.
    let cancel = pays(1, 3_000);
    assert_eq!(sight(&mut w, &cancel, peer(30), NOW), Sighted::Conflicting);
    assert_eq!(w.transactions().count(), 2);
    for later in [NOW, NOW + 10_000, NOW + 1_000_000] {
        assert!(status_at(&mut w, &chain, later).conflict_seen);
    }
    // The cancel is never seen again; the flag stays.
    sight(&mut w, &original, peer(31), NOW + 5);
    assert!(status_at(&mut w, &chain, NOW + 99).conflict_seen);
}

#[test]
fn wr_42_a_replacement_that_pays_the_shop_nothing_is_a_cancel() {
    let mut w = watch(10_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), NOW);
    let cancel = spend(1, &[(&script(0x55), 10_000)]);
    assert_eq!(sight(&mut w, &cancel, peer(20), NOW), Sighted::Conflicting);
}

#[test]
fn wr_42_a_transaction_that_replaces_two_is_a_fee_bump_only_for_both() {
    let mut w = watch(20_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), NOW);
    sight(&mut w, &pays(2, 10_000), peer(20), NOW);
    let merged_low = spend_coins(&[coin(1), coin(2)], &[(&shop(), 9_999)]);
    assert_eq!(
        sight(&mut w, &merged_low, peer(21), NOW),
        Sighted::Conflicting
    );
    assert_eq!(w.transactions().count(), 3);

    let mut w = watch(20_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), NOW);
    sight(&mut w, &pays(2, 10_000), peer(20), NOW);
    let merged = spend_coins(&[coin(1), coin(2)], &[(&shop(), 20_000)]);
    assert_eq!(sight(&mut w, &merged, peer(21), NOW), Sighted::Replaced);
    assert_eq!(w.transactions().count(), 1);
    assert!(!status_at(&mut w, &TestChain::new(1), NOW).conflict_seen);
}

#[test]
fn wr_42_a_transaction_that_was_confirmed_is_not_replaced_by_a_fee_bump() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    confirm_in(&mut w, &mut chain, &[tx], 100);
    let bump = spend(1, &[(&shop(), 10_000), (&script(0x55), 1)]);
    assert_eq!(sight(&mut w, &bump, peer(20), NOW), Sighted::Conflicting);
    assert_eq!(w.transactions().count(), 2);
    let s = status(&mut w, &chain);
    assert!(s.conflict_seen);
    // The confirmed one stands; the other is displaced.
    assert_eq!((s.received_sats, s.min_confirmations), (10_000, 1));
}

#[test]
fn wr_42_the_conflict_flag_survives_the_conflicting_transaction_confirming_and_a_reorg() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    let original = pays(1, 10_000);
    let cancel = pays(1, 2_000);
    seen_by(&mut w, &original, 3, NOW);
    sight(&mut w, &cancel, peer(30), NOW);
    assert!(status(&mut w, &chain).conflict_seen);
    // The cancel confirms.
    let b = confirm_in(&mut w, &mut chain, &[cancel], 100);
    assert!(status(&mut w, &chain).conflict_seen);
    // The block leaves the chain.
    chain.remove(&b.block_hash());
    assert!(status(&mut w, &chain).conflict_seen);
    chain.tip = 500;
    assert!(status(&mut w, &chain).conflict_seen);
}

#[test]
fn wr_42_a_block_that_spends_the_payments_coin_sets_the_conflict_flag() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    seen_by(&mut w, &pays(1, 10_000), 3, NOW);
    // A block holds a transaction that spends the same coin and pays the
    // shop nothing.
    let other = spend(1, &[(&script(0x55), 10_000)]);
    let b = block(vec![coinbase(9), other.clone()]);
    chain.add(b.block_hash(), 100);
    let spends = spends_in_block(&chain, &bytes_of(&b), &w.watched_outpoints()).unwrap();
    assert_eq!(
        spends,
        vec![(coin(1), other.compute_txid(), b.block_hash())]
    );
    assert!(!status(&mut w, &chain).conflict_seen);
    for (outpoint, by, block) in spends {
        w.spent_elsewhere(outpoint, by, block);
    }
    let s = status(&mut w, &chain);
    assert!(s.conflict_seen);
    // The payment can no longer be mined while that block stands.
    assert_eq!(s.received_sats, 0);
    // The block leaves the chain: the flag stays, the payment stands again.
    chain.remove(&b.block_hash());
    let s = status(&mut w, &chain);
    assert!(s.conflict_seen);
    assert_eq!(s.received_sats, 10_000);
    assert_eq!(w.spent_elsewhere_in().len(), 1);
}

fn bytes_of(b: &Block) -> Vec<u8> {
    bitcoin::consensus::encode::serialize(b)
}

#[test]
fn wr_42_a_spend_by_the_payment_itself_or_of_a_coin_not_watched_is_no_conflict() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, peer(20), NOW);
    let b = block(vec![coinbase(9), tx.clone()]);
    w.spent_elsewhere(coin(1), tx.compute_txid(), b.block_hash());
    w.spent_elsewhere(coin(77), coin(78).txid, b.block_hash());
    assert!(!status(&mut w, &TestChain::new(1)).conflict_seen);
    assert!(w.spent_elsewhere_in().is_empty());
}

// WR-43.

#[test]
fn wr_43_a_confirmed_transaction_displaces_the_one_it_conflicts_with() {
    let mut chain = TestChain::new(100);
    let mut w = watch(40_000, 0);
    let original = pays(1, 40_000);
    let other = pays(1, 10_000);
    seen_by(&mut w, &original, 3, NOW);
    sight(&mut w, &other, peer(30), NOW);
    // Both stand while neither is confirmed, and count once (WR-31).
    assert_eq!(status(&mut w, &chain).received_sats, 10_000);
    let b = confirm_in(&mut w, &mut chain, &[other], 100);
    let s = status(&mut w, &chain);
    assert_eq!(s.received_sats, 10_000);
    assert_eq!(s.confirmed_sats, 10_000);
    assert!(s.conflict_seen);
    // The block leaves the chain: the original stands again.
    chain.remove(&b.block_hash());
    let s = status(&mut w, &chain);
    assert_eq!(s.received_sats, 10_000);
    assert!(s.conflict_seen);
}

#[test]
fn wr_43_the_displaced_transaction_does_not_count_toward_confirmation() {
    let mut chain = TestChain::new(100);
    let mut w = watch(40_000, 0);
    let original = pays(1, 40_000);
    let other = pays(1, 10_000);
    confirm_in(&mut w, &mut chain, std::slice::from_ref(&original), 98);
    sight(&mut w, &other, peer(30), NOW);
    let s = status(&mut w, &chain);
    assert_eq!(s.received_sats, 40_000);
    assert_eq!(s.min_confirmations, 3);
    // The displaced one is not the weakest unconfirmed payment either.
    assert_eq!((s.mode, s.peer_groups), (None, 0));
}

#[test]
fn wr_43_transactions_that_do_not_conflict_are_not_displaced() {
    let mut chain = TestChain::new(100);
    let mut w = watch(20_000, 0);
    confirm_in(&mut w, &mut chain, &[pays(1, 10_000)], 100);
    sight(&mut w, &pays(2, 10_000), peer(20), NOW);
    let s = status(&mut w, &chain);
    assert_eq!((s.received_sats, s.confirmed_sats), (20_000, 10_000));
    assert!(!s.conflict_seen);
}

// WR-44, WR-45.

#[test]
fn wr_44_three_groups_make_it_seen_by_the_light_client() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    let chain = TestChain::new(1);
    for (n, mode) in [
        (1u8, None),
        (2, None),
        (3, Some(CheckMode::Light)),
        (4, Some(CheckMode::Light)),
    ] {
        sight(&mut w, &tx, peer(19 + n), NOW);
        let s = status(&mut w, &chain);
        assert_eq!((s.peer_groups, s.mode), (n as usize, mode));
    }
}

#[test]
fn wr_45_the_four_cases_of_the_mode_table() {
    let chain = TestChain::new(1);
    // Reported by the Box and seen by the light client.
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    seen_by(&mut w, &tx, 3, NOW);
    sight(&mut w, &tx, Source::Box, NOW);
    assert_eq!(
        status(&mut w, &chain).mode,
        Some(CheckMode::NodeAlsoSeenByLight)
    );
    // Seen by the light client, not reported by the Box.
    let mut w = watch(10_000, 0);
    seen_by(&mut w, &tx, 3, NOW);
    assert_eq!(status(&mut w, &chain).mode, Some(CheckMode::Light));
    // Reported by the Box, not seen by the light client.
    let mut w = watch(10_000, 0);
    sight(&mut w, &tx, Source::Box, NOW);
    seen_by(&mut w, &tx, 2, NOW);
    assert_eq!(status(&mut w, &chain).mode, Some(CheckMode::BoxReportsOnly));
    // Anything less: no mode.
    let mut w = watch(10_000, 0);
    seen_by(&mut w, &tx, 2, NOW);
    assert_eq!(status(&mut w, &chain).mode, None);
    let mut w = watch(10_000, 0);
    assert_eq!(status(&mut w, &chain).mode, None);
}

#[test]
fn wr_45_the_mode_follows_the_evidence_whatever_the_source_says() {
    // A peer that claims to be the Box, or a Box that claims to be many
    // peers, adds nothing: only the kind of the sighting counts.
    let chain = TestChain::new(1);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    for _ in 0..10 {
        sight(&mut w, &tx, Source::Box, NOW);
    }
    let s = status(&mut w, &chain);
    assert_eq!(
        (s.mode, s.peer_groups),
        (Some(CheckMode::BoxReportsOnly), 0)
    );
}

#[test]
fn wr_45_a_confirmed_payment_has_no_mode() {
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    seen_by(&mut w, &tx, 3, NOW);
    confirm_in(&mut w, &mut chain, &[tx], 100);
    let s = status(&mut w, &chain);
    assert_eq!(
        (s.mode, s.peer_groups, s.secs_since_first_seen),
        (None, 0, 0)
    );
}

#[test]
fn wr_45_a_sale_is_only_as_checked_as_its_least_checked_part() {
    let chain = TestChain::new(1);
    let mut w = watch(10_000, 0);
    let strong = pays(1, 6_000);
    let weak = pays(2, 4_000);
    seen_by(&mut w, &strong, 4, 1_000);
    sight(&mut w, &weak, peer(20), 2_000);
    sight(&mut w, &weak, Source::Box, 2_000);
    let s = status_at(&mut w, &chain, 3_000);
    assert_eq!(s.received_sats, 10_000);
    assert_eq!(s.peer_groups, 1);
    assert_eq!(s.mode, Some(CheckMode::BoxReportsOnly));
    assert_eq!(s.secs_since_first_seen, 1_000);
    // Among parts with as many groups, the latest first-seen counts.
    let mut w = watch(10_000, 0);
    seen_by(&mut w, &pays(1, 5_000), 3, 1_000);
    seen_by(&mut w, &pays(2, 5_000), 3, 2_500);
    assert_eq!(status_at(&mut w, &chain, 3_000).secs_since_first_seen, 500);
    // A confirmed part is not the weakest one.
    let mut chain = TestChain::new(100);
    let mut w = watch(10_000, 0);
    confirm_in(&mut w, &mut chain, &[pays(1, 5_000)], 100);
    seen_by(&mut w, &pays(2, 5_000), 3, 1_000);
    let s = status_at(&mut w, &chain, 1_100);
    assert_eq!(
        (s.mode, s.peer_groups, s.secs_since_first_seen),
        (Some(CheckMode::Light), 3, 100)
    );
}

// WR-46.

#[test]
fn wr_46_the_time_counts_from_the_sighting_that_made_it_seen() {
    let chain = TestChain::new(1);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, peer(20), 1_000);
    sight(&mut w, &tx, peer(21), 1_100);
    assert_eq!(status_at(&mut w, &chain, 1_200).secs_since_first_seen, 0);
    sight(&mut w, &tx, peer(22), 1_300);
    sight(&mut w, &tx, peer(23), 1_400);
    assert_eq!(status_at(&mut w, &chain, 1_500).secs_since_first_seen, 200);
}

#[test]
fn wr_46_the_time_counts_from_the_box_report_when_that_came_first() {
    let chain = TestChain::new(1);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, Source::Box, 1_000);
    seen_by(&mut w, &tx, 3, 1_400);
    assert_eq!(status_at(&mut w, &chain, 1_500).secs_since_first_seen, 500);
    // And from the third group when that came first.
    let mut w = watch(10_000, 0);
    seen_by(&mut w, &tx, 3, 1_000);
    sight(&mut w, &tx, Source::Box, 1_400);
    assert_eq!(status_at(&mut w, &chain, 1_500).secs_since_first_seen, 500);
}

#[test]
fn wr_46_a_clock_that_goes_back_gives_zero_seconds() {
    let chain = TestChain::new(1);
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, Source::Box, 1_000);
    assert_eq!(status_at(&mut w, &chain, 1_000).secs_since_first_seen, 0);
    assert_eq!(status_at(&mut w, &chain, 999).secs_since_first_seen, 0);
    assert_eq!(status_at(&mut w, &chain, 0).secs_since_first_seen, 0);
    assert_eq!(status_at(&mut w, &chain, 1_001).secs_since_first_seen, 1);
}

#[test]
fn wr_46_a_sighting_with_an_earlier_clock_does_not_move_the_time_back() {
    let chain = TestChain::new(1);
    let mut w = watch(10_000, 0);
    let other = pays(2, 1);
    sight(&mut w, &other, peer(20), 5_000);
    // A later sighting, with a clock that went back, is recorded at the
    // latest time already seen.
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, Source::Box, 100);
    assert_eq!(status_at(&mut w, &chain, 5_100).secs_since_first_seen, 100);
}

#[test]
fn wr_46_a_payment_neither_seen_nor_reported_by_the_box_has_no_time() {
    let chain = TestChain::new(1);
    let mut w = watch(10_000, 0);
    sight(&mut w, &pays(1, 10_000), peer(20), 1_000);
    assert_eq!(status_at(&mut w, &chain, 9_000).secs_since_first_seen, 0);
}

// WR-31: what stands.

#[test]
fn wr_31_standing_lists_what_each_payment_pays_ascending_by_id() {
    let mut w = watch(30_000, 0);
    assert!(w.standing().is_empty());
    let (a, b, c) = (pays(1, 5_000), pays(2, 7_000), pays(3, 9_000));
    for tx in [&a, &b, &c] {
        sight(&mut w, tx, peer(20), NOW);
    }
    let s = status_at(&mut w, &TestChain::new(1), NOW);
    assert_eq!(s.received_sats, 21_000);
    let mut expect = vec![
        (a.compute_txid(), 5_000),
        (b.compute_txid(), 7_000),
        (c.compute_txid(), 9_000),
    ];
    expect.sort_by_key(|(t, _)| bitcoin::hashes::Hash::to_byte_array(*t));
    assert_eq!(w.standing(), expect);
}

#[test]
fn wr_31_standing_names_the_smaller_of_two_conflicting_payments() {
    let mut w = watch(13_000, 0);
    let big = pays(1, 10_000);
    let small = pays(1, 3_000);
    sight(&mut w, &big, peer(20), NOW);
    sight(&mut w, &small, peer(21), NOW);
    status_at(&mut w, &TestChain::new(1), NOW);
    assert_eq!(w.standing(), vec![(small.compute_txid(), 3_000)]);
}

#[test]
fn wr_31_standing_leaves_out_a_payment_a_confirmed_conflict_displaced() {
    let mut w = watch(10_000, 0);
    let good = pays(1, 10_000);
    let other = pays(1, 3_000);
    sight(&mut w, &good, peer(20), NOW);
    sight(&mut w, &other, peer(21), NOW);
    let mut chain = TestChain::new(5);
    confirm_in(&mut w, &mut chain, std::slice::from_ref(&good), 5);
    status_at(&mut w, &chain, NOW);
    assert_eq!(w.standing(), vec![(good.compute_txid(), 10_000)]);
}

#[test]
fn wr_31_standing_leaves_out_a_replaced_payment() {
    let mut w = watch(10_000, 0);
    let old = spend(1, &[(&shop(), 10_000)]);
    let bump = spend(1, &[(&shop(), 10_000), (&script(0x55), 5)]);
    sight(&mut w, &old, peer(20), NOW);
    sight(&mut w, &bump, peer(21), NOW);
    status_at(&mut w, &TestChain::new(1), NOW);
    assert_eq!(w.standing(), vec![(bump.compute_txid(), 10_000)]);
}

#[test]
fn wr_31_a_conflict_that_pays_nothing_leaves_nothing_standing() {
    // The pair counts once at the smaller amount, which is nothing.
    let mut w = watch(10_000, 0);
    let pay = pays(1, 10_000);
    sight(&mut w, &pay, peer(20), NOW);
    let nothing = spend_coins(&[coin(1)], &[(&script(0x55), 1)]);
    assert_eq!(sight(&mut w, &nothing, peer(22), NOW), Sighted::Conflicting);
    let s = status_at(&mut w, &TestChain::new(1), NOW);
    assert_eq!(s.received_sats, 0);
    assert!(w.standing().is_empty());
}

#[test]
fn wr_31_the_first_seen_time_is_the_earliest_of_the_standing_payments() {
    let mut w = watch(20_000, 0);
    let late = pays(1, 10_000);
    let early = pays(2, 10_000);
    sight(&mut w, &late, Source::Box, 2_000);
    sight(&mut w, &early, Source::Box, 1_000);
    // The clock cannot go back (WR-46): the second sighting is recorded at
    // the latest time already seen.
    status_at(&mut w, &TestChain::new(1), 3_000);
    assert_eq!(w.standing_first_seen(), Some(2_000));
    let mut v = watch(10_000, 0);
    assert_eq!(v.standing_first_seen(), None);
    sight(&mut v, &pays(1, 10_000), peer(20), 1_000);
    status_at(&mut v, &TestChain::new(1), 3_000);
    // Reported by one peer only: it never counted as seen.
    assert_eq!(v.standing_first_seen(), None);
}

#[test]
fn wr_31_a_payment_only_ever_seen_in_a_block_has_the_time_the_block_was_learned() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    let mut chain = TestChain::new(5);
    let b = block_at(vec![coinbase(5), tx.clone()], 5);
    chain.add(b.block_hash(), 5);
    w.confirmed_at(proven(&b, 1, 5), 777).unwrap();
    // A later report of the same block does not move the time.
    w.confirmed_at(proven(&b, 1, 5), 900).unwrap();
    status_at(&mut w, &chain, NOW);
    assert_eq!(w.standing(), vec![(tx.compute_txid(), 10_000)]);
    assert_eq!(w.standing_first_seen(), Some(777));
    // Without a time given there is none.
    let mut v = watch(10_000, 0);
    v.confirmed(proven(&b, 1, 5)).unwrap();
    status_at(&mut v, &chain, NOW);
    assert_eq!(v.standing_first_seen(), None);
}

#[test]
fn wr_31_a_seen_payment_keeps_its_first_seen_time_when_a_block_holds_it() {
    let mut w = watch(10_000, 0);
    let tx = pays(1, 10_000);
    sight(&mut w, &tx, Source::Box, 1_000);
    let mut chain = TestChain::new(5);
    let b = block_at(vec![coinbase(5), tx], 5);
    chain.add(b.block_hash(), 5);
    w.confirmed_at(proven(&b, 1, 5), 4_000).unwrap();
    status_at(&mut w, &chain, 5_000);
    assert_eq!(w.standing_first_seen(), Some(1_000));
}
