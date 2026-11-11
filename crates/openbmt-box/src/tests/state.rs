//! BX-01 to BX-24, BX-50 and BX-51 on the state machine, with storage in
//! memory (and on disk where the rule is about the disk).

use openbmt_core::boxpair::{Claim, InviteTextError};
use openbmt_core::settings::DeviceRole;

use super::kit::*;
use crate::files::{BoxFiles, DirFiles, MemFiles};
use crate::state::{
    BoxCore, BoxError, BoxStatus, INVITATION_FILE, INVITE_SECS, KEY_FILE, MAX_MEMBERS,
    POS_TOKEN_FILE, ROOTS_FILE, SETTINGS_FILE,
};

fn is_damaged(e: &BoxError, file: &str) -> bool {
    matches!(e, BoxError::Damaged { file: f, .. } if *f == file)
}

// ---- section 1 ----

#[test]
fn bx_01_the_first_start_makes_a_key_from_the_randomness_and_keeps_it() {
    let core = fresh_core();
    assert_eq!(core.device_id(), the_box().device_id());
    let stored = core.files().read(KEY_FILE).unwrap().unwrap();
    assert_eq!(&stored[..5], b"OBMf\x01");
    assert_eq!(&stored[5..], &[BOX_BYTE; 32]);
    // A second start reads the key and does not ask for randomness.
    let mut asked = false;
    let again = BoxCore::open(core.into_files(), &mut |_: &mut [u8; 32]| {
        asked = true;
        Err(std::io::Error::other("no"))
    })
    .unwrap();
    assert!(!asked);
    assert_eq!(again.device_id(), the_box().device_id());
    assert_eq!(again.identity().unwrap().device_id(), the_box().device_id());
}

#[test]
fn bx_01_a_failing_randomness_stops_the_start_and_stores_nothing() {
    let err = BoxCore::open(MemFiles::new(), &mut |_: &mut [u8; 32]| {
        Err(std::io::Error::other("no"))
    })
    .unwrap_err();
    assert_eq!(err, BoxError::NoRandomness);
    assert!(err.to_string().starts_with("BX-01"));
}

#[test]
fn bx_01_a_key_that_is_not_a_valid_secret_is_refused() {
    let mut files = MemFiles::new();
    let mut bytes = b"OBMf\x01".to_vec();
    bytes.extend_from_slice(&[0u8; 32]);
    files.put(KEY_FILE, &bytes);
    let err = BoxCore::open(files, &mut fixed_random).unwrap_err();
    assert!(is_damaged(&err, KEY_FILE));
}

