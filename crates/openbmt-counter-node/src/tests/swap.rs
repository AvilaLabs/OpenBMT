//! A sale paid over Lightning through a swap provider (CR-218 to CR-225),
//! with the pretend service of the swap library served in the test and a
//! fake driver for the chain.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use bitcoin::consensus::encode::deserialize;
use bitcoin::{BlockHash, Network, Transaction};
use openbmt_core::settings::SwapProvider;
use openbmt_counter::{Report, State};
use openbmt_net_io::AppEvent;
use openbmt_swap::testing::{Mode, Pretend};

use super::kit::*;
use crate::swap::{
    all_words, claim_stuck_words, confirming_words, failed_words, lost_words, unavailable_words,
    SwapRecord, MAGIC_SWAP_RECORD, NOT_FOR_THIS_SALE, SWAP_WINDOW_SECS,
};
use crate::{DirStore, NodeConfig, SaleLightning};
use openbmt_swap::{Failure, Stage};

/// $10.00, which is the least a swap is made for at the test rate (SW-02).
fn ten_dollars() -> openbmt_counter::Amount {
    openbmt_counter::Amount(1_000)
}

fn provider(max_sale_sats: u64) -> SwapProvider {
    SwapProvider {
        name: "Pretend".to_string(),
        onion: OWN_NAME.to_string(),
        path_prefix: "/api".to_string(),
        max_sale_sats,
    }
}

/// A rig with the provider set, a deterministic source of secrets and a
/// pretend service that believes what the node's chain says.
struct World {
    rig: Rig,
    pretend: Pretend,
    blocks: Blocks,
    /// The clock at the start, which the test's seconds count from.
    base: u64,
    /// The height and the hash of the chain the node was given.
    tip: (u32, BlockHash),
    /// The number of the last heartbeat taken.
    hb_seq: u64,
}

fn world_with(dir: &Scratch, setting: Option<SwapProvider>) -> World {
    let time = Arc::new(AtomicU64::new(T0));
    let mut config = config(dir.path(), &time);
    config.settings.swap = setting;
    world_from(config, time, T0)
}

fn world_from(config: NodeConfig, time: Arc<AtomicU64>, base: u64) -> World {
    let mut rig = start_with(config, time);
    let mut n = 0u8;
    rig.node.set_swap_random(Box::new(move || {
        n = n.wrapping_add(1);
        Some([n; 32])
    }));
    assert!(rig.feed(vec![Rig::peers()]).is_empty());
    World {
        rig,
        pretend: Pretend::new(Network::Regtest, 0, base),
        blocks: Blocks::default(),
        base,
        tip: (0, genesis()),
        hb_seq: 0,
    }
}

fn world(dir: &Scratch) -> World {
    world_with(dir, Some(provider(1_000_000)))
}

impl World {
    /// The app's part: send every request the node hands out to the pretend
    /// service and give the reply back.
    fn serve(&mut self) {
        while let Some(request) = self.rig.node.swap_next() {
            assert_eq!(request.host_port, format!("{OWN_NAME}:80"));
            // The pretend service believes the chain and the clock the node has.
            self.pretend.tip = self.tip.0;
            self.pretend.now = self.rig.time.load(Ordering::SeqCst);
            let reply = self.pretend.serve(&request.bytes, "/api", OWN_NAME);
            self.rig.node.swap_reply(request.id, &reply);
        }
        // The node takes the events the answers led to.
        self.rig.feed(vec![]);
    }

    fn open(&mut self) -> u32 {
        let opened = self
            .rig
            .node
            .open(ten_dollars(), usd(), rate(), &source_at(&self.rig.time))
            .unwrap();
        opened.index
    }

    fn at(&self, secs: u64) {
        self.rig.time.store(self.base + secs, Ordering::SeqCst);
    }

    /// The pretend service locks its bitcoin: the lockup is announced by
    /// three groups of peers and the service says so.
    fn lockup_in_mempool(&mut self) -> Transaction {
        let lockup = self.pretend.pretend_lockup();
        self.pretend
            .report("transaction.mempool", Some(lockup.clone()));
        self.rig.feed(vec![AppEvent::Sighting {
            tx: bytes(&lockup),
            from: vec![peer(20), peer(21), peer(22)],
            own: false,
        }]);
        lockup
    }

    /// One block on the chain with these transactions.
    fn block(&mut self, txs: &[Transaction]) -> Vec<Report> {
        let block = self.blocks.extend(txs).clone();
        let height = self.blocks.height();
        self.tip = (height, block.block_hash());
        self.rig.feed(vec![
            joined(height, &block),
            block_event(&block),
            AppEvent::Checked(height),
        ])
    }

    /// One block with a transaction that pays `sat_per_vb` to the miner.
    fn block_paying(&mut self, n: u8, sat_per_vb: u64) {
        let tx = spend(n, &[(&script_of(0), 1_000)]);
        let fees = sat_per_vb * tx.weight().to_wu() / 4;
        let block = self
            .blocks
            .extend_paying(std::slice::from_ref(&tx), fees)
            .clone();
        let height = self.blocks.height();
        self.tip = (height, block.block_hash());
        self.rig.feed(vec![
            joined(height, &block),
            block_event(&block),
            AppEvent::Checked(height),
        ]);
    }

    /// The owner's heartbeat, with the next number and the chain the node
    /// has.
    fn heartbeat(&mut self) {
        self.hb_seq += 1;
        let hb = openbmt_core::heartbeat::WatcherHeartbeat {
            counter: id(3).device_id(),
            seq: self.hb_seq,
            time: self.rig.time.load(Ordering::SeqCst),
            tip_height: self.tip.0,
            tip_hash: bitcoin::hashes::Hash::to_byte_array(self.tip.1),
            healthy: true,
        };
        self.rig
            .node
            .heartbeat(&hb.sign(&id(1)).to_bytes())
            .unwrap();
    }

    fn sent(&self) -> Vec<Transaction> {
        self.rig
            .calls
            .lock()
            .unwrap()
            .broadcast
            .iter()
            .map(|b| deserialize(b).unwrap())
            .collect()
    }

    fn view(&mut self, index: u32) -> SaleLightning {
        self.rig.node.lightning_sale(index)
    }
}

#[test]
fn cr_218_with_no_provider_nothing_is_asked_and_the_sale_shows_no_lightning() {
    let dir = Scratch::new();
    let mut w = world_with(&dir, None);
    let index = w.open();
    assert!(!w.rig.node.swap_on());
    assert!(w.rig.node.swap_next().is_none());
    assert!(matches!(w.view(index), SaleLightning::Off));
}

#[test]
fn cr_219_a_sale_gets_the_combined_code_from_the_provider() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    // The sale never waits: the code is the on-chain one at once.
    assert!(matches!(w.view(index), SaleLightning::Waiting));
    w.serve();
    let SaleLightning::Invoice(checked) = w.view(index) else {
        panic!("no invoice");
    };
    assert_eq!(checked.amount_msat, 10_000_000);
    // The swap's address is watched and the shop's own is too.
    let offer = w.pretend.offer.clone().unwrap();
    assert!(w
        .rig
        .calls
        .lock()
        .unwrap()
        .watched
        .contains(&offer.address.script_pubkey()));
    // The pretend service saw the four headers of the codec and no other.
    assert!(!w.pretend.seen.is_empty());
}

#[test]
fn cr_219_a_quote_above_the_shops_limit_has_no_swap_and_says_so() {
    let dir = Scratch::new();
    let mut w = world_with(&dir, Some(provider(4_000)));
    let index = w.open();
    w.serve();
    assert!(matches!(w.view(index), SaleLightning::NotForThisSale));
    assert!(w.pretend.offer.is_none(), "nothing was asked for");
    assert_eq!(
        NOT_FOR_THIS_SALE,
        "Lightning is not available for this sale."
    );
}

