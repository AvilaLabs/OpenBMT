//! A sale's facts from the owner's own view (`docs/OWNER-COINS-RULES.md`,
//! OC-14).

use bitcoin::BlockHash;
use openbmt_owner::{SaleFacts, SaleKey};

use super::refunds::{new_owner, new_owner_for, view_with, DEPTH, TIP};
use super::*;
use crate::coins::CoinView;
use crate::facts::{verified_facts, FactsRefused};

fn sale(index: u32) -> SaleKey {
    SaleKey {
        counter: counter(),
        index,
    }
}

fn facts(view: &CoinView, index: u32) -> Result<SaleFacts, FactsRefused> {
    verified_facts(view, &new_owner(), sale(index), TIP, DEPTH)
}

#[test]
fn oc_14_a_view_that_is_not_ready_or_not_at_the_tip_gives_no_facts() {
    // A new view has checked nothing.
    assert_eq!(facts(&CoinView::new(0), 0), Err(FactsRefused::ViewNotReady));
    // A view that is behind the tip.
    let view = view_with(&[(0, 5_000, DEPTH)]);
    assert_eq!(
        verified_facts(&view, &new_owner(), sale(0), TIP + 1, DEPTH),
        Err(FactsRefused::ViewNotReady)
    );
    // A view that is rescanning because new addresses were added.
    let mut view = view_with(&[(0, 5_000, DEPTH)]);
    view.raise_marks_and_rescan(5, 0, TIP, |_| Ok::<(), ()>(()))
        .unwrap();
    assert!(!view.ready());
    assert_eq!(facts(&view, 0), Err(FactsRefused::ViewNotReady));
    // A view in the deep reorg state.
    let mut view = view_with(&[(0, 5_000, DEPTH)]);
    let left = vec![BlockHash::from_byte_array([9; 32]); 101];
    view.reorganize(&left, 0, |_| Ok::<(), ()>(())).unwrap();
    assert_eq!(facts(&view, 0), Err(FactsRefused::ViewNotReady));
    // It waits: it is not a refusal of the request.
    assert!(FactsRefused::ViewNotReady.waits());
    assert!(FactsRefused::ReceiptLogFull.waits());
    assert!(!FactsRefused::NotThisCountersAddress.waits());
    // Once the view has caught up the same call answers.
    let view = view_with(&[(0, 5_000, DEPTH)]);
    assert!(facts(&view, 0).is_ok());
    assert_eq!(
        FactsRefused::ViewNotReady.to_string(),
        "OC-14: the wallet's view has not checked the chain up to the tip yet"
    );
}

#[test]
fn oc_14_an_index_issued_to_another_counter_is_refused() {
    let view = view_with(&[(0, 5_000, DEPTH), (19, 1_000, DEPTH), (20, 1_000, DEPTH)]);
    let other = new_owner_for(id(4).device_id());
    assert_eq!(
        verified_facts(&view, &other, sale(0), TIP, DEPTH),
        Err(FactsRefused::NotThisCountersAddress)
    );
    // The first and the last index of the range are the counter's; one past
    // it never was.
    assert!(facts(&view, 0).is_ok());
    assert!(facts(&view, 19).is_ok());
    assert_eq!(facts(&view, 20), Err(FactsRefused::NotThisCountersAddress));
    // An owner that issued nothing refuses every index.
    let fresh = openbmt_owner::Owner::new(
        &[1; 32],
        super::refunds::new_owner().wallet().clone(),
        id(2).device_id(),
        openbmt_core::batch::BatchPolicy { min_count: 3 },
        openbmt_owner::Options::default(),
    )
    .unwrap();
    assert_eq!(
        verified_facts(&view, &fresh, sale(0), TIP, DEPTH),
        Err(FactsRefused::NotThisCountersAddress)
    );
    // A view that is not ready is tried first: the request waits.
    assert_eq!(
        verified_facts(&CoinView::new(0), &other, sale(0), TIP, DEPTH),
        Err(FactsRefused::ViewNotReady)
    );
    assert!(FactsRefused::NotThisCountersAddress
        .to_string()
        .starts_with("OC-14:"));
}

#[test]
fn oc_14_the_amount_is_the_sum_of_the_receipts_at_the_index() {
    let view = view_with(&[(0, 3_000, DEPTH), (0, 2_000, DEPTH), (1, 9_000, DEPTH)]);
    assert_eq!(
        facts(&view, 0),
        Ok(SaleFacts {
            received_sats: 5_000,
            reached_refund_depth: true
        })
    );
    // Nothing received is not deep.
    assert_eq!(
        facts(&view, 2),
        Ok(SaleFacts {
            received_sats: 0,
            reached_refund_depth: false
        })
    );
    // A payment at any depth of at least 1 counts for the amount.
    let view = view_with(&[(0, 3_000, 1)]);
    assert_eq!(facts(&view, 0).unwrap().received_sats, 3_000);
    // A payment in a block above the tip given is not counted.
    assert_eq!(
        verified_facts(&view, &new_owner(), sale(0), TIP - 1, DEPTH)
            .unwrap()
            .received_sats,
        0
    );
}

#[test]
fn oc_14_the_refund_depth_needs_every_payment_at_the_index_to_be_deep() {
    let at = |depths: &[u32]| {
        let paid: Vec<(u32, u64, u32)> = depths.iter().map(|d| (0, 1_000, *d)).collect();
        facts(&view_with(&paid), 0).unwrap().reached_refund_depth
    };
    assert!(at(&[DEPTH]), "exactly the refund depth");
    assert!(!at(&[DEPTH - 1]), "one block short");
    assert!(at(&[DEPTH + 10, DEPTH]));
    assert!(!at(&[DEPTH + 10, 1]), "one shallow payment");
    assert!(!at(&[DEPTH, DEPTH, DEPTH - 1]));
    assert!(!at(&[]), "nothing received");
}
