//! One history for the Key app (`docs/SIGNER-RULES.md` SR-92): the stored
//! state carries a generation that every save raises by one, a save over a
//! different generation is refused, and every unlock reads the store.

use std::sync::Arc;

use openbmt_owner::SaleKey;

use super::kit::*;
use crate::lock;
use crate::{
    Decision, KeyApp, KeyError, KeyStore, LineKind, LockStatus, SessionHandle, SetupState,
};

/// The generation inside the frame that is stored.
fn generation_of(store: &MemStore) -> u64 {
    let (_, key) = lock::open(&store.saved().expect("stored"), PIN)
        .map_err(|_| "opens")
        .expect("opens");
    key.generation()
}

fn version_of(store: &MemStore) -> u8 {
    store.saved().expect("stored")[4]
}

fn rule(r: Result<impl std::fmt::Debug, KeyError>) -> String {
    r.expect_err("refused").rule().to_string()
}

/// Every line of the session approved, then the finish.
fn sign(app: &Arc<KeyApp>, request: &[u8]) -> (Arc<SessionHandle>, Result<Vec<u8>, KeyError>) {
    let session = app.open_session(request.to_vec()).expect("opens");
    for line in session.lines().expect("lines") {
        session
            .decide(line.item, Decision::Approve)
            .expect("decides");
    }
    let done = session.finish();
    (session, done)
}

/// A second request of the same owner: one more refund, from other coins.
fn second_request(rig: &mut Rig) -> Vec<u8> {
    let sale = SaleKey {
        counter: counter(1).device_id(),
        index: 2,
    };
    rig.owner
        .set_sale_facts(
            sale,
            openbmt_owner::SaleFacts {
                received_sats: 50_000,
                reached_refund_depth: true,
            },
            ok_store,
        )
        .expect("facts");
    rig.owner
        .queue_refund(sale, "Mia", external(1), 11_000, None, ok_store)
        .expect("the refund is queued");
    let coins: Vec<_> = (0..2)
        .map(|i| coin(&rig.wallet, 30 + i, 10 + i, 300_000))
        .collect();
    build_request(&mut rig.owner, &coins, 1, 2).request
}

#[test]
fn sr_92_every_save_raises_the_generation_by_one() {
    let store = MemStore::new();
    let app = open_new(store.clone());
    assert_eq!(app.lock_status().expect("status"), LockStatus::Open);
    app.create_approver_only().expect("a key");
    assert_eq!(generation_of(&store), 1);
    assert_eq!(version_of(&store), 2);
    for n in 2..6 {
        app.create_approver_only().expect("saved again");
        assert_eq!(generation_of(&store), n);
    }
    // A handle that opens it remembers the generation it read.
    let again = reopen(store.clone());
    again.create_approver_only().expect("saved");
    assert_eq!(generation_of(&store), 6);
    // A save that failed in the store does not raise it, for the handle or
    // for the store.
    store.fail_saves(true);
    assert_eq!(rule(again.create_approver_only()), "AS-34");
    store.fail_saves(false);
    assert_eq!(generation_of(&store), 6);
    again.create_approver_only().expect("saved");
    assert_eq!(generation_of(&store), 7);
}

#[test]
fn sr_92_a_stale_handle_is_refused_signs_nothing_and_reloads() {
    let mut rig = rig();
    let first = session_request(&mut rig);
    let second = second_request(&mut rig);
    let store = rig.store.clone();
    // B opens the state as it is now and stays on it.
    let b = reopen(store.clone());
    let before = generation_of(&store);

    // A signs a payment and saves it.
    let (_, signed) = sign(&rig.app, &first);
    signed.expect("A signs");
    assert_eq!(generation_of(&store), before + 1);
    let day = rig.app.signer_status().expect("status").other_day;
    assert!(day > 0);
    // The day's ceiling leaves room for 5,000 sats more.
    let mut status = rig.app.signer_status().expect("status");
    status.ceilings_other.per_day = day + 5_000;
    rig.app
        .set_ceilings(status.ceilings_other, status.ceilings_allowlisted)
        .expect("lowered");
    let stored = store.saved().expect("stored");
    let saves = store.saves();

    // B still holds the old history, and its own check passes: with that
    // history the 11,000 sats fit. Its save is refused, nothing is written
    // and nothing signed is returned.
    let (session, done) = sign(&b, &second);
    let err = done.expect_err("refused");
    assert_eq!(err.rule(), "SR-92", "{}", err.text());
    assert!(matches!(err, KeyError::Refused { .. }));
    assert_eq!(store.saved().expect("stored"), stored);
    assert_eq!(store.saves(), saves);

    // The handle is locked and the session is over.
    assert_eq!(b.lock_status().expect("status"), LockStatus::Locked);
    assert_eq!(b.state().expect("state"), SetupState::Locked);
    assert!(
        session.finish().is_err(),
        "nothing is signed by the ended session"
    );
    assert_eq!(rule(b.signer_status()), "locked");
    assert_eq!(
        b.open_session(second.clone())
            .err()
            .expect("refused")
            .rule(),
        "locked"
    );

    // After it reloads, its history has A's signature, and the payment that
    // would break the day's total is refused.
    b.unlock(PIN.to_string()).expect("unlocks");
    let seen = b.signer_status().expect("status");
    assert_eq!(seen.other_day, day);
    assert_eq!(seen.ceilings_other.per_day, day + 5_000);
    let session = b.open_session(second.clone()).expect("opens");
    let lines = session.lines().expect("lines");
    let spend = lines
        .iter()
        .find(|l| l.kind == LineKind::Spend)
        .expect("the spend line");
    assert!(!spend.can_approve, "{:?}", spend.failure);
    assert!(
        spend.failure.as_deref().unwrap_or("").contains("SR-36"),
        "{:?}",
        spend.failure
    );
    assert_eq!(generation_of(&store), before + 2);
}

