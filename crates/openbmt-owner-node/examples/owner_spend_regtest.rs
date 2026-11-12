//! The owner node against Bitcoin nodes on regtest, as a command that reads
//! simple lines on its standard input and prints one line for each thing it
//! does. `scripts/owner-spend-e2e-core.sh` drives it.
//!
//! ```text
//! owner_spend_regtest <data dir> <peer> [<peer>...] [--birthday <height>]
//!     [--without-tor] [--proxy <address>] [--refund-depth <n>]
//!     [--live-bound <n>] [--quiet-secs <n>] [--deep-secs <n>]
//! ```
//!
//! The last three shrink the live watch's numbers (OC-22 to OC-24) so that a
//! check against a real node can see them at work; the defaults are the
//! constants of the rules.
//!
//! The peers are fixed local nodes. Each is on its own loopback address, so
//! that the driver counts them as different groups (NR-87). The owner node
//! keeps its coin view and its queue in the data directory
//! (`docs/OWNER-COINS-RULES.md`). The Key app, key 1 and key 2 are in this
//! process, and their states are kept in the data directory too, so that a
//! restart does not forget which indexes were signed.
//!
//! **Test only, never use.** The identities and the wallet keys are made
//! from fixed bytes that anyone can read here.
//!
//! Commands, one per line:
//!
//! - `receive <i>`: the receive address at index `i`
//! - `batch <start> <count>`: queue a batch of receive addresses (the Key
//!   app has no device list here and declines it; the queue counts it as
//!   handed out all the same)
//! - `refund <sale> <sats> <address>`: queue a refund
//! - `spend <rate>`: build a session at `rate` satoshis per 1000 virtual
//!   bytes, have the Key app approve it, take the response
//! - `line`: the line about receipts (OC-16 to OC-18)
//! - `offer`: the offer to merge, when there is one (OC-19)
//! - `fees`: the fee history: samples, rated samples, recent and usual rate
//! - `merge`: approve the offer (OC-20), have the Key app approve the session
//!   and take the response, as `spend` does
//! - `conflict <sale> <sats> <address> <rate>`: print a second transaction
//!   that spends the same coins as the next `spend` (the node is unchanged)
//! - `clock <seconds>`: move the node's clock forward, which ages the
//!   addresses it has issued (test only)
//! - `deepcheck`: ask for a deep check of the old addresses (OC-24)
//! - `old`: the line about old addresses (OC-25)
//! - `coins`, `states`, `relay <txid>`, `status`, `rescan`, `pump <seconds>`,
//!   `quit`
//!
//! Lines printed:
//!
//! - `descriptor <text>`, `started checked=<h> tip=<h> view=<text>`
//! - `receive <i> <address>`
//! - `coin <txid>:<vout> sats=<n> confs=<n>`, and `coins n=<n> balance=<n>
//!   view=<text>`
//! - `send <txid> <state> <text>` for each transaction followed, `entry
//!   <id> <stage>` for each refund, and `queue outbox=<n> sessions=<n>`
//! - `relay <txid> asked=<n> fetched=<n>`
//! - `line payments=<n> cost=<sats> rate=<per kvb>` or `line <state>`, `offer
//!   coins=<n> of=<n> rate=<n> cost=<sats> usual=<sats>` or `offer none`, and
//!   `fees samples=<n> rated=<n> recent=<n> usual=<n> low=<bool>`
//! - `spent txid=<txid>`, `refused <text>`
//! - `status tip=<h> checked=<h> view=<text>`
//! - `clock offset=<seconds>`, `watch live=<n> aside=<n>`
//! - `old none`, or `old <state> count=<n> next=<time> days=<n>` where the
//!   state is `first`, `monthly`, `checking` or `failed`
//! - `note <text>`, `stopped`
//!
//! Every command ends with a line `done <n>`.

use std::fs;
use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use bitcoin::{Address, Network, Txid};
use openbmt_approver::{Approver, Choice};
use openbmt_core::batch::{BatchBuilder, BatchPolicy};
use openbmt_core::identity::Identity;
use openbmt_core::wallet::{KeySlot, WalletDescription};
use openbmt_net_io::{Clock, DriverConfig};
use openbmt_owner::{Built, Delivery, Owner, SaleFacts, SaleKey};
use openbmt_owner_node::watcher::write_file;
use openbmt_owner_node::{
    DeepLine, OwnerConfig, OwnerNode, OwnerParts, ReceiptsLine, SendState, WatchLimits,
};
use openbmt_signer::SignerState;
use openbmt_watch::HeaderChain;

