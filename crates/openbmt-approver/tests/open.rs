//! Opening a request (AS-20 to AS-22).

mod common;

use common::*;
use openbmt_approver::{Choice, RefuseSession};
use openbmt_core::wire::DecodeError;
use openbmt_session::{Item, Request};

fn health_request(session: u8) -> Vec<u8> {
    request_bytes(session, vec![health_item(1)])
}

#[test]
fn as_20_a_good_request_opens() {
    let mut a = approver_with_key2();
    let bytes = health_request(1);
    let session = a.open(&bytes, NOW).unwrap();
    assert_eq!(session.session_id(), &[1; 32]);
    assert_eq!(session.lines().len(), 1);
}

#[test]
fn as_20_the_size_is_checked_first() {
    let mut a = approver_with_key2();
    let big = vec![0u8; openbmt_session::MAX_SESSION_BYTES + 1];
    assert_eq!(refused(&mut a, &big), RefuseSession::TooLarge);
}

#[test]
fn as_20_the_frame_comes_before_the_signature() {
    let mut a = approver_with_key2();
    assert!(matches!(
        refused(&mut a, b"nonsense"),
        RefuseSession::Frame(DecodeError::Truncated | DecodeError::BadMagic)
    ));
    let mut bytes = health_request(1);
    bytes.push(0);
    assert_eq!(
        refused(&mut a, &bytes),
        RefuseSession::Frame(DecodeError::TrailingBytes)
    );
}

#[test]
fn as_20_a_request_signed_by_another_key_is_refused() {
    let mut a = approver_with_key2();
    let other = identity([0x77; 32]);
    let request =
        Request::new(&other, [1; 32], wallet_id(), numbered(vec![health_item(1)])).unwrap();
    assert_eq!(
        refused(&mut a, request.as_bytes()),
        RefuseSession::WrongOwner
    );
}

#[test]
fn as_20_a_request_with_a_changed_byte_is_refused() {
    let mut a = approver_with_key2();
    let mut bytes = health_request(1);
    // A byte of the session ID: the owner's signature no longer holds.
    bytes[6] ^= 1;
    assert_eq!(refused(&mut a, &bytes), RefuseSession::BadSignature);
}

#[test]
fn as_20_a_request_for_another_wallet_is_refused() {
    let mut a = approver_with_key2();
    let mut other = wallet_id();
    other[31] ^= 1;
    let request = Request::new(&owner(), [1; 32], other, numbered(vec![health_item(1)])).unwrap();
    assert_eq!(
        refused(&mut a, request.as_bytes()),
        RefuseSession::WrongWallet
    );
}

#[test]
fn as_20_the_checks_run_in_their_order() {
    let mut a = approver_with_key2();
    // Wrong owner and wrong wallet: the owner is named first.
    let other = identity([0x77; 32]);
    let mut wrong_wallet = wallet_id();
    wrong_wallet[0] ^= 1;
    let request = Request::new(
        &other,
        [1; 32],
        wrong_wallet,
        numbered(vec![health_item(1)]),
    )
    .unwrap();
    assert_eq!(
        refused(&mut a, request.as_bytes()),
        RefuseSession::WrongOwner
    );
    // Wrong wallet and a session that was finished: the wallet is named first.
    let store = Store::default();
    let first = health_request(5);
    approve_all(&mut a, &first, &store);
    let request = Request::new(
        &owner(),
        [5; 32],
        wrong_wallet,
        numbered(vec![health_item(1)]),
    )
    .unwrap();
    assert_eq!(
        refused(&mut a, request.as_bytes()),
        RefuseSession::WrongWallet
    );
}

#[test]
fn as_21_opening_listing_and_deciding_change_nothing() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let before = a.snapshot();
    let coins = [coin(1, 0, 300_000)];
    let bytes = request_bytes(
        1,
        vec![
            refund_item(&coins, 1, 50_000, 0),
            health_item(2),
            batch_item(1, 0, 3),
        ],
    );
    {
        let mut session = a.open(&bytes, NOW).unwrap();
        let _ = session.lines();
        session.decide(0, Choice::Approve).unwrap();
        session.decide(1, Choice::Decline("no".into())).unwrap();
        let _ = session.lines();
    }
    assert_eq!(a.snapshot(), before);
    assert_eq!(*store.calls.borrow(), 0);
    assert_eq!(a.seen_sessions().len(), 0);
    // The same session can still be opened: nothing was used up.
    assert!(a.open(&bytes, NOW).is_ok());
}

#[test]
fn as_22_a_finished_session_is_refused_when_opened_again() {
    let mut a = approver_with_key2();
    let store = Store::default();
    let bytes = health_request(9);
    approve_all(&mut a, &bytes, &store);
    assert_eq!(refused(&mut a, &bytes), RefuseSession::SeenBefore);
    assert_eq!(a.seen_sessions(), &[[9u8; 32]]);
}

#[test]
fn as_22_a_session_that_was_not_finished_leaves_no_trace() {
    let mut a = approver_with_key2();
    let bytes = health_request(9);
    drop(a.open(&bytes, NOW).unwrap());
    assert!(a.seen_sessions().is_empty());
    assert!(a.open(&bytes, NOW).is_ok());
}

#[test]
fn as_22_only_the_last_256_sessions_are_remembered() {
    let mut a = approver_only();
    let store = Store::default();
    let ids: Vec<[u8; 32]> = (0..257u32)
        .map(|n| {
            let mut id = [0u8; 32];
            id[..4].copy_from_slice(&n.to_le_bytes());
            id
        })
        .collect();
    let make = |id: [u8; 32]| {
        Request::new(
            &owner(),
            id,
            wallet_id(),
            numbered(vec![Item::HealthCheck { nonce: id }]),
        )
        .unwrap()
        .as_bytes()
        .to_vec()
    };
    for id in &ids {
        approve_all(&mut a, &make(*id), &store);
    }
    assert_eq!(a.seen_sessions().len(), 256);
    // The oldest was forgotten, the newest is remembered.
    assert!(a.open(&make(ids[0]), NOW).is_ok());
    assert_eq!(refused(&mut a, &make(ids[256])), RefuseSession::SeenBefore);
    assert_eq!(refused(&mut a, &make(ids[1])), RefuseSession::SeenBefore);
}
