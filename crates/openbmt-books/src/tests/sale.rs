//! BK-05 to BK-11: one sale and its fields.

use openbmt_core::identity::tagged_hash;
use openbmt_counter::Event;

use super::fixture::*;
use crate::{
    BooksError, Counting, PosFacts, PosMark, SaleId, SaleState, Settlement, UnknownField, Value,
};

fn state_of(kit: &Kit, index: u32) -> SaleState {
    kit.books().sale(&kit.key(index)).expect("sale").state
}

#[test]
fn bk_05_the_state_is_the_one_the_last_event_gives() {
    let kit = rich_kit();
    assert_eq!(state_of(&kit, 0), SaleState::Final);
    assert_eq!(state_of(&kit, 1), SaleState::Expired);
    assert_eq!(state_of(&kit, 2), SaleState::Cancelled);
    assert_eq!(state_of(&kit, 3), SaleState::LatePayment);
    assert_eq!(state_of(&kit, 4), SaleState::Confirmed);
    assert_eq!(state_of(&kit, 5), SaleState::Final);
    assert_eq!(state_of(&kit, 6), SaleState::Replaced);
    assert_eq!(state_of(&kit, 7), SaleState::Open);
}

#[test]
fn bk_05_paid_at_once_then_confirmed_keeps_the_amount_at_risk() {
    let kit = rich_kit();
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.at_risk_sats, Some(SATS_PAID));
    assert_eq!(s.first_funds_at, Some(JAN_31_235930));
    assert_eq!(s.counting(), Counting::Counted);
}

#[test]
fn bk_05_a_replaced_sale_is_not_paid_until_it_is_paid_again() {
    let kit = rich_kit();
    let s = kit.books().sale(&kit.key(6)).expect("sale").clone();
    assert!(!s.funds_seen && s.first_funds_at.is_none());
    let mut events = rich_events();
    events.push(Event::Confirmed {
        index: 6,
        at: FEB_2_NOON + 800,
        received_sats: SATS_PAID,
        depth: 1,
    });
    let again = kit_with(&events).books();
    let s = again.sale(&kit.key(6)).expect("sale");
    assert_eq!(s.counting(), Counting::Pending);
    assert_eq!(s.first_funds_at, Some(FEB_2_NOON + 800));
}

#[test]
fn bk_05_a_reopened_sale_is_pending_again() {
    let mut events = rich_events();
    events.push(Event::Reopened {
        index: 0,
        at: FEB_2_NOON + 7000,
    });
    let kit = kit_with(&events);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.state, SaleState::Reopened);
    assert_eq!(s.counting(), Counting::Pending);
}

#[test]
fn bk_05_a_late_payment_gives_its_state_only_to_a_sale_without_its_own() {
    let mut events = rich_events();
    events.push(Event::LatePayment {
        index: 0,
        at: FEB_2_NOON + 8000,
        txid: [0x11; 32],
        sats: 5,
    });
    let kit = kit_with(&events);
    assert_eq!(state_of(&kit, 0), SaleState::Final);
}

#[test]
fn bk_06_the_fields_the_records_carry_are_known() {
    let kit = rich_kit();
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.opened_at, JAN_31_235930 - 60);
    assert_eq!((s.amount_minor, s.currency), (1850, usd()));
    assert_eq!(s.first_quote_sats, 4_321_987);
    assert_eq!(s.quote_rate_minor_per_btc, 4_281_700);
    assert_eq!(s.sats_received, Value::Known(SATS_PAID));
    let late = books.sale(&kit.key(3)).expect("sale");
    assert_eq!(late.late_payments.len(), 1);
    assert_eq!(late.late_payments[0].txid, TXID_LATE);
    let o = books.sale(&kit.key(5)).expect("sale");
    assert_eq!(o.overrides[0].name, NASTY_NAME);
    assert_eq!(o.pos, PosMark::Paid);
    let d = books.sale(&kit.key(7)).expect("sale");
    assert_eq!(d.dust_sats, SATS_DUST);
}

