//! The shop the demo owner starts from.

use super::*;
use openbmt_core::wallet::WalletDescription;

#[test]
fn the_wallet_text_is_the_shop_wallet_the_counter_takes_payments_into() {
    let text = wallet_text().expect("a text");
    let read = WalletDescription::parse(&text, Network::Regtest).expect("it reads back");
    assert_eq!(read, demo_shop_wallet().expect("the wallet"));
    // The first address is the one the counter's first sale is paid to: the first script of its batch.
    let script = read.receive_script(0).expect("a script");
    let first = bitcoin::Address::from_script(&script, Network::Regtest).expect("an address");
    assert_eq!(address(0).expect("an address"), first.to_string());
    assert!(first.to_string().starts_with("bcrt1q"));
}

#[test]
fn key_1_is_one_of_the_wallets_three_keys_and_the_root_is_the_approvers() {
    use bitcoin::bip32::{Xpriv, Xpub};
    let account = Xpriv::decode(&key1().expect("a key")).expect("it reads back");
    let xpub = Xpub::from_priv(&bitcoin::secp256k1::Secp256k1::new(), &account);
    let wallet = demo_shop_wallet().expect("the wallet");
    assert!(wallet.slots().iter().any(|s| s.xpub == xpub), "Key 1 is in the wallet");
    assert_eq!(approver_root().expect("a root").len(), 32);
}
