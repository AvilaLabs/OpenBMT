//! SP-40 to SP-42: the PSBT.

mod common;

use bitcoin::psbt::{Input, Output, Psbt};
use common::*;
use openbmt_core::wallet::Keychain;
use openbmt_signer::OutputKind;
use openbmt_spend::{plan, ChangePosition, PlanOutput, PsbtError};

#[test]
fn sp_40_the_file_holds_what_the_signer_needs_and_nothing_else() {
    let coins = [
        coin(1, 2, 300_000),
        coin_tagged(2, Keychain::Change, 4, 400_000, 6),
    ];
    let p = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 500_000)], 1000),
    )
    .unwrap();
    let psbt = p.to_psbt(wallet()).unwrap();
    assert_eq!(psbt.version, 0);
    assert_eq!(psbt.unsigned_tx, *p.unsigned_tx());
    assert!(psbt.xpub.is_empty() && psbt.unknown.is_empty() && psbt.proprietary.is_empty());
    assert_eq!(psbt.inputs.len(), 2);
    for (entry, planned) in psbt.inputs.iter().zip(p.inputs()) {
        let c = &planned.coin;
        assert_eq!(entry.non_witness_utxo.as_ref(), Some(&c.prev_tx));
        assert_eq!(entry.witness_utxo.as_ref(), c.output());
        assert_eq!(
            entry.witness_script,
            Some(wallet().witness_script(c.chain, c.index).unwrap())
        );
        // The three keys, with the slots' origins.
        assert_eq!(entry.bip32_derivation.len(), 3);
        let keys = wallet().derived_keys(c.chain, c.index).unwrap();
        for key in keys {
            assert!(entry.bip32_derivation.contains_key(&key.inner));
        }
        for (slot, key) in wallet().slots().iter().zip(keys) {
            let (fingerprint, path) = &entry.bip32_derivation[&key.inner];
            assert_eq!(*fingerprint, slot.origin_fingerprint);
            let steps: &[bitcoin::bip32::ChildNumber] = path.as_ref();
            let origin: &[bitcoin::bip32::ChildNumber] = slot.origin_path.as_ref();
            assert_eq!(&steps[..origin.len()], origin);
            assert_eq!(steps.len(), origin.len() + 2);
        }
        // Nothing else.
        let expected = Input {
            non_witness_utxo: entry.non_witness_utxo.clone(),
            witness_utxo: entry.witness_utxo.clone(),
            witness_script: entry.witness_script.clone(),
            bip32_derivation: entry.bip32_derivation.clone(),
            ..Input::default()
        };
        assert_eq!(*entry, expected);
    }
    // It survives its own bytes.
    assert_eq!(Psbt::deserialize(&psbt.serialize()).unwrap(), psbt);
}

#[test]
fn sp_41_only_the_change_output_carries_hints() {
    let coins = [coin(1, 0, 1_000_000)];
    let mut r = request(vec![(external(1), 100_000), (external(2), 200_000)], 1000);
    r.change_index = 6;
    let p = plan(wallet(), &coins, &r).unwrap();
    let psbt = p.to_psbt(wallet()).unwrap();
    assert_eq!(psbt.outputs.len(), 3);
    assert_eq!(psbt.outputs[0], Output::default());
    assert_eq!(psbt.outputs[1], Output::default());
    let change = &psbt.outputs[2];
    assert_eq!(
        change.witness_script,
        Some(wallet().witness_script(Keychain::Change, 6).unwrap())
    );
    assert_eq!(change.bip32_derivation.len(), 3);
    for key in wallet().derived_keys(Keychain::Change, 6).unwrap() {
        assert!(change.bip32_derivation.contains_key(&key.inner));
    }
}

#[test]
fn sp_41_the_consolidation_output_carries_hints_too() {
    let coins = [coin(1, 0, 300_000), coin(2, 1, 300_000)];
    let mut r = request(vec![(wallet().change_script(3).unwrap(), 0)], 1000);
    r.change_index = 3;
    r.send_all = true;
    let p = plan(wallet(), &coins, &r).unwrap();
    let psbt = p.to_psbt(wallet()).unwrap();
    assert_eq!(psbt.outputs.len(), 1);
    assert_eq!(psbt.outputs[0].bip32_derivation.len(), 3);
    // The signer reads it as coins moving inside the wallet (SR-25).
    let review = signer(1).review(&psbt.serialize(), NOW).unwrap();
    assert!(review.moves_coins_inside_wallet());
    assert_eq!(review.change(), Some((3, p.payments()[0].sats)));
}

#[test]
fn sp_41_a_plan_for_other_coins_does_not_become_a_file_for_this_wallet() {
    let coins = [coin(1, 0, 300_000)];
    let p = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 100_000)], 1000),
    )
    .unwrap();
    // Another wallet's description.
    let other = {
        use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
        use openbmt_core::wallet::{KeySlot, WalletDescription};
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let slots: Vec<KeySlot> = (0..3u8)
            .map(|n| {
                let master = Xpriv::new_master(NET, &[n + 50; 32]).unwrap();
                let account = master.derive_priv(secp(), &path).unwrap();
                KeySlot {
                    index: n + 1,
                    origin_fingerprint: master.fingerprint(secp()),
                    origin_path: path.clone(),
                    xpub: Xpub::from_priv(secp(), &account),
                }
            })
            .collect();
        WalletDescription::new(NET, slots.try_into().unwrap()).unwrap()
    };
    assert!(matches!(p.to_psbt(&other), Err(PsbtError::Coin(_))));
}

