//! Pair a Box through the exported API (SR-80 to SR-82), with the owner
//! crate on the other side.

use openbmt_core::boxpair::{Claim, Invitation, INVITATION_SECS};
use openbmt_core::identity::DeviceId;

use super::kit::*;
use crate::{CheckStatus, Decision, KeyStore, LineDetail, LineKind};

fn box_key() -> DeviceId {
    identity([0x77; 32]).device_id()
}

/// A request holding the claim `claim` and the settings that add its Box.
fn box_request(rig: &mut Rig, claim: &Claim) -> Vec<u8> {
    let queued = rig
        .owner
        .queue_box_claim(&claim.to_bytes(), ok_store)
        .expect("the claim is queued");
    rig.owner
        .queue_box(queued.id, 1_000, ok_store)
        .expect("the settings are queued");
    build_request(&mut rig.owner, &[], 0, 3).request
}

#[test]
fn sr_80_the_invitation_is_stored_before_it_is_shown_and_lasts_30_minutes() {
    let rig = rig();
    assert!(rig.app.open_invitation().expect("open").is_none());
    let saves = rig.store.saves();
    let made = rig.app.make_invitation().expect("made");
    // Stored before it was returned.
    assert_eq!(rig.store.saves(), saves + 1);
    let groups: Vec<&str> = made.text.split('-').collect();
    assert_eq!(groups.len(), 6);
    assert!(groups.iter().all(|g| g.len() == 5));
    assert!(Invitation::from_text(&made.text).is_ok());
    assert_eq!(made.expires_at - rig.store.now(), INVITATION_SECS);
    assert_eq!(made.secs_left, INVITATION_SECS);
    assert_eq!(rig.app.open_invitation().expect("open"), Some(made.clone()));
    // It survives a restart of the app and counts down on the phone's clock.
    let again = reopen(rig.store.clone());
    rig.store.set_now(rig.store.now() + 600);
    let left = again.open_invitation().expect("open").expect("held");
    assert_eq!(left.text, made.text);
    assert_eq!(left.secs_left, INVITATION_SECS - 600);
    // It runs out.
    rig.store.set_now(made.expires_at);
    assert!(again.open_invitation().expect("open").is_none());
}

#[test]
fn sr_80_a_new_invitation_replaces_the_old_and_cancel_forgets_it() {
    let rig = rig();
    let first = rig.app.make_invitation().expect("made");
    let second = rig.app.make_invitation().expect("made");
    assert_ne!(first.text, second.text);
    assert_eq!(rig.app.open_invitation().expect("open"), Some(second));
    rig.app.cancel_invitation().expect("cancelled");
    assert!(rig.app.open_invitation().expect("open").is_none());
    // Nothing held: cancelling again is fine.
    rig.app.cancel_invitation().expect("cancelled");
}

#[test]
fn sr_80_a_failing_store_gives_no_invitation() {
    let rig = rig();
    rig.store.fail_saves(true);
    assert!(rig.app.make_invitation().is_err());
    rig.store.fail_saves(false);
    assert!(rig.app.open_invitation().expect("open").is_none());
}

#[test]
fn sr_82_a_claim_shows_the_boxs_words_and_the_invitation_is_spent_by_approving() {
    let mut rig = rig();
    let made = rig.app.make_invitation().expect("made");
    let invitation = Invitation::from_text(&made.text).expect("text");
    let claim = Claim::make(&invitation, box_key(), [9; 32]);
    let request = box_request(&mut rig, &claim);
    let session = rig.app.open_session(request).expect("opens");
    // The settings that add the Box come first: until they are approved the
    // claim cannot be.
    let lines = session.lines().expect("lines");
    let claim_line = lines
        .iter()
        .find(|l| l.kind == LineKind::BoxClaim)
        .expect("the claim line");
    assert!(!claim_line.can_approve);
    let settings = lines
        .iter()
        .find(|l| l.kind == LineKind::Settings)
        .expect("the settings line");
    session
        .decide(settings.item, Decision::Approve)
        .expect("decides");
    let lines = session.lines().expect("lines");
    let line = lines
        .iter()
        .find(|l| l.kind == LineKind::BoxClaim)
        .expect("the claim line");
    assert!(line.can_approve, "{:?} {:?}", line.failure, line.detail);
    let LineDetail::BoxClaim { detail } = &line.detail else {
        panic!("a Box claim detail");
    };
    assert_eq!(detail.box_key, box_key().to_bytes().to_vec());
    let words: Vec<String> = box_key()
        .fingerprint_words()
        .iter()
        .map(|w| w.to_string())
        .collect();
    assert_eq!(detail.words.as_ref(), Some(&words));
    assert_eq!(detail.checks.len(), 3);
    assert!(detail
        .checks
        .iter()
        .all(|c| c.status == CheckStatus::Passed));
    for l in &lines {
        session.decide(l.item, Decision::Approve).expect("decides");
    }
    let response = session.finish().expect("finishes");
    // The invitation is spent.
    assert!(rig.app.open_invitation().expect("open").is_none());
    let taken = rig
        .owner
        .take_response(&response, &mut rig.key1, &rig.key1_account, NOW, |_, _| {
            Ok::<(), String>(())
        })
        .expect("the owner takes the response");
    assert!(!taken.outcomes.is_empty());
    assert_eq!(rig.owner.boxes_waiting().len(), 1);
}

#[test]
fn sr_82_declining_signs_nothing_and_spends_the_invitation() {
    let mut rig = rig();
    let made = rig.app.make_invitation().expect("made");
    let invitation = Invitation::from_text(&made.text).expect("text");
    let claim = Claim::make(&invitation, box_key(), [9; 32]);
    let request = box_request(&mut rig, &claim);
    let session = rig.app.open_session(request).expect("opens");
    for l in session.lines().expect("lines") {
        session
            .decide(
                l.item,
                Decision::Decline {
                    reason: "not now".to_string(),
                },
            )
            .expect("decides");
    }
    let response = session.finish().expect("finishes");
    assert!(rig.app.open_invitation().expect("open").is_none());
    rig.owner
        .take_response(&response, &mut rig.key1, &rig.key1_account, NOW, |_, _| {
            Ok::<(), String>(())
        })
        .expect("the owner takes the response");
    assert!(rig.owner.boxes_waiting().is_empty());
}

#[test]
fn sr_82_a_claim_without_an_open_invitation_is_not_approvable() {
    let mut rig = rig();
    let made = rig.app.make_invitation().expect("made");
    let invitation = Invitation::from_text(&made.text).expect("text");
    rig.app.cancel_invitation().expect("cancelled");
    let claim = Claim::make(&invitation, box_key(), [9; 32]);
    let request = box_request(&mut rig, &claim);
    let session = rig.app.open_session(request).expect("opens");
    let lines = session.lines().expect("lines");
    let line = lines
        .iter()
        .find(|l| l.kind == LineKind::BoxClaim)
        .expect("the claim line");
    assert!(!line.can_approve);
    assert!(line.failure.as_deref().is_some_and(|f| !f.is_empty()));
}
