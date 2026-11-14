//! Adding a Box, the Key app's part (AS-80 to AS-82).

mod common;

use common::*;
use openbmt_approver::{Choice, Detail, RestoreError, StepStatus};
use openbmt_core::boxpair::{BoxRoots, Claim, Invitation, Welcome, INVITATION_SECS};
use openbmt_core::identity::DeviceId;
use openbmt_session::{DeclineCode, Item, Payload, Response, Verdict};

const RANDOM: [u8; 17] = [0x42; 17];

fn box_key() -> DeviceId {
    identity([0x77; 32]).device_id()
}

fn claim_for(invitation: &Invitation) -> Claim {
    Claim::make(invitation, box_key(), [9; 32])
}

/// The settings that add the Box (version 1, no counters).
fn box_settings(version: u32) -> Item {
    let mut s = settings_with(version, &[]);
    s.devices.insert(box_key());
    s.boxes.insert(box_key());
    settings_item(s)
}

fn session_items(claim: Claim) -> Vec<Item> {
    vec![box_settings(1), Item::BoxClaim { claim }, health_item(1)]
}

/// Approve the lines of `numbers`, decline the others, and finish.
fn run(a: &mut openbmt_approver::Approver, items: Vec<Item>, approve: &[u32]) -> Response {
    static NEXT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(100);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let request = request_with(id, items);
    let store = Store::default();
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    for n in approve {
        session.decide(*n, Choice::Approve).unwrap();
    }
    let bytes = session.finish(|b| store.put(b)).unwrap();
    Response::from_bytes(&bytes).unwrap()
}

fn verdict(r: &Response, number: u32) -> Verdict {
    r.results()
        .iter()
        .find(|x| x.number == number)
        .unwrap()
        .verdict
        .clone()
}

// ---- AS-80: the invitation ---------------------------------------------------

#[test]
fn as_80_an_invitation_is_stored_before_it_is_returned_and_lasts_30_minutes() {
    let mut a = approver_only();
    let store = Store::default();
    assert!(a.open_invitation(NOW).is_none());
    let made = a.make_invitation(&RANDOM, NOW, |b| store.put(b)).unwrap();
    assert_eq!(made.text, Invitation::from_random(&RANDOM).to_text());
    assert_eq!(made.expires_at, NOW + INVITATION_SECS);
    assert_eq!(INVITATION_SECS, 1800);
    // What was stored is the state with the invitation, and it opens again.
    let stored = store.last.borrow().clone().unwrap();
    assert_eq!(stored, a.snapshot());
    assert_eq!(stored[4], 2);
    let back = restore(&stored, false).unwrap();
    assert_eq!(back.open_invitation(NOW), Some(made.clone()));
    // Open until the end time and not at it.
    assert!(a.open_invitation(NOW + INVITATION_SECS - 1).is_some());
    assert!(a.open_invitation(NOW + INVITATION_SECS).is_none());
    // A failing store changes nothing and no text is given.
    let mut b = approver_only();
    *store.fail.borrow_mut() = true;
    assert!(b.make_invitation(&RANDOM, NOW, |x| store.put(x)).is_err());
    assert!(b.open_invitation(NOW).is_none());
    assert_eq!(b.snapshot()[4], 1);
}

#[test]
fn as_80_one_open_invitation_a_new_one_replaces_the_old() {
    let mut a = approver_only();
    let store = Store::default();
    let first = a.make_invitation(&RANDOM, NOW, |b| store.put(b)).unwrap();
    let second = a
        .make_invitation(&[0x43; 17], NOW + 10, |b| store.put(b))
        .unwrap();
    assert_ne!(first.text, second.text);
    assert_eq!(a.open_invitation(NOW + 10), Some(second));
    // A claim for the first no longer passes.
    let old = Invitation::from_random(&RANDOM);
    let r = run(&mut a, session_items(claim_for(&old)), &[0]);
    assert!(matches!(
        verdict(&r, 1),
        Verdict::Declined {
            code: DeclineCode::NotDecided,
            ..
        }
    ));
    // Cancelling forgets it and stores nothing when none is held.
    let mut c = approver_only();
    let calls = *store.calls.borrow();
    c.cancel_invitation(|b| store.put(b)).unwrap();
    assert_eq!(*store.calls.borrow(), calls);
    c.make_invitation(&RANDOM, NOW, |b| store.put(b)).unwrap();
    c.cancel_invitation(|b| store.put(b)).unwrap();
    assert!(c.open_invitation(NOW).is_none());
    assert_eq!(c.snapshot()[4], 1);
}

