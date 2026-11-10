#![no_main]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use bitcoin::absolute::LockTime;
use bitcoin::block::{Header, Version as BlockVersion};
use bitcoin::consensus::encode::serialize;
use bitcoin::hashes::Hash;
use bitcoin::transaction::Version;
use bitcoin::{
    Amount as Sats, Block, BlockHash, CompactTarget, OutPoint, ScriptBuf, Sequence, Transaction,
    TxIn, TxMerkleNode, TxOut, Txid, WScriptHash, Witness,
};
use libfuzzer_sys::fuzz_target;
use openbmt_core::batch::{AddressBatch, BatchPolicy, PinnedRoots, SignedBatch};
use openbmt_core::command::{Action, OwnerCommand, SignedCommand};
use openbmt_core::identity::Identity;
use openbmt_core::policy::{CheckMode, Heartbeat};
use openbmt_core::records::{Chain, Record};
use openbmt_core::settings::Settings;
use openbmt_counter::{
    sale_payment_id, Actor, Amount, Counter, Currency, Event, Facts, OpenFacts, Pairing, Rate,
    RateSource, Report, Role, Slot, State, Store, Stored,
};
use openbmt_watch::{HeaderChain, Health, PeerAddr, Source, WellFormedBlock};

const T0: u64 = 1_000_000;
/// The most operations one run does, so that a long input cannot stall a run.
const MAX_OPS: usize = 3_000;
const NAMES: [&str; 4] = ["ann", "bob", "cy", ""];

fn id(n: u8) -> Identity {
    Identity::from_secret_bytes(&[n; 32]).unwrap()
}

fn roots() -> PinnedRoots {
    PinnedRoots::new(id(1).device_id(), id(2).device_id()).unwrap()
}

/// The pairing of the counter under test (identity 3).
fn pairing() -> Pairing {
    Pairing {
        roots: roots(),
        wallet_checksum: [9; 32],
        batch_policy: BatchPolicy { min_count: 1 },
    }
}

/// Light checks that can be accepted at once, and this counter holds the
/// whole share.
fn settings() -> Settings {
    let mut s = Settings::defaults(1_000);
    s.customer_wifi = false;
    s.devices.insert(id(3).device_id());
    s.counters.insert(id(3).device_id(), 10_000);
    s
}

fn p2wsh(n: u8) -> ScriptBuf {
    ScriptBuf::new_p2wsh(&WScriptHash::from_byte_array([n; 32]))
}

/// One signed batch of 100 addresses for the counter under test.
fn batch() -> &'static SignedBatch {
    static BATCH: OnceLock<SignedBatch> = OnceLock::new();
    BATCH.get_or_init(|| {
        let scripts = (0..100u32).map(|i| p2wsh(i as u8 + 1)).collect();
        let batch = AddressBatch {
            wallet_checksum: [9; 32],
            counter: id(3).device_id(),
            start: 0,
            count: 100,
            scripts,
        };
        SignedBatch {
            owner_sig: batch.sign(&id(1)),
            approver_sig: batch.sign(&id(2)),
            batch,
        }
    })
}

/// A memory store that fails the writes it is told to.
#[derive(Default)]
struct Mem {
    slots: BTreeMap<Slot, Vec<u8>>,
    records: Vec<Vec<u8>>,
    /// Writes since the current operation began.
    writes: usize,
    fail_on: Option<usize>,
    fail_all: bool,
    /// Failed writes in the whole run.
    failures: usize,
}

impl Mem {
    fn step(&mut self) -> Result<(), &'static str> {
        self.writes += 1;
        if self.fail_all || self.fail_on == Some(self.writes) {
            self.failures += 1;
            return Err("disk full");
        }
        Ok(())
    }

    /// Start an operation: 0 fails nothing, 1 to 14 fail that write of the
    /// operation, 15 fails all of them.
    fn begin(&mut self, plan: u8) {
        self.writes = 0;
        self.fail_on = (1..=14).contains(&plan).then_some(usize::from(plan));
        self.fail_all = plan == 15;
    }

    fn stored(&self) -> Stored<'_> {
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
}

impl Store for Mem {
    type Error = &'static str;