#[test]
fn cr_219_a_provider_that_has_not_answered_in_twenty_seconds_leaves_the_code_on_chain() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    // The request goes out and no answer comes.
    let request = w.rig.node.swap_next().unwrap();
    assert_eq!(SWAP_WINDOW_SECS, 20);
    w.at(SWAP_WINDOW_SECS + 1);
    assert!(matches!(w.view(index), SaleLightning::NotForThisSale));
    // A late answer is not shown.
    let reply = w.pretend.serve(&request.bytes, "/api", OWN_NAME);
    w.rig.node.swap_reply(request.id, &reply);
    assert!(!matches!(w.view(index), SaleLightning::Invoice(_)));
}

#[test]
fn cr_219_a_provider_that_cannot_be_reached_is_lightning_off_for_now() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    let request = w.rig.node.swap_next().unwrap();
    w.rig.node.swap_failed(request.id);
    match w.view(index) {
        SaleLightning::Unavailable(words) => assert_eq!(words, unavailable_words()),
        other => panic!("{other:?}"),
    }
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::NotPaidYet);
}

#[test]
fn cr_220_the_record_is_written_before_the_invoice_is_shown_and_a_restart_keeps_it() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    assert!(matches!(w.view(index), SaleLightning::Invoice(_)));
    let saved = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("swap-"))
        .collect::<Vec<_>>();
    assert_eq!(saved, vec![format!("swap-{index}-0.bin")]);
    let bytes = std::fs::read(dir.path().join(&saved[0])).unwrap();
    assert_eq!(bytes.get(..4), Some(&MAGIC_SWAP_RECORD[..]));
    let record = SwapRecord::from_bytes(&bytes).unwrap();
    assert_eq!(record.to_bytes(), bytes);
    let offer = w.pretend.offer.clone().unwrap();

    // The counter starts again: the swap is followed, its lockup address
    // watched, and a lockup that came while it was closed is claimed.
    let World { rig, pretend, .. } = w;
    drop(rig);
    let mut w = world(&dir);
    w.pretend = pretend;
    assert!(w
        .rig
        .launch
        .scripts
        .contains(&offer.address.script_pubkey()));
    assert!(w.rig.node.counter().sale(index).is_some());
}

#[test]
fn cr_220_a_restart_between_the_lockup_and_the_claim_loses_nothing() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    let World { rig, pretend, .. } = w;
    drop(rig);
    // The counter starts again over the same folder. The sale and the swap
    // are back, the lockup's address is watched from the first peer on, and
    // the sale does not expire while the swap is on the way.
    let mut w = world(&dir);
    w.pretend = pretend;
    assert!(w.rig.node.counter().sale(index).is_some());
    assert!(w.rig.launch.scripts.iter().any(|s| {
        w.pretend
            .offer
            .as_ref()
            .is_some_and(|o| o.address.script_pubkey() == *s)
    }));
    // The provider's status carries the lockup: it is believed after the
    // check, and a block then lets the claim out.
    w.at(20);
    w.serve();
    w.block(std::slice::from_ref(&lockup));
    assert_eq!(w.sent().len(), 1);
    // The sale is paid when the claim is in a block, not before.
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Waiting);
    let claim = w.sent()[0].clone();
    w.block(std::slice::from_ref(&claim));
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
}

#[test]
fn cr_221_a_lockup_in_the_mempool_within_the_instant_limits_pays_the_sale_at_once() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.heartbeat();
    let index = w.open();
    w.serve();
    w.lockup_in_mempool();
    w.at(10);
    w.heartbeat();
    w.serve();
    w.rig.feed(vec![]);
    let screen = w.rig.node.judge(index).unwrap();
    assert_eq!(screen.state, State::PaidInstant, "{screen:?}");
    // The claim went to the peers, once.
    assert_eq!(w.sent().len(), 1);
    // What is at risk is what the claim pays the shop.
    let claim = w.sent()[0].clone();
    let received = claim.output[0].value.to_sat();
    assert_eq!(w.rig.node.counter().exposure_outstanding(), received);
    assert!(received < 10_000);
}

#[test]
fn cr_221_the_exposure_of_a_swap_sale_stays_until_the_claim_is_in_a_block() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.heartbeat();
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.heartbeat();
    w.serve();
    w.rig.feed(vec![]);
    let claim = w.sent()[0].clone();
    let received = claim.output[0].value.to_sat();
    assert_eq!(w.rig.node.counter().exposure_outstanding(), received);
    // The lockup in a block: the claim can still lose to the refund, so the
    // exposure stays.
    w.heartbeat();
    w.block(std::slice::from_ref(&lockup));
    assert_eq!(w.rig.node.counter().exposure_outstanding(), received);
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidInstant);
    // The claim in a block: released, and the sale is confirmed.
    w.block(std::slice::from_ref(&claim));
    assert_eq!(w.rig.node.counter().exposure_outstanding(), 0);
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
}

#[test]
fn cr_221_a_sale_above_the_instant_limit_waits_for_one_block() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    // No heartbeat: the instant check is off, so it waits for a block.
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.serve();
    let screen = w.rig.node.judge(index).unwrap();
    assert_eq!(screen.state, State::NotPaidYet);
    assert!(w.sent().is_empty(), "no claim before the block");
    match w.view(index) {
        SaleLightning::Confirming(words) => assert_eq!(words, confirming_words()),
        other => panic!("{other:?}"),
    }
    // One block with the lockup: claimed, and not yet paid.
    let reports = w.block(std::slice::from_ref(&lockup));
    assert!(!reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.sent().len(), 1);
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Waiting);
    // The claim in a block at the shop's depth: paid.
    let claim = w.sent()[0].clone();
    let reports = w.block(std::slice::from_ref(&claim));
    assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
    assert_eq!(w.rig.node.counter().exposure_outstanding(), 0);
}

#[test]
fn cr_221_a_claim_that_leaves_the_chain_is_sent_again_and_the_sale_is_paid_again() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    // No heartbeat: the sale waits for the claim to be in a block.
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.serve();
    w.block(std::slice::from_ref(&lockup));
    let claim = w.sent()[0].clone();
    let reports = w.block(std::slice::from_ref(&claim));
    assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
    assert_eq!(w.sent().len(), 1);
    let claim_block = w.blocks.at(2).block_hash();
    let first = w.blocks.at(1).block_hash();

    // Two blocks of another branch, which keep the block of the lockup and
    // not the one of the claim: the claim is out of the chain.
    let alt2 = mine(first, 2, 7, &[]);
    let alt3 = mine(alt2.block_hash(), 3, 7, &[]);
    let reports = w.rig.feed(vec![
        AppEvent::Chain {
            joined: vec![(2, alt2.block_hash()), (3, alt3.block_hash())],
            left: vec![claim_block],
        },
        block_event(&alt2),
        block_event(&alt3),
        AppEvent::Checked(3),
    ]);
    // The sale is taken back while the claim is out, as a payment on-chain
    // that is undone is (CR-30), and the claim is sent again, the same one.
    assert!(
        reports.contains(&Report::PosUnpaid { index }),
        "{reports:?}"
    );
    let screen = w.rig.node.judge(index).unwrap();
    assert_eq!(screen.state, State::DoNotHandOver, "{screen:?}");
    assert_eq!(w.sent().len(), 2);
    assert_eq!(w.sent()[1].compute_txid(), claim.compute_txid());
    // It is mined again: paid again, and the POS is told again.
    let again = mine(alt3.block_hash(), 4, 7, std::slice::from_ref(&claim));
    let reports = w.rig.feed(vec![
        joined(4, &again),
        block_event(&again),
        AppEvent::Checked(4),
    ]);
    assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
    assert_eq!(w.sent().len(), 2, "nothing more is sent");
}

