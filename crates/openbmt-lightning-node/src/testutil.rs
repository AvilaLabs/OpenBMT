//! Helpers for the tests of this crate.

use std::path::PathBuf;

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
