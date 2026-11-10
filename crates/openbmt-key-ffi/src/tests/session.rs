//! A session driven through the exported API, with the owner crate on the
//! other side.

use openbmt_owner::ItemOutcome;

use super::kit::*;
use crate::{
    CheckStatus, Decision, KeyApp, KeyError, LineDetail, LineKind, OutputKindView, OwnerLabelKind,
    SetupState,
};

fn kinds(lines: &[crate::LineView]) -> Vec<LineKind> {
    lines.iter().map(|l| l.kind).collect()
}

fn line_of(lines: &[crate::LineView], kind: LineKind) -> &crate::LineView {
    lines
        .iter()
        .find(|l| l.kind == kind)
        .expect("the line is there")
}

#[test]
fn a_whole_session_with_a_refund_settings_a_pairing_and_a_health_check() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let saves_before = rig.store.saves();

    let session = rig.app.open_session(request.clone()).expect("opens");
    let lines = session.lines().expect("lines");
    assert_eq!(lines.len(), 4);
    for kind in [
        LineKind::Pairing,
        LineKind::Settings,
        LineKind::Spend,
        LineKind::HealthCheck,
    ] {
        assert!(kinds(&lines).contains(&kind), "{kind:?}");
    }
    assert!(lines.iter().all(|l| l.can_approve && l.decision.is_none()));

    // The refund: the signer's review, with the owner phone's label marked
    // as a claim.
    let LineDetail::Spend { detail } = &line_of(&lines, LineKind::Spend).detail else {
        panic!("a spend detail");
    };
    let review = detail.review.as_ref().expect("a review");
    let paid = review
        .outputs
        .iter()
        .find(|o| o.amount_sats == 10_000)
        .expect("the refund output");
    assert_eq!(paid.kind, OutputKindView::Other);
    assert!(paid
        .address
        .as_deref()
        .is_some_and(|a| a.starts_with("bcrt1")));
    let label = paid.owner_phone_label.as_ref().expect("the owner's label");
    assert!(label.is_claim);
    assert_eq!(label.kind, OwnerLabelKind::Refund);
    assert_eq!(label.cashier.as_deref(), Some("Mia"));
    assert!(review.fee_sats > 0 && review.fee_rate_sat_per_vb > 0);
    assert!(review.change_index.is_some());
    assert_eq!(review.remaining.other_per_tx, 100_000 - 10_000);
    assert!(detail.outputs_without_label.is_empty());

    // Settings: every field is new and the change loosens.
    let LineDetail::Settings { detail } = &line_of(&lines, LineKind::Settings).detail else {
        panic!("a settings detail");
    };
    assert_eq!(detail.new_version, 1);
    assert!(!detail.changes.is_empty());
    assert_eq!(detail.overall, Some(crate::Direction::Loosening));
    assert!(detail
        .checks
        .iter()
        .all(|c| c.status == CheckStatus::Passed));

    // The pairing shows the counter's key in full and the four words.
    let LineDetail::Pairing { detail } = &line_of(&lines, LineKind::Pairing).detail else {
        panic!("a pairing detail");
    };
    assert_eq!(detail.name, "Counter 1");
    assert_eq!(
        detail.counter_key,
        counter(1).device_id().to_bytes().to_vec()
    );
    assert_eq!(detail.session_words.as_ref().map(Vec::len), Some(4));

    let LineDetail::Health { detail } = &line_of(&lines, LineKind::HealthCheck).detail else {
        panic!("a health detail");
    };
    assert_eq!(detail.nonce.len(), 32);

    // Deciding changes nothing that is kept.
    for l in &lines {
        session.decide(l.item, Decision::Approve).expect("decides");
    }
    assert_eq!(rig.store.saves(), saves_before);
    assert!(session
        .lines()
        .expect("lines")
        .iter()
        .all(|l| l.decision == Some(Decision::Approve)));

    // The finish stores once, then returns the response.
    let response = session.finish().expect("finishes");
    assert_eq!(rig.store.saves(), saves_before + 1);
    assert!(session.lines().expect("lines after").is_empty());

    // The owner phone takes it: the refund comes back signed by key 2 and
    // key 1 joins it.
    let taken = rig
        .owner
        .take_response(&response, &mut rig.key1, &rig.key1_account, NOW, |_, _| {
            Ok::<(), String>(())
        })
        .expect("the owner takes the response");
    assert_eq!(taken.outcomes.len(), 4);
    assert!(taken
        .outcomes
        .iter()
        .any(|(_, o)| matches!(o, ItemOutcome::Healthy)));
    let delivered = taken
        .outcomes
        .iter()
        .filter(|(_, o)| matches!(o, ItemOutcome::Delivered(_)))
        .count();
    assert_eq!(delivered, 3);
    let status = rig.app.signer_status().expect("status");
    assert_eq!(status.spend_log_count, 1);
}

