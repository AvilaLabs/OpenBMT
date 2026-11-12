//! Reading a scanned wallet description through `dispatch` (op 145,
//! `docs/WALLET-RULES.md` WL-08 to WL-15): the key maker guide shows the
//! fingerprint and first address it gets back.

use std::str::FromStr;

use bitcoin::bip32::{DerivationPath, Fingerprint, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::Network;

use openbmt_core::wallet::{descriptor_checksum, KeySlot, WalletDescription};
use openbmt_core::wallet_single::{ImportRefused, SingleKeyWallet};

use super::wire::{frame, Rd, W};
use crate::{dispatch, Registry};

/// The fingerprint, account key and first address of the test seed with
/// fingerprint 818bc9b6, from the key maker's Electrum table.
fn seed() -> (String, String, String) {
    let path = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR")
            .expect("cargo sets CARGO_MANIFEST_DIR when it runs tests"),
    )
    .join("../openbmt-core/testdata/keymaker-electrum-expected.txt");
    let table = std::fs::read_to_string(path).expect("the table is there");
    let row = table
        .lines()
        .find(|l| !l.starts_with('#') && l.contains(" 818bc9b6 "))
        .expect("the seed is in the table");
    let mut tail = row.rsplit(' ');
    let address = tail.next().expect("address").to_string();
    let xpub = tail.next().expect("xpub").to_string();
    let fingerprint = tail.next().expect("fingerprint").to_string();
    (fingerprint, xpub, address)
}

pub(super) fn descriptor() -> String {
    let (fp, xpub, _) = seed();
    SingleKeyWallet::new(
        Fingerprint::from_str(&fp).expect("fingerprint"),
        0,
        Xpub::from_str(&xpub).expect("xpub"),
    )
    .expect("a wallet")
    .canonical_string()
    .to_string()
}

fn call(text: &str) -> Vec<u8> {
    dispatch(&Registry::new(), &frame(145, 0, &W::new().string(text).0))
}

fn refusal(text: &str) -> super::wire::Fault {
    Rd::error(&call(text)).expect("refused")
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn wl_14_the_test_seed_gives_its_fingerprint_and_first_address() {
    let (fp, _, address) = seed();
    assert_eq!(fp, "818bc9b6");
    let text = descriptor();
    let response = call(&text);
    let mut r = Rd::ok(&response);
    let (kind, fingerprint, first) = (r.u8(), r.opt(|r| r.string()), r.string());
    r.done();
    assert_eq!(kind, 1, "single-key");
    assert_eq!(fingerprint.as_deref(), Some("818bc9b6"));
    assert_eq!(first, address);
    // The view holds neither the description nor the key in it.
    assert!(!contains(&response, &text));
    assert!(!contains(&response, &seed().1));
}

#[test]
fn wl_14_the_two_single_path_descriptions_are_read_too() {
    let (fp, xpub, address) = seed();
    let one = |branch: u8| {
        let body = format!("wpkh([{fp}/84h/0h/0h]{xpub}/{branch}/*)");
        format!("{body}#{}", descriptor_checksum(&body).expect("checksum"))
    };
    let response = call(&format!("{} {}", one(0), one(1)));
    let mut r = Rd::ok(&response);
    let (_, fingerprint, first) = (r.u8(), r.opt(|r| r.string()), r.string());
    r.done();
    assert_eq!(
        (fingerprint.as_deref(), first.as_str()),
        (Some(fp.as_str()), address.as_str())
    );
}

#[test]
fn wl_15_each_refusal_gives_the_cores_words_and_none_of_the_text() {
    let (fp, xpub, _) = seed();
    let secret = "xprv9s21ZrQH143K3QTDL4LXw2F7HEK3wJUD2nW2nRk4stbPy6cq3jPPqjiChkVvvNKmPGJxWUtg6LnF5kejMRNNU3TGtRBeJgk33yuGBxrMPHi";
    let tpub = format!("wpkh([{fp}/84h/1h/0h]tpubDC5FSnBiZDMmhiuCmWAYsLwgLYrrT9rAqvTySfuCCrgsWz8wxMXUS9Tb9iVMvcRbvFcAHGkMD5Kx8koh4GquNGNTfohfk7pgjhaPCdXpoba/0/*)");
    let ok_sum = |body: &str| format!("{body}#{}", descriptor_checksum(body).expect("checksum"));
    let other = ok_sum(&format!("pkh([{fp}/44h/0h/0h]{xpub}/0/*)"));
    let cases: [(&str, ImportRefused, &str); 4] = [
        (secret, ImportRefused::HasPrivateKey, "WL-08"),
        (
            "wpkh([818bc9b6/84h/0h/0h]xpub/0/*)#aaaaaaaa",
            ImportRefused::Damaged,
            "WL-10",
        ),
        (&tpub, ImportRefused::TestNetwork, "WL-11"),
        (&other, ImportRefused::Unsupported, "WL-12"),
    ];
    for (text, why, rule) in cases {
        let response = call(text);
        let fault = Rd::error(&response).expect("refused");
        assert_eq!(fault.status, 1);
        assert_eq!(fault.rule, rule);
        let words = why.words();
        assert_eq!(fault.words.heading, words.heading);
        assert_eq!(fault.words.what, words.what);
        assert_eq!(fault.words.next, words.next);
        assert!(!contains(&response, text), "{rule}: the text came back");
    }
    // Text that is not a description at all, and text that is too long.
    assert_eq!(refusal("hello").rule, "WL-10");
    assert_eq!(refusal(&"a".repeat(100_000)).rule, "WL-12");
}

#[test]
fn wl_15_the_request_is_read_completely() {
    let mut extra = W::new().string("x").0;
    extra.push(0);
    let response = dispatch(&Registry::new(), &frame(145, 0, &extra));
    assert!(Rd::error(&response).is_some());
}

/// A mainnet 2-of-3 wallet from three fixed seeds.
fn multisig() -> WalletDescription {
    let secp = Secp256k1::new();
    let path: DerivationPath = "m/48'/0'/0'/2'".parse().expect("a path");
    let slots: Vec<KeySlot> = (0..3u8)
        .map(|n| {
            let master = Xpriv::new_master(Network::Bitcoin, &[n + 1; 32]).expect("a master key");
            let account = master.derive_priv(&secp, &path).expect("an account");
            KeySlot {
                index: n + 1,
                origin_fingerprint: master.fingerprint(&secp),
                origin_path: path.clone(),
                xpub: Xpub::from_priv(&secp, &account),
            }
        })
        .collect();
    WalletDescription::new(Network::Bitcoin, slots.try_into().expect("three")).expect("a wallet")
}

#[test]
fn wl_14_a_multisig_description_has_no_fingerprint_and_gives_its_first_address() {
    let wallet = multisig();
    let response = call(wallet.canonical_string());
    let mut r = Rd::ok(&response);
    let (kind, fingerprint, first) = (r.u8(), r.opt(|r| r.string()), r.string());
    r.done();
    assert_eq!(kind, 2, "multisig");
    assert_eq!(fingerprint, None);
    let want = wallet
        .address(openbmt_core::wallet::Keychain::Receive, 0)
        .expect("an address");
    assert_eq!(first, want.to_string());
}
