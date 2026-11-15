//! The shop the demo owner starts from: the demo shop's wallet (the 2-of-3 wallet the demo counter takes its payments
//! into, its receive scripts the counter's first batch), held by the owner phone with its Key 1, as a shop that has the
//! Key app's three keys. The owner node watches the wallet's receive scripts, so a sale paid to the counter's address
//! is the owner's balance. The keys are the demo's fixed ones and guard nothing.

use bitcoin::Network;
use openbmt_demo::wallet::{demo_shop_wallet, key1_account};

/// The owner's secret: the demo's fixed key, the same the demo counter's
/// pairing is signed by.
pub const OWNER_SECRET: [u8; 32] = [1; 32];

/// The approver's root, as `start` takes it: the device id of the demo's approver key.
pub fn approver_root() -> Result<Vec<u8>, String> {
    let approver = openbmt_core::identity::Identity::from_secret_bytes(&[2; 32])
        .map_err(|e| format!("the demo approver key: {e:?}"))?;
    Ok(approver.device_id().to_bytes().to_vec())
}

/// The shop wallet's description text, as `start` takes it.
pub fn wallet_text() -> Result<String, String> {
    let wallet = demo_shop_wallet().ok_or("the demo shop's wallet could not be made")?;
    Ok(wallet.canonical_string().to_string())
}

/// Key 1's account key, as `start` takes it.
pub fn key1() -> Result<Vec<u8>, String> {
    key1_account()
        .map(|k| k.to_vec())
        .ok_or_else(|| "the demo Key 1 could not be made".to_string())
}

/// The regtest address of the shop wallet at receive `index`.
pub fn address(index: u32) -> Result<String, String> {
    let wallet = demo_shop_wallet().ok_or("the demo shop's wallet could not be made")?;
    let script = wallet.receive_script(index).map_err(|e| format!("{e:?}"))?;
    bitcoin::Address::from_script(&script, Network::Regtest)
        .map(|a| a.to_string())
        .map_err(|e| format!("{e}"))
}

#[cfg(test)]
mod tests;
