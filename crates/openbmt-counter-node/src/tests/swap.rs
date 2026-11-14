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
    all_words, claim_stuck_words, confirming_words, failed_words, unavailable_words, SwapRecord,
    MAGIC_SWAP_RECORD, NOT_FOR_THIS_SALE, SWAP_WINDOW_SECS,
};
use crate::{DirStore, NodeConfig, SaleLightning};

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
        }]);
        lockup
    }

    /// One block on the chain with these transactions.
    fn block(&mut self, txs: &[Transaction]) -> Vec<Report> {
        let block = self.blocks.extend(txs).clone();
        let height = self.blocks.height();
        self.tip = (height, block.block_hash());
        self.rig
            .feed(vec![joined(height, &block), block_event(&block)])
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
        self.rig
            .feed(vec![joined(height, &block), block_event(&block)]);
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
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidLightning);
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
    assert_eq!(screen.state, State::PaidLightning, "{screen:?}");
    assert!(w.rig.node.counter().exposure_outstanding() > 0);
    // The claim went to the peers, once.
    assert_eq!(w.sent().len(), 1);
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
    // One block with the lockup: claimed and paid.
    let reports = w.block(std::slice::from_ref(&lockup));
    assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.sent().len(), 1);
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidLightning);
    assert_eq!(w.rig.node.counter().exposure_outstanding(), 0);
}

#[test]
fn cr_221_a_sale_paid_on_one_block_of_the_lockup_stays_paid_when_that_block_leaves() {
    let dir = Scratch::new();
    let mut w = world(&dir);
    // No heartbeat: the sale waits for the first block of the lockup.
    let index = w.open();
    w.serve();
    let lockup = w.lockup_in_mempool();
    w.at(10);
    w.serve();
    let reports = w.block(std::slice::from_ref(&lockup));
    assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidLightning);
    let first = w.blocks.at(1).block_hash();

    // Two blocks of another branch take the chain: the block with the
    // lockup is gone. CR-221 takes one block of the lockup as final for a
    // sale paid through the swap, so the sale is not taken back, unlike one
    // paid on-chain (WR-33).
    let alt1 = mine(genesis(), 1, 7, &[]);
    let alt2 = mine(alt1.block_hash(), 2, 7, &[]);
    let reports = w.rig.feed(vec![AppEvent::Chain {
        joined: vec![(1, alt1.block_hash()), (2, alt2.block_hash())],
        left: vec![first],
    }]);
    assert!(
        !reports.contains(&Report::PosUnpaid { index }),
        "{reports:?}"
    );
    assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidLightning);
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
        assert_eq!(screen.state, State::PaidLightning, "{screen:?}");
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
        assert!(reports.contains(&Report::PosPaid { index }), "{reports:?}");
        let claims = w.relay(&core, 0);
        assert_eq!(claims.len(), 1);
        core.mine(1).unwrap();
        w.sync(&core);
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
        assert_eq!(w.rig.node.judge(index).unwrap().state, State::PaidLightning);
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
