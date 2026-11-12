//! Joining two signers' signatures (section 6 of the rules).

use std::fmt;

use bitcoin::ecdsa;
use bitcoin::psbt::{Input, Psbt};
use bitcoin::secp256k1::Message;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{PublicKey, ScriptBuf, Transaction};

use crate::common::{input_message, script_keys, secp, shape_is_v0, single_input_message};
use crate::kind::{single_claimed_path, single_key, WalletKind, WalletRef};

/// Which file a problem was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The base file.
    Base,
    /// The signed copy at this position (from 0).
    Copy(usize),
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Base => write!(f, "the base file"),
            Self::Copy(n) => write!(f, "signed copy {n}"),
        }
    }
}

/// Why signatures could not be joined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombineError {
    /// The file is not a PSBT version 0 with one entry per input and
    /// output.
    NotVersionZero {
        /// The file.
        source: Source,
    },
    /// SP-50: the signed copy holds another unsigned transaction.
    DifferentTransaction {
        /// The copy.
        copy: usize,
    },
    /// SP-50: an input of the signed copy holds other data than the base.
    DifferentInputData {
        /// The copy.
        copy: usize,
        /// The input.
        input: usize,
    },
    /// SP-50: an input already has a final script or witness.
    InputAlreadyFinal {
        /// The file.
        source: Source,
        /// The input.
        input: usize,
    },
    /// The base has no previous transaction for the input.
    MissingPreviousTransaction {
        /// The input.
        input: usize,
    },
    /// The base's previous transaction does not hash to the input's
    /// outpoint.
    PreviousTransactionMismatch {
        /// The input.
        input: usize,
    },
    /// The previous output of the input is not in its previous transaction.
    PreviousOutputMissing {
        /// The input.
        input: usize,
    },
    /// The base's `witness_utxo` is not the output in the previous
    /// transaction.
    WitnessUtxoMismatch {
        /// The input.
        input: usize,
    },
    /// The base has no witness script for the input, or it is not a 2-of-3
    /// script of three keys.
    WitnessScriptShape {
        /// The input.
        input: usize,
    },
    /// The witness script does not hash to the script being spent.
    WitnessScriptNotSpent {
        /// The input.
        input: usize,
    },
    /// SP-52: a hash type other than `SIGHASH_ALL`.
    SighashType {
        /// The file.
        source: Source,
        /// The input.
        input: usize,
    },
    /// SP-51: a signature for a key that is not in the input's script.
    ForeignKey {
        /// The file.
        source: Source,
        /// The input.
        input: usize,
        /// The key.
        key: PublicKey,
    },
    /// SP-53: a signature that does not verify.
    BadSignature {
        /// The file.
        source: Source,
        /// The input.
        input: usize,
        /// The key.
        key: PublicKey,
    },
    /// SP-80: the input is of the other kind of wallet.
    WrongKind {
        /// The input.
        input: usize,
        /// The kind of wallet the path expected.
        expected: WalletKind,
    },
    /// SP-84: the input's key-path hint does not name the wallet's one key,
    /// or that key does not hash to the script being spent.
    KeyNotSpent {
        /// The input.
        input: usize,
    },
    /// A check the library makes of its own work failed.
    Internal(&'static str),
}

impl fmt::Display for CombineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotVersionZero { source } => {
                write!(f, "{source} is not a PSBT version 0 of the right shape")
            }
            Self::DifferentTransaction { copy } => write!(
                f,
                "SP-50: signed copy {copy} holds another unsigned transaction than the base"
            ),
            Self::DifferentInputData { copy, input } => write!(
                f,
                "SP-50: input {input} of signed copy {copy} holds other data than the base"
            ),
            Self::InputAlreadyFinal { source, input } => {
                write!(f, "SP-50: input {input} of {source} is already final")
            }
            Self::MissingPreviousTransaction { input } => {
                write!(f, "input {input} of the base has no previous transaction")
            }
            Self::PreviousTransactionMismatch { input } => write!(
                f,
                "input {input} of the base: the previous transaction does not hash to the outpoint"
            ),
            Self::PreviousOutputMissing { input } => write!(
                f,
                "input {input} of the base: the output spent is not in the previous transaction"
            ),
            Self::WitnessUtxoMismatch { input } => write!(
                f,
                "input {input} of the base: the previous output differs from the previous \
                 transaction's"
            ),
            Self::WitnessScriptShape { input } => write!(
                f,
                "input {input} of the base has no witness script of three keys, 2 of 3"
            ),
            Self::WitnessScriptNotSpent { input } => write!(
                f,
                "input {input} of the base: the witness script does not hash to the script spent"
            ),
            Self::SighashType { source, input } => write!(
                f,
                "SP-52: input {input} of {source} has a hash type other than SIGHASH_ALL"
            ),
            Self::ForeignKey { source, input, key } => write!(
                f,
                "SP-51: input {input} of {source} carries a signature by {key}, which is not a \
                 key of its script"
            ),
            Self::BadSignature { source, input, key } => write!(
                f,
                "SP-53: the signature by {key} on input {input} of {source} does not verify"
            ),
            Self::WrongKind { input, expected } => write!(
                f,
                "SP-80: input {input} of the base is not of the kind this path takes; it \
                 expects {expected}"
            ),
            Self::KeyNotSpent { input } => write!(
                f,
                "SP-84: input {input} of the base: the key-path hint does not name the \
                 wallet's key for the script spent"
            ),
            Self::Internal(what) => write!(f, "internal check failed: {what}"),
        }
    }
}