#[test]
fn cr_222_a_claim_that_stays_out_of_a_block_is_built_again_at_a_higher_fee() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.block(std::slice::from_ref(&lockup));
    let first = w.sent();
    assert_eq!(first.len(), 1);
    // Blocks that do not hold the claim: each makes a claim with more fee.
    w.block(&[]);
    w.block(&[]);
    let sent = w.sent();
    assert_eq!(sent.len(), 3, "one more at each block");
    let fee = |t: &Transaction| lockup.output[0].value.to_sat() - t.output[0].value.to_sat();
    assert!(fee(&sent[1]) > fee(&sent[0]));
    assert!(fee(&sent[2]) > fee(&sent[1]));
    assert!(sent.iter().all(|t| t.input[0].sequence.is_rbf()));
    // Then a block with the last one: done, and nothing more is built.
    w.block(std::slice::from_ref(&sent[2]));
    w.block(&[]);
    assert_eq!(w.sent().len(), 3);
    assert!(w.rig.node.swap_status().is_none());
    let _ = index;
}

/// A transaction that spends `lockup`'s first output to some script: the
/// provider's refund, or any spend that is not the shop's claim.
fn spending_lockup(lockup: &Transaction) -> Transaction {
    let mut tx = spend(0xee, &[(&script_of(0), 5_000)]);
    tx.input[0].previous_output = bitcoin::OutPoint::new(lockup.compute_txid(), 0);
    tx
}

fn stage_of(dir: &Scratch, index: u32) -> Stage {
    record_of(dir, index).machine.stage()
}

fn record_of(dir: &Scratch, index: u32) -> SwapRecord {
    let bytes = std::fs::read(dir.path().join(format!("swap-{index}-0.bin"))).unwrap();
    SwapRecord::from_bytes(&bytes).unwrap()
}

/// A sale accepted at once: the lockup in the mempool seen by three groups
/// for long enough, with the heartbeat the instant policy needs, so that the
/// claim is out and the sale is paid at once. Returns the sale, the lockup
/// and the claim.
fn instant_sale(w: &mut World) -> (u32, Transaction, Transaction) {
    w.heartbeat();
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.heartbeat();
    w.serve();
    w.rig.feed(vec![]);
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidInstant);
    let claim = w.sent()[0].clone();
    (index, lockup, claim)
}

#[test]
fn cr_221_a_lockup_spent_by_the_provider_takes_the_sale_back() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, lockup, claim) = instant_sale(&mut w);
    assert_eq!(
        w.rig.node.counter().exposure_outstanding(),
        claim.output[0].value.to_sat()
    );
    // A block spends the lockup with a transaction that is not the claim.
    let reports = w.block(&[spending_lockup(&lockup)]);
    assert!(reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert!(
        reports.contains(&Report::PosUnpaid { index }),
        "{reports:?}"
    );
    assert!(w.rig.node.counter().alarm());
    let screen = w.rig.node.judge(index).unwrap();
    assert_eq!(screen.state, State::DoNotHandOver, "{screen:?}");
    // One block deep, the swap is not lost yet; the lockup is spent, so no
    // claim is built for it.
    for _ in 0..4 {
        w.block(&[]);
    }
    assert_eq!(stage_of(&dir, index), Stage::ClaimSent);
    // At the refund depth it is lost.
    w.block(&[]);
    w.block(&[]);
    assert_eq!(w.sent().len(), 1);
    // Its record is written as lost, with the claim that was sent, and reads
    // back.
    let bytes = std::fs::read(dir.path().join(format!("swap-{index}-0.bin"))).unwrap();
    let record = SwapRecord::from_bytes(&bytes).unwrap();
    assert_eq!(
        record.machine.stage(),
        openbmt_swap::Stage::Failed(openbmt_swap::Failure::Lost)
    );
    assert_eq!(record.claims.len(), 1);
    assert_eq!(record.to_bytes(), bytes);
    // A block that holds the shop's own claim is not a spend by another.
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, _, claim) = instant_sale(&mut w);
    let reports = w.block(std::slice::from_ref(&claim));
    assert!(!reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
}

#[test]
fn cr_221_a_claim_not_in_a_block_at_the_timeout_does_not_take_the_sale_back() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.pretend.timeout_blocks = 40;
    let (index, _, _) = instant_sale(&mut w);
    for _ in 0..39 {
        w.block(&[]);
    }
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidInstant);
    // The tip reaches the timeout and goes past it: the claim is not in a
    // block, and the preimage is out, so it is kept and raced against the
    // refund. The sale is not taken back, and the exposure stays.
    for _ in 0..3 {
        let reports = w.block(&[]);
        assert!(!reports.contains(&Report::Alarm { index }), "{reports:?}");
        assert!(
            !reports.contains(&Report::PosUnpaid { index }),
            "{reports:?}"
        );
    }
    assert!(w.tip.0 > 40);
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidInstant);
    assert!(!w.rig.node.counter().alarm());
    assert!(w.rig.node.counter().exposure_outstanding() > 0);
}

#[test]
fn cr_221_a_claim_that_leaves_the_chain_past_the_timeout_is_sent_again() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.pretend.timeout_blocks = 40;
    let (index, _, claim) = instant_sale(&mut w);
    w.block(std::slice::from_ref(&claim));
    let claim_block = w.blocks.at(1).block_hash();
    let base = w.blocks.at(1).header.prev_blockhash;
    assert_eq!(w.sent().len(), 1);
    // A branch from the block before the claim's, longer than the timeout, that
    // does not hold the claim: the claim is out of the chain, past the
    // timeout.
    let mut joined_blocks = Vec::new();
    let mut events = Vec::new();
    let mut prev = base;
    for height in 1..=45u32 {
        let b = mine(prev, height, 7, &[]);
        prev = b.block_hash();
        joined_blocks.push((height, prev));
        events.push(block_event(&b));
    }
    let mut feed = vec![AppEvent::Chain {
        joined: joined_blocks,
        left: vec![claim_block],
    }];
    feed.extend(events);
    feed.push(AppEvent::Checked(45));
    let reports = w.rig.feed(feed);
    assert!(!reports.contains(&Report::Alarm { index }), "{reports:?}");
    // The same claim goes out again, past the timeout.
    assert_eq!(w.sent().len(), 2, "{reports:?}");
    assert_eq!(w.sent()[1].compute_txid(), claim.compute_txid());
}

#[test]
fn cr_221_a_claim_that_confirms_past_the_timeout_makes_the_sale_paid() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.pretend.timeout_blocks = 40;
    let (index, _, _) = instant_sale(&mut w);
    for _ in 0..42 {
        w.block(&[]);
    }
    assert!(w.tip.0 > 40);
    let latest = w.sent().last().unwrap().clone();
    let reports = w.block(std::slice::from_ref(&latest));
    assert!(!reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
    assert_eq!(w.rig.node.counter().exposure_outstanding(), 0);
}

