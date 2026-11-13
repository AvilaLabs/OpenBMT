//! The owner's payment requests on the node (`docs/REQUEST-RULES.md`): the
//! book in the node's folder, the desk's addresses in the live watch, the
//! payments credited from the blocks the view applies at the shop's depth,
//! the blocks that leave the chain and the returns on the owner's queue.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;

use openbmt_owner::Stage;

use openbmt_core::currency::ShopCurrency;
use openbmt_owner::own_book::{AllocationBook, RANGE_LEN};
use openbmt_requests::{
    day_of, Day, Every, NewRequest, PriceIn, ReturnAmount, ReturnState, Status,
};

fn external_address(n: u8) -> String {
    bitcoin::Address::from_script(&external(n), bitcoin::Network::Bitcoin)
        .unwrap()
        .to_string()
}

use super::live::{pay_to_script, DAY, START};
use super::own::{counter, own_receive, own_setup};
use super::*;
use crate::requests::{
    return_label, PendingArrival, RequestContext, RequestError, RequestsOff, REQUEST_ARRIVALS_FILE,
};
use crate::{NotWatching, OwnNode, REQUEST_BOOK_FILE};

fn context(depth: u8) -> RequestContext {
    RequestContext {
        currency: ShopCurrency::Usd,
        min_sale_sats: 0,
        min_confirmations: depth,
        tz_offset_secs: 0,
    }
}

fn due_in(days: u64) -> Day {
    day_of(START + days * DAY, 0)
}

fn deposit(amount: u64) -> NewRequest {
    NewRequest {
        note: "Deposit".to_string(),
        amount,
        price_in: PriceIn::Bitcoin,
        due: due_in(10),
        repeat: None,
    }
}