#[test]
fn as_80_a_state_without_an_invitation_is_written_as_before_and_decoding_is_strict() {
    let a = approver_only();
    let plain = a.snapshot();
    assert_eq!(plain[4], 1);
    assert!(restore(&plain, false).is_ok());
    let mut b = approver_only();
    let store = Store::default();
    b.make_invitation(&RANDOM, NOW, |x| store.put(x)).unwrap();
    let with = b.snapshot();
    assert_eq!(with.len(), plain.len() + 17 + 8);
    // Version 2 holds an invitation, version 1 does not, and nothing else.
    let mut as_v1 = with.clone();
    as_v1[4] = 1;
    assert!(matches!(
        restore(&as_v1, false),
        Err(RestoreError::Decode(_))
    ));
    let mut as_v2 = plain.clone();
    as_v2[4] = 2;
    assert!(matches!(
        restore(&as_v2, false),
        Err(RestoreError::Decode(_))
    ));
    let mut v3 = with.clone();
    v3[4] = 3;
    assert!(matches!(restore(&v3, false), Err(RestoreError::Decode(_))));
    // The spare bits of the invitation's last byte must be zero.
    let mut spare = with.clone();
    let at = with.len() - 8 - 1;
    spare[at] |= 0x04;
    assert!(matches!(
        restore(&spare, false),
        Err(RestoreError::Decode(_))
    ));
    let mut long = with.clone();
    long.push(0);
    assert!(restore(&long, false).is_err());
    for cut in 0..with.len() {
        assert!(restore(&with[..cut], false).is_err(), "cut at {cut}");
    }
}

// ---- AS-81: the line --------------------------------------------------------

fn open_with_invitation() -> (openbmt_approver::Approver, Invitation) {
    let mut a = approver_only();
    let store = Store::default();
    a.make_invitation(&RANDOM, NOW, |b| store.put(b)).unwrap();
    (a, Invitation::from_random(&RANDOM))
}

#[test]
fn as_81_the_line_shows_the_box_key_its_words_and_three_checks() {
    let (mut a, inv) = open_with_invitation();
    let request = request_with(5, session_items(claim_for(&inv)));
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    let lines = session.lines();
    let Detail::BoxClaim(line) = &lines[1].detail else {
        panic!("not a box line")
    };
    assert_eq!(line.box_key, box_key());
    assert_eq!(line.words, Some(box_key().fingerprint_words()));
    assert_eq!(line.checks.len(), 3);
    // Before the settings line is approved, the third check fails.
    assert_eq!(line.checks[0].status, StepStatus::Passed);
    assert_eq!(line.checks[1].status, StepStatus::Passed);
    assert_eq!(line.checks[2].status, StepStatus::Failed);
    assert!(!lines[1].can_approve);
    assert!(lines[1].failure.as_deref().unwrap().starts_with("AS-81"));
    assert!(session.decide(1, Choice::Approve).is_err());
    // After it, all three pass and the line can be approved.
    session.decide(0, Choice::Approve).unwrap();
    let lines = session.lines();
    let Detail::BoxClaim(line) = &lines[1].detail else {
        panic!("not a box line")
    };
    assert!(line.checks.iter().all(|c| c.status == StepStatus::Passed));
    assert!(lines[1].can_approve);
    session.decide(1, Choice::Approve).unwrap();
}

#[test]
fn as_81_a_claim_for_another_invitation_is_refused_in_the_second_check() {
    let (mut a, _) = open_with_invitation();
    let other = Invitation::from_random(&[0x11; 17]);
    let request = request_with(5, session_items(claim_for(&other)));
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    let lines = session.lines();
    let Detail::BoxClaim(line) = &lines[1].detail else {
        panic!("not a box line")
    };
    assert_eq!(line.checks[0].status, StepStatus::Passed);
    assert_eq!(line.checks[1].status, StepStatus::Failed);
    assert_eq!(line.checks[2].status, StepStatus::NotChecked);
    assert!(!lines[1].can_approve);
    // A claim of the right invitation with a flipped MAC is refused too.
    let (mut a, inv) = open_with_invitation();
    let mut claim = claim_for(&inv);
    claim.mac[31] ^= 1;
    let request = request_with(6, session_items(claim));
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    assert!(!session.lines()[1].can_approve);
}

#[test]
fn as_81_with_no_invitation_or_an_expired_one_the_first_check_fails() {
    let inv = Invitation::from_random(&RANDOM);
    let mut a = approver_only();
    let request = request_with(5, session_items(claim_for(&inv)));
    let session = a.open(request.as_bytes(), NOW).unwrap();
    let lines = session.lines();
    let Detail::BoxClaim(line) = &lines[1].detail else {
        panic!("not a box line")
    };
    assert_eq!(line.checks[0].status, StepStatus::Failed);
    assert!(!lines[1].can_approve);
    drop(session);
    // Open, then the Key app's clock passes the end.
    let (mut a, inv) = open_with_invitation();
    let request = request_with(7, session_items(claim_for(&inv)));
    let session = a.open(request.as_bytes(), NOW + INVITATION_SECS).unwrap();
    assert!(!session.lines()[1].can_approve);
    drop(session);
    let mut session = a
        .open(request.as_bytes(), NOW + INVITATION_SECS - 1)
        .unwrap();
    session.decide(0, Choice::Approve).unwrap();
    assert!(session.lines()[1].can_approve);
}

