#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter::Currency;
use openbmt_pos::{Cursor, LocationCounters, Settings, Text, MAX_CURSOR_BYTES, MAX_PAGE_ORDERS};
use openbmt_pos_readers::{loyverse, ReadRefused};

fuzz_target!(|data: &[u8]| {
    // The first byte gives the decimal places and the second the length of the
    // cursor, then the cursor, then the reply; the rest of the reply is split
    // in two at its first zero byte into the receipts and the payment types.
    let (Some(&places), Some(&clen)) = (data.first(), data.get(1)) else {
        return;
    };
    let rest = data.get(2..).unwrap_or(&[]);
    let clen = usize::from(clen % 64).min(rest.len());
    let (cursor, body) = rest.split_at(clen);
    let Ok(cursor) = Cursor::new(cursor.to_vec()) else {
        return;
    };
    let (Ok(name), Ok(location), Some(usd)) = (
        Text::new("Bitcoin"),
        Text::new("STORE01"),
        Currency::new(*b"USD"),
    ) else {
        return;
    };
    let Ok(settings) = Settings::new(
        vec![name],
        vec![LocationCounters {
            location,
            counters: vec![],
        }],
        vec![usd],
    ) else {
        return;
    };
    let (receipts, tail) = match body.iter().position(|b| *b == 0) {
        Some(i) => (body.get(..i).unwrap_or(&[]), body.get(i + 1..)),
        None => (body, None),
    };
    let Ok(request) = loyverse::Request::new(&settings, usd, places % 12, &cursor) else {
        return;
    };
    // The query is ASCII with nothing in it that ends or splits a value.
    let query = request.path_and_query();
    assert!(query.starts_with("/v1.0/receipts?updated_at_min="));
    assert!(query
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/?&=%-._~".contains(&b)));
    assert_eq!(query, request.path_and_query());

    let result = request.parse(receipts, tail, None);
    // The same bytes always give the same answer.
    assert_eq!(result, request.parse(receipts, tail, None));
    // The merchant reply, on the same bytes, never panics.
    let _ = loyverse::read_merchant(body);
    match result {
        Ok(page) => {
            assert!(page.orders.len() <= MAX_PAGE_ORDERS);
            assert!(page.next.as_bytes().len() <= MAX_CURSOR_BYTES);
            for o in &page.orders {
                o.validate()
                    .expect("an order that is read is within its bounds");
            }
            // Oldest change first.
            assert!(page
                .orders
                .windows(2)
                .all(|w| w[0].updated_at <= w[1].updated_at));
            // The cursor it gives is one the next request takes.
            loyverse::Request::new(&settings, usd, places % 12, &page.next)
                .expect("the next cursor reads");
        }
        // Either reply may be the one that is refused.
        Err(ReadRefused::TooLong) => {
            assert!(receipts.len().max(tail.map_or(0, <[u8]>::len)) > 8 * 1024 * 1024)
        }
        Err(ReadRefused::NotUtf8) => assert!(
            std::str::from_utf8(receipts).is_err()
                || tail.is_some_and(|t| std::str::from_utf8(t).is_err())
        ),
        Err(_) => {}
    }
});
