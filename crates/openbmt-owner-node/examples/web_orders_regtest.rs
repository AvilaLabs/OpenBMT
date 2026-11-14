//! The owner node of a shop with its own wallet and its web orders, against a
//! Bitcoin node on regtest, as a command that reads simple lines on its
//! standard input and prints lines for what it does.
//! `scripts/web-orders-e2e-core.sh` drives it, and plays the app's part: it
//! sends each call the node makes to the plugin's own REST callbacks and
//! hands the store's answer back.
//!
//! ```text
//! web_orders_regtest <data dir> <peer> [--birthday <height>] [--depth <n>]
//! ```
//!
//! The wallet is the one of `requests_regtest`: a single key of the key
//! maker's test words. **Test only, never use.** The identity and the wallet
//! are fixed.
//!
//! Commands, one per line:
//!
//! - `pair <url> <shop name>`: make the pairing code for a store
//! - `confirm`: the owner says the fingerprints match
//! - `step`: the node's next step of a visit, as it is due now
//! - `reply <status> <body>`: the store's answer to the call in the air
//! - `web`: the store's state in numbers
//! - `order <number>`: how an order stands
//! - `requests`: the book's requests
//! - `regtest <address>`: an address in its regtest form
//! - `tip`, `pump <seconds>`, `quit`
//!
//! Lines printed:
//!
//! - `started checked=<h> tip=<h> view=<text>`
//! - `code <text>` and `fingerprint <text>`
//! - `idle`, `busy` or a call: `call <kind>`, `url <text>`,
//!   `header <name> <value>`, `type <text>`, `body <text>`, `end`
//! - `web paired=<bool> left=<n|-> sent=<n> orders=<n> waiting=<n> told=<n> notices=<n> refused=<n>`
//! - `order <number> <stand>`, `request <number> <status> credited=<sats> <note>`
//! - `regtest <address>`, `refused <text>`, `status tip=<h> checked=<h> view=<text>`
//!
//! Every command ends with a line `done <n>`.

