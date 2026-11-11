//! The file for a signer made by someone else, and what comes back from it
//! (section 5 of the rules, SP-45 to SP-48).

use std::fmt;

use bitcoin::psbt::{Input, Output, Psbt, PsbtSighashType};
use bitcoin::sighash::EcdsaSighashType;
use bitcoin::{PublicKey, Transaction, Witness};
use openbmt_core::wallet::{Keychain, WalletDescription};

use crate::combine::{combine, CombineError};
use crate::common::{claimed_path, key_hints, shape_is_v0};
use crate::finalize::{audit, FinalizeError};

/// Why a file could not be written for a signer, or what came back from one
/// was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignerFileError {
    /// The file is not a PSBT version 0 with one entry per input and
    /// output.
    NotVersionZero,
    /// SP-45: an input lacks what a signer needs, or its script, keys or
    /// previous transaction are not the wallet's.
    Input {
        /// The input.
        input: usize,
        /// What is wrong.
        what: &'static str,
    },
    /// SP-45: an output that claims to be the wallet's carries keys or a
    /// script that are not.
    Output {
        /// The output.
        output: usize,
    },
    /// SP-45: a signature of the file is not `SIGHASH_ALL`.
    SighashType {
        /// The input.
        input: usize,
    },
    /// SP-46: what came back holds another unsigned transaction than the one
    /// sent.
    DifferentTransaction,
    /// SP-46: what came back has a signature that does not verify, or one
    /// that the sent file's rules refuse.
    Combine(CombineError),
    /// SP-46: no signature of the signer's key on this input.
    NotSigned {
        /// The input.
        input: usize,
    },
    /// SP-47: the finished transaction does not pass the check of SP-62.
    Finished(FinalizeError),
    /// SP-45: a slot that is not 1, 2 or 3.
    Slot,
}

impl fmt::Display for SignerFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotVersionZero => {
                write!(f, "the file is not a PSBT version 0 of the right shape")
            }
            Self::Input { input, what } => write!(f, "SP-45: input {input}: {what}"),
            Self::Output { output } => write!(
                f,
                "SP-45: output {output} claims to be the wallet's but is not"
            ),
            Self::SighashType { input } => {
                write!(f, "SP-45: a signature on input {input} is not SIGHASH_ALL")
            }
            Self::DifferentTransaction => write!(
                f,
                "SP-46: what came back holds another transaction than the one sent"
            ),
            Self::Combine(e) => write!(f, "SP-46: {e}"),
            Self::NotSigned { input } => write!(
                f,
                "SP-46: input {input} has no signature of the signer's key"
            ),
            Self::Finished(e) => write!(f, "SP-47: {e}"),
            Self::Slot => write!(f, "SP-45: the slot is not 1, 2 or 3"),
        }
    }
}

impl std::error::Error for SignerFileError {}

fn bad_input(input: usize, what: &'static str) -> SignerFileError {
    SignerFileError::Input { input, what }
}

/// SP-45: `psbt`, made for a signer that someone else made: the file of
/// SP-40 with the hash type `SIGHASH_ALL` on every input and the three
/// keys as global extended public keys with their origins, so that the
/// signer can show and check the wallet without being told it first.
///
/// `psbt` is a plan's file (`Plan::to_psbt`) or what the owner's own signer
/// returned, with its signature. The file is built again from the fields
/// that are read here and nothing else (unknown and proprietary fields are
/// not copied), and every input is held to the wallet: the previous
/// transaction hashes to the outpoint, the previous output is the wallet's
/// script at the place the hints name, the witness script is the wallet's,
/// and the hints are exactly the three keys. An output that carries a
/// witness script or hints must be the wallet's change script at the index
/// they name, with exactly the wallet's script and keys.
pub fn psbt_for_signer(wallet: &WalletDescription, psbt: &Psbt) -> Result<Psbt, SignerFileError> {
    if !shape_is_v0(psbt) {
        return Err(SignerFileError::NotVersionZero);
    }
    let mut out = Psbt::from_unsigned_tx(psbt.unsigned_tx.clone())
        .map_err(|_| SignerFileError::NotVersionZero)?;
    for slot in wallet.slots() {
        out.xpub.insert(
            slot.xpub,
            (slot.origin_fingerprint, slot.origin_path.clone()),
        );
    }
    for (i, (txin, input)) in psbt.unsigned_tx.input.iter().zip(&psbt.inputs).enumerate() {
        let prev = input
            .non_witness_utxo
            .as_ref()
            .ok_or_else(|| bad_input(i, "no previous transaction"))?;
        if prev.compute_txid() != txin.previous_output.txid {
            return Err(bad_input(
                i,
                "the previous transaction is not the outpoint's",
            ));
        }
        let spent = usize::try_from(txin.previous_output.vout)
            .ok()
            .and_then(|v| prev.output.get(v))
            .ok_or_else(|| bad_input(i, "the output spent is not in the previous transaction"))?;
        if input.witness_utxo.as_ref().is_some_and(|w| w != spent) {
            return Err(bad_input(
                i,
                "the previous output differs from the previous transaction's",
            ));
        }
        let (chain, index) = claimed_path(wallet, &input.bip32_derivation)
            .map_err(|_| bad_input(i, "the key hints do not name a place in the wallet"))?;
        let want_script = wallet
            .script(chain, index)
            .map_err(|_| bad_input(i, "the wallet cannot derive its script"))?;
        if want_script != spent.script_pubkey {
            return Err(bad_input(i, "the script spent is not the wallet's"));
        }
        let witness_script = wallet
            .witness_script(chain, index)
            .map_err(|_| bad_input(i, "the wallet cannot derive its script"))?;
        if input
            .witness_script
            .as_ref()
            .is_some_and(|w| *w != witness_script)
        {
            return Err(bad_input(i, "the witness script is not the wallet's"));
        }
        let hints = key_hints(wallet, chain, index)
            .ok_or_else(|| bad_input(i, "the wallet cannot derive its keys"))?;
        if input.bip32_derivation != hints {
            return Err(bad_input(i, "the key hints are not exactly the three keys"));
        }
        let mut sigs = input.partial_sigs.clone();
        for sig in sigs.values_mut() {
            if sig.sighash_type != EcdsaSighashType::All {
                return Err(SignerFileError::SighashType { input: i });
            }
        }
        let Some(slot) = out.inputs.get_mut(i) else {
            return Err(SignerFileError::NotVersionZero);
        };
        *slot = Input {
            non_witness_utxo: Some(prev.clone()),
            witness_utxo: Some(spent.clone()),
            witness_script: Some(witness_script),
            bip32_derivation: hints,
            partial_sigs: sigs,
            sighash_type: Some(PsbtSighashType::from(EcdsaSighashType::All)),
            ..Input::default()
        };
    }
    for (o, (txout, entry)) in psbt
        .unsigned_tx
        .output
        .iter()
        .zip(&psbt.outputs)
        .enumerate()
    {
        if entry.witness_script.is_none() && entry.bip32_derivation.is_empty() {
            continue;
        }
        let (chain, index) = claimed_path(wallet, &entry.bip32_derivation)
            .map_err(|_| SignerFileError::Output { output: o })?;
        let fits = chain == Keychain::Change
            && wallet.script(chain, index).ok().as_ref() == Some(&txout.script_pubkey)
            && wallet.witness_script(chain, index).ok() == entry.witness_script
            && Some(&entry.bip32_derivation) == key_hints(wallet, chain, index).as_ref();
        if !fits {
            return Err(SignerFileError::Output { output: o });
        }
        let Some(slot) = out.outputs.get_mut(o) else {
            return Err(SignerFileError::NotVersionZero);
        };
        *slot = Output {
            witness_script: entry.witness_script.clone(),
            bip32_derivation: entry.bip32_derivation.clone(),
            ..Output::default()
        };
    }
    Ok(out)
}

