//! Books built from the chain of a real counter run: the counter judges
//! sales, takes sightings and blocks, and writes its own records; nothing
//! here is a hand-made event. The counter's test kit is private to its
//! crate, so a small driver is repeated here.

use std::collections::BTreeMap;

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount as Sats, Block, BlockHash, CompactTarget, OutPoint, ScriptBuf, Sequence, Transaction,
    TxIn, TxMerkleNode, TxOut, Txid, WScriptHash, Witness,
};
use openbmt_core::batch::{AddressBatch, BatchPolicy, PinnedRoots, SignedBatch};
use openbmt_core::identity::Identity;
use openbmt_core::policy::Heartbeat;
use openbmt_core::records::{Chain, DeviceList};
use openbmt_core::settings::Settings;
use openbmt_counter::{
    Amount, Counter, Currency, Facts, OpenFacts, Opened, Pairing, Rate, RateSource, Slot, State,
    Store,
};
use openbmt_watch::{HeaderChain, Health, PeerAddr, Source, WellFormedBlock};

use super::fixture::*;
use crate::{Books, Counting, PaymentList, RevealsPayments, UnknownField, Value};

const START: u64 = 1_738_400_000;

fn id(n: u8) -> Identity {
    identity(n)
}

/// A store that accepts everything: the counter keeps its own records.
struct Null;

impl Store for Null {
    type Error = &'static str;
    fn put(&mut self, _slot: Slot, _bytes: &[u8]) -> Result<(), &'static str> {
        Ok(())
    }
    fn delete(&mut self, _slot: Slot) -> Result<(), &'static str> {
        Ok(())
    }
    fn append_record(&mut self, _bytes: &[u8]) -> Result<(), &'static str> {
        Ok(())
    }
    fn read_records(&self, _from: u64, _max: usize) -> Result<Vec<Vec<u8>>, &'static str> {
        Ok(Vec::new())
    }
}

struct Headers {
    heights: BTreeMap<BlockHash, u32>,
    tip: u32,
}

impl HeaderChain for Headers {
    fn height_of(&self, block: &BlockHash) -> Option<u32> {
        self.heights.get(block).copied()
    }
    fn tip_height(&self) -> u32 {
        self.tip
    }
    fn tip_time(&self) -> u64 {
        START
    }
}

fn script(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32]))
}

fn batch(count: u32) -> SignedBatch {
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: id(3).device_id(),
        start: 0,
        count,
        scripts: (0..count).map(|i| script(i as u8 + 1)).collect(),
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}

fn pay_tx(coin: u8, to: &ScriptBuf, sats: u64) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array([coin; 32]),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(sats),
            script_pubkey: to.clone(),
        }],
    }
}

fn header(root: TxMerkleNode) -> Header {
    let mut h = Header {
        version: BlockVersion::ONE,
        prev_blockhash: BlockHash::all_zeros(),
        merkle_root: root,
        time: 1_000,
        bits: CompactTarget::from_consensus(0x207f_ffff),
        nonce: 0,
    };
    while h.validate_pow(h.target()).is_err() {
        h.nonce += 1;
    }
    h
}

fn mined(tag: u8, txs: &[Transaction]) -> Block {
    let coinbase = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(vec![0x04, 1, 2, 3, tag]),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(5_000_000_000),
            script_pubkey: script(0xee),
        }],
    };
    let mut all = vec![coinbase];
    all.extend(txs.iter().cloned());
    let mut block = Block {
        header: header(TxMerkleNode::all_zeros()),
        txdata: all,
    };
    let root = block.compute_merkle_root().expect("a root");
    block.header = header(root);
    block
}

/// One counter and what it needs to run.
struct Run {
    c: Counter,
    chain: Headers,
    now: u64,
    next_coin: u8,
    next_block: u8,
}

impl Run {
    fn new() -> Self {
        let mut settings = Settings::defaults(1_000);
        settings.customer_wifi = false;
        // No floor, so small sales open (CR-75).
        settings.min_sale_sats = 0;
        settings.devices.insert(id(3).device_id());
        settings.counters.insert(id(3).device_id(), 10_000);
        let pairing = Pairing {
            roots: PinnedRoots::new(id(1).device_id(), id(2).device_id()).expect("roots"),
            wallet_checksum: [9; 32],
            batch_policy: BatchPolicy { min_count: 1 },
        };
        let mut c = Counter::new(id(3), pairing, settings);
        c.add_batch(batch(20), &mut Null).expect("a batch");
        Self {
            c,
            chain: Headers {
                heights: BTreeMap::new(),
                tip: 100,
            },
            now: START,
            next_coin: 1,
            next_block: 1,
        }
    }

