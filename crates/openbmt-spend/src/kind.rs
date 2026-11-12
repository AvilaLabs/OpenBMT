//! The two kinds of wallet the spend path serves (SP-80): the 2-of-3
//! wallet and the wallet with one key.

use std::collections::BTreeMap;
use std::fmt;

use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint};
use bitcoin::secp256k1;
use bitcoin::{PublicKey, ScriptBuf};
use openbmt_core::wallet::{Keychain, WalletDescription};
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};

use crate::common::chain_number;

/// One of the two kinds of wallet. An error that says a coin, a file or a
/// transaction is of the other kind names the kind the path expected
/// (SP-80).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletKind {
    /// The wallet of three keys, two of which sign (P2WSH).
    Multisig,
    /// The wallet with one key (P2WPKH).
    Single,
}

impl fmt::Display for WalletKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Multisig => write!(f, "a wallet of three keys"),
            Self::Single => write!(f, "a wallet with one key"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Inner<'a> {
    /// `typed` is set when the caller handed in an `ImportedWallet`: then a
    /// coin or file of the other kind is a typed error (SP-80). A bare
    /// `WalletDescription` keeps the refusals the 2-of-3 path always had.
    Multisig {
        wallet: &'a WalletDescription,
        typed: bool,
    },
    Single(&'a SingleKeyWallet),
}

/// A wallet as the spend functions take it (SP-80): an
/// [`ImportedWallet`], or, for callers that hold a 2-of-3 wallet, a
/// `&WalletDescription`.
#[derive(Debug, Clone, Copy)]
pub struct WalletRef<'a>(Inner<'a>);

impl<'a> From<&'a WalletDescription> for WalletRef<'a> {
    fn from(wallet: &'a WalletDescription) -> Self {
        Self(Inner::Multisig {
            wallet,
            typed: false,
        })
    }
}

impl<'a> From<&'a SingleKeyWallet> for WalletRef<'a> {
    fn from(wallet: &'a SingleKeyWallet) -> Self {
        Self(Inner::Single(wallet))
    }
}

impl<'a> From<&'a ImportedWallet> for WalletRef<'a> {
    fn from(wallet: &'a ImportedWallet) -> Self {
        match wallet {
            ImportedWallet::Multisig(wallet) => Self(Inner::Multisig {
                wallet,
                typed: true,
            }),
            ImportedWallet::Single(wallet) => Self(Inner::Single(wallet)),
        }
    }
}

impl<'a> From<&&'a WalletDescription> for WalletRef<'a> {
    fn from(wallet: &&'a WalletDescription) -> Self {
        Self::from(*wallet)
    }
}

impl<'a> WalletRef<'a> {
    /// Which kind of wallet this is.
    pub fn kind(&self) -> WalletKind {
        match self.0 {
            Inner::Multisig { .. } => WalletKind::Multisig,
            Inner::Single(_) => WalletKind::Single,
        }
    }

    /// The 2-of-3 wallet, if that is the kind.
    pub(crate) fn multisig(&self) -> Option<&'a WalletDescription> {
        match self.0 {
            Inner::Multisig { wallet, .. } => Some(wallet),
            Inner::Single(_) => None,
        }
    }

    /// The wallet with one key, if that is the kind.
    pub(crate) fn single(&self) -> Option<&'a SingleKeyWallet> {
        match self.0 {
            Inner::Single(wallet) => Some(wallet),
            Inner::Multisig { .. } => None,
        }
    }

    /// The wallet's script at `chain/index`, as `openbmt-core` derives it.
    pub(crate) fn script(&self, chain: Keychain, index: u32) -> Option<ScriptBuf> {
        match self.0 {
            Inner::Multisig { wallet, .. } => wallet.script(chain, index).ok(),
            Inner::Single(wallet) => wallet.script(chain, index).ok(),
        }
    }

    /// Whether `script`, which is not the wallet's own, is of the other
    /// kind of wallet: for a wallet with one key, a P2WSH script; for a
    /// wallet of three keys handed in as an `ImportedWallet`, a P2WPKH
    /// script.
    pub(crate) fn is_other_kind_script(&self, script: &bitcoin::Script) -> bool {
        match self.0 {
            Inner::Single(_) => script.is_p2wsh(),
            Inner::Multisig { typed, .. } => typed && script.is_p2wpkh(),
        }
    }

    /// Whether an input of a file spending `script`, with or without a
    /// witness script, is of the other kind of wallet.
    pub(crate) fn is_other_kind_input(
        &self,
        script: &bitcoin::Script,
        has_witness_script: bool,
    ) -> bool {
        match self.0 {
            Inner::Single(_) => script.is_p2wsh() || has_witness_script,
            Inner::Multisig { typed, .. } => typed && script.is_p2wpkh(),
        }
    }
}