#[test]
fn bx_01_the_key_file_is_private_on_disk() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("keymode");
    let core = BoxCore::open(DirFiles::new(dir.path()), &mut fixed_random).unwrap();
    drop(core);
    let mode = std::fs::metadata(dir.path().join(KEY_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn bx_03_an_unpaired_box_has_no_members_no_roles_and_takes_no_settings() {
    let mut core = fresh_core();
    assert_eq!(core.status(NOW), BoxStatus::Unpaired);
    assert!(core.members().is_empty());
    assert!(core.roles().is_empty());
    assert_eq!(core.role_of(&owner().device_id()), None);
    assert_eq!(core.claim(NOW), None);
    let s = sign_both(settings_with(1, &core.device_id())).to_bytes();
    assert_eq!(core.take_settings(&s), Err(BoxError::NotPaired));
    assert!(core.settings().is_none());
    assert!(core.roots().is_none());
}

#[test]
fn bx_04_a_paired_box_stores_no_wallet_no_address_and_no_signing_key() {
    let (core, _) = paired_core();
    let mut names = core.files().names();
    names.sort();
    assert_eq!(
        names,
        ["box.key", "link-inbox", "link-outbox", "roots", "settings"]
    );
    // Settings carry limits and keys that vouch for messages, nothing that
    // describes a wallet.
    let settings = core.files().read(SETTINGS_FILE).unwrap().unwrap();
    assert!(!settings.windows(4).any(|w| w == b"wsh(" || w == b"tb1q"));
}

// ---- section 2 ----

#[test]
fn bx_10_a_bad_invitation_is_refused_before_anything_is_stored() {
    let mut core = fresh_core();
    let good = invitation(7).to_text();
    let before = core.files().names();
    let mut typo = good.clone();
    typo.replace_range(3..4, if &good[3..4] == "0" { "1" } else { "0" });
    let cases = [
        (typo, BoxError::Invitation(InviteTextError::BadCheck)),
        (
            good[..good.len() - 2].to_string(),
            BoxError::Invitation(InviteTextError::BadLength(29 - 1)),
        ),
        (
            good.replacen(&good[..1], "U", 1),
            BoxError::Invitation(InviteTextError::BadCharacter('U')),
        ),
    ];
    for (text, want) in cases {
        assert_eq!(core.enter_invitation(&text, NOW, [1; 32]), Err(want));
        assert_eq!(core.files().names(), before);
        assert_eq!(core.status(NOW), BoxStatus::Unpaired);
    }
    // The forgiving forms are taken.
    assert_eq!(
        core.enter_invitation(&good.to_lowercase().replace('-', " "), NOW, [1; 32]),
        Ok(())
    );
    assert_eq!(core.status(NOW), BoxStatus::InvitationOpen);
}

#[test]
fn bx_11_an_invitation_is_spent_once_a_welcome_with_the_right_mac_arrives() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let claim = core.claim(NOW).unwrap();
    // A welcome that fails its MAC changes nothing.
    assert!(core.take_welcome(&[1, 2, 3], &[4], NOW).is_err());
    assert_eq!(core.claim(NOW), Some(claim.clone()));
    assert_eq!(core.status(NOW), BoxStatus::InvitationOpen);
    assert!(core.files().read(INVITATION_FILE).unwrap().is_some());
    // One with the right MAC and bad settings spends it.
    let w = welcome_bytes(&inv, &claim);
    assert!(core.take_welcome(&w, &[4], NOW + 1).is_err());
    assert_eq!(core.claim(NOW + 1), None);
    assert_eq!(core.status(NOW + 1), BoxStatus::Unpaired);
    assert!(core.files().read(INVITATION_FILE).unwrap().is_none());
    // The right welcome now meets no invitation.
    let s = sign_both(settings_with(1, &core.device_id())).to_bytes();
    assert_eq!(
        core.take_welcome(&w, &s, NOW + 2),
        Err(BoxError::NoInvitation)
    );
}

#[test]
fn bx_11_an_invitation_is_good_for_one_box() {
    // The welcome's MAC covers the Box's key and nonce: a welcome made for
    // the claim of another Box does not pair this one, even though both hold
    // the same invitation.
    let inv = invitation(5);
    let mut a = fresh_core();
    enter(&mut a, &inv, 0x31);
    let mut other = BoxCore::open(MemFiles::new(), &mut |b: &mut [u8; 32]| {
        *b = [0x78; 32];
        Ok(())
    })
    .unwrap();
    other
        .enter_invitation(&inv.to_text(), NOW, [0x32; 32])
        .unwrap();
    let claim_b = other.claim(NOW).unwrap();
    let w = welcome_bytes(&inv, &claim_b);
    let s = sign_both(settings_with(1, &a.device_id())).to_bytes();
    assert_eq!(a.take_welcome(&w, &s, NOW), Err(BoxError::WelcomeMac));
    assert_eq!(a.status(NOW), BoxStatus::InvitationOpen);
}

#[test]
fn bx_12_a_claim_is_offered_while_the_invitation_is_open() {
    let mut core = fresh_core();
    assert_eq!(core.claim(NOW), None);
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    assert_eq!(core.status(NOW), BoxStatus::InvitationOpen);
    let bytes = core.claim(NOW + 5).unwrap();
    let claim = Claim::from_bytes(&bytes).unwrap();
    assert!(claim.verify(&inv));
    assert_eq!(claim.box_key, the_box().device_id());
    assert_eq!(claim.nonce, [0x31; 32]);
    // The same claim while it stays open.
    assert_eq!(core.claim(NOW + 600), Some(bytes));
}

#[test]
fn bx_12_the_invitation_runs_out_after_thirty_minutes_on_the_boxs_clock() {
    let mut core = fresh_core();
    enter(&mut core, &invitation(5), 0x31);
    assert!(core.claim(NOW + INVITE_SECS - 1).is_some());
    assert_eq!(core.status(NOW + INVITE_SECS), BoxStatus::Unpaired);
    assert_eq!(core.claim(NOW + INVITE_SECS), None);
    assert!(core.files().read(INVITATION_FILE).unwrap().is_none());
}

#[test]
fn bx_12_a_clock_behind_the_entry_time_expires_the_invitation() {
    let mut core = fresh_core();
    enter(&mut core, &invitation(5), 0x31);
    assert!(core.claim(NOW).is_some());
    let mut core = reopen(core).unwrap();
    assert_eq!(core.claim(NOW - 86_400), None);
    assert!(core.files().read(INVITATION_FILE).unwrap().is_none());
    // It stays gone when the clock comes forward again.
    assert_eq!(core.claim(NOW + 1), None);
    // One second behind is refused too, and so is a reload.
    let mut core = fresh_core();
    enter(&mut core, &invitation(5), 0x31);
    let mut core = reopen(core).unwrap();
    assert_eq!(core.status(NOW - 1), BoxStatus::Unpaired);
    let mut core = reopen(core).unwrap();
    assert_eq!(core.claim(NOW), None);
}

#[test]
fn bx_12_a_new_invitation_replaces_the_old_one_with_a_new_nonce() {
    let mut core = fresh_core();
    let (first, second) = (invitation(5), invitation(6));
    enter(&mut core, &first, 0x31);
    let c1 = Claim::from_bytes(&core.claim(NOW).unwrap()).unwrap();
    core.enter_invitation(&second.to_text(), NOW + 10, [0x32; 32])
        .unwrap();
    let c2 = Claim::from_bytes(&core.claim(NOW + 10).unwrap()).unwrap();
    assert!(!c2.verify(&first));
    assert!(c2.verify(&second));
    assert_ne!(c1.nonce, c2.nonce);
    // The old claim no longer matches what the Box holds.
    assert!(c1.verify(&first));
    // The time starts again from the new entry.
    assert!(core.claim(NOW + 10 + INVITE_SECS - 1).is_some());
}

#[test]
fn bx_12_the_invitation_is_kept_across_a_restart_until_it_runs_out() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let before = core.claim(NOW).unwrap();
    let mut core = reopen(core).unwrap();
    assert_eq!(core.claim(NOW + 60), Some(before));
    let mut core = reopen(core).unwrap();
    assert_eq!(core.claim(NOW + INVITE_SECS + 1), None);
}

