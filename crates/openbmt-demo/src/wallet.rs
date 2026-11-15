//! The demo's identities and wallet.
//!
//! The wallet is a single-key account made once from random bytes that were
//! thrown away; only its public account key is kept, here. Nothing can
//! spend what is paid to it, and its addresses are shown as regtest
//! addresses (`bcrt1...`), which no wallet on the main network accepts, so
//! no real bitcoin can be sent to one by mistake. The three keys of the
//! shop, the counter and the owner are fixed numbers: they sign only the
//! demo's own records.

use bitcoin::bip32::{ChildNumber, DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::Identity;
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_counter::OwnPairing;
use std::str::FromStr;

/// The master fingerprint of the demo wallet.
pub const FINGERPRINT: &str = "6bbfde79";
/// The account key of the demo wallet, at `84h/0h/0h`.
pub const ACCOUNT_XPUB: &str = "xpub6CET2RTt9Z3SBNSXqXennZxgF1hFHRGn6GogT59cyyztU6wc97KaW3yeVr359X7RQDEP89PtH2tEgVZqE3oJsjvFUgYmGHPBwifxNQh5foG";

/// The owner's key: a fixed number, used only in the demo.
pub fn owner() -> Result<Identity, String> {
    identity(1)
}

/// The counter's key.
pub fn counter() -> Result<Identity, String> {
    identity(3)
}

fn identity(n: u8) -> Result<Identity, String> {
    Identity::from_secret_bytes(&[n; 32]).map_err(|e| format!("the demo key {n}: {e:?}"))
}

/// The demo wallet.
pub fn demo_wallet() -> Result<PinnedWallet, String> {
    let fingerprint = Fingerprint::from_str(FINGERPRINT).map_err(|e| format!("{e}"))?;
    let xpub = Xpub::from_str(ACCOUNT_XPUB).map_err(|e| format!("{e}"))?;
    let single = SingleKeyWallet::new(fingerprint, 0, xpub).map_err(|e| format!("{e:?}"))?;
    Ok(PinnedWallet::new(ImportedWallet::Single(single)))
}

/// Key 1 of the demo shop's wallet is the owner phone's: its account key is made from a fixed number, like the demo's
/// identities, so that the demo owner can be started on the shop's wallet as a shop with the Key app's three keys.
/// Keys 2 and 3 are public keys only. The wallet is a regtest wallet (`bcrt1...` addresses) and guards nothing.
const KEY1_SEED: [u8; 32] = [0x11; 32];

/// The fingerprint and the account key at `48h/1h/0h/2h` of Keys 2 and 3, made once from random bytes that were thrown
/// away. Only these public keys are kept.
const OTHER_KEYS: [(&str, &str); 2] = [
    ("7f2dc2e3", "tpubDEmhcxEte8e1JWBZTPZs22Pk1BPSBxiozuB7XN97aanQY6v1HXTRt46TSuiPtkapHZVDhttAF79GTwgUcg3p2U6g7GGRi1kYSdu47E6q4rG"),
    ("4ed6df90", "tpubDFTXfAqAVw1qU3K9xyiM1q5R5F3dWZ4L1Yr1Hp5ALFWuuAFDjZK1mVg1V1KhHoDqpYYitihDYnPm5iQiUfNVQZTLyAPibWoZE7Lmd9LanZg"),
];

/// The path of the shop wallet's account keys.
fn shop_path() -> DerivationPath {
    DerivationPath::from(vec![
        ChildNumber::Hardened { index: 48 },
        ChildNumber::Hardened { index: 1 },
        ChildNumber::Hardened { index: 0 },
        ChildNumber::Hardened { index: 2 },
    ])
}

/// Key 1 of the demo shop: the fingerprint of its master key and its account key at `48h/1h/0h/2h`.
fn key1() -> Option<(Fingerprint, Xpriv)> {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, &KEY1_SEED).ok()?;
    let account = master.derive_priv(&secp, &shop_path()).ok()?;
    Some((master.fingerprint(&secp), account))
}

