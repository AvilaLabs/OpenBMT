#![no_main]

use libfuzzer_sys::fuzz_target;
use openbmt_counter::Currency;
use openbmt_json::{parse, Limits};
use openbmt_pos::{Cursor, LocationCounters, Settings, Text, MAX_CURSOR_BYTES, MAX_PAGE_ORDERS};
use openbmt_pos_readers::{square::Request, ReadRefused};

fuzz_target!(|data: &[u8]| {
    // The first byte says whether a payments reply is handed in and gives
    // the page size, the second the length of the cursor, then the cursor,
    // then the orders reply, then (if asked) a payments reply after a split.
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
        Text::new("LOC1"),
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
    let limit = 1 + usize::from(flags >> 1) * 4;
    let Ok(request) = Request::new(&settings, limit, &cursor) else {
        return;
    };
    // The body of the request is JSON with the three keys it always has.
    let text = request.body();
    let doc = parse(text.as_bytes(), &Limits::DEFAULT).expect("the body is JSON");
    assert!(doc.get("location_ids").is_some() && doc.get("query").is_some());
    assert_eq!(text, request.body());

    let (orders, payments) = if flags & 1 == 1 {
        let (a, b) = body.split_at(body.len() / 2);
        (a, Some(b))
    } else {
        (body, None)
    };
    let result = request.parse(orders, payments);
    // The same bytes always give the same answer.
    assert_eq!(result, request.parse(orders, payments));
    match result {
        Ok(page) => {
            assert!(page.orders.len() <= MAX_PAGE_ORDERS);
            assert!(page.next.as_bytes().len() <= MAX_CURSOR_BYTES);
            for o in &page.orders {
                o.validate()
                    .expect("an order that is read is within its bounds");
            }
            // The cursor it gives is one the next request takes, and a page
            // with more has the vendor's marker in it.
            let next = Request::new(&settings, limit, &page.next).expect("the next cursor reads");
            assert_eq!(page.more, page.next.as_bytes().contains(&b'|'));
            let _ = next.body();
        }
        Err(ReadRefused::TooLong) => assert!(
            orders.len() > 8 * 1024 * 1024 || payments.is_some_and(|p| p.len() > 8 * 1024 * 1024)
        ),
        Err(ReadRefused::NotUtf8) => assert!(
            std::str::from_utf8(orders).is_err()
                || payments.is_some_and(|p| std::str::from_utf8(p).is_err())
        ),
        Err(_) => {}
    }
});
