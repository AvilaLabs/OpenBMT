//! Chains, signed batches, transactions, blocks, a memory store that can be
//! told to fail, and a small world that drives one counter.

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
use openbmt_core::command::{Action, OwnerCommand, SignedCommand};
use openbmt_core::identity::Identity;
use openbmt_core::policy::Heartbeat;
use openbmt_core::records::{Chain, Record};
use openbmt_core::settings::Settings;
use openbmt_watch::{HeaderChain, Health, PeerAddr, Problem, Source, WellFormedBlock};

use crate::{
    Actor, Amount, Counter, Currency, Facts, OpenFacts, Opened, Pairing, Rate, RateSource, Report,
    Role, Screen, Sightings, Slot, State, Store, Stored,
};

/// Satoshis per dollar in the test settings.
pub const SATS_PER_USD: u64 = 1_000;
/// The first moment.
pub const T0: u64 = 1_000_000;

/// A numbered identity.
pub fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

/// The owner (1) and the Key app (2).
pub fn roots() -> PinnedRoots {
    PinnedRoots::new(id(1).device_id(), id(2).device_id()).unwrap()
}

/// $100,000 per bitcoin, in cents: 1,000 sats per dollar.
pub fn rate() -> Rate {
    Rate::new(10_000_000).unwrap()
}

/// A rate source whose rate was fetched at `now`.
pub fn fresh(now: u64) -> RateSource {
    RateSource::new("test-fixed", now).unwrap()
}

/// Dollars.
pub fn usd() -> Currency {
    Currency::new(*b"USD").unwrap()
}

/// $5.00.
pub fn five_dollars() -> Amount {
    Amount(500)
}

/// Settings with light checks that can be accepted at once: the customers
/// cannot join the Wi-Fi. The smallest bitcoin sale is off (CR-75), so
/// sales of a few dollars open; the tests of CR-75 set it.
pub fn settings() -> Settings {
    let mut s = Settings::defaults(SATS_PER_USD);
    s.customer_wifi = false;
    s.min_sale_sats = 0;
    s.devices.insert(id(3).device_id());
    s.counters.insert(id(3).device_id(), 10_000);
    s
}

/// The pairing of the test counter (identity 3).
pub fn pairing() -> Pairing {
    Pairing {
        roots: roots(),
        wallet_checksum: [9; 32],
        batch_policy: BatchPolicy { min_count: 1 },
    }
}

/// A script that is P2WSH-shaped for the byte `n`.
pub fn p2wsh(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32]))
}

/// A signed batch of `count` addresses from `start` for the test counter.
pub fn batch(start: u32, count: u32) -> SignedBatch {
    let scripts = (start..start + count).map(|i| p2wsh(i as u8 + 1)).collect();
    let batch = AddressBatch {
        wallet_checksum: [9; 32],
        counter: id(3).device_id(),
        start,
        count,
        scripts,
    };
    SignedBatch {
        owner_sig: batch.sign(&id(1)),
        approver_sig: batch.sign(&id(2)),
        batch,
    }
}

/// A memory store. It counts writes and can be told to fail the n-th one
/// (counted from 1 after the last [`Mem::reset`]) or every one.
#[derive(Debug, Default)]
pub struct Mem {
    pub slots: BTreeMap<Slot, Vec<u8>>,
    pub records: Vec<Vec<u8>>,
    pub writes: usize,
    pub fail_on: Option<usize>,
    pub fail_all: bool,
    /// The slot or `record` each write went to, in order.
    pub log: Vec<String>,
}

impl Mem {
    /// Forget the count and the log, and stop failing.
    pub fn reset(&mut self) {
        self.writes = 0;
        self.fail_on = None;
        self.fail_all = false;
        self.log.clear();
    }

