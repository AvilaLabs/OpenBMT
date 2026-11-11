//! The claim transaction (SW-40 to SW-45): spend the lockup by the claim
//! leaf, to the shop's own receive address.

use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::key::Keypair;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::LeafVersion;
use bitcoin::transaction::Version;
use bitcoin::{Amount, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};
use miniscript::interpreter::Interpreter;

use crate::error::Refused;
use crate::lockup::LockupSeen;
use crate::state::{Phase, SwapState};

/// SW-40 to SW-44: the claim of `lockup`, signed, at `fee_rate` satoshis
/// per 1000 virtual bytes, for a chain at `tip_height`. The transaction is
/// checked against the output it spends before it is returned.
pub fn build_claim(
    state: &SwapState,
    lockup: &LockupSeen,
    fee_rate_per_kvb: u64,
    tip_height: u32,
) -> Result<Transaction, Refused> {
    if state.phase() != Phase::Waiting {
        return Err(Refused::AlreadyClaimed);
    }
    if tip_height >= state.timeout() {
        return Err(Refused::TooLate);
    }
    let secrets = state.secrets();
    let tree = state.tree()?;
    let spk = tree.script_pubkey();
    let control = tree
        .claim_control_block()
        .ok_or(Refused::ClaimUnbuildable("control block"))?;
    let leaf = tree.claim_leaf().clone();
    let prevout = TxOut {
        value: Amount::from_sat(lockup.value_sats),
        script_pubkey: spk.clone(),
    };
    let dest_script = state.destination().script_pubkey();

    let mut tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: lockup.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(lockup.value_sats),
            script_pubkey: dest_script.clone(),
        }],
    };
    let witness_of = |sig: &[u8]| {
        Witness::from_slice(&[
            sig,
            secrets.preimage().as_slice(),
            leaf.as_bytes(),
            control.serialize().as_slice(),
        ])
    };

    // The size does not depend on the amount, so the fee is known from a
    // transaction with a signature of the right width.
    set_witness(&mut tx, witness_of(&[0u8; 64]))?;
    let vsize = u64::try_from(tx.vsize()).map_err(|_| Refused::FeeTooHigh)?;
    let fee = u64::try_from((u128::from(vsize) * u128::from(fee_rate_per_kvb)).div_ceil(1000))
        .map_err(|_| Refused::FeeTooHigh)?;
    let out_value = lockup
        .value_sats
        .checked_sub(fee)
        .filter(|_| fee <= lockup.value_sats / 2)
        .ok_or(Refused::FeeTooHigh)?;
    if out_value < dest_script.minimal_non_dust().to_sat() {
        return Err(Refused::FeeTooHigh);
    }
    tx.output
        .first_mut()
        .ok_or(Refused::ClaimUnbuildable("output"))?
        .value = Amount::from_sat(out_value);

    // SW-42: the signature of the claim leaf by the claim key.
    let secp = Secp256k1::new();
    let leaf_hash = bitcoin::taproot::TapLeafHash::from_script(&leaf, LeafVersion::TapScript);
    let prevouts = [prevout];
    let sighash = SighashCache::new(&tx)
        .taproot_script_spend_signature_hash(
            0,
            &Prevouts::All(&prevouts),
            leaf_hash,
            TapSighashType::Default,
        )
        .map_err(|_| Refused::ClaimUnbuildable("sighash"))?;
    let keypair = Keypair::from_secret_key(&secp, secrets.claim_key());
    let sig =
        secp.sign_schnorr_no_aux_rand(&Message::from_digest(sighash.to_byte_array()), &keypair);
    set_witness(&mut tx, witness_of(sig.as_ref()))?;

    verify_claim(&tx, &prevouts, &spk)?;
    Ok(tx)
}

fn set_witness(tx: &mut Transaction, w: Witness) -> Result<(), Refused> {
    tx.input
        .first_mut()
        .ok_or(Refused::ClaimUnbuildable("input"))?
        .witness = w;
    Ok(())
}

/// SW-43: run the claim against the script it spends.
pub(crate) fn verify_claim(
    tx: &Transaction,
    prevouts: &[TxOut],
    spk: &ScriptBuf,
) -> Result<(), Refused> {
    let input = tx.input.first().ok_or(Refused::ClaimFailsScript)?;
    let interpreter = Interpreter::from_txdata(
        spk,
        &input.script_sig,
        &input.witness,
        input.sequence,
        tx.lock_time,
    )
    .map_err(|_| Refused::ClaimFailsScript)?;
    let secp = Secp256k1::verification_only();
    let mut satisfied = 0usize;
    for step in interpreter.iter(&secp, tx, 0, &Prevouts::All(prevouts)) {
        step.map_err(|_| Refused::ClaimFailsScript)?;
        satisfied += 1;
    }
    // The preimage and the signature are the two conditions of the leaf.
    if satisfied < 2 {
        return Err(Refused::ClaimFailsScript);
    }
    Ok(())
}
