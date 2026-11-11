//! Adding a Box: its claim and four words, the settings queued with it, the
//! welcome the Key app made and what the Box is handed (AS-83 to AS-87).

mod common;

use common::*;
use openbmt_approver::Choice;
use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::DeviceId;
use openbmt_owner::{BoxGap, Delivery, ItemOutcome, Refusal, Stage, MAX_BOX_CLAIMS};
use openbmt_session::{DeclineCode, Item, ItemResult, Payload, Response, Verdict};

const RATE: u64 = 1_000;
const RANDOM: [u8; 17] = [0x42; 17];

fn box_key(n: u8) -> DeviceId {
    identity([0x70 + n; 32]).device_id()
}

fn invitation() -> Invitation {
    Invitation::from_random(&RANDOM)
}

/// The claim of Box `n` for the invitation the Key app of the rig holds.
fn claim(n: u8) -> Claim {
    Claim::make(&invitation(), box_key(n), [n; 32])
}

/// The Key app makes its invitation.
fn invite(rig: &mut Rig) {
    let store = Store::default();
    rig.approver
        .make_invitation(&RANDOM, NOW, |b| store.put(b))
        .unwrap();
}

/// Queue Box `n`'s claim and its settings.
fn add(rig: &mut Rig, n: u8) -> openbmt_owner::BoxQueued {
    let queued = ok(rig.owner.queue_box_claim(&claim(n).to_bytes(), nop));
    ok(rig.owner.queue_box(queued.id, RATE, nop))
}

// ---- AS-83 ---------------------------------------------------------------------

#[test]
fn as_83_a_claim_is_queued_with_the_four_words_of_the_boxs_key() {
    let mut o = new_owner(openbmt_owner::Options::default());
    let queued = ok(o.queue_box_claim(&claim(1).to_bytes(), nop));
    assert_eq!(queued.box_key, box_key(1));
    assert_eq!(queued.words, box_key(1).fingerprint_words());
    assert_eq!(o.snapshot().unwrap()[4], 2);
    assert!(Claim::from_bytes(&claim(1).to_bytes()).is_ok());
    let words = queued.words;
    assert_eq!(words.len(), 4);
}

#[test]
fn as_83_what_is_not_a_claim_and_the_shops_own_keys_are_refused() {
    let mut o = new_owner(openbmt_owner::Options::default());
    let before = o.snapshot().unwrap();
    assert!(matches!(
        refused(o.queue_box_claim(b"junk", nop)),
        Refusal::BoxNotClaim(_)
    ));
    // A welcome is not a claim.
    let welcome = Welcome::make(
        &invitation(),
        &claim(1),
        openbmt_core::batch::PinnedRoots::new(owner_id(), approver_id()).unwrap(),
    );
    assert!(matches!(
        refused(o.queue_box_claim(&welcome.to_bytes(), nop)),
        Refusal::BoxNotClaim(_)
    ));
    // One byte more or fewer.
    let mut long = claim(1).to_bytes();
    long.push(0);
    assert!(matches!(
        refused(o.queue_box_claim(&long, nop)),
        Refusal::BoxNotClaim(_)
    ));
    let short = claim(1).to_bytes();
    assert!(matches!(
        refused(o.queue_box_claim(&short[..100], nop)),
        Refusal::BoxNotClaim(_)
    ));
    // A claim that names a root of the shop.
    for root in [owner_id(), approver_id()] {
        let c = Claim::make(&invitation(), root, [1; 32]);
        assert_eq!(
            refused(o.queue_box_claim(&c.to_bytes(), nop)),
            Refusal::BoxIsRoot
        );
    }
    assert_eq!(o.snapshot().unwrap(), before);
}