    fn put(&mut self, slot: Slot, bytes: &[u8]) -> Result<(), &'static str> {
        self.step()?;
        self.slots.insert(slot, bytes.to_vec());
        Ok(())
    }

    fn append_record(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        self.step()?;
        self.records.push(bytes.to_vec());
        Ok(())
    }
}

/// A header chain the run can grow and cut back.
struct Headers {
    heights: BTreeMap<BlockHash, u32>,
    tip: u32,
    time: u64,
}

impl HeaderChain for Headers {
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

fn spend(prev: u8, script: &ScriptBuf, sats: u64) -> Transaction {
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint {
                txid: Txid::from_byte_array([prev; 32]),
                vout: 0,
            },
            script_sig: ScriptBuf::new(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(sats),
            script_pubkey: script.clone(),
        }],
    }
}

fn coinbase(tag: u32) -> Transaction {
    let mut sig = vec![0x04];
    sig.extend_from_slice(&tag.to_le_bytes());
    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: ScriptBuf::from_bytes(sig),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Sats::from_sat(5_000_000_000),
            script_pubkey: p2wsh(0xee),
        }],
    }
}

fn mine_header(root: TxMerkleNode) -> Header {
    let mut header = Header {
        version: BlockVersion::ONE,
        prev_blockhash: BlockHash::all_zeros(),
        merkle_root: root,
        time: 1_000,
        bits: CompactTarget::from_consensus(0x207f_ffff),
        nonce: 0,
    };
    while header.validate_pow(header.target()).is_err() {
        header.nonce += 1;
    }
    header
}

fn mined(tag: u32, txs: &[Transaction]) -> Block {
    let mut all = vec![coinbase(tag)];
    all.extend(txs.iter().cloned());
    let mut block = Block {
        header: mine_header(TxMerkleNode::all_zeros()),
        txdata: all,
    };
    let root = block.compute_merkle_root().unwrap();
    block.header = mine_header(root);
    block
}

fn facts(chain: &Headers, now: u64) -> Facts<'_, Headers> {
    Facts {
        chain,
        health: Health {
            healthy: true,
            problem: None,
            behind: false,
        },
        heartbeat: Heartbeat {
            verified: true,
            age_secs: 0,
        },
        remote_order: false,
        now,
    }
}

fn is_green(state: State) -> bool {
    matches!(
        state,
        State::PaidInstant | State::Confirmed | State::Final | State::Overridden
    )
}

/// The input as a stream of bytes; a missing byte reads as 0.
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn next(&mut self) -> Option<u8> {
        let b = self.data.get(self.at).copied();
        self.at += 1;
        b
    }
    fn byte(&mut self) -> u8 {
        self.next().unwrap_or(0)
    }
    fn word(&mut self) -> u16 {
        u16::from_le_bytes([self.byte(), self.byte()])
    }
}

/// What the run has seen of one sale's reports.
#[derive(Default)]
struct Seen {
    /// Paid reports minus unpaid reports.
    paid: i32,
    books_final: u32,
}

struct Run {
    counter: Counter,
    mem: Mem,
    chain: Headers,
    now: u64,
    settings: Settings,
    max_slice: u64,
    /// Every sale handed out: its index and script.
    opened: Vec<(u32, ScriptBuf)>,
    scripts: HashSet<ScriptBuf>,
    txs: Vec<Transaction>,
    blocks: Vec<(BlockHash, u32)>,
    next_block: u32,
    events: Vec<Event>,
    seen: HashMap<u32, Seen>,
    alarm_was_up: bool,
}

impl Run {
    fn new() -> Self {
        let settings = settings();
        let me = id(3).device_id();
        let max_slice = [
            CheckMode::Light,
            CheckMode::NodeAlsoSeenByLight,
            CheckMode::BoxReportsOnly,
        ]
        .into_iter()
        .map(|mode| settings.slice_sats(&me, mode))
        .max()
        .unwrap();
        let mut counter = Counter::new(id(3), pairing(), settings.clone());
        let mut mem = Mem::default();
        counter.add_batch(batch().clone(), &mut mem).unwrap();
        Self {
            counter,
            mem,
            chain: Headers {
                heights: BTreeMap::new(),
                tip: 100,
                time: T0,
            },
            now: T0,
            settings,
            max_slice,
            opened: Vec::new(),
            scripts: HashSet::new(),
            txs: Vec::new(),
            blocks: Vec::new(),
            next_block: 1,
            events: Vec::new(),
            seen: HashMap::new(),
            alarm_was_up: false,
        }
    }

