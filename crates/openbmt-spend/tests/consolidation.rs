//! SP-38, SP-39: the size and the cost of a consolidation.

mod common;

use common::*;
use openbmt_spend::{
    consolidation_cost_sats, consolidation_size, plan, Plan, MAX_CONSOLIDATION_INPUTS,
};

/// A consolidation (SP-24) of `n` coins to the change script at 0.
fn consolidation_plan(n: u32, rate: u64) -> Plan {
    let coins: Vec<_> = (0..n).map(|i| coin(i + 1, i, 100_000)).collect();
    let mut r = request(vec![(wallet().change_script(0).unwrap(), 0)], rate);
    r.send_all = true;
    plan(wallet(), &coins, &r).unwrap()
}

#[test]
fn sp_38_a_consolidation_has_the_size_and_fee_the_planner_gives() {
    // One input and the 34-byte output: 4 + 1 + 41 + 1 + (8 + 1 + 34) + 4 =
    // 94 base bytes, 376 + 256 = 632 units, 158 vB.
    assert_eq!(consolidation_size(1), Some((632, 158)));
    for n in [1u32, 2, 3, 10, 12, 50] {
        for rate in [1_000u64, 1_234, 2_500, 25_000] {
            let p = consolidation_plan(n, rate);
            let (weight, vsize) = consolidation_size(n as usize).unwrap();
            assert_eq!((p.weight(), p.vsize()), (weight, vsize), "{n} coins");
            assert_eq!(
                consolidation_cost_sats(n as usize, rate),
                Some(p.fee_sats()),
                "{n} coins at {rate}"
            );
            assert!(p.change().is_none());
        }
    }
    assert_eq!(consolidation_size(0), None);
    assert_eq!(consolidation_cost_sats(0, 1_000), None);
}

#[test]
fn sp_39_more_coins_than_one_consolidation_takes_are_counted_in_rounds() {
    let max = MAX_CONSOLIDATION_INPUTS;
    // The largest consolidation is inside the signer's weight limit.
    let (weight, _) = consolidation_size(max).unwrap();
    assert!(weight <= openbmt_signer::MAX_WEIGHT);
    let full = consolidation_cost_sats(max, 1_000).unwrap();
    let rest = consolidation_cost_sats(7, 1_000).unwrap();
    assert_eq!(consolidation_cost_sats(max + 7, 1_000), Some(full + rest));
    assert_eq!(consolidation_cost_sats(2 * max, 1_000), Some(2 * full));
    // A rate that overflows the sum gives no answer.
    assert_eq!(consolidation_cost_sats(max, u64::MAX), None);
}
