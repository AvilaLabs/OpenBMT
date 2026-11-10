//! The lines (AS-23 to AS-29).

mod common;

use common::*;
use openbmt_approver::{Choice, DecideError, Detail, StepStatus};
use openbmt_core::pairing::{CounterPairing, PairOffer};
use openbmt_core::settings::{Change, Limit};
use openbmt_session::{Item, Label, OutputLabel};
use openbmt_signer::OutputKind;

fn lines_of(a: &mut openbmt_approver::Approver, items: Vec<Item>) -> Vec<openbmt_approver::Line> {
    let bytes = request_bytes(1, items);
    let session = a.open(&bytes, NOW).unwrap();
    session.lines()
}

fn spend_detail(line: &openbmt_approver::Line) -> &openbmt_approver::SpendLine {
    match &line.detail {
        Detail::Spend(s) => s,
        other => panic!("not a spend line: {other:?}"),
    }
}

#[test]
fn as_24_the_change_is_shown_as_change_first_and_in_the_middle() {
    let coins = [coin(1, 0, 300_000)];
    let payments = vec![(external(8), 30_000), (external(9), 40_000)];
    for position in [0usize, 1, 2] {
        let mut a = approver_with_key2();
        let psbt = psbt_bytes_with_change_at(&coins, payments.clone(), 0, position);
        // Honest labels: the refunds at the places of the payments, and the
        // change at its own place.
        let mut labels = Vec::new();
        let mut next_refund = 0u32;
        for at in 0..3u32 {
            if at as usize == position {
                labels.push(change_label(at));
            } else {
                labels.push(refund_label(at, "Mia"));
                next_refund += 1;
            }
        }
        assert_eq!(next_refund, 2);
        let lines = lines_of(&mut a, vec![Item::Spend { psbt, labels }]);
        let spend = spend_detail(&lines[0]);
        let review = spend
            .review
            .as_ref()
            .unwrap_or_else(|| panic!("{:?}", lines[0].failure));
        assert!(lines[0].can_approve, "change at {position}");
        assert_eq!(review.outputs.len(), 3);
        for (at, out) in review.outputs.iter().enumerate() {
            if at == position {
                assert!(matches!(out.kind, OutputKind::Change { index: 0 }));
            } else {
                assert_eq!(out.kind, OutputKind::Other);
            }
        }
        assert!(review.change.is_some());
        assert!(spend.outputs_without_label.is_empty());
        assert!(spend.labels_without_output.is_empty());
        assert!(spend.claimed_change_not_proven.is_empty());
        // A label that claims the change at the wrong place is not believed.
        let wrong = (position + 1) % 3;
        let mut labels = Vec::new();
        for at in 0..3u32 {
            labels.push(if at as usize == wrong {
                change_label(at)
            } else {
                refund_label(at, "Mia")
            });
        }
        let psbt = psbt_bytes_with_change_at(&coins, payments.clone(), 0, position);
        let lines = lines_of(&mut a, vec![Item::Spend { psbt, labels }]);
        assert_eq!(
            spend_detail(&lines[0]).claimed_change_not_proven,
            vec![wrong]
        );
    }
}

#[test]
fn as_23_the_line_comes_from_the_transaction_and_not_from_the_labels() {
    let mut a = approver_with_key2();
    let coins = [coin(1, 0, 300_000)];
    // The labels claim a refund of sale 0 for a cashier, to output 0, and
    // change for output 1. The transaction pays external(9) and returns the
    // change.
    let item = refund_item(&coins, 9, 60_000, 0);
    let lines = lines_of(&mut a, vec![item]);
    let spend = spend_detail(&lines[0]);
    let review = spend.review.as_ref().unwrap();
    let paid = &review.outputs[0];
    assert_eq!(paid.amount_sats, 60_000);
    assert_eq!(
        paid.address.as_deref(),
        Some(
            bitcoin::Address::from_script(&external(9), NET)
                .unwrap()
                .to_string()
                .as_str()
        )
    );
    // The label is there, as the owner phone's claim.
    assert!(matches!(
        paid.label_from_owner_phone,
        Some(Label::Refund { .. })
    ));
    assert!(matches!(review.outputs[1].kind, OutputKind::Change { .. }));
}

