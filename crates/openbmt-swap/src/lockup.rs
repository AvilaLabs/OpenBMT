//! The lockup the service made on chain (SW-30, SW-31).

use bitcoin::{OutPoint, ScriptBuf, Transaction};

use crate::error::Refused;

/// Where the caller found the transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// In the mempool.
    Mempool,
    /// In a block.
    Block,
}

/// A lockup that pays the swap's address what was promised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockupSeen {
    /// The output to spend.
    pub outpoint: OutPoint,
    /// What it holds, in satoshis.
    pub value_sats: u64,
    /// Whether the transaction is in a block.
    pub confirmed: bool,
    /// Whether the transaction signals replacement (BIP 125).
    pub signals_rbf: bool,
}

/// SW-30, SW-31: confirm that `tx` pays `script` at least `promised`
/// satoshis in exactly one output, and say whether it is confirmed.
pub fn check_lockup(
    tx: &Transaction,
    script: &ScriptBuf,
    promised: u64,
    seen: Seen,
) -> Result<LockupSeen, Refused> {
    let mut found = None;
    for (vout, out) in tx.output.iter().enumerate() {
        if out.script_pubkey == *script {
            if found.is_some() {
                return Err(Refused::TwoLockupOutputs);
            }
            found = Some((vout, out.value.to_sat()));
        }
    }
    let (vout, value_sats) = found.ok_or(Refused::NoLockupOutput)?;
    if value_sats < promised {
        return Err(Refused::LockupTooSmall);
    }
    Ok(LockupSeen {
        outpoint: OutPoint::new(
            tx.compute_txid(),
            u32::try_from(vout).map_err(|_| Refused::NoLockupOutput)?,
        ),
        value_sats,
        confirmed: seen == Seen::Block,
        signals_rbf: tx.is_explicitly_rbf(),
    })
}