#[test]
fn as_81_settings_that_do_not_list_the_box_do_not_make_the_line_approvable() {
    let (mut a, inv) = open_with_invitation();
    // Settings that list another device as a Box, not this one.
    let mut s = settings_with(1, &[]);
    let other = identity([0x78; 32]).device_id();
    s.devices.insert(other);
    s.boxes.insert(other);
    let request = request_with(
        5,
        vec![
            settings_item(s),
            Item::BoxClaim {
                claim: claim_for(&inv),
            },
        ],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    assert!(!session.lines()[1].can_approve);
    // Settings that list it as a counter are refused by their own checks.
    let (mut a, inv) = open_with_invitation();
    let mut s = settings_with(1, &[]);
    s.devices.insert(box_key());
    s.counters.insert(box_key(), 10_000);
    let request = request_with(
        6,
        vec![
            settings_item(s),
            Item::BoxClaim {
                claim: claim_for(&inv),
            },
        ],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    assert!(!session.lines()[1].can_approve);
}

// ---- AS-82: the welcome and the spent invitation -------------------------------

#[test]
fn as_82_approving_makes_the_welcome_the_box_accepts_and_spends_the_invitation() {
    let (mut a, inv) = open_with_invitation();
    let claim = claim_for(&inv);
    let r = run(&mut a, session_items(claim), &[0, 1, 2]);
    let Verdict::Approved(Payload::BoxWelcome { welcome }) = verdict(&r, 1) else {
        panic!("no welcome: {:?}", verdict(&r, 1))
    };
    // It is the welcome the Box checks with its invitation and nonce.
    let pinned = welcome.verify(&inv, &box_key(), &claim.nonce).unwrap();
    assert_eq!(pinned, BoxRoots::KeyApp(roots()));
    // The settings were co-signed and the invitation is spent.
    assert!(matches!(
        verdict(&r, 0),
        Verdict::Approved(Payload::Settings { .. })
    ));
    assert!(a.open_invitation(NOW).is_none());
    assert_eq!(a.snapshot()[4], 1);
    assert!(a
        .last_settings()
        .unwrap()
        .settings
        .boxes
        .contains(&box_key()));
    // The same claim again finds no invitation.
    let items = vec![box_settings(2), Item::BoxClaim { claim }, health_item(1)];
    let again = run(&mut a, items, &[0]);
    assert!(!matches!(
        verdict(&again, 1),
        Verdict::Approved(Payload::BoxWelcome { .. })
    ));
    let _: Welcome = welcome;
}

#[test]
fn as_82_a_declined_claim_signs_no_welcome_and_still_spends_the_invitation() {
    let (mut a, inv) = open_with_invitation();
    let r = run(&mut a, session_items(claim_for(&inv)), &[0]);
    assert!(matches!(
        verdict(&r, 1),
        Verdict::Declined {
            code: DeclineCode::NotDecided,
            ..
        }
    ));
    assert!(a.open_invitation(NOW).is_none());
    // Declined by the person.
    let (mut a, inv) = open_with_invitation();
    let request = request_with(5, session_items(claim_for(&inv)));
    let store = Store::default();
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    session
        .decide(1, Choice::Decline("the words differ".into()))
        .unwrap();
    let bytes = session.finish(|b| store.put(b)).unwrap();
    let r = Response::from_bytes(&bytes).unwrap();
    assert!(matches!(
        verdict(&r, 1),
        Verdict::Declined {
            code: DeclineCode::ByPerson,
            ..
        }
    ));
    assert!(a.open_invitation(NOW).is_none());
    // The state that was stored is the one without it.
    assert_eq!(store.last.borrow().as_ref().unwrap()[4], 1);
}

#[test]
fn as_82_a_claim_that_fails_its_mac_spends_the_invitation_too() {
    let (mut a, _) = open_with_invitation();
    let other = Invitation::from_random(&[0x11; 17]);
    let r = run(&mut a, session_items(claim_for(&other)), &[0]);
    assert!(!matches!(
        verdict(&r, 1),
        Verdict::Approved(Payload::BoxWelcome { .. })
    ));
    assert!(a.open_invitation(NOW).is_none());
}

#[test]
fn as_82_a_session_that_is_not_finished_spends_nothing_and_a_failed_store_changes_nothing() {
    let (mut a, inv) = open_with_invitation();
    let request = request_with(5, session_items(claim_for(&inv)));
    {
        let mut session = a.open(request.as_bytes(), NOW).unwrap();
        session.decide(0, Choice::Approve).unwrap();
        session.decide(1, Choice::Approve).unwrap();
        // Dropped without the finish.
    }
    assert!(a.open_invitation(NOW).is_some());
    let store = Store::default();
    *store.fail.borrow_mut() = true;
    {
        let mut session = a.open(request.as_bytes(), NOW).unwrap();
        session.decide(0, Choice::Approve).unwrap();
        session.decide(1, Choice::Approve).unwrap();
        assert!(session.finish(|b| store.put(b)).is_err());
    }
    assert!(a.open_invitation(NOW).is_some());
    // The session can be finished again, and then it is spent.
    *store.fail.borrow_mut() = false;
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    session.decide(1, Choice::Approve).unwrap();
    session.finish(|b| store.put(b)).unwrap();
    assert!(a.open_invitation(NOW).is_none());
}