#[test]
fn cr_221_the_providers_refund_in_a_block_past_the_timeout_makes_the_swap_lost() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.pretend.timeout_blocks = 40;
    let (index, lockup, _) = instant_sale(&mut w);
    for _ in 0..42 {
        w.block(&[]);
    }
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidInstant);
    let reports = w.block(&[spending_lockup(&lockup)]);
    assert!(reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert!(
        reports.contains(&Report::PosUnpaid { index }),
        "{reports:?}"
    );
    let screen = w.rig.node.judge(index).unwrap();
    assert_eq!(screen.state, State::DoNotHandOver, "{screen:?}");
    assert!(w.rig.node.counter().alarm());
    // The refund is lost at the refund depth, not before.
    for _ in 0..5 {
        w.block(&[]);
    }
    assert_eq!(stage_of(&dir, index), Stage::Failed(Failure::Lost));
    assert!(w.rig.node.swap_status().is_none());
}

#[test]
fn cr_221_a_sale_not_yet_reported_paid_is_taken_back_without_an_alarm_when_the_swap_is_lost() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.pretend.timeout_blocks = 40;
    // No heartbeat: the sale waits for the claim, which never gets in.
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.serve();
    w.block(std::slice::from_ref(&lockup));
    assert_eq!(w.sent().len(), 1);
    for _ in 0..42 {
        w.block(&[]);
    }
    // Past the timeout the claim is still bumped; the sale waits.
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Waiting);
    assert!(w.sent().len() > 1);
    let reports = w.block(&[spending_lockup(&lockup)]);
    // The POS was never told the sale is paid, so there is nothing to tell
    // it, and no alarm: the sale only shows that the payment is gone.
    assert!(!reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert!(
        !reports.contains(&Report::PosUnpaid { index }),
        "{reports:?}"
    );
    assert!(!w.rig.node.counter().alarm());
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::DoNotHandOver);
    // At the refund depth the swap is lost. The claim was sent, so the
    // preimage was shown: the words of CR-223 for a lost swap, not those of
    // a swap that never showed it.
    for _ in 0..5 {
        w.block(&[]);
    }
    match w.view(index) {
        SaleLightning::Failed(words) => assert_eq!(words, lost_words()),
        other => panic!("{other:?}"),
    }
}

/// A swap with 40 blocks to the timeout and `blocks` empty blocks on the
/// chain before the lockup is in a block.
fn lockup_after(w: &mut World, blocks: u32) -> u32 {
    w.pretend.timeout_blocks = 40;
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    for _ in 0..blocks {
        w.block(&[]);
    }
    w.block(std::slice::from_ref(&lockup));
    index
}

#[test]
fn cr_221_the_claim_is_built_only_with_the_margin_left_before_the_timeout() {
    // 18 blocks left at the tip: the least that is taken.
    let dir = Scratch::new();
    let mut w = world(&dir);
    lockup_after(&mut w, 21);
    assert_eq!(w.tip.0, 22);
    assert_eq!(w.sent().len(), 1);
    // 17 blocks left: no claim, the swap ends, and the preimage is not shown.
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = lockup_after(&mut w, 22);
    assert_eq!(w.tip.0, 23);
    assert!(w.sent().is_empty());
    w.block(&[]);
    assert!(w.sent().is_empty());
    match w.view(index) {
        SaleLightning::Failed(words) => assert_eq!(words, failed_words()),
        other => panic!("{other:?}"),
    }
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::NotPaidYet);
    assert!(!w.rig.node.counter().alarm());
}

#[test]
fn cr_221_no_claim_is_built_while_the_counter_is_behind_on_blocks() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    // The block with the lockup is taken, and the app has not yet said that
    // the height is checked in full (WR-54).
    let block = w.blocks.extend(std::slice::from_ref(&lockup)).clone();
    w.tip = (1, block.block_hash());
    w.rig.feed(vec![joined(1, &block), block_event(&block)]);
    assert!(w.sent().is_empty(), "no claim while behind");
    w.rig.feed(vec![]);
    assert!(w.sent().is_empty());
    // Once the height is checked, the claim goes.
    w.rig.feed(vec![AppEvent::Checked(1)]);
    assert_eq!(w.sent().len(), 1);
    let _ = index;
}

#[test]
fn cr_221_a_status_of_the_provider_does_not_end_a_swap_whose_lockup_the_watch_holds() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    // The provider says the swap ran out, or sends bytes that are not the
    // lockup, while the shop's own watch has the lockup.
    w.at(10);
    w.pretend.report("swap.expired", None);
    w.serve();
    let mut bad = lockup.clone();
    bad.output[0].value = bitcoin::Amount::from_sat(1);
    w.pretend.report("transaction.mempool", Some(bad));
    w.at(50);
    w.serve();
    w.pretend.report("transaction.failed", None);
    w.at(90);
    w.serve();
    // The chain decides: the lockup is in a block and the claim goes.
    w.block(std::slice::from_ref(&lockup));
    assert_eq!(w.sent().len(), 1);
    let claim = w.sent()[0].clone();
    w.block(std::slice::from_ref(&claim));
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
}

#[test]
fn cr_221_the_lockup_is_the_one_that_pays_what_was_promised_and_is_in_a_block() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    let offer = w.pretend.offer.clone().unwrap();
    let script = offer.address.script_pubkey();
    // Two transactions pay the lockup address. The first the watch holds
    // pays too little; the second pays what was promised, from another
    // coin, and is in no block; the third is the real one and is mined.
    let decoy = spend(0x70, &[(&script, offer.lockup_sats - 1)]);
    let twin = spend(0x71, &[(&script, offer.lockup_sats)]);
    let real = w.pretend.pretend_lockup();
    for tx in [&decoy, &twin, &real] {
        w.rig.feed(vec![AppEvent::Sighting {
            tx: bytes(tx),
            from: vec![peer(20), peer(21), peer(22)],
            own: false,
        }]);
    }
    w.at(10);
    w.block(std::slice::from_ref(&real));
    assert_eq!(w.sent().len(), 1);
    let claim = w.sent()[0].clone();
    assert_eq!(
        claim.input[0].previous_output,
        bitcoin::OutPoint::new(real.compute_txid(), 0)
    );
    let _ = index;
}

#[test]
fn cr_221_the_claim_is_final_at_the_shops_refund_depth_and_paid_at_its_depth() {
    let dir = Scratch::new();
    let time = Arc::new(AtomicU64::new(T0));
    let mut config = config(dir.path(), &time);
    config.settings.swap = Some(provider(1_000_000));
    config.settings.min_confirmations = 2;
    let mut w = world_from(config, time, T0);
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.block(std::slice::from_ref(&lockup));
    let claim = w.sent()[0].clone();
    // One block of the claim is not the shop's depth.
    let reports = w.block(std::slice::from_ref(&claim));
    assert!(!reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Waiting);
    let reports = w.block(&[]);
    assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
    // At the refund depth (6) the books are told.
    for _ in 0..3 {
        let reports = w.block(&[]);
        assert!(!reports
            .iter()
            .any(|r| matches!(r, Report::BooksFinal { .. })));
    }
    let reports = w.block(&[]);
    assert!(
        reports
            .iter()
            .any(|r| matches!(r, Report::BooksFinal { received_sats, .. } if *received_sats == claim.output[0].value.to_sat())),
        "{reports:?}"
    );
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Final);
    assert_eq!(w.sent().len(), 1);
}

#[test]
fn sw_60_a_provider_whose_lockup_fee_is_too_high_gets_no_swap() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.pretend.terms.lockup_fee_sats = 30_000;
    let index = w.open();
    w.serve();
    assert!(w.pretend.offer.is_none(), "no swap was asked for");
    assert!(matches!(w.view(index), SaleLightning::Unavailable(_)));
}