#[test]
fn a_second_session_is_refused_until_the_first_is_released() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let first = rig.app.open_session(request.clone()).expect("opens");
    let err = rig
        .app
        .open_session(request.clone())
        .err()
        .expect("refused");
    assert_eq!(err.rule(), "session-open");
    assert!(matches!(err, KeyError::State { .. }));
    // Own-screen changes are refused too while it is open.
    let err = rig.app.raise_change_mark(5).expect_err("refused");
    assert_eq!(err.rule(), "session-open");
    // Dropping the handle without finish releases the slot and changes
    // nothing.
    let saves = rig.store.saves();
    drop(first);
    assert_eq!(rig.store.saves(), saves);
    let second = rig.app.open_session(request.clone()).expect("opens again");
    // `cancel` releases it as well, and a cancelled handle is closed.
    second.cancel();
    assert!(second.lines().expect("closed").is_empty());
    assert!(second.finish().is_err());
    let third = rig.app.open_session(request).expect("opens a third time");
    drop(third);
}

#[test]
fn a_store_that_fails_changes_nothing_and_the_session_finishes_later() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let before = rig.store.saved().expect("something is stored");

    let session = rig.app.open_session(request.clone()).expect("opens");
    for l in session.lines().expect("lines") {
        session.decide(l.item, Decision::Approve).expect("decides");
    }
    rig.store.fail_saves(true);
    let err = session.finish().expect_err("the store fails");
    assert!(matches!(err, KeyError::Store { .. }));
    assert_eq!(err.rule(), "AS-34");
    assert!(err.text().contains("disk full"));
    // Nothing was stored and nothing kept changed.
    assert_eq!(rig.store.saved().expect("stored"), before);
    assert_eq!(rig.app.signer_status().expect("status").spend_log_count, 0);

    // The same session finishes once the store works.
    rig.store.fail_saves(false);
    let response = session.finish().expect("finishes now");
    assert!(!response.is_empty());
    assert_ne!(rig.store.saved().expect("stored"), before);
    assert_eq!(rig.app.signer_status().expect("status").spend_log_count, 1);
    // A second finish is refused.
    assert!(session.finish().is_err());
}

#[test]
fn a_restart_from_the_stored_bytes_keeps_what_the_session_used_up() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let (_, response) = approve_all(&rig, &request);
    assert!(!response.is_empty());
    let status = rig.app.signer_status().expect("status");

    // A new object from the same bytes.
    let again = KeyApp::open(rig.store.clone()).expect("restarts");
    assert_eq!(
        again.state().expect("state"),
        SetupState::Ready { holds_key2: true }
    );
    assert_eq!(again.signer_status().expect("status"), status);
    assert_eq!(
        again.wallet_view().expect("view").wallet_id,
        rig.view.wallet_id
    );
    // The session was finished: it is refused (AS-22), and the refusal
    // names its rule.
    let err = again.open_session(request.clone()).err().expect("refused");
    assert_eq!(err.rule(), "AS-22");
    assert!(matches!(err, KeyError::Refused { .. }));

    // A request that was never finished opens after the restart.
    let fresh = {
        let mut owner_rig = rig_with_same_keys(&rig);
        queue_all(&mut owner_rig, 11_000);
        let coins = vec![coin(&rig.wallet, 9, 5, 300_000)];
        build_request(&mut owner_rig.owner, &coins, 3, 7).request
    };
    let session = again.open_session(fresh).expect("a new request opens");
    assert_eq!(session.lines().expect("lines").len(), 4);
}

/// A second owner phone for the same wallet, for a second request.
fn rig_with_same_keys(rig: &Rig) -> Rig {
    let approver = openbmt_core::identity::DeviceId::from_bytes(
        &rig.app.device_root().expect("root").try_into().expect("32"),
    )
    .expect("a root");
    let owner = openbmt_owner::Owner::new(
        &OWNER_SECRET,
        rig.wallet.clone(),
        approver,
        policy(),
        openbmt_owner::Options::default(),
    )
    .expect("an owner");
    let key1 = rig.key1.clone();
    Rig {
        app: rig.app.clone(),
        store: rig.store.clone(),
        owner,
        wallet: rig.wallet.clone(),
        key1,
        key1_account: rig.key1_account,
        view: rig.view.clone(),
    }
}

