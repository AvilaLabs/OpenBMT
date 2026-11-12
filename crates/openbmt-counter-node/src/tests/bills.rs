//! Bills, tabs and the till's reference through the node (CR-245 to CR-253):
//! a part paid by a block settles its bill, and bills are on disk after a
//! restart.

use openbmt_counter::{Amount, BillKind, PartWay, Report, SaleExtras};

use super::kit::*;

fn extras(reference: Option<&'static str>) -> SaleExtras<'static> {
    SaleExtras {
        note: None,
        request: None,
        reference,
    }
}

fn open_part(rig: &mut Rig, number: u32, cents: u64) -> u32 {
    rig.node
        .open_bill_part(
            number,
            Amount(cents),
            Amount(0),
            rate(),
            &source_at(&rig.time),
            None,
            extras(None),
        )
        .unwrap()
        .index
}

#[test]
fn cr_249_a_part_paid_in_a_block_settles_the_bill_and_a_restart_keeps_it() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    let number = rig
        .node
        .open_bill(BillKind::Bill, Amount(800), Some("T-14"))
        .unwrap();
    let part = open_part(&mut rig, number, 500);
    rig.node
        .add_other_part(number, Amount(300), PartWay::Cash)
        .unwrap();
    assert_eq!(rig.node.bill_of_sale(part), Some(number));
    let views = rig.node.bill_views();
    assert_eq!(views[0].left, 500);
    assert!(views[0].bill.is_open());

    let pay = spend(1, &[(&script_of(part), 5_000)]);
    let mut blocks = Blocks::default();
    let first = blocks.extend(std::slice::from_ref(&pay)).clone();
    let reports = rig.feed(vec![joined(1, &first), block_event(&first)]);
    assert_eq!(reports, vec![Report::PosPaid { index: part }]);
    let views = rig.node.bill_views();
    assert_eq!(views[0].left, 0);
    assert!(!views[0].bill.is_open());

    drop(rig);
    let rig = start(dir.path());
    let views = rig.node.bill_views();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].bill.reference(), Some("T-14"));
    assert!(!views[0].bill.is_open());
    // The number was stored: the next bill does not reuse it.
    assert_eq!(rig.node.counter().next_bill_number(), number + 1);
}

#[test]
fn cr_245_a_reference_added_to_a_sale_is_kept_after_a_restart() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    let opened = rig
        .node
        .open(five_dollars(), usd(), rate(), &source_at(&rig.time))
        .unwrap();
    rig.node.add_reference(opened.index, "T-9").unwrap();
    assert_eq!(rig.node.counter().sale_reference(opened.index), Some("T-9"));
    drop(rig);
    let rig = start(dir.path());
    assert_eq!(rig.node.counter().sale_reference(0), Some("T-9"));
}

#[test]
fn cr_251_a_tab_is_raised_and_finished_through_the_node() {
    let dir = Scratch::new();
    let mut rig = start(dir.path());
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    let number = rig
        .node
        .open_bill(BillKind::Tab, Amount(500), None)
        .unwrap();
    rig.node.set_tab_amount(number, Amount(800)).unwrap();
    rig.node
        .add_other_part(number, Amount(800), PartWay::Card)
        .unwrap();
    // A tab is settled only by the cashier.
    assert!(rig.node.bill_views()[0].bill.is_open());
    rig.node.finish_tab(number).unwrap();
    assert!(!rig.node.bill_views()[0].bill.is_open());
}