/// The account private key of Key 1, as the owner's start takes it (78 bytes).
pub fn key1_account() -> Option<[u8; 78]> {
    key1().map(|(_, account)| account.encode())
}

/// The demo shop's wallet: the 2-of-3 wallet of the counter's pairing, whose
/// receive scripts the first batch holds. The counter takes a batch of
/// P2WSH scripts only, and a pairing with a single-key wallet (the demo
/// wallet of `openbmt_demo::wallet`) is for the main network only, so the
/// shop's wallet here is a 2-of-3 one on regtest.
pub fn demo_shop_wallet() -> Option<WalletDescription> {
    let path = shop_path();
    let (fingerprint, account) = key1()?;
    let secp = Secp256k1::new();
    let first = KeySlot {
        index: 1,
        origin_fingerprint: fingerprint,
        origin_path: path.clone(),
        xpub: Xpub::from_priv(&secp, &account),
    };
    let other = |i: usize| -> Option<KeySlot> {
        let (fingerprint, xpub) = OTHER_KEYS.get(i)?;
        Some(KeySlot {
            index: u8::try_from(i + 2).ok()?,
            origin_fingerprint: Fingerprint::from_str(fingerprint).ok()?,
            origin_path: path.clone(),
            xpub: Xpub::from_str(xpub).ok()?,
        })
    };
    WalletDescription::new(Network::Regtest, [first, other(0)?, other(1)?]).ok()
}

/// What the demo counter is paired with: the owner, the demo wallet and the
/// range of its first 2,000 addresses.
pub fn pairing() -> Result<OwnPairing, String> {
    let owner = owner()?;
    let counter = counter()?;
    let range = IndexRange::new(counter.device_id(), 0, 2_000).map_err(|e| format!("{e:?}"))?;
    let first_range = SignedRange::sign(&owner, range).map_err(|e| format!("{e:?}"))?;
    Ok(OwnPairing {
        owner: owner.device_id(),
        wallet: demo_wallet()?,
        first_range,
        batch_policy: BatchPolicy { min_count: 1_000 },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::{Address, Network};

    /// The test vector: the demo wallet's description, and its first two
    /// addresses as regtest addresses.
    const DESCRIPTION: &str = "wpkh([6bbfde79/84h/0h/0h]xpub6CET2RTt9Z3SBNSXqXennZxgF1hFHRGn6GogT59cyyztU6wc97KaW3yeVr359X7RQDEP89PtH2tEgVZqE3oJsjvFUgYmGHPBwifxNQh5foG/<0;1>/*)#5epqtxzz";
    const FIRST_ADDRESS: &str = "bcrt1q9p0hpmws4pupj85nrw7d8uxna83wuyp0wnmg50";

    #[test]
    fn the_demo_wallet_is_the_committed_one() {
        let w = demo_wallet().unwrap();
        assert_eq!(w.canonical_string(), DESCRIPTION);
        assert_eq!(PinnedWallet::from_text(DESCRIPTION).unwrap(), w);
        let first = Address::from_script(&w.receive_script(0).unwrap(), Network::Regtest).unwrap();
        assert_eq!(first.to_string(), FIRST_ADDRESS);
        assert!(FIRST_ADDRESS.starts_with("bcrt1"));
    }

    #[test]
    fn no_address_of_the_wallet_is_a_main_network_address() {
        let w = demo_wallet().unwrap();
        for i in [0, 1, 1_999] {
            let script = w.receive_script(i).unwrap();
            let regtest = Address::from_script(&script, Network::Regtest)
                .unwrap()
                .to_string();
            assert!(regtest.starts_with("bcrt1"));
            assert!(Address::from_str(&regtest)
                .unwrap()
                .require_network(Network::Bitcoin)
                .is_err());
        }
    }

    #[test]
    fn the_pairing_is_checked_by_the_counter() {
        let p = pairing().unwrap();
        assert!(openbmt_counter::Counter::new_own(
            counter().unwrap(),
            &p,
            openbmt_core::settings::Settings::defaults(1_000)
        )
        .is_ok());
    }
}
