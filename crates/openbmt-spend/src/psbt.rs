//! Writing a plan as a PSBT (section 5 of the rules).

use std::fmt;

use bitcoin::psbt::{Psbt, PsbtSighashType};
use bitcoin::sighash::EcdsaSighashType;
use openbmt_core::wallet::Keychain;

use crate::coin::CoinError;
use crate::common::key_hints;
use crate::kind::{single_account_path, single_hint, WalletRef};
use crate::plan::Plan;

/// Why a plan could not be written as a PSBT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PsbtError {
    /// SP-02: a coin of the plan is not the wallet's.
    Coin(CoinError),
    /// The wallet could not derive a script or keys.
    Derivation,
    /// The transaction could not be made into a PSBT.
    Internal(&'static str),
}

impl fmt::Display for PsbtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Coin(e) => write!(f, "{e}"),
            Self::Derivation => write!(f, "SP-40: the wallet could not derive its keys"),
            Self::Internal(what) => write!(f, "internal check failed: {what}"),
        }
    }
}

impl std::error::Error for PsbtError {}

impl Plan {
    /// SP-40, SP-41: the plan as a PSBT version 0 with everything the
    /// signer needs and nothing else.
    ///
    /// Each input carries the whole previous transaction, the previous
    /// output, the witness script and the key-path hints of the three keys.
    /// The change output (at the plan's change position) and the one output of
    /// a consolidation carry the
    /// witness script and the hints, so that the signer can prove it is the
    /// wallet's. The coins are checked against `wallet` again.
    ///
    /// For a wallet with one key the file is that of SP-82: each input
    /// carries the whole previous transaction, the previous output, the
    /// hash type `SIGHASH_ALL` and the key-path hint of its one key; the
    /// change output carries its hint; the account's extended public key
    /// is the one global extended public key; nothing else is written.
    pub fn to_psbt<'a>(&self, wallet: impl Into<WalletRef<'a>>) -> Result<Psbt, PsbtError> {
        let wref = wallet.into();
        if let Some(single) = wref.single() {
            return self.to_psbt_single(wref, single);
        }
        let Some(wallet) = wref.multisig() else {
            return Err(PsbtError::Internal("a wallet of neither kind"));
        };
        let mut psbt = Psbt::from_unsigned_tx(self.tx.clone())
            .map_err(|_| PsbtError::Internal("the transaction carries signatures"))?;
        if psbt.inputs.len() != self.inputs.len() {
            return Err(PsbtError::Internal("input count"));
        }
        for (entry, planned) in psbt.inputs.iter_mut().zip(&self.inputs) {
            let coin = &planned.coin;
            coin.check(wref).map_err(PsbtError::Coin)?;
            let output = coin
                .output()
                .ok_or(PsbtError::Internal("a checked coin has no output"))?;
            entry.non_witness_utxo = Some(coin.prev_tx.clone());
            entry.witness_utxo = Some(output.clone());
            entry.witness_script = Some(
                wallet
                    .witness_script(coin.chain, coin.index)
                    .map_err(|_| PsbtError::Derivation)?,
            );
            entry.bip32_derivation =
                key_hints(wallet, coin.chain, coin.index).ok_or(PsbtError::Derivation)?;
        }
        // The change output sits at the plan's change position; the output
        // of a consolidation is a payment to the change script.
        let mut payments = self.payments.iter();
        for (position, entry) in psbt.outputs.iter_mut().enumerate() {
            let at_change = self.change_position == Some(position);
            let payment = if at_change { None } else { payments.next() };
            let index = match (&self.change, payment) {
                (Some(change), _) if at_change => change.index,
                (_, Some(payment)) if payment.to_own_change => self.change_index,
                _ => continue,
            };
            entry.witness_script = Some(
                wallet
                    .witness_script(Keychain::Change, index)
                    .map_err(|_| PsbtError::Derivation)?,
            );
            entry.bip32_derivation =
                key_hints(wallet, Keychain::Change, index).ok_or(PsbtError::Derivation)?;
        }
        Ok(psbt)
    }
}

impl Plan {
    /// SP-82: the file for a wallet with one key.
    fn to_psbt_single(
        &self,
        wallet: WalletRef<'_>,
        single: &openbmt_core::wallet_single::SingleKeyWallet,
    ) -> Result<Psbt, PsbtError> {
        let mut psbt = Psbt::from_unsigned_tx(self.tx.clone())
            .map_err(|_| PsbtError::Internal("the transaction carries signatures"))?;
        if psbt.inputs.len() != self.inputs.len() {
            return Err(PsbtError::Internal("input count"));
        }
        psbt.xpub.insert(
            *single.xpub(),
            (single.fingerprint(), single_account_path(single)),
        );
        for (entry, planned) in psbt.inputs.iter_mut().zip(&self.inputs) {
            let coin = &planned.coin;
            coin.check(wallet).map_err(PsbtError::Coin)?;
            let output = coin
                .output()
                .ok_or(PsbtError::Internal("a checked coin has no output"))?;
            entry.non_witness_utxo = Some(coin.prev_tx.clone());
            entry.witness_utxo = Some(output.clone());
            entry.sighash_type = Some(PsbtSighashType::from(EcdsaSighashType::All));
            entry.bip32_derivation =
                single_hint(single, coin.chain, coin.index).ok_or(PsbtError::Derivation)?;
        }
        let mut payments = self.payments.iter();
        for (position, entry) in psbt.outputs.iter_mut().enumerate() {
            let at_change = self.change_position == Some(position);
            let payment = if at_change { None } else { payments.next() };
            let index = match (&self.change, payment) {
                (Some(change), _) if at_change => change.index,
                (_, Some(payment)) if payment.to_own_change => self.change_index,
                _ => continue,
            };
            entry.bip32_derivation =
                single_hint(single, Keychain::Change, index).ok_or(PsbtError::Derivation)?;
        }
        Ok(psbt)
    }
}