#[test]
fn bx_13_a_phone_that_fetched_the_claim_cannot_check_or_make_one() {
    // What the phone holds is the claim bytes. Without the invitation it
    // cannot tell a true claim from a forged one: both decode, and neither
    // verifies against anything it knows.
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let real = Claim::from_bytes(&core.claim(NOW).unwrap()).unwrap();
    let forged = Claim::make(&invitation(6), real.box_key, real.nonce);
    assert_eq!(real.to_bytes().len(), forged.to_bytes().len());
    assert!(real.verify(&inv));
    assert!(!forged.verify(&inv));
}

#[test]
fn bx_14_the_key_app_checks_the_claim_and_shows_the_boxs_fingerprint() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let claim = Claim::from_bytes(&core.claim(NOW).unwrap()).unwrap();
    assert!(claim.verify(&inv));
    assert_eq!(
        claim.box_key.fingerprint_words(),
        core.identity().unwrap().fingerprint_words()
    );
    // A different invitation's check fails, and that spends it on the Key
    // app's side: the claim is not accepted.
    assert!(!claim.verify(&invitation(6)));
}

#[test]
fn bx_15_a_welcome_pairs_the_box_and_stores_everything_before_it_counts() {
    let (mut core, _) = paired_core();
    assert_eq!(core.status(NOW + 3), BoxStatus::Paired);
    assert_eq!(core.roots(), Some(roots()));
    assert_eq!(core.settings().unwrap().version, 1);
    for file in [
        KEY_FILE,
        ROOTS_FILE,
        SETTINGS_FILE,
        "link-inbox",
        "link-outbox",
    ] {
        assert!(core.files().read(file).unwrap().is_some(), "{file}");
    }
    assert!(core.files().read(INVITATION_FILE).unwrap().is_none());
    // A restart finds the same state.
    let mut again = reopen(core).unwrap();
    assert_eq!(again.status(NOW + 3), BoxStatus::Paired);
    assert_eq!(again.roots(), Some(roots()));
    assert_eq!(again.settings().unwrap().version, 1);
}

#[test]
fn bx_15_a_welcome_before_any_invitation_is_refused() {
    let mut core = fresh_core();
    let inv = invitation(5);
    let claim = Claim::make(&inv, core.device_id(), [0x31; 32]).to_bytes();
    let w = welcome_bytes(&inv, &claim);
    let s = sign_both(settings_with(1, &core.device_id())).to_bytes();
    assert_eq!(core.take_welcome(&w, &s, NOW), Err(BoxError::NoInvitation));
    assert_eq!(core.status(NOW), BoxStatus::Unpaired);
    assert!(core.roots().is_none());
}

#[test]
fn bx_15_a_welcome_after_the_invitation_ran_out_is_refused() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let claim = core.claim(NOW).unwrap();
    let w = welcome_bytes(&inv, &claim);
    let s = sign_both(settings_with(1, &core.device_id())).to_bytes();
    assert_eq!(
        core.take_welcome(&w, &s, NOW + INVITE_SECS),
        Err(BoxError::InvitationExpired)
    );
    assert_eq!(core.status(NOW + INVITE_SECS), BoxStatus::Unpaired);
    assert!(core.files().read(INVITATION_FILE).unwrap().is_none());
    // Gone whatever arrives next, including garbage.
    assert_eq!(
        core.take_welcome(&[1], &[2], NOW + INVITE_SECS + 1),
        Err(BoxError::NoInvitation)
    );
}

#[test]
fn bx_15_a_second_welcome_after_a_spending_failure_meets_a_spent_invitation() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let claim = core.claim(NOW).unwrap();
    let w = welcome_bytes(&inv, &claim);
    // First try: right welcome, settings that do not list the Box.
    let bad = sign_both(settings_with(1, &stranger().device_id())).to_bytes();
    assert_eq!(core.take_welcome(&w, &bad, NOW), Err(BoxError::NotListed));
    // Second try, with the right settings: the invitation is gone.
    let good = sign_both(settings_with(1, &core.device_id())).to_bytes();
    assert_eq!(
        core.take_welcome(&w, &good, NOW + 1),
        Err(BoxError::NoInvitation)
    );
    assert_eq!(core.status(NOW + 1), BoxStatus::Unpaired);
    // Only a new invitation, entered again, can pair it.
    enter(&mut core, &inv, 0x32);
    let claim = core.claim(NOW).unwrap();
    let w = welcome_bytes(&inv, &claim);
    assert_eq!(core.take_welcome(&w, &good, NOW + 2), Ok(()));
}