const USAGE: &str = "usage: owner_spend_regtest <data dir> <peer> [<peer>...] \
                     [--birthday <height>] [--without-tor] [--proxy <address>] \
                     [--refund-depth <n>] [--live-bound <n>] [--quiet-secs <n>] \
                     [--deep-secs <n>]";

/// TEST ONLY, NEVER USE: the secret bytes of the identities.
const OWNER_SECRET: [u8; 32] = [0x41; 32];
const APPROVER_SECRET: [u8; 32] = [0x42; 32];
const COUNTER_SECRET: [u8; 32] = [0x43; 32];
/// TEST ONLY, NEVER USE: the seeds of the three wallet keys.
const KEY_SEEDS: [[u8; 32]; 3] = [[0xb1; 32], [0xb2; 32], [0xb3; 32]];

const KEY1_FILE: &str = "key1.bin";
const APPROVER_FILE: &str = "approver.bin";

fn say(line: impl AsRef<str>) {
    println!("{}", line.as_ref());
    let _ = std::io::stdout().flush();
}

fn identity(secret: &[u8; 32]) -> Result<Identity, String> {
    Identity::from_secret_bytes(secret).map_err(|e| format!("identity: {e}"))
}

/// One of the three test keys, on the wallet's account path for regtest.
fn account(seed: &[u8; 32]) -> Result<(Xpriv, KeySlot), String> {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, seed).map_err(|e| e.to_string())?;
    let path = DerivationPath::from_str("m/48h/1h/0h/2h").map_err(|e| e.to_string())?;
    let account = master
        .derive_priv(&secp, &path)
        .map_err(|e| e.to_string())?;
    let slot = KeySlot {
        index: 0,
        origin_fingerprint: master.fingerprint(&secp),
        origin_path: path,
        xpub: Xpub::from_priv(&secp, &account),
    };
    Ok((account, slot))
}

struct Keys {
    wallet: WalletDescription,
    account1: Xpriv,
    key1: SignerState,
    approver: Approver,
}

fn keys(dir: &Path) -> Result<Keys, String> {
    let secp = Secp256k1::new();
    let mut accounts = Vec::new();
    let mut slots = Vec::new();
    for (n, seed) in KEY_SEEDS.iter().enumerate() {
        let (account, mut slot) = account(seed)?;
        slot.index = u8::try_from(n + 1).map_err(|e| e.to_string())?;
        accounts.push(account);
        slots.push(slot);
    }
    let slots: [KeySlot; 3] = slots.try_into().map_err(|_| "three slots".to_string())?;
    let wallet = WalletDescription::new(Network::Regtest, slots).map_err(|e| e.to_string())?;
    let xpubs: Vec<Xpub> = accounts.iter().map(|a| Xpub::from_priv(&secp, a)).collect();
    let (Some(a1), Some(a2), Some(x1), Some(x2), Some(x3)) = (
        accounts.first().copied(),
        accounts.get(1).copied(),
        xpubs.first(),
        xpubs.get(1),
        xpubs.get(2),
    ) else {
        return Err("three keys".to_string());
    };
    let owner = identity(&OWNER_SECRET)?.device_id();
    let policy = BatchPolicy { min_count: 3 };

    let key1 = match fs::read(dir.join(KEY1_FILE)) {
        Ok(bytes) => SignerState::from_bytes(&bytes, Network::Regtest, x1)
            .map_err(|e| format!("{KEY1_FILE}: {e}"))?,
        Err(_) => SignerState::register(wallet.canonical_string(), Network::Regtest, x1, Some(x3))
            .map_err(|e| e.to_string())?,
    };
    let approver = match fs::read(dir.join(APPROVER_FILE)) {
        Ok(bytes) => Approver::restore(
            &APPROVER_SECRET,
            wallet.clone(),
            owner,
            policy,
            Some(a2),
            &bytes,
        )
        .map_err(|e| format!("{APPROVER_FILE}: {e}"))?,
        Err(_) => {
            let mut approver = Approver::new(&APPROVER_SECRET, wallet.clone(), owner, policy)
                .map_err(|e| e.to_string())?;
            let key2 =
                SignerState::register(wallet.canonical_string(), Network::Regtest, x2, Some(x3))
                    .map_err(|e| e.to_string())?;
            approver
                .attach_signer(key2, a2)
                .map_err(|e| e.to_string())?;
            approver
        }
    };
    Ok(Keys {
        wallet,
        account1: a1,
        key1,
        approver,
    })
}

