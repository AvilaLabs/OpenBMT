//! Finishing a transaction (section 7 of the rules).

use std::fmt;

use bitcoin::absolute::LockTime;
use bitcoin::ecdsa;
use bitcoin::psbt::{Input, Psbt};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{PublicKey, Transaction, Witness};
use openbmt_signer::MAX_WEIGHT;

use crate::common::{
    claimed_path, input_message, script_keys, secp, shape_is_v0, single_input_message, HintFault,
    MAX_MONEY,
};
use crate::kind::{single_claimed_path, single_key, SingleHintFault, WalletKind, WalletRef};
use crate::plan::Plan;

/// Why a transaction could not be finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinalizeError {
    /// The file is not a PSBT version 0 with one entry per input and
    /// output.
    NotVersionZero,
    /// The input has no previous transaction.
    MissingPreviousTransaction {
        /// The input.
        input: usize,
    },
    /// The previous transaction does not hash to the input's outpoint.
    PreviousTransactionMismatch {
        /// The input.
        input: usize,
    },
    /// The previous output is not in the previous transaction.
    PreviousOutputMissing {
        /// The input.
        input: usize,
    },
    /// The input's `witness_utxo` differs from the previous transaction's
    /// output.
    WitnessUtxoMismatch {
        /// The input.
        input: usize,
    },
    /// SP-61: no key-path hint names a place in the wallet.
    NoWalletKeyHint {
        /// The input.
        input: usize,
    },
    /// SP-61: the hints name different places, or a key the wallet does not
    /// derive there.
    HintsInconsistent {
        /// The input.
        input: usize,
    },
    /// SP-61: the script spent is not the wallet's script at the place the
    /// hints name.
    ScriptNotInWallet {
        /// The input.
        input: usize,
    },
    /// SP-61: the witness script in the file is not the wallet's.
    WitnessScriptMismatch {
        /// The input.
        input: usize,
    },
    /// The input already has a final script or witness.
    InputAlreadyFinal {
        /// The input.
        input: usize,
    },
    /// A hash type other than `SIGHASH_ALL`.
    SighashType {
        /// The input.
        input: usize,
    },
    /// A signature for a key that is not in the input's script.
    ForeignKey {
        /// The input.
        input: usize,
        /// The key.
        key: PublicKey,
    },
    /// A signature that does not verify.
    BadSignature {
        /// The input.
        input: usize,
        /// The key.
        key: PublicKey,
    },
    /// SP-60: fewer than two signatures by different keys.
    NotEnoughSignatures {
        /// The input.
        input: usize,
        /// How many good signatures it has.
        have: usize,
    },
    /// SP-62: an input amount is out of range or the totals overflow.
    AmountOutOfRange,
    /// SP-62: the outputs are more than the inputs.
    OutputsExceedInputs,
    /// SP-63: the finished transaction is heavier than the limit.
    TooHeavy {
        /// Its weight in weight units.
        weight: u64,
    },
    /// SP-62: the check of the finished transaction failed.
    FinalCheckFailed(&'static str),
    /// SP-37: the lock time is a time, not a block height.
    LockTimeNotHeight {
        /// The lock time found.
        lock_time: u32,
    },
    /// SP-64: the file's transaction is not the plan's.
    NotThePlan,
    /// SP-64: the fee is not the plan's fee.
    FeeNotThePlan {
        /// The fee of the file.
        found: u64,
        /// The fee of the plan.
        planned: u64,
    },
    /// SP-80: the input is of the other kind of wallet.
    WrongKind {
        /// The input.
        input: usize,
        /// The kind of wallet the path expected.
        expected: WalletKind,
    },
    /// SP-85: an input of a wallet with one key has no signature by its own
    /// key.
    NoSignature {
        /// The input.
        input: usize,
    },
}

