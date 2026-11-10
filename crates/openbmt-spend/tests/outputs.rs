//! SP-20 to SP-25: outputs.

mod common;

use bitcoin::opcodes::all::OP_RETURN;
use bitcoin::script::Builder;
use bitcoin::{PubkeyHash, ScriptBuf, ScriptHash, WScriptHash};
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_spend::{dust_limit, plan, PlanError};

fn p2pkh() -> ScriptBuf {
    ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([3; 20]))
}
fn p2sh() -> ScriptBuf {
    ScriptBuf::new_p2sh(&ScriptHash::from_byte_array([4; 20]))
}
fn p2wsh() -> ScriptBuf {
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([5; 32]))
}
fn p2tr() -> ScriptBuf {
    let key = kit().xpubs[0].public_key.x_only_public_key().0;
    ScriptBuf::new_p2tr(secp(), key, None)
}

use bitcoin::hashes::Hash;

#[test]
fn sp_20_payments_come_in_the_order_given_and_change_comes_last() {
    let coins = [coin(1, 0, 1_000_000)];
    let pays = vec![
        (external(9), 30_000),
        (external(2), 20_000),
        (p2wsh(), 10_000),
    ];
    let p = plan(wallet(), &coins, &request(pays.clone(), 1000)).unwrap();
    let outs = &p.unsigned_tx().output;
    assert_eq!(outs.len(), 4);
    for (out, (script, sats)) in outs.iter().zip(&pays) {
        assert_eq!(out.script_pubkey, *script);
        assert_eq!(out.value.to_sat(), *sats);
    }
    assert_eq!(
        outs[3].script_pubkey,
        wallet().change_script(0).unwrap(),
        "change is last"
    );
    assert_eq!(p.payments().len(), 3);
}

#[test]
fn sp_21_change_goes_to_the_change_script_at_the_given_index() {
    let coins = [coin(1, 0, 1_000_000)];
    let mut r = request(vec![(external(9), 300_000)], 1000);
    r.change_index = 7;
    let p = plan(wallet(), &coins, &r).unwrap();
    let change = p.change().unwrap();
    assert_eq!(change.index, 7);
    assert_eq!(change.script, wallet().change_script(7).unwrap());
    assert_eq!(change.sats, 1_000_000 - 300_000 - p.fee_sats());
    let last = p.unsigned_tx().output.last().unwrap();
    assert_eq!(last.script_pubkey, change.script);
    assert_eq!(last.value.to_sat(), change.sats);
}

#[test]
fn sp_22_the_five_script_kinds_are_paid_and_others_are_refused() {
    let coins = [coin(1, 0, 5_000_000)];
    for (n, script) in [external(1), p2wsh(), p2tr(), p2pkh(), p2sh()]
        .into_iter()
        .enumerate()
    {
        let dust = dust_limit(&script).unwrap();
        let p = plan(wallet(), &coins, &request(vec![(script, dust)], 1000));
        assert!(p.is_ok(), "kind {n}: {p:?}");
    }
    let data = Builder::new()
        .push_opcode(OP_RETURN)
        .push_slice([1u8; 8])
        .into_script();
    let bare_key = Builder::new()
        .push_slice(kit().xpubs[0].public_key.serialize())
        .push_opcode(bitcoin::opcodes::all::OP_CHECKSIG)
        .into_script();
    let v2 = Builder::new()
        .push_int(2)
        .push_slice([7u8; 32])
        .into_script();
    for (n, script) in [data, bare_key, v2, ScriptBuf::new()]
        .into_iter()
        .enumerate()
    {
        let r = plan(wallet(), &coins, &request(vec![(script, 100_000)], 1000));
        assert_eq!(r, Err(PlanError::PaymentScriptKind { payment: 0 }), "{n}");
        assert!(r.unwrap_err().to_string().starts_with("SP-22"));
    }
}

#[test]
fn sp_22_payments_below_the_dust_limit_of_their_script_are_refused() {
    let coins = [coin(1, 0, 5_000_000)];
    let cases = [
        (external(1), 294),
        (p2wsh(), 330),
        (p2tr(), 330),
        (p2pkh(), 546),
        (p2sh(), 540),
    ];
    for (script, dust) in cases {
        assert_eq!(dust_limit(&script), Some(dust));
        // The limit is what Core's policy takes at 3 sat/vB.
        assert_eq!(
            script.minimal_non_dust().to_sat(),
            dust,
            "the table is not Core's"
        );
        let r = plan(
            wallet(),
            &coins,
            &request(vec![(script.clone(), dust - 1)], 1000),
        );
        assert_eq!(
            r,
            Err(PlanError::PaymentBelowDust {
                payment: 0,
                sats: dust - 1,
                dust
            })
        );
        assert!(plan(wallet(), &coins, &request(vec![(script, dust)], 1000)).is_ok());
    }
    let r = plan(wallet(), &coins, &request(vec![(external(1), 0)], 1000));
    assert!(matches!(r, Err(PlanError::PaymentBelowDust { .. })));
}

#[test]
fn sp_22_amounts_above_all_bitcoin_are_refused() {
    let coins = [coin(1, 0, 5_000_000)];
    let r = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 21_000_000 * 100_000_000 + 1)], 1000),
    );
    assert_eq!(r, Err(PlanError::AmountTooLarge));
    let r = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), u64::MAX), (external(2), u64::MAX)], 1000),
    );
    assert_eq!(r, Err(PlanError::Overflow));
}

