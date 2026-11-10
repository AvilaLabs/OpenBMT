//! PS-20 to PS-22: the sale reference.

use super::kit::*;
use crate::reference::{ALPHABET, REF_LEN};
use crate::*;

#[test]
fn ps_20_a_reference_is_six_characters_of_the_alphabet() {
    let code = RefCode::for_sale(&counter(3), 17);
    assert_eq!(code.as_str().len(), REF_LEN);
    assert!(code.as_bytes().iter().all(|b| ALPHABET.contains(b)));
    for bad in b"01OIL" {
        assert!(!ALPHABET.contains(bad));
    }
    assert_eq!(ALPHABET.len(), 31);
    // A pure function of the key and the index.
    assert_eq!(code, RefCode::for_sale(&counter(3), 17));
    assert_ne!(code, RefCode::for_sale(&counter(3), 18));
    assert_ne!(code, RefCode::for_sale(&counter(4), 17));
    assert_eq!(RefCode::parse(code.as_str()), Some(code));
}

#[test]
fn ps_20_the_codes_of_many_sales_are_nearly_all_different() {
    let mut seen = std::collections::BTreeSet::new();
    for i in 0..2_000u32 {
        seen.insert(RefCode::for_sale(&counter(3), i));
    }
    // 31^5 is about 28 million: a few collisions in 2,000 at most.
    assert!(seen.len() >= 1_990, "{}", seen.len());
}

#[test]
fn ps_20_the_check_sees_any_one_wrong_character_and_any_swap() {
    for i in 0..200u32 {
        let code = RefCode::for_sale(&counter(5), i);
        let chars: Vec<char> = code.as_str().chars().collect();
        for place in 0..REF_LEN {
            for &other in ALPHABET {
                let other = char::from(other);
                if other == chars[place] {
                    continue;
                }
                let mut changed = chars.clone();
                changed[place] = other;
                let s: String = changed.into_iter().collect();
                assert_eq!(RefCode::parse(&s), None, "{code} -> {s}");
            }
        }
        for place in 0..REF_LEN - 1 {
            if chars[place] == chars[place + 1] {
                continue;
            }
            let mut swapped = chars.clone();
            swapped.swap(place, place + 1);
            let s: String = swapped.into_iter().collect();
            assert_eq!(RefCode::parse(&s), None, "{code} -> {s}");
        }
    }
}

#[test]
fn ps_21_a_reference_is_found_in_free_text_whatever_the_case() {
    let code = RefCode::for_sale(&counter(3), 4);
    let s = code.as_str().to_string();
    let lower = s.to_ascii_lowercase();
    for text in [
        s.clone(),
        lower.clone(),
        format!("btc {s}"),
        format!("BTC {lower}"),
        format!("  {s}  "),
        format!("paid: {s}."),
        format!("order 12, ref={s}, thanks"),
        format!("btc\t{s}\n"),
    ] {
        assert_eq!(find_reference(&text), Some(code), "{text:?}");
    }
}

#[test]
fn ps_21_text_without_exactly_one_reference_finds_nothing() {
    let a = RefCode::for_sale(&counter(3), 1);
    let b = RefCode::for_sale(&counter(3), 2);
    assert_eq!(find_reference(""), None);
    assert_eq!(find_reference("bitcoin"), None);
    assert_eq!(find_reference("btc"), None);
    // Glued to other characters it is not a run of six.
    assert_eq!(find_reference(&format!("btc{a}")), None);
    assert_eq!(find_reference(&format!("{a}9")), None);
    // Two different references are ambiguous; the same twice is not.
    assert_eq!(find_reference(&format!("{a} {b}")), None);
    assert_eq!(find_reference(&format!("{a} {a}")), Some(a));
    // A look-alike is not guessed.
    let bad = a.as_str().replacen(|c: char| c.is_ascii_digit(), "O", 1);
    if bad != a.as_str() {
        assert_eq!(find_reference(&bad), None);
    }
}

#[test]
fn ps_22_a_reference_never_makes_a_sale_paid() {
    // The cashier types a good reference for a sale the counter never
    // reported paid. It is matched to the sale, and the alert says the sale
    // is not paid. Nothing about the sale changes.
    let s = sale(3, 1, 500, T0);
    let mut t = tender("t", 500, T0 + 30);
    t.reference = Some(text(&format!("btc {}", s.reference.unwrap())));
    let o = order("o1", "Main", vec![t]);
    let mut l = Ledger::new();
    step(&mut l, &[o], &[s.clone()], T0 + 2_000);
    assert_eq!(l.matches().len(), 1);
    assert_eq!(
        l.matches().values().next().unwrap().how,
        MatchHow::Reference
    );
    assert_eq!(kinds(&l), vec![AlertKind::TenderOnUnpaidSale]);
    assert_eq!(s.paid_at, None);
}

#[test]
fn ps_81_the_idempotency_key_is_the_hash_of_the_key_and_the_index() {
    let k = IdempotencyKey::for_sale(&counter(3), 9);
    assert_eq!(k, IdempotencyKey::for_sale(&counter(3), 9));
    assert_ne!(k, IdempotencyKey::for_sale(&counter(3), 10));
    assert_ne!(k, IdempotencyKey::for_sale(&counter(4), 9));
    assert_eq!(k.to_hex().len(), 64);
}
