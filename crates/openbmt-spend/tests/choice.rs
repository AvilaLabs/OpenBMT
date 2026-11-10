//! SP-10 to SP-12: which coins are spent.

mod common;

use bitcoin::OutPoint;
use common::*;
use openbmt_spend::{plan, PlanError};

fn spent(p: &openbmt_spend::Plan) -> Vec<OutPoint> {
    p.inputs().iter().map(|i| i.coin.outpoint).collect()
}

#[test]
fn sp_10_unconfirmed_coins_are_not_spent_unless_asked_for_by_name() {
    let confirmed = coin_conf(1, 0, 200_000, 3);
    let fresh = coin_conf(2, 1, 900_000, 0);
    let coins = [confirmed.clone(), fresh.clone()];
    // The big coin has no confirmation: with min 1 it is not touched, and
    // a payment only it could pay is a lack of funds.
    let r = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 500_000)], 1000),
    );
    assert!(
        matches!(r, Err(PlanError::InsufficientFunds { .. })),
        "{r:?}"
    );
    let p = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 50_000)], 1000),
    )
    .unwrap();
    assert_eq!(spent(&p), vec![confirmed.outpoint]);
    // With 0 it is spent.
    let mut asked = request(vec![(external(1), 500_000)], 1000);
    asked.min_confirmations = 0;
    let p = plan(wallet(), &coins, &asked).unwrap();
    assert!(spent(&p).contains(&fresh.outpoint));
    // Only unconfirmed coins, min 1: nothing is eligible.
    let r = plan(
        wallet(),
        &[fresh],
        &request(vec![(external(1), 50_000)], 1000),
    );
    assert_eq!(r, Err(PlanError::NoEligibleCoins));
    // A higher minimum leaves out the shallow coin.
    let mut deep = request(vec![(external(1), 50_000)], 1000);
    deep.min_confirmations = 4;
    assert_eq!(
        plan(wallet(), &[confirmed], &deep),
        Err(PlanError::NoEligibleCoins)
    );
}

#[test]
fn sp_11_coins_are_taken_oldest_first_and_only_as_many_as_needed() {
    let a = coin_conf(1, 0, 40_000, 2);
    let b = coin_conf(2, 1, 40_000, 9);
    let c = coin_conf(3, 2, 40_000, 5);
    let d = coin_conf(4, 3, 40_000, 1);
    let coins = [a.clone(), b.clone(), c.clone(), d.clone()];
    let p = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 70_000)], 1000),
    )
    .unwrap();
    // 9 deep, then 5 deep; two coins pay 70,000 and the fee.
    assert_eq!(spent(&p), vec![b.outpoint, c.outpoint]);
    // The transaction's inputs are in that order.
    let tx = p.unsigned_tx();
    assert_eq!(tx.input[0].previous_output, b.outpoint);
    assert_eq!(tx.input[1].previous_output, c.outpoint);
    // The order the coins were handed in does not matter.
    let reversed = [d, c, b, a];
    let q = plan(
        wallet(),
        &reversed,
        &request(vec![(external(1), 70_000)], 1000),
    )
    .unwrap();
    assert_eq!(p, q);
}

#[test]
fn sp_11_equal_depth_is_broken_by_the_outpoint() {
    use bitcoin::hashes::Hash;
    let coins: Vec<_> = (1..=6).map(|t| coin_conf(t, 0, 30_000, 4)).collect();
    let mut sorted: Vec<_> = coins.iter().map(|c| c.outpoint).collect();
    sorted.sort_by_key(|o| (o.txid.to_byte_array(), o.vout));
    let p = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 100_000)], 1000),
    )
    .unwrap();
    let got = spent(&p);
    assert!(got.len() >= 4);
    assert_eq!(got, sorted[..got.len()].to_vec());
    // Shuffled input gives the same plan.
    let mut shuffled = coins.clone();
    shuffled.reverse();
    shuffled.swap(1, 4);
    assert_eq!(
        plan(
            wallet(),
            &shuffled,
            &request(vec![(external(1), 100_000)], 1000)
        )
        .unwrap(),
        p
    );
}

#[test]
fn sp_12_send_all_spends_every_eligible_coin_to_one_payment_with_no_change() {
    let coins = [
        coin_conf(1, 0, 100_000, 3),
        coin_conf(2, 1, 250_000, 8),
        coin_conf(3, 2, 70_000, 1),
        coin_conf(4, 3, 999_999, 0),
    ];
    let mut r = request(vec![(external(7), 0)], 2000);
    r.send_all = true;
    let p = plan(wallet(), &coins, &r).unwrap();
    assert_eq!(p.inputs().len(), 3, "the unconfirmed coin is left");
    assert!(p.change().is_none());
    assert_eq!(p.dropped_change_sats(), None);
    assert_eq!(p.payments().len(), 1);
    assert_eq!(p.unsigned_tx().output.len(), 1);
    let total: u64 = 100_000 + 250_000 + 70_000;
    assert_eq!(p.total_in_sats(), total);
    assert_eq!(p.payments()[0].sats, total - p.fee_sats());
    assert_eq!(p.fee_sats(), (p.vsize() * 2000).div_ceil(1000));
    // With min 0 the unconfirmed coin goes too.
    r.min_confirmations = 0;
    assert_eq!(plan(wallet(), &coins, &r).unwrap().inputs().len(), 4);
}

#[test]
fn sp_12_send_all_takes_one_payment_with_no_amount() {
    let coins = [coin(1, 0, 500_000)];
    let mut r = request(vec![(external(7), 1000)], 1000);
    r.send_all = true;
    assert_eq!(
        plan(wallet(), &coins, &r),
        Err(PlanError::SendAllAmountGiven)
    );
    let mut two = request(vec![(external(7), 0), (external(8), 0)], 1000);
    two.send_all = true;
    assert_eq!(
        plan(wallet(), &coins, &two),
        Err(PlanError::SendAllNeedsOnePayment)
    );
    // One input and one P2WPKH output weigh 584 units (146 vB). At 1 sat/vB
    // a coin of 400 sat leaves 254, below the 294 limit of P2WPKH.
    let mut low = request(vec![(external(7), 0)], 1000);
    low.send_all = true;
    let r = plan(wallet(), &[coin(4, 0, 400)], &low);
    assert_eq!(
        r,
        Err(PlanError::SendAllBelowDust {
            sats: 254,
            dust: 294
        })
    );
    // A coin that does not even pay the fee.
    let r = plan(wallet(), &[coin(5, 0, 100)], &low);
    assert!(
        matches!(r, Err(PlanError::InsufficientFunds { .. })),
        "{r:?}"
    );
}
