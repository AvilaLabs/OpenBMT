//! What the owner paid out (`docs/OWNER-COINS-RULES.md` OC-48 to OC-51), on a
//! driver that the test feeds by hand.

use std::sync::atomic::Ordering;

use openbmt_owner::{CashAmount, RateNote, RefundBooks};
use openbmt_session::ShopValue;

use super::*;
use crate::paid_log::{PaidEntry, PaidValue, PaidWhat, PAID_LOG_FILE};

const USD: [u8; 3] = *b"USD";

/// A node whose clock the test holds.
fn start_timed(dir: &Dir, at: u64) -> (Rig, Arc<AtomicU64>) {
    let time = Arc::new(AtomicU64::new(at));
    let setup = Setup {
        time: Some(time.clone()),
        ..Setup::default()
    };
    (start(dir, &setup).unwrap(), time)
}

/// Queue a batch, a sale and a refund of `sats` to `external(9)` that came
/// from request 7 of the counter, with these facts of its value.
fn queue_with_books(
    rig: &mut Rig,
    sats: u64,
    shop_value: Option<ShopValue>,
    last_quote: Option<RateNote>,
) -> u32 {
    let write = rig.node.queue_writer();
    let owner = rig.node.owner_mut().unwrap();
    owner.queue_batch(batch(0, 3), 1_000, &write).unwrap();
    rig.node.refresh_marks();
    let write = rig.node.queue_writer();
    let owner = rig.node.owner_mut().unwrap();
    owner
        .set_sale_facts(
            sale(1),
            SaleFacts {
                received_sats: 50_000,
                reached_refund_depth: true,
            },
            &write,
        )
        .unwrap();
    let write = rig.node.queue_writer();
    rig.node
        .owner_mut()
        .unwrap()
        .queue_refund_with_books(
            sale(1),
            "Mia",
            external(9),
            sats,
            shop_value,
            Some(RefundBooks {
                request: 7,
                last_quote,
            }),
            &write,
        )
        .unwrap()
}

fn quote(minor_per_btc: u64) -> Option<RateNote> {
    Some(RateNote {
        currency: USD,
        minor_per_btc,
    })
}

#[test]
fn oc_48_a_sent_refund_is_logged_once_with_its_time_and_what_went_out() {
    let dir = Dir::new();
    let (mut rig, time) = start_timed(&dir, 4_000);
    let refund = queue_with_books(&mut rig, 10_000, None, None);
    rig.fund(300_000);
    time.store(5_000, Ordering::SeqCst);
    let txid = rig.spend();
    let tx = rig.tx(&txid);
    let out = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == external(9))
        .unwrap();
    let log = rig.node.paid_log().to_vec();
    assert_eq!(log.len(), 1);
    assert_eq!(
        log[0],
        PaidEntry {
            what: PaidWhat::Refund {
                sale: sale(1),
                request: Some(7),
            },
            number: refund,
            txid,
            sats: out.value.to_sat(),
            at: 5_000,
            value: None,
        }
    );
    // Sending it again, as the next start does, writes nothing more.
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(rig.node.paid_log(), log.as_slice());
}

#[test]
fn oc_48_a_log_that_cannot_be_stored_leaves_the_entry_signed_and_the_next_start_logs_it() {
    let dir = Dir::new();
    let (mut rig, _) = start_timed(&dir, 4_000);
    let refund = queue_with_books(&mut rig, 10_000, None, None);
    rig.fund(300_000);
    // A folder where the file goes: the store fails.
    fs::create_dir(dir.path().join(PAID_LOG_FILE)).unwrap();
    let txid = rig.spend();
    assert!(rig.node.paid_log().is_empty());
    assert!(matches!(
        rig.node.owner().refunds().iter().find(|r| r.id == refund).unwrap().stage,
        Stage::Signed { txid: t } if t == txid
    ));
    let notes = rig.node.watcher_mut().take_notes();
    assert!(
        notes.iter().any(|n| n.contains("could not be logged")),
        "{notes:?}"
    );
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    fs::remove_dir(dir.path().join(PAID_LOG_FILE)).unwrap();
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(rig.node.paid_log().len(), 1);
    assert!(matches!(
        rig.node.owner().refunds().iter().find(|r| r.id == refund).unwrap().stage,
        Stage::Sent { txid: t } if t == txid
    ));
}