#[test]
fn a_bad_request_is_refused_with_its_rule_and_nothing_opens() {
    let rig = rig();
    let err = rig.app.open_session(vec![0; 10]).err().expect("refused");
    assert_eq!(err.rule(), "AS-03");
    let err = rig.app.open_session(Vec::new()).err().expect("refused");
    assert!(matches!(err, KeyError::Refused { .. }));
    // Nothing is open: a good request opens at once.
    let mut rig = rig;
    let request = session_request(&mut rig);
    assert!(rig.app.open_session(request).is_ok());
}

#[test]
fn a_decline_needs_a_reason_and_is_carried_to_the_owner() {
    let mut rig = rig();
    let request = session_request(&mut rig);
    let session = rig.app.open_session(request).expect("opens");
    let lines = session.lines().expect("lines");
    let spend = line_of(&lines, LineKind::Spend).item;
    let err = session
        .decide(
            spend,
            Decision::Decline {
                reason: String::new(),
            },
        )
        .expect_err("an empty reason");
    assert_eq!(err.rule(), "AS-30");
    let err = session
        .decide(999, Decision::Approve)
        .expect_err("no such item");
    assert_eq!(err.rule(), "AS-30");
    for l in &lines {
        let d = if l.item == spend {
            Decision::Decline {
                reason: "not this one".to_string(),
            }
        } else {
            Decision::Approve
        };
        session.decide(l.item, d).expect("decides");
    }
    // A decision can be changed until the finish.
    let shown = session.lines().expect("lines");
    assert_eq!(
        line_of(&shown, LineKind::Spend).decision,
        Some(Decision::Decline {
            reason: "not this one".to_string()
        })
    );
    let response = session.finish().expect("finishes");
    let taken = rig
        .owner
        .take_response(&response, &mut rig.key1, &rig.key1_account, NOW, |_, _| {
            Ok::<(), String>(())
        })
        .expect("taken");
    assert!(taken
        .outcomes
        .iter()
        .any(|(_, o)| matches!(o, ItemOutcome::Returned(r) if r.contains("not this one"))));
}

#[test]
fn an_approver_only_key_app_declines_a_spend_and_signs_the_rest() {
    let store = MemStore::new();
    let app = KeyApp::open(store.clone()).expect("opens");
    app.create_approver_only().expect("an identity");
    assert!(app.key2_public().expect("asked").is_none());
    // The wallet's key 2 is on a hardware wallet: a fixed key here.
    let (_, slot1) = fixed_key(1, 1);
    let (_, slot2) = fixed_key(2, 2);
    let (_, slot3) = fixed_key(3, 3);
    let wallet = openbmt_core::wallet::WalletDescription::new(NET, [slot1, slot2, slot3.clone()])
        .expect("a wallet");
    app.accept_wallet(setup_for(&wallet, &slot3.xpub))
        .expect("accepted");
    assert_eq!(
        app.state().expect("state"),
        SetupState::Ready { holds_key2: false }
    );
    // The signer's screen is not available without key 2.
    assert_eq!(app.signer_status().expect_err("no signer").rule(), "AS-36");

    let approver = openbmt_core::identity::DeviceId::from_bytes(
        &app.device_root().expect("root").try_into().expect("32"),
    )
    .expect("a root");
    let owner = openbmt_owner::Owner::new(
        &OWNER_SECRET,
        wallet.clone(),
        approver,
        policy(),
        openbmt_owner::Options::default(),
    )
    .expect("an owner");
    let mut rig = Rig {
        app: app.clone(),
        store,
        owner,
        wallet: wallet.clone(),
        key1: openbmt_signer::SignerState::register(
            wallet.canonical_string(),
            NET,
            &fixed_key(1, 1).1.xpub,
            None,
        )
        .expect("key 1"),
        key1_account: fixed_key(1, 1).0,
        view: app.wallet_view().expect("view"),
    };
    let request = session_request(&mut rig);
    let session = app.open_session(request).expect("opens");
    let lines = session.lines().expect("lines");
    let spend = line_of(&lines, LineKind::Spend);
    assert!(!spend.can_approve);
    assert!(spend
        .failure
        .as_deref()
        .is_some_and(|f| f.contains("AS-36")));
    assert!(session.decide(spend.item, Decision::Approve).is_err());
    assert!(line_of(&lines, LineKind::Pairing).can_approve);
}
