//! The shop the demo owner starts from: the demo wallet (the one the demo
//! counter takes its payments into) held as the shop's own wallet, with the
//! first range of its addresses given to the demo counter. The keys are the
//! demo's fixed ones and guard nothing.

use bitcoin::Network;
use openbmt_counter_node::vfs::fs;
use openbmt_demo::wallet::{counter, demo_wallet, owner};
use openbmt_owner::own_shop::OwnShop;
use openbmt_owner_node::OWN_STATE_FILE;

/// The owner's secret: the demo's fixed key, the same the demo counter's
/// pairing is signed by.
pub const OWNER_SECRET: [u8; 32] = [1; 32];

/// The wallet as the app keeps it (`OBM)`), for `start`'s own wallet.
pub fn wallet_file() -> Result<Vec<u8>, String> {
    Ok(demo_wallet()?.to_bytes())
}

/// The regtest address of the demo wallet at receive `index`.
pub fn address(index: u32) -> Result<String, String> {
    let script = demo_wallet()?
        .receive_script(index)
        .map_err(|e| format!("{e:?}"))?;
    bitcoin::Address::from_script(&script, Network::Regtest)
        .map(|a| a.to_string())
        .map_err(|e| format!("{e}"))
}

/// Writes the shop's state under `dir`, as the owner's phone holds it after
/// the demo counter was paired: its first range given. A state already there
/// is left as it is.
pub fn seed(dir: &str, now: u64) -> Result<(), String> {
    let path = std::path::Path::new(dir).join(OWN_STATE_FILE);
    if fs::metadata(&path).is_ok() {
        return Ok(());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut shop = OwnShop::new(owner()?, demo_wallet()?);
    shop.allocate(counter()?.device_id(), now, |bytes| fs::write(&path, bytes))
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests;
