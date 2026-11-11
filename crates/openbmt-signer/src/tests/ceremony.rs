//! The public parts of the key ceremony (`docs/SIGNER-RULES.md`, SR-64 to
//! SR-69): key text, the Key app's answer and the wallet setup.

use bitcoin::base58;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::Network;
use openbmt_core::identity::Identity;
use openbmt_core::say::{check, Who};

use super::kit::*;
use crate::crypto::secp;
use crate::{
    check_key3, check_key3_in_wallet, CeremonyRefusal, KeyAnswer, KeyPart, WalletSetupBytes,
    ALL_REFUSALS,
};

/// A key at `path` from a seed byte, as an `Xpub`, with its master
/// fingerprint.
fn key_at(network: Network, seed: u8, path: &str) -> KeyPart {
    let master = Xpriv::new_master(network, &[seed; 32]).unwrap();
    let path: DerivationPath = path.parse().unwrap();
    let account = master.derive_priv(secp(), &path).unwrap();
    KeyPart {
        fingerprint: master.fingerprint(secp()),
        xpub: Xpub::from_priv(secp(), &account),
    }
}

fn main_key(seed: u8) -> KeyPart {
    key_at(Network::Bitcoin, seed, "m/48'/0'/0'/2'")
}

fn test_key(seed: u8) -> KeyPart {
    key_at(Network::Regtest, seed, "m/48'/1'/0'/2'")
}

/// The key's text again with another set of version bytes, as the other
/// wallet formats write the same key.
fn reversioned(part: &KeyPart, version: [u8; 4]) -> String {
    let mut raw = part.xpub.encode().to_vec();
    raw[..4].copy_from_slice(&version);
    base58::encode_check(&raw)
}

const ZPUB_UPPER: [u8; 4] = [0x02, 0xAA, 0x7E, 0xD3];
const VPUB_UPPER: [u8; 4] = [0x02, 0x57, 0x54, 0x83];

fn answer() -> KeyAnswer {
    KeyAnswer {
        network: Network::Regtest,
        key2: Some(test_key(2)),
        key3: test_key(3),
        device_root: Identity::from_secret_bytes(&[0x22; 32])
            .unwrap()
            .device_id(),
    }
}

fn setup() -> WalletSetupBytes {
    WalletSetupBytes {
        network: NET,
        description: kit().wallet.clone(),
        owner_root: Identity::from_secret_bytes(&[0x11; 32])
            .unwrap()
            .device_id(),
        batch_min_count: 3,
    }
}

#[test]
fn sr_64_key_text_with_its_origin_is_read_in_both_hardened_marks() {
    let k = test_key(3);
    let plain = format!("[{}/48h/1h/0h/2h]{}", k.fingerprint, k.xpub);
    assert_eq!(KeyPart::parse(&plain), Ok(k.clone()));
    let ticks = format!("[{}/48'/1'/0'/2']{}", k.fingerprint, k.xpub);
    assert_eq!(KeyPart::parse(&ticks), Ok(k.clone()));
    assert_eq!(KeyPart::parse(&format!("  {plain}\n")), Ok(k.clone()));
    assert_eq!(k.origin_text(), plain);
    assert_eq!(KeyPart::parse(&k.origin_text()), Ok(k));
    let m = main_key(3);
    assert_eq!(KeyPart::parse(&m.origin_text()), Ok(m.clone()));
    assert_eq!(m.origin_path(), "48h/0h/0h/2h");
}

#[test]
fn sr_64_the_multisig_segwit_forms_are_read_as_the_plain_key() {
    let m = main_key(3);
    let text = format!(
        "[{}/48h/0h/0h/2h]{}",
        m.fingerprint,
        reversioned(&m, ZPUB_UPPER)
    );
    assert!(text.contains("Zpub"));
    assert_eq!(KeyPart::parse(&text), Ok(m));
    let t = test_key(3);
    let text = format!(
        "[{}/48'/1'/0'/2']{}",
        t.fingerprint,
        reversioned(&t, VPUB_UPPER)
    );
    assert!(text.contains("Vpub"));
    assert_eq!(KeyPart::parse(&text), Ok(t));
}

#[test]
fn sr_64_a_key_without_its_origin_is_refused_and_nothing_is_filled_in() {
    let k = test_key(3);
    assert_eq!(
        KeyPart::parse(&k.xpub.to_string()),
        Err(CeremonyRefusal::NoOrigin)
    );
    let m = main_key(3);
    let zpub = reversioned(&m, ZPUB_UPPER);
    assert_eq!(KeyPart::parse(&zpub), Err(CeremonyRefusal::NoOrigin));
    // A fingerprint typed in front without a path is not an origin.
    assert_eq!(
        KeyPart::parse(&format!("[{}]{}", k.fingerprint, k.xpub)),
        Err(CeremonyRefusal::BadOrigin)
    );
}

