//! Chain data from the compact-filter watcher (SL-26, SL-27): the counter's
//! own peer layer (`openbmt-net-io`) with no server that would learn the
//! shop's addresses, in place of the shop's Bitcoin Core.
//!
//! Two parts:
//!
//! - [`Follower`] holds the bookkeeping and nothing else: it is told what the
//!   driver says (the best chain, the blocks whose filters matched, the
//!   headers) and what the library registers through `chain::Filter`, and it
//!   calls `chain::Confirm` on the library's objects in the order the library
//!   asks for. It has no thread and no socket, so its tests drive it with
//!   made-up events.
//! - [`FilterRun`] owns the driver and the thread that reads its events and
//!   hands them to the [`Follower`].
//!
//! The driver checks the headers (`openbmt_watch::HeaderStore`, WR-05) and the
//! blocks it hands over (WR-20, WR-21); nothing here checks a header again. The
//! one place a header is read back is [`window`], which reads the driver's
//! stored header file through the store's own reader.
//!
//! What is watched (SL-27): the funding output of each channel and every
//! output the library registers with `chain::Filter`, as scripts. A script
//! that the library registers is watched until the node stops; the set is
//! made again at each start from the monitors and the sweeper, which register
//! what they hold when they are loaded, so no copy of it is kept on disk and
//! nothing about it is written in the order of SL-24. What the node does keep
//! across a start is where the library had got to: each monitor, the manager
//! and the sweeper keep their own best block, and the watcher starts its scan
//! from the lowest of them.
//!
//! Reorganizations: the library's `get_relevant_txids` names each transaction
//! it still needs told about, with the block it was confirmed in. When blocks
//! leave the best chain, or when the watcher starts, each such transaction
//! whose block is no longer on the best chain at its height is given to
//! `transaction_unconfirmed`. The blocks of the new chain that matter come
//! from the driver as blocks again and are given to `transactions_confirmed`.

use crate::chain::{CoreFees, FilterWatcher};
use crate::feehistory::FeeKeeper;
use bitcoin::block::Header;
use bitcoin::consensus::deserialize;
use bitcoin::{Block, BlockHash, Network, OutPoint, Script, ScriptBuf, Transaction, Txid};
use lightning::chain::{Confirm, WatchedOutput};
use openbmt_net_io::{
    AppEvent, DirLock, Driver, DriverConfig, FileStorage, Handle, Key, OsRandomness, Storage,
    SystemClock,
};
use openbmt_watch::{HeaderStore, WellFormedBlock};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// An object of the library that is told the chain: the channel manager, the
/// chain monitor and the output sweeper.
pub type Confirmable = Arc<dyn Confirm + Send + Sync>;

/// Headers kept behind the tip, for a reorganization to be told from them.
const KEEP_BEHIND: u32 = 2016;
/// Blocks held while the best chain does not yet say where they are.
const MAX_WAITING_BLOCKS: usize = 8;
/// Times a block is given again because a delivery registered something new.
const MAX_PASSES: usize = 64;
/// How long the thread waits for an event before it looks at the rest.
const WAIT: Duration = Duration::from_millis(100);

/// Where the filter watcher keeps its data and how it reaches peers.
#[derive(Debug, Clone)]
pub struct FilterConfig {
    /// The watcher's directory: the header store and the address book of the
    /// driver. The node owns it for as long as it runs (NR-79).
    pub dir: PathBuf,
    /// The driver's settings. `watch_from` is set by the node; `proxy` is set
    /// by the node when Tor is on (SL-32).
    pub driver: DriverConfig,
}

/// What the library registered, held until the follower takes it.
enum Registration {
    Tx(Txid, ScriptBuf),
    Output {
        block_hash: Option<BlockHash>,
        outpoint: OutPoint,
        script: ScriptBuf,
    },
}

/// The [`FilterWatcher`] the library is given: it only queues what it is told,
/// so that a call made in the middle of a block never waits for the follower.
#[derive(Default)]
pub struct FilterQueue {
    queue: Mutex<Vec<Registration>>,
}

impl FilterQueue {
    fn take(&self) -> Vec<Registration> {
        std::mem::take(&mut *self.queue.lock().unwrap())
    }
}

impl FilterWatcher for FilterQueue {
    fn register_tx(&self, txid: &Txid, script_pubkey: &Script) {
        self.queue
            .lock()
            .unwrap()
            .push(Registration::Tx(*txid, script_pubkey.to_owned()));
    }

    fn register_output(&self, output: WatchedOutput) {
        self.queue.lock().unwrap().push(Registration::Output {
            block_hash: output.block_hash,
            outpoint: output.outpoint.into_bitcoin_outpoint(),
            script: output.script_pubkey,
        });
    }
}

/// What the follower asks of the driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Watch this script.
    Watch(ScriptBuf),
    /// Check the blocks above this height again against every script watched.
    Rescan(u32),
    /// The transaction the node sent through the peers is in a block.
    Forget(Txid),
    /// A line for the node's log.
    Note(String),
}

/// The headers of the best chain from `from` up, read from the driver's
/// stored header file by the store's own reader (WR-08), which checks every
/// header again (WR-05).
pub fn window(network: Network, stored: &[u8], from: u32) -> Result<Vec<(u32, Header)>, String> {
    let store = HeaderStore::from_bytes(network, stored).map_err(|e| e.to_string())?;
    let first = from.max(store.start_height());
    Ok((first..=store.tip().0)
        .filter_map(|h| store.header_at(h).map(|header| (h, *header)))
        .collect())
}

/// The bookkeeping of the watcher. See the module docs.
pub struct Follower {
    confirmables: Vec<Confirmable>,
    queue: Arc<FilterQueue>,
    /// The best chain as the driver has told it: height to hash.
    chain: BTreeMap<u32, BlockHash>,
    heights: HashMap<BlockHash, u32>,
    headers: HashMap<BlockHash, Header>,
    /// The last best block the library was given.
    tip_given: Option<(u32, BlockHash)>,
    /// The lowest height any part of the library had reached at the start.
    floor: u32,
    started: bool,
    txs: HashMap<Txid, ScriptBuf>,
    outputs: HashMap<OutPoint, ScriptBuf>,
    scripts: HashSet<ScriptBuf>,
    /// What was given to `transactions_confirmed`, by block.
    given: HashSet<(BlockHash, Txid)>,
    broadcast: HashSet<Txid>,
    waiting: Vec<(BlockHash, Vec<u8>)>,
    /// The height of the block being given, while it is.
    current: Option<u32>,
    /// The height up to which the driver says every block is checked.
    checked: u32,
    effects: Vec<Effect>,
}