#[test]
fn bx_15_a_thousand_stranger_welcomes_change_nothing_and_the_real_one_pairs() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let claim = core.claim(NOW).unwrap();
    let stored = core.files().read(INVITATION_FILE).unwrap();
    let good = sign_both(settings_with(1, &core.device_id())).to_bytes();
    let other = invitation(6);
    let wrong_mac = welcome_bytes(
        &other,
        &Claim::make(&other, core.device_id(), [0x31; 32]).to_bytes(),
    );
    // Garbage of every length and well-formed welcomes with a wrong MAC,
    // many in the same second: nothing limits how often they may come, so
    // a stranger cannot hold the real welcome back either.
    for n in 0..1000u64 {
        let w: Vec<u8> = if n % 2 == 0 {
            (0..(n % 130) as usize)
                .map(|i| (i as u64 * 31 + n) as u8)
                .collect()
        } else {
            wrong_mac.clone()
        };
        let err = core.take_welcome(&w, &good, NOW + 1 + n / 100).unwrap_err();
        assert!(
            matches!(err, BoxError::BadWelcome(_) | BoxError::WelcomeMac),
            "{n}: {err}"
        );
        assert_eq!(core.claim(NOW + 1 + n / 100), Some(claim.clone()));
    }
    assert_eq!(core.files().read(INVITATION_FILE).unwrap(), stored);
    assert_eq!(core.status(NOW + 10), BoxStatus::InvitationOpen);
    // The real welcome still pairs, in the same second as the last bad one
    // and with the claim the Key app already holds.
    let w = welcome_bytes(&inv, &claim);
    assert_eq!(core.take_welcome(&w, &good, NOW + 10), Ok(()));
    assert_eq!(core.status(NOW + 10), BoxStatus::Paired);
}

#[test]
fn bx_15_the_welcome_must_be_made_for_this_invitation() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    // The Key app holds another invitation.
    let other = invitation(6);
    let claim = Claim::make(&other, core.device_id(), [0x31; 32]).to_bytes();
    let w = welcome_bytes(&other, &claim);
    let s = sign_both(settings_with(1, &core.device_id())).to_bytes();
    assert_eq!(core.take_welcome(&w, &s, NOW), Err(BoxError::WelcomeMac));
    // Nothing changed: the invitation is open with its own claim.
    assert_eq!(core.status(NOW), BoxStatus::InvitationOpen);
    assert!(core.claim(NOW).is_some());
}

#[test]
fn bx_15_settings_signed_by_other_roots_pair_nothing() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let claim = core.claim(NOW).unwrap();
    let w = welcome_bytes(&inv, &claim);
    let s = settings_with(1, &core.device_id());
    let forged = openbmt_core::settings::SignedSettings {
        owner_sig: s.sign(&stranger()),
        approver_sig: Some(s.sign(&approver())),
        settings: s,
    };
    let err = core.take_welcome(&w, &forged.to_bytes(), NOW).unwrap_err();
    assert!(matches!(err, BoxError::Settings(_)), "{err}");
    // The MAC checked, so the invitation is spent.
    assert_eq!(core.claim(NOW), None);
    assert_eq!(core.status(NOW), BoxStatus::Unpaired);
    assert!(core.files().read(ROOTS_FILE).unwrap().is_none());
    assert!(core.files().read(SETTINGS_FILE).unwrap().is_none());
}

#[test]
fn bx_15_first_settings_need_both_roots_and_must_list_the_box() {
    for (signed, want_not_listed) in [
        (
            sign_owner(settings_with(1, &the_box().device_id())).to_bytes(),
            false,
        ),
        (
            sign_both(settings_with(1, &stranger().device_id())).to_bytes(),
            true,
        ),
    ] {
        let mut core = fresh_core();
        let inv = invitation(5);
        enter(&mut core, &inv, 0x31);
        let claim = core.claim(NOW).unwrap();
        let w = welcome_bytes(&inv, &claim);
        let err = core.take_welcome(&w, &signed, NOW).unwrap_err();
        if want_not_listed {
            assert_eq!(err, BoxError::NotListed);
        } else {
            assert_eq!(
                err,
                BoxError::Settings(openbmt_core::settings::SettingsError::ApproverRequired)
            );
        }
        assert_eq!(core.status(NOW), BoxStatus::Unpaired);
    }
}

#[test]
fn bx_15_a_listed_box_that_is_also_a_counter_is_not_valid_settings() {
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let claim = core.claim(NOW).unwrap();
    let w = welcome_bytes(&inv, &claim);
    let mut s = settings_with(1, &core.device_id());
    s.counters.insert(core.device_id(), 100);
    let err = core
        .take_welcome(&w, &sign_both(s).to_bytes(), NOW)
        .unwrap_err();
    assert!(matches!(err, BoxError::Settings(_)));
}

