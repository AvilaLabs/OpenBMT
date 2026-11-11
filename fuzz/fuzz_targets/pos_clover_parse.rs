#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter::Currency;
use openbmt_pos::{Cursor, LocationCounters, Settings, Text, MAX_CURSOR_BYTES, MAX_PAGE_ORDERS};
use openbmt_pos_readers::{clover::Request, ReadRefused};

fuzz_target!(|data: &[u8]| {
    // The first byte gives the page size, the second the length of the
    // cursor, then the cursor, then the reply.
    let (Some(&flags), Some(&clen)) = (data.first(), data.get(1)) else {
        return;
    };
    let rest = data.get(2..).unwrap_or(&[]);
    let clen = usize::from(clen % 48).min(rest.len());
    let (cursor, body) = rest.split_at(clen);
    let Ok(cursor) = Cursor::new(cursor.to_vec()) else {
        return;
    };
    let (Ok(name), Ok(location), Some(usd)) = (
        Text::new("Bitcoin"),
        Text::new("MERCHANT01"),
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
    let limit = 1 + usize::from(flags) * 2;
    let Ok(request) = Request::new("MERCHANT01", &settings, limit, &cursor) else {
        return;
    };
    // The query is ASCII with nothing in it that ends or splits a value.
    let query = request.path_and_query();
    assert!(query.starts_with("/v3/merchants/MERCHANT01/orders?filter="));
    assert!(query
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/?&=%-._~".contains(&b)));
    assert_eq!(query, request.path_and_query());

    let result = request.parse(body);
    // The same bytes always give the same answer.
    assert_eq!(result, request.parse(body));
    match result {
        Ok(page) => {
            assert!(page.orders.len() <= MAX_PAGE_ORDERS && page.orders.len() <= limit);
            assert!(page.next.as_bytes().len() <= MAX_CURSOR_BYTES);
            for o in &page.orders {
                o.validate()
                    .expect("an order that is read is within its bounds");
            }
            // A page of exactly the limit has more, a shorter one has not,
            // and the cursor it gives is one the next request takes.
            assert_eq!(page.more, page.orders.len() == limit);
            Request::new("MERCHANT01", &settings, limit, &page.next)
                .expect("the next cursor reads");
        }
        Err(ReadRefused::TooLong) => assert!(body.len() > 8 * 1024 * 1024),
        Err(ReadRefused::NotUtf8) => assert!(std::str::from_utf8(body).is_err()),
        Err(_) => {}
    }
});
