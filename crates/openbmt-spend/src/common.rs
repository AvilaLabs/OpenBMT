//! Pieces the planner, the combiner and the finisher share: the key-path
//! hints, the keys of a witness script, the signature hash and the weight.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint};
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{self, Message, Secp256k1, VerifyOnly};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{Amount, PublicKey, Script, Transaction, Witness};
use openbmt_core::wallet::{Keychain, WalletDescription};
use openbmt_signer::{SIGNATURE_BYTES, WITNESS_SCRIPT_BYTES};

/// The version every transaction here has (SP-35).
pub(crate) const TX_VERSION: i32 = 2;
/// The sequence of every input (SP-35): signals replacement, sets no
/// relative time lock.
pub(crate) const SEQUENCE: u32 = 0xFFFF_FFFD;
/// 21,000,000 BTC in satoshis (SR-15, SR-24).
pub(crate) const MAX_MONEY: u64 = openbmt_signer::MAX_MONEY_SATS;

/// A secp256k1 context that only verifies. This library never signs.
pub(crate) fn secp() -> &'static Secp256k1<VerifyOnly> {
    static CONTEXT: OnceLock<Secp256k1<VerifyOnly>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::verification_only)
}

pub(crate) fn chain_number(chain: Keychain) -> ChildNumber {
    ChildNumber::Normal {
        index: match chain {
            Keychain::Receive => 0,
            Keychain::Change => 1,
        },
    }
}

/// The key-path hints of the three keys at `chain/index`: each slot's
/// derived public key with the slot's origin and the two steps.
pub(crate) fn key_hints(
    wallet: &WalletDescription,
    chain: Keychain,
    index: u32,
) -> Option<BTreeMap<secp256k1::PublicKey, (Fingerprint, DerivationPath)>> {
    let keys = wallet.derived_keys(chain, index).ok()?;
    Some(
        wallet
            .slots()
            .iter()
            .zip(keys)
            .map(|(slot, key)| {
                let path = slot
                    .origin_path
                    .child(chain_number(chain))
                    .child(ChildNumber::Normal { index });
                (key.inner, (slot.origin_fingerprint, path))
            })
            .collect(),
    )
}

/// Why the hints of an input did not name one place in the wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HintFault {
    /// No hint names a slot of the wallet.
    None,
    /// Two hints name different places.
    Disagree,
    /// A hint's key is not the one the wallet derives for its slot.
    KeyMismatch,
}

/// The keychain and index the hints claim. A hint counts only if its
/// fingerprint and path are a slot's origin followed by the keychain (0 or
/// one) and an index, all unhardened; counted hints agree, and each names
/// the key the wallet derives for its slot. This is the reading that SR-12
/// asks of the signer.
pub(crate) fn claimed_path(
    wallet: &WalletDescription,
    hints: &BTreeMap<secp256k1::PublicKey, (Fingerprint, DerivationPath)>,
) -> Result<(Keychain, u32), HintFault> {
    let mut claim: Option<(Keychain, u32)> = None;
    let mut counted: Vec<(usize, secp256k1::PublicKey)> = Vec::new();
    for (key, (fingerprint, path)) in hints {
        let steps: &[ChildNumber] = path.as_ref();
        for (slot, s) in wallet.slots().iter().enumerate() {
            if s.origin_fingerprint != *fingerprint {
                continue;
            }
            let origin: &[ChildNumber] = s.origin_path.as_ref();
            let Some(tail) = steps.strip_prefix(origin) else {
                continue;
            };
            let [ChildNumber::Normal { index: chain }, ChildNumber::Normal { index }] = tail else {
                continue;
            };
            let chain = match chain {
                0 => Keychain::Receive,
                1 => Keychain::Change,
                _ => continue,
            };
            match claim {
                None => claim = Some((chain, *index)),
                Some(c) if c != (chain, *index) => return Err(HintFault::Disagree),
                Some(_) => {}
            }
            counted.push((slot, *key));
            break;
        }
    }
    let Some((chain, index)) = claim else {
        return Err(HintFault::None);
    };
    let derived = wallet
        .derived_keys(chain, index)
        .map_err(|_| HintFault::KeyMismatch)?;
    for (slot, key) in counted {
        if derived.get(slot).map(|k| k.inner) != Some(key) {
            return Err(HintFault::KeyMismatch);
        }
    }
    Ok((chain, index))
}