impl Follower {
    /// A follower for these objects, fed by this queue. `floor` is the lowest
    /// best block height of the library's parts, or the start of the chain.
    pub fn new(confirmables: Vec<Confirmable>, queue: Arc<FilterQueue>, floor: u32) -> Self {
        Self {
            confirmables,
            queue,
            chain: BTreeMap::new(),
            heights: HashMap::new(),
            headers: HashMap::new(),
            tip_given: None,
            floor,
            started: false,
            txs: HashMap::new(),
            outputs: HashMap::new(),
            scripts: HashSet::new(),
            given: HashSet::new(),
            broadcast: HashSet::new(),
            waiting: Vec::new(),
            current: None,
            checked: 0,
            effects: Vec::new(),
        }
    }

    /// The best chain as it was stored, before the driver tells any change.
    pub fn seed(&mut self, stored: Vec<(u32, Header)>) {
        for (height, header) in stored {
            let hash = header.block_hash();
            self.headers.insert(hash, header);
            self.set_at(height, hash);
        }
    }

    /// Takes what the library registered while it was loaded and returns the
    /// scripts to watch from the first peer on. No rescan is asked for: the
    /// driver starts at the floor. Then checks the transactions the library
    /// holds as confirmed against the stored chain.
    pub fn begin(&mut self) -> Vec<ScriptBuf> {
        self.drain_registrations();
        self.started = true;
        self.reconcile(None);
        self.try_tip();
        let _ = self.take_effects();
        self.scripts.iter().cloned().collect()
    }

    /// The height of the last best block the library was given.
    pub fn tip_given(&self) -> Option<(u32, BlockHash)> {
        self.tip_given
    }

    /// The height of a block of the best chain, as the driver told it.
    pub fn height_of(&self, hash: &BlockHash) -> Option<u32> {
        self.heights.get(hash).copied()
    }

    /// The hash of the block at a height of the best chain.
    pub fn hash_at(&self, height: u32) -> Option<BlockHash> {
        self.chain.get(&height).copied()
    }

    /// The newest block the driver has told of.
    pub fn chain_tip(&self) -> Option<(u32, BlockHash)> {
        self.chain.iter().next_back().map(|(h, b)| (*h, *b))
    }

    /// The lowest height the scan starts from.
    pub fn floor(&self) -> u32 {
        self.floor
    }

    /// A transaction the node sent through the peers. It is forgotten by the
    /// driver when a block with it is seen.
    pub fn note_broadcast(&mut self, txid: Txid) {
        self.broadcast.insert(txid);
    }

    /// Every block up to this height is checked against the scripts watched.
    pub fn on_checked(&mut self, height: u32) -> Vec<Effect> {
        self.checked = self.checked.max(height);
        self.try_tip();
        self.take_effects()
    }

    /// Headers the driver has stored (they are of its best chain, not
    /// necessarily the one the follower was told last).
    pub fn on_headers(&mut self, stored: Vec<(u32, Header)>) -> Vec<Effect> {
        for (_, header) in stored {
            self.headers.insert(header.block_hash(), header);
        }
        self.drain_registrations();
        self.retry_waiting();
        self.try_tip();
        self.take_effects()
    }

    /// The best chain changed: `left` (old tip first) are out of it, `joined`
    /// (lowest first) are in.
    pub fn on_chain(&mut self, joined: &[(u32, BlockHash)], left: &[BlockHash]) -> Vec<Effect> {
        let mut lowest_left: Option<u32> = None;
        for hash in left {
            if let Some(height) = self.heights.remove(hash) {
                if self.chain.get(&height) == Some(hash) {
                    self.chain.remove(&height);
                }
                lowest_left = Some(lowest_left.map_or(height, |l| l.min(height)));
            }
            self.given.retain(|(b, _)| b != hash);
        }
        for (height, hash) in joined {
            self.set_at(*height, *hash);
        }
        self.drain_registrations();
        if !left.is_empty() {
            self.reconcile(lowest_left);
        }
        self.retry_waiting();
        self.try_tip();
        self.take_effects()
    }

    /// A block of the best chain whose filter matched, as the driver checked
    /// it. The transactions of it that the library asked to see are given to
    /// `transactions_confirmed`, and then the best block.
    pub fn on_block(&mut self, hash: BlockHash, bytes: Vec<u8>) -> Vec<Effect> {
        self.drain_registrations();
        self.give_block(hash, bytes);
        self.try_tip();
        self.take_effects()
    }

    fn set_at(&mut self, height: u32, hash: BlockHash) {
        if let Some(old) = self.chain.insert(height, hash) {
            if old != hash && self.heights.get(&old) == Some(&height) {
                self.heights.remove(&old);
            }
        }
        if let Some(old_height) = self.heights.insert(hash, height) {
            if old_height != height && self.chain.get(&old_height) == Some(&hash) {
                self.chain.remove(&old_height);
            }
        }
    }

    fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Takes what the library registered. A script that is new is watched; one
    /// that may be in a block already passed makes the driver check again from
    /// there.
    fn drain_registrations(&mut self) {
        let base = self
            .current
            .or(self.tip_given.map(|(h, _)| h))
            .unwrap_or(self.floor);
        for registration in self.queue.take() {
            let (script, hint) = match registration {
                Registration::Tx(txid, script) => {
                    self.txs.insert(txid, script.clone());
                    (script, None)
                }
                Registration::Output {
                    block_hash,
                    outpoint,
                    script,
                } => {
                    self.outputs.insert(outpoint, script.clone());
                    (
                        script,
                        block_hash.and_then(|b| self.heights.get(&b).copied()),
                    )
                }
            };
            let new_script = self.scripts.insert(script.clone());
            if new_script {
                self.effects.push(Effect::Watch(script));
            }
            // At the start the driver begins at the floor anyway.
            if self.started && (new_script || hint.is_some_and(|h| h < base)) {
                let from = hint.map_or(base, |h| h.min(base));
                self.effects.push(Effect::Rescan(from.saturating_sub(1)));
            }
        }
    }

