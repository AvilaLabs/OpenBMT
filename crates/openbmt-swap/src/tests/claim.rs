//! SW-30, SW-31 and SW-40 to SW-45: the lockup and the claim.

use bitcoin::absolute::LockTime;
use bitcoin::hashes::{ripemd160, sha256, Hash};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::LeafVersion;
use bitcoin::transaction::Version;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};

use super::fixtures::*;
use crate::claim::verify_claim;
use crate::{build_claim, check_lockup, rebuild_claim, LockupSeen, Refused, Seen, SwapState};

fn funding(outputs: &[(ScriptBuf, u64)], rbf: bool) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::new(),
            sequence: if rbf {
                Sequence::ENABLE_RBF_NO_LOCKTIME
            } else {
                Sequence::MAX
            },
            witness: Witness::new(),
        }],
        output: outputs
            .iter()
            .map(|(s, v)| TxOut {
                value: Amount::from_sat(*v),
                script_pubkey: s.clone(),
            })
            .collect(),
    }
}

fn state() -> SwapState {
    SwapState::new(&good_swap(), &secrets(), NET, &destination()).unwrap()
}

fn lockup_of(value: u64) -> LockupSeen {
    let swap = good_swap();
    let tx = funding(&[(swap.tree.script_pubkey(), value)], false);
    check_lockup(
        &tx,
        &swap.tree.script_pubkey(),
        swap.onchain_sats,
        Seen::Block,
    )
    .unwrap()
}

fn claim(rate: u64) -> (Transaction, LockupSeen) {
    let lockup = lockup_of(99_350);
    (build_claim(&state(), &lockup, rate, TIP).unwrap(), lockup)
}

fn prevout(lockup: &LockupSeen) -> TxOut {
    TxOut {
        value: Amount::from_sat(lockup.value_sats),
        script_pubkey: good_swap().tree.script_pubkey(),
    }
}

// SW-30, SW-31

#[test]
fn sw_30_exactly_one_output_pays_the_lockup_address_enough() {
    let swap = good_swap();
    let spk = swap.tree.script_pubkey();
    let other = destination().script_pubkey();
    let promised = swap.onchain_sats;
    let tx = funding(&[(other.clone(), 5), (spk.clone(), promised + 7)], false);
    let got = check_lockup(&tx, &spk, promised, Seen::Mempool).unwrap();
    assert_eq!(got.outpoint, OutPoint::new(tx.compute_txid(), 1));
    assert_eq!(got.value_sats, promised + 7);
    let exact = funding(&[(spk.clone(), promised)], false);
    assert!(check_lockup(&exact, &spk, promised, Seen::Block).is_ok());

    let none = funding(&[(other.clone(), promised)], false);
    assert_eq!(
        check_lockup(&none, &spk, promised, Seen::Block).unwrap_err(),
        Refused::NoLockupOutput
    );
    let two = funding(&[(spk.clone(), promised), (spk.clone(), promised)], false);
    assert_eq!(
        check_lockup(&two, &spk, promised, Seen::Block).unwrap_err(),
        Refused::TwoLockupOutputs
    );
    let less = funding(&[(spk.clone(), promised - 1)], false);
    assert_eq!(
        check_lockup(&less, &spk, promised, Seen::Block).unwrap_err(),
        Refused::LockupTooSmall
    );
    // Two outputs that together pay enough are still refused.
    let split = funding(
        &[(spk.clone(), promised / 2), (spk.clone(), promised / 2 + 1)],
        false,
    );
    assert!(check_lockup(&split, &spk, promised, Seen::Block).is_err());
}

#[test]
fn sw_31_the_result_says_confirmed_and_replaceable() {
    let swap = good_swap();
    let spk = swap.tree.script_pubkey();
    for (rbf, seen) in [
        (false, Seen::Mempool),
        (true, Seen::Mempool),
        (false, Seen::Block),
        (true, Seen::Block),
    ] {
        let tx = funding(&[(spk.clone(), swap.onchain_sats)], rbf);
        let got = check_lockup(&tx, &spk, swap.onchain_sats, seen).unwrap();
        assert_eq!(got.confirmed, seen == Seen::Block);
        assert_eq!(got.signals_rbf, rbf);
    }
}

// SW-40 to SW-45

#[test]
fn sw_40_one_input_one_output_to_the_receive_address() {
    let (tx, lockup) = claim(1000);
    assert_eq!(tx.version, Version::TWO);
    assert_eq!(tx.lock_time, LockTime::ZERO);
    assert_eq!(tx.input.len(), 1);
    assert_eq!(tx.input[0].previous_output, lockup.outpoint);
    assert!(tx.input[0].sequence.is_rbf());
    assert!(tx.input[0].script_sig.is_empty());
    assert_eq!(tx.output.len(), 1);
    assert_eq!(tx.output[0].script_pubkey, destination().script_pubkey());
}