impl fmt::Display for FinalizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotVersionZero => {
                write!(f, "the file is not a PSBT version 0 of the right shape")
            }
            Self::MissingPreviousTransaction { input } => {
                write!(f, "input {input} has no previous transaction")
            }
            Self::PreviousTransactionMismatch { input } => write!(
                f,
                "input {input}: the previous transaction does not hash to the outpoint"
            ),
            Self::PreviousOutputMissing { input } => write!(
                f,
                "input {input}: the output spent is not in the previous transaction"
            ),
            Self::WitnessUtxoMismatch { input } => write!(
                f,
                "input {input}: the previous output differs from the previous transaction's"
            ),
            Self::NoWalletKeyHint { input } => write!(
                f,
                "SP-61: input {input} has no key-path hint that names a place in the wallet"
            ),
            Self::HintsInconsistent { input } => write!(
                f,
                "SP-61: the key-path hints of input {input} disagree or name the wrong key"
            ),
            Self::ScriptNotInWallet { input } => write!(
                f,
                "SP-61: the script spent by input {input} is not the wallet's script at its hints"
            ),
            Self::WitnessScriptMismatch { input } => write!(
                f,
                "SP-61: the witness script of input {input} is not the wallet's"
            ),
            Self::InputAlreadyFinal { input } => {
                write!(f, "input {input} is already final")
            }
            Self::SighashType { input } => {
                write!(f, "input {input} has a hash type other than SIGHASH_ALL")
            }
            Self::ForeignKey { input, key } => write!(
                f,
                "input {input} carries a signature by {key}, which is not a key of its script"
            ),
            Self::BadSignature { input, key } => {
                write!(f, "the signature by {key} on input {input} does not verify")
            }
            Self::NotEnoughSignatures { input, have } => write!(
                f,
                "SP-60: input {input} has {have} good signature(s) by different keys; it needs 2"
            ),
            Self::AmountOutOfRange => write!(f, "SP-62: an amount is out of range"),
            Self::OutputsExceedInputs => write!(f, "SP-62: the outputs are more than the inputs"),
            Self::TooHeavy { weight } => write!(
                f,
                "SP-63: the finished transaction weighs {weight} units; the limit is {MAX_WEIGHT}"
            ),
            Self::FinalCheckFailed(what) => {
                write!(
                    f,
                    "SP-62: the check of the finished transaction failed: {what}"
                )
            }
            Self::LockTimeNotHeight { lock_time } => write!(
                f,
                "SP-37: the lock time {lock_time} is not a block height (below 500,000,000)"
            ),
            Self::NotThePlan => write!(f, "SP-64: the file's transaction is not the plan's"),
            Self::FeeNotThePlan { found, planned } => write!(
                f,
                "SP-64: the fee is {found} sat; the plan's fee is {planned} sat"
            ),
            Self::WrongKind { input, expected } => write!(
                f,
                "SP-80: input {input} is not of the kind this path takes; it expects {expected}"
            ),
            Self::NoSignature { input } => {
                write!(f, "SP-85: input {input} has no signature by its own key")
            }
        }
    }
}

impl std::error::Error for FinalizeError {}

/// What is known about an input once its wallet place is settled.
struct InputFacts {
    amount: u64,
    spend: Spend,
}

/// What is spent, by kind of wallet.
enum Spend {
    /// A P2WSH 2-of-3 output: the wallet's witness script and its keys.
    Multisig {
        witness_script: bitcoin::ScriptBuf,
        keys: [PublicKey; 3],
    },
    /// A P2WPKH output: the script and the one key it pays to (SP-84).
    Single {
        script_pubkey: bitcoin::ScriptBuf,
        key: PublicKey,
    },
}