    /// Gives a transaction that the library holds as confirmed in a block that
    /// is no longer on the best chain to `transaction_unconfirmed`. A block
    /// below the heights the follower knows is taken to be too deep to have
    /// left.
    fn reconcile(&mut self, lowest_left: Option<u32>) {
        let gone = |height: u32| lowest_left.is_some_and(|l| height >= l);
        for confirmable in &self.confirmables {
            for (txid, height, hash) in confirmable.get_relevant_txids() {
                let stale = match (hash, self.chain.get(&height)) {
                    (Some(h), Some(on)) => *on != h,
                    _ => gone(height),
                };
                if stale {
                    confirmable.transaction_unconfirmed(&txid);
                    self.given.retain(|(_, t)| *t != txid);
                    self.effects.push(Effect::Note(format!(
                        "unconfirmed {txid} (was at {height})"
                    )));
                }
            }
        }
    }

    fn retry_waiting(&mut self) {
        let held = std::mem::take(&mut self.waiting);
        for (hash, bytes) in held {
            self.give_block(hash, bytes);
        }
    }

    fn give_block(&mut self, hash: BlockHash, bytes: Vec<u8>) {
        let Some(&height) = self.heights.get(&hash) else {
            // The best chain does not say where this block is yet.
            if self.waiting.len() >= MAX_WAITING_BLOCKS {
                self.waiting.remove(0);
            }
            self.waiting.push((hash, bytes));
            return;
        };
        let block: Block = match WellFormedBlock::parse(&bytes) {
            Ok(wb) if wb.hash() == hash => match deserialize(&bytes) {
                Ok(block) => block,
                Err(_) => return,
            },
            _ => {
                self.effects
                    .push(Effect::Note(format!("block {hash} not as told")));
                return;
            }
        };
        self.headers.insert(hash, block.header);
        let ids: Vec<Txid> = block.txdata.iter().map(Transaction::compute_txid).collect();
        self.current = Some(height);
        for _ in 0..MAX_PASSES {
            // What the last delivery registered may be in this block too.
            self.drain_registrations();
            let matched: Vec<(usize, &Transaction)> = block
                .txdata
                .iter()
                .enumerate()
                .filter(|(i, tx)| {
                    !self.given.contains(&(hash, ids[*i]))
                        && (self.txs.contains_key(&ids[*i])
                            || tx
                                .input
                                .iter()
                                .any(|i| self.outputs.contains_key(&i.previous_output)))
                })
                .collect();
            if matched.is_empty() {
                break;
            }
            for (i, _) in &matched {
                self.given.insert((hash, ids[*i]));
                self.effects
                    .push(Effect::Note(format!("given {} at {height}", ids[*i])));
            }
            for confirmable in &self.confirmables {
                confirmable.transactions_confirmed(&block.header, &matched, height);
            }
        }
        self.current = None;
        for id in ids {
            if self.broadcast.remove(&id) {
                self.effects.push(Effect::Forget(id));
            }
        }
    }

    /// Gives the library the best block, once its header is known and the
    /// driver has checked every block up to it, so that a transaction in an
    /// earlier block is given before the library is told the chain is longer.
    fn try_tip(&mut self) {
        let Some((&height, &hash)) = self.chain.iter().next_back() else {
            return;
        };
        if self.tip_given == Some((height, hash)) || self.checked < height {
            return;
        }
        let Some(header) = self.headers.get(&hash) else {
            return;
        };
        for confirmable in &self.confirmables {
            confirmable.best_block_updated(header, height);
        }
        self.tip_given = Some((height, hash));
        self.prune(height);
    }

    fn prune(&mut self, tip: u32) {
        let keep = tip.saturating_sub(KEEP_BEHIND);
        let below: Vec<u32> = self.chain.range(..keep).map(|(h, _)| *h).collect();
        for h in below {
            if let Some(hash) = self.chain.remove(&h) {
                self.heights.remove(&hash);
                self.headers.remove(&hash);
            }
        }
        // Headers of blocks the chain does not name (yet) are few; if they
        // pile up, only those of the chain are kept.
        if self.headers.len() > 4 * KEEP_BEHIND as usize {
            let heights = &self.heights;
            self.headers.retain(|hash, _| heights.contains_key(hash));
        }
        let heights = &self.heights;
        self.given.retain(|(b, _)| heights.contains_key(b));
    }
}

/// The latest header file the driver saved, handed from the driver's thread to
/// the follower's.
#[derive(Default)]
pub struct HeaderTap {
    latest: Mutex<Option<Vec<u8>>>,
}

impl HeaderTap {
    fn put(&self, bytes: &[u8]) {
        *self.latest.lock().unwrap() = Some(bytes.to_vec());
    }

    fn take(&self) -> Option<Vec<u8>> {
        self.latest.lock().unwrap().take()
    }
}

/// The driver's storage with a copy of each header file it saves, so that the
/// follower reads the headers the driver already checked. The driver saves the
/// headers before it hands over a block, and otherwise at most once a minute.
struct TapStorage {
    inner: FileStorage,
    tap: Arc<HeaderTap>,
}

impl Storage for TapStorage {
    type Error = <FileStorage as Storage>::Error;

    fn load(&mut self, key: Key) -> Result<Option<Vec<u8>>, Self::Error> {
        self.inner.load(key)
    }

    fn save(&mut self, key: Key, bytes: &[u8]) -> Result<(), Self::Error> {
        let saved = self.inner.save(key, bytes);
        if key == Key::Headers && saved.is_ok() {
            self.tap.put(bytes);
        }
        saved
    }
}