    fn step(&mut self, what: String) -> Result<(), &'static str> {
        self.writes += 1;
        if self.fail_all || self.fail_on == Some(self.writes) {
            self.log.push(format!("FAILED {what}"));
            return Err("disk full");
        }
        self.log.push(what);
        Ok(())
    }

    /// The state as the next start would read it.
    pub fn stored(&self) -> Stored<'_> {
        Stored {
            batches: self.slots.get(&Slot::Batches).map(Vec::as_slice),
            exposure: self.slots.get(&Slot::Exposure).map(Vec::as_slice),
            pause: self.slots.get(&Slot::Pause).map(Vec::as_slice),
            desk: self.slots.get(&Slot::Desk).map(Vec::as_slice),
            sales: self
                .slots
                .iter()
                .filter_map(|(slot, bytes)| match slot {
                    Slot::Sale(i) => Some((*i, bytes.as_slice())),
                    _ => None,
                })
                .collect(),
            records: Chain::from_records(
                self.records
                    .iter()
                    .map(|b| Record::from_bytes(b).unwrap())
                    .collect(),
            ),
        }
    }

    /// A counter started from what is stored.
    pub fn reload(&self) -> Counter {
        Counter::load(id(3), pairing(), settings(), self.stored()).unwrap()
    }

    /// The decoded events of the records written so far.
    pub fn events(&self) -> Vec<crate::Event> {
        self.records
            .iter()
            .map(|b| crate::Event::decode(&Record::from_bytes(b).unwrap().body).unwrap())
            .collect()
    }
}

impl Store for Mem {
    type Error = &'static str;

    fn put(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), &'static str> {
        self.step(format!("{slot:?}"))?;
        self.slots.insert(slot, bytes.to_vec());
        Ok(())
    }

    fn append_record(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.step("record".to_string())?;
        self.records.push(bytes.to_vec());
        Ok(())
    }
}

/// A header chain the tests can change.
pub struct TestChain {
    heights: BTreeMap<BlockHash, u32>,
    pub tip: u32,
    pub time: u64,
}

impl TestChain {
    pub fn new(tip: u32) -> Self {
        Self {
            heights: BTreeMap::new(),
            tip,
            time: T0,
        }
    }

    pub fn add(&mut self, hash: BlockHash, height: u32) {
        self.heights.insert(hash, height);
    }

    pub fn remove(&mut self, hash: &BlockHash) {
        self.heights.remove(hash);
    }
}

impl HeaderChain for TestChain {
    fn height_of(&self, block: &BlockHash) -> Option<u32> {
        self.heights.get(block).copied()
    }
    fn tip_height(&self) -> u32 {
        self.tip
    }
    fn tip_time(&self) -> u64 {
        self.time
    }
}

pub fn healthy() -> Health {
    Health {
        healthy: true,
        problem: None,
        behind: false,
    }
}

pub fn unhealthy(problem: Problem) -> Health {
    Health {
        healthy: false,
        problem: Some(problem),
        behind: false,
    }
}

pub fn heartbeat() -> Heartbeat {
    Heartbeat {
        verified: true,
        age_secs: 0,
    }
}

fn input(previous_output: OutPoint) -> TxIn {
    TxIn {
        previous_output,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::MAX,
        witness: Witness::new(),
    }
}

/// A coin that is not null.
pub fn coin(n: u8) -> OutPoint {
    OutPoint {
        txid: Txid::from_byte_array([n; 32]),
        vout: 0,
    }
}

/// A transaction spending coin `prev` and paying each (script, sats).
pub fn spend(prev: u8, outs: &[(&ScriptBuf, u64)]) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![input(coin(prev))],
        output: outs
            .iter()
            .map(|(s, v)| TxOut {
                value: Sats::from_sat(*v),
                script_pubkey: (*s).clone(),
            })
            .collect(),
    }
}

/// A coinbase; `tag` makes its id different from other coinbases.
pub fn coinbase(tag: u32) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(vec![0x04, 1, 2, 3, tag as u8]),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(5_000_000_000),
            script_pubkey: p2wsh(0xee),
        }],
    }
}

pub fn bytes(tx: &Transaction) -> Vec<u8> {
    serialize(tx)
}

fn mine_header(root: TxMerkleNode, time: u32) -> Header {
    let mut header = Header {
        version: BlockVersion::ONE,
        prev_blockhash: BlockHash::all_zeros(),
        merkle_root: root,
        time,
        bits: CompactTarget::from_consensus(0x207f_ffff),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce += 1;
    }
    header
}

