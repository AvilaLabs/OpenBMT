//! Tests of the owner's side of the key ceremony (`docs/OWNER-COINS-RULES.md`,
//! OC-40 to OC-46).

use super::*;
use openbmt_core::say::{check, check_distinct};

const NET: Network = Network::Regtest;

fn random(a: u8, b: u8) -> [u8; 64] {
    let mut r = [a; 64];
    r[32..].fill(b);
    r
}

fn part(seed: u8) -> KeyPart {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(NET, &[seed; 32]).unwrap();
    let account = master.derive_priv(&secp, &account_path(NET)).unwrap();
    KeyPart {
        fingerprint: master.fingerprint(&secp),
        xpub: Xpub::from_priv(&secp, &account),
    }
}

fn root(secret: u8) -> DeviceId {
    Identity::from_secret_bytes(&[secret; 32])
        .unwrap()
        .device_id()
}

fn answer() -> KeyAnswer {
    KeyAnswer {
        network: NET,
        key2: Some(part(2)),
        key3: part(3),
        device_root: root(0x22),
    }
}

fn made() -> OwnerSetup {
    OwnerSetup::make_key1(&random(0x31, 0x11), NET, false)
        .unwrap()
        .0
}

fn built() -> WalletBuilt {
    made().build_wallet(&answer(), None, 3).unwrap()
}

#[test]
fn oc_40_key_1_is_made_from_the_random_bytes_and_shows_words_only_when_asked() {
    let (a, none) = OwnerSetup::make_key1(&random(0x31, 0x11), NET, false).unwrap();
    assert!(none.is_none());
    let (b, words) = OwnerSetup::make_key1(&random(0x31, 0x11), NET, true).unwrap();
    // The same bytes make the same key; other bytes another.
    assert_eq!(a.key1_public(), b.key1_public());
    let other = OwnerSetup::make_key1(&random(0x32, 0x11), NET, false)
        .unwrap()
        .0;
    assert_ne!(a.key1_public(), other.key1_public());
    // 24 words from the first 32 bytes: the standard phrase for 0x7f.
    let (_, words7f) = OwnerSetup::make_key1(&random(0x7f, 0x11), NET, true).unwrap();
    assert_eq!(
        words7f.unwrap().join(" "),
        "legal winner thank year wave sausage worth useful legal winner thank \
         year wave sausage worth useful legal winner thank year wave sausage \
         worth title"
    );
    assert_eq!(words.unwrap().len(), 24);
    // The owner root comes from the last 32 bytes.
    assert_eq!(a.owner_root().unwrap(), root(0x11));
    assert_eq!(other.owner_root().unwrap(), root(0x11));
    // The account key is at the BIP 48 path of the network.
    assert_eq!(a.key1_account.depth, 4);
    assert_eq!(a.key1_public().origin_path(), "48h/1h/0h/2h");
    assert_eq!(a.stage(), SetupStage::KeyMade);
    let main = OwnerSetup::make_key1(&random(0x31, 0x11), Network::Bitcoin, false)
        .unwrap()
        .0;
    assert_eq!(main.key1_public().origin_path(), "48h/0h/0h/2h");
    // A failed random source is refused, and so is a secret that is not a
    // key.
    for r in [[0u8; 64], random(0, 0x11), random(0x31, 0)] {
        assert_eq!(
            OwnerSetup::make_key1(&r, NET, false).err(),
            Some(CeremonyError::Random)
        );
    }
    assert_eq!(
        OwnerSetup::make_key1(&random(0x31, 0xff), NET, false).err(),
        Some(CeremonyError::Random),
        "0xff... is above the curve order"
    );
}

#[test]
fn oc_40_the_record_holds_no_words_and_the_debug_form_hides_the_secrets() {
    let (a, words) = OwnerSetup::make_key1(&random(0x7f, 0x11), NET, true).unwrap();
    let phrase = words.unwrap().join(" ");
    let bytes = a.to_bytes();
    assert!(!String::from_utf8_lossy(&bytes).contains("legal winner"));
    assert!(!phrase.is_empty());
    assert!(!format!("{a:?}").contains("11"));
}