#[test]
fn sp_42_the_signer_reviews_the_file_with_no_refusal() {
    let coins = [
        coin(1, 0, 300_000),
        coin_tagged(2, Keychain::Change, 1, 400_000, 6),
        coin(3, 5, 250_000),
    ];
    let mut r = request(vec![(external(1), 500_000), (external(2), 40_000)], 3000);
    r.change_index = 2;
    let p = plan(wallet(), &coins, &r).unwrap();
    let psbt = p.to_psbt(wallet()).unwrap();
    let review = signer(1).review(&psbt.serialize(), NOW).unwrap();
    // What the signer shows is what the plan holds.
    assert_eq!(review.fee_sats(), p.fee_sats());
    assert_eq!(review.vsize(), p.vsize());
    assert_eq!(review.weight(), p.weight());
    assert_eq!(review.input_total_sats(), p.total_in_sats());
    assert_eq!(review.input_count(), p.inputs().len());
    let change = p.change().unwrap();
    assert_eq!(review.change(), Some((change.index, change.sats)));
    assert_eq!(review.outputs().len(), 3);
    assert_eq!(review.outputs()[0].kind, OutputKind::Other);
    assert_eq!(review.outputs()[2].kind, OutputKind::Change { index: 2 });
    assert_eq!(review.txid(), p.txid());
    // And it signs.
    let signed = sign_with_signer(&psbt, 1);
    assert!(signed.inputs.iter().all(|i| i.partial_sigs.len() == 1));
}

#[test]
fn sp_42_a_plan_with_no_change_and_a_send_all_pass_the_signer_too() {
    let coins = [coin(1, 0, 50_146)];
    let p = plan(
        wallet(),
        &coins,
        &request(vec![(external(1), 50_000)], 1000),
    )
    .unwrap();
    assert!(p.change().is_none());
    let signed = sign_with_signer(&p.to_psbt(wallet()).unwrap(), 0);
    assert_eq!(signed.inputs[0].partial_sigs.len(), 1);

    let many = [coin(2, 0, 100_000), coin(3, 1, 100_000)];
    let mut r = request(vec![(external(4), 0)], 5000);
    r.send_all = true;
    let p = plan(wallet(), &many, &r).unwrap();
    let signed = sign_with_signer(&p.to_psbt(wallet()).unwrap(), 1);
    assert!(signed.inputs.iter().all(|i| i.partial_sigs.len() == 1));
}

#[test]
fn sp_37_the_lock_height_survives_the_file() {
    let coins = [coin(1, 0, 300_000)];
    let mut r = request(vec![(external(1), 100_000)], 1000);
    r.lock_height = 851_234;
    let p = plan(wallet(), &coins, &r).unwrap();
    let psbt = p.to_psbt(wallet()).unwrap();
    assert_eq!(
        psbt.unsigned_tx.lock_time,
        bitcoin::absolute::LockTime::from_height(851_234).unwrap()
    );
    let again = Psbt::deserialize(&psbt.serialize()).unwrap();
    assert_eq!(again.unsigned_tx, *p.unsigned_tx());
    assert_eq!(again.unsigned_tx.lock_time.to_consensus_u32(), 851_234);
    // The signer reviews and signs it.
    let signed = sign_with_signer(&psbt, 0);
    assert!(signed.inputs.iter().all(|i| i.partial_sigs.len() == 1));
}

#[test]
fn sp_41_the_change_carries_hints_wherever_it_is() {
    let coins = [coin(1, 0, 1_000_000)];
    let mut at = std::collections::BTreeSet::new();
    for n in 0..40u8 {
        let mut r = request(vec![(external(1), 100_000), (external(2), 50_000)], 1000);
        r.change_index = 5;
        r.change_position = ChangePosition::Seed([n; 32]);
        let p = plan(wallet(), &coins, &r).unwrap();
        let place = p.change_position().unwrap();
        at.insert(place);
        let psbt = p.to_psbt(wallet()).unwrap();
        for (position, entry) in psbt.outputs.iter().enumerate() {
            if position == place {
                assert_eq!(entry.bip32_derivation.len(), 3);
                assert!(entry.witness_script.is_some());
            } else {
                assert!(entry.bip32_derivation.is_empty() && entry.witness_script.is_none());
            }
        }
        // The signer shows exactly that output as change (SP-42).
        let review = signer(1).review(&psbt.serialize(), NOW).unwrap();
        let change = p.change().unwrap();
        assert_eq!(review.change(), Some((5, change.sats)));
        for (position, out) in review.outputs().iter().enumerate() {
            match p.outputs()[position] {
                PlanOutput::Change(_) => assert_eq!(out.kind, OutputKind::Change { index: 5 }),
                PlanOutput::Payment(_) => assert_eq!(out.kind, OutputKind::Other),
            }
        }
        assert!(sign_with_signer(&psbt, 1)
            .inputs
            .iter()
            .all(|i| i.partial_sigs.len() == 1));
    }
    assert_eq!(at.len(), 3, "every place of three was met");
}