/// A node of a shop with its own wallet whose book already holds the desk's
/// range, with the shop's context given. Returns the rig, the book and the
/// desk.
fn desk_rig(dir: &Dir, depth: u8) -> (Rig, AllocationBook, Setup) {
    let (setup, _time) = own_setup(10_000);
    let mut rig = start(dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    let desk = rig.node.owner().owner_id();
    rig.give(&mut book, desk, START);
    rig.node.set_request_context(context(depth), None);
    (rig, book, setup)
}

fn payment(index: u32, from: u8, sats: u64) -> Transaction {
    pay_to_script(&own_receive(index), from, sats)
}

fn status(rig: &Rig, number: u64) -> Status {
    rig.node
        .requests()
        .unwrap()
        .request(number)
        .unwrap()
        .status(0)
}

fn received(rig: &Rig, number: u64) -> u64 {
    rig.node
        .requests()
        .unwrap()
        .request(number)
        .unwrap()
        .received_sats()
}

#[test]
fn rq_01_a_shop_with_a_key_app_has_no_requests_of_the_owner() {
    let dir = Dir::new();
    let mut rig = start(&dir, &Setup::default()).unwrap();
    assert_eq!(rig.node.requests().unwrap_err(), &RequestsOff::KeyApp);
    rig.node.set_request_context(context(1), None);
    let refused = rig.node.make_request(&deposit(100_000), 0, START);
    assert!(matches!(
        refused,
        Err(RequestError::Off(RequestsOff::KeyApp))
    ));
    assert!(refused
        .unwrap_err()
        .words()
        .heading
        .starts_with("Requests need own wallet"));
    assert!(!dir.path().join(REQUEST_BOOK_FILE).exists());
}

#[test]
fn rq_03_the_book_is_stored_before_the_address_is_given_and_a_restart_reads_it() {
    let dir = Dir::new();
    let (mut rig, book, setup) = desk_rig(&dir, 1);
    let made = rig.node.make_request(&deposit(100_000), 0, START).unwrap();
    assert_eq!(made.number, 1);
    assert_eq!(
        made.address,
        bitcoin::Address::from_script(&own_receive(0), bitcoin::Network::Bitcoin)
            .unwrap()
            .to_string()
    );
    // The file holds the request already.
    let on_disk = openbmt_requests::RequestBook::open(dir.path()).unwrap();
    assert_eq!(on_disk.requests().len(), 1);
    // An index a request has is refused, and nothing is written.
    let before = std::fs::read(dir.path().join(REQUEST_BOOK_FILE)).unwrap();
    let again = rig.node.make_request(&deposit(5_000), 0, START);
    assert!(matches!(again, Err(RequestError::Make(_))));
    assert_eq!(
        std::fs::read(dir.path().join(REQUEST_BOOK_FILE)).unwrap(),
        before
    );
    // The next request takes the next number and index.
    assert_eq!(
        rig.node
            .make_request(&deposit(5_000), 1, START)
            .unwrap()
            .number,
        2
    );
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let mut setup = setup;
    setup.own = Some(book);
    let rig = start_at(&dir, &setup, chain).unwrap();
    assert_eq!(rig.node.requests().unwrap().requests().len(), 2);
    assert_eq!(rig.node.request_indexes(), BTreeSet::from([0, 1]));
}

#[test]
fn rq_31_a_book_that_does_not_read_back_is_not_replaced_and_makes_no_request() {
    let dir = Dir::new();
    std::fs::write(dir.path().join(REQUEST_BOOK_FILE), b"not a book").unwrap();
    let (setup, _time) = own_setup(10_000);
    let mut rig = start(&dir, &setup).unwrap();
    assert!(matches!(
        rig.node.requests().unwrap_err(),
        RequestsOff::Unreadable(_)
    ));
    rig.node.set_request_context(context(1), None);
    let refused = rig
        .node
        .make_request(&deposit(100_000), 0, START)
        .unwrap_err();
    assert!(refused.is_store());
    assert_eq!(refused.words().code, Some("RQ-31"));
    assert_eq!(
        std::fs::read(dir.path().join(REQUEST_BOOK_FILE)).unwrap(),
        b"not a book"
    );
    // The same for a file of arrivals that does not read.
    let dir = Dir::new();
    std::fs::write(dir.path().join(REQUEST_ARRIVALS_FILE), b"no").unwrap();
    let rig = start(&dir, &setup).unwrap();
    assert!(matches!(
        rig.node.requests().unwrap_err(),
        RequestsOff::Unreadable(_)
    ));
}

#[test]
fn rq_04_the_desk_address_is_watched_from_the_moment_it_is_made_and_never_set_aside() {
    let dir = Dir::new();
    // A live bound of 540: the first range is set aside after the quiet time.
    let (setup, time) = own_setup(540);
    let mut rig = start(&dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    let desk = rig.node.owner().owner_id();
    rig.give(&mut book, desk, START);
    assert!(rig.node.scripts.contains_key(&own_receive(3)));
    rig.node.set_request_context(context(1), None);
    let made = rig.node.make_request(&deposit(100_000), 3, START).unwrap();
    assert_eq!(made.index, 3);
    time.store(START + 61 * DAY, Ordering::SeqCst);
    rig.tick();
    rig.mine_empty(1);
    // The quiet addresses are set aside, but the request's is not: the next
    // one takes its place among the 500.
    assert_eq!(rig.released().len(), 500);
    assert!(rig.released().contains(&500));
    assert!(!rig.released().contains(&3));
    assert!(rig.node.scripts.contains_key(&own_receive(3)));
    // A payment to it is still seen at once, years on.
    rig.mine(&[payment(3, 7, 60_000)]);
    assert_eq!(rig.node.view().receipts_at(3).len(), 1);
    assert_eq!(status(&rig, 1), Status::PartPaid { owed: 40_000 });
}

#[test]
fn rq_10_rq_15_a_request_is_credited_from_checked_blocks_only_at_the_depth() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 3);
    rig.node.make_request(&deposit(100_000), 0, START).unwrap();
    assert_eq!(status(&rig, 1), Status::Waiting);
    // Part of it arrives in a block: it waits for the depth.
    rig.mine(&[payment(0, 7, 40_000)]);
    assert_eq!(rig.node.request_arrivals().len(), 1);
    assert_eq!(status(&rig, 1), Status::Waiting);
    rig.mine_empty(1);
    assert_eq!(status(&rig, 1), Status::Waiting);
    // The third confirmation credits it.
    rig.mine_empty(1);
    assert_eq!(status(&rig, 1), Status::PartPaid { owed: 60_000 });
    assert_eq!(received(&rig, 1), 40_000);
    assert!(rig.node.request_arrivals().is_empty());
    // The rest arrives, and a block counts once however often it is seen.
    rig.mine(&[payment(0, 8, 60_000)]);
    rig.mine_empty(2);
    assert_eq!(status(&rig, 1), Status::Paid);
    let credits = rig
        .node
        .requests()
        .unwrap()
        .request(1)
        .unwrap()
        .credits
        .len();
    assert_eq!(credits, 2);
    rig.node.pump(Duration::ZERO);
    assert_eq!(
        rig.node
            .requests()
            .unwrap()
            .request(1)
            .unwrap()
            .credits
            .len(),
        2
    );
}