/// SP-10 and SP-61 for one input of `psbt`: the previous output, the place
/// in the wallet, and the wallet's own witness script and keys.
fn input_facts(wallet: WalletRef<'_>, psbt: &Psbt, i: usize) -> Result<InputFacts, FinalizeError> {
    let (txin, input) = psbt
        .unsigned_tx
        .input
        .get(i)
        .zip(psbt.inputs.get(i))
        .ok_or(FinalizeError::NotVersionZero)?;
    let prev = input
        .non_witness_utxo
        .as_ref()
        .ok_or(FinalizeError::MissingPreviousTransaction { input: i })?;
    if prev.compute_txid() != txin.previous_output.txid {
        return Err(FinalizeError::PreviousTransactionMismatch { input: i });
    }
    let spent = usize::try_from(txin.previous_output.vout)
        .ok()
        .and_then(|v| prev.output.get(v))
        .ok_or(FinalizeError::PreviousOutputMissing { input: i })?;
    if input.witness_utxo.as_ref().is_some_and(|w| w != spent) {
        return Err(FinalizeError::WitnessUtxoMismatch { input: i });
    }
    let amount = spent.value.to_sat();
    if amount > MAX_MONEY {
        return Err(FinalizeError::AmountOutOfRange);
    }
    if wallet.is_other_kind_input(&spent.script_pubkey, input.witness_script.is_some()) {
        return Err(FinalizeError::WrongKind {
            input: i,
            expected: wallet.kind(),
        });
    }
    if let Some(single) = wallet.single() {
        return single_facts(wallet, single, i, input, spent, amount);
    }
    let Some(wallet) = wallet.multisig() else {
        return Err(FinalizeError::FinalCheckFailed("a wallet of neither kind"));
    };
    let (chain, index) = claimed_path(wallet, &input.bip32_derivation).map_err(|f| match f {
        HintFault::None => FinalizeError::NoWalletKeyHint { input: i },
        HintFault::Disagree | HintFault::KeyMismatch => {
            FinalizeError::HintsInconsistent { input: i }
        }
    })?;
    let script = wallet
        .script(chain, index)
        .map_err(|_| FinalizeError::ScriptNotInWallet { input: i })?;
    if script != spent.script_pubkey {
        return Err(FinalizeError::ScriptNotInWallet { input: i });
    }
    let witness_script = wallet
        .witness_script(chain, index)
        .map_err(|_| FinalizeError::ScriptNotInWallet { input: i })?;
    if input
        .witness_script
        .as_ref()
        .is_some_and(|w| *w != witness_script)
    {
        return Err(FinalizeError::WitnessScriptMismatch { input: i });
    }
    let keys =
        script_keys(&witness_script).ok_or(FinalizeError::WitnessScriptMismatch { input: i })?;
    Ok(InputFacts {
        amount,
        spend: Spend::Multisig {
            witness_script,
            keys,
        },
    })
}

/// SP-84, SP-85 for one input of a wallet with one key: the hint names a
/// place in the wallet, the key the wallet derives there hashes to the
/// script spent, and nothing but the hint is read.
fn single_facts(
    wallet: WalletRef<'_>,
    single: &openbmt_core::wallet_single::SingleKeyWallet,
    i: usize,
    input: &Input,
    spent: &bitcoin::TxOut,
    amount: u64,
) -> Result<InputFacts, FinalizeError> {
    let (chain, index) =
        single_claimed_path(single, &input.bip32_derivation).map_err(|f| match f {
            SingleHintFault::None => FinalizeError::NoWalletKeyHint { input: i },
            SingleHintFault::KeyMismatch => FinalizeError::HintsInconsistent { input: i },
        })?;
    let script = wallet
        .script(chain, index)
        .ok_or(FinalizeError::ScriptNotInWallet { input: i })?;
    if script != spent.script_pubkey {
        return Err(FinalizeError::ScriptNotInWallet { input: i });
    }
    let key =
        single_key(single, chain, index).ok_or(FinalizeError::ScriptNotInWallet { input: i })?;
    Ok(InputFacts {
        amount,
        spend: Spend::Single {
            script_pubkey: script,
            key,
        },
    })
}

/// SP-60, SP-85: the witness of input `i`: for a wallet of three keys, from
/// two valid signatures by different keys, in the order of the keys in the
/// script; for a wallet with one key, the one signature by its own key and
/// then the key.
fn witness_for(
    psbt: &Psbt,
    cache: &mut SighashCache<&Transaction>,
    i: usize,
    facts: &InputFacts,
) -> Result<Witness, FinalizeError> {
    let input = psbt.inputs.get(i).ok_or(FinalizeError::NotVersionZero)?;
    if input.final_script_sig.is_some() || input.final_script_witness.is_some() {
        return Err(FinalizeError::InputAlreadyFinal { input: i });
    }
    if input
        .sighash_type
        .is_some_and(|t| t.to_u32() != EcdsaSighashType::All as u32)
    {
        return Err(FinalizeError::SighashType { input: i });
    }
    let keys: &[PublicKey] = match &facts.spend {
        Spend::Multisig { keys, .. } => keys,
        Spend::Single { key, .. } => std::slice::from_ref(key),
    };
    for key in input.partial_sigs.keys() {
        if !keys.contains(key) {
            return Err(FinalizeError::ForeignKey {
                input: i,
                key: *key,
            });
        }
    }
    let message = match &facts.spend {
        Spend::Multisig { witness_script, .. } => {
            input_message(cache, i, witness_script, facts.amount)
        }
        Spend::Single { script_pubkey, .. } => {
            single_input_message(cache, i, script_pubkey, facts.amount)
        }
    }
    .ok_or(FinalizeError::FinalCheckFailed("signature hash"))?;
    let mut chosen: Vec<Vec<u8>> = Vec::with_capacity(2);
    // Script order, not file order: OP_CHECKMULTISIG wants the signatures
    // in the order of their keys.
    for key in keys {
        let Some(sig) = input.partial_sigs.get(key) else {
            continue;
        };
        if sig.sighash_type != EcdsaSighashType::All {
            return Err(FinalizeError::SighashType { input: i });
        }
        if secp()
            .verify_ecdsa(&message, &sig.signature, &key.inner)
            .is_err()
        {
            return Err(FinalizeError::BadSignature {
                input: i,
                key: *key,
            });
        }
        if chosen.len() < 2 {
            chosen.push(sig.to_vec());
        }
    }
    let mut witness = Witness::new();
    match &facts.spend {
        Spend::Multisig { witness_script, .. } => {
            let [first, second] = chosen.as_slice() else {
                return Err(FinalizeError::NotEnoughSignatures {
                    input: i,
                    have: chosen.len(),
                });
            };
            witness.push([] as [u8; 0]);
            witness.push(first);
            witness.push(second);
            witness.push(witness_script.as_bytes());
        }
        Spend::Single { key, .. } => {
            let [sig] = chosen.as_slice() else {
                return Err(FinalizeError::NoSignature { input: i });
            };
            witness.push(sig);
            witness.push(key.inner.serialize());
        }
    }
    Ok(witness)
}