    fn tick(&mut self, secs: u64) {
        self.now += secs;
    }

    fn healthy() -> Health {
        Health {
            healthy: true,
            problem: None,
            behind: false,
        }
    }

    fn open(&mut self, cents: u64, label: &str) -> Opened {
        let facts = OpenFacts {
            health: Self::healthy(),
            rescan_pending: false,
            now: self.now,
        };
        let source = RateSource::new(label, self.now - 2).expect("a source");
        self.c
            .open(
                Amount(cents),
                Currency::new(*b"USD").expect("a currency"),
                Rate::new(10_000_000).expect("a rate"),
                &source,
                facts,
                &mut Null,
            )
            .expect("opened")
    }

    fn requote(&mut self, o: &Opened, minor_per_btc: u64, label: &str) {
        let source = RateSource::new(label, self.now - 1).expect("a source");
        self.c
            .requote(
                o.index,
                Rate::new(minor_per_btc).expect("a rate"),
                &source,
                self.now,
                &self.chain,
                &mut Null,
            )
            .expect("requoted");
    }

    fn pay(&mut self, o: &Opened, sats: u64) -> Transaction {
        self.next_coin += 1;
        pay_tx(self.next_coin, &o.script, sats)
    }

    /// Three network groups report the transaction.
    fn see(&mut self, tx: &Transaction) {
        for n in 20..23u8 {
            let peer = Source::Peer(PeerAddr::Ip(format!("{n}.{n}.1.1").parse().expect("an ip")));
            self.c
                .sighting(&serialize(tx), peer, self.now, &mut Null)
                .expect("a sighting");
        }
    }

    /// A block of these transactions at `height`, given to the counter.
    fn confirm(&mut self, txs: &[Transaction], height: u32) {
        let block = mined(self.next_block, txs);
        self.next_block += 1;
        self.chain.heights.insert(block.block_hash(), height);
        let wf = WellFormedBlock::parse(&serialize(&block)).expect("a block");
        self.c
            .block(&wf, &self.chain, self.now, &mut Null)
            .expect("a block");
    }

    fn judge(&mut self, o: &Opened) -> State {
        let facts = Facts {
            chain: &self.chain,
            health: Self::healthy(),
            heartbeat: Heartbeat {
                verified: true,
                age_secs: 0,
            },
            remote_order: false,
            now: self.now,
        };
        self.c
            .judge(o.index, &facts, &mut Null)
            .expect("judged")
            .state
    }
}

struct Scene {
    books: Books,
    counter: openbmt_core::identity::DeviceId,
    t_paid: Transaction,
    t_part: Transaction,
    t_rest: Transaction,
    t_final: Transaction,
    t_late_after_final: Transaction,
    t_late: Transaction,
    times: Times,
}

struct Times {
    seen_s0: u64,
    seen_s1: u64,
    block_s4: u64,
}