/// SP-46: the file that was sent (`sent`, with the first signer's
/// signatures) joined with what the signer in `slot` (1 to 3) returned.
///
/// Only the unsigned transaction and the signatures of `returned` are
/// read; its other fields, which signers rewrite in their own ways, are
/// not. The unsigned transaction must be the sent one's (inputs, outputs,
/// amounts, version and lock time). Every signature goes through
/// [`combine`], which verifies each against the BIP 143 hash, and the
/// result must hold a signature of the slot's key on every input.
pub fn take_signer_signatures(
    wallet: &WalletDescription,
    sent: &Psbt,
    returned: &Psbt,
    slot: u8,
) -> Result<Psbt, SignerFileError> {
    let slot_index = match slot {
        1..=3 => usize::from(slot) - 1,
        _ => return Err(SignerFileError::Slot),
    };
    if !shape_is_v0(sent) || !shape_is_v0(returned) {
        return Err(SignerFileError::NotVersionZero);
    }
    if returned.unsigned_tx != sent.unsigned_tx {
        return Err(SignerFileError::DifferentTransaction);
    }
    let mut copy = sent.clone();
    for (into, from) in copy.inputs.iter_mut().zip(&returned.inputs) {
        into.partial_sigs = from.partial_sigs.clone();
        into.sighash_type = from.sighash_type;
    }
    let both = combine(sent, &[copy]).map_err(SignerFileError::Combine)?;
    for (i, input) in both.inputs.iter().enumerate() {
        let (chain, index) = claimed_path(wallet, &input.bip32_derivation)
            .map_err(|_| bad_input(i, "the key hints do not name a place in the wallet"))?;
        let keys = wallet
            .derived_keys(chain, index)
            .map_err(|_| bad_input(i, "the wallet cannot derive its keys"))?;
        let key: PublicKey = keys.get(slot_index).copied().ok_or(SignerFileError::Slot)?;
        if !input.partial_sigs.contains_key(&key) {
            return Err(SignerFileError::NotSigned { input: i });
        }
    }
    Ok(both)
}

/// SP-47: the fee of `tx` if it is a finished transaction of the file
/// `sent`: the same transaction ID, version, lock time, outputs and
/// sequences, no script signature, and on every input the witness of SP-60
/// whose two signatures verify against two keys of the script in order. This
/// is the check of SP-62, and it does not say whose signatures they are.
pub fn check_finished(
    wallet: &WalletDescription,
    sent: &Psbt,
    tx: &Transaction,
) -> Result<u64, SignerFileError> {
    if !shape_is_v0(sent) {
        return Err(SignerFileError::NotVersionZero);
    }
    audit(wallet, sent, tx).map_err(SignerFileError::Finished)
}

/// SP-48: the transaction in a file whose every input is final (a signer
/// that finishes what it holds a second signature for). `None` when any
/// input is not final or any input has a script signature.
pub fn transaction_of_final_file(psbt: &Psbt) -> Option<Transaction> {
    if !shape_is_v0(psbt) {
        return None;
    }
    let mut tx = psbt.unsigned_tx.clone();
    for (txin, input) in tx.input.iter_mut().zip(&psbt.inputs) {
        let witness: &Witness = input.final_script_witness.as_ref()?;
        if input
            .final_script_sig
            .as_ref()
            .is_some_and(|s| !s.is_empty())
        {
            return None;
        }
        txin.witness = witness.clone();
    }
    Some(tx)
}