/// A mined block of a coinbase and these transactions.
pub fn mined(tag: u32, txs: &[Transaction]) -> Block {
    let mut all = vec![coinbase(tag)];
    all.extend(txs.iter().cloned());
    let mut block = Block {
        header: mine_header(TxMerkleNode::all_zeros(), 1_000),
        txdata: all,
    };
    let root = block.compute_merkle_root().unwrap();
    block.header = mine_header(root, 1_000);
    block
}

/// A peer in its own /16 for each `n` from 20 up.
pub fn peer(n: u8) -> Source {
    Source::Peer(PeerAddr::Ip(format!("{n}.{n}.1.1").parse().unwrap()))
}

/// One counter, its store, a chain and a clock.
pub struct World {
    pub c: Counter,
    pub m: Mem,
    pub chain: TestChain,
    pub now: u64,
    pub health: Health,
    pub remote_order: bool,
    /// What the last override reported, made or refused.
    pub override_reports: Vec<Report>,
    next_coin: u8,
    next_block: u32,
}

impl World {
    /// A counter with 100 addresses, a chain with its tip at 100.
    pub fn new() -> Self {
        Self::with_addresses(100)
    }

    pub fn with_addresses(count: u32) -> Self {
        let mut c = Counter::new(id(3), pairing(), settings());
        let mut m = Mem::default();
        c.add_batch(batch(0, count), &mut m).unwrap();
        m.reset();
        Self {
            c,
            m,
            chain: TestChain::new(100),
            now: T0,
            health: healthy(),
            remote_order: false,
            override_reports: Vec::new(),
            next_coin: 1,
            next_block: 1,
        }
    }

    pub fn tick(&mut self, secs: u64) {
        self.now += secs;
    }

    pub fn open_facts(&self) -> OpenFacts {
        OpenFacts {
            health: self.health,
            rescan_pending: false,
            now: self.now,
        }
    }

    /// Open a sale of `cents` dollars-cents at the test rate.
    pub fn open(&mut self, cents: u64) -> Opened {
        let facts = self.open_facts();
        self.c
            .open(
                Amount(cents),
                usd(),
                rate(),
                &fresh(facts.now),
                facts,
                &mut self.m,
            )
            .unwrap()
    }

    pub fn open_five(&mut self) -> Opened {
        self.open(500)
    }

    pub fn judge(&mut self, index: u32) -> Screen {
        let facts = Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        self.c.judge(index, &facts, &mut self.m).unwrap()
    }