    /// A sale chosen by a byte: one of the sales opened, or one that does
    /// not exist.
    fn pick(&self, b: u8) -> (u32, ScriptBuf) {
        if b == 255 || self.opened.is_empty() {
            return (9_999, p2wsh(0xaa));
        }
        self.opened[usize::from(b) % self.opened.len()].clone()
    }

    /// Read the records the store holds that have not been read yet.
    fn sync_events(&mut self) {
        while self.events.len() < self.mem.records.len() {
            let bytes = &self.mem.records[self.events.len()];
            let record = Record::from_bytes(bytes).expect("a stored record reads back");
            let event = Event::decode(&record.body).expect("a stored event reads back");
            self.events.push(event);
        }
    }

    /// Check what an operation reported. `from` is how many events the log
    /// held before the operation: every report must be backed by an event
    /// the operation wrote.
    fn check_reports(&mut self, reports: &[Report], from: usize) {
        self.sync_events();
        let fresh = |pred: &dyn Fn(&Event) -> bool| self.events[from..].iter().any(pred);
        for report in reports {
            match report {
                Report::PosPaid { index } => {
                    assert!(
                        fresh(&|e| e.index() == *index
                            && matches!(
                                e,
                                Event::PaidAtOnce { .. }
                                    | Event::Confirmed { .. }
                                    | Event::Final { .. }
                                    | Event::Override { .. }
                            )),
                        "PosPaid for sale {index} without a record"
                    );
                }
                Report::PosUnpaid { index } => {
                    assert!(
                        fresh(&|e| e.index() == *index
                            && matches!(e, Event::Replaced { .. } | Event::Reopened { .. })),
                        "PosUnpaid for sale {index} without a record"
                    );
                }
                Report::Alarm { index } => {
                    assert!(
                        fresh(&|e| e.index() == *index
                            && matches!(e, Event::Replaced { .. } | Event::Reopened { .. })),
                        "Alarm for sale {index} without a record"
                    );
                    assert!(self.counter.alarm(), "Alarm reported but the alarm is down");
                }
                Report::BooksFinal { index, .. } => {
                    assert!(
                        fresh(&|e| e.index() == *index && matches!(e, Event::Final { .. })),
                        "BooksFinal for sale {index} without a record"
                    );
                }
                Report::LatePayment { index, sats } => {
                    assert!(
                        fresh(&|e| matches!(e, Event::LatePayment { index: i, sats: s, .. }
                            if i == index && s == sats)),
                        "LatePayment for sale {index} without a record"
                    );
                }
                Report::SurplusOwed { index, sats } => {
                    assert!(
                        fresh(&|e| matches!(e, Event::SurplusOwed { index: i, sats: s, .. }
                            if i == index && s == sats)),
                        "SurplusOwed for sale {index} without a record"
                    );
                }
                Report::Dust { index, sats } => {
                    assert!(
                        fresh(&|e| matches!(e, Event::Dust { index: i, sats: s, .. }
                            if i == index && s == sats)),
                        "Dust for sale {index} without a record"
                    );
                }
            }
            // The POS is told paid and unpaid in turn, starting with paid,
            // and the books are told once.
            match report {
                Report::PosPaid { index } => {
                    let seen = self.seen.entry(*index).or_default();
                    assert_eq!(seen.paid, 0, "PosPaid twice in a row for sale {index}");
                    seen.paid = 1;
                }
                Report::PosUnpaid { index } => {
                    let seen = self.seen.entry(*index).or_default();
                    assert_eq!(seen.paid, 1, "PosUnpaid without PosPaid for sale {index}");
                    seen.paid = 0;
                }
                Report::BooksFinal { index, .. } => {
                    let seen = self.seen.entry(*index).or_default();
                    seen.books_final += 1;
                    assert_eq!(seen.books_final, 1, "BooksFinal twice for sale {index}");
                }
                _ => {}
            }
        }
    }