fn number<T: FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    word.ok_or_else(|| format!("{what} is missing"))?
        .parse()
        .map_err(|_| format!("{what} is not a number"))
}

/// Seconds that the `clock` command has added to the node's time.
static CLOCK_OFFSET: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
        .saturating_add(CLOCK_OFFSET.load(Ordering::SeqCst))
}

/// The node's clock: the system's, moved forward by the `clock` command.
struct OffsetClock;

impl Clock for OffsetClock {
    fn now(&self) -> u64 {
        now_secs()
    }
}

fn sale(index: u32) -> Result<SaleKey, String> {
    Ok(SaleKey {
        counter: identity(&COUNTER_SECRET)?.device_id(),
        index,
    })
}

fn tag(state: SendState) -> String {
    match state {
        SendState::Waiting => "waiting".to_string(),
        SendState::Sent => "sent".to_string(),
        SendState::SeenBack { groups } => format!("seenback-{groups}"),
        SendState::NotSeen => "notseen".to_string(),
        SendState::HeldByNode { groups } => format!("heldbynode-{groups}"),
        SendState::Confirmed { depth } => format!("confirmed-{depth}"),
        SendState::Done => "done".to_string(),
        SendState::Replaced => "replaced".to_string(),
        SendState::Unresolved => "unresolved".to_string(),
    }
}

fn notes(node: &mut OwnerNode) {
    for note in node.watcher_mut().take_notes() {
        say(format!("note {note}"));
    }
}

/// Have the Key app (key 2) approve a built session, take its response with
/// key 1 and print what came out. `before` is the outbox length before the
/// build.
fn sign_and_take(
    node: &mut OwnerNode,
    keys: &mut Keys,
    dir: &Path,
    built: &Built,
    before: usize,
) -> Result<(), String> {
    let response = {
        let mut session = keys
            .approver
            .open(&built.request, now_secs())
            .map_err(|e| e.to_string())?;
        let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
        for item in items {
            let Some(l) = session.lines().into_iter().find(|l| l.item == item) else {
                continue;
            };
            let choice = if l.can_approve {
                Choice::Approve
            } else {
                Choice::Decline("not here".to_string())
            };
            session.decide(item, choice).map_err(|e| e.to_string())?;
        }
        session
            .finish(|b| write_file(dir, APPROVER_FILE, b))
            .map_err(|e| e.to_string())?
    };
    let account1 = keys.account1;
    let persist = |b: &[u8]| write_file(dir, KEY1_FILE, b);
    match node.take(&response, &mut keys.key1, &account1, now_secs(), persist) {
        Ok(taken) => {
            for (item, outcome) in &taken.outcomes {
                say(format!("outcome {item} {outcome:?}"));
            }
            let new: Vec<Txid> = node
                .owner()
                .outbox()
                .iter()
                .filter_map(|d| match &d.what {
                    Delivery::Transaction { txid, .. } => Some(*txid),
                    _ => None,
                })
                .skip(before)
                .collect();
            for txid in new {
                say(format!("spent txid={txid}"));
            }
        }
        Err(e) => say(format!("refused {e}")),
    }
    Ok(())
}