    pub fn override_sale(
        &mut self,
        index: u32,
        role: Role,
        name: &str,
    ) -> Result<Vec<Report>, crate::OverrideRefused<&'static str>> {
        let facts = Facts {
            chain: &self.chain,
            health: self.health,
            heartbeat: heartbeat(),
            remote_order: self.remote_order,
            now: self.now,
        };
        let done = self.c.override_sale(
            index,
            Actor {
                role,
                name: name.to_string(),
            },
            &facts,
            &mut self.m,
        );
        self.override_reports = done.reports.clone();
        done.result.map(|()| done.reports)
    }

    /// A transaction paying the sale `sats`, spending a coin of its own.
    pub fn pay(&mut self, o: &Opened, sats: u64) -> Transaction {
        self.pay_to(&o.script, sats)
    }

    pub fn pay_to(&mut self, script: &ScriptBuf, sats: u64) -> Transaction {
        let coin = self.next_coin;
        self.next_coin += 1;
        spend(coin, &[(script, sats)])
    }

    /// A transaction paying the sale that spends the coin `prev`.
    pub fn pay_from(&self, o: &Opened, prev: u8, sats: u64) -> Transaction {
        spend(prev, &[(&o.script, sats)])
    }

    /// Offer the transaction from `groups` peers in different groups.
    pub fn see(&mut self, tx: &Transaction, groups: u8) -> Sightings {
        let mut last = Sightings::default();
        for n in 20..20 + groups {
            last = self
                .c
                .sighting(&bytes(tx), peer(n), self.now, &mut self.m)
                .unwrap();
        }
        last
    }

    /// Offer the transaction from the Box.
    pub fn see_by_box(&mut self, tx: &Transaction) -> Sightings {
        self.c
            .sighting(&bytes(tx), Source::Box, self.now, &mut self.m)
            .unwrap()
    }

    /// Mine a block of these transactions at `height` on the chain, offer it
    /// to the counter and return it.
    pub fn confirm(&mut self, txs: &[Transaction], height: u32) -> Block {
        let block = self.mine_at(txs, height);
        self.offer(&block);
        block
    }

    /// Put a block on the chain without telling the counter.
    pub fn mine_at(&mut self, txs: &[Transaction], height: u32) -> Block {
        let block = mined(self.next_block, txs);
        self.next_block += 1;
        self.chain.add(block.block_hash(), height);
        block
    }

    pub fn offer(&mut self, block: &Block) -> crate::BlockTaken {
        let wf = WellFormedBlock::parse(&serialize(block)).unwrap();
        self.c
            .block(&wf, &self.chain, self.now, &mut self.m)
            .unwrap()
    }

    /// Take the block off the chain.
    pub fn orphan(&mut self, block: &Block) {
        self.chain.remove(&block.block_hash());
    }

    /// Bring a payment to the point where it is accepted at once: seen by
    /// three groups, then the conflict watch runs out.
    pub fn accept_at_once(&mut self, o: &Opened, tx: &Transaction) -> Screen {
        self.tick(10);
        self.see(tx, 3);
        let waiting = self.judge(o.index);
        assert_eq!(waiting.state, State::Waiting);
        self.tick(8);
        let screen = self.judge(o.index);
        assert_eq!(screen.state, State::PaidInstant);
        screen
    }

    /// An owner command with this number.
    pub fn owner_command(&self, seq: u64, action: Action) -> SignedCommand {
        let command = OwnerCommand {
            counter: id(3).device_id(),
            seq,
            action,
        };
        SignedCommand {
            owner_sig: command.sign(&id(1)),
            command,
        }
    }

    pub fn command(
        &mut self,
        seq: u64,
        action: Action,
    ) -> Result<(), crate::CommandRefused<&'static str>> {
        let signed = self.owner_command(seq, action);
        self.c.command(&signed, self.now, &mut self.m)
    }

    /// Start a second counter from what is stored, with the same chain.
    pub fn restart(&mut self) {
        self.c = self.m.reload();
    }

    /// Give the sale a new quote at this rate (minor units per bitcoin).
    pub fn requote(
        &mut self,
        index: u32,
        minor_per_btc: u64,
    ) -> Result<crate::Quote, crate::RequoteRefused<&'static str>> {
        self.c.requote(
            index,
            Rate::new(minor_per_btc).unwrap(),
            &fresh(self.now),
            self.now,
            &self.chain,
            &mut self.m,
        )
    }
}

/// A fee bump of `tx`: the same coin, more to the same script.
pub fn bump(tx: &Transaction, sats: u64) -> Transaction {
    let mut out = tx.clone();
    for o in &mut out.output {
        o.value = Sats::from_sat(sats);
    }
    out.lock_time = LockTime::from_height(1).unwrap();
    out
}

/// A transaction that spends the same coin as `tx` and pays the script
/// nothing: a cancel.
pub fn cancel_of(tx: &Transaction) -> Transaction {
    let mut out = tx.clone();
    out.output = vec![TxOut {
        value: Sats::from_sat(1_000),
        script_pubkey: p2wsh(0xdd),
    }];
    out
}

/// A transaction paying `script` that is too large for the watcher.
pub fn huge_paying(script: &ScriptBuf) -> Transaction {
    let mut tx = spend(250, &[(script, 5_000)]);
    for _ in 0..10_000 {
        tx.output.push(TxOut {
            value: Sats::from_sat(1),
            script_pubkey: p2wsh(0xcc),
        });
    }
    assert!(tx.total_size() > openbmt_watch::MAX_TX_BYTES);
    tx
}