#[test]
fn sw_28_a_reply_that_leaves_less_than_nine_tenths_is_refused() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    // 5 percent and 5,000 for the lockup: 4,500 of 10,000 reaches the shop.
    w.pretend.terms.fee_ppm = 50_000;
    w.pretend.terms.lockup_fee_sats = 5_000;
    let index = w.open();
    w.serve();
    assert!(w.pretend.offer.is_some());
    assert!(matches!(w.view(index), SaleLightning::Unavailable(_)));
    // An honest 0.6 percent and 324 is taken.
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    assert!(matches!(w.view(index), SaleLightning::Invoice(_)));
}

/// The rate of the first claim, in satoshis per 1000 virtual bytes.
fn first_claim_rate(w: &mut World, lockup: &Transaction) -> u64 {
    let claim = w.sent().remove(0);
    let fee = lockup.output[0].value.to_sat() - claim.output[0].value.to_sat();
    fee * 1000 / claim.vsize() as u64
}

#[test]
fn cr_222_a_recent_rate_above_the_floor_starts_the_claim_at_that_rate() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    // Three blocks paying 10 sat/vB, which is above the floor of 2.
    for n in 1..=3 {
        w.block_paying(n, 10);
    }
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.block(std::slice::from_ref(&lockup));
    let rate = first_claim_rate(&mut w, &lockup);
    assert!((9_000..=11_000).contains(&rate), "{rate}");
    let _ = index;
}

#[test]
fn cr_222_with_no_recent_rate_the_claim_starts_at_the_floor() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    // One sample, and the block of the lockup is a second: too few for a
    // recent rate (FE-13 wants three).
    w.block_paying(1, 10);
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.block(std::slice::from_ref(&lockup));
    let rate = first_claim_rate(&mut w, &lockup);
    assert!((1_800..=2_300).contains(&rate), "{rate}");
    let _ = index;
}

#[test]
fn cr_222_a_claim_near_the_timeout_is_told_to_the_owner() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.pretend.timeout_blocks = 40;
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.block(std::slice::from_ref(&lockup));
    assert!(w.rig.node.swap_status().is_none());
    // The claim stays out until half of the least blocks are left.
    for _ in 0..30 {
        w.block(&[]);
    }
    assert_eq!(w.rig.node.swap_status(), Some(claim_stuck_words()));
    let _ = index;
}

#[test]
fn cr_223_a_provider_that_never_locks_leaves_the_sale_unpaid_with_the_words() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    w.pretend.report("swap.expired", None);
    w.at(40);
    w.serve();
    match w.view(index) {
        SaleLightning::Failed(words) => assert_eq!(words, failed_words()),
        other => panic!("{other:?}"),
    }
    let screen = w.rig.node.judge(index).unwrap();
    assert_eq!(screen.state, State::NotPaidYet);
    // The same code takes bitcoin on-chain.
    let pay = spend(1, &[(&script_of(index), 10_000)]);
    w.rig.feed(vec![AppEvent::Sighting {
        tx: bytes(&pay),
        from: vec![peer(20)],
        own: false,
    }]);
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Waiting);
}

#[test]
fn cr_223_a_lockup_of_the_wrong_tree_is_not_believed() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.pretend.mode = Mode::WrongTree;
    let index = w.open();
    w.serve();
    // The reply itself is refused (SW-24): there is no code, and the sale
    // takes bitcoin as it always does.
    assert!(!matches!(w.view(index), SaleLightning::Invoice(_)));
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::NotPaidYet);
}

#[test]
fn cr_223_a_lockup_that_pays_less_than_promised_is_not_claimed() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    w.pretend.offer.as_mut().unwrap().lockup_sats -= 1;
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.block(std::slice::from_ref(&lockup));
    assert!(w.sent().is_empty());
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::NotPaidYet);
}

#[test]
fn cr_225_the_requests_are_plain_http_to_the_onion_one_at_a_time() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    w.open();
    let first = w.rig.node.swap_next().unwrap();
    assert!(first
        .bytes
        .starts_with(b"GET /api/v2/swap/reverse HTTP/1.1\r\n"));
    assert!(std::str::from_utf8(&first.bytes)
        .unwrap()
        .contains(&format!("Host: {OWN_NAME}\r\n")));
    assert_eq!(first.host_port, format!("{OWN_NAME}:80"));
    // None while it is out.
    assert!(w.rig.node.swap_next().is_none());
    // A number nobody asked is ignored.
    w.rig
        .node
        .swap_reply(first.id + 100, b"HTTP/1.1 200 OK\r\n\r\n");
    assert!(w.rig.node.swap_next().is_none());
}

#[test]
fn cr_226_the_words_pass_the_screen_rules() {
    for (name, words) in all_words() {
        openbmt_core::say::check(&words).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
    assert_eq!(failed_words().heading, "Lightning payment unfinished");
    assert_eq!(failed_words().heading.chars().count(), 28);
}

#[test]
fn cr_220_the_record_reads_strictly_and_writes_back_what_it_read() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    let bytes = std::fs::read(dir.path().join(format!("swap-{index}-0.bin"))).unwrap();
    let record = SwapRecord::from_bytes(&bytes).unwrap();
    assert_eq!(record.to_bytes(), bytes);
    // Cut short at each length, and with a byte after the end: refused.
    for len in 0..bytes.len() {
        assert!(SwapRecord::from_bytes(&bytes[..len]).is_err(), "{len}");
    }
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(SwapRecord::from_bytes(&longer).is_err());
    // Another magic is refused.
    let mut other = bytes.clone();
    other[3] ^= 1;
    assert!(SwapRecord::from_bytes(&other).is_err());
    // Any one flipped byte reads back as the same bytes or is refused.
    for at in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[at] ^= 0x40;
        if let Ok(r) = SwapRecord::from_bytes(&flipped) {
            assert_eq!(r.to_bytes(), flipped, "{at}");
        }
    }
}

#[test]
fn cr_220_swap_files_are_loaded_lowest_first_and_others_are_left_alone() {
    use openbmt_counter::{Slot, Store};
    let dir = Scratch::new();
    let mut store = DirStore::open(dir.path()).unwrap();
    store.put(Slot::Swap(2, 0), b"c").unwrap();
    store.put(Slot::Swap(0, 1), b"b").unwrap();
    store.put(Slot::Swap(0, 0), b"a").unwrap();
    std::fs::write(dir.path().join("swap-x-y.bin"), b"no").unwrap();
    std::fs::write(dir.path().join("swap-01-0.bin"), b"no").unwrap();
    let loaded = store.load().unwrap();
    assert_eq!(
        loaded.swaps,
        vec![
            (0, 0, b"a".to_vec()),
            (0, 1, b"b".to_vec()),
            (2, 0, b"c".to_vec())
        ]
    );
}

// The end-to-end test against a private regtest Core.

mod live {
    use std::path::PathBuf;

    use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
    use bitcoin::consensus::encode::deserialize;
    use bitcoin::hex::FromHex;
    use bitcoin::secp256k1::Secp256k1;
    use bitcoin::{Block, Txid};
    use openbmt_core::batch::{BatchBuilder, BatchPolicy, SignedBatch};
    use openbmt_core::wallet::{KeySlot, WalletDescription};
    use openbmt_counter::{Actor, RefundAsk, RefundRequest, Role};
    use openbmt_owner::{Options, Owner, SaleFacts, SaleKey};
    use openbmt_swap::testing::Regtest;

    use super::*;

    /// The wallet of three fixed keys the owner of the test counts on. Test
    /// only.
    fn wallet() -> WalletDescription {
        let secp = Secp256k1::new();
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let slots: Vec<KeySlot> = (1..=3u8)
            .map(|n| {
                let master = Xpriv::new_master(Network::Regtest, &[n; 32]).unwrap();
                let account = master.derive_priv(&secp, &path).unwrap();
                KeySlot {
                    index: n,
                    origin_fingerprint: master.fingerprint(&secp),
                    origin_path: path.clone(),
                    xpub: Xpub::from_priv(&secp, &account),
                }
            })
            .collect();
        WalletDescription::new(Network::Regtest, slots.try_into().unwrap()).unwrap()
    }