#[test]
fn bk_06_a_field_the_records_do_not_carry_is_an_unknown_that_explains_itself() {
    let kit = rich_kit();
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(s.rate_source, Value::Unknown(UnknownField::RateSource));
    assert_eq!(
        s.value_at_first_seen_minor,
        Value::Unknown(UnknownField::ValueAtFirstSeen)
    );
    assert_eq!(s.sales_tax_minor, Value::Unknown(UnknownField::SalesTax));
    assert_eq!(s.address, Value::Unknown(UnknownField::Address));
    for f in [
        UnknownField::RateSource,
        UnknownField::ValueAtFirstSeen,
        UnknownField::SalesTax,
        UnknownField::SatsNotRecorded,
        UnknownField::SatsLateAmbiguous,
        UnknownField::Address,
        UnknownField::Txid,
        UnknownField::DisposalValue,
        UnknownField::DisposalBasis,
    ] {
        let u = f.explain();
        assert!(u.why.len() > 10 && u.next_step.len() > 10, "{f:?}");
        assert_eq!(UnknownField::from_u8(f as u8), Some(f));
        assert!(!f.name().is_empty());
    }
}

#[test]
fn bk_06_overpaid_is_read_from_a_surplus_and_underpaid_from_the_first_quote() {
    let kit = rich_kit();
    let books = kit.books();
    assert_eq!(
        books.sale(&kit.key(5)).expect("sale").settlement(),
        Some(Settlement::Overpaid {
            surplus_sats: SATS_SURPLUS
        })
    );
    assert_eq!(books.sale(&kit.key(0)).expect("sale").settlement(), None);
    let short = kit_with(&[
        opened(0, 10, 100),
        Event::Final {
            index: 0,
            at: 11,
            received_sats: 4_000_000,
            depth: 6,
        },
    ]);
    assert_eq!(
        short
            .books()
            .sale(&short.key(0))
            .expect("sale")
            .settlement(),
        Some(Settlement::BelowFirstQuote {
            short_sats: 321_987
        })
    );
}

fn crockford_by_bits(bytes: &[u8; 8]) -> String {
    const A: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut bits: Vec<u8> = Vec::new();
    for b in bytes {
        for i in (0..8).rev() {
            bits.push((b >> i) & 1);
        }
    }
    bits.push(0);
    let mut out = String::new();
    for (n, chunk) in bits.chunks(5).enumerate() {
        if n == 4 || n == 8 {
            out.push('-');
        }
        let v = chunk.iter().fold(0usize, |a, b| a * 2 + usize::from(*b));
        out.push(char::from(A[v]));
    }
    out
}