/// Five sales, run through the counter.
fn scene() -> Scene {
    let mut r = Run::new();

    // Sale 0: seen by three groups, then deep in a block: final.
    let s0 = r.open(500, "feed-one");
    let t_paid = r.pay(&s0, 5_000);
    r.tick(10);
    let seen_s0 = r.now;
    r.see(&t_paid);
    r.tick(30);
    r.confirm(std::slice::from_ref(&t_paid), 95);
    assert_eq!(r.judge(&s0), State::Final);

    // Sale 1: a part, a new quote at double the price, the rest; final.
    let s1 = r.open(1_000, "feed-one");
    let t_part = r.pay(&s1, 4_000);
    r.tick(5);
    let seen_s1 = r.now;
    r.see(&t_part);
    r.judge(&s1);
    r.tick(301);
    r.requote(&s1, 20_000_000, "feed-two");
    let t_rest = r.pay(&s1, 3_000);
    r.see(&t_rest);
    r.judge(&s1);
    r.tick(20);
    r.confirm(&[t_part.clone(), t_rest.clone()], 94);
    assert_eq!(r.judge(&s1), State::Final);

    // Sale 2: confirmed at depth 1, not final.
    let s2 = r.open(500, "feed-one");
    let t_conf = r.pay(&s2, 5_000);
    r.see(&t_conf);
    r.confirm(std::slice::from_ref(&t_conf), 100);
    assert_eq!(r.judge(&s2), State::Confirmed);

    // Sale 3: expires, and then a payment arrives.
    let s3 = r.open(500, "feed-one");
    r.tick(901);
    assert_eq!(r.judge(&s3), State::Expired);
    let t_late = r.pay(&s3, 700);
    r.tick(60);
    r.c.sighting(&serialize(&t_late), Source::Box, r.now, &mut Null)
        .expect("a late sighting");

    // Sale 4: never seen before its block (so the time is the time the
    // counter learned of the block), final, then a payment after final.
    let s4 = r.open(500, "feed-one");
    let t_final = r.pay(&s4, 5_000);
    r.tick(25);
    let block_s4 = r.now;
    r.confirm(std::slice::from_ref(&t_final), 93);
    r.tick(50);
    assert_eq!(r.judge(&s4), State::Final);
    let t_late_after_final = r.pay(&s4, 900);
    r.tick(10);
    r.c.sighting(
        &serialize(&t_late_after_final),
        Source::Box,
        r.now,
        &mut Null,
    )
    .expect("a late sighting");

    let counter = id(3).device_id();
    let devices = DeviceList::new([counter, id(1).device_id()]);
    // The counter's records are all in its memory: the store keeps none.
    let chain = Chain::from_records(r.c.records_from(0, usize::MAX, &Null).expect("records"));
    let books = Books::from_chains(&devices, &[(counter, &chain)]).expect("books");
    Scene {
        books,
        counter,
        t_paid,
        t_part,
        t_rest,
        t_final,
        t_late_after_final,
        t_late,
        times: Times {
            seen_s0,
            seen_s1,
            block_s4,
        },
    }
}

fn txid_bytes(t: &Transaction) -> [u8; 32] {
    t.compute_txid().to_byte_array()
}

fn sale(s: &Scene, index: u32) -> &crate::Sale {
    s.books
        .sale(&crate::SaleKey {
            counter: s.counter,
            index,
        })
        .expect("a sale")
}

#[test]
fn bk_06_a_real_run_gives_every_formerly_unknown_field_for_a_final_sale() {
    let s = scene();
    let a = sale(&s, 0);
    assert_eq!(a.state, crate::SaleState::Final);
    assert_eq!(a.counting(), Counting::Counted);
    let first = a.rate_source.known().expect("a rate source");
    assert_eq!(first.label(), "feed-one");
    assert_eq!(first.fetched_at(), START - 2);
    assert_eq!(a.last_rate_source, a.rate_source);
    assert_eq!((a.last_quote_sats, a.first_quote_sats), (5_000, 5_000));
    assert_eq!(a.sats_received, Value::Known(5_000));
    assert_eq!(a.first_seen_at, Some(s.times.seen_s0));
    // $5.00 at 1,000 sats per dollar, 5,000 sats: 500 cents.
    assert_eq!(a.value_at_first_seen_minor, Value::Known(500));
    assert_eq!(
        a.payments,
        Value::Known(PaymentList {
            payments: vec![crate::Payment {
                txid: txid_bytes(&s.t_paid),
                sats: 5_000
            }],
            more: false
        })
    );
}

#[test]
fn bk_06_a_real_run_with_a_requote_has_both_sources_and_the_value_at_each_quote() {
    let s = scene();
    let b = sale(&s, 1);
    assert_eq!(b.rate_source.known().expect("first").label(), "feed-one");
    let last = b.last_rate_source.known().expect("last");
    assert_eq!(last.label(), "feed-two");
    assert_eq!(b.first_quote_sats, 10_000);
    assert_eq!(b.quote_rate_minor_per_btc, 10_000_000);
    // The whole $10 at the doubled price needs 5,000 sats.
    assert_eq!(b.last_quote_sats, 5_000);
    assert_eq!(b.last_quote_rate_minor_per_btc, 20_000_000);
    // 4,000 sats at the first quote (400 cents) and 3,000 at the second
    // (600 cents).
    assert_eq!(b.sats_received, Value::Known(7_000));
    assert_eq!(b.value_at_first_seen_minor, Value::Known(1_000));
    assert_eq!(b.first_seen_at, Some(s.times.seen_s1));
    let Value::Known(list) = &b.payments else {
        panic!("not known");
    };
    let mut want = vec![
        (txid_bytes(&s.t_part), 4_000),
        (txid_bytes(&s.t_rest), 3_000),
    ];
    want.sort();
    let got: Vec<([u8; 32], u64)> = list.payments.iter().map(|p| (p.txid, p.sats)).collect();
    assert_eq!(got, want);
    // Received 7,000 sats against the last quote's 5,000: no shortfall.
    assert_eq!(b.settlement(), None);
}