#[test]
fn as_83_a_box_that_is_queued_listed_or_waiting_is_refused_and_the_queue_is_bounded() {
    let mut rig = Rig::new();
    ok(rig.owner.queue_box_claim(&claim(1).to_bytes(), nop));
    assert_eq!(
        refused(rig.owner.queue_box_claim(&claim(1).to_bytes(), nop)),
        Refusal::BoxKnown
    );
    // The bound.
    for n in 2..=MAX_BOX_CLAIMS as u8 {
        ok(rig.owner.queue_box_claim(&claim(n).to_bytes(), nop));
    }
    assert_eq!(
        refused(rig.owner.queue_box_claim(&claim(9).to_bytes(), nop)),
        Refusal::Full("the Box claim queue")
    );
    // Listed in settings that are queued, and waiting after the Key app.
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    assert_eq!(
        refused(rig.owner.queue_box_claim(&claim(1).to_bytes(), nop)),
        Refusal::BoxKnown
    );
    rig.round(&[], 0);
    assert_eq!(rig.owner.boxes_waiting().len(), 1);
    assert_eq!(
        refused(rig.owner.queue_box_claim(&claim(1).to_bytes(), nop)),
        Refusal::BoxKnown
    );
}

// ---- AS-84 ---------------------------------------------------------------------

#[test]
fn as_84_the_settings_add_the_box_and_nothing_else_and_are_queued_with_the_claim() {
    let mut rig = Rig::new();
    let queued = add(&mut rig, 1);
    assert_eq!(queued.settings_version, 1);
    let s = &rig.owner.queued_settings()[0].signed.settings;
    assert!(s.devices.contains(&box_key(1)) && s.boxes.contains(&box_key(1)));
    assert!(s.counters.is_empty());
    assert_eq!(rig.owner.queued_settings()[0].id, queued.settings_id);
    assert_eq!(rig.owner.queued_settings()[0].stage, Stage::Queued);
    // A second Box builds on the queued settings and keeps the first.
    let two = ok(rig.owner.queue_box_claim(&claim(2).to_bytes(), nop));
    let second = ok(rig.owner.queue_box(two.id, RATE, nop));
    assert_eq!(second.settings_version, 2);
    let s = &rig.owner.queued_settings()[0].signed.settings;
    assert!(s.boxes.contains(&box_key(1)) && s.boxes.contains(&box_key(2)));
    assert_eq!(rig.owner.queued_settings().len(), 1);
}

#[test]
fn as_84_a_rate_a_known_claim_and_the_ninth_box_are_needed_or_refused() {
    let mut rig = Rig::new();
    let queued = ok(rig.owner.queue_box_claim(&claim(1).to_bytes(), nop));
    let before = rig.owner.snapshot().unwrap();
    assert_eq!(
        refused(rig.owner.queue_box(queued.id, 0, nop)),
        Refusal::NoSettingsRate
    );
    assert!(matches!(
        refused(rig.owner.queue_box(999, RATE, nop)),
        Refusal::UnknownEntry(999)
    ));
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    // A store that fails changes nothing.
    let store = Store::default();
    store.fail_next(true);
    assert!(rig
        .owner
        .queue_box(queued.id, RATE, |b| store.put(b))
        .is_err());
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert!(rig.owner.queued_settings().is_empty());
    // Eight Boxes fit and a ninth does not (BX-20).
    let mut rig = Rig::new();
    for n in 1..=8u8 {
        let c = Claim::make(&invitation(), box_key(n), [n; 32]);
        let q = ok(rig.owner.queue_box_claim(&c.to_bytes(), nop));
        ok(rig.owner.queue_box(q.id, RATE, nop));
        // Free the claim queue: a claim of a Box that is on the queued
        // settings leaves it.
        ok(rig.owner.cancel(q.id, nop));
    }
    let q = ok(rig.owner.queue_box_claim(
        &Claim::make(&invitation(), box_key(9), [9; 32]).to_bytes(),
        nop,
    ));
    assert!(matches!(
        refused(rig.owner.queue_box(q.id, RATE, nop)),
        Refusal::SettingsInvalid(_)
    ));
}

#[test]
fn as_84_the_claim_comes_after_the_settings_in_the_session() {
    let mut rig = Rig::new();
    add(&mut rig, 1);
    let built = rig.build(&[], 0);
    let request = openbmt_session::Request::from_bytes(&built.request).unwrap();
    let kinds: Vec<_> = request.items().iter().map(|i| i.item.kind()).collect();
    use openbmt_session::ItemKind as K;
    assert_eq!(kinds, vec![K::Settings, K::BoxClaim, K::HealthCheck]);
    assert_eq!(rig.owner.snapshot().unwrap()[4], 2);
    // The queue and its open session come back from the snapshot.
    let again = restore_owner(&rig.owner.snapshot().unwrap()).unwrap();
    assert_eq!(again.snapshot().unwrap(), rig.owner.snapshot().unwrap());
}