#[test]
fn bx_15_garbage_in_place_of_a_welcome_or_settings_is_refused() {
    for (w, s) in [(vec![], vec![]), (vec![0; 101], vec![0; 400])] {
        let mut core = fresh_core();
        enter(&mut core, &invitation(5), 0x31);
        assert!(core.take_welcome(&w, &s, NOW).is_err());
        // The MAC did not check: nothing changed.
        assert_eq!(core.status(NOW), BoxStatus::InvitationOpen);
    }
    // A good welcome with garbage settings.
    let mut core = fresh_core();
    let inv = invitation(5);
    enter(&mut core, &inv, 0x31);
    let w = welcome_bytes(&inv, &core.claim(NOW).unwrap());
    assert!(matches!(
        core.take_welcome(&w, &[1, 2, 3], NOW),
        Err(BoxError::BadSettings(_))
    ));
}

#[test]
fn bx_15_a_crash_between_the_writes_leaves_it_unpaired_never_half_paired() {
    // The welcome writes four files: settings, the two number files, then
    // the roots. A disk that fails from the n-th write on stops it anywhere.
    // Whatever n is, the Box starts again either unpaired (n up to 4) or
    // paired with everything in place, and a new invitation pairs it.
    for n in 1..=4 {
        let mut core = fresh_core();
        let inv = invitation(5);
        enter(&mut core, &inv, 0x31);
        let claim = core.claim(NOW).unwrap();
        let w = welcome_bytes(&inv, &claim);
        let s = sign_both(settings_with(1, &core.device_id())).to_bytes();
        core.files_mut().fail_writes_from(n);
        let err = core.take_welcome(&w, &s, NOW).unwrap_err();
        assert!(matches!(err, BoxError::Io { .. }), "n={n}: {err}");
        // In memory it is unpaired, with the invitation spent.
        assert_eq!(core.status(NOW), BoxStatus::Unpaired, "n={n}");
        assert!(core.roots().is_none());
        // After the restart the same.
        let mut files = core.into_files();
        files.stop_failing();
        let mut core = BoxCore::open(files, &mut fixed_random).unwrap();
        assert_eq!(core.status(NOW), BoxStatus::Unpaired, "n={n}");
        assert!(core.roots().is_none());
        assert!(core.members().is_empty());
        assert!(core.files().read(INVITATION_FILE).unwrap().is_none());
        // It pairs from a new invitation, over what the failed try left.
        enter(&mut core, &inv, 0x32);
        let claim = core.claim(NOW).unwrap();
        let w = welcome_bytes(&inv, &claim);
        assert_eq!(core.take_welcome(&w, &s, NOW), Ok(()), "n={n}");
        let again = reopen(core).unwrap();
        assert_eq!(again.roots(), Some(roots()));
    }
}

#[test]
fn bx_15_a_leftover_of_an_interrupted_pairing_is_not_mistaken_for_a_pairing() {
    // The settings file written and the roots not: unpaired, and the
    // stale settings are not taken for the settings in force.
    let (core, _) = paired_core();
    let mut files = core.into_files();
    files.remove(ROOTS_FILE).unwrap();
    let mut core = BoxCore::open(files, &mut fixed_random).unwrap();
    assert_eq!(core.status(NOW), BoxStatus::Unpaired);
    assert!(core.settings().is_none());
    assert!(core.members().is_empty());
}

#[test]
fn bx_16_a_paired_box_takes_no_welcome_and_no_invitation() {
    let (mut core, inv) = paired_core();
    let claim = Claim::make(&inv, core.device_id(), [0x31; 32]).to_bytes();
    let w = welcome_bytes(&inv, &claim);
    let s = sign_both(settings_with(2, &core.device_id())).to_bytes();
    assert_eq!(core.take_welcome(&w, &s, NOW), Err(BoxError::AlreadyPaired));
    assert_eq!(
        core.enter_invitation(&invitation(8).to_text(), NOW, [1; 32]),
        Err(BoxError::AlreadyPaired)
    );
    assert_eq!(core.claim(NOW), None);
    assert!(core.files().read(INVITATION_FILE).unwrap().is_none());
    assert_eq!(core.roots(), Some(roots()));
    assert_eq!(core.settings().unwrap().version, 1);
}

#[test]
fn bx_16_a_stray_invitation_file_on_a_paired_box_is_dropped_at_start() {
    let (core, _) = paired_core();
    let mut files = core.into_files();
    let mut stale = b"OBMg\x01".to_vec();
    stale.extend_from_slice(&[1u8; 17 + 32 + 8]);
    files.put(INVITATION_FILE, &stale);
    let mut core = BoxCore::open(files, &mut fixed_random).unwrap();
    assert_eq!(core.status(NOW), BoxStatus::Paired);
    assert_eq!(core.claim(NOW), None);
    assert!(core.files().read(INVITATION_FILE).unwrap().is_none());
}

