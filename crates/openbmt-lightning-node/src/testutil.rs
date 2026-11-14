//! Helpers for the tests of this crate.

use crate::bip85::NodeSeed;
use crate::device::{DeviceStatus, DirDevice, GatedDevice};
use crate::logger::Logger;
use crate::seal::SealKey;
use crate::store::{Background, DualStore};
use lightning::sign::KeysManager;
use lightning::util::logger::Level;
use lightning_persister::fs_store::FilesystemStore;
use std::path::PathBuf;
use std::sync::Arc;

/// A fresh empty directory under the cache directory the guard sets as
/// `TMPDIR`, never under `/tmp` by hand.
pub fn temp_dir(name: &str) -> PathBuf {
    let base = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap()).join(".cache/openbmt/tmp")
        });
    let d = base.join(format!("shop-ln-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The text of a single-key pinned wallet made from the BIP-85 text's master
/// key (account 0), with the checksum, as the counter pins it.
pub fn wallet_text() -> String {
    use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
    use bitcoin::secp256k1::Secp256k1;
    use std::str::FromStr;
    let secp = Secp256k1::new();
    let master = Xpriv::from_str("xprv9s21ZrQH143K2LBWUUQRFXhucrQqBpKdRRxNVq2zBqsx8HVqFk2uYo8kmbaLLHRdqtQpUm98uKfu3vca1LqdGhUtyoFnCNkfmXRyPXLjbKb").unwrap();
    let account = master
        .derive_priv(&secp, &DerivationPath::from_str("m/84h/0h/0h").unwrap())
        .unwrap();
    openbmt_core::wallet_single::SingleKeyWallet::new(
        master.fingerprint(&secp),
        0,
        Xpub::from_priv(&secp, &account),
    )
    .unwrap()
    .canonical_string()
    .to_string()
}

/// The two copies of a test node's store: the counter's folder and a second
/// device that can be made to go away.
pub struct Disks {
    pub dir: PathBuf,
    pub device: GatedDevice<DirDevice>,
    pub key: SealKey,
}

/// Fresh disks in a directory of this test's own.
pub fn disks(name: &str) -> Disks {
    let dir = temp_dir(name);
    Disks {
        device: GatedDevice::new(DirDevice::new(dir.join("device")).unwrap()),
        key: SealKey::from_seed(&NodeSeed([5; 64])),
        dir,
    }
}

/// The background store over `d`, with its flusher running: a start. A
/// second call on the same disks is a restart.
pub fn start_background(d: &Disks) -> (Arc<Background>, tokio::task::JoinHandle<()>) {
    let store = Arc::new(DualStore::new(
        Arc::new(FilesystemStore::new(d.dir.join("local"))),
        Arc::new(d.device.clone()),
        d.key.clone(),
        Arc::new(KeysManager::new(&[1; 32], 1, 1, true)),
        Arc::new(DeviceStatus::new()),
        Arc::new(Logger::open(&d.dir.join("log"), Level::Debug).unwrap()),
    ));
    let flusher = store.spawn_lazy_flusher(&tokio::runtime::Handle::current());
    (Arc::new(Background(store)), flusher)
}
