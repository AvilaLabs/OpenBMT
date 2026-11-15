//! The demo's identities and wallet.
//!
//! The wallet is a single-key account made once from random bytes that were
//! thrown away; only its public account key is kept, here. Nothing can
//! spend what is paid to it, and its addresses are shown as regtest
//! addresses (`bcrt1...`), which no wallet on the main network accepts, so
//! no real bitcoin can be sent to one by mistake. The three keys of the
//! shop, the counter and the owner are fixed numbers: they sign only the
//! demo's own records.

use bitcoin::bip32::{Fingerprint, Xpub};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::identity::Identity;
use openbmt_core::pinned_wallet::{IndexRange, PinnedWallet, SignedRange};
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