#[test]
fn as_23_a_line_whose_checks_fail_can_only_be_declined_and_names_the_rule() {
    let mut a = approver_with_key2();
    let coins = [coin(1, 0, 300_000)];
    // 150,000 is above the per-transaction ceiling of 100,000.
    let bytes = request_bytes(1, vec![refund_item(&coins, 1, 150_000, 0)]);
    let mut session = a.open(&bytes, NOW).unwrap();
    let line = session.lines().remove(0);
    assert!(!line.can_approve);
    let why = line.failure.clone().unwrap();
    assert!(why.starts_with("SR-35"), "{why}");
    match session.decide(0, Choice::Approve).unwrap_err() {
        DecideError::CannotApprove(text) => assert_eq!(text, why),
        other => panic!("{other:?}"),
    }
    session
        .decide(0, Choice::Decline("too much".into()))
        .unwrap();
}

#[test]
fn as_24_a_spend_line_shows_what_the_signer_found() {
    let mut a = approver_with_key2();
    let coins = [coin(1, 0, 300_000)];
    let lines = lines_of(&mut a, vec![refund_item(&coins, 1, 60_000, 0)]);
    assert!(lines[0].can_approve);
    let review = spend_detail(&lines[0]).review.as_ref().unwrap();
    assert_eq!(review.outputs.len(), 2);
    assert_eq!(review.outputs[0].kind, OutputKind::Other);
    assert_eq!(review.outputs[0].amount_sats, 60_000);
    let (index, amount) = review.change.unwrap();
    assert_eq!(index, 0);
    assert_eq!(review.outputs[1].amount_sats, amount);
    assert!(review.fee_sats > 0);
    assert_eq!(review.input_count, 1);
    assert_eq!(review.input_total_sats, 300_000);
    assert_eq!(review.other_total_sats, 60_000);
    assert_eq!(review.remaining.other_per_tx, 100_000 - 60_000);
    assert!(!review.moves_coins_inside_wallet);
    assert!(!review.already_counted);
}

#[test]
fn as_24_an_output_the_labels_leave_out_is_shown_and_marked() {
    let mut a = approver_with_key2();
    let coins = [coin(1, 0, 300_000)];
    // Two payments and change; the owner phone labels the first and the
    // change only.
    let item = Item::Spend {
        psbt: psbt_bytes(
            &coins,
            vec![(external(1), 20_000), (external(2), 30_000)],
            0,
        ),
        labels: vec![refund_label(0, "Mia"), change_label(2)],
    };
    let lines = lines_of(&mut a, vec![item]);
    let spend = spend_detail(&lines[0]);
    let review = spend.review.as_ref().unwrap();
    assert_eq!(spend.outputs_without_label, vec![1]);
    assert_eq!(review.outputs[1].amount_sats, 30_000);
    assert!(review.outputs[1].address.is_some());
    assert!(review.outputs[1].label_from_owner_phone.is_none());
    assert_eq!(review.other_total_sats, 50_000);
}

#[test]
fn as_24_a_change_label_the_signer_cannot_prove_is_marked() {
    let mut a = approver_with_key2();
    let coins = [coin(1, 0, 300_000)];
    // The owner phone calls the payment to a stranger "change".
    let item = Item::Spend {
        psbt: psbt_bytes(&coins, vec![(external(1), 20_000)], 0),
        labels: vec![change_label(0), change_label(1)],
    };
    let lines = lines_of(&mut a, vec![item]);
    assert_eq!(spend_detail(&lines[0]).claimed_change_not_proven, vec![0]);
}

#[test]
fn as_25_a_label_for_an_output_that_does_not_exist_blocks_the_line() {
    let mut a = approver_with_key2();
    let coins = [coin(1, 0, 300_000)];
    let item = Item::Spend {
        psbt: psbt_bytes(&coins, vec![(external(1), 20_000)], 0),
        labels: vec![
            refund_label(0, "Mia"),
            OutputLabel {
                output: 7,
                label: Label::Change,
            },
        ],
    };
    let lines = lines_of(&mut a, vec![item]);
    assert!(!lines[0].can_approve);
    assert!(lines[0].failure.as_ref().unwrap().starts_with("AS-25"));
    assert_eq!(spend_detail(&lines[0]).labels_without_output, vec![7]);
}