#[test]
fn sw_40_a_receive_address_of_another_network_is_refused() {
    let main = bitcoin::Address::p2wpkh(
        &bitcoin::CompressedPublicKey(service_key()),
        bitcoin::Network::Bitcoin,
    );
    assert_eq!(
        SwapState::new(&good_swap(), &secrets(), NET, &main).unwrap_err(),
        Refused::DestinationNetwork
    );
}

#[test]
fn sw_41_the_fee_is_the_rate_times_the_virtual_size_rounded_up() {
    for rate in [1_000u64, 1_001, 2_500, 12_345] {
        let (tx, lockup) = claim(rate);
        let vsize = tx.vsize() as u64;
        let fee = lockup.value_sats - tx.output[0].value.to_sat();
        assert_eq!(fee, (vsize * rate).div_ceil(1000), "{rate}");
    }
    // A taproot script-path claim of this shape is about 140 virtual bytes.
    let (tx, _) = claim(1000);
    assert!((130..=160).contains(&tx.vsize()), "{}", tx.vsize());
}

#[test]
fn sw_41_a_fee_above_half_or_a_dust_output_is_refused() {
    let lockup = lockup_of(99_350);
    // Half of the lockup at about 140 vB is 355 sat per vB.
    assert_eq!(
        build_claim(&state(), &lockup, 400_000, TIP).unwrap_err(),
        Refused::FeeTooHigh
    );
    assert_eq!(
        build_claim(&state(), &lockup, u64::MAX, TIP).unwrap_err(),
        Refused::FeeTooHigh
    );
    assert!(build_claim(&state(), &lockup, 300_000, TIP).is_ok());
    // A lockup so small that the output is dust.
    let tiny = LockupSeen {
        value_sats: 400,
        ..lockup_of(99_350)
    };
    assert_eq!(
        build_claim(&state(), &tiny, 1_000, TIP).unwrap_err(),
        Refused::FeeTooHigh
    );
    assert_eq!(Refused::FeeTooHigh.rule(), "SW-41");
}

#[test]
fn sw_42_the_witness_is_the_signature_the_preimage_the_leaf_and_the_control_block() {
    let (tx, lockup) = claim(1000);
    let items: Vec<&[u8]> = tx.input[0].witness.iter().collect();
    assert_eq!(items.len(), 4);
    let swap = good_swap();
    assert_eq!(items[0].len(), 64);
    assert_eq!(items[1], secrets().preimage());
    assert_eq!(items[2], swap.tree.claim_leaf().as_bytes());
    assert_eq!(
        items[3],
        swap.tree.claim_control_block().unwrap().serialize()
    );
    // The preimage hashes to what the invoice pays and to what the leaf holds.
    assert_eq!(
        sha256::Hash::hash(items[1]).to_byte_array(),
        secrets().preimage_hash()
    );
    assert_eq!(
        &items[2][6..26],
        ripemd160::Hash::hash(&secrets().preimage_hash()).as_byte_array()
    );
    // The signature is the claim key's over the sighash of input 0, default.
    let prev = [prevout(&lockup)];
    let sighash = SighashCache::new(&tx)
        .taproot_script_spend_signature_hash(
            0,
            &Prevouts::All(&prev),
            bitcoin::taproot::TapLeafHash::from_script(
                swap.tree.claim_leaf(),
                LeafVersion::TapScript,
            ),
            TapSighashType::Default,
        )
        .unwrap();
    let sig = bitcoin::secp256k1::schnorr::Signature::from_slice(items[0]).unwrap();
    assert!(bitcoin::secp256k1::Secp256k1::new()
        .verify_schnorr(
            &sig,
            &bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array()),
            &secrets().claim_public().x_only_public_key().0
        )
        .is_ok());
}

