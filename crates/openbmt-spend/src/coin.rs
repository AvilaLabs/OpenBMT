//! Coins of the wallet (section 1 of the rules).

use std::fmt;

use bitcoin::{OutPoint, Transaction, TxOut};
use openbmt_core::wallet::{Keychain, WalletDescription};

/// A coin of the wallet: the whole previous transaction, the output being
/// spent, and where in the wallet its script sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coin {
    /// The outpoint: the previous transaction's ID and the output index.
    pub outpoint: OutPoint,
    /// The whole previous transaction (SR-10 wants all of it).
    pub prev_tx: Transaction,
    /// The output index in `prev_tx`.
    pub vout: u32,
    /// The wallet keychain of the output's script.
    pub chain: Keychain,
    /// The wallet index of the output's script.
    pub index: u32,
    /// How many blocks deep the coin is; 0 is unconfirmed.
    pub confirmations: u32,
}

/// Why a coin was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoinError {
    /// SP-01: the previous transaction does not hash to the outpoint.
    PreviousTransactionMismatch {
        /// The outpoint the coin names.
        outpoint: OutPoint,
    },
    /// SP-01: the outpoint's output index is not in the transaction, or
    /// differs from the coin's `vout`.
    OutputMissing {
        /// The outpoint the coin names.
        outpoint: OutPoint,
    },
    /// SP-02: the output's script is not the wallet's script at that
    /// keychain and index.
    ScriptNotInWallet {
        /// The outpoint the coin names.
        outpoint: OutPoint,
    },
}

impl fmt::Display for CoinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PreviousTransactionMismatch { outpoint } => write!(
                f,
                "SP-01: the previous transaction of coin {outpoint} does not have the \
                 transaction ID of the outpoint"
            ),
            Self::OutputMissing { outpoint } => write!(
                f,
                "SP-01: coin {outpoint} names an output that is not in its previous transaction"
            ),
            Self::ScriptNotInWallet { outpoint } => write!(
                f,
                "SP-02: the script of coin {outpoint} is not the wallet's script at the \
                 keychain and index it names"
            ),
        }
    }
}

impl std::error::Error for CoinError {}

impl Coin {
    /// The output this coin spends, if it is in the previous transaction.
    pub fn output(&self) -> Option<&TxOut> {
        usize::try_from(self.vout)
            .ok()
            .and_then(|v| self.prev_tx.output.get(v))
    }

    /// SP-01 and SP-02: the previous transaction hashes to the outpoint,
    /// the output exists, and its script is exactly the wallet's script at
    /// the coin's keychain and index.
    pub fn check(&self, wallet: &WalletDescription) -> Result<(), CoinError> {
        let outpoint = self.outpoint;
        if self.prev_tx.compute_txid() != outpoint.txid {
            return Err(CoinError::PreviousTransactionMismatch { outpoint });
        }
        if self.vout != outpoint.vout {
            return Err(CoinError::OutputMissing { outpoint });
        }
        let output = self.output().ok_or(CoinError::OutputMissing { outpoint })?;
        let script = wallet
            .script(self.chain, self.index)
            .map_err(|_| CoinError::ScriptNotInWallet { outpoint })?;
        if script != output.script_pubkey {
            return Err(CoinError::ScriptNotInWallet { outpoint });
        }
        Ok(())
    }

    /// The order key of SP-11: the transaction ID bytes, then the index.
    pub(crate) fn outpoint_key(&self) -> ([u8; 32], u32) {
        use bitcoin::hashes::Hash;
        (self.outpoint.txid.to_byte_array(), self.outpoint.vout)
    }
}
