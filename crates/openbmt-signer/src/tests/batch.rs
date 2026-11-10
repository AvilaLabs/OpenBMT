//! Section 10: address batches, with the receive mark inside the signer
//! state.

use std::cell::RefCell;
use std::collections::BTreeSet;

use openbmt_core::batch::{
    AddressBatch, Approver, BatchBuilder, BatchError, BatchPolicy, CosignError,
};
use openbmt_core::identity::{DeviceId, Identity};

use super::kit::*;
use crate::SignerState;

const POLICY: BatchPolicy = BatchPolicy { min_count: 3 };

fn never(_: &[u8]) -> Result<(), &'static str> {
    panic!("the store must not be reached")
}

fn identity(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

struct Fx {
    approver: Approver,
    owner: Identity,
    counter: DeviceId,
    approved: BTreeSet<DeviceId>,
}

fn fx() -> Fx {
    let counter = identity(20).device_id();
    Fx {
        approver: Approver::new(identity(1), POLICY),
        owner: identity(10),
        counter,
        approved: BTreeSet::from([counter]),
    }
}

fn batch(s: &SignerState, f: &Fx, start: u32) -> AddressBatch {
    BatchBuilder::new(POLICY)
        .derive(s.wallet(), f.counter, start, 3)
        .unwrap()
}

fn cosign<E>(
    s: &mut SignerState,
    f: &Fx,
    b: &AddressBatch,
    persist: impl FnOnce(&[u8]) -> Result<(), E>,
) -> Result<openbmt_core::identity::Sig, CosignError<E>> {
    s.cosign_batch(
        &f.approver,
        b,
        &f.approved,
        &f.owner.device_id(),
        &b.sign(&f.owner),
        persist,
    )
}

#[test]
fn section_10_the_receive_mark_lives_in_the_signer_state() {
    let f = fx();
    let mut s = state();
    assert_eq!(s.receive_mark(), 0);
    let b0 = batch(&s, &f, 0);
    let sig = cosign(&mut s, &f, &b0, ok).unwrap();
    // Schnorr signatures here are deterministic: this is the Key app's signature.
    assert_eq!(sig, b0.sign(&identity(1)));
    assert_eq!(f.approver.device_id(), identity(1).device_id());
    assert_eq!(s.receive_mark(), 3);
    // The next batch must start exactly at the mark.
    let b3 = batch(&s, &f, 3);
    cosign(&mut s, &f, &b3, ok).unwrap();
    assert_eq!(s.receive_mark(), 6);
    assert!(matches!(
        cosign(&mut s, &f, &b3, ok),
        Err(CosignError::Refused(BatchError::Overlap {
            expected: 6,
            found: 3
        }))
    ));
    let b9 = batch(&s, &f, 9);
    assert!(matches!(
        cosign(&mut s, &f, &b9, ok),
        Err(CosignError::Refused(BatchError::Gap {
            expected: 6,
            found: 9
        }))
    ));
    assert_eq!(s.receive_mark(), 6);
    // The mark is part of the snapshot.
    let back = SignerState::from_bytes(&s.to_bytes(), NET, &kit().xpubs[OWN]).unwrap();
    assert_eq!(back.receive_mark(), 6);
}

#[test]
fn sr_70_after_the_owner_raises_the_mark_the_next_batch_starts_there() {
    let f = fx();
    // A restored Key app: the mark is 0, the shop has issued 0 to 299.
    let mut s = state();
    let b300 = batch(&s, &f, 300);
    assert!(matches!(
        cosign(&mut s, &f, &b300, never),
        Err(CosignError::Refused(BatchError::Gap {
            expected: 0,
            found: 300
        }))
    ));
    s.raise_receive_mark(300, ok).unwrap();
    // Indexes below the mark are never signed for again.
    let b0 = batch(&s, &f, 0);
    assert!(matches!(
        cosign(&mut s, &f, &b0, never),
        Err(CosignError::Refused(BatchError::Overlap {
            expected: 300,
            found: 0
        }))
    ));
    cosign(&mut s, &f, &b300, ok).unwrap();
    assert_eq!(s.receive_mark(), 303);
}

#[test]
fn section_10_the_whole_state_with_the_new_mark_is_stored_before_the_signature() {
    let f = fx();
    let mut s = state();
    s.enroll_destination(external(1), "a", NOW, ok).unwrap();
    let b = batch(&s, &f, 0);
    let seen = RefCell::new(Vec::new());
    cosign(&mut s, &f, &b, |bytes: &[u8]| {
        seen.borrow_mut().push(bytes.to_vec());
        Ok::<(), &str>(())
    })
    .unwrap();
    let seen = seen.into_inner();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0], s.to_bytes());
    let stored = SignerState::from_bytes(&seen[0], NET, &kit().xpubs[OWN]).unwrap();
    assert_eq!(stored.receive_mark(), 3);
    assert_eq!(stored.allowlist().count(), 1);
}

