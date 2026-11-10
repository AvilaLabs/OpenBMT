//! Signing (section 8 of the rules).

use std::collections::BTreeMap;

use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::ecdsa;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1;
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::Txid;
use openbmt_core::wallet::{Keychain, WalletDescription};

use crate::analysis::input_message;
use crate::crypto::secp;
use crate::refusal::{Refusal, SignError};
use crate::review::Review;
use crate::state::SignerState;

/// A signed transaction file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    /// The PSBT the signer built (SR-09), with the signatures it checked
    /// and its own.
    pub psbt: Vec<u8>,
    /// The transaction ID.
    pub txid: Txid,
    /// How many valid wallet signatures each input now carries.
    pub signatures_per_input: Vec<usize>,
}

fn chain_number(chain: Keychain) -> ChildNumber {
    ChildNumber::Normal {
        index: match chain {
            Keychain::Receive => 0,
            Keychain::Change => 1,
        },
    }
}

/// The key-path hints for the three keys at `chain/index`: the derived
/// public key of each slot, with the slot's origin and the two steps.
fn key_hints(
    wallet: &WalletDescription,
    chain: Keychain,
    index: u32,
) -> Result<BTreeMap<secp256k1::PublicKey, (Fingerprint, DerivationPath)>, Refusal> {
    let keys = wallet
        .derived_keys(chain, index)
        .map_err(|_| Refusal::Internal("key derivation"))?;
    Ok(wallet
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
        .collect())
}

impl SignerState {
    /// Sign a reviewed transaction (SR-51 to SR-56).
    ///
    /// The kinds of the outputs, the change index and the ceilings are
    /// worked out again from the state and `now` as they are now. The new
    /// state is handed to `persist` first; the signed file is returned only
    /// if `persist` returns `Ok`. On any error the state is unchanged and no
    /// signature leaves this function.
    pub fn sign<E>(
        &mut self,
        review: Review,
        account_key: &Xpriv,
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<Signed, SignError<E>> {
        let secp = secp();
        let p = review.parsed();
        let own = usize::from(self.own_slot) - 1;
        if p.wallet_checksum != self.wallet.checksum32() {
            return Err(Refusal::ReviewForOtherWallet.into());
        }
        // SR-52: the whole account key, not only its fingerprint.
        if Xpub::from_priv(secp, account_key) != self.wallet.slots()[own].xpub {
            return Err(Refusal::WrongKey.into());
        }
        // SR-51
        let effective = self.effective_time(now);
        let classified = self.classify(p, effective)?;
        if let Some(output) = classified
            .kinds
            .iter()
            .zip(review.kinds())
            .position(|(now_kind, shown)| now_kind != shown)
        {
            return Err(Refusal::ReviewOutdated { output }.into());
        }
        let limits = self.limits(p, &classified, effective)?;

        // SR-53, SR-54, SR-56
        let mut cache = SighashCache::new(&p.tx);
        let mut own_sigs = Vec::with_capacity(p.tx.input.len());
        for i in 0..p.tx.input.len() {
            let (chain, index) = p.paths[i];
            let witness_script = self
                .wallet
                .witness_script(chain, index)
                .map_err(|_| Refusal::Internal("key derivation"))?;
            let public = self
                .wallet
                .derived_keys(chain, index)
                .map_err(|_| Refusal::Internal("key derivation"))?[own];
            let message = input_message(&mut cache, i, &witness_script, p.amounts[i])?;
            let mut child = account_key
                .derive_priv(secp, &[chain_number(chain), ChildNumber::Normal { index }])
                .map_err(|_| Refusal::Internal("key derivation"))?;
            // RFC 6979 nonce, low S: `sign_ecdsa` and nothing else.
            let signature = secp.sign_ecdsa(&message, &child.private_key);
            let checked = secp.verify_ecdsa(&message, &signature, &public.inner);
            // Best effort: the compiler and the operating system may keep
            // other copies, including the intermediate keys of the
            // derivation.
            child.private_key.non_secure_erase();
            checked.map_err(|_| Refusal::Internal("own signature did not verify"))?;
            own_sigs.push(ecdsa::Signature {
                signature,
                sighash_type: EcdsaSighashType::All,
            });
        }

        // SR-09: the file is built from scratch.
        let mut psbt = Psbt::from_unsigned_tx(p.tx.clone())
            .map_err(|_| Refusal::Internal("unsigned transaction"))?;
        let mut signatures_per_input = Vec::with_capacity(p.tx.input.len());
        for (i, own_sig) in own_sigs.into_iter().enumerate() {
            let (chain, index) = p.paths[i];
            let public = self
                .wallet
                .derived_keys(chain, index)
                .map_err(|_| Refusal::Internal("key derivation"))?[own];
            let vout = p.tx.input[i].previous_output.vout as usize;
            let input = &mut psbt.inputs[i];
            input.non_witness_utxo = Some(p.prevs[i].clone());
            input.witness_utxo = Some(p.prevs[i].output[vout].clone());
            input.witness_script = Some(
                self.wallet
                    .witness_script(chain, index)
                    .map_err(|_| Refusal::Internal("key derivation"))?,
            );
            input.bip32_derivation = key_hints(&self.wallet, chain, index)?;
            for (key, sig) in &p.sigs[i] {
                input.partial_sigs.insert(*key, *sig);
            }
            input.partial_sigs.insert(public, own_sig);
            signatures_per_input.push(input.partial_sigs.len());
        }
        if let Some((pos, index)) = classified.change {
            let output = &mut psbt.outputs[pos];
            output.witness_script = Some(
                self.wallet
                    .witness_script(Keychain::Change, index)
                    .map_err(|_| Refusal::Internal("key derivation"))?,
            );
            output.bip32_derivation = key_hints(&self.wallet, Keychain::Change, index)?;
        }

        // SR-55: store first, release second.
        let mut next = self.clone();
        next.latest_time_seen = effective;
        if let Some((_, index)) = classified.change {
            next.change_mark = next.change_mark.max(index + 1);
        }
        next.log = limits.next_log;
        persist(&next.to_bytes()).map_err(SignError::Persist)?;
        *self = next;
        Ok(Signed {
            psbt: psbt.serialize(),
            txid: p.txid,
            signatures_per_input,
        })
    }
}