#[test]
fn sr_64_an_origin_that_is_not_the_shared_wallet_path_is_refused() {
    let k = test_key(3);
    let x = k.xpub.to_string();
    let fp = k.fingerprint.to_string();
    let bad = [
        format!("[{fp}/84h/1h/0h]{x}"),
        format!("[{fp}/48h/0h/0h/2h]{x}"),
        format!("[{fp}/48h/1h/0h/1h]{x}"),
        format!("[{fp}/48h/1h/1h/2h]{x}"),
        format!("[{fp}/48h/1h/0h/2]{x}"),
        format!("[{}/48h/1h/0h/2h]{x}", &fp[..6]),
        format!("[{fp}0/48h/1h/0h/2h]{x}"),
        format!("[zzzzzzzz/48h/1h/0h/2h]{x}"),
        format!("[{fp}/48h/1h/0h/2h{x}"),
        format!("[{fp}48h/1h/0h/2h]{x}"),
        format!("[{fp}/48h/1h/0h/2h/0]{x}"),
    ];
    for text in bad {
        let got = KeyPart::parse(&text);
        assert!(
            matches!(
                got,
                Err(CeremonyRefusal::BadOrigin | CeremonyRefusal::NotAKey)
            ),
            "{text}: {got:?}"
        );
        assert!(got.is_err());
    }
    // The ones that are well formed but for another path name the origin.
    assert_eq!(
        KeyPart::parse(&format!("[{fp}/84h/1h/0h]{x}")),
        Err(CeremonyRefusal::BadOrigin)
    );
    assert_eq!(
        KeyPart::parse(&format!("[{fp}/48h/0h/0h/2h]{x}")),
        Err(CeremonyRefusal::BadOrigin)
    );
}

#[test]
fn sr_64_text_that_is_not_an_account_key_is_refused() {
    let k = test_key(3);
    let fp = k.fingerprint;
    for text in ["", "   ", "[]", "not a key", "[00000000/48h/1h/0h/2h]xpub"] {
        assert!(KeyPart::parse(text).is_err(), "{text:?}");
    }
    assert_eq!(
        KeyPart::parse(&"x".repeat(300)),
        Err(CeremonyRefusal::NotKeyText)
    );
    assert_eq!(
        KeyPart::parse("[00000000/48h/1h/0h/2h]\u{e9}"),
        Err(CeremonyRefusal::NotKeyText)
    );
    // One character changed: the base58 check fails.
    let mut x = k.xpub.to_string();
    let last = x.pop().unwrap();
    x.push(if last == 'a' { 'b' } else { 'a' });
    assert_eq!(
        KeyPart::parse(&format!("[{fp}/48h/1h/0h/2h]{x}")),
        Err(CeremonyRefusal::NotAKey)
    );
    // A key one level up (depth 3) and the master key (depth 0).
    let up = key_at(Network::Regtest, 3, "m/48'/1'/0'").xpub;
    assert_eq!(
        KeyPart::parse(&format!("[{fp}/48h/1h/0h/2h]{up}")),
        Err(CeremonyRefusal::BadLevel)
    );
    let master = Xpub::from_priv(
        secp(),
        &Xpriv::new_master(Network::Regtest, &[3; 32]).unwrap(),
    );
    assert_eq!(
        KeyPart::parse(&format!("[{fp}/48h/1h/0h/2h]{master}")),
        Err(CeremonyRefusal::BadLevel)
    );
}

#[test]
fn sr_64_a_secret_key_is_refused_and_never_echoed() {
    let secret = Xpriv::new_master(Network::Regtest, &[3; 32])
        .unwrap()
        .to_string();
    let err = KeyPart::parse(&format!("[00000000/48h/1h/0h/2h]{secret}")).unwrap_err();
    assert_eq!(err, CeremonyRefusal::SecretKey);
    assert!(!err.to_string().contains(&secret[..12]));
    assert_eq!(KeyPart::parse(&secret), Err(CeremonyRefusal::SecretKey));
}