#[test]
fn as_25_a_file_the_signer_refuses_can_only_be_declined() {
    let mut a = approver_with_key2();
    let item = Item::Spend {
        psbt: vec![1, 2, 3],
        labels: vec![],
    };
    let lines = lines_of(&mut a, vec![item]);
    assert!(!lines[0].can_approve);
    assert!(lines[0].failure.as_ref().unwrap().starts_with("SR-01"));
    assert!(spend_detail(&lines[0]).review.is_none());
}

fn counters_one(a: &mut openbmt_approver::Approver) {
    cosign_settings(a, 200, 2, &[1], &Store::default());
}

#[test]
fn as_26_a_batch_line_shows_the_range_and_the_checks_in_order() {
    let mut a = approver_with_key2();
    counters_one(&mut a);
    let lines = lines_of(&mut a, vec![batch_item(1, 0, 3)]);
    let Detail::Batch(b) = &lines[0].detail else {
        panic!()
    };
    assert!(lines[0].can_approve);
    assert_eq!(
        (b.start, b.end, b.count, b.receive_mark),
        (0, Some(3), 3, 0)
    );
    assert_eq!(b.counter, counter(1).device_id());
    assert_eq!(b.checks.len(), 6);
    assert!(b.checks.iter().all(|c| c.status == StepStatus::Passed));
    let names: Vec<&str> = b.checks.iter().map(|c| c.name).collect();
    assert!(names[0].contains("wallet") && names[3].contains("device list"));
}

fn statuses(line: &openbmt_approver::Line) -> Vec<StepStatus> {
    let Detail::Batch(b) = &line.detail else {
        panic!()
    };
    b.checks.iter().map(|c| c.status).collect()
}

#[test]
fn as_26_each_failing_check_is_named_and_the_later_ones_are_not_reached() {
    use StepStatus::{Failed as F, NotChecked as N, Passed as P};
    let mut a = approver_with_key2();
    counters_one(&mut a);
    // A gap: the range starts above the receive mark.
    let lines = lines_of(&mut a, vec![batch_item(1, 5, 3)]);
    assert_eq!(statuses(&lines[0]), vec![P, P, P, P, F, N]);
    assert!(!lines[0].can_approve);
    assert!(lines[0].failure.as_ref().unwrap().starts_with("AS-26"));
    // A counter that is not on the device list.
    let lines = lines_of(&mut a, vec![batch_item(2, 0, 3)]);
    assert_eq!(statuses(&lines[0]), vec![P, P, P, F, N, N]);
    // A signature that is not the owner's.
    let mut b = batch(1, 0, 3);
    let sig = b.sign(&identity([0x55; 32]));
    let lines = lines_of(
        &mut a,
        vec![Item::BatchRefill {
            batch: b.clone(),
            owner_sig: sig,
        }],
    );
    assert_eq!(statuses(&lines[0]), vec![P, P, F, N, N, N]);
    // Another wallet.
    b.wallet_checksum = [9; 32];
    let sig = b.sign(&owner());
    let lines = lines_of(
        &mut a,
        vec![Item::BatchRefill {
            batch: b,
            owner_sig: sig,
        }],
    );
    assert_eq!(statuses(&lines[0]), vec![F, N, N, N, N, N]);
    // A batch that is too small.
    let small = batch(1, 0, 3);
    let mut a2 = {
        let mut other = openbmt_approver::Approver::new(
            &APPROVER_SECRET,
            wallet().clone(),
            owner().device_id(),
            openbmt_core::batch::BatchPolicy { min_count: 10 },
        )
        .unwrap();
        other
            .attach_signer(signer_state(None), kit().accounts[1])
            .unwrap();
        cosign_settings(&mut other, 201, 2, &[1], &Store::default());
        other
    };
    let sig = small.sign(&owner());
    let lines = lines_of(
        &mut a2,
        vec![Item::BatchRefill {
            batch: small,
            owner_sig: sig,
        }],
    );
    assert_eq!(statuses(&lines[0]), vec![P, F, N, N, N, N]);
}