#[test]
fn oc_41_the_wallet_has_key_1_key_2_and_key_3_in_their_slots() {
    let b = built();
    let d = b.facts.fingerprints;
    let k1 = b.setup.key1_public();
    assert_eq!(d[0], k1.fingerprint);
    assert_eq!(d[1], part(2).fingerprint);
    assert_eq!(d[2], part(3).fingerprint);
    let desc = b.setup.description().unwrap();
    assert_eq!(desc.slots()[0].xpub, k1.xpub);
    assert_eq!(desc.slots()[1].xpub, part(2).xpub);
    assert_eq!(desc.slots()[2].xpub, part(3).xpub);
    assert_eq!(b.setup.stage(), SetupStage::WalletBuilt);
    assert_eq!(
        desc.slots()[0].origin_path.to_string(),
        "48'/1'/0'/2'",
        "full origins"
    );
}

#[test]
fn oc_41_key_2_comes_from_the_answer_or_from_text_and_never_from_both() {
    let none = KeyAnswer {
        key2: None,
        ..answer()
    };
    assert_eq!(
        made().build_wallet(&none, None, 3).err(),
        Some(CeremonyError::NoKey2)
    );
    let text = part(2).origin_text();
    let b = made().build_wallet(&none, Some(&text), 3).unwrap();
    assert_eq!(b.facts.pairs[0].holders[1], Holder::OtherSigner);
    assert_eq!(built().facts.pairs[0].holders[1], Holder::KeyApp);
    assert_eq!(
        made().build_wallet(&answer(), Some(&text), 3).err(),
        Some(CeremonyError::Key2Twice)
    );
    // The text is read under SR-64 and SR-65.
    assert_eq!(
        made()
            .build_wallet(&none, Some(&part(2).xpub.to_string()), 3)
            .err(),
        Some(CeremonyError::Key(CeremonyRefusal::NoOrigin))
    );
    assert_eq!(
        made().build_wallet(&none, Some("junk"), 3).err(),
        Some(CeremonyError::Key(CeremonyRefusal::NotAKey))
    );
}

#[test]
fn oc_41_the_network_and_the_roots_and_the_keys_must_fit() {
    // The answer is for another network.
    let other = KeyAnswer {
        network: Network::Signet,
        ..answer()
    };
    assert_eq!(
        made().build_wallet(&other, None, 3).err(),
        Some(CeremonyError::Key(CeremonyRefusal::NetworkDiffers))
    );
    // A device with no key 2 knows only the kind of network.
    let kind_only = KeyAnswer {
        network: Network::Testnet,
        key2: None,
        ..answer()
    };
    assert!(made()
        .build_wallet(&kind_only, Some(&part(2).origin_text()), 3)
        .is_ok());
    let main = KeyAnswer {
        network: Network::Bitcoin,
        key2: None,
        ..answer()
    };
    assert_eq!(
        made()
            .build_wallet(&main, Some(&part(2).origin_text()), 3)
            .err(),
        Some(CeremonyError::Key(CeremonyRefusal::NetworkDiffers))
    );
    // One phone for both roots.
    let same = KeyAnswer {
        device_root: root(0x11),
        ..answer()
    };
    assert_eq!(
        made().build_wallet(&same, None, 3).err(),
        Some(CeremonyError::SameRoot)
    );
    // Key 3 is key 1, or key 2 is key 1.
    let k1 = made().key1_public();
    let dup = KeyAnswer {
        key3: k1.clone(),
        ..answer()
    };
    assert!(matches!(
        made().build_wallet(&dup, None, 3).err(),
        Some(CeremonyError::Wallet(
            WalletError::DuplicateFingerprint | WalletError::DuplicateXpub
        ))
    ));
    let dup = KeyAnswer {
        key2: Some(k1),
        ..answer()
    };
    assert!(matches!(
        made().build_wallet(&dup, None, 3).err(),
        Some(CeremonyError::Wallet(_))
    ));
}

