//! Reading a transaction file (sections 2 to 4 of the rules).
//!
//! Everything here needs no key and no clock: the checks depend only on the
//! file and the registered wallet.

use std::collections::BTreeMap;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint};
use bitcoin::ecdsa;
use bitcoin::hashes::Hash;
use bitcoin::psbt::{Input, Output, Psbt};
use bitcoin::secp256k1::{self, Message};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::transaction::Version;
use bitcoin::{Amount, OutPoint, PublicKey, Transaction, Txid, Witness};
use openbmt_core::wallet::{Keychain, WalletDescription};

use crate::crypto::secp;
use crate::refusal::Refusal;
use crate::state::SignerState;
use crate::{
    MAX_INPUTS, MAX_MONEY_SATS, MAX_OUTPUTS, MAX_PSBT_BYTES, SIGNATURE_BYTES, WITNESS_SCRIPT_BYTES,
};

/// The lowest sequence number that carries no relative time lock.
const LOWEST_FINAL_SEQUENCE: u32 = 0xFFFF_FFFD;

/// A transaction file that passed every check that needs no key.
#[derive(Debug)]
pub(crate) struct Parsed {
    pub txid: Txid,
    pub tx: Transaction,
    /// The full previous transaction of each input, as verified by SR-10.
    pub prevs: Vec<Transaction>,
    /// The amount each input spends, from the previous transaction.
    pub amounts: Vec<u64>,
    /// The keychain and index of each input.
    pub paths: Vec<(Keychain, u32)>,
    /// The signatures already in the file that passed SR-14.
    pub sigs: Vec<Vec<(PublicKey, ecdsa::Signature)>>,
    /// For each output, the change index if the output qualifies under
    /// SR-20 apart from the change mark.
    pub change_candidates: Vec<Option<u32>>,
    pub input_total: u64,
    pub output_total: u64,
    pub wallet_checksum: [u8; 32],
}

/// The weight of `tx` once every input carries two signatures of the
/// largest size and the witness script (section 7).
pub(crate) fn signed_weight(tx: &Transaction) -> u64 {
    let mut signed = tx.clone();
    let sig = [0u8; SIGNATURE_BYTES];
    let script = [0u8; WITNESS_SCRIPT_BYTES];
    for input in &mut signed.input {
        input.witness = Witness::from_slice(&[&[][..], &sig[..], &sig[..], &script[..]]);
    }
    signed.weight().to_wu()
}

/// The BIP143 hash a signature of input `index` is made over, from the
/// signer's own witness script and the amount from the previous
/// transaction (SR-53).
pub(crate) fn input_message(
    cache: &mut SighashCache<&Transaction>,
    index: usize,
    witness_script: &bitcoin::Script,
    amount: u64,
) -> Result<Message, Refusal> {
    let hash = cache
        .p2wsh_signature_hash(
            index,
            witness_script,
            Amount::from_sat(amount),
            EcdsaSighashType::All,
        )
        .map_err(|_| Refusal::Internal("signature hash"))?;
    Ok(Message::from_digest(hash.to_byte_array()))
}

/// A counted hint (SR-12): the slot (0, 1, 2), keychain and index it names.
struct CountedHint {
    slot: usize,
    chain: Keychain,
    index: u32,
}

/// Whether a key-path hint counts: its fingerprint is a slot's, and its
/// path is that slot's origin path followed by two unhardened steps, the
/// keychain (0 or 1) and the index.
fn counted_hint(
    wallet: &WalletDescription,
    fingerprint: &Fingerprint,
    path: &DerivationPath,
) -> Option<CountedHint> {
    let path: &[ChildNumber] = path.as_ref();
    for (slot, key) in wallet.slots().iter().enumerate() {
        if key.origin_fingerprint != *fingerprint {
            continue;
        }
        let origin: &[ChildNumber] = key.origin_path.as_ref();
        if path.len() != origin.len() + 2 || path[..origin.len()] != *origin {
            continue;
        }
        let (ChildNumber::Normal { index: chain }, ChildNumber::Normal { index }) =
            (path[origin.len()], path[origin.len() + 1])
        else {
            continue;
        };
        let chain = match chain {
            0 => Keychain::Receive,
            1 => Keychain::Change,
            _ => continue,
        };
        return Some(CountedHint { slot, chain, index });
    }
    None
}

enum HintFault {
    Disagree,
    KeyMismatch,
}