/// The path `84'/0'/account'` of the account key of a wallet with one key.
pub(crate) fn single_account_path(wallet: &SingleKeyWallet) -> DerivationPath {
    DerivationPath::from(vec![
        ChildNumber::Hardened { index: 84 },
        ChildNumber::Hardened { index: 0 },
        ChildNumber::Hardened {
            index: wallet.account(),
        },
    ])
}

/// SP-82: the key-path hint of the one key at `chain/index`: the master
/// fingerprint and the path `84'/0'/account'/chain/index`.
pub(crate) fn single_hint(
    wallet: &SingleKeyWallet,
    chain: Keychain,
    index: u32,
) -> Option<BTreeMap<secp256k1::PublicKey, (Fingerprint, DerivationPath)>> {
    let key = wallet.public_key(chain, index).ok()?;
    let path = single_account_path(wallet)
        .child(chain_number(chain))
        .child(ChildNumber::Normal { index });
    Some(BTreeMap::from([(key.0, (wallet.fingerprint(), path))]))
}

/// The one key of the wallet at `chain/index`.
pub(crate) fn single_key(
    wallet: &SingleKeyWallet,
    chain: Keychain,
    index: u32,
) -> Option<PublicKey> {
    let key = wallet.public_key(chain, index).ok()?;
    Some(PublicKey::new(key.0))
}

/// Why the hints of an input of a wallet with one key did not name one
/// place in the wallet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SingleHintFault {
    /// No hint, or more than one, or a hint of another origin.
    None,
    /// The hint's key is not the one the wallet derives at its path.
    KeyMismatch,
}

/// The place the one hint of an input names: exactly one hint, whose
/// fingerprint and path are the wallet's `84'/0'/account'` followed by
/// the chain (0 or 1) and an index, all unhardened, and whose key is the
/// one the wallet derives there.
pub(crate) fn single_claimed_path(
    wallet: &SingleKeyWallet,
    hints: &BTreeMap<secp256k1::PublicKey, (Fingerprint, DerivationPath)>,
) -> Result<(Keychain, u32), SingleHintFault> {
    let mut it = hints.iter();
    let (Some((key, (fingerprint, path))), None) = (it.next(), it.next()) else {
        return Err(SingleHintFault::None);
    };
    if *fingerprint != wallet.fingerprint() {
        return Err(SingleHintFault::None);
    }
    let steps: &[ChildNumber] = path.as_ref();
    let origin = single_account_path(wallet);
    let origin: &[ChildNumber] = origin.as_ref();
    let Some(tail) = steps.strip_prefix(origin) else {
        return Err(SingleHintFault::None);
    };
    let [ChildNumber::Normal { index: chain }, ChildNumber::Normal { index }] = tail else {
        return Err(SingleHintFault::None);
    };
    let chain = match chain {
        0 => Keychain::Receive,
        1 => Keychain::Change,
        _ => return Err(SingleHintFault::None),
    };
    let derived = wallet
        .public_key(chain, *index)
        .map_err(|_| SingleHintFault::KeyMismatch)?;
    if derived.0 != *key {
        return Err(SingleHintFault::KeyMismatch);
    }
    Ok((chain, *index))
}