/// The three keys of a witness script of the exact shape
/// `OP_2 <key> <key> <key> OP_3 OP_CHECKMULTISIG`, in script order.
pub(crate) fn script_keys(script: &Script) -> Option<[PublicKey; 3]> {
    let bytes = script.as_bytes();
    if bytes.len() != WITNESS_SCRIPT_BYTES {
        return None;
    }
    let (first, rest) = bytes.split_first()?;
    if *first != 0x52 {
        return None;
    }
    let mut keys = Vec::with_capacity(3);
    let mut rest = rest;
    for _ in 0..3 {
        let (push, tail) = rest.split_first()?;
        if *push != 33 || tail.len() < 33 {
            return None;
        }
        let (key, tail) = tail.split_at(33);
        keys.push(PublicKey::from_slice(key).ok()?);
        rest = tail;
    }
    if rest != [0x53, 0xae] {
        return None;
    }
    keys.try_into().ok()
}

/// The BIP 143 hash `SIGHASH_ALL` signs for input `index` of the
/// transaction behind `cache`.
pub(crate) fn input_message(
    cache: &mut SighashCache<&Transaction>,
    index: usize,
    witness_script: &Script,
    amount: u64,
) -> Option<Message> {
    let hash = cache
        .p2wsh_signature_hash(
            index,
            witness_script,
            Amount::from_sat(amount),
            EcdsaSighashType::All,
        )
        .ok()?;
    Some(Message::from_digest(hash.to_byte_array()))
}

/// Bytes of a compact size.
pub(crate) fn varint_len(n: u64) -> u64 {
    match n {
        0..=0xfc => 1,
        0xfd..=0xffff => 3,
        0x1_0000..=0xffff_ffff => 5,
        _ => 9,
    }
}

/// Bytes of one input outside its witness: the outpoint, an empty script
/// and the sequence.
pub(crate) const INPUT_BASE_BYTES: u64 = 36 + 1 + 4;

/// Bytes of the witness of one input once signed: item count, an empty
/// item, two signatures of the longest size, the witness script, each item
/// with its length byte.
pub(crate) const INPUT_WITNESS_BYTES: u64 = 1 + 1 + 2 * (1 + SIGNATURE_BYTES as u64) + 1 + 105;

/// The weight of a transaction of `inputs` P2WSH 2-of-3 inputs, signed with
/// the longest signatures, and outputs with scripts of the given lengths
/// (SP-30). `None` on overflow.
pub(crate) fn shape_weight(inputs: u64, script_lens: &[usize]) -> Option<u64> {
    let outputs = u64::try_from(script_lens.len()).ok()?;
    let mut base: u64 = 4u64
        .checked_add(varint_len(inputs))?
        .checked_add(varint_len(outputs))?
        .checked_add(4)?;
    base = base.checked_add(inputs.checked_mul(INPUT_BASE_BYTES)?)?;
    for len in script_lens {
        let len = u64::try_from(*len).ok()?;
        base = base
            .checked_add(8)?
            .checked_add(varint_len(len))?
            .checked_add(len)?;
    }
    let witness = if inputs == 0 {
        0
    } else {
        inputs.checked_mul(INPUT_WITNESS_BYTES)?.checked_add(2)?
    };
    base.checked_mul(4)?.checked_add(witness)
}

/// The weight the `bitcoin` crate gives `tx` once every input carries the
/// witness shape of SP-30. The planner checks `shape_weight` against it.
pub(crate) fn template_weight(tx: &Transaction) -> u64 {
    let mut signed = tx.clone();
    let sig = [0u8; SIGNATURE_BYTES];
    let script = [0u8; WITNESS_SCRIPT_BYTES];
    for input in &mut signed.input {
        input.witness = Witness::from_slice(&[&[][..], &sig[..], &sig[..], &script[..]]);
    }
    signed.weight().to_wu()
}

/// Whether `psbt` has no PSBT version other than 0 and one entry for each
/// input and output of its transaction.
pub(crate) fn shape_is_v0(psbt: &Psbt) -> bool {
    // A transaction with no input or no output is never part of this wallet, and
    // every per-input check would pass over it without looking at anything.
    psbt.version == 0
        && !psbt.unsigned_tx.input.is_empty()
        && !psbt.unsigned_tx.output.is_empty()
        && psbt.inputs.len() == psbt.unsigned_tx.input.len()
        && psbt.outputs.len() == psbt.unsigned_tx.output.len()
}