#[test]
fn oc_42_the_setup_for_the_key_app_carries_the_wallet_and_the_owner_root() {
    let b = built();
    let s = &b.for_key_app;
    assert_eq!(s.network, NET);
    assert_eq!(s.batch_min_count, 3);
    assert_eq!(s.owner_root, root(0x11));
    assert_eq!(
        s.description.canonical_string(),
        b.setup.description().unwrap().canonical_string()
    );
    let bytes = s.to_bytes();
    assert_eq!(WalletSetupBytes::from_bytes(&bytes).as_ref(), Ok(s));
    // The words are the description's own.
    assert_eq!(b.facts.checksum_words, s.description.checksum_words());
    assert_eq!(
        b.facts.first_receive_address,
        s.description
            .address(Keychain::Receive, 0)
            .unwrap()
            .to_string()
    );
}

#[test]
fn oc_43_every_pair_of_keys_is_listed_with_who_holds_each() {
    let p = built().facts.pairs;
    assert_eq!(p.map(|p| p.keys), [[1, 2], [1, 3], [2, 3]]);
    assert_eq!(
        p[0].holders,
        [Holder::OwnerPhone, Holder::KeyApp],
        "keys 1 and 2"
    );
    assert_eq!(p[1].holders, [Holder::OwnerPhone, Holder::OtherWalletApp]);
    assert_eq!(p[2].holders, [Holder::KeyApp, Holder::OtherWalletApp]);
    assert_eq!(check(&pairs_words()), Ok(()));
}