/// Run one command. `Ok(false)` is `quit`.
fn command(node: &mut OwnerNode, keys: &mut Keys, dir: &Path, line: &str) -> Result<bool, String> {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("receive") => {
            let i: u32 = number(words.next(), "the index")?;
            let script = keys.wallet.receive_script(i).map_err(|e| e.to_string())?;
            let address =
                Address::from_script(&script, Network::Regtest).map_err(|e| e.to_string())?;
            say(format!("receive {i} {address}"));
        }
        Some("batch") => {
            let start: u32 = number(words.next(), "the start")?;
            let count: u32 = number(words.next(), "the count")?;
            let counter = identity(&COUNTER_SECRET)?.device_id();
            let batch = BatchBuilder::new(BatchPolicy { min_count: 3 })
                .derive(&keys.wallet, counter, start, count)
                .map_err(|e| e.to_string())?;
            let write = node.queue_writer();
            node.owner_mut()
                .map_err(|e| e.to_string())?
                .queue_batch(batch, now_secs(), &write)
                .map_err(|e| e.to_string())?;
            node.refresh_marks();
        }
        Some("refund") => {
            let index: u32 = number(words.next(), "the sale")?;
            let sats: u64 = number(words.next(), "the amount")?;
            let to = words.next().ok_or("the address is missing")?;
            let script = Address::from_str(to)
                .map_err(|e| e.to_string())?
                .require_network(Network::Regtest)
                .map_err(|e| e.to_string())?
                .script_pubkey();
            let write = node.queue_writer();
            let facts = SaleFacts {
                received_sats: sats.saturating_mul(2),
                reached_refund_depth: true,
            };
            let key = sale(index)?;
            node.owner_mut()
                .map_err(|e| e.to_string())?
                .set_sale_facts(key, facts, &write)
                .map_err(|e| e.to_string())?;
            let id = node
                .owner_mut()
                .map_err(|e| e.to_string())?
                .queue_refund(key, "Mia", script, sats, None, &write)
                .map_err(|e| e.to_string())?;
            say(format!("queued refund id={id}"));
        }
        Some("spend") => {
            let rate: u64 = number(words.next(), "the rate")?;
            let before = node.owner().outbox().len();
            let built = match node.build_request(rate, now_secs()) {
                Ok(b) => b,
                Err(e) => {
                    say(format!("refused {e}"));
                    return Ok(true);
                }
            };
            for skipped in &built.skipped {
                say(format!("skipped {} {}", skipped.id, skipped.reason));
            }
            sign_and_take(node, keys, dir, &built, before)?;
        }
        Some("line") => match node.receipts_line() {
            ReceiptsLine::Known {
                payments,
                cost_sats,
                rate_per_kvb,
            } => say(format!(
                "line payments={payments} cost={cost_sats} rate={rate_per_kvb}"
            )),
            other => say(format!("line {other:?}")),
        },
        Some("offer") => match node.merge_offer() {
            Some(o) => say(format!(
                "offer coins={} of={} rate={} cost={} usual={}",
                o.coins, o.of, o.rate_per_kvb, o.cost_sats, o.usual_cost_sats
            )),
            None => say("offer none"),
        },
        Some("fees") => {
            let tip = node.watcher().chain().tip_height();
            let h = node.fee_history();
            let rated = h
                .samples()
                .iter()
                .filter(|s| s.rate_milli.is_some())
                .count();
            say(format!(
                "fees samples={} rated={rated} recent={:?} usual={:?} low={:?}",
                h.samples().len(),
                h.recent_rate(tip),
                h.usual_rate(tip),
                h.is_low(tip)
            ));
        }
        Some("merge") => {
            let before = node.owner().outbox().len();
            match node.approve_merge(now_secs()) {
                Ok(built) => {
                    say(format!("merge built skipped={}", built.skipped.len()));
                    sign_and_take(node, keys, dir, &built, before)?;
                }
                Err(e) => say(format!("refused {e}")),
            }
        }
        Some("conflict") => {
            // A second transaction that spends the same coins as the next
            // `spend`, made on a copy of the queue, so that the script can
            // have a block spend them first. Nothing here changes the node.
            let index: u32 = number(words.next(), "the sale")?;
            let sats: u64 = number(words.next(), "the amount")?;
            let to = words.next().ok_or("the address is missing")?;
            let rate: u64 = number(words.next(), "the rate")?;
            let script = Address::from_str(to)
                .map_err(|e| e.to_string())?
                .require_network(Network::Regtest)
                .map_err(|e| e.to_string())?
                .script_pubkey();
            let snapshot = node.owner().snapshot().map_err(|e| e.to_string())?;
            let mut copy = Owner::restore(
                &OWNER_SECRET,
                keys.wallet.clone(),
                identity(&APPROVER_SECRET)?.device_id(),
                BatchPolicy { min_count: 3 },
                &snapshot,
            )
            .map_err(|e| e.to_string())?;
            let nop = |_: &[u8]| Ok::<(), String>(());
            let key = sale(index)?;
            let facts = SaleFacts {
                received_sats: sats.saturating_mul(2),
                reached_refund_depth: true,
            };
            copy.set_sale_facts(key, facts, nop)
                .map_err(|e| e.to_string())?;
            copy.queue_refund(key, "Mia", script, sats, None, nop)
                .map_err(|e| e.to_string())?;
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos());
            let mut random = [0u8; 96];
            for (i, b) in random.iter_mut().enumerate() {
                let n = nanos.to_le_bytes();
                *b = n.get(i % 16).copied().unwrap_or(0) ^ (i as u8).wrapping_mul(31);
            }
            let tip = node.watcher().chain().tip_height();
            let built = copy
                .build_request(
                    &node.coins(),
                    rate,
                    node.view().change_next(),
                    tip,
                    now_secs(),
                    &random,
                    nop,
                )
                .map_err(|e| e.to_string())?;
            let response = {
                let mut session = keys
                    .approver
                    .open(&built.request, now_secs())
                    .map_err(|e| e.to_string())?;
                let items: Vec<u32> = session.lines().iter().map(|l| l.item).collect();
                for item in items {
                    let Some(l) = session.lines().into_iter().find(|l| l.item == item) else {
                        continue;
                    };
                    let choice = if l.can_approve {
                        Choice::Approve
                    } else {
                        Choice::Decline("not here".to_string())
                    };
                    session.decide(item, choice).map_err(|e| e.to_string())?;
                }
                session
                    .finish(|b| write_file(dir, APPROVER_FILE, b))
                    .map_err(|e| e.to_string())?
            };
            let account1 = keys.account1;
            copy.take_response(&response, &mut keys.key1, &account1, now_secs(), |_, k| {
                write_file(dir, KEY1_FILE, k)
            })
            .map_err(|e| e.to_string())?;
            for d in copy.outbox() {
                if let Delivery::Transaction { tx, .. } = &d.what {
                    let hex: String = tx.iter().map(|b| format!("{b:02x}")).collect();
                    say(format!("conflict {hex}"));
                }
            }
        }
        Some("clock") => {
            let secs: u64 = number(words.next(), "the seconds")?;
            CLOCK_OFFSET.fetch_add(secs, Ordering::SeqCst);
            say(format!(
                "clock offset={}",
                CLOCK_OFFSET.load(Ordering::SeqCst)
            ));
        }
        Some("deepcheck") => {
            let asked = node.request_deep_check().map_err(|e| e.to_string())?;
            say(format!("deepcheck asked={asked}"));
        }
        Some("old") => match node.old_addresses_line() {
            None => say("old none"),
            Some(line) => {
                let now = now_secs();
                let (state, count, next) = match line {
                    DeepLine::FirstCheckWaiting { count, next } => ("first", count, next),
                    DeepLine::Monthly { count, next, .. } => ("monthly", count, next),
                    DeepLine::Checking { count } => ("checking", count, 0),
                    DeepLine::Failed { count, next } => ("failed", count, next),
                };
                let days = match line {
                    DeepLine::Monthly { days_ago, .. } => days_ago,
                    _ => 0,
                };
                say(format!(
                    "old {state} count={count} next_in={} days={days}",
                    next.saturating_sub(now)
                ));
            }
        },
        Some("watch") => say(format!(
            "watch live={} aside={}",
            node.view().live_count(),
            node.view().released().len()
        )),
        Some("coins") => {
            let mut coins = node.coins();
            coins.sort_by_key(|c| c.outpoint);
            for c in &coins {
                let sats = c.output().map_or(0, |o| o.value.to_sat());
                say(format!(
                    "coin {} sats={sats} confs={}",
                    c.outpoint, c.confirmations
                ));
            }
            say(format!(
                "coins n={} balance={} view={}",
                coins.len(),
                node.balance(),
                node.state()
            ));
        }
        Some("states") => {
            for (txid, state) in node.send_states() {
                say(format!("send {txid} {} {state}", tag(state)));
            }
            for r in node.owner().refunds() {
                let stage = format!("{:?}", r.stage);
                let name = stage.split([' ', '{']).next().unwrap_or("");
                say(format!("entry {} {name}", r.id));
            }
            say(format!(
                "queue outbox={} sessions={}",
                node.owner().outbox().len(),
                node.owner().open_sessions().len()
            ));
        }
        Some("relay") => {
            let txid: Txid = words
                .next()
                .ok_or("the txid is missing")?
                .parse()
                .map_err(|_| "the txid is not a txid")?;
            match node.relay_counts(&txid) {
                Some((a, f)) => say(format!("relay {txid} asked={a} fetched={f}")),
                None => say(format!("relay {txid} none")),
            }
        }
        Some("status") => say(format!(
            "status tip={} checked={} view={}",
            node.watcher().chain().tip_height(),
            node.view().checked(),
            node.state()
        )),
        Some("rescan") => node.rescan().map_err(|e| e.to_string())?,
        Some("pump") => {
            let secs: f64 = number(words.next(), "the seconds")?;
            node.pump(Duration::from_secs_f64(secs.clamp(0.0, 600.0)));
        }
        Some("quit") => return Ok(false),
        Some(other) => return Err(format!("unknown command: {other}")),
        None => {}
    }
    notes(node);
    Ok(true)
}

