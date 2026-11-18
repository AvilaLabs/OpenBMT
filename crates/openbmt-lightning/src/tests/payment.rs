//! LN-13, LN-21.

use super::*;
use crate::error::AskRefused;
use crate::invoice::{sale_description, MAX_EXPIRY_SECS, MAX_MSAT};
use crate::PaidFacts;

#[test]
fn ln_13_the_description_is_the_sale_number_and_nothing_else() {
    assert_eq!(sale_description(17), "Sale 17");
    assert_eq!(sale_description(0), "Sale 0");
    assert_eq!(sale_description(u64::MAX), "Sale 18446744073709551615");
    assert_eq!(ask().description(), "Sale 17");
}

#[test]
fn ln_13_the_amount_is_in_millisatoshis_and_the_lifetime_is_the_quotes() {
    let a = ask();
    assert_eq!(a.amount_msat(), 25_000_000);
    assert_eq!(a.expiry_secs(), 300);
}

#[test]
fn ln_13_an_amount_or_a_lifetime_out_of_range_is_not_asked_for() {
    let sale = |msat, secs| Ask::sale(1, msat, secs);
    assert!(sale(1, 1).is_ok());
    assert!(sale(MAX_MSAT, MAX_EXPIRY_SECS).is_ok());
    assert_eq!(sale(0, 300).unwrap_err(), AskRefused::BadAmount);
    assert_eq!(sale(MAX_MSAT + 1, 300).unwrap_err(), AskRefused::BadAmount);
    assert_eq!(sale(u64::MAX, 300).unwrap_err(), AskRefused::BadAmount);
    assert_eq!(sale(1_000, 0).unwrap_err(), AskRefused::BadExpiry);
    assert_eq!(
        sale(1_000, MAX_EXPIRY_SECS + 1).unwrap_err(),
        AskRefused::BadExpiry
    );
}

#[test]
fn ln_21_a_paid_invoice_is_final_and_gives_its_facts() {
    // The hash, the amount received and the time the node settled it: all
    // the counter keeps of a payment that cannot be undone.
    let facts = PaidFacts {
        payment_hash: hash(1),
        amount_received_msat: 25_000_000,
        settled_at: Some(1_700_000_123),
    };
    let kept = facts;
    assert_eq!(kept, facts);
    assert_eq!(kept.payment_hash, hash(1));
    assert_eq!(kept.amount_received_msat, ask().amount_msat());
    assert_eq!(kept.settled_at, Some(1_700_000_123));
}
