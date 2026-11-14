//! SK-53 to SK-56: judging, showing and signing a payment. The payment is a
//! PSBT (BIP 174, version 0) of a wallet with one key (BIP84, real bitcoin).
//! Nothing here reads a claim of the file as true: every key, script and
//! address is derived again from the 12 words.

use std::collections::HashMap;

use bitcoin::bip32::{ChildNumber, Xpriv, Xpub};
use bitcoin::ecdsa::Signature;
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{self, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{Address, CompressedPublicKey, Network, NetworkKind, PublicKey, ScriptBuf, TxOut};

/// SK-55: own outputs are looked for at the receive and the change chain up
/// to this index (exclusive). An output of the wallet at a higher index is
/// shown as sent away.
pub const OWN_INDEX_BOUND: u32 = 1000;

/// The most the stick takes of a payment file, inputs and outputs counted
/// (a file of the owner phone holds a handful).
pub const MAX_PARTS: usize = 500;

/// The most bytes of a payment file the stick parses. The same bound as
/// `MAX_PSBT_BYTES` of the signer (`openbmt-signer`), whose `analyze` also
/// takes the trailing-bytes check below.
pub const MAX_PSBT_BYTES: usize = 8 * 1024 * 1024;

/// The bytes of a signed P2WPKH input's witness at most: the item count, a
/// 72-byte signature with its hash type and a 33-byte key, each with its
/// length (1 + 1 + 72 + 1 + 33 = 108).
const WITNESS_BYTES: u64 = 108;

/// Why a payment is refused (SK-54, SK-53). An index is the input's or
/// output's number from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The bytes are not a PSBT version 0 of a shape the stick takes.
    NotAPsbt,
    /// The payment is not for real bitcoin.
    NotMainnet,
    /// An input lacks its previous output.
    NoPrevout(usize),
    /// An input's two previous outputs (full transaction and output alone)
    /// disagree, or the full transaction is not the one the input spends.
    PrevoutMismatch(usize),
    /// An input lacks its key path (none, or more than one).
    NoPath(usize),
    /// An input's key path is under another master fingerprint: the
    /// input is not of the wallet of these words.
    OtherWallet(usize),
    /// An input's key path is not `m/84'/0'/0'/0/i` or `m/84'/0'/0'/1/i`.
    PathNotWallet(usize),
    /// The key at the input's path is not the input's key.
    KeyMismatch(usize),
    /// An input asks for a signature other than `SIGHASH_ALL`.
    SighashNotAll(usize),
    /// The outputs pay more than the inputs hold.
    OutputsExceedInputs,
    /// The words or keys could not be worked out. Cannot happen for 12
    /// words of the list.
    Internal,
}

/// One output as the owner sees it before signing (SK-55).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewOutput {
    /// The address, or the script in hex when it has none.
    pub address: String,
    /// The amount in satoshis.
    pub sats: u64,
    /// True only when the stick derived this script itself from the words at
    /// `m/84'/0'/0'/{0,1}/i` with `i` below [`OWN_INDEX_BOUND`].
    pub own: bool,
}

/// What the owner is shown (SK-55).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// Every output, in the transaction's order.
    pub outputs: Vec<ReviewOutput>,
    /// The sum of the outputs that are not the wallet's own.
    pub sent_away: u64,
    /// Inputs minus outputs.
    pub fee: u64,
    /// The signed transaction's size in virtual bytes, estimated with a
    /// signature of 72 bytes on every input.
    pub vsize: u64,
}

impl Review {
    /// The fee per virtual byte, in hundredths of a satoshi.
    pub fn fee_rate_hundredths(&self) -> u64 {
        self.fee.saturating_mul(100) / self.vsize.max(1)
    }
}

/// An amount of satoshis as bitcoin, always eight decimals.
pub fn btc(sats: u64) -> String {
    format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000)
}

/// A rate in hundredths as `n.nn`.
pub fn hundredths(v: u64) -> String {
    format!("{}.{:02}", v / 100, v % 100)
}

/// A payment that passed every check of SK-54, with its review.
#[derive(Debug, Clone)]
pub struct Prepared {
    psbt: Psbt,
    /// For each input: the chain (0 or 1) and index of its key.
    places: Vec<(u32, u32)>,
    review: Review,
}

fn unhardened(n: &ChildNumber) -> Option<u32> {
    match n {
        ChildNumber::Normal { index } => Some(*index),
        ChildNumber::Hardened { .. } => None,
    }
}

fn hard(n: &ChildNumber) -> Option<u32> {
    match n {
        ChildNumber::Hardened { index } => Some(*index),
        ChildNumber::Normal { .. } => None,
    }
}