#[test]
fn rq_10_the_depth_is_at_least_one_confirmation() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 0);
    rig.node.make_request(&deposit(100_000), 0, START).unwrap();
    rig.mine(&[payment(0, 7, 100_000)]);
    assert_eq!(status(&rig, 1), Status::Paid);
}

#[test]
fn rq_15_nothing_is_credited_before_the_app_gave_the_shops_time_zone() {
    let dir = Dir::new();
    let (setup, _time) = own_setup(10_000);
    let mut rig = start(&dir, &setup).unwrap();
    let mut book = AllocationBook::new();
    let desk = rig.node.owner().owner_id();
    rig.give(&mut book, desk, START);
    rig.node.set_request_context(context(1), None);
    rig.node.make_request(&deposit(100_000), 0, START).unwrap();
    // The node is started again: the context is the app's to give.
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let mut setup = setup;
    setup.own = Some(book);
    let mut rig = start_at(&dir, &setup, chain).unwrap();
    rig.mine(&[payment(0, 7, 100_000)]);
    assert_eq!(status(&rig, 1), Status::Waiting);
    assert_eq!(rig.node.request_arrivals().len(), 1);
    rig.node.set_request_context(context(1), None);
    rig.tick();
    assert_eq!(status(&rig, 1), Status::Paid);
}

#[test]
fn rq_37_a_block_that_leaves_takes_the_credit_back_and_the_new_chain_credits_again() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 2);
    rig.node.make_request(&deposit(100_000), 0, START).unwrap();
    let pay = payment(0, 7, 100_000);
    let block = rig.mine(std::slice::from_ref(&pay));
    rig.mine_empty(1);
    assert_eq!(status(&rig, 1), Status::Paid);
    let height = u32::try_from(rig.chain.len()).unwrap() - 1;
    // The chain forks below the paying block and the new chain has none of
    // it: the credit is taken back, and stored.
    let other = mine(block.header.prev_blockhash, height, 9, &[]);
    let after = mine(other.block_hash(), height + 1, 9, &[]);
    rig.chain.truncate(rig.chain.len() - 2);
    rig.chain.push(other.clone());
    rig.chain.push(after.clone());
    rig.feed(vec![
        AppEvent::Chain {
            joined: vec![
                (height, other.block_hash()),
                (height + 1, after.block_hash()),
            ],
            left: vec![
                rig.node.watcher().chain().hash_at(height + 1).unwrap(),
                block.block_hash(),
            ],
        },
        AppEvent::Block {
            hash: other.block_hash(),
            bytes: serialize(&other),
        },
        AppEvent::Block {
            hash: after.block_hash(),
            bytes: serialize(&after),
        },
        AppEvent::Checked(height + 1),
    ]);
    assert_eq!(status(&rig, 1), Status::Waiting);
    assert_eq!(received(&rig, 1), 0);
    let on_disk = openbmt_requests::RequestBook::open(dir.path()).unwrap();
    assert!(on_disk.request(1).unwrap().credits.is_empty());
    // The payment is mined again in the new chain, and is credited at depth.
    let again = mine(
        after.block_hash(),
        height + 2,
        9,
        std::slice::from_ref(&pay),
    );
    rig.chain.push(again.clone());
    rig.feed(vec![
        AppEvent::Chain {
            joined: vec![(height + 2, again.block_hash())],
            left: vec![],
        },
        AppEvent::Block {
            hash: again.block_hash(),
            bytes: serialize(&again),
        },
        AppEvent::Checked(height + 2),
    ]);
    assert_eq!(status(&rig, 1), Status::Waiting);
    rig.mine_empty(1);
    assert_eq!(status(&rig, 1), Status::Paid);
    assert_eq!(received(&rig, 1), 100_000);
}