#[test]
fn section_10_a_failing_store_signs_nothing_and_keeps_the_mark() {
    let f = fx();
    let mut s = state();
    let b = batch(&s, &f, 0);
    let before = s.to_bytes();
    assert_eq!(
        cosign(&mut s, &f, &b, fail),
        Err(CosignError::Persist("disk full"))
    );
    assert_eq!(s.to_bytes(), before);
    assert_eq!(s.receive_mark(), 0);
    // The same range can still be signed.
    cosign(&mut s, &f, &b, ok).unwrap();
    assert_eq!(s.receive_mark(), 3);
}

#[test]
fn section_10_a_refused_batch_never_reaches_the_store() {
    let f = fx();
    let mut s = state();
    let before = s.to_bytes();
    let never = |_: &[u8]| -> Result<(), &'static str> { panic!("reached the store") };
    // Signed by a stranger instead of the owner.
    let b = batch(&s, &f, 0);
    let stranger = identity(99);
    let r = s.cosign_batch(
        &f.approver,
        &b,
        &f.approved,
        &f.owner.device_id(),
        &b.sign(&stranger),
        never,
    );
    assert_eq!(
        r,
        Err(CosignError::Refused(BatchError::OwnerSignatureInvalid))
    );
    // A counter that is not on the device list.
    let mut unknown = f.approved.clone();
    unknown.clear();
    let r = s.cosign_batch(
        &f.approver,
        &b,
        &unknown,
        &f.owner.device_id(),
        &b.sign(&f.owner),
        never,
    );
    assert_eq!(r, Err(CosignError::Refused(BatchError::UnknownCounter)));
    // A batch for another wallet.
    let other = super::owner::other_wallet_state();
    let foreign = batch(&other, &f, 0);
    let r = s.cosign_batch(
        &f.approver,
        &foreign,
        &f.approved,
        &f.owner.device_id(),
        &foreign.sign(&f.owner),
        never,
    );
    assert_eq!(r, Err(CosignError::Refused(BatchError::WrongWallet)));
    assert_eq!(s.to_bytes(), before);
}

#[test]
fn section_10_cosigning_does_not_move_the_clock_or_touch_other_state() {
    let f = fx();
    let mut s = state();
    sign_ok(&mut s, &simple(10_000, 500), NOW);
    let before = SignerState::from_bytes(&s.to_bytes(), NET, &kit().xpubs[OWN]).unwrap();
    let b = batch(&s, &f, 0);
    cosign(&mut s, &f, &b, ok).unwrap();
    assert_eq!(s.latest_time_seen(), before.latest_time_seen());
    assert_eq!(s.change_mark(), before.change_mark());
    assert_eq!(
        s.log().cloned().collect::<Vec<_>>(),
        before.log().cloned().collect::<Vec<_>>()
    );
    // Only the receive mark differs.
    let mut expected = before.clone();
    expected.receive_mark = 3;
    assert_eq!(s.to_bytes(), expected.to_bytes());
}