/// A fee sample from a block the driver handed over, at its height.
fn sample(
    keeper: &mut FeeKeeper,
    follower: &Follower,
    hash: &BlockHash,
    bytes: &[u8],
    log: &Arc<dyn Fn(&str) + Send + Sync>,
) {
    let (Some(height), Some((tip, _))) = (follower.height_of(hash), follower.chain_tip()) else {
        return;
    };
    for note in keeper.take(hash, height, bytes, tip) {
        log(&note);
    }
}

/// What the thread tells the node as it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The height of the last best block the library was given.
    pub height: u32,
    /// The driver has peers, none is ahead of it, and the library has been
    /// given the driver's tip with every block up to it checked.
    pub in_sync: bool,
}

/// The driver and the thread that follows it. Dropping it stops both.
pub struct FilterRun {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// What [`FilterRun::start`] is given.
pub struct FilterStart {
    /// The directory and the driver's settings.
    pub config: FilterConfig,
    /// The network.
    pub network: Network,
    /// The follower, with the library's registrations still queued.
    pub follower: Follower,
    /// Transactions the library sends, to go to the peers.
    pub broadcasts: Receiver<Vec<u8>>,
    /// The fee rates the library reads (SL-26a): the watcher keeps their
    /// samples.
    pub fees: Arc<CoreFees>,
    /// The node's directory, where `fee-history.bin` is kept.
    pub data_dir: PathBuf,
    /// Told after each event.
    pub progress: Arc<dyn Fn(Progress) + Send + Sync>,
    /// The node's log.
    pub log: Arc<dyn Fn(&str) + Send + Sync>,
}

impl FilterRun {
    /// Takes the directory, starts the driver at the follower's floor with the
    /// scripts known now, and starts the thread that follows it.
    pub fn start(start: FilterStart) -> Result<Self, String> {
        let FilterStart {
            config,
            network,
            mut follower,
            broadcasts,
            fees,
            data_dir,
            progress,
            log,
        } = start;
        let lock = DirLock::acquire(&config.dir).map_err(|e| e.to_string())?;
        let mut storage = FileStorage::new(&config.dir).map_err(|e| e.to_string())?;
        if let Some(stored) = storage.load(Key::Headers).map_err(|e| e.to_string())? {
            match window(
                network,
                &stored,
                follower.floor().saturating_sub(KEEP_BEHIND),
            ) {
                Ok(headers) => follower.seed(headers),
                Err(e) => log(&format!("stored headers not read: {e}")),
            }
        }
        let scripts = follower.begin();
        let tip_now = follower.chain_tip().map_or(follower.floor(), |(h, _)| h);
        let (mut keeper, notes) = FeeKeeper::load(&data_dir, network, fees, tip_now);
        for note in notes {
            log(&note);
        }
        for note in keeper.prune(|h| follower.hash_at(h), tip_now) {
            log(&note);
        }
        let tap = Arc::new(HeaderTap::default());
        let mut driver = config.driver;
        driver.network = network;
        driver.watch_from = follower.floor();
        let handle = Driver::spawn(
            driver,
            TapStorage {
                inner: storage,
                tap: Arc::clone(&tap),
            },
            SystemClock,
            OsRandomness::new().map_err(|e| e.to_string())?,
            scripts,
        )
        .map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("shop-ln-filters".into())
            .spawn(move || {
                let _lock = lock;
                run(
                    handle, follower, keeper, tap, broadcasts, flag, network, progress, log,
                );
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Stops the thread and the driver, which saves its headers.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for FilterRun {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    mut handle: Handle,
    mut follower: Follower,
    mut keeper: FeeKeeper,
    tap: Arc<HeaderTap>,
    broadcasts: Receiver<Vec<u8>>,
    stop: Arc<AtomicBool>,
    network: Network,
    progress: Arc<dyn Fn(Progress) + Send + Sync>,
    log: Arc<dyn Fn(&str) + Send + Sync>,
) {
    let mut driver_tip = 0u32;
    let mut fee_tip = 0u32;
    let mut peers_ready = false;
    let mut last_progress: Option<Progress> = None;
    loop {
        if stop.load(Ordering::SeqCst) {
            handle.shutdown();
            return;
        }
        let mut effects = Vec::new();
        for tx in broadcasts.try_iter() {
            if let Ok(parsed) = deserialize::<Transaction>(&tx) {
                follower.note_broadcast(parsed.compute_txid());
            }
            handle.broadcast(tx);
        }
        if let Some(stored) = tap.take() {
            let from = follower.floor().saturating_sub(KEEP_BEHIND);
            match window(network, &stored, from) {
                Ok(headers) => effects.extend(follower.on_headers(headers)),
                Err(e) => log(&format!("headers not read: {e}")),
            }
        }
        match handle.events().recv_timeout(WAIT) {
            Ok(AppEvent::Chain { joined, left }) => {
                if !left.is_empty() {
                    let lowest_left = left.iter().filter_map(|h| follower.height_of(h)).min();
                    let from = lowest_left
                        .into_iter()
                        .chain(joined.first().map(|(h, _)| *h))
                        .min();
                    if let Some(from) = from {
                        let tip = joined.last().map_or(from, |(h, _)| *h);
                        for note in keeper.forget_from(from, &left, tip) {
                            log(&note);
                        }
                    }
                }
                effects.extend(follower.on_chain(&joined, &left));
            }
            Ok(AppEvent::Block { hash, bytes }) => {
                effects.extend(follower.on_block(hash, bytes.clone()));
                sample(&mut keeper, &follower, &hash, &bytes, &log);
            }
            Ok(AppEvent::FeeBlock { hash, bytes }) => {
                sample(&mut keeper, &follower, &hash, &bytes, &log);
            }
            Ok(AppEvent::Checked(height)) => effects.extend(follower.on_checked(height)),
            Ok(AppEvent::Peers(peers, ahead, (height, _))) => {
                peers_ready = !peers.is_empty() && ahead.is_none();
                driver_tip = height;
            }
            Ok(AppEvent::Note(text)) => log(&format!("driver: {text}")),
            Ok(AppEvent::TxRelayed {
                txid,
                peers_asked,
                peers_fetched,
            }) => log(&format!(
                "relayed {txid}: asked {peers_asked}, fetched {peers_fetched}"
            )),
            Ok(AppEvent::TxState { txid, state }) => log(&format!("peers on {txid}: {state:?}")),
            Ok(AppEvent::Stopped) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(_) | Err(RecvTimeoutError::Timeout) => {}
        }
        // The fee history: the windows move with the tip, and a block is asked
        // for when none was sampled for 24 blocks (FE-20).
        if let (Some((height, hash)), Some((given, _))) =
            (follower.chain_tip(), follower.tip_given())
        {
            if given == height {
                if height != fee_tip {
                    fee_tip = height;
                    keeper.tip_moved(height);
                }
                if let Some(ask) = keeper.want_block(height, hash) {
                    handle.fetch_block(ask);
                }
            }
        }
        // What the library registered meanwhile, and a header that came late.
        effects.extend(follower.on_headers(Vec::new()));
        let mut rescan: Option<u32> = None;
        for effect in effects {
            match effect {
                Effect::Watch(script) => handle.watch_script(script),
                Effect::Rescan(height) => rescan = Some(rescan.map_or(height, |r| r.min(height))),
                Effect::Forget(txid) => handle.forget(txid),
                Effect::Note(text) => log(&text),
            }
        }
        if let Some(height) = rescan {
            handle.rescan_from(height);
        }
        let given = follower.tip_given();
        let now = Progress {
            height: given.map_or(0, |(h, _)| h),
            in_sync: peers_ready && given.is_some_and(|(h, _)| h == driver_tip),
        };
        if last_progress != Some(now) || now.in_sync {
            progress(now);
            last_progress = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::absolute::LockTime;
    use bitcoin::block::Version;
    use bitcoin::hashes::Hash;
    use bitcoin::transaction::Version as TxVersion;
    use bitcoin::{Amount, CompactTarget, Sequence, TxIn, TxOut, Witness};

    /// A confirmable that records what it is told, and holds what it says is
    /// relevant.
    #[derive(Default)]
    struct Rec {
        calls: Mutex<Vec<String>>,
        relevant: Mutex<Vec<(Txid, u32, Option<BlockHash>)>>,
        /// Registered when a transaction is confirmed, as the library does
        /// for the outputs of a transaction it has just seen.
        on_confirm: Mutex<Option<(Arc<FilterQueue>, WatchedOutput)>>,
    }

    impl Rec {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Confirm for Rec {
        fn transactions_confirmed(
            &self,
            header: &Header,
            txdata: &lightning::chain::transaction::TransactionData,
            height: u32,
        ) {
            let ids: Vec<String> = txdata
                .iter()
                .map(|(i, tx)| format!("{i}:{}", &tx.compute_txid().to_string()[..8]))
                .collect();
            self.calls.lock().unwrap().push(format!(
                "confirmed [{}] at {height} in {}",
                ids.join(","),
                &header.block_hash().to_string()[..8]
            ));
            if let Some((queue, output)) = self.on_confirm.lock().unwrap().take() {
                queue.register_output(output);
            }
        }

        fn transaction_unconfirmed(&self, txid: &Txid) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("unconfirmed {}", &txid.to_string()[..8]));
            self.relevant.lock().unwrap().retain(|(t, _, _)| t != txid);
        }

        fn best_block_updated(&self, header: &Header, height: u32) {
            self.calls.lock().unwrap().push(format!(
                "best {height} {}",
                &header.block_hash().to_string()[..8]
            ));
        }

        fn get_relevant_txids(&self) -> Vec<(Txid, u32, Option<BlockHash>)> {
            self.relevant.lock().unwrap().clone()
        }
    }

    fn script(n: u8) -> ScriptBuf {
        let mut bytes = vec![0x00, 0x14];
        bytes.extend_from_slice(&[n; 20]);
        ScriptBuf::from_bytes(bytes)
    }

    fn spend(from: OutPoint, to: &ScriptBuf, sats: u64) -> Transaction {
        Transaction {
            version: TxVersion::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: from,
                script_sig: ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(sats),
                script_pubkey: to.clone(),
            }],
        }
    }

    /// A transaction that spends an outpoint of no one in particular.
    fn fresh(seed: u32, to: &ScriptBuf, sats: u64) -> Transaction {
        spend(
            OutPoint {
                txid: Txid::from_byte_array([seed as u8; 32]),
                vout: seed,
            },
            to,
            sats,
        )
    }

    fn mined(prev: BlockHash, salt: u32, txs: Vec<Transaction>) -> Block {
        let mut coinbase = spend(OutPoint::null(), &script(0), 50);
        coinbase.input[0].script_sig = ScriptBuf::from_bytes(salt.to_le_bytes().to_vec());
        let mut txdata = vec![coinbase];
        txdata.extend(txs);
        let mut block = Block {
            header: Header {
                version: Version::ONE,
                prev_blockhash: prev,
                merkle_root: bitcoin::TxMerkleNode::all_zeros(),
                time: 1_700_000_000 + salt,
                bits: CompactTarget::from_consensus(0x207f_ffff),
                nonce: 0,
            },
            txdata,
        };
        block.header.merkle_root = block.compute_merkle_root().unwrap();
        while block.header.validate_pow(block.header.target()).is_err() {
            block.header.nonce += 1;
        }
        block
    }

    fn bytes(block: &Block) -> Vec<u8> {
        bitcoin::consensus::serialize(block)
    }

    struct Rig {
        rec: Arc<Rec>,
        queue: Arc<FilterQueue>,
        follower: Follower,
    }

    fn rig(floor: u32) -> Rig {
        let rec = Arc::new(Rec::default());
        let queue = Arc::new(FilterQueue::default());
        let follower = Follower::new(
            vec![Arc::clone(&rec) as Confirmable],
            Arc::clone(&queue),
            floor,
        );
        Rig {
            rec,
            queue,
            follower,
        }
    }

    fn short(h: BlockHash) -> String {
        h.to_string()[..8].to_string()
    }

    /// The chain a test runs on: `n` blocks above a base, the first with `txs`.
    fn chain(n: u32, txs: Vec<Transaction>, salt: u32) -> Vec<Block> {
        let mut out: Vec<Block> = Vec::new();
        for i in 0..n {
            let prev = out
                .last()
                .map_or(BlockHash::all_zeros(), |b: &Block| b.block_hash());
            let t = if i == 0 { txs.clone() } else { Vec::new() };
            out.push(mined(prev, salt + i, t));
        }
        out
    }

    fn join(f: &mut Follower, blocks: &[Block], from: u32) -> Vec<Effect> {
        let joined: Vec<(u32, BlockHash)> = blocks
            .iter()
            .enumerate()
            .map(|(i, b)| (from + i as u32, b.block_hash()))
            .collect();
        let mut effects = f.on_chain(&joined, &[]);
        effects.extend(
            f.on_headers(
                blocks
                    .iter()
                    .enumerate()
                    .map(|(i, b)| (from + i as u32, b.header))
                    .collect(),
            ),
        );
        effects
    }

    #[test]
    fn sl_26_a_funding_transaction_is_given_with_its_block_and_then_the_best_block() {
        let mut r = rig(0);
        let funding = fresh(1, &script(7), 100_000);
        let fid = funding.compute_txid();
        r.queue.register_tx(&fid, &script(7));
        assert_eq!(r.follower.begin(), vec![script(7)]);
        let blocks = chain(3, vec![funding], 10);
        join(&mut r.follower, &blocks, 1);
        // The best block waits until every block up to it is checked.
        assert!(r.rec.calls().is_empty());
        r.follower
            .on_block(blocks[0].block_hash(), bytes(&blocks[0]));
        r.follower.on_checked(3);
        assert_eq!(
            r.rec.calls(),
            vec![
                format!(
                    "confirmed [1:{}] at 1 in {}",
                    &fid.to_string()[..8],
                    short(blocks[0].block_hash())
                ),
                format!("best 3 {}", short(blocks[2].block_hash())),
            ]
        );
        assert_eq!(r.follower.tip_given(), Some((3, blocks[2].block_hash())));
    }

    #[test]
    fn sl_26_the_best_block_waits_for_its_header() {
        let mut r = rig(0);
        r.follower.begin();
        let blocks = chain(2, vec![], 20);
        let joined: Vec<(u32, BlockHash)> = blocks
            .iter()
            .enumerate()
            .map(|(i, b)| (1 + i as u32, b.block_hash()))
            .collect();
        r.follower.on_chain(&joined, &[]);
        r.follower.on_checked(2);
        assert!(r.rec.calls().is_empty());
        r.follower.on_headers(vec![(2, blocks[1].header)]);
        assert_eq!(
            r.rec.calls(),
            vec![format!("best 2 {}", short(blocks[1].block_hash()))]
        );
    }

    #[test]
    fn sl_27_a_registered_output_is_watched_and_its_spend_is_given() {
        let mut r = rig(0);
        let funding = fresh(2, &script(8), 100_000);
        let out = OutPoint {
            txid: funding.compute_txid(),
            vout: 0,
        };
        r.queue.register_output(WatchedOutput {
            block_hash: None,
            outpoint: lightning::chain::transaction::OutPoint {
                txid: out.txid,
                index: 0,
            },
            script_pubkey: script(8),
        });
        assert_eq!(r.follower.begin(), vec![script(8)]);
        let closing = spend(out, &script(9), 90_000);
        let cid = closing.compute_txid();
        let blocks = chain(2, vec![closing, fresh(3, &script(9), 1)], 30);
        join(&mut r.follower, &blocks, 1);
        r.follower
            .on_block(blocks[0].block_hash(), bytes(&blocks[0]));
        r.follower.on_checked(2);
        // Only the spend is given, not the other transaction of the block.
        assert_eq!(
            r.rec.calls()[0],
            format!(
                "confirmed [1:{}] at 1 in {}",
                &cid.to_string()[..8],
                short(blocks[0].block_hash())
            )
        );
    }

    #[test]
    fn sl_27_an_output_registered_while_a_block_is_given_is_looked_for_in_that_block() {
        let mut r = rig(0);
        let funding = fresh(4, &script(5), 100_000);
        let fid = funding.compute_txid();
        let out = OutPoint { txid: fid, vout: 0 };
        // The spend of the funding output is in the same block.
        let closing = spend(out, &script(6), 90_000);
        let cid = closing.compute_txid();
        // The second output of the closing transaction is spent in the block too.
        let sweep = spend(OutPoint { txid: cid, vout: 0 }, &script(10), 80_000);
        let sid = sweep.compute_txid();
        r.queue.register_tx(&fid, &script(5));
        *r.rec.on_confirm.lock().unwrap() = Some((
            Arc::clone(&r.queue),
            WatchedOutput {
                block_hash: None,
                outpoint: lightning::chain::transaction::OutPoint {
                    txid: fid,
                    index: 0,
                },
                script_pubkey: script(5),
            },
        ));
        r.follower.begin();
        let blocks = chain(1, vec![funding, closing, sweep], 40);
        join(&mut r.follower, &blocks, 1);
        r.follower
            .on_block(blocks[0].block_hash(), bytes(&blocks[0]));
        r.follower.on_checked(1);
        let calls = r.rec.calls();
        // The funding transaction first, then the spend the first delivery
        // made the follower look for; the sweep is not watched.
        assert!(calls[0].contains(&fid.to_string()[..8]), "{calls:?}");
        assert!(calls[1].contains(&cid.to_string()[..8]), "{calls:?}");
        assert!(!calls.iter().any(|c| c.contains(&sid.to_string()[..8])));
        assert_eq!(calls.len(), 3, "{calls:?}"); // and the best block
    }

    #[test]
    fn sl_27_a_script_registered_after_the_start_is_watched_and_the_driver_looks_again() {
        let mut r = rig(0);
        r.follower.begin();
        let blocks = chain(3, vec![], 50);
        join(&mut r.follower, &blocks, 1);
        r.follower.on_checked(3);
        assert_eq!(r.follower.tip_given().map(|t| t.0), Some(3));
        r.queue
            .register_tx(&Txid::from_byte_array([9; 32]), &script(3));
        let effects = r.follower.on_headers(Vec::new());
        assert_eq!(
            effects,
            vec![Effect::Watch(script(3)), Effect::Rescan(2)],
            "from one below the tip the library had"
        );
        // The same script again: nothing more to watch or to look for.
        r.queue
            .register_tx(&Txid::from_byte_array([10; 32]), &script(3));
        assert!(r.follower.on_headers(Vec::new()).is_empty());
    }

    #[test]
    fn sl_27_an_output_whose_block_was_passed_makes_the_driver_look_again_from_there() {
        let mut r = rig(0);
        r.follower.begin();
        let blocks = chain(5, vec![], 60);
        join(&mut r.follower, &blocks, 1);
        r.follower.on_checked(5);
        r.queue.register_output(WatchedOutput {
            block_hash: Some(blocks[1].block_hash()),
            outpoint: lightning::chain::transaction::OutPoint {
                txid: Txid::from_byte_array([1; 32]),
                index: 0,
            },
            script_pubkey: script(2),
        });
        // Block 2 of the chain is at height 2: the check starts below it.
        assert_eq!(
            r.follower.on_headers(Vec::new()),
            vec![Effect::Watch(script(2)), Effect::Rescan(1)]
        );
    }

    #[test]
    fn sl_26_a_reorganization_unconfirms_what_was_in_a_block_that_left_and_reconfirms_it() {
        let mut r = rig(0);
        let funding = fresh(5, &script(7), 100_000);
        let fid = funding.compute_txid();
        r.queue.register_tx(&fid, &script(7));
        r.follower.begin();
        let old = chain(3, vec![funding.clone()], 70);
        join(&mut r.follower, &old, 1);
        r.follower.on_checked(3);
        r.follower.on_block(old[0].block_hash(), bytes(&old[0]));
        r.rec
            .relevant
            .lock()
            .unwrap()
            .push((fid, 1, Some(old[0].block_hash())));
        r.rec.calls.lock().unwrap().clear();

        // Three blocks of another branch from the same base, with the funding
        // transaction in the second.
        let new_first = mined(BlockHash::all_zeros(), 700, vec![]);
        let new_second = mined(new_first.block_hash(), 701, vec![funding]);
        let new_third = mined(new_second.block_hash(), 702, vec![]);
        let new = [new_first, new_second, new_third];
        let left: Vec<BlockHash> = old.iter().rev().map(Block::block_hash).collect();
        let joined: Vec<(u32, BlockHash)> = new
            .iter()
            .enumerate()
            .map(|(i, b)| (1 + i as u32, b.block_hash()))
            .collect();
        r.follower.on_chain(&joined, &left);
        assert_eq!(
            r.rec.calls(),
            vec![format!("unconfirmed {}", &fid.to_string()[..8])]
        );
        // The header of the new tip comes, then the block with the funding
        // transaction, now at height 2.
        r.follower.on_headers(
            new.iter()
                .enumerate()
                .map(|(i, b)| (1 + i as u32, b.header))
                .collect(),
        );
        r.follower.on_block(new[1].block_hash(), bytes(&new[1]));
        let calls = r.rec.calls();
        assert_eq!(calls[0], format!("unconfirmed {}", &fid.to_string()[..8]));
        assert_eq!(
            calls[1],
            format!("best 3 {}", short(new[2].block_hash())),
            "{calls:?}"
        );
        assert_eq!(
            calls[2],
            format!(
                "confirmed [1:{}] at 2 in {}",
                &fid.to_string()[..8],
                short(new[1].block_hash())
            )
        );
        assert_eq!(calls.len(), 3);
    }

    #[test]
    fn sl_26_a_transaction_in_a_block_still_on_the_chain_is_not_unconfirmed() {
        let mut r = rig(0);
        r.follower.begin();
        let old = chain(4, vec![], 80);
        join(&mut r.follower, &old, 1);
        r.follower.on_checked(4);
        // Confirmed at height 1; the blocks above it leave.
        r.rec.relevant.lock().unwrap().push((
            Txid::from_byte_array([4; 32]),
            1,
            Some(old[0].block_hash()),
        ));
        r.rec.calls.lock().unwrap().clear();
        let new = mined(old[2].block_hash(), 800, vec![]);
        r.follower
            .on_chain(&[(4, new.block_hash())], &[old[3].block_hash()]);
        assert!(!r.rec.calls().iter().any(|c| c.starts_with("unconfirmed")));
    }

    #[test]
    fn sl_26_a_transaction_with_no_block_named_is_unconfirmed_from_the_fork_up() {
        let mut r = rig(0);
        r.follower.begin();
        let old = chain(4, vec![], 90);
        join(&mut r.follower, &old, 1);
        r.follower.on_checked(4);
        let above = Txid::from_byte_array([5; 32]);
        let below = Txid::from_byte_array([6; 32]);
        *r.rec.relevant.lock().unwrap() = vec![(above, 4, None), (below, 2, None)];
        let new = mined(old[2].block_hash(), 900, vec![]);
        r.follower
            .on_chain(&[(4, new.block_hash())], &[old[3].block_hash()]);
        let calls = r.rec.calls();
        assert!(calls.contains(&format!("unconfirmed {}", &above.to_string()[..8])));
        assert!(!calls.contains(&format!("unconfirmed {}", &below.to_string()[..8])));
    }

    #[test]
    fn sl_26_the_start_checks_what_the_library_holds_against_the_stored_chain() {
        let mut r = rig(0);
        let stored = chain(3, vec![], 100);
        let other = mined(BlockHash::all_zeros(), 999, vec![]);
        r.follower.seed(
            stored
                .iter()
                .enumerate()
                .map(|(i, b)| (1 + i as u32, b.header))
                .collect(),
        );
        // The library has a transaction confirmed at height 2 in a block that
        // is not the stored chain's, and one in the stored chain's.
        let stale = Txid::from_byte_array([7; 32]);
        let fine = Txid::from_byte_array([8; 32]);
        *r.rec.relevant.lock().unwrap() = vec![
            (stale, 2, Some(other.block_hash())),
            (fine, 3, Some(stored[2].block_hash())),
        ];
        r.follower.begin();
        let calls = r.rec.calls();
        assert!(calls.contains(&format!("unconfirmed {}", &stale.to_string()[..8])));
        assert!(!calls.contains(&format!("unconfirmed {}", &fine.to_string()[..8])));
    }

    #[test]
    fn sl_26_a_block_is_given_once_and_again_after_it_left_and_came_back() {
        let mut r = rig(0);
        let funding = fresh(6, &script(7), 100_000);
        let fid = funding.compute_txid();
        r.queue.register_tx(&fid, &script(7));
        r.follower.begin();
        let blocks = chain(2, vec![funding], 110);
        join(&mut r.follower, &blocks, 1);
        r.follower.on_checked(2);
        r.follower
            .on_block(blocks[0].block_hash(), bytes(&blocks[0]));
        r.follower
            .on_block(blocks[0].block_hash(), bytes(&blocks[0]));
        let confirmed = |r: &Rig| {
            r.rec
                .calls()
                .iter()
                .filter(|c| c.starts_with("confirmed"))
                .count()
        };
        assert_eq!(confirmed(&r), 1);
        // It leaves and the same block returns.
        let left: Vec<BlockHash> = blocks.iter().rev().map(Block::block_hash).collect();
        r.follower.on_chain(&[], &left);
        join(&mut r.follower, &blocks, 1);
        r.follower
            .on_block(blocks[0].block_hash(), bytes(&blocks[0]));
        assert_eq!(confirmed(&r), 2);
    }

    #[test]
    fn sl_26_a_block_the_chain_does_not_name_yet_waits() {
        let mut r = rig(0);
        let funding = fresh(7, &script(7), 100_000);
        let fid = funding.compute_txid();
        r.queue.register_tx(&fid, &script(7));
        r.follower.begin();
        let blocks = chain(2, vec![funding], 120);
        r.follower
            .on_block(blocks[0].block_hash(), bytes(&blocks[0]));
        assert!(r.rec.calls().is_empty());
        join(&mut r.follower, &blocks, 1);
        assert!(r.rec.calls().iter().any(|c| c.starts_with("confirmed")));
    }

    #[test]
    fn sl_26_a_block_that_is_not_the_one_named_is_not_given() {
        let mut r = rig(0);
        let funding = fresh(8, &script(7), 100_000);
        r.queue.register_tx(&funding.compute_txid(), &script(7));
        r.follower.begin();
        let blocks = chain(2, vec![funding], 130);
        join(&mut r.follower, &blocks, 1);
        let effects = r
            .follower
            .on_block(blocks[1].block_hash(), bytes(&blocks[0]));
        assert!(matches!(effects.as_slice(), [Effect::Note(_)]));
        assert!(r.rec.calls().is_empty());
    }

    #[test]
    fn sl_26_a_transaction_sent_through_the_peers_is_forgotten_once_it_is_in_a_block() {
        let mut r = rig(0);
        let sweep = fresh(9, &script(7), 100_000);
        let sid = sweep.compute_txid();
        r.follower.begin();
        r.follower.note_broadcast(sid);
        let blocks = chain(2, vec![sweep], 140);
        join(&mut r.follower, &blocks, 1);
        let effects = r
            .follower
            .on_block(blocks[0].block_hash(), bytes(&blocks[0]));
        assert!(effects.contains(&Effect::Forget(sid)));
    }

    #[test]
    fn sl_26_the_follower_keeps_two_thousand_blocks_behind_the_tip() {
        let mut r = rig(0);
        r.follower.begin();
        let mut blocks: Vec<Block> = Vec::new();
        for i in 0..(KEEP_BEHIND + 50) {
            let prev = blocks
                .last()
                .map_or(BlockHash::all_zeros(), Block::block_hash);
            blocks.push(mined(prev, 1000 + i, vec![]));
        }
        join(&mut r.follower, &blocks, 1);
        r.follower.on_checked(KEEP_BEHIND + 50);
        assert!(r.follower.chain.len() <= KEEP_BEHIND as usize + 1);
        assert!(r.follower.headers.len() <= KEEP_BEHIND as usize + 1);
        assert_eq!(
            r.follower.chain.keys().next_back(),
            Some(&(KEEP_BEHIND + 50))
        );
    }

    #[test]
    fn sl_26_the_header_store_gives_its_best_chain_from_a_height() {
        let mut store = HeaderStore::new(Network::Regtest).unwrap();
        let (_, genesis) = store.tip();
        let mut prev = genesis;
        let mut time = 1_296_688_602u32;
        let mut headers = Vec::new();
        for _ in 0..3 {
            time += 600;
            let mut header = Header {
                version: Version::ONE,
                prev_blockhash: prev,
                merkle_root: bitcoin::TxMerkleNode::all_zeros(),
                time,
                bits: CompactTarget::from_consensus(0x207f_ffff),
                nonce: 0,
            };
            while header.validate_pow(header.target()).is_err() {
                header.nonce += 1;
            }
            store.accept(&header, u64::from(time)).unwrap();
            prev = header.block_hash();
            headers.push(header);
        }
        let all = window(Network::Regtest, &store.to_bytes(), 0).unwrap();
        assert_eq!(
            all.iter().map(|(h, _)| *h).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        let from_two = window(Network::Regtest, &store.to_bytes(), 2).unwrap();
        assert_eq!(from_two.len(), 2);
        assert_eq!(from_two[0].1, headers[1]);
        // A file that is not the store's is refused.
        assert!(window(Network::Regtest, b"nope", 0).is_err());
        assert!(window(Network::Signet, &store.to_bytes(), 0).is_err());
    }

    #[test]
    fn sl_26_the_header_tap_keeps_the_latest_file_the_driver_saved() {
        let dir = crate::testutil::temp_dir("tap");
        let tap = Arc::new(HeaderTap::default());
        let mut storage = TapStorage {
            inner: FileStorage::new(&dir).unwrap(),
            tap: Arc::clone(&tap),
        };
        storage.save(Key::Book, b"book").unwrap();
        assert_eq!(tap.take(), None, "only the headers are copied");
        storage.save(Key::Headers, b"one").unwrap();
        storage.save(Key::Headers, b"two").unwrap();
        assert_eq!(tap.take(), Some(b"two".to_vec()));
        assert_eq!(tap.take(), None);
        assert_eq!(storage.load(Key::Headers).unwrap(), Some(b"two".to_vec()));
    }
}