#[test]
fn sr_92_the_refused_save_changes_nothing_in_the_other_handle() {
    let store = MemStore::new();
    let a = open_new(store.clone());
    a.create_approver_only().expect("a key");
    let b = reopen(store.clone());
    a.create_approver_only().expect("A saves");
    let stored = store.saved().expect("stored");
    // B's save is refused whatever it was for, and nothing is written.
    assert_eq!(rule(b.create_approver_only()), "SR-92");
    assert_eq!(store.saved().expect("stored"), stored);
    // B is locked and reads the store at its next unlock; its next save
    // goes through and A's is then the stale one.
    b.unlock(PIN.to_string()).expect("unlocks");
    b.create_approver_only().expect("B saves");
    assert_eq!(rule(a.create_approver_only()), "SR-92");
    assert_eq!(generation_of(&store), 3);
}

#[test]
fn sr_92_a_save_after_another_handle_changed_the_pin_is_refused() {
    let store = MemStore::new();
    let a = open_new(store.clone());
    a.create_approver_only().expect("a key");
    let b = reopen(store.clone());
    a.change_pin(PIN.to_string(), "654321".to_string())
        .expect("changed");
    let stored = store.saved().expect("stored");
    // B holds the old key, which does not open the stored frame: the stored
    // state is not the one B opened.
    assert_eq!(rule(b.create_approver_only()), "SR-92");
    assert_eq!(store.saved().expect("stored"), stored);
    assert_eq!(rule(b.unlock(PIN.to_string())), "pin-wrong");
    b.unlock("654321".to_string()).expect("the new PIN");
    // A change of PIN is a save too: a stale handle's is refused.
    let c = KeyApp::open(store.clone()).expect("opens");
    c.unlock("654321".to_string()).expect("unlocks");
    b.create_approver_only().expect("B saves");
    assert_eq!(
        rule(c.change_pin("654321".to_string(), PIN.to_string())),
        "SR-92"
    );
    assert_eq!(c.lock_status().expect("status"), LockStatus::Locked);
    assert_eq!(generation_of_with(&store, "654321"), 3);
}

fn generation_of_with(store: &MemStore, pin: &str) -> u64 {
    let (_, key) = lock::open(&store.saved().expect("stored"), pin)
        .map_err(|_| "opens")
        .expect("opens");
    key.generation()
}

#[test]
fn sr_92_an_unlock_reads_the_store_and_not_what_was_read_at_open() {
    let store = MemStore::new();
    let a = open_new(store.clone());
    a.create_approver_only().expect("a key");
    // B opens while the stored state is at generation 1.
    let b = KeyApp::open(store.clone()).expect("opens");
    assert_eq!(b.lock_status().expect("status"), LockStatus::Locked);
    a.raise_receive_mark(9).expect_err("no wallet yet");
    a.create_approver_only().expect("A saves again");
    a.create_approver_only().expect("and again");
    assert_eq!(generation_of(&store), 3);
    // B's unlock opens generation 3, so its first save is not refused.
    b.unlock(PIN.to_string()).expect("unlocks");
    b.create_approver_only().expect("saved");
    assert_eq!(generation_of(&store), 4);

    // With the signer: the unlock shows what the others saved.
    let rig = rig();
    let late = KeyApp::open(rig.store.clone()).expect("opens");
    rig.app.raise_receive_mark(21).expect("a write");
    late.unlock(PIN.to_string()).expect("unlocks");
    assert_eq!(late.signer_status().expect("status").receive_mark, 21);
    // A store that is empty at the unlock is no longer a locked state.
    let gone = KeyApp::open(rig.store.clone()).expect("opens");
    rig.store.set_saved(None);
    assert_eq!(rule(gone.unlock(PIN.to_string())), "not-locked");
    assert_eq!(gone.lock_status().expect("status"), LockStatus::NoPin);
}

