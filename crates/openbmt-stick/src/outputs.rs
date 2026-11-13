//! What comes out (SK-23, SK-26): from the 16 bytes of entropy, the 12 words
//! and the wallet's public description, equal to KM-06 for the same words.

use std::str::FromStr;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::NetworkKind;
use openbmt_core::wallet_single::SingleKeyWallet;

use crate::mix::ENTROPY_BYTES;

/// The account of SK-23 (BIP84, real bitcoin).
pub const ACCOUNT_PATH: &str = "m/84'/0'/0'";

/// The public result of SK-26.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Public {
    /// The master fingerprint, 8 hex characters.
    pub fingerprint: String,
    /// The first receive address, `bc1q...`.
    pub address: String,
    /// `wpkh([fp/84h/0h/0h]xpub/<0;1>/*)#checksum`.
    pub descriptor: String,
}

/// Why words could not be turned into a wallet. Cannot happen for words
/// made from 16 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputsError;

/// The 12 words of 16 bytes of entropy (SK-23).
pub fn words_of(entropy: &[u8; ENTROPY_BYTES]) -> Result<Vec<&'static str>, OutputsError> {
    let m = bip39::Mnemonic::from_entropy(entropy).map_err(|_| OutputsError)?;
    Ok(m.words().collect())
}

/// SK-26: the public result of 12 words, no passphrase.
pub fn public_of(words: &[&str]) -> Result<Public, OutputsError> {
    let m = bip39::Mnemonic::parse_normalized(&words.join(" ")).map_err(|_| OutputsError)?;
    if m.word_count() != 12 {
        return Err(OutputsError);
    }
    let seed = m.to_seed_normalized("");
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(NetworkKind::Main, &seed).map_err(|_| OutputsError)?;
    let path = DerivationPath::from_str(ACCOUNT_PATH).map_err(|_| OutputsError)?;
    let account = master.derive_priv(&secp, &path).map_err(|_| OutputsError)?;
    let xpub = Xpub::from_priv(&secp, &account);
    let wallet =
        SingleKeyWallet::new(master.fingerprint(&secp), 0, xpub).map_err(|_| OutputsError)?;
    let address = wallet.first_receive_address().map_err(|_| OutputsError)?;
    Ok(Public {
        fingerprint: wallet.fingerprint().to_string(),
        address: address.to_string(),
        descriptor: wallet.canonical_string().to_string(),
    })
}