fn script_of(key: &secp256k1::PublicKey) -> ScriptBuf {
    ScriptBuf::new_p2wpkh(&CompressedPublicKey(*key).wpubkey_hash())
}

/// The previous output of input `i`, from the file, checked against itself.
fn prevout(psbt: &Psbt, i: usize) -> Result<TxOut, Refusal> {
    let txin = psbt.unsigned_tx.input.get(i).ok_or(Refusal::NotAPsbt)?;
    let input = psbt.inputs.get(i).ok_or(Refusal::NotAPsbt)?;
    let full = match &input.non_witness_utxo {
        Some(tx) => {
            if tx.compute_txid() != txin.previous_output.txid {
                return Err(Refusal::PrevoutMismatch(i));
            }
            let vout =
                usize::try_from(txin.previous_output.vout).map_err(|_| Refusal::NoPrevout(i))?;
            Some(tx.output.get(vout).ok_or(Refusal::NoPrevout(i))?.clone())
        }
        None => None,
    };
    match (full, &input.witness_utxo) {
        (Some(a), Some(b)) if a != *b => Err(Refusal::PrevoutMismatch(i)),
        (Some(a), _) => Ok(a),
        (None, Some(b)) => Ok(b.clone()),
        (None, None) => Err(Refusal::NoPrevout(i)),
    }
}

/// A BIP174 compact size at `*at`, which moves past it. As in the signer's
/// `analysis.rs`.
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

/// Where a PSBT of `maps` key-value maps ends in `bytes`; `None` if the
/// bytes end first. As `psbt_len` in the signer's `analysis.rs`: the parser
/// stops after the last map, so the file must end there too.
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

/// SK-53, SK-54, SK-55: judges the payment `bytes` for the wallet of
/// `master`. Nothing is shown before every check has passed.
pub fn prepare(bytes: &[u8], master: &Xpriv) -> Result<Prepared, Refusal> {
    let secp = Secp256k1::new();
    if bytes.len() > MAX_PSBT_BYTES {
        return Err(Refusal::NotAPsbt);
    }
    let psbt = Psbt::deserialize(bytes).map_err(|_| Refusal::NotAPsbt)?;
    let maps = 1 + psbt.inputs.len() + psbt.outputs.len();
    if psbt_len(bytes, maps) != Some(bytes.len()) {
        return Err(Refusal::NotAPsbt);
    }
    let n_in = psbt.unsigned_tx.input.len();
    let n_out = psbt.unsigned_tx.output.len();
    if psbt.version != 0
        || n_in == 0
        || n_out == 0
        || n_in + n_out > MAX_PARTS
        || psbt.inputs.len() != n_in
        || psbt.outputs.len() != n_out
    {
        return Err(Refusal::NotAPsbt);
    }
    // Real bitcoin only: no global key of another network, no key path of
    // the test coin type.
    let test_path = |p: &[ChildNumber]| p.get(1).and_then(hard) == Some(1);
    if psbt.xpub.keys().any(|x| x.network != NetworkKind::Main)
        || psbt.outputs.iter().any(|o| {
            o.bip32_derivation
                .values()
                .any(|(_, p)| test_path(p.as_ref()))
        })
        || psbt.inputs.iter().any(|i| {
            i.bip32_derivation
                .values()
                .any(|(_, p)| test_path(p.as_ref()))
        })
    {
        return Err(Refusal::NotMainnet);
    }
    let fingerprint = master.fingerprint(&secp);
    let account_path = [
        ChildNumber::Hardened { index: 84 },
        ChildNumber::Hardened { index: 0 },
        ChildNumber::Hardened { index: 0 },
    ];
    let account = master
        .derive_priv(&secp, &account_path)
        .map_err(|_| Refusal::Internal)?;
    let account_pub = Xpub::from_priv(&secp, &account);

    let mut places = Vec::with_capacity(n_in);
    let mut total_in: u64 = 0;
    for i in 0..n_in {
        let out = prevout(&psbt, i)?;
        let input = psbt.inputs.get(i).ok_or(Refusal::NotAPsbt)?;
        let mut hints = input.bip32_derivation.iter();
        let (Some((key, (fp, path))), None) = (hints.next(), hints.next()) else {
            return Err(Refusal::NoPath(i));
        };
        if *fp != fingerprint {
            return Err(Refusal::OtherWallet(i));
        }
        let steps: &[ChildNumber] = path.as_ref();
        let (Some(rest), true) = (steps.strip_prefix(&account_path), steps.len() == 5) else {
            return Err(Refusal::PathNotWallet(i));
        };
        let (Some(chain), Some(index)) = (
            rest.first().and_then(unhardened),
            rest.get(1).and_then(unhardened),
        ) else {
            return Err(Refusal::PathNotWallet(i));
        };
        if chain > 1 {
            return Err(Refusal::PathNotWallet(i));
        }
        let derived = account_pub
            .derive_pub(
                &secp,
                &[
                    ChildNumber::Normal { index: chain },
                    ChildNumber::Normal { index },
                ],
            )
            .map_err(|_| Refusal::Internal)?;
        if derived.public_key != *key || out.script_pubkey != script_of(key) {
            return Err(Refusal::KeyMismatch(i));
        }
        if let Some(t) = input.sighash_type {
            if t.ecdsa_hash_ty().ok() != Some(EcdsaSighashType::All) {
                return Err(Refusal::SighashNotAll(i));
            }
        }
        total_in = total_in
            .checked_add(out.value.to_sat())
            .ok_or(Refusal::OutputsExceedInputs)?;
        places.push((chain, index));
    }
    let mut total_out: u64 = 0;
    for o in &psbt.unsigned_tx.output {
        total_out = total_out
            .checked_add(o.value.to_sat())
            .ok_or(Refusal::OutputsExceedInputs)?;
    }
    if total_out > total_in {
        return Err(Refusal::OutputsExceedInputs);
    }

    // SK-55: the wallet's own scripts, derived here and nowhere else.
    let mut own: HashMap<ScriptBuf, ()> = HashMap::new();
    for chain in 0..2u32 {
        let chain_pub = account_pub
            .ckd_pub(&secp, ChildNumber::Normal { index: chain })
            .map_err(|_| Refusal::Internal)?;
        for index in 0..OWN_INDEX_BOUND {
            let k = chain_pub
                .ckd_pub(&secp, ChildNumber::Normal { index })
                .map_err(|_| Refusal::Internal)?;
            own.insert(script_of(&k.public_key), ());
        }
    }
    let mut outputs = Vec::with_capacity(n_out);
    let mut sent_away = 0u64;
    for o in &psbt.unsigned_tx.output {
        let is_own = own.contains_key(&o.script_pubkey);
        if !is_own {
            sent_away += o.value.to_sat();
        }
        let address = Address::from_script(&o.script_pubkey, Network::Bitcoin)
            .map(|a| a.to_string())
            .unwrap_or_else(|_| {
                o.script_pubkey
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect()
            });
        outputs.push(ReviewOutput {
            address,
            sats: o.value.to_sat(),
            own: is_own,
        });
    }
    let base = psbt.unsigned_tx.base_size() as u64;
    let weight = 4 * base + 2 + WITNESS_BYTES * n_in as u64;
    let review = Review {
        outputs,
        sent_away,
        fee: total_in - total_out,
        vsize: weight.div_ceil(4),
    };
    Ok(Prepared {
        psbt,
        places,
        review,
    })
}