#[test]
fn sw_43_the_finished_claim_passes_the_script_check_and_a_changed_one_does_not() {
    let (tx, lockup) = claim(1500);
    let spk = good_swap().tree.script_pubkey();
    let prev = [prevout(&lockup)];
    assert!(verify_claim(&tx, &prev, &spk).is_ok());

    // The preimage changed.
    let mut bad = tx.clone();
    let mut items: Vec<Vec<u8>> = bad.input[0].witness.iter().map(<[u8]>::to_vec).collect();
    items[1][0] ^= 1;
    bad.input[0].witness = Witness::from_slice(&items);
    assert_eq!(
        verify_claim(&bad, &prev, &spk),
        Err(Refused::ClaimFailsScript)
    );

    // The signature changed.
    let mut bad = tx.clone();
    let mut items: Vec<Vec<u8>> = bad.input[0].witness.iter().map(<[u8]>::to_vec).collect();
    items[0][10] ^= 1;
    bad.input[0].witness = Witness::from_slice(&items);
    assert_eq!(
        verify_claim(&bad, &prev, &spk),
        Err(Refused::ClaimFailsScript)
    );

    // The output changed after the signature was made.
    let mut bad = tx.clone();
    bad.output[0].value = Amount::from_sat(bad.output[0].value.to_sat() + 1);
    assert_eq!(
        verify_claim(&bad, &prev, &spk),
        Err(Refused::ClaimFailsScript)
    );

    // The control block of another tree.
    let mut bad = tx.clone();
    let mut items: Vec<Vec<u8>> = bad.input[0].witness.iter().map(<[u8]>::to_vec).collect();
    items[3][5] ^= 1;
    bad.input[0].witness = Witness::from_slice(&items);
    assert_eq!(
        verify_claim(&bad, &prev, &spk),
        Err(Refused::ClaimFailsScript)
    );

    // The same claim against another output.
    let other = destination().script_pubkey();
    assert_eq!(
        verify_claim(&tx, &prev, &other),
        Err(Refused::ClaimFailsScript)
    );
    // No witness at all.
    let mut bare = tx.clone();
    bare.input[0].witness = Witness::new();
    assert_eq!(
        verify_claim(&bare, &prev, &spk),
        Err(Refused::ClaimFailsScript)
    );
}

#[test]
fn sw_44_no_claim_at_or_after_the_timeout() {
    let lockup = lockup_of(99_350);
    assert!(build_claim(&state(), &lockup, 1000, TIMEOUT - 1).is_ok());
    assert_eq!(
        build_claim(&state(), &lockup, 1000, TIMEOUT).unwrap_err(),
        Refused::TooLate
    );
    assert_eq!(
        build_claim(&state(), &lockup, 1000, u32::MAX).unwrap_err(),
        Refused::TooLate
    );
}

#[test]
fn sw_45_the_claim_is_by_the_script_path() {
    let (tx, _) = claim(1000);
    let last = tx.input[0].witness.last().unwrap();
    // A control block: the leaf version in the high bits, then the internal
    // key and one hash of the sibling leaf.
    assert_eq!(last[0] & 0xfe, 0xc0);
    assert_eq!(last.len(), 33 + 32);
    assert_eq!(tx.input[0].witness.len(), 4);
}

#[test]
fn sw_53_a_claimed_swap_is_not_claimed_again() {
    let lockup = lockup_of(99_350);
    let (tx, _) = claim(1000);
    let claimed = state().claimed(tx.compute_txid()).unwrap();
    assert_eq!(
        build_claim(&claimed, &lockup, 1000, TIP).unwrap_err(),
        Refused::AlreadyClaimed
    );
}

#[test]
fn sw_46_a_claim_is_built_again_at_another_rate_with_the_same_rules() {
    let (first, lockup) = claim(2_000);
    let waiting = state();
    // A swap that was not claimed has nothing to build again.
    assert_eq!(
        rebuild_claim(&waiting, &lockup, 3_000, TIP)
            .unwrap_err()
            .rule(),
        "SW-71"
    );
    let sent = waiting.claimed(first.compute_txid()).unwrap();
    let second = rebuild_claim(&sent, &lockup, 4_000, TIP).unwrap();
    assert_ne!(first.compute_txid(), second.compute_txid());
    // The same input and address, a higher fee, replacement signalled.
    assert_eq!(
        first.input[0].previous_output,
        second.input[0].previous_output
    );
    assert_eq!(
        first.output[0].script_pubkey,
        second.output[0].script_pubkey
    );
    assert!(second.output[0].value < first.output[0].value);
    assert!(second.is_explicitly_rbf());
    // The same limits: at the timeout, or a fee above half, it is refused.
    assert_eq!(
        rebuild_claim(&sent, &lockup, 4_000, TIMEOUT).unwrap_err(),
        Refused::TooLate
    );
    assert_eq!(
        rebuild_claim(&sent, &lockup, 4_000_000, TIP).unwrap_err(),
        Refused::FeeTooHigh
    );
    // The state names the claim sent last, and a swap not claimed refuses it.
    let again = sent.reclaimed(second.compute_txid()).unwrap();
    assert_eq!(again.phase(), crate::Phase::Claimed(second.compute_txid()));
    assert!(waiting.reclaimed(second.compute_txid()).is_err());
}
