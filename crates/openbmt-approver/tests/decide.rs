//! Decisions, independence and order (AS-30 to AS-32).

mod common;

use common::*;
use openbmt_approver::{Choice, DecideError};
use openbmt_session::{DeclineCode, Payload, Verdict};

fn verdicts(response: &[u8], request: &openbmt_session::Request) -> Vec<Verdict> {
    decode(response, request)
        .results()
        .iter()
        .map(|r| r.verdict.clone())
        .collect()
}

fn declined(code: DeclineCode, v: &Verdict) -> bool {
    matches!(v, Verdict::Declined { code: c, .. } if *c == code)
}

#[test]
fn as_30_a_line_nobody_decided_is_declined_as_not_decided() {
    let mut a = approver_only();
    let store = Store::default();
    let request = request_with(1, vec![health_item(1), health_item(2)]);
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(1, Choice::Approve).unwrap();
    let response = session.finish(|b| store.put(b)).unwrap();
    let v = verdicts(&response, &request);
    assert!(declined(DeclineCode::NotDecided, &v[0]));
    assert!(matches!(v[1], Verdict::Approved(Payload::Health { .. })));
}

#[test]
fn as_30_a_decision_can_be_changed_until_the_finish() {
    let mut a = approver_only();
    let store = Store::default();
    let request = request_with(1, vec![health_item(1)]);
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    session
        .decide(0, Choice::Decline("changed my mind".into()))
        .unwrap();
    assert_eq!(
        session.lines()[0].decision,
        Some(Choice::Decline("changed my mind".into()))
    );
    let response = session.finish(|b| store.put(b)).unwrap();
    let v = verdicts(&response, &request);
    assert!(
        matches!(&v[0], Verdict::Declined { code: DeclineCode::ByPerson, text } if text == "changed my mind")
    );
}

#[test]
fn as_30_an_unknown_item_and_a_bad_reason_are_refused() {
    let mut a = approver_only();
    let bytes = request_bytes(1, vec![health_item(1)]);
    let mut session = a.open(&bytes, NOW).unwrap();
    assert_eq!(
        session.decide(9, Choice::Approve),
        Err(DecideError::UnknownItem(9))
    );
    for reason in [String::new(), "x".repeat(161), "a\nb".to_string()] {
        assert_eq!(
            session.decide(0, Choice::Decline(reason)),
            Err(DecideError::BadReason)
        );
    }
    assert!(session.decide(0, Choice::Decline("x".repeat(160))).is_ok());
}

#[test]
fn as_30_a_finished_session_takes_no_decision() {
    let mut a = approver_only();
    let store = Store::default();
    let bytes = request_bytes(1, vec![health_item(1)]);
    let mut session = a.open(&bytes, NOW).unwrap();
    session.finish(|b| store.put(b)).unwrap();
    assert_eq!(
        session.decide(0, Choice::Approve),
        Err(DecideError::Finished)
    );
    assert!(session.lines().is_empty());
    assert!(matches!(
        session.finish(|b| store.put(b)),
        Err(openbmt_approver::FinishError::Finished)
    ));
}

#[test]
fn as_31_declining_one_line_does_not_stop_another() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000), coin(2, 1, 300_000)];
    let request = request_with(
        1,
        vec![
            refund_item(&coins[..1], 1, 40_000, 0),
            refund_item(&coins[1..], 2, 40_000, 1),
            health_item(1),
        ],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    session
        .decide(0, Choice::Decline("not this one".into()))
        .unwrap();
    session.decide(1, Choice::Approve).unwrap();
    session.decide(2, Choice::Approve).unwrap();
    let response = session.finish(|b| store.put(b)).unwrap();
    let v = verdicts(&response, &request);
    assert!(declined(DeclineCode::ByPerson, &v[0]));
    assert!(matches!(v[1], Verdict::Approved(Payload::Spend { .. })));
    assert!(matches!(v[2], Verdict::Approved(Payload::Health { .. })));
}