/// The keychain and index that the counted hints of one input or output
/// claim. `None` if no hint counts. All counted hints must agree, and each
/// must name the key the signer derives for its slot.
fn claimed_path(
    wallet: &WalletDescription,
    hints: &BTreeMap<secp256k1::PublicKey, (Fingerprint, DerivationPath)>,
) -> Result<Option<(Keychain, u32)>, HintFault> {
    let mut counted: Vec<(usize, secp256k1::PublicKey)> = Vec::new();
    let mut claim: Option<(Keychain, u32)> = None;
    for (key, (fingerprint, path)) in hints {
        let Some(hint) = counted_hint(wallet, fingerprint, path) else {
            continue;
        };
        match claim {
            None => claim = Some((hint.chain, hint.index)),
            Some(c) if c != (hint.chain, hint.index) => return Err(HintFault::Disagree),
            Some(_) => {}
        }
        counted.push((hint.slot, *key));
    }
    let Some((chain, index)) = claim else {
        return Ok(None);
    };
    let derived = wallet
        .derived_keys(chain, index)
        .map_err(|_| HintFault::KeyMismatch)?;
    if counted
        .iter()
        .any(|(slot, key)| derived[*slot].inner != *key)
    {
        return Err(HintFault::KeyMismatch);
    }
    Ok(Some((chain, index)))
}

fn check_input_fields(i: usize, input: &Input) -> Result<(), Refusal> {
    if input.final_script_sig.is_some() || input.final_script_witness.is_some() {
        return Err(Refusal::InputAlreadyFinal { input: i });
    }
    let forbidden = [
        (input.redeem_script.is_some(), "a redeem script"),
        (input.tap_key_sig.is_some(), "a Taproot key signature"),
        (
            !input.tap_script_sigs.is_empty(),
            "Taproot script signatures",
        ),
        (!input.tap_scripts.is_empty(), "Taproot scripts"),
        (!input.tap_key_origins.is_empty(), "Taproot key origins"),
        (input.tap_internal_key.is_some(), "a Taproot internal key"),
        (input.tap_merkle_root.is_some(), "a Taproot Merkle root"),
        (
            !input.ripemd160_preimages.is_empty(),
            "a RIPEMD-160 preimage",
        ),
        (!input.sha256_preimages.is_empty(), "a SHA-256 preimage"),
        (!input.hash160_preimages.is_empty(), "a HASH160 preimage"),
        (!input.hash256_preimages.is_empty(), "a HASH256 preimage"),
    ];
    if let Some((_, field)) = forbidden.iter().find(|(present, _)| *present) {
        return Err(Refusal::ForbiddenInputField { input: i, field });
    }
    if let Some(t) = input.sighash_type {
        if t.to_u32() != EcdsaSighashType::All as u32 {
            return Err(Refusal::SighashType {
                input: i,
                found: t.to_u32(),
            });
        }
    }
    Ok(())
}

fn check_output_fields(j: usize, output: &Output) -> Result<(), Refusal> {
    let forbidden = [
        (output.redeem_script.is_some(), "a redeem script"),
        (output.tap_internal_key.is_some(), "a Taproot internal key"),
        (output.tap_tree.is_some(), "a Taproot tree"),
        (!output.tap_key_origins.is_empty(), "Taproot key origins"),
    ];
    if let Some((_, field)) = forbidden.iter().find(|(present, _)| *present) {
        return Err(Refusal::ForbiddenOutputField { output: j, field });
    }
    Ok(())
}

/// A BIP174 compact size at `*at`, which moves past it.
fn compact_size(bytes: &[u8], at: &mut usize) -> Option<usize> {
    let first = *bytes.get(*at)?;
    *at += 1;
    let width = match first {
        0xfd => 2,
        0xfe => 4,
        0xff => 8,
        small => return Some(usize::from(small)),
    };
    let raw = bytes.get(*at..at.checked_add(width)?)?;
    *at += width;
    let value = raw.iter().rev().fold(0u64, |v, b| (v << 8) | u64::from(*b));
    usize::try_from(value).ok()
}

/// Where a PSBT of `maps` key-value maps ends in `bytes`: after the five
/// magic bytes, each map is a run of (key length, key, value length, value)
/// closed by a zero key length. `None` if the bytes end first.
fn psbt_len(bytes: &[u8], maps: usize) -> Option<usize> {
    let mut at = 5usize;
    for _ in 0..maps {
        loop {
            let key_len = compact_size(bytes, &mut at)?;
            if key_len == 0 {
                break;
            }
            at = at.checked_add(key_len)?;
            let value_len = compact_size(bytes, &mut at)?;
            at = at.checked_add(value_len)?;
            if at > bytes.len() {
                return None;
            }
        }
    }
    Some(at)
}