// ---- AS-85 ---------------------------------------------------------------------

#[test]
fn as_85_an_approved_claim_waits_in_the_outbox_with_a_welcome_the_box_accepts() {
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    let taken = rig.round(&[], 0);
    assert!(taken
        .outcomes
        .iter()
        .all(|(_, o)| !matches!(o, ItemOutcome::Returned(_))));
    let (id, bytes) = rig
        .owner
        .outbox()
        .iter()
        .find_map(|d| match &d.what {
            Delivery::BoxWelcome(b) => Some((d.id, b.clone())),
            _ => None,
        })
        .expect("a welcome waits");
    assert_eq!(bytes.len(), 202);
    assert!(rig.owner.outbox().iter().any(|d| d.id != id));
    // The Box, which holds the invitation, accepts it.
    let c = Claim::from_bytes(&bytes[..101]).unwrap();
    let w = Welcome::from_bytes(&bytes[101..]).unwrap();
    let pinned = w.verify(&invitation(), &c.box_key, &c.nonce).unwrap();
    assert_eq!(pinned.owner, owner_id());
    assert_eq!(pinned.approver, approver_id());
    // The claim and its entry left the queue.
    assert!(rig.owner.queued_settings().is_empty());
    assert_eq!(rig.owner.snapshot().unwrap()[4], 2);
    assert_eq!(
        restore_owner(&rig.owner.snapshot().unwrap())
            .unwrap()
            .boxes_waiting(),
        rig.owner.boxes_waiting()
    );
}