impl Prepared {
    /// What the owner is shown (SK-55).
    pub fn review(&self) -> &Review {
        &self.review
    }

    /// SK-56: the file with this wallet's signature (ECDSA, `SIGHASH_ALL`,
    /// P2WPKH) added to every input, not finalized, nothing else changed.
    pub fn sign(&self, master: &Xpriv) -> Result<Vec<u8>, Refusal> {
        let secp = Secp256k1::new();
        let mut psbt = self.psbt.clone();
        let tx = psbt.unsigned_tx.clone();
        let mut cache = SighashCache::new(&tx);
        for (i, (chain, index)) in self.places.iter().enumerate() {
            let out = prevout(&psbt, i)?;
            let path = [
                ChildNumber::Hardened { index: 84 },
                ChildNumber::Hardened { index: 0 },
                ChildNumber::Hardened { index: 0 },
                ChildNumber::Normal { index: *chain },
                ChildNumber::Normal { index: *index },
            ];
            let mut child = master
                .derive_priv(&secp, &path)
                .map_err(|_| Refusal::Internal)?;
            let public = child.private_key.public_key(&secp);
            let hash = cache
                .p2wpkh_signature_hash(i, &out.script_pubkey, out.value, EcdsaSighashType::All)
                .map_err(|_| Refusal::Internal)?;
            let message = secp256k1::Message::from_digest(hash.to_byte_array());
            let signature = secp.sign_ecdsa(&message, &child.private_key);
            let checked = secp.verify_ecdsa(&message, &signature, &public);
            child.private_key.non_secure_erase();
            checked.map_err(|_| Refusal::Internal)?;
            let input = psbt.inputs.get_mut(i).ok_or(Refusal::Internal)?;
            input.partial_sigs.insert(
                PublicKey::new(public),
                Signature {
                    signature,
                    sighash_type: EcdsaSighashType::All,
                },
            );
        }
        Ok(psbt.serialize())
    }
}