use std::io::{BufRead, Write};
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::str::FromStr;
use std::sync::mpsc::{channel, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bitcoin::bip32::{Fingerprint, Xpub};
use bitcoin::{Address, Network};
use openbmt_core::batch::BatchPolicy;
use openbmt_core::currency::ShopCurrency;
use openbmt_core::identity::Identity;
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::wallet_single::{ImportedWallet, SingleKeyWallet};
use openbmt_net_io::DriverConfig;
use openbmt_owner::starter::{KernelRandom, KERNEL_BYTES};
use openbmt_owner_node::{
    own_approver, OrderStand, OwnNode, OwnerConfig, OwnerNode, OwnerParts, PairState,
    RequestContext, WebStep,
};
use openbmt_requests::Status;
use openbmt_watch::HeaderChain;

const USAGE: &str =
    "usage: web_orders_regtest <data dir> <peer> [--birthday <height>] [--depth <n>]";

/// TEST ONLY, NEVER USE: the secret bytes of the owner root.
const OWNER_SECRET: [u8; 32] = [0x51; 32];
/// The key maker's table: the account key of the test seed 818bc9b6.
const FINGERPRINT: &str = "818bc9b6";
const XPUB: &str = "xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8";

fn say(line: impl AsRef<str>) {
    println!("{}", line.as_ref());
    let _ = std::io::stdout().flush();
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn identity() -> Result<Identity, String> {
    Identity::from_secret_bytes(&OWNER_SECRET).map_err(|e| format!("identity: {e}"))
}

fn pinned() -> Result<PinnedWallet, String> {
    let wallet = SingleKeyWallet::new(
        Fingerprint::from_str(FINGERPRINT).map_err(|e| e.to_string())?,
        0,
        Xpub::from_str(XPUB).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(PinnedWallet::new(ImportedWallet::Single(wallet)))
}

/// The same script as an address of another network.
fn rewrite(address: &str, to: Network) -> Result<String, String> {
    let parsed = Address::from_str(address)
        .map_err(|e| format!("address {address}: {e}"))?
        .assume_checked();
    Address::from_script(&parsed.script_pubkey(), to)
        .map(|a| a.to_string())
        .map_err(|e| format!("address {address}: {e}"))
}

fn number<T: FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    word.ok_or_else(|| format!("{what} is missing"))?
        .parse()
        .map_err(|_| format!("{what} is not a number"))
}

fn status_word(status: Status) -> String {
    match status {
        Status::Waiting => "waiting".to_string(),
        Status::PartPaid { owed } => format!("partpaid-{owed}"),
        Status::Paid => "paid".to_string(),
        Status::Overpaid { excess } => format!("overpaid-{excess}"),
        Status::Almost { short } => format!("almost-{short}"),
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let mut words = args.iter().skip(1);
    let dir = words.next().ok_or(USAGE)?;
    let mut peers: Vec<SocketAddr> = Vec::new();
    let (mut birthday, mut depth) = (0u32, 2u32);
    while let Some(word) = words.next() {
        match word.as_str() {
            "--birthday" => birthday = number(words.next().map(String::as_str), "the birthday")?,
            "--depth" => depth = number(words.next().map(String::as_str), "the depth")?,
            peer => peers.push(peer.parse().map_err(|_| format!("node address: {peer}"))?),
        }
    }
    if peers.is_empty() {
        return Err(USAGE.to_string());
    }
    let dir_path = Path::new(dir);
    std::fs::create_dir_all(dir_path).map_err(|e| e.to_string())?;

    let (wallet, book, former) = {
        let shop = OwnNode::open(dir_path, identity()?, pinned()?).map_err(|e| e.to_string())?;
        shop.settle_coins().map_err(|e| e.to_string())?;
        (
            shop.shop().wallet().clone(),
            shop.shop().book().clone(),
            shop.former_parts(),
        )
    };
    let mut driver = DriverConfig::new(Network::Regtest, 0);
    driver.allow_local = true;
    driver.fixed_peers = peers;
    let mut config = OwnerConfig::new(dir, identity()?, driver);
    config.birthday = birthday;
    config.broadcast_without_tor = true;
    let approver = own_approver(&OWNER_SECRET).map_err(|e| e.to_string())?;
    let mut parts = OwnerParts::new(
        OWNER_SECRET,
        wallet.wallet().clone(),
        approver,
        BatchPolicy { min_count: 3 },
    )
    .map_err(|e| e.to_string())?;
    parts.refund_depth = 6;
    parts.own_book = Some(book);
    parts.former = former;
    let mut node = OwnerNode::start(config, parts).map_err(|e| e.to_string())?;
    // The shop's settings are not read here: the context is what the app
    // would give for a shop in bitcoin whose settings ask for `depth`.
    node.set_request_context(
        RequestContext {
            currency: ShopCurrency::Sats,
            min_sale_sats: 0,
            min_confirmations: u8::try_from(depth).map_err(|_| "the depth is at most 255")?,
            tz_offset_secs: 0,
        },
        None,
    );
    say(format!(
        "started checked={} tip={} view={}",
        node.view().checked(),
        node.watcher().chain().tip_height(),
        node.state()
    ));

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
                match command(&mut node, dir_path, line.trim()) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(why) => say(format!("refused {why}")),
                }
                done += 1;
                say(format!("done {done}"));
            }
            Err(TryRecvError::Empty) => node.pump(Duration::from_millis(100)),
            Err(TryRecvError::Disconnected) => break,
        }
        for note in node.watcher_mut().take_notes() {
            say(format!("note {note}"));
        }
        if node.halted().is_some() {
            say("stopped");
            break;
        }
    }
    Ok(())
}

/// The kernel's bytes, from the operating system.
struct Kernel;

impl KernelRandom for Kernel {
    fn read32(&mut self) -> Option<[u8; KERNEL_BYTES]> {
        use std::io::Read;
        let mut out = [0u8; KERNEL_BYTES];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut out))
            .ok()?;
        Some(out)
    }
}