impl std::error::Error for CombineError {}

/// What the base says about one input: the message signatures are made
/// over, the keys that may sign, and the data copies must repeat.
struct Context {
    message: Message,
    keys: Vec<PublicKey>,
}

fn check_final(source: Source, i: usize, input: &Input) -> Result<(), CombineError> {
    if input.final_script_sig.is_some() || input.final_script_witness.is_some() {
        return Err(CombineError::InputAlreadyFinal { source, input: i });
    }
    // SP-52: the input's own hash-type field.
    if input
        .sighash_type
        .is_some_and(|t| t.to_u32() != EcdsaSighashType::All as u32)
    {
        return Err(CombineError::SighashType { source, input: i });
    }
    Ok(())
}

/// Whether two previous transactions are the same one. They are compared by
/// transaction ID: the ID covers every output and every other field except
/// the witnesses, and Bitcoin Core writes previous transactions into a PSBT
/// without their witnesses.
fn same_previous(a: &Option<Transaction>, b: &Option<Transaction>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.compute_txid() == y.compute_txid(),
        _ => false,
    }
}

/// Check the signatures of `input` and add the good ones to `into`.
fn take_signatures(
    source: Source,
    i: usize,
    context: &Context,
    input: &Input,
    into: &mut Input,
) -> Result<(), CombineError> {
    for (key, sig) in &input.partial_sigs {
        if !context.keys.contains(key) {
            return Err(CombineError::ForeignKey {
                source,
                input: i,
                key: *key,
            });
        }
        if sig.sighash_type != EcdsaSighashType::All {
            return Err(CombineError::SighashType { source, input: i });
        }
        // `verify_ecdsa` accepts low-S signatures only.
        if secp()
            .verify_ecdsa(&context.message, &sig.signature, &key.inner)
            .is_err()
        {
            return Err(CombineError::BadSignature {
                source,
                input: i,
                key: *key,
            });
        }
        // Two different valid signatures by one key: keep the one with the
        // smaller bytes, so that the order of the copies does not matter.
        let taken = ecdsa::Signature {
            signature: sig.signature,
            sighash_type: EcdsaSighashType::All,
        };
        match into.partial_sigs.get(key) {
            Some(have)
                if have.signature.serialize_compact() <= taken.signature.serialize_compact() => {}
            _ => {
                into.partial_sigs.insert(*key, taken);
            }
        }
    }
    Ok(())
}

/// SP-50 to SP-54: join the signatures of `signed` into `base`.
///
/// The keys that may sign each input are the three keys of its witness
/// script in `base`, which has to hash to the script being spent. Every
/// signature, those in `base` included, is verified against the BIP 143 hash
/// of its input before it is taken; a bad one is an error that names the
/// key. The result is `base` with the good signatures added. It does not
/// depend on the order of `signed`.
pub fn combine(base: &Psbt, signed: &[Psbt]) -> Result<Psbt, CombineError> {
    combine_inner(None, base, signed)
}

/// SP-50 to SP-54, SP-84: [`combine`] for a wallet of either kind (SP-80).
///
/// For a wallet of three keys this is [`combine`], with a file of a wallet
/// with one key refused as the wrong kind. For a wallet with one key the
/// only key that may sign an input is the one the wallet derives at the
/// place the input's hint names, whose hash is the script being spent, and
/// each signature is checked against the BIP 143 hash with the pay-to-key-hash
/// script code.
pub fn combine_wallet<'a>(
    wallet: impl Into<WalletRef<'a>>,
    base: &Psbt,
    signed: &[Psbt],
) -> Result<Psbt, CombineError> {
    combine_inner(Some(wallet.into()), base, signed)
}