/// SP-62: check the finished transaction from scratch. Nothing is taken
/// from the steps that built it: the witnesses are read back out of `tx`,
/// and the signature hashes, scripts and amounts are worked out again from
/// the file's previous transactions and the wallet. Returns the fee.
pub(crate) fn audit(
    wallet: WalletRef<'_>,
    psbt: &Psbt,
    tx: &Transaction,
) -> Result<u64, FinalizeError> {
    let fail = FinalizeError::FinalCheckFailed;
    let unsigned = &psbt.unsigned_tx;
    // SP-37: any block height; a time is refused.
    if let LockTime::Seconds(t) = tx.lock_time {
        return Err(FinalizeError::LockTimeNotHeight {
            lock_time: t.to_consensus_u32(),
        });
    }
    // The transaction ID does not cover witnesses, so this is also the
    // check that nothing but witnesses changed.
    if tx.compute_txid() != unsigned.compute_txid() {
        return Err(fail("the transaction ID changed"));
    }
    if tx.version != unsigned.version
        || tx.lock_time != unsigned.lock_time
        || tx.output != unsigned.output
        || tx.input.len() != unsigned.input.len()
    {
        return Err(fail("the outputs, version or lock time changed"));
    }
    let mut cache = SighashCache::new(tx);
    let mut input_total: u64 = 0;
    for (i, txin) in tx.input.iter().enumerate() {
        let unsigned_in = unsigned.input.get(i).ok_or(fail("an input is missing"))?;
        if txin.previous_output != unsigned_in.previous_output
            || txin.sequence != unsigned_in.sequence
            || !txin.script_sig.is_empty()
        {
            return Err(fail("an input changed"));
        }
        let facts = input_facts(wallet, psbt, i)?;
        input_total = input_total
            .checked_add(facts.amount)
            .ok_or(FinalizeError::AmountOutOfRange)?;
        match &facts.spend {
            Spend::Multisig {
                witness_script,
                keys,
            } => {
                // The witness: an empty item, two signatures, the witness script.
                let items: Vec<&[u8]> = txin.witness.iter().collect();
                let [empty, first, second, script] = items.as_slice() else {
                    return Err(fail("a witness does not have four items"));
                };
                if !empty.is_empty() || *script != witness_script.as_bytes() {
                    return Err(fail("a witness does not have the shape of SP-60"));
                }
                let message = input_message(&mut cache, i, witness_script, facts.amount)
                    .ok_or(fail("signature hash"))?;
                let first = ecdsa::Signature::from_slice(first)
                    .map_err(|_| fail("a signature does not parse"))?;
                let second = ecdsa::Signature::from_slice(second)
                    .map_err(|_| fail("a signature does not parse"))?;
                if first.sighash_type != EcdsaSighashType::All
                    || second.sighash_type != EcdsaSighashType::All
                {
                    return Err(fail("a signature is not SIGHASH_ALL"));
                }
                // The way OP_CHECKMULTISIG matches: signatures and keys both in
                // script order, each signature to a later key than the last.
                let mut matched = 0usize;
                for key in keys {
                    let Some(sig) = [first, second].get(matched).copied() else {
                        break;
                    };
                    if secp()
                        .verify_ecdsa(&message, &sig.signature, &key.inner)
                        .is_ok()
                    {
                        matched += 1;
                    }
                }
                if matched != 2 {
                    return Err(fail(
                        "two signatures do not verify against two keys in order",
                    ));
                }
            }
            Spend::Single { script_pubkey, key } => {
                // SP-85: the witness is the signature, then the key.
                let items: Vec<&[u8]> = txin.witness.iter().collect();
                let [sig, witness_key] = items.as_slice() else {
                    return Err(fail("a witness does not have two items"));
                };
                if *witness_key != key.inner.serialize() {
                    return Err(fail("the witness key is not the input's own key"));
                }
                let message = single_input_message(&mut cache, i, script_pubkey, facts.amount)
                    .ok_or(fail("signature hash"))?;
                let sig = ecdsa::Signature::from_slice(sig)
                    .map_err(|_| fail("a signature does not parse"))?;
                if sig.sighash_type != EcdsaSighashType::All {
                    return Err(fail("a signature is not SIGHASH_ALL"));
                }
                if secp()
                    .verify_ecdsa(&message, &sig.signature, &key.inner)
                    .is_err()
                {
                    return Err(fail("the signature is not the input's own key's"));
                }
            }
        }
    }
    let mut output_total: u64 = 0;
    for out in &tx.output {
        let sats = out.value.to_sat();
        if sats > MAX_MONEY {
            return Err(FinalizeError::AmountOutOfRange);
        }
        output_total = output_total
            .checked_add(sats)
            .ok_or(FinalizeError::AmountOutOfRange)?;
    }
    let fee = input_total
        .checked_sub(output_total)
        .ok_or(FinalizeError::OutputsExceedInputs)?;
    let weight = tx.weight().to_wu();
    if weight > MAX_WEIGHT {
        return Err(FinalizeError::TooHeavy { weight });
    }
    Ok(fee)
}