#[test]
fn sr_65_single_signature_keys_are_refused_in_every_form() {
    let k = main_key(3);
    let fp = k.fingerprint;
    let versions: [([u8; 4], &str); 4] = [
        ([0x04, 0x9D, 0x7C, 0xB2], "ypub"),
        ([0x04, 0xB2, 0x47, 0x46], "zpub"),
        ([0x04, 0x4A, 0x52, 0x62], "upub"),
        ([0x04, 0x5F, 0x1C, 0xF6], "vpub"),
    ];
    for (version, name) in versions {
        let key = reversioned(&k, version);
        assert!(key.starts_with(name), "{key}");
        for text in [
            format!("[{fp}/48h/0h/0h/2h]{key}"),
            format!("[{fp}/84h/0h/0h]{key}"),
            key.clone(),
        ] {
            assert_eq!(
                KeyPart::parse(&text),
                Err(CeremonyRefusal::SingleSignature),
                "{name}"
            );
        }
    }
    for (version, name) in [
        ([0x02, 0x95, 0xB4, 0x3F], "Ypub"),
        ([0x02, 0x42, 0x89, 0xEF], "Upub"),
    ] {
        let key = reversioned(&k, version);
        assert!(key.starts_with(name), "{key}");
        assert_eq!(
            KeyPart::parse(&format!("[{fp}/48h/0h/0h/1h]{key}")),
            Err(CeremonyRefusal::OtherScript)
        );
    }
}

#[test]
fn sr_66_key_3_must_match_the_network_and_must_not_be_key_2() {
    let (k2, k3) = (test_key(2), test_key(3));
    assert_eq!(check_key3(&k3, Some(&k2), Network::Regtest), Ok(()));
    assert_eq!(check_key3(&k3, None, Network::Signet), Ok(()));
    assert_eq!(
        check_key3(&k3, Some(&k2), Network::Bitcoin),
        Err(CeremonyRefusal::NetworkDiffers)
    );
    assert_eq!(
        check_key3(&main_key(3), None, Network::Testnet),
        Err(CeremonyRefusal::NetworkDiffers)
    );
    assert_eq!(
        check_key3(&k2, Some(&k2), Network::Regtest),
        Err(CeremonyRefusal::SameAsKey2)
    );
    // Another account key under key 2's fingerprint: the same master key.
    let same_master = KeyPart {
        fingerprint: k2.fingerprint,
        xpub: k3.xpub,
    };
    assert_eq!(
        check_key3(&same_master, Some(&k2), Network::Regtest),
        Err(CeremonyRefusal::SameAsKey2)
    );
}

#[test]
fn sr_67_key_3_must_be_one_key_of_the_wallet_in_full() {
    let k = kit();
    let k3 = test_key(3);
    assert_eq!(check_key3_in_wallet(&k3, &k.wallet), Ok(()));
    assert_eq!(
        check_key3_in_wallet(&test_key(9), &k.wallet),
        Err(CeremonyRefusal::Key3NotInWallet)
    );
    // The right account key under another fingerprint is another key.
    let wrong_fp = KeyPart {
        fingerprint: test_key(9).fingerprint,
        xpub: k3.xpub,
    };
    assert_eq!(
        check_key3_in_wallet(&wrong_fp, &k.wallet),
        Err(CeremonyRefusal::Key3NotInWallet)
    );
    // And the other way round.
    let wrong_key = KeyPart {
        fingerprint: k3.fingerprint,
        xpub: test_key(9).xpub,
    };
    assert_eq!(
        check_key3_in_wallet(&wrong_key, &k.wallet),
        Err(CeremonyRefusal::Key3NotInWallet)
    );
}

#[test]
fn sr_68_the_answer_and_the_setup_read_back_and_nothing_else_does() {
    let a = answer();
    let bytes = a.to_bytes();
    assert_eq!(&bytes[..5], b"OBMs\x01");
    assert_eq!(KeyAnswer::from_bytes(&bytes), Ok(a.clone()));
    let none = KeyAnswer { key2: None, ..a };
    assert_eq!(KeyAnswer::from_bytes(&none.to_bytes()), Ok(none));

    let s = setup();
    let bytes = s.to_bytes();
    assert_eq!(&bytes[..5], b"OBMt\x01");
    assert_eq!(WalletSetupBytes::from_bytes(&bytes), Ok(s));
}

