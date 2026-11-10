//! SR-30 and the owner's other calls on the Key app's own screen, and
//! SR-60 to SR-63 (registration).

use std::cell::RefCell;

use bitcoin::bip32::{Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::{PubkeyHash, ScriptBuf};
use openbmt_core::wallet::{KeySlot, WalletDescription, WalletError};

use super::kit::*;
use crate::crypto::secp;
use crate::{
    Ceilings, ChangeError, ChangeRefusal, FeeCeilings, OutputKind, RegisterError, SignerState,
    FIRST_ALLOWLISTED, MAX_ALLOWLIST,
};

type Store<'a> = &'a dyn Fn(&[u8]) -> Result<(), &'static str>;

/// Run `op` against `s` with a store that fails, then with one that works.
/// The failure leaves the state untouched; the working store is handed
/// exactly the bytes of the state afterwards.
#[track_caller]
fn exercise(
    mut s: SignerState,
    op: impl Fn(&mut SignerState, Store<'_>) -> Result<(), ChangeError<&'static str>>,
) {
    let before = s.to_bytes();
    let r = op(&mut s, &|_| Err("disk full"));
    assert_eq!(r, Err(ChangeError::Persist("disk full")));
    assert_eq!(
        s.to_bytes(),
        before,
        "state changed although the store failed"
    );
    let seen = RefCell::new(None);
    op(&mut s, &|b| {
        *seen.borrow_mut() = Some(b.to_vec());
        Ok(())
    })
    .unwrap();
    let after = s.to_bytes();
    assert_ne!(after, before);
    assert_eq!(seen.into_inner().unwrap(), after);
}

fn never(_: &[u8]) -> Result<(), &'static str> {
    panic!("a refused call reached the store");
}

#[test]
fn sr_30_enrolls_only_the_five_script_kinds() {
    let mut s = state();
    let before = s.to_bytes();
    for script in [
        ScriptBuf::new(),
        ScriptBuf::from_bytes(vec![0x51]),
        ScriptBuf::new_op_return(bitcoin::script::PushBytesBuf::new()),
    ] {
        assert_eq!(
            s.enroll_destination(script, "x", NOW, never),
            Err(ChangeError::Refused(ChangeRefusal::ScriptKind))
        );
    }
    assert_eq!(s.to_bytes(), before);
    for script in [
        external(1),
        ScriptBuf::new_p2pkh(&PubkeyHash::from_byte_array([1; 20])),
    ] {
        assert!(s.enroll_destination(script, "ok", NOW, ok).is_ok());
    }
    assert_eq!(ChangeRefusal::ScriptKind.rule(), "SR-30");
}

#[test]
fn sr_30_labels_are_one_to_sixty_four_characters_without_control_characters() {
    let mut s = state();
    let bad = [
        String::new(),
        "x".repeat(65),
        "tab\there".to_string(),
        "line\nbreak".to_string(),
        "nul\0".to_string(),
        "\u{7f}".to_string(),
        "\u{85}".to_string(),
        // 64 characters of four bytes each do not fit the stored form.
        "\u{1F600}".repeat(64),
    ];
    for (n, label) in bad.iter().enumerate() {
        assert_eq!(
            s.enroll_destination(external(n as u8 + 1), label, NOW, never),
            Err(ChangeError::Refused(ChangeRefusal::InvalidLabel)),
            "{label:?}"
        );
    }
    let good = [
        "x".to_string(),
        "x".repeat(64),
        "Caf\u{e9} Bank".to_string(),
        "\u{1F600}".repeat(63),
        "\u{65e5}\u{672c}".repeat(32),
    ];
    for (n, label) in good.iter().enumerate() {
        s.enroll_destination(external(n as u8 + 100), label, NOW, ok)
            .unwrap_or_else(|e| panic!("{label:?}: {e}"));
    }
    // What was enrolled survives a snapshot.
    let k = kit();
    let back = SignerState::from_bytes(&s.to_bytes(), NET, &k.xpubs[OWN]).unwrap();
    assert_eq!(back.to_bytes(), s.to_bytes());
}

#[test]
fn sr_30_no_script_twice_and_at_most_sixty_four_entries() {
    let mut s = state();
    s.enroll_destination(external(1), "a", NOW, ok).unwrap();
    assert_eq!(
        s.enroll_destination(external(1), "b", NOW, never),
        Err(ChangeError::Refused(ChangeRefusal::AlreadyEnrolled))
    );
    for n in 2..=MAX_ALLOWLIST as u8 {
        s.enroll_destination(external(n), "x", NOW, ok).unwrap();
    }
    assert_eq!(s.allowlist().count(), MAX_ALLOWLIST);
    assert_eq!(
        s.enroll_destination(external(200), "x", NOW, never),
        Err(ChangeError::Refused(ChangeRefusal::AllowlistFull))
    );
    // A repeat is still reported as a repeat when the list is full.
    assert_eq!(
        s.enroll_destination(external(1), "x", NOW, never),
        Err(ChangeError::Refused(ChangeRefusal::AlreadyEnrolled))
    );
}

#[test]
fn sr_30_an_entry_is_active_exactly_24_hours_after_enrollment() {
    let mut s = state();
    let script = external(40);
    s.enroll_destination(script.clone(), "Cash-out", NOW, ok)
        .unwrap();
    let entry = s.allowlist().next().unwrap();
    assert_eq!(entry.enrolled_at(), NOW);
    assert_eq!(entry.active_at(), Some(NOW + 86_400));
    let p = simple_to(script, 50_000, 1_000);
    let kind = |t: u64| review_ok(&s, &p, t).outputs()[0].kind.clone();
    assert_eq!(kind(NOW), OutputKind::Other);
    assert_eq!(kind(NOW + 86_399), OutputKind::Other);
    assert_eq!(
        kind(NOW + 86_400),
        OutputKind::Allowlisted {
            label: "Cash-out".into()
        }
    );
}

#[test]
fn sr_30_removal_takes_effect_at_once() {
    let mut s = state();
    let script = external(40);
    s.enroll_destination(script.clone(), "Cash-out", NOW, ok)
        .unwrap();
    let later = NOW + 86_400;
    let p = simple_to(script.clone(), 50_000, 1_000);
    assert!(matches!(
        review_ok(&s, &p, later).outputs()[0].kind,
        OutputKind::Allowlisted { .. }
    ));
    s.remove_destination(&script, ok).unwrap();
    assert_eq!(
        review_ok(&s, &p, later).outputs()[0].kind,
        OutputKind::Other
    );
    assert_eq!(
        s.remove_destination(&script, never),
        Err(ChangeError::Refused(ChangeRefusal::NotEnrolled))
    );
}

#[test]
fn sr_30_the_allowlist_is_kept_in_script_order() {
    let mut s = state();
    for n in [30u8, 10, 20, 5, 25] {
        s.enroll_destination(external(n), "x", NOW, ok).unwrap();
    }
    let scripts: Vec<_> = s.allowlist().map(|e| e.script().to_bytes()).collect();
    let mut sorted = scripts.clone();
    sorted.sort();
    assert_eq!(scripts, sorted);
}

#[test]
fn owner_calls_store_the_new_state_first_and_leave_the_old_one_on_failure() {
    exercise(state(), |s, p| {
        s.enroll_destination(external(1), "a", NOW, |b| p(b))
    });
    let mut with_entry = state();
    with_entry
        .enroll_destination(external(1), "a", NOW, ok)
        .unwrap();
    exercise(with_entry, |s, p| {
        s.remove_destination(&external(1), |b| p(b))
    });
    exercise(state(), |s, p| {
        s.set_ceilings(
            Ceilings {
                per_tx: 1,
                per_day: 2,
                per_week: 3,
            },
            FIRST_ALLOWLISTED,
            |b| p(b),
        )
    });
    exercise(state(), |s, p| {
        s.set_fee_ceilings(
            FeeCeilings {
                sat_per_vb: 5,
                absolute_sats: 6,
            },
            |b| p(b),
        )
    });
    exercise(state(), |s, p| s.raise_change_mark(9, |b| p(b)));
    exercise(state(), |s, p| s.raise_receive_mark(9, |b| p(b)));
    let mut ahead = state();
    ahead.enroll_destination(external(1), "a", NOW, ok).unwrap();
    exercise(ahead, |s, p| s.correct_clock(NOW - 5_000, |b| p(b)));
}

#[test]
fn owner_calls_that_are_refused_never_reach_the_store() {
    let mut s = state();
    let before = s.to_bytes();
    assert!(s.raise_change_mark(0, never).is_err());
    assert!(s.raise_receive_mark(0, never).is_err());
    assert!(s
        .set_fee_ceilings(
            FeeCeilings {
                sat_per_vb: 0,
                absolute_sats: 0
            },
            never
        )
        .is_err());
    assert!(s.remove_destination(&external(1), never).is_err());
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn sr_34_the_change_mark_only_goes_up_and_stops_at_two_to_the_31() {
    let mut s = state();
    s.raise_change_mark(5, ok).unwrap();
    assert_eq!(s.change_mark(), 5);
    for lower in [0, 4, 5] {
        assert_eq!(
            s.raise_change_mark(lower, never),
            Err(ChangeError::Refused(ChangeRefusal::MarkNotHigher))
        );
    }
    assert_eq!(
        s.raise_change_mark((1 << 31) + 1, never),
        Err(ChangeError::Refused(ChangeRefusal::MarkTooHigh))
    );
    s.raise_change_mark(1 << 31, ok).unwrap();
    assert_eq!(s.change_mark(), 1 << 31);
    assert_eq!(ChangeRefusal::MarkNotHigher.rule(), "SR-34");
    assert_eq!(ChangeRefusal::MarkTooHigh.rule(), "SR-34");
}

#[test]
fn sr_70_the_receive_mark_only_goes_up_and_stops_at_two_to_the_31() {
    let mut s = state();
    s.raise_receive_mark(5, ok).unwrap();
    assert_eq!(s.receive_mark(), 5);
    // The change mark is a separate number.
    assert_eq!(s.change_mark(), 0);
    for lower in [0, 4, 5] {
        assert_eq!(
            s.raise_receive_mark(lower, never),
            Err(ChangeError::Refused(ChangeRefusal::ReceiveMarkNotHigher))
        );
    }
    assert_eq!(
        s.raise_receive_mark((1 << 31) + 1, never),
        Err(ChangeError::Refused(ChangeRefusal::ReceiveMarkTooHigh))
    );
    s.raise_receive_mark(1 << 31, ok).unwrap();
    assert_eq!(s.receive_mark(), 1 << 31);
    assert_eq!(ChangeRefusal::ReceiveMarkNotHigher.rule(), "SR-70");
    assert_eq!(ChangeRefusal::ReceiveMarkTooHigh.rule(), "SR-70");
}

// ---- registration ------------------------------------------------------

#[test]
fn sr_60_the_description_must_be_a_valid_wallet_description() {
    let k = kit();
    let own = &k.xpubs[OWN];
    let cases = [
        "",
        "wsh(sortedmulti(2,a,b,c))",
        &k.description[..k.description.len() - 1],
    ];
    for text in cases {
        let got = SignerState::register(text, NET, own, None).unwrap_err();
        assert!(matches!(got, RegisterError::Wallet(_)), "{got:?}");
        assert_eq!(got.rule(), "SR-60");
    }
    // A checksum that is right for an edited, non-canonical wallet.
    let body = k.description.rsplit_once('#').unwrap().0;
    let edited = body.replacen("sortedmulti(2", "sortedmulti(3", 1);
    let text = format!(
        "{edited}#{}",
        openbmt_core::wallet::descriptor_checksum(&edited).unwrap()
    );
    assert_eq!(
        SignerState::register(&text, NET, own, None).unwrap_err(),
        RegisterError::Wallet(WalletError::NonCanonical)
    );
    // The right text for another network.
    assert!(matches!(
        SignerState::register(&k.description, bitcoin::Network::Bitcoin, own, None),
        Err(RegisterError::Wallet(_))
    ));
}

#[test]
fn sr_60_only_four_networks_are_supported() {
    let k = kit();
    assert_eq!(
        SignerState::register(
            &k.description,
            bitcoin::Network::Testnet4,
            &k.xpubs[OWN],
            None
        )
        .unwrap_err(),
        RegisterError::UnsupportedNetwork
    );
}

#[test]
fn sr_61_exactly_one_slot_holds_this_devices_key_in_full() {
    let k = kit();
    for (n, want) in [(0usize, 1u8), (1, 2), (2, 3)] {
        let s = SignerState::register(&k.description, NET, &k.xpubs[n], None).unwrap();
        assert_eq!(s.own_slot(), want);
    }
    let foreign = Xpub::from_priv(secp(), &Xpriv::new_master(NET, &[9; 32]).unwrap());
    let got = SignerState::register(&k.description, NET, &foreign, None).unwrap_err();
    assert_eq!(got, RegisterError::OwnKeyNotInWallet);
    assert_eq!(got.rule(), "SR-61");
    // The same fingerprint is not enough: a sibling account of key 2.
    let master = Xpriv::new_master(NET, &[2; 32]).unwrap();
    let sibling = master
        .derive_priv(
            secp(),
            &"m/48'/1'/1'/2'"
                .parse::<bitcoin::bip32::DerivationPath>()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        SignerState::register(
            &k.description,
            NET,
            &Xpub::from_priv(secp(), &sibling),
            None
        )
        .unwrap_err(),
        RegisterError::OwnKeyNotInWallet
    );
}

#[test]
fn sr_62_key_three_must_be_in_exactly_one_other_slot() {
    let k = kit();
    let s = SignerState::register(&k.description, NET, &k.xpubs[OWN], Some(&k.xpubs[2])).unwrap();
    assert_eq!(s.key3_slot(), Some(3));
    let s = SignerState::register(&k.description, NET, &k.xpubs[OWN], Some(&k.xpubs[0])).unwrap();
    assert_eq!(s.key3_slot(), Some(1));
    let foreign = Xpub::from_priv(secp(), &Xpriv::new_master(NET, &[9; 32]).unwrap());
    for key3 in [&foreign, &k.xpubs[OWN]] {
        let got =
            SignerState::register(&k.description, NET, &k.xpubs[OWN], Some(key3)).unwrap_err();
        assert_eq!(got, RegisterError::Key3NotInWallet);
        assert_eq!(got.rule(), "SR-62");
    }
}

#[test]
fn sr_63_a_registered_wallet_is_never_replaced() {
    // There is no call that changes the wallet: a snapshot made for one
    // wallet is refused by a device that holds another key, and a state
    // keeps the wallet it was registered with through every change.
    let mut s = state();
    let checksum = s.wallet().checksum32();
    s.enroll_destination(external(1), "a", NOW, ok).unwrap();
    s.raise_change_mark(3, ok).unwrap();
    sign_ok(&mut s, &simple(10_000, 500), NOW);
    assert_eq!(s.wallet().checksum32(), checksum);
    let back = SignerState::from_bytes(&s.to_bytes(), NET, &kit().xpubs[OWN]).unwrap();
    assert_eq!(back.wallet().checksum32(), checksum);
    // A new wallet is a new setup with empty state.
    let other = other_wallet_state();
    assert_ne!(other.wallet().checksum32(), checksum);
    assert_eq!(other.log().count(), 0);
    assert_eq!(other.change_mark(), 0);
}

/// A state for a different wallet (other seeds).
pub fn other_wallet_state() -> SignerState {
    let path: bitcoin::bip32::DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
    let mut slots = Vec::new();
    let mut own = None;
    for n in 0..3u8 {
        let master = Xpriv::new_master(NET, &[n + 10; 32]).unwrap();
        let account = master.derive_priv(secp(), &path).unwrap();
        let xpub = Xpub::from_priv(secp(), &account);
        if n == 1 {
            own = Some(xpub);
        }
        slots.push(KeySlot {
            index: n + 1,
            origin_fingerprint: master.fingerprint(secp()),
            origin_path: path.clone(),
            xpub,
        });
    }
    let wallet = WalletDescription::new(NET, slots.try_into().unwrap()).unwrap();
    SignerState::register(wallet.canonical_string(), NET, &own.unwrap(), None).unwrap()
}