    /// Rebuild the counter from what the store holds.
    fn restart(&mut self) {
        let loaded = Counter::load(id(3), pairing(), self.settings.clone(), self.mem.stored());
        let counter = match loaded {
            Ok(counter) => counter,
            Err(e) => panic!("the counter's own store did not load: {e:?}"),
        };
        self.counter = counter;
        let unreadable: Vec<u32> = self.counter.unreadable().collect();
        for slot in self.mem.slots.keys() {
            if let Slot::Sale(i) = slot {
                assert!(
                    self.counter.sale(*i).is_some() || unreadable.contains(i),
                    "sale {i} was stored but is neither loaded nor listed unreadable"
                );
            }
        }
        // The memory store writes whole or not at all, so what the counter
        // stored always reads back, failed writes or not.
        assert!(
            unreadable.is_empty(),
            "sales {unreadable:?} did not read back"
        );
    }

    fn op(&mut self, input: &mut Bytes<'_>, head: u8) {
        self.mem.begin(head >> 4);
        let from = {
            self.sync_events();
            self.events.len()
        };
        let failures_before = self.mem.failures;
        let mut cleared = false;
        let mut reports: Vec<Report> = Vec::new();
        match head % 11 {
            // Open a sale.
            0 => {
                let amount = Amount(1 + u64::from(input.word()));
                let rate = Rate::new((1 + u64::from(input.byte())) * 100_000).unwrap();
                let facts = OpenFacts {
                    health: Health {
                        healthy: true,
                        problem: None,
                        behind: false,
                    },
                    rescan_pending: false,
                    now: self.now,
                };
                let usd = Currency::new(*b"USD").unwrap();
                let source = RateSource::new("fuzz", self.now).unwrap();
                if let Ok(opened) = self
                    .counter
                    .open(amount, usd, rate, &source, facts, &mut self.mem)
                {
                    assert!(
                        self.scripts.insert(opened.script.clone()),
                        "a script was handed out twice"
                    );
                    self.opened.push((opened.index, opened.script));
                }
            }
            // Requote.
            1 => {
                let (index, _) = self.pick(input.byte());
                let rate = Rate::new((1 + u64::from(input.byte())) * 100_000).unwrap();
                let source = RateSource::new("fuzz", self.now).unwrap();
                let _ = self.counter.requote(
                    index,
                    rate,
                    &source,
                    self.now,
                    &self.chain,
                    &mut self.mem,
                );
            }
            // A sighting of a payment to a sale from some coin, seen by the
            // network groups in a mask.
            2 => {
                let (index, script) = self.pick(input.byte());
                let coin = input.byte() % 8;
                let kind = input.byte();
                let raw = u64::from(input.word());
                let groups = input.byte();
                let need = self
                    .counter
                    .sale(index)
                    .map_or(1_000, |s| s.needs_sats().max(1));
                let sats = match kind % 4 {
                    0 => need,
                    1 => need / 2 + 1,
                    2 => need.saturating_mul(2),
                    _ => raw * 4,
                };
                let tx = spend(coin, &script, sats);
                let bytes = serialize(&tx);
                self.txs.push(tx);
                for bit in 0..7u8 {
                    if groups & (1 << bit) == 0 {
                        continue;
                    }
                    let source = if bit == 6 {
                        Source::Box
                    } else {
                        let n = 20 + bit;
                        Source::Peer(PeerAddr::Ip(format!("{n}.{n}.1.1").parse().unwrap()))
                    };
                    if let Ok(mut out) = self.counter.sighting(&bytes, source, self.now, &mut self.mem)
                    {
                        reports.append(&mut out.reports);
                    }
                }
            }
            // A block of earlier transactions at the next height.
            3 => {
                let mask = input.byte();
                let skip = input.byte() & 1 == 1;
                let chosen: Vec<Transaction> = (0..8)
                    .filter(|bit| mask & (1 << bit) != 0)
                    .filter_map(|bit| {
                        self.txs
                            .len()
                            .checked_sub(1 + bit)
                            .and_then(|i| self.txs.get(i))
                    })
                    .cloned()
                    .collect();
                // A block never holds the same transaction twice.
                let mut chosen = chosen;
                let mut ids = HashSet::new();
                chosen.retain(|tx| ids.insert(tx.compute_txid()));
                let block = mined(self.next_block, &chosen);
                self.next_block += 1;
                let height = self.chain.tip + 1;
                self.chain.heights.insert(block.block_hash(), height);
                self.chain.tip = height;
                self.chain.time = self.now;
                self.blocks.push((block.block_hash(), height));
                if !skip {
                    let wf = WellFormedBlock::parse(&serialize(&block))
                        .expect("a block the run built is well formed");
                    if let Ok(mut out) = self.counter.block(&wf, &self.chain, self.now, &mut self.mem)
                    {
                        reports.append(&mut out.reports);
                    }
                }
            }
            // Take the tip block off the chain.
            4 => {
                if let Some((hash, height)) = self.blocks.pop() {
                    self.chain.heights.remove(&hash);
                    self.chain.tip = height - 1;
                }
            }
            // Time passes.
            5 => {
                self.now += u64::from(input.word()) * 2;
            }
            // Judge a sale.
            6 => {
                let (index, _) = self.pick(input.byte());
                let facts = facts(&self.chain, self.now);
                let result = self.counter.judge(index, &facts, &mut self.mem);
                if let Ok(mut screen) = result {
                    if is_green(screen.state)
                        && screen
                            .reports
                            .iter()
                            .any(|r| matches!(r, Report::PosPaid { .. }))
                    {
                        assert_eq!(
                            self.mem.failures, failures_before,
                            "a green state with PosPaid after a failed write"
                        );
                    }
                    reports.append(&mut screen.reports);
                }
            }
            // A person overrides.
            7 => {
                let (index, _) = self.pick(input.byte());
                let role = if input.byte() & 1 == 0 {
                    Role::Cashier
                } else {
                    Role::Manager
                };
                let name = NAMES[usize::from(input.byte()) % NAMES.len()].to_string();
                let facts = facts(&self.chain, self.now);
                let mut done =
                    self.counter
                        .override_sale(index, Actor { role, name }, &facts, &mut self.mem);
                reports.append(&mut done.reports);
            }
            // The cashier cancels.
            8 => {
                let (index, _) = self.pick(input.byte());
                let _ = self.counter.cancel(index, self.now, &mut self.mem);
            }
            // An owner command.
            9 => {
                let b = input.byte();
                let seq = if b < 128 {
                    u64::from(b)
                } else {
                    self.counter.desk().last_command_seq() + 1
                };
                let (index, _) = self.pick(input.byte());
                let action = match input.byte() % 3 {
                    0 => Action::ClearCancelAlarm,
                    1 => Action::ResetPause,
                    _ => Action::ReleaseExposure(sale_payment_id(index)),
                };
                let command = OwnerCommand {
                    counter: id(3).device_id(),
                    seq,
                    action,
                };
                let signed = SignedCommand {
                    owner_sig: command.sign(&id(1)),
                    command,
                };
                if self.counter.command(&signed, self.now, &mut self.mem).is_ok()
                    && action == Action::ClearCancelAlarm
                {
                    cleared = true;
                }
            }
            // Restart from the store.
            _ => self.restart(),
        }

        self.check_reports(&reports, from);

        assert!(
            self.counter.exposure_outstanding() <= self.max_slice,
            "exposure passes the largest slice"
        );
        if cleared {
            assert!(!self.counter.alarm(), "the alarm stayed up after a clear");
        } else if self.alarm_was_up {
            assert!(self.counter.alarm(), "the alarm went down without a clear");
        }
        self.alarm_was_up = self.counter.alarm();
    }
}

fuzz_target!(|data: &[u8]| {
    // Operations: a byte whose low 4 bits choose the operation and whose
    // high 4 bits choose which write of that operation fails (0 none, 15
    // all); the bytes after it are its parameters.
    let mut input = Bytes { data, at: 0 };
    let mut run = Run::new();
    for _ in 0..MAX_OPS {
        let Some(head) = input.next() else {
            break;
        };
        run.op(&mut input, head);
    }
});