/// `n` unused indexes of the desk, as the app's host takes them.
fn desk_indexes(node: &mut OwnerNode, dir: &Path, now: u64, n: usize) -> Result<Vec<u32>, String> {
    let mut taken = node.request_indexes();
    let mut shop = OwnNode::open(dir, identity()?, pinned()?).map_err(|e| e.to_string())?;
    let mut got = Vec::new();
    for _ in 0..n {
        let index = shop
            .desk_index(&|i| taken.contains(&i), now, node)
            .map_err(|e| e.to_string())?;
        taken.insert(index);
        got.push(index);
    }
    Ok(got)
}

fn command(node: &mut OwnerNode, dir: &Path, line: &str) -> Result<bool, String> {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("pair") => {
            let url = words.next().ok_or("the address is missing")?;
            let name = words.collect::<Vec<_>>().join(" ");
            let shown = node
                .web_pair_start(url, &name, now_secs(), &mut Kernel)
                .map_err(|e| format!("{e:?}"))?;
            say(format!("code {}", shown.code));
            say(format!("fingerprint {}", shown.fingerprint));
        }
        Some("confirm") => node.web_pair_confirm().map_err(|e| format!("{e:?}"))?,
        Some("step") => {
            let now = now_secs();
            for _ in 0..4 {
                match node
                    .web_step(now, true, &mut Kernel)
                    .map_err(|e| format!("{e:?}"))?
                {
                    WebStep::Idle => say("idle"),
                    WebStep::Busy => say("busy"),
                    WebStep::NeedIndexes(n) => {
                        let indexes = desk_indexes(node, dir, now, n)?;
                        node.web_give_indexes(&indexes)
                            .map_err(|e| format!("{e:?}"))?;
                        continue;
                    }
                    WebStep::Call(call) => {
                        say(format!("call {:?}", call.kind));
                        say(format!("url {}", call.url));
                        for (name, value) in &call.headers {
                            say(format!("header {name} {value}"));
                        }
                        say(format!("type {}", call.content_type));
                        let body = String::from_utf8(call.body).map_err(|e| e.to_string())?;
                        say(format!("body {body}"));
                        say("end");
                    }
                }
                break;
            }
        }
        Some("reply") => {
            let status: u16 = number(words.next(), "the status")?;
            let body = words.collect::<Vec<_>>().join(" ");
            node.web_reply(status, body.as_bytes(), now_secs())
                .map_err(|e| format!("{e:?}"))?;
        }
        Some("web") => {
            let s = node.web_state();
            say(format!(
                "web paired={} left={} sent={} orders={} waiting={} told={} notices={} refused={}",
                s.pairing == Some(PairState::Paired),
                s.left.map_or("-".to_string(), |n| n.to_string()),
                s.addresses_sent,
                s.orders,
                s.waiting,
                s.told,
                s.notices_waiting,
                s.refused.len()
            ));
        }
        Some("order") => {
            let n: u64 = number(words.next(), "the number")?;
            let stand = match node.web_order_stand(n) {
                None => "none",
                Some(OrderStand::Waiting) => "waiting",
                Some(OrderStand::Told) => "told",
                Some(OrderStand::NotTold) => "nottold",
                Some(OrderStand::Refused) => "refused",
            };
            say(format!("order {n} {stand}"));
        }
        Some("requests") => {
            let book = node.requests().map_err(|e| e.to_string())?;
            for r in book.requests() {
                say(format!(
                    "request {} {} credited={} {}",
                    r.number,
                    status_word(r.status(0)),
                    r.credited(),
                    r.note
                ));
            }
        }
        Some("regtest") => say(format!(
            "regtest {}",
            rewrite(
                words.next().ok_or("the address is missing")?,
                Network::Regtest
            )?
        )),
        Some("status_chain") | Some("tip") => say(format!(
            "status tip={} checked={} view={}",
            node.watcher().chain().tip_height(),
            node.view().checked(),
            node.state()
        )),
        Some("pump") => {
            let secs: f64 = number(words.next(), "the seconds")?;
            node.pump(Duration::from_secs_f64(secs.clamp(0.0, 600.0)));
        }
        Some("quit") => return Ok(false),
        Some(other) => return Err(format!("unknown command: {other}")),
        None => {}
    }
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("{why}");
            ExitCode::from(2)
        }
    }
}