#[test]
fn bk_07_the_sale_id_is_13_crockford_characters_in_groups_of_4_4_5() {
    let kit = rich_kit();
    let key = kit.key(3);
    let digest = {
        let mut msg = Vec::new();
        msg.extend_from_slice(&key.counter.to_bytes());
        msg.extend_from_slice(&3u32.to_le_bytes());
        tagged_hash("OpenBMT/books-sale/v1", &msg)
    };
    let mut first8 = [0u8; 8];
    first8.copy_from_slice(&digest[..8]);
    let id = SaleId::for_sale(&key);
    assert_eq!(id.as_bytes(), &first8);
    let text = id.to_string();
    assert_eq!(text, crockford_by_bits(&first8));
    let groups: Vec<usize> = text.split('-').map(str::len).collect();
    assert_eq!(groups, vec![4, 4, 5]);
    assert!(text
        .chars()
        .all(|c| c == '-' || "0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(c)));
    assert_eq!(kit.books().sale(&key).expect("sale").id, id);
}

#[test]
fn bk_07_the_sale_id_is_stable_and_differs_by_sale_and_counter() {
    let kit = rich_kit();
    let a = SaleId::for_sale(&kit.key(1));
    assert_eq!(a, SaleId::for_sale(&kit.key(1)));
    assert_ne!(a, SaleId::for_sale(&kit.key(2)));
    let other = crate::SaleKey {
        counter: identity(5).device_id(),
        index: 1,
    };
    assert_ne!(a, SaleId::for_sale(&other));
    // It is a function of public values only: anyone can recompute it.
    assert_eq!(a, SaleId::for_sale(&kit.key(1)));
}

#[test]
fn bk_08_only_a_sale_that_received_funds_is_an_income_line() {
    let kit = rich_kit();
    let books = kit.books();
    let counting: Vec<Counting> = (0..8)
        .map(|i| books.sale(&kit.key(i)).expect("sale").counting())
        .collect();
    assert_eq!(
        counting,
        vec![
            Counting::Counted,
            Counting::NoFunds,
            Counting::NoFunds,
            Counting::Pending,
            Counting::Pending,
            Counting::Counted,
            Counting::NoFunds,
            Counting::NoFunds,
        ]
    );
}

#[test]
fn bk_09_the_sats_received_come_from_the_last_confirmed_or_final_event() {
    let kit = rich_kit();
    let books = kit.books();
    let sats = |i| books.sale(&kit.key(i)).expect("sale").sats_received.clone();
    assert_eq!(sats(0), Value::Known(SATS_PAID));
    assert_eq!(sats(5), Value::Known(SATS_PAID + SATS_SURPLUS));
    assert_eq!(sats(3), Value::Known(SATS_LATE));
    assert_eq!(sats(1), Value::Known(0));
    assert_eq!(sats(2), Value::Unknown(UnknownField::SatsNotRecorded));
}

#[test]
fn bk_09_a_sale_only_accepted_at_once_has_unknown_sats() {
    let kit = kit_with(&[
        opened(0, 10, 100),
        Event::PaidAtOnce {
            index: 0,
            at: 11,
            at_risk_sats: 777,
            label: openbmt_core::policy::InstantLabel::YourBox,
            mode: openbmt_core::policy::CheckMode::BoxReportsOnly,
        },
    ]);
    let books = kit.books();
    let s = books.sale(&kit.key(0)).expect("sale");
    assert_eq!(
        s.sats_received,
        Value::Unknown(UnknownField::SatsNotRecorded)
    );
    assert_eq!(s.at_risk_sats, Some(777));
}

#[test]
fn bk_09_a_late_payment_beside_a_confirmation_makes_the_sum_unknown() {
    let kit = kit_with(&[
        opened(0, 10, 100),
        Event::Confirmed {
            index: 0,
            at: 11,
            received_sats: 500,
            depth: 1,
        },
        Event::LatePayment {
            index: 0,
            at: 12,
            txid: [3; 32],
            sats: 40,
        },
    ]);
    let books = kit.books();
    assert_eq!(
        books.sale(&kit.key(0)).expect("sale").sats_received,
        Value::Unknown(UnknownField::SatsLateAmbiguous)
    );
}

#[test]
fn bk_10_sales_tax_comes_only_from_the_pos_facts() {
    let kit = rich_kit();
    let mut facts = PosFacts::new();
    facts.insert_tax(kit.key(0), 148);
    let books = kit.books().with_pos_facts(&facts).expect("facts");
    assert_eq!(
        books.sale(&kit.key(0)).expect("sale").sales_tax_minor,
        Value::Known(148)
    );
    assert_eq!(
        books.sale(&kit.key(5)).expect("sale").sales_tax_minor,
        Value::Unknown(UnknownField::SalesTax)
    );
}

#[test]
fn bk_10_a_tax_for_a_sale_not_in_the_books_is_refused() {
    let kit = rich_kit();
    let mut facts = PosFacts::new();
    facts.insert_tax(kit.key(99), 1);
    let err = kit.books().with_pos_facts(&facts).expect_err("refused");
    assert_eq!(err, BooksError::SaleNotInBooks { sale: kit.key(99) });
}

#[test]
fn bk_11_the_high_water_mark_is_the_highest_sale_index_named() {
    let kit = rich_kit();
    assert_eq!(
        kit.books().high_water().get(&kit.counter_id()),
        Some(&Some(7))
    );
    let empty = kit_with(&[]);
    assert_eq!(
        empty.books().high_water().get(&empty.counter_id()),
        Some(&None)
    );
    // The mark follows the highest index, not the order of the records.
    let out_of_order = kit_with(&[opened(9, 5, 1), opened(2, 6, 1)]);
    assert_eq!(
        out_of_order
            .books()
            .high_water()
            .get(&out_of_order.counter_id()),
        Some(&Some(9))
    );
}