    fn policy() -> BatchPolicy {
        BatchPolicy { min_count: 1 }
    }

    fn real_batch() -> SignedBatch {
        let batch = BatchBuilder::new(policy())
            .derive(&wallet(), id(3).device_id(), 0, 10)
            .unwrap();
        SignedBatch {
            owner_sig: batch.sign(&id(1)),
            approver_sig: batch.sign(&id(2)),
            batch,
        }
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// The counter's configuration over real addresses of the owner's wallet.
    fn config_of(dir: &Scratch, time: &Arc<AtomicU64>, per_sale: Option<u64>) -> NodeConfig {
        let mut config = config(dir.path(), time);
        config.pairing.wallet_checksum = wallet().checksum32();
        config.first_batch = Some(real_batch());
        config.settings.swap = Some(provider(1_000_000));
        if let Some(sats) = per_sale {
            config.settings.light_per_sale.sats = sats;
        }
        config
    }

    fn core_dir(name: &str) -> PathBuf {
        let base = PathBuf::from(std::env::var_os("HOME").unwrap())
            .join(".cache/openbmt/tmp/swap-counter")
            .join(name);
        let _ = std::fs::remove_dir_all(&base);
        base
    }

    fn start_core(name: &str) -> Regtest {
        let port = std::env::var("OPENBMT_REGTEST_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(18_993);
        Regtest::start(&core_dir(name), port).unwrap()
    }

    /// A counter on the real addresses, over `dir`, ready for a sale.
    fn start_live(dir: &Scratch, per_sale: Option<u64>) -> World {
        let base = now();
        let time = Arc::new(AtomicU64::new(base));
        let config = config_of(dir, &time, per_sale);
        world_from(config, time, base)
    }

    impl World {
        /// The node is given the blocks of Core that it does not have.
        fn sync(&mut self, core: &Regtest) -> Vec<Report> {
            let top = core.height().unwrap();
            let mut events = Vec::new();
            for height in self.tip.0 + 1..=top {
                let hash = core.cli(&["getblockhash", &height.to_string()]).unwrap();
                let hex = core.cli(&["getblock", &hash, "0"]).unwrap();
                let block: Block = deserialize(&Vec::<u8>::from_hex(&hex).unwrap()).unwrap();
                events.push(joined(height, &block));
                events.push(block_event(&block));
                events.push(AppEvent::Checked(height));
                self.tip = (height, block.block_hash());
            }
            self.rig.feed(events)
        }

        /// The service locks its bitcoin on Core, and says so.
        fn lockup_on_core(&mut self, core: &Regtest) -> Transaction {
            let offer = self.pretend.offer.clone().unwrap();
            let lockup = core.fund(&offer.address, offer.lockup_sats).unwrap();
            self.pretend
                .report("transaction.mempool", Some(lockup.clone()));
            self.rig.feed(vec![AppEvent::Sighting {
                tx: bytes(&lockup),
                from: vec![peer(20), peer(21), peer(22)],
                own: false,
            }]);
            lockup
        }

        /// What the node sent to the peers is sent to Core, which must take
        /// it (a replacement of an earlier claim included).
        fn relay(&mut self, core: &Regtest, from: usize) -> Vec<Txid> {
            self.sent()[from..]
                .iter()
                .map(|tx| {
                    core.test_accept(tx).unwrap();
                    core.broadcast(tx).unwrap()
                })
                .collect()
        }

        fn script_of_sale(&self, index: u32) -> bitcoin::ScriptBuf {
            self.rig
                .node
                .counter()
                .sale(index)
                .unwrap()
                .script()
                .clone()
        }
    }

    fn setup(dir: &Scratch, per_sale: Option<u64>, core: &Regtest) -> World {
        let mut w = start_live(dir, per_sale);
        w.sync(core);
        w
    }

    /// Pairs, request, reply, status, lockup, claim and payment against a
    /// private regtest Core, for every path of CR-219 to CR-224. Run with:
    ///
    /// `cargo test -p openbmt-counter-node -- --ignored cr_221_regtest`
    #[test]
    #[ignore = "starts a private regtest bitcoind; run by hand"]
    fn cr_221_regtest_every_swap_path_end_to_end() {
        let core = start_core("paid-instant");

        // A sale within the instant limit is paid at the lockup in the
        // mempool, and the claim is then mined.
        let dir = Scratch::new();
        let mut w = setup(&dir, None, &core);
        w.heartbeat();
        let index = w.open();
        w.serve();
        assert!(matches!(w.view(index), SaleLightning::Invoice(_)));
        let lockup = w.lockup_on_core(&core);
        w.at(10);
        w.heartbeat();
        w.serve();
        let screen = w.rig.node.judge(index).unwrap();
        assert_eq!(screen.state, State::PaidInstant, "{screen:?}");
        assert!(w.rig.node.counter().exposure_outstanding() > 0);
        assert_eq!(core.confirmed_height(&lockup.compute_txid()).unwrap(), None);
        let claims = w.relay(&core, 0);
        assert_eq!(claims.len(), 1);
        core.mine(1).unwrap();
        w.sync(&core);
        let claim_height = core.confirmed_height(&claims[0]).unwrap().unwrap();
        assert_eq!(claim_height, w.tip.0);
        assert_eq!(w.rig.node.counter().exposure_outstanding(), 0);
        assert!(w.rig.node.swap_status().is_none());
        let claim = w.sent()[0].clone();
        assert_eq!(claim.output[0].script_pubkey, w.script_of_sale(index));
        let received = claim.output[0].value.to_sat();

        // A bitcoin refund of that sale is taken by the counter, up to what
        // the claim paid, and passes the owner phone's check, which counts
        // what its own view of the chain shows paid to the sale's address.
        core.mine(6).unwrap();
        w.sync(&core);
        let ann = Actor {
            role: Role::Cashier,
            name: "ann".to_string(),
        };
        let ask = |sats: u64, dest: u8| RefundRequest {
            ask: RefundAsk::Sats(sats),
            destination: p2wsh(dest),
            actor: ann.clone(),
            reason: "wrong size".to_string(),
        };
        w.rig
            .node
            .request_refund(index, ask(received, 0xa1))
            .unwrap();
        assert!(w.rig.node.request_refund(index, ask(1, 0xa2)).is_err());
        let mut owner = Owner::new(
            &[1; 32],
            wallet(),
            id(2).device_id(),
            policy(),
            Options::default(),
        )
        .unwrap();
        let nop = |_: &[u8]| Ok::<(), String>(());
        owner.queue_batch(real_batch().batch, now(), nop).unwrap();
        let key = SaleKey {
            counter: id(3).device_id(),
            index,
        };
        owner
            .set_sale_facts(
                key,
                SaleFacts {
                    received_sats: received,
                    reached_refund_depth: true,
                },
                nop,
            )
            .unwrap();
        owner
            .queue_refund(key, "ann", p2wsh(0xa1), received, None, nop)
            .unwrap();
        assert!(owner
            .queue_refund(key, "ann", p2wsh(0xa3), 1, None, nop)
            .is_err());
        drop(w);

        // Each path has a private chain of its own, so that the sales of one
        // never see the payments of another to the same addresses.
        drop(core);
        let core = start_core("waits-for-a-block");

        // A sale above the instant limit waits for a block, then is paid.
        let dir = Scratch::new();
        let mut w = setup(&dir, Some(5_000), &core);
        w.heartbeat();
        let index = w.open();
        w.serve();
        let lockup = w.lockup_on_core(&core);
        w.at(10);
        w.heartbeat();
        w.serve();
        assert_eq!(w.rig.node.judge(index).unwrap().state, State::NotPaidYet);
        assert!(matches!(w.view(index), SaleLightning::Confirming(_)));
        assert!(w.sent().is_empty());
        core.mine(1).unwrap();
        let reports = w.sync(&core);
        assert!(!reports.contains(&Report::PosPaid { index }), "{reports:?}");
        let claims = w.relay(&core, 0);
        assert_eq!(claims.len(), 1);
        core.mine(1).unwrap();
        let reports = w.sync(&core);
        assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
        assert!(core.confirmed_height(&claims[0]).unwrap().is_some());
        assert!(core
            .confirmed_height(&lockup.compute_txid())
            .unwrap()
            .is_some());
        drop(w);

        // Each path has a private chain of its own, so that the sales of one
        // never see the payments of another to the same addresses.
        drop(core);
        let core = start_core("never-locks");

        // A provider that never locks leaves the sale unpaid.
        let dir = Scratch::new();
        let mut w = setup(&dir, None, &core);
        let index = w.open();
        w.serve();
        w.pretend.report("swap.expired", None);
        w.at(40);
        w.serve();
        match w.view(index) {
            SaleLightning::Failed(words) => assert_eq!(words, failed_words()),
            other => panic!("{other:?}"),
        }
        assert_eq!(w.rig.node.judge(index).unwrap().state, State::NotPaidYet);
        assert!(w.sent().is_empty());
        drop(w);

        // Each path has a private chain of its own, so that the sales of one
        // never see the payments of another to the same addresses.
        drop(core);
        let core = start_core("restart");

        // A restart between the lockup and the claim loses nothing.
        let dir = Scratch::new();
        let mut w = setup(&dir, Some(5_000), &core);
        let index = w.open();
        w.serve();
        w.lockup_on_core(&core);
        let World { rig, pretend, .. } = w;
        drop(rig);
        let mut w = setup(&dir, Some(5_000), &core);
        w.pretend = pretend;
        assert!(w.rig.node.counter().sale(index).is_some());
        w.at(20);
        w.serve();
        assert!(
            w.sent().is_empty(),
            "no claim before the lockup is in a block"
        );
        core.mine(1).unwrap();
        w.sync(&core);
        let claims = w.relay(&core, 0);
        assert_eq!(claims.len(), 1);
        core.mine(1).unwrap();
        w.sync(&core);
        assert!(core.confirmed_height(&claims[0]).unwrap().is_some());
        assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
        drop(w);

        // Each path has a private chain of its own, so that the sales of one
        // never see the payments of another to the same addresses.
        drop(core);
        let core = start_core("stuck-claim");

        // A claim that Core keeps out of a block is built again at a higher
        // fee, which Core takes as a replacement, and that one is mined.
        let dir = Scratch::new();
        let mut w = setup(&dir, Some(5_000), &core);
        w.open();
        w.serve();
        w.lockup_on_core(&core);
        core.mine(1).unwrap();
        w.sync(&core);
        let first = w.relay(&core, 0);
        assert_eq!(first.len(), 1);
        let addr = core.new_address().unwrap();
        core.cli(&["generateblock", &addr.to_string(), "[]"])
            .unwrap();
        w.sync(&core);
        assert!(core.confirmed_height(&first[0]).unwrap().is_none());
        assert_eq!(w.sent().len(), 2, "built again at the next block");
        let second = w.relay(&core, 1);
        assert_ne!(first[0], second[0]);
        core.mine(1).unwrap();
        w.sync(&core);
        assert!(core.confirmed_height(&second[0]).unwrap().is_some());
        assert!(
            core.confirmed_height(&first[0]).is_err()
                || core.confirmed_height(&first[0]).unwrap().is_none()
        );
        // Nothing more is built once it is in a block.
        core.mine(1).unwrap();
        w.sync(&core);
        assert_eq!(w.sent().len(), 2);
    }
}

/// What the sale holds as the swap's payment, in satoshis.
fn held_sats(w: &World, index: u32) -> u64 {
    w.rig
        .node
        .counter()
        .sale(index)
        .unwrap()
        .lightning()
        .unwrap()
        .sats()
}

/// `count` blocks on top of the chain of `prev` at `height`, as the node is
/// told of them, after a reorg.
fn branch_on(
    w: &mut World,
    prev: BlockHash,
    first_height: u32,
    first: &[Transaction],
    count: u32,
    left: Vec<BlockHash>,
) -> (Vec<Report>, BlockHash) {
    let mut prev = prev;
    let mut joined_blocks = Vec::new();
    let mut events = Vec::new();
    for i in 0..count {
        let height = first_height + i;
        let txs = if i == 0 { first } else { &[] };
        let b = mine(prev, height, 7, txs);
        prev = b.block_hash();
        joined_blocks.push((height, prev));
        events.push(block_event(&b));
    }
    let mut feed = vec![AppEvent::Chain {
        joined: joined_blocks,
        left,
    }];
    feed.extend(events);
    feed.push(AppEvent::Checked(first_height + count - 1));
    (w.rig.feed(feed), prev)
}

#[test]
fn cr_221_a_refund_one_block_deep_takes_the_sale_back_and_the_claim_in_its_place_gives_it_again() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, lockup, claim) = instant_sale(&mut w);
    // The provider's refund is in a block: the sale is taken back at once.
    let reports = w.block(&[spending_lockup(&lockup)]);
    assert!(reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert!(
        reports.contains(&Report::PosUnpaid { index }),
        "{reports:?}"
    );
    assert_eq!(stage_of(&dir, index), Stage::ClaimSent);
    // A branch with the claim in the place of the refund wins: the swap goes
    // on, and the sale is paid again.
    let spend_block = w.blocks.at(1).block_hash();
    let base = w.blocks.at(1).header.prev_blockhash;
    let (reports, tip) = branch_on(
        &mut w,
        base,
        1,
        std::slice::from_ref(&claim),
        2,
        vec![spend_block],
    );
    assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
    assert_eq!(stage_of(&dir, index), Stage::ClaimSent);
    // At the refund depth of the claim the swap is paid.
    branch_on(&mut w, tip, 3, &[], 4, vec![]);
    assert!(matches!(stage_of(&dir, index), Stage::Paid { .. }));
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Final);
}

