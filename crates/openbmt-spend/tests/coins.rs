//! SP-01 to SP-03: coins.

mod common;

use bitcoin::{Amount, OutPoint};
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_spend::{plan, CoinError, PlanError};

#[test]
fn sp_01_a_previous_transaction_that_does_not_hash_to_the_outpoint_is_refused() {
    let good = coin(1, 0, 100_000);
    assert_eq!(good.check(wallet()), Ok(()));
    let mut bad = good.clone();
    bad.prev_tx.output[1].value = Amount::from_sat(900_000);
    assert_eq!(
        bad.check(wallet()),
        Err(CoinError::PreviousTransactionMismatch {
            outpoint: bad.outpoint
        })
    );
    let r = plan(
        wallet(),
        &[bad.clone()],
        &request(vec![(external(1), 10_000)], 1000),
    );
    assert_eq!(
        r,
        Err(PlanError::Coin(CoinError::PreviousTransactionMismatch {
            outpoint: bad.outpoint
        }))
    );
    assert!(r.unwrap_err().to_string().starts_with("SP-01"));
}

#[test]
fn sp_01_an_output_index_that_is_not_in_the_transaction_is_refused() {
    let mut bad = coin(1, 0, 100_000);
    bad.vout = 7;
    bad.outpoint = OutPoint {
        txid: bad.outpoint.txid,
        vout: 7,
    };
    assert_eq!(
        bad.check(wallet()),
        Err(CoinError::OutputMissing {
            outpoint: bad.outpoint
        })
    );
    // A coin whose `vout` differs from its outpoint's is refused too.
    let mut odd = coin(2, 0, 100_000);
    odd.vout = 0;
    assert!(matches!(
        odd.check(wallet()),
        Err(CoinError::OutputMissing { .. })
    ));
}

#[test]
fn sp_02_a_coin_at_the_wrong_index_is_refused() {
    let mut bad = coin(1, 3, 100_000);
    bad.index = 4;
    assert_eq!(
        bad.check(wallet()),
        Err(CoinError::ScriptNotInWallet {
            outpoint: bad.outpoint
        })
    );
    let mut wrong_chain = coin(2, 3, 100_000);
    wrong_chain.chain = Keychain::Change;
    assert!(wrong_chain.check(wallet()).is_err());
    let r = plan(
        wallet(),
        &[bad.clone()],
        &request(vec![(external(1), 10_000)], 1000),
    );
    assert!(matches!(
        r,
        Err(PlanError::Coin(CoinError::ScriptNotInWallet { .. }))
    ));
    assert!(r.unwrap_err().to_string().starts_with("SP-02"));
}

#[test]
fn sp_02_a_coin_that_pays_another_script_is_refused() {
    let mut bad = coin(1, 0, 100_000);
    bad.prev_tx.output[1].script_pubkey = external(9);
    bad.outpoint = OutPoint {
        txid: bad.prev_tx.compute_txid(),
        vout: 1,
    };
    assert_eq!(
        bad.check(wallet()),
        Err(CoinError::ScriptNotInWallet {
            outpoint: bad.outpoint
        })
    );
}

#[test]
fn sp_02_a_bad_coin_is_refused_even_when_it_is_not_eligible() {
    // The bad coin has no confirmation, and the other could pay alone.
    let mut bad = coin_conf(2, 1, 50_000, 0);
    bad.index = 9;
    let coins = [coin(1, 0, 500_000), bad];
    let r = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 10_000)], 1000),
    );
    assert!(matches!(r, Err(PlanError::Coin(_))), "{r:?}");
}

#[test]
fn sp_03_the_same_outpoint_twice_is_refused() {
    let c = coin(1, 0, 500_000);
    let r = plan(
        wallet(),
        &[c.clone(), c.clone()],
        &request(vec![(external(1), 10_000)], 1000),
    );
    assert_eq!(r, Err(PlanError::DuplicateCoin(c.outpoint)));
    assert!(r.unwrap_err().to_string().starts_with("SP-03"));
}
