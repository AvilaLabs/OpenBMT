//! PS-80 to PS-82: the connector boundary.

use super::kit::*;
use crate::*;

fn cursor(n: u8) -> Cursor {
    Cursor::new(vec![n]).unwrap()
}

fn page(orders: Vec<PosOrder>, next: u8, more: bool) -> Page {
    Page {
        orders,
        next: cursor(next),
        more,
    }
}

#[test]
fn ps_80_a_sync_follows_the_pages_from_the_stored_cursor() {
    let mut l = Ledger::new();
    let mut pos = MemoryPos::new([
        page(vec![simple("a", "Main", 100, T0)], 1, true),
        page(vec![simple("b", "Main", 200, T0)], 2, true),
        page(vec![], 3, false),
    ]);
    let r = sync(&mut pos, &mut l, &[], &settings(), T0 + 10, ok).unwrap();
    assert_eq!((r.pages, r.orders_read, r.more), (3, 2, false));
    assert_eq!(pos.polled(), &[Cursor::start(), cursor(1), cursor(2)]);
    assert_eq!(l.cursor(), &cursor(3));
    // The next sync starts where the last one ended.
    let mut pos = MemoryPos::default();
    sync(&mut pos, &mut l, &[], &settings(), T0 + 20, ok).unwrap();
    assert_eq!(pos.polled(), &[cursor(3)]);
}

#[test]
fn ps_80_a_page_of_more_than_five_hundred_orders_is_refused() {
    let many: Vec<PosOrder> = (0..=MAX_PAGE_ORDERS)
        .map(|i| simple(&format!("o{i}"), "Main", 100, T0))
        .collect();
    let mut l = Ledger::new();
    let r = sync(
        &mut MemoryPos::new([page(many.clone(), 1, false)]),
        &mut l,
        &[],
        &settings(),
        T0,
        ok,
    );
    assert_eq!(r.unwrap_err().error, SyncError::PageTooLarge);
    assert_eq!(l, Ledger::new());
    let fits = many[..MAX_PAGE_ORDERS].to_vec();
    assert!(sync(
        &mut MemoryPos::new([page(fits, 1, false)]),
        &mut l,
        &[],
        &settings(),
        T0,
        ok
    )
    .is_ok());
}

#[test]
fn ps_80_a_reader_that_says_more_and_does_not_move_is_refused() {
    let mut l = Ledger::new();
    let stall = Page {
        orders: vec![],
        next: Cursor::start(),
        more: true,
    };
    let r = sync(
        &mut MemoryPos::new([stall]),
        &mut l,
        &[],
        &settings(),
        T0,
        ok,
    );
    assert_eq!(r.unwrap_err().error, SyncError::NoProgress);
}

#[test]
fn ps_80_a_sync_pulls_at_most_sixty_four_pages_and_leaves_the_rest() {
    let pages: Vec<Page> = (1..=70u8)
        .map(|i| page(vec![simple(&format!("o{i}"), "Main", 100, T0)], i, true))
        .collect();
    let mut pos = MemoryPos::new(pages);
    let mut l = Ledger::new();
    let r = sync(&mut pos, &mut l, &[], &settings(), T0 + 10, ok).unwrap();
    assert_eq!(
        (r.pages, r.more, r.orders_read),
        (MAX_PAGES, true, MAX_PAGES)
    );
    assert_eq!(pos.remaining(), 70 - MAX_PAGES);
    assert_eq!(l.cursor(), &cursor(64));
    // The next sync goes on from there.
    let r = sync(&mut pos, &mut l, &[], &settings(), T0 + 20, ok).unwrap();
    assert_eq!(r.orders_read, 6);
    assert_eq!(r.pages, 7);
    assert_eq!(pos.polled().last(), Some(&cursor(70)));
}

#[test]
fn ps_80_a_sync_hands_the_matcher_the_new_matches_and_alerts() {
    let s = paid(sale(3, 1, 500, T0), T0 + 60);
    let mut l = Ledger::new();
    let mut pos = MemoryPos::new([page(
        vec![
            simple("a", "Main", 500, T0 + 90),
            simple("b", "Main", 999, T0 + 90),
        ],
        1,
        false,
    )]);
    let r = sync(&mut pos, &mut l, &[s], &settings(), T0 + 2_000, ok).unwrap();
    assert_eq!(r.delta.new_matches.len(), 1);
    assert_eq!(r.delta.new_alerts.len(), 1);
    assert_eq!(
        r.delta.new_alerts[0].key.kind,
        AlertKind::TenderWithoutPayment
    );
}