#[test]
fn oc_49_a_refund_is_valued_as_stated_else_at_the_last_quote_else_not_at_all() {
    // Stated.
    let dir = Dir::new();
    let (mut rig, _) = start_timed(&dir, 4_000);
    let stated = ShopValue {
        minor: 912,
        currency: *b"EUR",
    };
    queue_with_books(&mut rig, 10_000, Some(stated), quote(9_000_000));
    rig.fund(300_000);
    rig.spend();
    assert_eq!(
        rig.node.paid_log()[0].value,
        Some(PaidValue {
            currency: *b"EUR",
            minor: 912
        })
    );

    // At the last quote: what went out at 9,000,000 cents a bitcoin, rounded
    // down.
    let dir = Dir::new();
    let (mut rig, _) = start_timed(&dir, 4_000);
    queue_with_books(&mut rig, 10_001, None, quote(9_000_000));
    rig.fund(300_000);
    rig.spend();
    eprintln!("DBG {:?}", rig.node.owner().cash_outs());
    let entry = rig.node.paid_log()[0];
    assert_eq!(
        entry.value,
        Some(PaidValue {
            currency: USD,
            minor: entry.sats * 9_000_000 / 100_000_000
        })
    );

    // Nothing to value it by.
    let dir = Dir::new();
    let (mut rig, _) = start_timed(&dir, 4_000);
    queue_with_books(&mut rig, 10_000, None, None);
    rig.fund(300_000);
    rig.spend();
    assert_eq!(rig.node.paid_log()[0].value, None);
}

#[test]
fn oc_49_a_cash_out_is_valued_at_the_price_the_owner_confirmed() {
    let dir = Dir::new();
    let (mut rig, _) = start_timed(&dir, 4_000);
    queue_with_books(&mut rig, 10_000, None, None);
    let write = rig.node.queue_writer();
    let cash = rig
        .node
        .owner_mut()
        .unwrap()
        .queue_cash_out_priced(
            external(8),
            CashAmount::Sats(30_000),
            "Bank",
            quote(9_000_000),
            &write,
        )
        .unwrap();
    rig.fund(300_000);
    rig.spend();
    let entry = rig
        .node
        .paid_log()
        .iter()
        .find(|e| e.what == PaidWhat::CashOut)
        .copied()
        .unwrap();
    assert_eq!(entry.number, cash);
    assert_eq!(entry.sats, 30_000);
    assert_eq!(
        entry.value,
        Some(PaidValue {
            currency: USD,
            minor: 2_700
        })
    );
    // The refund of the same session is in the log too, apart from it.
    assert_eq!(rig.node.paid_log().len(), 2);
}

#[test]
fn oc_50_archive_paid_keeps_the_log_and_a_restart_reads_it_back() {
    let dir = Dir::new();
    let (mut rig, _) = start_timed(&dir, 4_000);
    let refund = queue_with_books(&mut rig, 10_000, None, quote(9_000_000));
    rig.fund(300_000);
    let txid = rig.spend();
    let tx = rig.tx(&txid);
    rig.mine(std::slice::from_ref(&tx));
    rig.mine_empty(100);
    assert_eq!(rig.node.archive_paid().unwrap(), 1);
    assert!(rig.node.owner().refunds().iter().all(|r| r.id != refund));
    let log = rig.node.paid_log().to_vec();
    assert_eq!(log.len(), 1);
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    assert_eq!(rig.node.paid_log(), log.as_slice());
}

#[test]
fn oc_50_a_log_that_does_not_read_back_stops_the_node_with_a_file_fault() {
    let dir = Dir::new();
    let (mut rig, _) = start_timed(&dir, 4_000);
    queue_with_books(&mut rig, 10_000, None, None);
    rig.fund(300_000);
    rig.spend();
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let path = dir.path().join(PAID_LOG_FILE);
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let err = start_at(&dir, &Setup::default(), chain).err().unwrap();
    assert!(
        matches!(&err, NodeError::Files(m) if m.contains(PAID_LOG_FILE)),
        "{err}"
    );
}

#[test]
fn oc_51_the_queue_keeps_the_price_and_the_books_notes_through_a_restart() {
    let dir = Dir::new();
    let (mut rig, _) = start_timed(&dir, 4_000);
    let refund = queue_with_books(&mut rig, 10_000, None, quote(9_100_000));
    let write = rig.node.queue_writer();
    let cash = rig
        .node
        .owner_mut()
        .unwrap()
        .queue_cash_out_priced(
            external(8),
            CashAmount::All,
            "Bank",
            quote(9_200_000),
            &write,
        )
        .unwrap();
    let chain = rig.chain.clone();
    rig.node.shutdown();
    drop(rig);
    let rig = start_at(&dir, &Setup::default(), chain).unwrap();
    let r = rig
        .node
        .owner()
        .refunds()
        .iter()
        .find(|r| r.id == refund)
        .unwrap();
    assert_eq!(
        r.books,
        Some(RefundBooks {
            request: 7,
            last_quote: quote(9_100_000)
        })
    );
    let c = rig
        .node
        .owner()
        .cash_outs()
        .iter()
        .find(|c| c.id == cash)
        .unwrap();
    assert_eq!(c.price, quote(9_200_000));
}
