//! SS-01 to SS-05: the kernel's bytes, checked; the mix with the owner's die
//! rolls; the 12 words and the wallet of them.

use std::fmt;
use std::str::FromStr;

use bip39::{Language, Mnemonic};
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use bitcoin::{Address, ScriptBuf};
use openbmt_core::wallet::Keychain;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};

use super::{wipe, StarterRefusal};

pub use openbmt_core::mix::{ENTROPY_BYTES, KERNEL_BYTES};

/// SS-03: the tag hashed first, as the stick's (SK-20) with its own name.
pub const STARTER_TAG: &[u8] = b"OpenBMT/starter/v1";
/// SS-04: the number of words.
pub const STARTER_WORDS: usize = 12;

/// SS-01: where the 32 bytes come from. The trait and the checks live in
/// `openbmt-kernel-random`, which every key of the Rust side shares.
pub use openbmt_kernel_random::KernelRandom;

/// SS-02: two separate reads of the source, neither all one byte, and the
/// two different. The first is returned; the second is wiped.
pub fn checked_read(rng: &mut dyn KernelRandom) -> Result<[u8; KERNEL_BYTES], StarterRefusal> {
    openbmt_kernel_random::checked_read(rng).map_err(|e| match e {
        openbmt_kernel_random::Refusal::Unavailable => StarterRefusal::Random,
        openbmt_kernel_random::Refusal::Weak => StarterRefusal::RandomWeak,
        openbmt_kernel_random::Refusal::Repeat => StarterRefusal::RandomRepeat,
    })
}

/// The entropy of the 12 words, in memory only while a call uses it. The
/// buffer is overwritten when the value is dropped, and `Debug` hides it.
pub struct Seed([u8; ENTROPY_BYTES]);

impl Seed {
    /// The seed of 16 bytes of entropy.
    pub fn from_entropy(entropy: [u8; ENTROPY_BYTES]) -> Self {
        Self(entropy)
    }

    /// The seed of 12 words of the English list with a right check word.
    pub fn from_mnemonic(m: &Mnemonic) -> Result<Self, StarterRefusal> {
        let mut entropy = m.to_entropy();
        let out = <[u8; ENTROPY_BYTES]>::try_from(entropy.as_slice())
            .map(Self)
            .map_err(|_| StarterRefusal::Words);
        wipe(&mut entropy);
        out
    }

    pub(crate) fn entropy(&self) -> &[u8; ENTROPY_BYTES] {
        &self.0
    }

    /// The 12 words as the library holds them (indexes into the list, not
    /// text).
    pub(crate) fn mnemonic(&self) -> Result<Mnemonic, StarterRefusal> {
        Mnemonic::from_entropy_in(Language::English, &self.0).map_err(|_| StarterRefusal::Words)
    }

    /// SS-04: the wallet of the words: BIP84, `m/84'/0'/0'`, no passphrase.
    pub fn wallet(&self) -> Result<StarterWallet, StarterRefusal> {
        Ok(self.open()?.wallet.clone())
    }

    /// The account key and the wallet, only inside one call.
    pub(crate) fn open(&self) -> Result<Opened, StarterRefusal> {
        let secp = Secp256k1::new();
        let mut words = self.mnemonic()?;
        let mut seed = words.to_seed("");
        openbmt_core::entropy::wipe_mnemonic(&mut words);
        let master = Xpriv::new_master(Network::Bitcoin, &seed);
        wipe(&mut seed);
        let mut master = master.map_err(|_| StarterRefusal::Wallet)?;
        let path = DerivationPath::from_str("m/84'/0'/0'").map_err(|_| StarterRefusal::Wallet)?;
        let account = master
            .derive_priv(&secp, &path)
            .map_err(|_| StarterRefusal::Wallet);
        let fingerprint = master.fingerprint(&secp);
        master.private_key.non_secure_erase();
        let account = account?;
        let xpub = Xpub::from_priv(&secp, &account);
        let wallet =
            SingleKeyWallet::new(fingerprint, 0, xpub).map_err(|_| StarterRefusal::Wallet)?;
        Ok(Opened {
            account,
            wallet: StarterWallet(wallet),
        })
    }
}

impl fmt::Debug for Seed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Seed(<hidden>)")
    }
}

impl Drop for Seed {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// The wallet and its account key, only inside one call.
pub(crate) struct Opened {
    pub(crate) account: Xpriv,
    pub(crate) wallet: StarterWallet,
}

impl Drop for Opened {
    fn drop(&mut self) {
        self.account.private_key.non_secure_erase();
    }
}

/// SS-03, SS-04: a new seed: 32 checked bytes of the kernel and the rolls
/// (each 1 to 6, any number from 0) mixed into 16 bytes of entropy.
pub fn make_seed(rng: &mut dyn KernelRandom, rolls: &[u8]) -> Result<Seed, StarterRefusal> {
    let mut kernel = checked_read(rng)?;
    let mixed = openbmt_core::mix::mix(STARTER_TAG, &kernel, rolls);
    wipe(&mut kernel);
    let mut entropy = mixed.map_err(|_| StarterRefusal::Rolls)?;
    let seed = Seed::from_entropy(entropy);
    wipe(&mut entropy);
    Ok(seed)
}

/// SS-04: the starter wallet's public side, which the app may keep: it
/// holds no secret. Receive and change scripts are P2WPKH of the account
/// `84'/0'/0'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StarterWallet(SingleKeyWallet);

impl StarterWallet {
    /// The wallet from its canonical description, as the app kept it.
    pub fn parse(description: &str) -> Result<Self, StarterRefusal> {
        let w = SingleKeyWallet::parse(description).map_err(|_| StarterRefusal::Wallet)?;
        if w.account() != 0 {
            return Err(StarterRefusal::Wallet);
        }
        Ok(Self(w))
    }

    /// The description, checksum included.
    pub fn description(&self) -> &str {
        self.0.canonical_string()
    }

    /// The master fingerprint, 8 hex digits.
    pub fn fingerprint(&self) -> String {
        self.0.fingerprint().to_string()
    }

    /// The wallet as the spend crate takes it.
    pub fn imported(&self) -> ImportedWallet {
        ImportedWallet::Single(self.0.clone())
    }

    pub(crate) fn single(&self) -> &SingleKeyWallet {
        &self.0
    }

    /// The script at `chain/index`.
    pub fn script(&self, chain: Keychain, index: u32) -> Result<ScriptBuf, StarterRefusal> {
        self.0
            .script(chain, index)
            .map_err(|_| StarterRefusal::Wallet)
    }

    /// The receive address at `index`.
    pub fn receive_address(&self, index: u32) -> Result<Address, StarterRefusal> {
        self.0
            .address(Keychain::Receive, index)
            .map_err(|_| StarterRefusal::Wallet)
    }
}