#[test]
fn rq_37_oc_53_what_left_while_the_node_was_stopped_is_taken_back_at_start() {
    let dir = Dir::new();
    let (mut rig, book, setup) = desk_rig(&dir, 1);
    rig.node.make_request(&deposit(100_000), 0, START).unwrap();
    rig.mine(&[payment(0, 7, 100_000)]);
    rig.mine_empty(2);
    assert_eq!(status(&rig, 1), Status::Paid);
    let old = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    // The chain the driver saved has none of it.
    let mut new: Vec<Block> = Vec::new();
    for h in 1..=8u32 {
        let prev = new
            .last()
            .map_or_else(crate::tests::genesis, Block::block_hash);
        new.push(mine(prev, h, 7, &[]));
    }
    assert_ne!(old[0].block_hash(), new[0].block_hash());
    save_headers(&dir, &new);
    let mut setup = setup;
    setup.own = Some(book);
    let mut rig = start_at(&dir, &setup, new).unwrap();
    rig.node.set_request_context(context(1), None);
    assert_eq!(status(&rig, 1), Status::Waiting);
    assert_eq!(received(&rig, 1), 0);
    let on_disk = openbmt_requests::RequestBook::open(dir.path()).unwrap();
    assert!(on_disk.request(1).unwrap().credits.is_empty());
}

#[test]
fn rq_26_rq_29_an_accepted_return_is_queued_on_the_owners_queue_and_never_paid_at_once() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 1);
    rig.node.make_request(&deposit(100_000), 0, START).unwrap();
    rig.mine(&[payment(0, 7, 130_000)]);
    rig.mine_empty(1);
    assert_eq!(status(&rig, 1), Status::Overpaid { excess: 30_000 });
    let address = external_address(9);
    // An address of the shop's own wallet is refused.
    let own = bitcoin::Address::from_script(&own_receive(5), bitcoin::Network::Bitcoin).unwrap();
    let refused = rig
        .node
        .add_return(1, ReturnAmount::Sats(30_000), &own.to_string(), START)
        .unwrap_err();
    assert!(matches!(refused, RequestError::Address(_)));
    // More than it received is refused.
    let too_much = rig
        .node
        .add_return(1, ReturnAmount::Sats(130_001), &address, START)
        .unwrap_err();
    assert!(matches!(too_much, RequestError::Return(_)));
    let added = rig
        .node
        .add_return(1, ReturnAmount::Sats(30_000), &address, START + 5)
        .unwrap();
    assert_eq!(added.sats, 30_000);
    assert!(added.groups.contains(' '));
    // One refund on the queue, queued and not paid, for the desk's sale.
    let refunds = rig.node.owner().refunds();
    assert_eq!(refunds.len(), 1);
    assert_eq!(refunds[0].id, added.refund);
    assert_eq!(refunds[0].cashier, return_label(1, added.id));
    assert_eq!(refunds[0].sats, 30_000);
    assert_eq!(refunds[0].script, external(9));
    assert!(matches!(refunds[0].stage, Stage::Queued));
    assert_eq!(refunds[0].sale.counter, rig.node.owner().owner_id());
    assert_eq!(refunds[0].sale.index, 0);
    let book = rig.node.requests().unwrap();
    assert_eq!(book.returns().len(), 1);
    assert_eq!(book.returns()[0].state, ReturnState::Queued);
    assert_eq!(book.returnable_sats(1), Some(100_000));
    // A coin spend is not made by the call.
    assert_eq!(rig.node.send_states().len(), 0);
    // The facts of the refund are the book's, and survive a reorganization's
    // refresh of the facts.
    let facts = rig.node.owner().sale_facts(&refunds_sale(&rig)).unwrap();
    assert_eq!(facts.received_sats, 130_000);
    assert!(facts.reached_refund_depth);
}