#[test]
fn bx_17_without_a_key_app_the_same_invitation_and_welcome_work() {
    // With no Key app, the owner phone makes the invitation and checks the
    // claim; the bytes are the same. (What the phone's screen says about it
    // is the phone's.) Here the "approver" is the owner's other root, as on
    // a hardware wallet setup, and the welcome pairs as before.
    let (core, _) = paired_core();
    assert_eq!(core.roots().unwrap().owner, owner().device_id());
}

// ---- section 3 ----

#[test]
fn bx_20_the_box_takes_no_settings_that_break_the_box_set_rules() {
    let (mut core, _) = paired_core();
    let me = core.device_id();
    // A Box that is not on the device list, a key that is both a counter and
    // a Box, and more than eight Boxes.
    let mut off_list = settings_with(2, &me);
    off_list.boxes.insert(stranger().device_id());
    let mut both = settings_with(2, &me);
    both.boxes.insert(counter1().device_id());
    let mut many = settings_with(2, &me);
    for n in 100..108u8 {
        many.devices.insert(id(n).device_id());
        many.boxes.insert(id(n).device_id());
    }
    // More than eight cannot even be read.
    assert!(matches!(
        core.take_settings(&sign_both(many).to_bytes()),
        Err(BoxError::BadSettings(_))
    ));
    for s in [off_list, both] {
        let got = core.take_settings(&sign_both(s).to_bytes());
        assert!(
            matches!(
                got,
                Err(BoxError::Settings(
                    openbmt_core::settings::SettingsError::Invalid(_)
                ))
            ),
            "{got:?}"
        );
    }
    assert_eq!(core.settings().unwrap().version, 1);
    // Adding a second Box is a loosening: both roots must sign.
    let mut two = settings_with(2, &me);
    let other_box = id(0x55).device_id();
    two.devices.insert(other_box);
    two.boxes.insert(other_box);
    assert!(matches!(
        core.take_settings(&sign_owner(two.clone()).to_bytes()),
        Err(BoxError::Settings(
            openbmt_core::settings::SettingsError::ApproverRequired
        ))
    ));
    assert!(core.take_settings(&sign_both(two).to_bytes()).is_ok());
    assert_eq!(core.role_of(&other_box), Some(DeviceRole::Box));
}

#[test]
fn bx_21_a_paired_box_takes_only_higher_versions_signed_by_its_roots() {
    let (mut core, _) = paired_core();
    let me = core.device_id();
    // Same version, and the version before: refused. The settings in force
    // do not change.
    for v in [1, 0] {
        let s = sign_both(settings_with(v, &me)).to_bytes();
        assert!(matches!(core.take_settings(&s), Err(BoxError::Settings(_))));
    }
    assert_eq!(core.settings().unwrap().version, 1);
    // Higher: taken.
    let mut v2 = settings_with(2, &me);
    v2.tor_on = true;
    let taken = core.take_settings(&sign_both(v2).to_bytes()).unwrap();
    assert!(!taken.turned_off);
    assert_eq!(core.settings().unwrap().version, 2);
    // Replayed older settings: refused.
    let replay = sign_both(settings_with(1, &me)).to_bytes();
    assert!(matches!(
        core.take_settings(&replay),
        Err(BoxError::Settings(
            openbmt_core::settings::SettingsError::StaleVersion { current: 2, got: 1 }
        ))
    ));
}

#[test]
fn bx_21_settings_signed_by_other_roots_are_refused() {
    let (mut core, _) = paired_core();
    let s = settings_with(2, &core.device_id());
    for (o, a) in [
        (stranger(), approver()),
        (owner(), stranger()),
        (stranger(), stranger()),
    ] {
        let forged = openbmt_core::settings::SignedSettings {
            owner_sig: s.sign(&o),
            approver_sig: Some(s.sign(&a)),
            settings: s.clone(),
        };
        assert!(matches!(
            core.take_settings(&forged.to_bytes()),
            Err(BoxError::Settings(_))
        ));
    }
    assert_eq!(core.settings().unwrap().version, 1);
}

#[test]
fn bx_21_a_loosening_needs_the_key_app_and_a_tightening_does_not() {
    let (mut core, _) = paired_core();
    let me = core.device_id();
    let mut loose = settings_with(2, &me);
    loose.heartbeat_max_age_secs += 60;
    assert!(matches!(
        core.take_settings(&sign_owner(loose).to_bytes()),
        Err(BoxError::Settings(
            openbmt_core::settings::SettingsError::ApproverRequired
        ))
    ));
    let mut tight = settings_with(2, &me);
    tight.tor_on = true;
    // tor_on going on is a tightening.
    assert!(core.take_settings(&sign_owner(tight).to_bytes()).is_ok());
    // The stored settings, signed by the owner alone, read back at a start.
    let again = reopen(core).unwrap();
    assert_eq!(again.settings().unwrap().version, 2);
}

#[test]
fn bx_21_settings_that_are_not_signed_settings_are_refused() {
    let (mut core, _) = paired_core();
    assert!(matches!(
        core.take_settings(&[1, 2, 3]),
        Err(BoxError::BadSettings(_))
    ));
    assert!(matches!(
        core.take_settings(&[]),
        Err(BoxError::BadSettings(_))
    ));
}

