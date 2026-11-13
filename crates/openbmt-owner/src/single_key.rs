//! The part the refund float (`FL-20`, `FL-21`) and the starter wallet
//! (`SS-30`, `SS-42`) share: signing the inputs of a plan with one BIP84
//! account key, and checking a finished transaction spends only the
//! wallet's coins and pays only the destination and the wallet's own
//! change. Each module keeps its own refusal and its own words for what
//! this reports.

use bitcoin::bip32::{ChildNumber, Xpriv};
use bitcoin::hashes::Hash;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{ecdsa, PublicKey, ScriptBuf, Transaction};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_spend::{Coin, Plan};

/// Sign every input of `planned` with the keys under `account`,
/// `SIGHASH_ALL`, P2WPKH. The error is the text of what went wrong; each
/// caller puts it in its own refusal. The derived key is erased on every
/// path.
pub(crate) fn sign_p2wpkh_inputs(
    account: &Xpriv,
    planned: &Plan,
    psbt: &mut Psbt,
) -> Result<(), String> {
    let secp = Secp256k1::new();
    let unsigned = psbt.unsigned_tx.clone();
    let mut cache = SighashCache::new(&unsigned);
    for (n, (entry, input)) in psbt.inputs.iter_mut().zip(planned.inputs()).enumerate() {
        let coin = &input.coin;
        let output = coin
            .output()
            .ok_or_else(|| "a coin has no output".to_string())?;
        let branch = match coin.chain {
            Keychain::Receive => 0,
            Keychain::Change => 1,
        };
        let mut child = account
            .derive_priv(
                &secp,
                &[
                    ChildNumber::Normal { index: branch },
                    ChildNumber::Normal { index: coin.index },
                ],
            )
            .map_err(|_| "the key cannot be derived".to_string())?;
        let hash = cache
            .p2wpkh_signature_hash(
                n,
                &output.script_pubkey,
                output.value,
                EcdsaSighashType::All,
            )
            .map_err(|e| e.to_string());
        let hash = match hash {
            Ok(h) => h,
            Err(e) => {
                child.private_key.non_secure_erase();
                return Err(e);
            }
        };
        let message = Message::from_digest(hash.to_byte_array());
        let signature = secp.sign_ecdsa(&message, &child.private_key);
        let key = PublicKey::new(child.private_key.public_key(&secp));
        child.private_key.non_secure_erase();
        entry.partial_sigs.insert(
            key,
            ecdsa::Signature {
                signature,
                sighash_type: EcdsaSighashType::All,
            },
        );
    }
    Ok(())
}

/// How a finished transaction broke the check.
pub(crate) enum Breach {
    /// It spends nothing.
    NoInputs,
    /// An input is not one of the coins.
    NotACoin,
    /// A sweep did not spend every coin.
    SweepMissesCoins,
    /// The destination is the wallet's own change.
    PaysOwnChange,
    /// The payment is not the set amount.
    WrongAmount,
    /// An output pays something other than the destination and the change.
    OtherOutput,
    /// The destination is not paid exactly once.
    NotPaidOnce,
    /// More than one change output.
    ManyChange,
    /// A coin does not belong to the wallet; the text is why.
    Coin(String),
}

/// The finished transaction `tx` spends only coins of the wallet (each of
/// `coins`, whose scripts are the wallet's) and pays only `pay_to` (with
/// `pay_sats` when it is a payment of a set amount) and the wallet's own
/// change script `change`. A sweep pays `pay_to` alone and spends every
/// coin it was given.
pub(crate) fn check_pays_only(
    wallet: &SingleKeyWallet,
    coins: &[&Coin],
    tx: &Transaction,
    pay_to: &ScriptBuf,
    pay_sats: Option<u64>,
    change: &ScriptBuf,
    sweep: bool,
) -> Result<(), Breach> {
    if tx.input.is_empty() {
        return Err(Breach::NoInputs);
    }
    let own = ImportedWallet::Single(wallet.clone());
    for input in &tx.input {
        let coin = coins
            .iter()
            .find(|c| c.outpoint == input.previous_output)
            .ok_or(Breach::NotACoin)?;
        coin.check(&own).map_err(|e| Breach::Coin(e.to_string()))?;
    }
    if sweep && coins.len() != tx.input.len() {
        return Err(Breach::SweepMissesCoins);
    }
    if pay_to == change {
        return Err(Breach::PaysOwnChange);
    }
    let mut paid = 0usize;
    let mut changes = 0usize;
    for output in &tx.output {
        if output.script_pubkey == *pay_to {
            paid += 1;
            if let Some(sats) = pay_sats {
                if output.value.to_sat() != sats {
                    return Err(Breach::WrongAmount);
                }
            }
        } else if output.script_pubkey == *change && !sweep {
            changes += 1;
        } else {
            return Err(Breach::OtherOutput);
        }
    }
    if paid != 1 {
        return Err(Breach::NotPaidOnce);
    }
    if changes > 1 {
        return Err(Breach::ManyChange);
    }
    Ok(())
}