#[test]
fn sr_92_an_older_frame_reads_as_generation_0_and_is_written_in_the_new_version() {
    let rig = rig();
    let plain = rig.store.plain().expect("stored");
    // The frame as the Key app wrote it before the generation: version 1.
    let old = lock::LockKey::new(lock::COST, PIN, [5; lock::SALT_LEN])
        .expect("a key")
        .seal_next([6; lock::NONCE_LEN], &plain)
        .expect("sealed");
    assert_eq!(old[4], 1);
    let store = MemStore::new();
    store.set_saved(Some(old.clone()));
    let app = reopen(store.clone());
    assert_eq!(
        app.state().expect("state"),
        SetupState::Ready { holds_key2: true }
    );
    assert_eq!(generation_of(&store), 0);
    assert_eq!(version_of(&store), 1);
    // The next save is written in version 2, at generation 1, with the
    // state unchanged.
    app.raise_receive_mark(5).expect("saved");
    assert_eq!(version_of(&store), 2);
    assert_eq!(generation_of(&store), 1);
    let again = reopen(store.clone());
    assert_eq!(again.signer_status().expect("status").receive_mark, 5);
    // Two handles on the old frame: the second to save is refused.
    let store = MemStore::new();
    store.set_saved(Some(old));
    let (a, b) = (reopen(store.clone()), reopen(store.clone()));
    a.raise_receive_mark(6).expect("A saves");
    assert_eq!(rule(b.raise_receive_mark(7)), "SR-92");
    assert_eq!(generation_of(&store), 1);
    // A change of PIN leaves a version 1 frame as it was.
    let store = MemStore::new();
    store.set_saved(Some(
        lock::LockKey::new(lock::COST, PIN, [5; lock::SALT_LEN])
            .expect("a key")
            .seal_next([6; lock::NONCE_LEN], &plain)
            .expect("sealed"),
    ));
    let c = reopen(store.clone());
    c.change_pin(PIN.to_string(), "654321".to_string())
        .expect("changed");
    assert_eq!(version_of(&store), 1);
    c.raise_receive_mark(8).expect("saved");
    assert_eq!(version_of(&store), 2);
    assert_eq!(generation_of_with(&store, "654321"), 1);
}

#[test]
fn sr_92_the_generation_cannot_be_read_or_changed_without_the_pin() {
    let rig = rig();
    let stored = rig.store.saved().expect("stored");
    // Not in the header, in the clear: the same bytes with another
    // generation are only the sealed bytes, and they do not open.
    let frame = lock::decode(&stored).expect("a frame");
    assert_eq!(frame.header.version, 2);
    let (plain, key) = lock::open(&stored, PIN).map_err(|_| ()).expect("opens");
    assert_eq!(key.generation(), generation_of(&rig.store));
    let marker = key.generation().to_le_bytes();
    assert!(!stored.windows(8).any(|w| w == marker));
    assert!(plain.starts_with(b"OBM7"));
    for i in lock::HEADER_LEN..stored.len().min(lock::HEADER_LEN + 8) {
        let mut flipped = stored.clone();
        flipped[i] ^= 1;
        assert!(lock::open(&flipped, PIN).is_err(), "byte {i}");
    }
    // A frame of version 2 shorter than the generation is refused when it is
    // read.
    let mut short = stored[..lock::HEADER_LEN].to_vec();
    short[lock::HEADER_LEN - 4..].copy_from_slice(&20u32.to_le_bytes());
    short.extend_from_slice(&[0; 20]);
    assert!(lock::decode(&short).is_err());
}

#[test]
fn sr_92_the_store_is_compared_before_it_is_written() {
    // The order is in the code, where the frame is made: the stored frame is
    // read and its generation compared before the platform's write.
    let src = include_str!("../lock.rs");
    let code = src.split("#[cfg(test)]").next().expect("code");
    let compare = code.find("stored_generation(self.raw").expect("compare");
    let write = code.find("self.raw.save(frame)").expect("write");
    assert!(compare < write);
    assert!(code.contains("SR-92: compare and save"));
}

#[test]
fn sr_92_the_activity_is_a_single_instance() {
    let manifest = include_str!("../../../../apps/key-android/AndroidManifest.xml");
    assert!(manifest.contains("android:launchMode=\"singleTask\""));
}

#[test]
fn sr_92_the_refusal_has_its_own_words_still_to_be_written() {
    let words = KeyError::Refused {
        rule: "SR-92".to_string(),
        text: "SR-92: x".to_string(),
    }
    .words();
    assert!(crate::say::is_placeholder(&words));
    assert_eq!(words.heading, "WORDS_NEEDED_SR92_HEADING");
    assert_eq!(words.what, "WORDS_NEEDED_SR92_WHAT");
    assert_eq!(words.next, "WORDS_NEEDED_SR92_NEXT");
    assert_eq!(words.code, Some("SR-92"));
}

/// A store that is only read through, to see what the library asks for.
#[allow(dead_code)]
fn _store_is_a_trait_object(s: Arc<MemStore>) -> Arc<dyn KeyStore> {
    s
}