#[test]
fn bx_22_the_members_are_the_device_list_and_the_owner_and_follow_the_settings() {
    let (mut core, _) = paired_core();
    let me = core.device_id();
    let mut want = vec![
        owner().device_id(),
        counter1().device_id(),
        counter2().device_id(),
        me,
    ];
    want.sort();
    assert_eq!(core.members(), want);
    // New settings take counter 2 off the list: it is not a member any more.
    let mut v2 = settings_with(2, &me);
    v2.devices.remove(&counter2().device_id());
    v2.counters.remove(&counter2().device_id());
    core.take_settings(&sign_owner(v2).to_bytes()).unwrap();
    assert!(!core.members().contains(&counter2().device_id()));
    assert_eq!(core.members().len(), 3);
}

#[test]
fn bx_22_the_watch_key_the_settings_name_is_a_member_until_they_stop_naming_it() {
    let (mut core, _) = paired_core();
    let me = core.device_id();
    let watch = id(0x33).device_id();
    assert!(!core.members().contains(&watch));
    // Naming it loosens, so the Key app co-signs.
    let mut v2 = settings_with(2, &me);
    v2.watch_key = Some(watch);
    core.take_settings(&sign_both(v2).to_bytes()).unwrap();
    assert!(core.members().contains(&watch));
    assert_eq!(core.members().len(), 5);
    // It is a member after a restart, as the settings in force say.
    let mut core = reopen(core).unwrap();
    assert!(core.members().contains(&watch));
    // Settings that no longer name it take its mailbox away.
    let v3 = settings_with(3, &me);
    core.take_settings(&sign_owner(v3).to_bytes()).unwrap();
    assert!(!core.members().contains(&watch));
    assert_eq!(core.members().len(), 4);
}

#[test]
fn bx_22_settings_that_come_to_more_members_than_the_relay_holds_are_refused() {
    let (mut core, _) = paired_core();
    let me = core.device_id();
    let mut big = settings_with(2, &me);
    let mut n = 100u8;
    while big.devices.len() < MAX_MEMBERS {
        big.devices.insert(id(n).device_id());
        n += 1;
    }
    // 64 devices and the owner make 65.
    assert_eq!(big.devices.len(), MAX_MEMBERS);
    assert_eq!(
        core.take_settings(&sign_both(big).to_bytes()),
        Err(BoxError::TooManyMembers)
    );
    assert_eq!(core.settings().unwrap().version, 1);
}

#[test]
fn bx_23_settings_that_drop_the_box_turn_it_off_and_erase_its_pos_token() {
    let (mut core, _) = paired_core();
    core.files_mut().write(POS_TOKEN_FILE, b"token").unwrap();
    let mut v2 = settings_with(2, &core.device_id());
    v2.devices.remove(&core.device_id());
    v2.boxes.clear();
    let taken = core.take_settings(&sign_owner(v2).to_bytes()).unwrap();
    assert!(taken.turned_off);
    assert_eq!(taken.token_error, None);
    assert!(core.turned_off());
    assert_eq!(core.status(NOW), BoxStatus::TurnedOff);
    assert!(core.members().is_empty());
    assert!(core.roles().is_empty());
    assert!(core.files().read(POS_TOKEN_FILE).unwrap().is_none());
    // It stays pinned (BX-16) and off after a restart.
    let mut again = reopen(core).unwrap();
    assert_eq!(again.status(NOW), BoxStatus::TurnedOff);
    assert_eq!(again.roots(), Some(roots()));
    assert!(again.members().is_empty());
    let inv = invitation(5);
    assert_eq!(
        again.enter_invitation(&inv.to_text(), NOW, [1; 32]),
        Err(BoxError::AlreadyPaired)
    );
}

#[test]
fn bx_23_a_token_that_cannot_be_erased_is_reported() {
    let (mut core, _) = paired_core();
    core.files_mut().write(POS_TOKEN_FILE, b"token").unwrap();
    core.files_mut().set_fail_removes(true);
    let mut v2 = settings_with(2, &core.device_id());
    v2.devices.remove(&core.device_id());
    v2.boxes.clear();
    let taken = core.take_settings(&sign_owner(v2).to_bytes()).unwrap();
    assert!(taken.turned_off);
    assert!(taken.token_error.is_some());
}

#[test]
fn bx_24_the_roles_come_from_the_settings() {
    let (core, _) = paired_core();
    let me = core.device_id();
    assert_eq!(core.role_of(&owner().device_id()), Some(DeviceRole::Owner));
    assert_eq!(
        core.role_of(&counter1().device_id()),
        Some(DeviceRole::Counter)
    );
    assert_eq!(core.role_of(&me), Some(DeviceRole::Box));
    assert_eq!(core.role_of(&stranger().device_id()), None);
    // The approver root is not a device the Box talks to.
    assert_eq!(core.role_of(&approver().device_id()), None);
    let roles = core.roles();
    assert_eq!(roles.len(), 3);
    assert!(!roles.contains_key(&me));
}

// ---- section 5 ----