#[test]
fn sp_23_change_below_dust_goes_into_the_fee_and_the_plan_says_so() {
    // One input, one P2WPKH payment, change: 756 WU, 189 vB. At 1 sat/vB
    // the fee with change is 189. The change is the coin minus 50,000 minus
    // 189. A coin of 50,189 + 329 leaves change 329, one under the limit.
    let pay = 50_000;
    let r = request(vec![(external(1), pay)], 1000);
    let at_dust = plan(wallet(), &[coin(1, 0, pay + 189 + 330)], &r).unwrap();
    assert_eq!(at_dust.change().unwrap().sats, 330, "dust itself is kept");
    assert_eq!(at_dust.dropped_change_sats(), None);

    let below = plan(wallet(), &[coin(2, 0, pay + 189 + 329)], &r).unwrap();
    assert!(below.change().is_none());
    assert_eq!(below.dropped_change_sats(), Some(329));
    assert_eq!(below.unsigned_tx().output.len(), 1);
    // The fee is everything that is not paid, and is above the asked rate.
    assert_eq!(below.fee_sats(), 189 + 329);
    assert_eq!(
        below.total_in_sats(),
        below.total_out_sats() + below.fee_sats()
    );
    assert!(below.fee_rate_per_kvb() > 1000);
    // Between the fee without change (146) and with it (189): no change
    // output, no dropped change, the rest is fee.
    let between = plan(wallet(), &[coin(3, 0, pay + 170)], &r).unwrap();
    assert!(between.change().is_none());
    assert_eq!(between.dropped_change_sats(), None);
    assert_eq!(between.fee_sats(), 170);
}

#[test]
fn sp_24_payments_to_the_wallets_own_scripts_are_refused() {
    let c = coin(1, 4, 1_000_000);
    let own_receive = wallet().receive_script(4).unwrap();
    let r = plan(
        wallet(),
        std::slice::from_ref(&c),
        &request(vec![(own_receive, 100_000)], 1000),
    );
    assert_eq!(r, Err(PlanError::PaymentToOwnScript { payment: 0 }));
    assert!(r.unwrap_err().to_string().starts_with("SP-24"));
    // The change script at the change index, as an ordinary payment.
    let own_change = wallet().change_script(0).unwrap();
    let r = plan(
        wallet(),
        std::slice::from_ref(&c),
        &request(vec![(own_change, 100_000)], 1000),
    );
    assert_eq!(r, Err(PlanError::PaymentToOwnScript { payment: 0 }));
    // A change coin's own script.
    let cc = coin_tagged(2, Keychain::Change, 2, 1_000_000, 3);
    let own = wallet().change_script(2).unwrap();
    assert!(plan(wallet(), &[cc], &request(vec![(own, 100_000)], 1000)).is_err());
}

#[test]
fn sp_24_send_all_to_the_change_script_is_a_consolidation() {
    let coins = [
        coin(1, 0, 400_000),
        coin(2, 1, 300_000),
        coin(3, 2, 200_000),
    ];
    let mut r = request(vec![(wallet().change_script(5).unwrap(), 0)], 1500);
    r.change_index = 5;
    r.send_all = true;
    let p = plan(wallet(), &coins, &r).unwrap();
    assert!(p.payments()[0].to_own_change);
    assert_eq!(p.inputs().len(), 3);
    assert!(p.change().is_none());
    // Any other own script is still refused, in send_all too.
    let mut bad = request(vec![(wallet().receive_script(1).unwrap(), 0)], 1500);
    bad.send_all = true;
    assert_eq!(
        plan(wallet(), &coins, &bad),
        Err(PlanError::PaymentToOwnScript { payment: 0 })
    );
    // The change script at another index is not known to be the wallet's
    // from the request alone, and is not refused; the signer treats such an
    // output as another destination (SR-20).
    let mut elsewhere = request(vec![(wallet().change_script(9).unwrap(), 0)], 1500);
    elsewhere.send_all = true;
    elsewhere.change_index = 5;
    assert!(plan(wallet(), &coins, &elsewhere).is_ok());
}

#[test]
fn sp_25_at_least_one_payment_and_at_most_100_outputs() {
    let coins = [coin(1, 0, 50_000_000)];
    assert_eq!(
        plan(wallet(), &coins, &request(vec![], 1000)),
        Err(PlanError::NoPayments)
    );
    let many = |n: usize| -> Vec<(ScriptBuf, u64)> {
        (0..n).map(|i| (external(i as u8), 10_000)).collect()
    };
    // 99 payments and change: 100 outputs.
    let p = plan(wallet(), &coins, &request(many(99), 1000)).unwrap();
    assert_eq!(p.unsigned_tx().output.len(), 100);
    assert!(p.change().is_some());
    // 100 payments leave no room for change; the rest is fee, as long as
    // the fee stays under the bounds.
    let r = plan(
        wallet(),
        &[coin(2, 0, 1_030_000)],
        &request(many(100), 1000),
    );
    let p = r.unwrap();
    assert_eq!(p.unsigned_tx().output.len(), 100);
    assert!(p.change().is_none());
    // 101 are refused.
    assert_eq!(
        plan(wallet(), &coins, &request(many(101), 1000)),
        Err(PlanError::TooManyOutputs)
    );
}