#[test]
fn as_27_the_first_settings_show_every_field_as_loosening() {
    let mut a = approver_with_key2();
    let lines = lines_of(&mut a, vec![settings_item(settings_with(2, &[1]))]);
    let Detail::Settings(s) = &lines[0].detail else {
        panic!()
    };
    assert!(lines[0].can_approve);
    assert_eq!(s.last_version, None);
    assert_eq!(s.overall, Some(Change::Loosening));
    assert!(s.changes.len() >= 17);
    assert!(s.changes.iter().all(|c| c.old.is_none()));
    assert!(s.changes.iter().all(|c| c.direction == Change::Loosening));
}

#[test]
fn as_27_a_later_change_shows_old_and_new_and_the_direction_of_each_field() {
    let mut a = approver_with_key2();
    cosign_settings(&mut a, 200, 2, &[1], &Store::default());
    let mut next = settings_with(3, &[1]);
    next.light_per_sale = Limit::from_usd(100, 1_000);
    next.refund_cap = Limit::from_usd(50, 1_000);
    let lines = lines_of(&mut a, vec![settings_item(next)]);
    let Detail::Settings(s) = &lines[0].detail else {
        panic!()
    };
    assert_eq!(s.last_version, Some(2));
    assert_eq!(s.new_version, 3);
    assert_eq!(s.overall, Some(Change::Loosening));
    assert_eq!(s.changes.len(), 2);
    let light = s
        .changes
        .iter()
        .find(|c| c.field == "light_per_sale")
        .unwrap();
    assert_eq!(light.direction, Change::Loosening);
    assert_eq!(
        light.old.as_deref(),
        Some("10000 sats ($10 from the owner phone)")
    );
    assert_eq!(light.new, "100000 sats ($100 from the owner phone)");
    let refund = s.changes.iter().find(|c| c.field == "refund_cap").unwrap();
    assert_eq!(refund.direction, Change::Tightening);
    assert!(lines[0].can_approve);
}

#[test]
fn as_27_the_smallest_bitcoin_sale_is_listed_in_sats_with_its_direction() {
    let mut a = approver_with_key2();
    cosign_settings(&mut a, 200, 2, &[1], &Store::default());
    // Raising the floor tightens.
    let mut up = settings_with(3, &[1]);
    up.min_sale_sats = 50_000;
    let lines = lines_of(&mut a, vec![settings_item(up.clone())]);
    let Detail::Settings(s) = &lines[0].detail else {
        panic!()
    };
    assert_eq!(s.changes.len(), 1);
    let c = &s.changes[0];
    assert_eq!(c.field, "Smallest bitcoin sale");
    assert_eq!(c.old.as_deref(), Some("0 sats"));
    assert_eq!(c.new, "50000 sats");
    assert_eq!(c.direction, Change::Tightening);
    assert_eq!(s.overall, Some(Change::Tightening));
    // Once the raised floor is co-signed, lowering it to nothing loosens.
    let bytes = request_bytes(201, vec![settings_item(up)]);
    approve_all(&mut a, &bytes, &Store::default());
    let mut off = settings_with(4, &[1]);
    off.min_sale_sats = 0;
    let lines = lines_of(&mut a, vec![settings_item(off)]);
    let Detail::Settings(s) = &lines[0].detail else {
        panic!()
    };
    let c = &s.changes[0];
    assert_eq!(c.field, "Smallest bitcoin sale");
    assert_eq!(c.old.as_deref(), Some("50000 sats"));
    assert_eq!(c.new, "0 sats");
    assert_eq!(c.direction, Change::Loosening);
    assert_eq!(s.overall, Some(Change::Loosening));
}

#[test]
fn as_27_a_change_that_only_tightens_can_be_approved() {
    let mut a = approver_with_key2();
    cosign_settings(&mut a, 200, 2, &[1], &Store::default());
    let mut next = settings_with(3, &[1]);
    next.refund_cap = Limit::from_usd(50, 1_000);
    let lines = lines_of(&mut a, vec![settings_item(next)]);
    let Detail::Settings(s) = &lines[0].detail else {
        panic!()
    };
    assert_eq!(s.overall, Some(Change::Tightening));
    assert!(lines[0].can_approve);
}