fn refunds_sale(rig: &Rig) -> openbmt_owner::SaleKey {
    rig.node.owner().refunds()[0].sale
}

#[test]
fn rq_29_the_cap_counts_the_returns_queued_or_in_a_spend() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 1);
    rig.node.make_request(&deposit(10_000), 0, START).unwrap();
    rig.mine(&[payment(0, 7, 5_000_000)]);
    rig.mine_empty(1);
    for n in 0..99u8 {
        let address = external_address(n.max(1));
        rig.node
            .add_return(1, ReturnAmount::Sats(1_000), &address, START)
            .unwrap();
    }
    let full = rig
        .node
        .add_return(1, ReturnAmount::Sats(1_000), &external_address(200), START)
        .unwrap_err();
    assert!(matches!(
        full,
        RequestError::Return(openbmt_requests::ReturnRefused::QueueFull)
    ));
    // The refusal changed neither the book nor the queue.
    assert_eq!(rig.node.requests().unwrap().returns().len(), 99);
    assert_eq!(rig.node.owner().refunds().len(), 99);
}

#[test]
fn rq_29_a_return_follows_its_refund_to_sent_back_to_the_queue_and_done() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 1);
    rig.node.make_request(&deposit(100_000), 0, START).unwrap();
    rig.mine(&[payment(0, 7, 130_000)]);
    rig.mine_empty(1);
    let added = rig
        .node
        .add_return(1, ReturnAmount::Sats(30_000), &external_address(9), START)
        .unwrap();
    let state = |rig: &Rig| rig.node.requests().unwrap().returns()[0].state;
    // The refund's transaction is signed and sent.
    let txid = Txid::from_byte_array([0x42; 32]);
    let label = return_label(1, added.id);
    let at = |stage: Stage| BTreeMap::from([(label.clone(), stage)]);
    rig.node.follow_returns_from(&at(Stage::Sent { txid }));
    assert_eq!(
        state(&rig),
        ReturnState::Sent {
            txid: txid.to_byte_array()
        }
    );
    // The spend is replaced without it: back to the queue.
    rig.node.follow_returns_from(&at(Stage::Queued));
    assert_eq!(state(&rig), ReturnState::Queued);
    // Sent again, then paid.
    rig.node.follow_returns_from(&at(Stage::Sent { txid }));
    rig.node.follow_returns_from(&at(Stage::Paid { txid }));
    assert!(matches!(state(&rig), ReturnState::Done { .. }));
    // It is stored.
    let on_disk = openbmt_requests::RequestBook::open(dir.path()).unwrap();
    assert!(matches!(
        on_disk.returns()[0].state,
        ReturnState::Done { .. }
    ));
}

#[test]
fn rq_21_rq_22_the_repeats_are_made_on_the_calls_the_app_makes_with_the_time_passed_in() {
    let dir = Dir::new();
    let (mut rig, _book, _setup) = desk_rig(&dir, 1);
    let mut weekly = deposit(100_000);
    weekly.repeat = Some(Every::Week);
    weekly.due = due_in(7);
    rig.node.make_request(&weekly, 0, START).unwrap();
    // Nothing is due yet, and nothing runs by itself.
    assert!(rig.node.repeats_due(due_in(0)).unwrap().is_empty());
    let later = due_in(30);
    let due = rig.node.repeats_due(later).unwrap();
    assert!(!due.is_empty() && due.len() <= 12);
    let indexes: Vec<u32> = (1..=u32::try_from(due.len()).unwrap()).collect();
    let made = rig
        .node
        .make_repeats(&due, &indexes, START + 30 * DAY)
        .unwrap();
    assert_eq!(made.len(), due.len());
    assert_eq!(made[0].number, 2);
    assert!(rig.node.request_indexes().contains(&indexes[0]));
    // The book was stored once with all of them.
    let on_disk = openbmt_requests::RequestBook::open(dir.path()).unwrap();
    assert_eq!(on_disk.requests().len(), 1 + due.len());
}