#[test]
fn as_85_a_declined_claim_goes_back_to_the_queue_with_the_reason() {
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    let built = rig.build(&[], 0);
    let response = rig.answer(&built.request, |line| match line.kind {
        openbmt_session::ItemKind::BoxClaim => Choice::Decline("the words differ".into()),
        _ => Choice::Approve,
    });
    let taken = ok(rig.take(&response));
    let returned: Vec<_> = taken
        .outcomes
        .iter()
        .filter_map(|(_, o)| match o {
            ItemOutcome::Returned(why) => Some(why.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(returned.len(), 1);
    assert!(returned[0].contains("the words differ"));
    assert!(rig.owner.boxes_waiting().is_empty());
    assert!(matches!(
        rig.owner.box_claims()[0].stage,
        Stage::Returned { .. }
    ));
}

#[test]
fn as_85_a_welcome_that_names_other_roots_is_returned() {
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    let built = rig.build(&[], 0);
    let request = openbmt_session::Request::from_bytes(&built.request).unwrap();
    // The real Key app answers; then the claim's result is swapped for a
    // welcome that pins another owner root, signed by the real Key app.
    let real = rig.answer_all(&built.request);
    let real = Response::from_bytes(&real).unwrap();
    let other = openbmt_core::batch::PinnedRoots::new(box_key(5), approver_id()).unwrap();
    let forged = Welcome::make(&invitation(), &claim(1), other);
    let results: Vec<ItemResult> = real
        .results()
        .iter()
        .map(|r| {
            let asked = request.item(r.number).unwrap();
            match &asked.item {
                Item::BoxClaim { .. } => ItemResult {
                    number: r.number,
                    verdict: Verdict::Approved(Payload::BoxWelcome { welcome: forged }),
                },
                _ => r.clone(),
            }
        })
        .collect();
    let swapped = Response::new(
        &identity(APPROVER_SECRET),
        *request.session_id(),
        request.hash(),
        results,
    )
    .unwrap();
    let taken = ok(rig.take(swapped.as_bytes()));
    assert!(taken.outcomes.iter().any(|(_, o)| matches!(
        o,
        ItemOutcome::Returned(why) if why.starts_with("AS-85")
    )));
    assert!(rig.owner.boxes_waiting().is_empty());
    let _ = DeclineCode::ByPerson;
}

// ---- AS-86 ---------------------------------------------------------------------

#[test]
fn as_86_the_handover_is_the_welcome_and_the_settings_with_both_signatures() {
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    // Nothing waits before the Key app has answered.
    assert_eq!(
        refused_ref(rig.owner.box_handover(&box_key(1))),
        Refusal::BoxNotReady(BoxGap::Welcome)
    );
    rig.round(&[], 0);
    let before = rig.owner.snapshot().unwrap();
    let h = rig.owner.box_handover(&box_key(1)).unwrap();
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert_eq!(h.box_key, box_key(1));
    assert_eq!(h.welcome.len(), 101);
    let settings = openbmt_core::settings::SignedSettings::from_bytes(&h.settings).unwrap();
    assert!(settings.approver_sig.is_some());
    assert!(settings.settings.boxes.contains(&box_key(1)));
    assert!(h.counters.is_empty());
    // The Box checks them under the two roots.
    let roots = openbmt_core::batch::PinnedRoots::new(owner_id(), approver_id()).unwrap();
    assert!(settings.verify(&roots, None).is_ok());
    // Another Box has nothing waiting.
    assert_eq!(
        refused_ref(rig.owner.box_handover(&box_key(2))),
        Refusal::BoxNotReady(BoxGap::Welcome)
    );
}

#[test]
fn as_86_once_the_box_accepted_the_welcome_and_the_settings_leave_the_outbox() {
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    rig.round(&[], 0);
    let outbox_before = rig.owner.outbox().len();
    ok(rig.owner.mark_box_delivered(&box_key(1), nop));
    assert_eq!(rig.owner.outbox().len(), outbox_before - 2);
    assert!(rig.owner.boxes_waiting().is_empty());
    assert_eq!(
        refused(rig.owner.mark_box_delivered(&box_key(1), nop)),
        Refusal::BoxNotReady(BoxGap::Welcome)
    );
    // A shop with a counter keeps the settings for the counter.
    let mut rig = Rig::new();
    let (_, request) = pair_request(&rig.owner, 1);
    let queued = ok(rig.owner.queue_counter_request(&request, nop));
    ok(rig
        .owner
        .queue_counter(queued.id, RATE, SMALL_BATCH, NOW, nop));
    rig.round(&[], 0);
    invite(&mut rig);
    add(&mut rig, 1);
    rig.round(&[], 0);
    let before = rig.owner.outbox().len();
    let h = rig.owner.box_handover(&box_key(1)).unwrap();
    assert_eq!(h.counters, vec![counter(1).device_id()]);
    ok(rig.owner.mark_box_delivered(&box_key(1), nop));
    assert_eq!(rig.owner.outbox().len(), before - 1);
    assert!(rig
        .owner
        .outbox()
        .iter()
        .any(|d| d.id == h.settings_id && matches!(d.what, Delivery::Settings(_))));
}

#[test]
fn as_86_settings_that_do_not_list_the_box_are_a_gap_and_a_failed_store_changes_nothing() {
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    // The Key app approves the claim only; the settings are declined.
    let built = rig.build(&[], 0);
    let response = rig.answer(&built.request, |line| match line.kind {
        openbmt_session::ItemKind::Settings => Choice::Decline("no".into()),
        _ => Choice::Approve,
    });
    let taken = ok(rig.take(&response));
    assert!(taken
        .outcomes
        .iter()
        .any(|(_, o)| matches!(o, ItemOutcome::Returned(_))));
    // The Key app's own check refuses the claim without the settings, so no
    // welcome waits and the gap is the welcome.
    assert_eq!(
        refused_ref(rig.owner.box_handover(&box_key(1))),
        Refusal::BoxNotReady(BoxGap::Welcome)
    );
    // A failing store.
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    rig.round(&[], 0);
    let before = rig.owner.snapshot().unwrap();
    let store = Store::default();
    store.fail_next(true);
    assert!(rig
        .owner
        .mark_box_delivered(&box_key(1), |b| store.put(b))
        .is_err());
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    assert_eq!(
        BoxGap::Welcome.to_string(),
        "no finished welcome of the Box waits"
    );
}

fn refused_ref<T: std::fmt::Debug>(r: Result<T, Refusal>) -> Refusal {
    match r {
        Err(x) => x,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn as_84_the_snapshot_is_version_2_only_with_something_of_a_box_and_decoding_is_strict() {
    let plain = new_owner(openbmt_owner::Options::default());
    let v1 = plain.snapshot().unwrap();
    assert_eq!(v1[4], 1);
    assert!(restore_owner(&v1).is_ok());
    // Version 2 with nothing of a Box is not what encoding gives.
    let mut as_v2 = v1.clone();
    as_v2[4] = 2;
    assert!(restore_owner(&as_v2).is_err());
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    let v2 = rig.owner.snapshot().unwrap();
    assert_eq!(v2[4], 2);
    assert!(restore_owner(&v2).is_ok());
    let mut as_v1 = v2.clone();
    as_v1[4] = 1;
    assert!(restore_owner(&as_v1).is_err());
    let mut v3 = v2.clone();
    v3[4] = 3;
    assert!(restore_owner(&v3).is_err());
    let mut long = v2.clone();
    long.push(0);
    assert!(restore_owner(&long).is_err());
    for cut in 0..v2.len() {
        assert!(restore_owner(&v2[..cut]).is_err(), "cut at {cut}");
    }
    // A waiting welcome and a frozen claim come back too.
    rig.round(&[], 0);
    let after = rig.owner.snapshot().unwrap();
    assert_eq!(after[4], 2);
    assert!(restore_owner(&after).is_ok());
}

// ---- AS-87 ---------------------------------------------------------------------

#[test]
fn as_87_a_box_that_was_not_handed_its_welcome_is_forgotten_in_one_write() {
    // A claim and its settings, queued and not yet in a session.
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    assert_eq!(rig.owner.box_claims().len(), 1);
    assert_eq!(rig.owner.queued_settings().len(), 1);
    ok(rig.owner.forget_box(&box_key(1), nop));
    assert!(rig.owner.box_claims().is_empty());
    assert!(rig.owner.queued_settings().is_empty());
    // The Box can be queued again, for it is no longer known.
    add(&mut rig, 1);

    // The Key app approved both: the welcome and the settings wait in the
    // outbox, and forgetting takes them out too.
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    rig.round(&[], 0);
    assert_eq!(rig.owner.boxes_waiting().len(), 1);
    assert!(!rig.owner.outbox().is_empty());
    ok(rig.owner.forget_box(&box_key(1), nop));
    assert!(rig.owner.boxes_waiting().is_empty());
    assert!(rig.owner.outbox().is_empty());
    assert!(rig.owner.queued_settings().is_empty());
    assert_eq!(
        refused_ref(rig.owner.box_handover(&box_key(1))),
        Refusal::BoxNotReady(BoxGap::Welcome)
    );
}

#[test]
fn as_87_with_nothing_queued_a_box_a_session_and_a_failing_store_change_nothing() {
    let mut rig = Rig::new();
    invite(&mut rig);
    // Nothing is queued for this Box.
    let before = rig.owner.snapshot().unwrap();
    assert_eq!(
        refused(rig.owner.forget_box(&box_key(1), nop)),
        Refusal::BoxNothingToForget
    );
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    // A Box that took its welcome has nothing left to forget.
    add(&mut rig, 1);
    rig.round(&[], 0);
    let store = Store::default();
    store.fail_next(true);
    let before = rig.owner.snapshot().unwrap();
    assert!(rig.owner.forget_box(&box_key(1), |b| store.put(b)).is_err());
    assert_eq!(rig.owner.snapshot().unwrap(), before);
    ok(rig.owner.mark_box_delivered(&box_key(1), nop));
    assert_eq!(
        refused(rig.owner.forget_box(&box_key(1), nop)),
        Refusal::BoxNothingToForget
    );
    // What is in an open session waits for the session to be abandoned.
    let mut rig = Rig::new();
    invite(&mut rig);
    add(&mut rig, 1);
    let _ = rig.build(&[], 0);
    let before = rig.owner.snapshot().unwrap();
    assert!(matches!(
        refused(rig.owner.forget_box(&box_key(1), nop)),
        Refusal::WrongStage(_)
    ));
    assert_eq!(rig.owner.snapshot().unwrap(), before);
}