/// What the base says about one input of a wallet of three keys.
fn multisig_context(
    cache: &mut SighashCache<&Transaction>,
    wallet: Option<WalletRef<'_>>,
    i: usize,
    input: &Input,
    spent: &bitcoin::TxOut,
) -> Result<Context, CombineError> {
    if wallet.is_some_and(|w| w.is_other_kind_input(&spent.script_pubkey, false)) {
        return Err(CombineError::WrongKind {
            input: i,
            expected: WalletKind::Multisig,
        });
    }
    let witness_script = input
        .witness_script
        .as_ref()
        .ok_or(CombineError::WitnessScriptShape { input: i })?;
    let keys = script_keys(witness_script).ok_or(CombineError::WitnessScriptShape { input: i })?;
    if ScriptBuf::new_p2wsh(&witness_script.wscript_hash()) != spent.script_pubkey {
        return Err(CombineError::WitnessScriptNotSpent { input: i });
    }
    let message = input_message(cache, i, witness_script, spent.value.to_sat())
        .ok_or(CombineError::Internal("signature hash"))?;
    Ok(Context {
        message,
        keys: keys.to_vec(),
    })
}

/// SP-84: what the base says about one input of a wallet with one key.
fn single_context(
    cache: &mut SighashCache<&Transaction>,
    wallet: WalletRef<'_>,
    i: usize,
    input: &Input,
    spent: &bitcoin::TxOut,
) -> Result<Context, CombineError> {
    let Some(single) = wallet.single() else {
        return Err(CombineError::Internal("a wallet of neither kind"));
    };
    if wallet.is_other_kind_input(&spent.script_pubkey, input.witness_script.is_some()) {
        return Err(CombineError::WrongKind {
            input: i,
            expected: WalletKind::Single,
        });
    }
    let (chain, index) = single_claimed_path(single, &input.bip32_derivation)
        .map_err(|_| CombineError::KeyNotSpent { input: i })?;
    let key = single_key(single, chain, index).ok_or(CombineError::KeyNotSpent { input: i })?;
    let script = wallet
        .script(chain, index)
        .ok_or(CombineError::KeyNotSpent { input: i })?;
    if script != spent.script_pubkey {
        return Err(CombineError::KeyNotSpent { input: i });
    }
    let message = single_input_message(cache, i, &spent.script_pubkey, spent.value.to_sat())
        .ok_or(CombineError::Internal("signature hash"))?;
    Ok(Context {
        message,
        keys: vec![key],
    })
}

fn combine_inner(
    wallet: Option<WalletRef<'_>>,
    base: &Psbt,
    signed: &[Psbt],
) -> Result<Psbt, CombineError> {
    if !shape_is_v0(base) {
        return Err(CombineError::NotVersionZero {
            source: Source::Base,
        });
    }
    let tx = &base.unsigned_tx;
    let mut cache = SighashCache::new(tx);
    let mut contexts = Vec::with_capacity(base.inputs.len());
    for (i, (txin, input)) in tx.input.iter().zip(&base.inputs).enumerate() {
        check_final(Source::Base, i, input)?;
        let prev = input
            .non_witness_utxo
            .as_ref()
            .ok_or(CombineError::MissingPreviousTransaction { input: i })?;
        if prev.compute_txid() != txin.previous_output.txid {
            return Err(CombineError::PreviousTransactionMismatch { input: i });
        }
        let spent = usize::try_from(txin.previous_output.vout)
            .ok()
            .and_then(|v| prev.output.get(v))
            .ok_or(CombineError::PreviousOutputMissing { input: i })?;
        if input.witness_utxo.as_ref().is_some_and(|w| w != spent) {
            return Err(CombineError::WitnessUtxoMismatch { input: i });
        }
        let context = match wallet.filter(|w| w.kind() == WalletKind::Single) {
            Some(single) => single_context(&mut cache, single, i, input, spent)?,
            None => multisig_context(&mut cache, wallet, i, input, spent)?,
        };
        contexts.push(context);
    }

    let mut result = base.clone();
    for (i, (context, input)) in contexts.iter().zip(&base.inputs).enumerate() {
        let Some(into) = result.inputs.get_mut(i) else {
            return Err(CombineError::Internal("input count"));
        };
        // The base's own signatures are checked like the others. Start
        // from none so that a bad one is an error and not a survivor.
        into.partial_sigs.clear();
        take_signatures(Source::Base, i, context, input, into)?;
    }
    for (n, copy) in signed.iter().enumerate() {
        let source = Source::Copy(n);
        if !shape_is_v0(copy) {
            return Err(CombineError::NotVersionZero { source });
        }
        if copy.unsigned_tx != *tx {
            return Err(CombineError::DifferentTransaction { copy: n });
        }
        for (i, ((context, input), base_input)) in contexts
            .iter()
            .zip(&copy.inputs)
            .zip(&base.inputs)
            .enumerate()
        {
            check_final(source, i, input)?;
            if !same_previous(&input.non_witness_utxo, &base_input.non_witness_utxo)
                || input.witness_utxo != base_input.witness_utxo
                || input.witness_script != base_input.witness_script
            {
                return Err(CombineError::DifferentInputData { copy: n, input: i });
            }
            let Some(into) = result.inputs.get_mut(i) else {
                return Err(CombineError::Internal("input count"));
            };
            take_signatures(source, i, context, input, into)?;
        }
    }
    Ok(result)
}