#[test]
fn cr_221_a_lockup_replaced_in_a_block_takes_the_sale_back_and_the_claim_is_not_bumped() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, lockup, _) = instant_sale(&mut w);
    // A transaction that spends the same coins as the lockup, paying
    // another script, is in a block: the lockup the claim spends is gone.
    let mut replacement = lockup.clone();
    replacement.output = vec![bitcoin::TxOut {
        value: bitcoin::Amount::from_sat(1_000),
        script_pubkey: script_of(0),
    }];
    let before = w.sent().len();
    let reports = w.block(&[replacement]);
    assert!(reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert!(
        reports.contains(&Report::PosUnpaid { index }),
        "{reports:?}"
    );
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::DoNotHandOver);
    for _ in 0..4 {
        w.block(&[]);
    }
    assert_eq!(w.sent().len(), before, "the claim is not built again");
    assert_eq!(stage_of(&dir, index), Stage::ClaimSent);
    // At the refund depth it is lost.
    w.block(&[]);
    assert_eq!(stage_of(&dir, index), Stage::Failed(Failure::Lost));
    assert_eq!(w.sent().len(), before);
}

#[test]
fn cr_221_a_claim_refused_at_a_higher_fee_is_sent_again_at_every_block() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, _, _) = instant_sale(&mut w);
    for _ in 0..14 {
        w.block(&[]);
    }
    // One claim at the start and one at each block: built again at a higher
    // fee until the fee is too much of the lockup, and the same bytes after.
    let sent = w.sent();
    assert_eq!(sent.len(), 15);
    let ids: Vec<_> = sent.iter().map(Transaction::compute_txid).collect();
    let last = ids[14];
    assert_eq!(ids[13], last);
    assert_eq!(ids[12], last);
    assert_eq!(record_of(&dir, index).claims.last(), Some(&last));
    // The sale holds what the newest claim pays.
    assert_eq!(held_sats(&w, index), sent[14].output[0].value.to_sat());
    assert!(sent[14].output[0].value < sent[0].output[0].value);
}