#[test]
fn as_27_the_checks_name_a_bad_signature_a_bad_value_and_a_stale_version() {
    use StepStatus::{Failed as F, NotChecked as N, Passed as P};
    let mut a = approver_with_key2();
    cosign_settings(&mut a, 200, 2, &[1], &Store::default());
    let checks = |line: &openbmt_approver::Line| match &line.detail {
        Detail::Settings(s) => s.checks.iter().map(|c| c.status).collect::<Vec<_>>(),
        _ => panic!(),
    };
    // Not signed by the owner.
    let s = settings_with(3, &[1]);
    let sig = s.sign(&identity([0x55; 32]));
    let item = Item::Settings {
        signed: openbmt_core::settings::SignedSettings {
            settings: s,
            owner_sig: sig,
            approver_sig: None,
        },
    };
    let lines = lines_of(&mut a, vec![item]);
    assert_eq!(checks(&lines[0]), vec![F, N, N]);
    assert!(!lines[0].can_approve);
    // A value the settings rules refuse.
    let mut s = settings_with(3, &[1]);
    s.underpayment_tolerance_bps = 900;
    let lines = lines_of(&mut a, vec![settings_item(s)]);
    assert_eq!(checks(&lines[0]), vec![P, F, N]);
    // A version that does not go up.
    let lines = lines_of(&mut a, vec![settings_item(settings_with(2, &[1]))]);
    assert_eq!(checks(&lines[0]), vec![P, P, F]);
    assert!(lines[0].failure.as_ref().unwrap().starts_with("AS-27"));
}

#[test]
fn as_28_a_pairing_line_shows_name_key_and_the_four_session_words() {
    let mut a = approver_with_key2();
    let (item, counter_side) = pairing_item(1, 4);
    let lines = lines_of(&mut a, vec![item]);
    let Detail::Pairing(p) = &lines[0].detail else {
        panic!()
    };
    assert!(lines[0].can_approve);
    assert_eq!(p.name, "Counter 1");
    assert_eq!(p.counter, counter(1).device_id());
    assert_eq!(p.session_words, Some(counter_side.session_words()));
    assert_eq!(p.claimed_settings_version, 4);
    assert_eq!(p.settings_version_to_sign, 4);
}

#[test]
fn as_28_the_version_signed_is_never_below_the_key_apps_own() {
    let mut a = approver_with_key2();
    cosign_settings(&mut a, 200, 7, &[1], &Store::default());
    let (item, _) = pairing_item(1, 3);
    let lines = lines_of(&mut a, vec![item]);
    let Detail::Pairing(p) = &lines[0].detail else {
        panic!()
    };
    assert_eq!(
        (p.claimed_settings_version, p.settings_version_to_sign),
        (3, 7)
    );
    // A claim above the Key app's own is kept: a higher floor only makes a
    // counter refuse more.
    let (item, _) = pairing_item(1, 12);
    let lines = lines_of(&mut a, vec![item]);
    let Detail::Pairing(p) = &lines[0].detail else {
        panic!()
    };
    assert_eq!(p.settings_version_to_sign, 12);
}

#[test]
fn as_28_a_request_made_for_other_roots_is_refused() {
    let mut a = approver_with_key2();
    let other = PairOffer::new(
        owner().device_id(),
        identity([0x66; 32]).device_id(),
        wallet_id(),
    )
    .unwrap();
    let (_, request) =
        CounterPairing::begin(&other.to_bytes(), &counter(1), "Counter 1", [1; 32], NOW).unwrap();
    let item = Item::Pairing {
        request,
        settings_version: 1,
    };
    let lines = lines_of(&mut a, vec![item]);
    assert!(!lines[0].can_approve);
    assert!(lines[0].failure.as_ref().unwrap().starts_with("AS-28"));
    let Detail::Pairing(p) = &lines[0].detail else {
        panic!()
    };
    assert_eq!(p.session_words, None);
}

#[test]
fn as_29_a_health_check_can_always_be_approved() {
    let mut a = approver_only();
    let lines = lines_of(&mut a, vec![health_item(5)]);
    let Detail::Health(h) = &lines[0].detail else {
        panic!()
    };
    assert_eq!(h.nonce, [5; 32]);
    assert!(lines[0].can_approve && lines[0].failure.is_none());
}