#[test]
fn bk_06_a_real_run_leaves_unknown_only_what_the_records_still_do_not_carry() {
    let s = scene();
    // Sale 2 is confirmed and not final.
    let c = sale(&s, 2);
    assert_eq!(c.state, crate::SaleState::Confirmed);
    assert_eq!(c.counting(), Counting::Pending);
    assert_eq!(c.payments, Value::Unknown(UnknownField::Txid));
    assert_eq!(
        c.value_at_first_seen_minor,
        Value::Unknown(UnknownField::ValueAtFirstSeen)
    );
    assert_eq!(c.first_seen_at, None);
    assert_eq!(c.sats_received, Value::Known(5_000));
    assert!(c.rate_source.known().is_some());
    // Every sale: the tax and the address stay unknown.
    for index in 0..5 {
        let x = sale(&s, index);
        assert_eq!(x.sales_tax_minor, Value::Unknown(UnknownField::SalesTax));
        assert_eq!(x.address, Value::Unknown(UnknownField::Address));
    }
    // Each unknown the ledger shows says why and what comes next.
    let ledger = s
        .books
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    assert!(ledger.contains(
        "txid: the sale is not final; the final record names its transactions. \
         Next: wait until the sale is final: the final record carries them"
    ));
    assert!(ledger.contains("value at first seen: the sale is not final"));
    assert!(ledger.contains("address: the records carry the sale index, not the address"));
    assert!(ledger.contains("sales tax: the counter does not know the order's tax"));
    assert!(!ledger.contains("rate source:"));
}

#[test]
fn bk_09_a_real_run_settles_a_late_payment_by_the_final_record() {
    let s = scene();
    // Sale 4 is final, then a payment of 900 sats arrives that the final
    // record does not name: it is added.
    let e = sale(&s, 4);
    assert_eq!(e.state, crate::SaleState::Final);
    assert_eq!(e.late_payments.len(), 1);
    assert_eq!(e.late_payments[0].txid, txid_bytes(&s.t_late_after_final));
    assert_eq!(e.sats_received, Value::Known(5_900));
    // Its payment was only ever seen in a block: the time is the time the
    // counter learned of the block.
    assert_eq!(e.first_seen_at, Some(s.times.block_s4));
    let Value::Known(list) = &e.payments else {
        panic!("not known");
    };
    assert_eq!(list.payments.len(), 1);
    assert_eq!(list.payments[0].txid, txid_bytes(&s.t_final));
    // A sale that only ever got a late payment has its sum and no final.
    let d = sale(&s, 3);
    assert_eq!(d.sats_received, Value::Known(700));
    assert_eq!(d.late_payments[0].txid, txid_bytes(&s.t_late));
    assert_eq!(d.payments, Value::Unknown(UnknownField::Txid));
}

#[test]
fn bk_19_a_real_run_puts_the_new_columns_in_the_full_ledger_only() {
    let s = scene();
    let ledger = s
        .books
        .full_ledger(RevealsPayments::Acknowledged)
        .expect("ledger");
    let hex =
        |t: &Transaction| -> String { txid_bytes(t).iter().map(|b| format!("{b:02x}")).collect() };
    for t in [&s.t_paid, &s.t_part, &s.t_rest, &s.t_final, &s.t_late] {
        assert!(ledger.contains(&hex(t)), "the ledger lacks a txid");
    }
    assert!(ledger.contains("feed-one") && ledger.contains("feed-two"));
    // The accounting export holds none of them and has three columns.
    let files = s.books.accounting_export().expect("export");
    let text: String = files.iter().map(|(_, f)| f.as_str()).collect();
    assert!(text.starts_with("Date,Description,Amount\r\n"));
    for t in [&s.t_paid, &s.t_part, &s.t_rest, &s.t_final, &s.t_late] {
        assert!(!text.contains(&hex(t)));
    }
    assert!(!text.contains("feed-"));
    for row in text.split("\r\n").filter(|r| !r.is_empty()) {
        assert_eq!(row.matches(',').count(), 2, "{row}");
    }
}

#[test]
fn bk_28_a_real_run_goes_through_the_bundle_unchanged() {
    let s = scene();
    assert_eq!(s.books.sales().len(), 5);
    let bytes = s.books.to_bundle();
    let back = Books::from_bundle(&bytes).expect("reads back");
    assert_eq!(back, s.books);
    assert_eq!(back.to_bundle(), bytes);
}