#[test]
fn as_31_a_line_whose_checks_fail_does_not_stop_another() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let coins = [coin(1, 0, 300_000)];
    // The first line is over the ceiling and the batch names a counter that
    // is not on the list; the health check still goes through.
    let request = request_with(
        1,
        vec![
            refund_item(&coins, 1, 150_000, 0),
            batch_item(1, 0, 3),
            health_item(1),
        ],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    assert!(session.decide(0, Choice::Approve).is_err());
    assert!(session.decide(1, Choice::Approve).is_err());
    session.decide(2, Choice::Approve).unwrap();
    let response = session.finish(|b| store.put(b)).unwrap();
    let v = verdicts(&response, &request);
    assert!(declined(DeclineCode::NotDecided, &v[0]));
    assert!(declined(DeclineCode::NotDecided, &v[1]));
    assert!(matches!(v[2], Verdict::Approved(Payload::Health { .. })));
}

fn day_ceiling() -> openbmt_signer::Ceilings {
    openbmt_signer::Ceilings {
        per_tx: 100_000,
        per_day: 150_000,
        per_week: 1_000_000,
    }
}

#[test]
fn as_32_two_spends_count_against_the_ceilings_in_item_order() {
    let mut a = approver_with_key2_ceilings(Some(day_ceiling()));
    let coins = [coin(1, 0, 300_000), coin(2, 1, 300_000)];
    let request = request_with(
        1,
        vec![
            refund_item(&coins[..1], 1, 90_000, 0),
            refund_item(&coins[1..], 2, 90_000, 1),
        ],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    // Each is fine alone.
    let lines = session.lines();
    assert!(lines[0].can_approve && lines[1].can_approve);
    // With the first approved, the second no longer fits the day.
    session.decide(0, Choice::Approve).unwrap();
    let lines = session.lines();
    assert!(lines[0].can_approve);
    assert!(!lines[1].can_approve);
    assert!(lines[1].failure.as_ref().unwrap().starts_with("SR-36"));
    assert!(matches!(
        session.decide(1, Choice::Approve),
        Err(DecideError::CannotApprove(_))
    ));
    // With the first declined, the second fits again.
    session.decide(0, Choice::Decline("no".into())).unwrap();
    assert!(session.lines()[1].can_approve);
    session.decide(1, Choice::Approve).unwrap();
}

#[test]
fn as_32_a_line_that_stops_fitting_at_the_finish_is_declined_with_its_rule() {
    let mut a = approver_with_key2_ceilings(Some(day_ceiling()));
    let store = Store::default();
    let coins = [coin(1, 0, 300_000), coin(2, 1, 300_000)];
    let request = request_with(
        1,
        vec![
            refund_item(&coins[..1], 1, 90_000, 0),
            refund_item(&coins[1..], 2, 90_000, 1),
        ],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    // The second is approved while the first is still undecided; then the
    // first is approved too.
    session.decide(1, Choice::Approve).unwrap();
    session.decide(0, Choice::Approve).unwrap();
    let response = session.finish(|b| store.put(b)).unwrap();
    let v = verdicts(&response, &request);
    assert!(matches!(v[0], Verdict::Approved(Payload::Spend { .. })));
    match &v[1] {
        Verdict::Declined { code, text } => {
            assert_eq!(*code, DeclineCode::ChecksFailed);
            assert!(text.starts_with("SR-36"), "{text}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn as_32_a_second_batch_starts_where_the_first_ended() {
    let mut a = approver_with_key2();
    cosign_settings(&mut a, 200, 2, &[1, 2], &Store::default());
    let request = request_with(1, vec![batch_item(1, 0, 3), batch_item(2, 3, 3)]);
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    // The second starts at 3, which is a gap until the first is approved.
    assert!(session.lines()[0].can_approve);
    assert!(!session.lines()[1].can_approve);
    session.decide(0, Choice::Approve).unwrap();
    assert!(session.lines()[1].can_approve);
    session.decide(1, Choice::Approve).unwrap();
    // Declined again, the second is a gap once more.
    session.decide(0, Choice::Decline("no".into())).unwrap();
    assert!(!session.lines()[1].can_approve);
}

#[test]
fn as_32_settings_approved_earlier_add_a_counter_for_a_batch_later() {
    let mut a = approver_with_key2();
    let request = request_with(
        1,
        vec![settings_item(settings_with(2, &[1])), batch_item(1, 0, 3)],
    );
    let mut session = a.open(request.as_bytes(), NOW).unwrap();
    assert!(!session.lines()[1].can_approve);
    session.decide(0, Choice::Approve).unwrap();
    assert!(session.lines()[1].can_approve);
    session.decide(1, Choice::Approve).unwrap();
}