fn finish(wallet: WalletRef<'_>, psbt: &Psbt) -> Result<(Transaction, u64), FinalizeError> {
    if !shape_is_v0(psbt) {
        return Err(FinalizeError::NotVersionZero);
    }
    if let LockTime::Seconds(t) = psbt.unsigned_tx.lock_time {
        return Err(FinalizeError::LockTimeNotHeight {
            lock_time: t.to_consensus_u32(),
        });
    }
    let mut tx = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&psbt.unsigned_tx);
    for i in 0..psbt.inputs.len() {
        let facts = input_facts(wallet, psbt, i)?;
        let witness = witness_for(psbt, &mut cache, i, &facts)?;
        let slot = tx.input.get_mut(i).ok_or(FinalizeError::NotVersionZero)?;
        slot.witness = witness;
    }
    let fee = audit(wallet, psbt, &tx)?;
    Ok((tx, fee))
}

/// SP-60 to SP-63: turn two signatures per input into a transaction.
///
/// The transaction is checked again from scratch before it is returned.
/// That check is this library's own, made with the `bitcoin` crate's
/// signature hash and secp256k1. It is not a consensus library; Bitcoin
/// Core's `testmempoolaccept` is the independent check.
///
/// The wallet is an `ImportedWallet` (SP-80), or a `WalletDescription`. For a
/// wallet with one key (SP-85) each input needs one valid signature by its
/// own key, and its witness is the signature and then the key.
pub fn finalize<'a>(
    wallet: impl Into<WalletRef<'a>>,
    psbt: &Psbt,
) -> Result<Transaction, FinalizeError> {
    finish(wallet.into(), psbt).map(|(tx, _)| tx)
}

/// SP-64: [`finalize`], and the file must also be the plan's: the same
/// unsigned transaction, and the plan's fee.
pub fn finalize_plan<'a>(
    wallet: impl Into<WalletRef<'a>>,
    psbt: &Psbt,
    plan: &Plan,
) -> Result<Transaction, FinalizeError> {
    if psbt.unsigned_tx != *plan.unsigned_tx() {
        return Err(FinalizeError::NotThePlan);
    }
    let (tx, fee) = finish(wallet.into(), psbt)?;
    if fee != plan.fee_sats() {
        return Err(FinalizeError::FeeNotThePlan {
            found: fee,
            planned: plan.fee_sats(),
        });
    }
    Ok(tx)
}