/// Parse `bytes` and run every check that needs no key: SR-01 to SR-15
/// and SR-20 to SR-24 (all but the parts that depend on the allowlist, the
/// change mark and the time).
pub(crate) fn analyze(state: &SignerState, bytes: &[u8]) -> Result<Parsed, Refusal> {
    if bytes.len() > MAX_PSBT_BYTES {
        return Err(Refusal::FileTooLarge { len: bytes.len() });
    }
    let psbt = Psbt::deserialize(bytes).map_err(|_| Refusal::NotAPsbt)?;
    if psbt.version != 0 {
        return Err(Refusal::NotAPsbt);
    }
    // The parser stops after the last map and does not look further. The
    // file must end there too.
    let maps = 1 + psbt.inputs.len() + psbt.outputs.len();
    if psbt_len(bytes, maps) != Some(bytes.len()) {
        return Err(Refusal::NotAPsbt);
    }
    let wallet = &state.wallet;
    let tx = &psbt.unsigned_tx;
    if tx.input.is_empty() || tx.input.len() > MAX_INPUTS || psbt.inputs.len() != tx.input.len() {
        return Err(Refusal::InputCount {
            found: tx.input.len(),
        });
    }
    if tx.output.is_empty()
        || tx.output.len() > MAX_OUTPUTS
        || psbt.outputs.len() != tx.output.len()
    {
        return Err(Refusal::OutputCount {
            found: tx.output.len(),
        });
    }

    // SR-03
    if tx.version != Version::TWO {
        return Err(Refusal::TransactionVersion {
            found: tx.version.0,
        });
    }
    if let LockTime::Seconds(t) = tx.lock_time {
        return Err(Refusal::LockTimeNotHeight {
            lock_time: t.to_consensus_u32(),
        });
    }
    for (i, txin) in tx.input.iter().enumerate() {
        if txin.sequence.0 < LOWEST_FINAL_SEQUENCE {
            return Err(Refusal::RelativeTimeLock {
                input: i,
                sequence: txin.sequence.0,
            });
        }
    }
    // SR-04, SR-05, SR-06, SR-07
    let mut seen: std::collections::HashSet<OutPoint> = std::collections::HashSet::new();
    for (i, txin) in tx.input.iter().enumerate() {
        if !seen.insert(txin.previous_output) {
            return Err(Refusal::DuplicateInput { input: i });
        }
    }
    for (i, input) in psbt.inputs.iter().enumerate() {
        check_input_fields(i, input)?;
    }
    for (j, output) in psbt.outputs.iter().enumerate() {
        check_output_fields(j, output)?;
    }
    // SR-08
    for (xpub, (fingerprint, path)) in &psbt.xpub {
        let known = wallet.slots().iter().any(|s| {
            s.xpub == *xpub && s.origin_fingerprint == *fingerprint && s.origin_path == *path
        });
        if !known {
            return Err(Refusal::ForeignXpub);
        }
    }

    // Section 3: inputs.
    let secp = secp();
    let mut cache = SighashCache::new(tx);
    let own_index = usize::from(state.own_slot) - 1;
    let mut prevs = Vec::with_capacity(tx.input.len());
    let mut amounts = Vec::with_capacity(tx.input.len());
    let mut paths = Vec::with_capacity(tx.input.len());
    let mut all_sigs = Vec::with_capacity(tx.input.len());
    let mut input_total: u64 = 0;
    let mut own_signed = 0usize;
    for (i, (txin, input)) in tx.input.iter().zip(&psbt.inputs).enumerate() {
        // SR-10
        let prev = input
            .non_witness_utxo
            .as_ref()
            .ok_or(Refusal::MissingPreviousTransaction { input: i })?;
        if prev.compute_txid() != txin.previous_output.txid {
            return Err(Refusal::PreviousTransactionMismatch { input: i });
        }
        let spent = usize::try_from(txin.previous_output.vout)
            .ok()
            .and_then(|v| prev.output.get(v))
            .ok_or(Refusal::PreviousOutputMissing {
                input: i,
                index: txin.previous_output.vout,
            })?;
        // SR-11
        if input.witness_utxo.as_ref().is_some_and(|w| w != spent) {
            return Err(Refusal::WitnessUtxoMismatch { input: i });
        }
        // SR-15
        let amount = spent.value.to_sat();
        if amount > MAX_MONEY_SATS {
            return Err(Refusal::InputAmountTooLarge { input: i, amount });
        }
        input_total = input_total
            .checked_add(amount)
            .ok_or(Refusal::InputTotalOverflow)?;
        // SR-12
        let (chain, index) = match claimed_path(wallet, &input.bip32_derivation) {
            Ok(Some(p)) => p,
            Ok(None) => return Err(Refusal::NoWalletKeyHint { input: i }),
            Err(HintFault::Disagree) => return Err(Refusal::HintsDisagree { input: i }),
            Err(HintFault::KeyMismatch) => return Err(Refusal::HintKeyMismatch { input: i }),
        };
        let script = wallet
            .script(chain, index)
            .map_err(|_| Refusal::ScriptNotInWallet { input: i })?;
        if script != spent.script_pubkey {
            return Err(Refusal::ScriptNotInWallet { input: i });
        }
        // SR-13
        let witness_script = wallet
            .witness_script(chain, index)
            .map_err(|_| Refusal::ScriptNotInWallet { input: i })?;
        if input
            .witness_script
            .as_ref()
            .is_some_and(|w| *w != witness_script)
        {
            return Err(Refusal::WitnessScriptMismatch { input: i });
        }
        // SR-14
        let keys = wallet
            .derived_keys(chain, index)
            .map_err(|_| Refusal::ScriptNotInWallet { input: i })?;
        let mut sigs = Vec::new();
        if !input.partial_sigs.is_empty() {
            let message = input_message(&mut cache, i, &witness_script, amount)?;
            for (key, sig) in &input.partial_sigs {
                if !keys.contains(key) {
                    return Err(Refusal::ForeignSignature { input: i });
                }
                if sig.sighash_type != EcdsaSighashType::All {
                    return Err(Refusal::SignatureSighashType { input: i });
                }
                if secp
                    .verify_ecdsa(&message, &sig.signature, &key.inner)
                    .is_err()
                {
                    return Err(Refusal::BadSignature { input: i });
                }
                sigs.push((*key, *sig));
            }
        }
        if input.partial_sigs.contains_key(&keys[own_index]) {
            own_signed += 1;
        }
        prevs.push(prev.clone());
        amounts.push(amount);
        paths.push((chain, index));
        all_sigs.push(sigs);
    }
    if own_signed == tx.input.len() {
        return Err(Refusal::AlreadySigned);
    }

    // Section 4: outputs.
    let mut output_total: u64 = 0;
    let mut change_candidates = Vec::with_capacity(tx.output.len());
    for (j, (txout, output)) in tx.output.iter().zip(&psbt.outputs).enumerate() {
        let amount = txout.value.to_sat();
        if amount == 0 {
            return Err(Refusal::OutputAmountZero { output: j });
        }
        if amount > MAX_MONEY_SATS {
            return Err(Refusal::OutputAmountTooLarge { output: j, amount });
        }
        output_total = output_total
            .checked_add(amount)
            .ok_or(Refusal::OutputTotalOverflow)?;
        let claim = match claimed_path(wallet, &output.bip32_derivation) {
            Ok(c) => c,
            Err(HintFault::Disagree) => return Err(Refusal::OutputHintsDisagree { output: j }),
            Err(HintFault::KeyMismatch) => {
                return Err(Refusal::OutputHintKeyMismatch { output: j })
            }
        };
        // SR-20: the hint must name the change keychain and the script
        // derived there must be the output's own.
        let candidate = match claim {
            Some((Keychain::Change, index)) => wallet
                .script(Keychain::Change, index)
                .ok()
                .filter(|s| *s == txout.script_pubkey)
                .map(|_| index),
            _ => None,
        };
        change_candidates.push(candidate);
    }
    if output_total > input_total {
        return Err(Refusal::OutputsExceedInputs {
            outputs: output_total,
            inputs: input_total,
        });
    }

    Ok(Parsed {
        txid: tx.compute_txid(),
        tx: tx.clone(),
        prevs,
        amounts,
        paths,
        sigs: all_sigs,
        change_candidates,
        input_total,
        output_total,
        wallet_checksum: wallet.checksum32(),
    })
}