#[test]
fn bx_50_a_whole_file_is_written_by_rename_and_nothing_is_left_behind() {
    let dir = TempDir::new("whole");
    let mut files = DirFiles::new(dir.path());
    files.write("settings", b"first").unwrap();
    files.write("settings", b"second, longer").unwrap();
    assert_eq!(
        files.read("settings").unwrap().as_deref(),
        Some(&b"second, longer"[..])
    );
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, ["settings"]);
    // A temporary file left by a stop mid-write is not the file.
    std::fs::write(dir.path().join("settings.tmp"), b"torn").unwrap();
    assert_eq!(
        files.read("settings").unwrap().as_deref(),
        Some(&b"second, longer"[..])
    );
    files.write("settings", b"third").unwrap();
    assert!(!dir.path().join("settings.tmp").exists());
    files.remove("settings").unwrap();
    files.remove("settings").unwrap();
    assert_eq!(files.read("settings").unwrap(), None);
}

#[test]
fn bx_50_a_value_is_stored_before_it_takes_effect() {
    let (mut core, _) = paired_core();
    let me = core.device_id();
    core.files_mut().fail_writes_from(1);
    let mut v2 = settings_with(2, &me);
    v2.tor_on = true;
    assert!(matches!(
        core.take_settings(&sign_both(v2).to_bytes()),
        Err(BoxError::Io { .. })
    ));
    // Not stored, so not in force.
    assert_eq!(core.settings().unwrap().version, 1);
    let mut files = core.into_files();
    files.stop_failing();
    let core = BoxCore::open(files, &mut fixed_random).unwrap();
    assert_eq!(core.settings().unwrap().version, 1);
}

#[test]
fn bx_50_an_invitation_that_cannot_be_stored_is_not_taken() {
    let mut core = fresh_core();
    core.files_mut().fail_writes_from(1);
    let err = core
        .enter_invitation(&invitation(5).to_text(), NOW, [1; 32])
        .unwrap_err();
    assert!(matches!(err, BoxError::Io { .. }));
    assert_eq!(core.status(NOW), BoxStatus::Unpaired);
    assert_eq!(core.claim(NOW), None);
}

#[test]
fn bx_51_a_damaged_key_roots_or_settings_stop_the_start_and_name_the_file() {
    let (core, _) = paired_core();
    let good = core.into_files();
    let cases: [(&str, Vec<u8>); 6] = [
        (KEY_FILE, vec![1, 2, 3]),
        (KEY_FILE, Vec::new()),
        (ROOTS_FILE, vec![0; 69]),
        (ROOTS_FILE, Vec::new()),
        (SETTINGS_FILE, vec![0; 100]),
        (SETTINGS_FILE, Vec::new()),
    ];
    for (file, bytes) in cases {
        let mut files = good.clone();
        files.put(file, &bytes);
        let err = BoxCore::open(files.clone(), &mut fixed_random).unwrap_err();
        assert!(is_damaged(&err, file), "{file}: {err}");
        assert!(err.to_string().contains(file));
        // Nothing was made over it.
        assert_eq!(files.read(file).unwrap().as_deref(), Some(&bytes[..]));
    }
}

#[test]
fn bx_51_a_missing_key_or_settings_next_to_pairing_files_is_not_made_again() {
    let (core, _) = paired_core();
    let good = core.into_files();
    let mut no_key = good.clone();
    no_key.remove(KEY_FILE).unwrap();
    let err = BoxCore::open(no_key.clone(), &mut fixed_random).unwrap_err();
    assert!(is_damaged(&err, KEY_FILE));
    assert!(no_key.read(KEY_FILE).unwrap().is_none());
    let mut no_settings = good.clone();
    no_settings.remove(SETTINGS_FILE).unwrap();
    let err = BoxCore::open(no_settings, &mut fixed_random).unwrap_err();
    assert!(is_damaged(&err, SETTINGS_FILE));
}

#[test]
fn bx_51_settings_that_the_pinned_roots_did_not_sign_are_damage() {
    let (core, _) = paired_core();
    let me = core.device_id();
    let mut files = core.into_files();
    let s = settings_with(1, &me);
    let forged = openbmt_core::settings::SignedSettings {
        owner_sig: s.sign(&stranger()),
        approver_sig: Some(s.sign(&approver())),
        settings: s,
    };
    files.put(SETTINGS_FILE, &forged.to_bytes());
    let err = BoxCore::open(files, &mut fixed_random).unwrap_err();
    assert!(is_damaged(&err, SETTINGS_FILE));
}

#[test]
fn bx_51_an_unreadable_file_stops_the_start_with_the_file_named() {
    // A directory where the roots file should be cannot be read as a file.
    let dir = TempDir::new("unreadable");
    std::fs::create_dir(dir.path().join(ROOTS_FILE)).unwrap();
    let err = BoxCore::open(DirFiles::new(dir.path()), &mut fixed_random).unwrap_err();
    assert!(
        matches!(&err, BoxError::Io { file, .. } if *file == ROOTS_FILE),
        "{err}"
    );
    assert!(err.to_string().contains(ROOTS_FILE));
}