#[test]
fn oc_44_the_record_reads_back_at_every_stage_and_nothing_else_does() {
    let a = made();
    let b = built().setup;
    let c = b.finish(6, 800_000).unwrap();
    for s in [a, b, c] {
        let bytes = s.to_bytes();
        assert_eq!(&bytes[..5], b"OBMu\x01");
        let back = OwnerSetup::from_bytes(&bytes).unwrap();
        assert_eq!(back.to_bytes(), bytes);
        assert_eq!(back.stage(), s.stage());
        for n in 0..bytes.len() {
            assert_eq!(
                OwnerSetup::from_bytes(&bytes[..n]).err(),
                Some(CeremonyError::Record),
                "cut at {n}"
            );
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(OwnerSetup::from_bytes(&longer).is_err());
        for i in 0..bytes.len() {
            let mut flipped = bytes.clone();
            flipped[i] ^= 0x01;
            if let Ok(read) = OwnerSetup::from_bytes(&flipped) {
                assert_eq!(read.to_bytes(), flipped, "byte {i}");
            }
        }
    }
}

#[test]
fn oc_44_a_record_whose_parts_do_not_belong_together_is_refused() {
    let good = built().setup.to_bytes();
    let stage_at = 5 + 1 + 32 + 4 + 78;
    assert_eq!(good[stage_at], 2);
    // Another key 1 under the same wallet.
    let other = OwnerSetup::make_key1(&random(0x41, 0x11), NET, false)
        .unwrap()
        .0
        .to_bytes();
    let mut mixed = good.clone();
    mixed[38..120].copy_from_slice(&other[38..120]);
    assert_eq!(
        OwnerSetup::from_bytes(&mixed).err(),
        Some(CeremonyError::Record)
    );
    // The Key app's root is the owner's root.
    let mut same = good.clone();
    let own = made().owner_root().unwrap().to_bytes();
    same[stage_at + 1..stage_at + 33].copy_from_slice(&own);
    assert_eq!(
        OwnerSetup::from_bytes(&same).err(),
        Some(CeremonyError::Record)
    );
    // A stage that is not 1 to 3, and a refund depth out of range.
    let mut stage = good.clone();
    stage[stage_at] = 4;
    assert!(OwnerSetup::from_bytes(&stage).is_err());
    let done = built().setup.finish(6, 1).unwrap().to_bytes();
    let depth_at = done.len() - 8;
    for bad in [0u32, 101] {
        let mut d = done.clone();
        d[depth_at..depth_at + 4].copy_from_slice(&bad.to_le_bytes());
        assert!(OwnerSetup::from_bytes(&d).is_err(), "{bad}");
    }
}

#[test]
fn oc_45_the_setup_finishes_from_a_built_wallet_and_gives_the_start_values() {
    let key_made = made();
    assert_eq!(
        key_made.finish(6, 0).err(),
        Some(CeremonyError::Stage(SetupStage::KeyMade))
    );
    assert!(key_made.start_fields().is_err());
    let b = built().setup;
    assert!(b.start_fields().is_err(), "not confirmed yet");
    for bad in [0, 101, u32::MAX] {
        assert_eq!(
            b.finish(bad, 0).err(),
            Some(CeremonyError::RefundDepth),
            "{bad}"
        );
    }
    let done = b.finish(6, 812_345).unwrap();
    assert_eq!(done.stage(), SetupStage::Finished);
    assert_eq!(
        done.finish(6, 1).err(),
        Some(CeremonyError::Stage(SetupStage::Finished))
    );
    assert_eq!(
        done.build_wallet(&answer(), None, 3).err(),
        Some(CeremonyError::Stage(SetupStage::Finished))
    );
    let f = done.start_fields().unwrap();
    assert_eq!(f.network, NET);
    assert_eq!(f.owner_secret, [0x11; 32]);
    assert_eq!(f.approver_root, root(0x22).to_bytes());
    assert_eq!(
        f.wallet,
        done.description().unwrap().canonical_string().to_string()
    );
    assert_eq!(f.key3_xpub, part(3).xpub.to_string());
    assert_eq!(f.batch_min_count, 3);
    assert_eq!((f.refund_depth, f.birthday), (6, 812_345));
    let account = Xpriv::decode(&f.key1_account).unwrap();
    assert_eq!(
        Xpub::from_priv(&Secp256k1::new(), &account),
        done.key1_public().xpub
    );
    // The wallet can be built again while it waits for the confirmation.
    assert!(b.build_wallet(&answer(), None, 4).is_ok());
}

fn error_name(e: &CeremonyError) -> &'static str {
    match e {
        CeremonyError::Key(_) => "Key",
        CeremonyError::Random => "Random",
        CeremonyError::NoKey2 => "NoKey2",
        CeremonyError::Key2Twice => "Key2Twice",
        CeremonyError::SameRoot => "SameRoot",
        CeremonyError::Wallet(_) => "Wallet",
        CeremonyError::Record => "Record",
        CeremonyError::Stage(_) => "Stage",
        CeremonyError::RefundDepth => "RefundDepth",
    }
}

#[test]
fn sc_01_every_variant_has_words_and_the_words_keep_the_rules() {
    let all = all_words();
    let named: std::collections::BTreeSet<&str> = all
        .iter()
        .filter_map(|(n, _)| n.strip_prefix("CeremonyError::"))
        .map(|n| n.split('(').next().unwrap_or(n))
        .collect();
    for e in [
        CeremonyError::Key(CeremonyRefusal::NoKey3),
        CeremonyError::Random,
        CeremonyError::NoKey2,
        CeremonyError::Key2Twice,
        CeremonyError::SameRoot,
        CeremonyError::Wallet(WalletError::DuplicateXpub),
        CeremonyError::Record,
        CeremonyError::Stage(SetupStage::KeyMade),
        CeremonyError::RefundDepth,
    ] {
        assert!(named.contains(error_name(&e)), "{e:?}");
        assert!(e.to_string().starts_with(e.rule()));
        assert_eq!(check(&e.words()), Ok(()));
    }
    for (name, w) in &all {
        assert_eq!(check(w), Ok(()), "{name}");
    }
    assert_eq!(check_distinct(&all), Ok(()));
    for stage in ["KeyMade", "WalletBuilt", "Finished", "Pairs"] {
        assert!(all.iter().any(|(n, _)| n == &format!("Setup::{stage}")));
    }
}