#[test]
fn sr_68_cut_changed_extended_and_reordered_answers_are_refused() {
    let a = answer().to_bytes();
    for n in 0..a.len() {
        assert_eq!(
            KeyAnswer::from_bytes(&a[..n]),
            Err(CeremonyRefusal::AnswerFrame),
            "cut at {n}"
        );
    }
    let mut longer = a.clone();
    longer.push(0);
    assert!(KeyAnswer::from_bytes(&longer).is_err());
    for i in 0..a.len() {
        let mut flipped = a.clone();
        flipped[i] ^= 0x01;
        if let Ok(read) = KeyAnswer::from_bytes(&flipped) {
            assert_eq!(read.to_bytes(), flipped, "byte {i} read but not as written");
        }
    }
    // Key 2 and key 3 swapped inside the frame are the other keys in the
    // other slots, which the owner phone sees as different fingerprints.
    let swapped = KeyAnswer {
        key2: Some(test_key(3)),
        key3: test_key(2),
        ..answer()
    };
    assert_ne!(swapped.to_bytes(), a);
    // The same key twice is refused.
    let twice = KeyAnswer {
        key2: Some(test_key(3)),
        ..answer()
    };
    assert_eq!(
        KeyAnswer::from_bytes(&twice.to_bytes()),
        Err(CeremonyRefusal::AnswerFrame)
    );
    // A main-network key under a test network byte.
    let mixed = KeyAnswer {
        key3: main_key(3),
        ..answer()
    };
    assert_eq!(
        KeyAnswer::from_bytes(&mixed.to_bytes()),
        Err(CeremonyRefusal::AnswerFrame)
    );
    // A flag that is not 0 or 1, and a network byte that is not defined.
    let mut flag = a.clone();
    flag[6] = 2;
    assert!(KeyAnswer::from_bytes(&flag).is_err());
    let mut net = a.clone();
    net[5] = 9;
    assert!(KeyAnswer::from_bytes(&net).is_err());
    // An answer is not a setup, and a setup is not an answer.
    assert!(WalletSetupBytes::from_bytes(&a).is_err());
    assert!(KeyAnswer::from_bytes(&setup().to_bytes()).is_err());
}

#[test]
fn sr_68_cut_changed_extended_and_respelled_setups_are_refused() {
    let s = setup().to_bytes();
    for n in 0..s.len() {
        assert_eq!(
            WalletSetupBytes::from_bytes(&s[..n]),
            Err(CeremonyRefusal::SetupFrame),
            "cut at {n}"
        );
    }
    let mut longer = s.clone();
    longer.push(0);
    assert!(WalletSetupBytes::from_bytes(&longer).is_err());
    for i in 0..s.len() {
        let mut flipped = s.clone();
        flipped[i] ^= 0x01;
        if let Ok(read) = WalletSetupBytes::from_bytes(&flipped) {
            assert_eq!(read.to_bytes(), flipped, "byte {i} read but not as written");
        }
    }
    // The wallet text with `'` for `h` is the same wallet but not the form
    // the setup writes: the checksum no longer fits, and it is refused.
    let text = kit().wallet.canonical_string().to_string();
    let ticks = text.replace("48h/1h/0h/2h", "48'/1'/0'/2'");
    assert_ne!(ticks, text);
    let at = s
        .windows(text.len())
        .position(|w| w == text.as_bytes())
        .unwrap();
    let mut other = s.clone();
    other.splice(at..at + text.len(), ticks.bytes());
    let len = (ticks.len() as u16).to_le_bytes();
    other[at - 2..at].copy_from_slice(&len);
    assert!(WalletSetupBytes::from_bytes(&other).is_err());
}

#[test]
fn sr_69_the_answer_holds_public_values_and_no_secret() {
    let a = answer();
    let bytes = a.to_bytes();
    // The size is fixed by the fields: a flag, two keys and the root.
    assert_eq!(bytes.len(), 5 + 1 + 1 + 82 + 82 + 32);
    let secret = Xpriv::new_master(Network::Regtest, &[2; 32])
        .unwrap()
        .derive_priv(secp(), &"m/48'/1'/0'/2'".parse::<DerivationPath>().unwrap())
        .unwrap();
    let find = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    assert!(
        find(&test_key(2).xpub.encode()),
        "key 2's public key is there"
    );
    assert!(
        !find(&secret.private_key.secret_bytes()),
        "key 2's secret is not"
    );
    assert!(!find(&[0x22; 32]), "the identity secret is not");
}

#[test]
fn the_words_of_every_refusal_pass_the_checks_for_both_readers() {
    for r in ALL_REFUSALS {
        for who in [Who::KeyHolder, Who::Owner] {
            let w = r.words_for(who);
            assert_eq!(check(&w), Ok(()), "{r:?}");
            assert_eq!(w.who, who);
            assert_eq!(w.code, Some(r.rule()));
        }
        assert!(r.to_string().starts_with(r.rule()));
    }
}