#[test]
fn rq_02_the_desk_takes_the_next_unused_index_and_a_new_range_when_the_last_is_used() {
    let dir = Dir::new();
    let mut node = OwnNode::open(
        dir.path(),
        id(0x11),
        openbmt_core::pinned_wallet::PinnedWallet::new(own::single()),
    )
    .unwrap();
    let mut used: BTreeSet<u32> = BTreeSet::new();
    // The first call takes a range of the desk: index 0.
    let first = node
        .desk_index(&|i| used.contains(&i), START, &mut NotWatching)
        .unwrap();
    assert_eq!(first, 0);
    assert_eq!(
        node.shop().book().ranges_of(&node.shop().owner_id()).len(),
        1
    );
    // An index in use is not given twice; the next unused is.
    used.insert(0);
    used.insert(1);
    assert_eq!(
        node.desk_index(&|i| used.contains(&i), START, &mut NotWatching)
            .unwrap(),
        2
    );
    // A counter's range between does not change the desk's.
    let (shop, store) = node.shop_and_store();
    shop.allocate(counter(), START, store).unwrap();
    // When the range is used up a new one is taken, after the counter's.
    used.extend(0..RANGE_LEN);
    let next = node
        .desk_index(&|i| used.contains(&i), START, &mut NotWatching)
        .unwrap();
    assert_eq!(next, 2 * RANGE_LEN);
    // It is in the file before it is given.
    let again = OwnNode::open(
        dir.path(),
        id(0x11),
        openbmt_core::pinned_wallet::PinnedWallet::new(own::single()),
    )
    .unwrap();
    assert_eq!(
        again
            .shop()
            .book()
            .ranges_of(&again.shop().owner_id())
            .len(),
        2
    );
}

#[test]
fn rq_30_the_arrivals_file_reads_strictly_and_encodes_to_the_same_bytes() {
    let a = |height: u32, tx: u8| PendingArrival {
        index: 4,
        txid: [tx; 32],
        vout: 1,
        height,
        block: [height as u8; 32],
        block_time: 1_700_000_000,
        sats: 5_000,
    };
    let list = vec![a(5, 1), a(5, 2), a(6, 3)];
    let bytes = crate::requests::encode_arrivals(&list);
    assert_eq!(&bytes[..5], b"OBM\\\x01");
    assert_eq!(crate::requests::decode_arrivals(&bytes).unwrap(), list);
    // Cut short, trailing bytes, a wrong magic, a wrong order, a repeat and
    // zero satoshis are refused.
    assert!(crate::requests::decode_arrivals(&bytes[..bytes.len() - 1]).is_err());
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(crate::requests::decode_arrivals(&longer).is_err());
    let mut other = bytes.clone();
    other[3] = b'=';
    assert!(crate::requests::decode_arrivals(&other).is_err());
    let backwards = crate::requests::encode_arrivals(&[a(6, 1), a(5, 1)]);
    assert!(crate::requests::decode_arrivals(&backwards).is_err());
    let twice = crate::requests::encode_arrivals(&[a(5, 1), a(6, 1), {
        let mut again = a(7, 1);
        again.txid = [1; 32];
        again
    }]);
    assert!(crate::requests::decode_arrivals(&twice).is_err());
    let mut nothing = a(5, 1);
    nothing.sats = 0;
    assert!(
        crate::requests::decode_arrivals(&crate::requests::encode_arrivals(&[nothing])).is_err()
    );
    // Any bytes at all are answered, never a panic.
    for n in 0..bytes.len() {
        let _ = crate::requests::decode_arrivals(&bytes[..n]);
    }
}