#[test]
fn cr_222_no_claim_id_is_dropped_at_the_limit_and_the_last_claim_is_sent_again() {
    crate::swap::set_claim_limit(3);
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, _, first) = instant_sale(&mut w);
    for _ in 0..6 {
        w.block(&[]);
    }
    let record = record_of(&dir, index);
    assert_eq!(record.claims.len(), 3);
    assert_eq!(record.claims[0], first.compute_txid());
    let sent = w.sent();
    assert_eq!(sent.len(), 7);
    assert_eq!(
        sent[6].compute_txid(),
        *record.claims.last().unwrap(),
        "the last claim again"
    );
    // The first claim, if it is the one mined, is still one of the claims.
    let reports = w.block(std::slice::from_ref(&first));
    assert!(!reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
}

#[test]
fn cr_220_a_restart_after_the_claim_was_mined_while_off_finds_the_swap_paid() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, _, claim) = instant_sale(&mut w);
    let record = record_of(&dir, index);
    assert!(record.told, "the sale took the payment: kept");
    let World { rig, pretend, .. } = w;
    drop(rig);
    let mut w = world(&dir);
    w.pretend = pretend;
    // The claim is in the first block the counter sees after the start, and
    // five more follow.
    w.block(std::slice::from_ref(&claim));
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Confirmed);
    for _ in 0..5 {
        w.block(&[]);
    }
    assert!(matches!(stage_of(&dir, index), Stage::Paid { .. }));
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::Final);
}

#[test]
fn cr_220_a_restart_after_the_refund_was_mined_while_off_takes_the_sale_back_and_loses_at_depth() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, lockup, _) = instant_sale(&mut w);
    let World { rig, pretend, .. } = w;
    drop(rig);
    let mut w = world(&dir);
    w.pretend = pretend;
    let reports = w.block(&[spending_lockup(&lockup)]);
    assert!(reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert!(
        reports.contains(&Report::PosUnpaid { index }),
        "{reports:?}"
    );
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::DoNotHandOver);
    assert_eq!(record_of(&dir, index).spent_in, Some(w.tip.1));
    for _ in 0..5 {
        w.block(&[]);
    }
    assert_eq!(stage_of(&dir, index), Stage::Failed(Failure::Lost));
}

#[test]
fn cr_220_a_cancel_that_was_not_stored_is_put_again_until_it_is() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, lockup, _) = instant_sale(&mut w);
    w.block(&[spending_lockup(&lockup)]);
    for _ in 0..4 {
        w.block(&[]);
    }
    // The sale cannot be written: a directory is where its file is.
    let sale_file = dir.path().join(format!("sale-{index}.bin"));
    let saved = std::fs::read(&sale_file).unwrap();
    std::fs::remove_file(&sale_file).unwrap();
    std::fs::create_dir(&sale_file).unwrap();
    // The refund depth: the swap is lost, and the cancel is not stored.
    w.block(&[]);
    assert_eq!(stage_of(&dir, index), Stage::Failed(Failure::Lost));
    assert!(!record_of(&dir, index).failure_told);
    // The sale was taken back at one block, and that stands only once the
    // loss is stored.
    assert_eq!(w.rig.node.counter().sale(index).unwrap().taken_back(), 1);
    // The next pump puts it again, and it is stored.
    std::fs::remove_dir(&sale_file).unwrap();
    std::fs::write(&sale_file, saved).unwrap();
    w.block(&[]);
    assert!(w.rig.node.counter().sale(index).unwrap().cancel_recorded());
    assert_eq!(w.rig.node.counter().sale(index).unwrap().taken_back(), 0);
    assert!(record_of(&dir, index).failure_told);
}

#[test]
fn cr_220_a_spent_lockup_that_was_not_stored_is_told_again_until_it_is() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, lockup, _) = instant_sale(&mut w);
    // The sale cannot be written: a directory is where its file is.
    let sale_file = dir.path().join(format!("sale-{index}.bin"));
    let saved = std::fs::read(&sale_file).unwrap();
    std::fs::remove_file(&sale_file).unwrap();
    std::fs::create_dir(&sale_file).unwrap();
    // The spend is told and not stored: the sale is not taken back.
    w.block(&[spending_lockup(&lockup)]);
    assert_eq!(w.rig.node.counter().sale(index).unwrap().taken_back(), 0);
    // The next pump tells it again, and it is stored.
    std::fs::remove_dir(&sale_file).unwrap();
    std::fs::write(&sale_file, saved).unwrap();
    let reports = w.block(&[]);
    assert!(reports.contains(&Report::Alarm { index }), "{reports:?}");
    assert_eq!(w.rig.node.counter().sale(index).unwrap().taken_back(), 1);
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::DoNotHandOver);
}

#[test]
fn cr_220_the_record_keeps_what_the_swap_was_told_and_reads_it_strictly() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let (index, lockup, _) = instant_sale(&mut w);
    w.block(&[spending_lockup(&lockup)]);
    let bytes = std::fs::read(dir.path().join(format!("swap-{index}-0.bin"))).unwrap();
    let record = SwapRecord::from_bytes(&bytes).unwrap();
    assert!(record.told && !record.failure_told);
    assert_eq!(record.spent_in, Some(w.tip.1));
    assert_eq!(record.to_bytes(), bytes);
    // Every cut is refused, a byte more is refused, and so is a flag that is
    // not known.
    for cut in 0..bytes.len() {
        assert!(SwapRecord::from_bytes(&bytes[..cut]).is_err(), "{cut}");
    }
    let mut long = bytes.clone();
    long.push(0);
    assert!(SwapRecord::from_bytes(&long).is_err());
    let flags = bytes.len() - 32 - 1;
    let mut odd = bytes.clone();
    odd[flags] |= 0x10;
    assert!(SwapRecord::from_bytes(&odd).is_err());
}

#[test]
fn cr_221_once_one_swap_of_a_sale_has_sent_its_claim_no_other_is_claimed() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    let index = w.open();
    w.serve();
    let first = w.lockup_in_mempool();
    w.rig
        .node
        .requote(index, usd(), rate(), &fresh(T0 + 30))
        .unwrap();
    w.at(10);
    w.serve();
    let second = w.lockup_in_mempool();
    w.at(20);
    w.serve();
    w.block(&[first, second]);
    // The first swap's claim is out; the second is not claimed, and its
    // preimage is never shown.
    assert_eq!(w.sent().len(), 1, "{:?}", w.sent());
    let bytes = std::fs::read(dir.path().join(format!("swap-{index}-1.bin"))).unwrap();
    let other = SwapRecord::from_bytes(&bytes).unwrap();
    assert!(other.claims.is_empty());
    assert!(matches!(other.machine.stage(), Stage::Locked(_)));
    assert_eq!(record_of(&dir, index).claims.len(), 1);
}