fn run(args: &[String]) -> Result<(), String> {
    let mut birthday = 0u32;
    let mut without_tor = false;
    let mut proxy: Option<SocketAddr> = None;
    let mut refund_depth = 6u32;
    let mut limits = WatchLimits::default();
    let mut rest: Vec<&str> = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--birthday" => birthday = number(args.next().map(String::as_str), "the birthday")?,
            "--without-tor" => without_tor = true,
            "--proxy" => {
                let addr = args.next().ok_or(USAGE)?;
                proxy = Some(addr.parse().map_err(|_| format!("proxy address: {addr}"))?);
            }
            "--refund-depth" => {
                refund_depth = number(args.next().map(String::as_str), "the refund depth")?;
            }
            "--live-bound" => {
                limits.live = number(args.next().map(String::as_str), "the live bound")?;
            }
            "--quiet-secs" => {
                limits.quiet_secs = number(args.next().map(String::as_str), "the quiet time")?;
            }
            "--deep-secs" => {
                limits.deep_secs = number(args.next().map(String::as_str), "the check time")?;
            }
            other => rest.push(other),
        }
    }
    let Some((dir, peers)) = rest.split_first() else {
        return Err(USAGE.to_string());
    };
    if peers.is_empty() {
        return Err(USAGE.to_string());
    }
    let peers: Vec<SocketAddr> = peers
        .iter()
        .map(|p| p.parse().map_err(|_| format!("node address: {p}")))
        .collect::<Result<_, _>>()?;
    let dir_path = Path::new(dir);
    fs::create_dir_all(dir_path).map_err(|e| e.to_string())?;
    let mut keys = keys(dir_path)?;

    let mut driver = DriverConfig::new(Network::Regtest, 0);
    driver.allow_local = true;
    driver.fixed_peers = peers;
    let mut config = OwnerConfig::new(dir, identity(&OWNER_SECRET)?, driver);
    config.birthday = birthday;
    config.broadcast_proxy = proxy;
    config.broadcast_without_tor = without_tor;
    config.clock = Box::new(OffsetClock);
    let mut parts = OwnerParts::new(
        OWNER_SECRET,
        keys.wallet.clone(),
        identity(&APPROVER_SECRET)?.device_id(),
        BatchPolicy { min_count: 3 },
    )
    .map_err(|e| e.to_string())?;
    parts.refund_depth = refund_depth;
    parts.limits = limits;

    say(format!("descriptor {}", keys.wallet.canonical_string()));
    let mut node = OwnerNode::start(config, parts).map_err(|e| e.to_string())?;
    if node.take_recovery_pending() {
        let account1 = keys.account1;
        node.recover_key1(&mut keys.key1, &account1, |bytes| {
            write_file(dir_path, KEY1_FILE, bytes)
        })
        .map_err(|e| format!("key 1 recovery: {e}"))?;
    }
    say(format!(
        "started checked={} tip={} view={}",
        node.view().checked(),
        node.watcher().chain().tip_height(),
        node.state()
    ));
    notes(&mut node);

    let (lines, input) = channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(l) => {
                    if lines.send(l).is_err() {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    });

    let mut done = 0u64;
    loop {
        match input.try_recv() {
            Ok(line) => {
                match command(&mut node, &mut keys, dir_path, line.trim()) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => say(format!("error {e}")),
                }
                done += 1;
                say(format!("done {done}"));
            }
            Err(TryRecvError::Empty) => {
                node.pump(Duration::from_millis(100));
                notes(&mut node);
            }
            Err(TryRecvError::Disconnected) => break,
        }
    }
    node.shutdown();
    notes(&mut node);
    say("stopped");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            say(format!("failed {why}"));
            ExitCode::from(1)
        }
    }
}