#[test]
fn ps_81_an_order_is_closed_only_for_a_sale_reported_paid_now() {
    let unpaid = sale(3, 1, 500, T0);
    let r = CloseRequest::for_sale(&unpaid, oid("o"), text("Bitcoin"));
    assert_eq!(r, Err(CloseRefused::SaleNotPaid));
    let mut again = paid(sale(3, 1, 500, T0), T0 + 5);
    again.unpaid_after = true;
    assert_eq!(
        CloseRequest::for_sale(&again, oid("o"), text("Bitcoin")),
        Err(CloseRefused::SaleNotPaid)
    );
    let ok_sale = paid(sale(3, 1, 500, T0), T0 + 5);
    let req = CloseRequest::for_sale(&ok_sale, oid("o"), text("Bitcoin")).unwrap();
    assert_eq!(req.amount_minor, 500);
    assert_eq!(req.currency, usd());
    assert_eq!(req.sale, ok_sale.key());
    assert_eq!(req.key, IdempotencyKey::for_sale(&counter(3), 1));
}

#[test]
fn ps_81_a_retry_with_the_same_key_does_not_close_twice() {
    let s = paid(sale(3, 1, 500, T0), T0 + 5);
    let req = CloseRequest::for_sale(&s, oid("o"), text("Bitcoin")).unwrap();
    let mut closer = MemoryCloser::default();
    assert_eq!(closer.close(&req), Ok(CloseOutcome::Closed));
    assert_eq!(closer.close(&req), Ok(CloseOutcome::AlreadyClosed));
    assert_eq!(closer.closed.len(), 1);
    // Another sale has another key.
    let s2 = paid(sale(3, 2, 500, T0), T0 + 5);
    let req2 = CloseRequest::for_sale(&s2, oid("o2"), text("Bitcoin")).unwrap();
    assert_ne!(req.key, req2.key);
    assert_eq!(closer.close(&req2), Ok(CloseOutcome::Closed));
    // A connector without the capability says so.
    struct NoClose;
    impl PosCloser for NoClose {
        fn close(&mut self, _: &CloseRequest) -> Result<CloseOutcome, PosError> {
            Err(PosError::Unsupported)
        }
    }
    assert_eq!(NoClose.close(&req), Err(PosError::Unsupported));
}

#[test]
fn ps_82_the_crate_has_no_network_clock_file_or_vendor_code_and_no_new_dependency() {
    let sources = [
        include_str!("../alerts.rs"),
        include_str!("../connector.rs"),
        include_str!("../facts.rs"),
        include_str!("../ledger.rs"),
        include_str!("../lib.rs"),
        include_str!("../matcher.rs"),
        include_str!("../model.rs"),
        include_str!("../reference.rs"),
        include_str!("../settings.rs"),
    ];
    for src in sources {
        for banned in [
            "std::net",
            "std::fs",
            "std::time",
            "std::thread",
            "std::process",
            "unsafe {",
            "http",
            "Square",
            "Clover",
            "Lightspeed",
            "Toast",
        ] {
            assert!(!src.contains(banned), "{banned}");
        }
    }
    let manifest = include_str!("../../Cargo.toml");
    let deps: Vec<&str> = manifest
        .split("[dependencies]")
        .nth(1)
        .unwrap()
        .lines()
        .filter(|l| l.contains('='))
        .collect();
    assert_eq!(deps.len(), 3, "{deps:?}");
    assert!(deps
        .iter()
        .all(|d| d.starts_with("openbmt-") || d.starts_with("bitcoin")));
}

#[test]
fn ps_82_the_scripted_reader_answers_an_empty_page_when_the_script_is_spent() {
    let mut pos = MemoryPos::default();
    let c = cursor(4);
    let p = pos.poll(&c).unwrap();
    assert_eq!(
        p,
        Page {
            orders: vec![],
            next: c,
            more: false
        }
    );
}
