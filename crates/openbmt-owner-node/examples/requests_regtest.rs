//! The owner node of a shop with its own wallet and its requests, against a
//! Bitcoin node on regtest, as a command that reads simple lines on its
//! standard input and prints one line for each thing it does.
//! `scripts/requests-e2e-core.sh` drives it.
//!
//! ```text
//! requests_regtest <data dir> <peer> [--birthday <height>] [--depth <n>]
//! ```
//!
//! The wallet is a single key of the key maker's test words on the main
//! network's account path (WL-11). The chain is regtest, so the addresses
//! the node writes are shown in their regtest form, which is the same
//! script, and the commands take regtest addresses.
//!
//! **Test only, never use.** The identity and the wallet are fixed.
//!
//! Commands, one per line:
//!
//! - `make <sats> <note>`: make a request for `sats`, due in a week
//! - `share <number>`: write a quote and print the link
//! - `status <number>`: where the request stands
//! - `return <number> <sats> <address>`: queue a return to a regtest address
//! - `queue`: the owner's queue
//! - `state`: the book's counts
//! - `rescan`, `pump <seconds>`, `quit`
//!
//! Lines printed:
//!
//! - `started checked=<h> tip=<h> view=<text>`
//! - `made <number> <index> <address>`
//! - `shared <number> sats=<n> <link>`
//! - `request <number> <status> credited=<sats> pending=<n>`
//! - `returned id=<n> refund=<n> sats=<n>`
//! - `entry <id> <stage>`, `return <id> <state>`
//! - `state open=<n> arrivals=<n> returns=<n> view=<text>`
//! - `refused <text>`, `status tip=<h> checked=<h> view=<text>`
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
use openbmt_owner_node::{
    own_approver, OwnNode, OwnerConfig, OwnerNode, OwnerParts, RequestContext,
};
use openbmt_requests::{day_of, NewRequest, PriceIn, ReturnAmount, ReturnState, Status};
use openbmt_watch::HeaderChain;

const USAGE: &str = "usage: requests_regtest <data dir> <peer> [--birthday <height>] [--depth <n>]";

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

fn command(node: &mut OwnerNode, dir: &Path, line: &str) -> Result<bool, String> {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("make") => {
            let sats: u64 = number(words.next(), "the satoshis")?;
            let note = words.collect::<Vec<_>>().join(" ");
            let now = now_secs();
            let new = NewRequest {
                note,
                amount: sats,
                price_in: PriceIn::Bitcoin,
                due: day_of(now, 0)
                    .plus_days(7)
                    .ok_or("the due date is out of range")?,
                repeat: None,
            };
            let taken = node.request_indexes();
            let wallet = pinned()?;
            let index = {
                let mut shop =
                    OwnNode::open(dir, identity()?, wallet).map_err(|e| e.to_string())?;
                shop.desk_index(&|i| taken.contains(&i), now, node)
                    .map_err(|e| e.to_string())?
            };
            let made = node
                .make_request(&new, index, now)
                .map_err(|e| e.to_string())?;
            say(format!(
                "made {} {} {}",
                made.number,
                made.index,
                rewrite(&made.address, Network::Regtest)?
            ));
        }
        Some("share") => {
            let n: u64 = number(words.next(), "the number")?;
            let shared = node
                .share_request(n, now_secs(), "requests-e2e")
                .map_err(|e| e.to_string())?;
            say(format!(
                "shared {n} sats={} {}",
                shared.quote.sats,
                rewrite_link(&shared.link)?
            ));
        }
        Some("status") => {
            let n: u64 = number(words.next(), "the number")?;
            let book = node.requests().map_err(|e| e.to_string())?;
            let r = book.request(n).ok_or("no such request")?;
            say(format!(
                "request {n} {} credited={} pending={}",
                status_word(r.status(0)),
                r.credited(),
                node.request_arrivals().len()
            ));
        }
        Some("return") => {
            let n: u64 = number(words.next(), "the number")?;
            let sats: u64 = number(words.next(), "the satoshis")?;
            let address = rewrite(
                words.next().ok_or("the address is missing")?,
                Network::Bitcoin,
            )?;
            let added = node
                .add_return(n, ReturnAmount::Sats(sats), &address, now_secs())
                .map_err(|e| e.to_string())?;
            say(format!(
                "returned id={} refund={} sats={}",
                added.id, added.refund, added.sats
            ));
        }
        Some("queue") => {
            for r in node.owner().refunds() {
                let stage = format!("{:?}", r.stage);
                let name = stage.split([' ', '{']).next().unwrap_or("");
                say(format!("entry {} {name}", r.id));
            }
            if let Ok(book) = node.requests() {
                for r in book.returns() {
                    let state = match r.state {
                        ReturnState::Queued => "queued",
                        ReturnState::Sent { .. } => "sent",
                        ReturnState::Done { .. } => "done",
                    };
                    say(format!("return {} {state}", r.id));
                }
            }
        }
        Some("state") => {
            let (open, returns) = node
                .requests()
                .map(|b| (b.open_count(), b.returns().len()))
                .map_err(|e| e.to_string())?;
            say(format!(
                "state open={open} arrivals={} returns={returns} view={}",
                node.request_arrivals().len(),
                node.state()
            ));
        }
        Some("status_chain") | Some("tip") => say(format!(
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
    Ok(true)
}

/// A BIP 321 link with the address in its regtest form.
fn rewrite_link(link: &str) -> Result<String, String> {
    let rest = link.strip_prefix("bitcoin:").ok_or("not a bitcoin link")?;
    let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
    let address = rewrite(address, Network::Regtest)?;
    Ok(if query.is_empty() {
        format!("bitcoin:{address}")
    } else {
        format!("bitcoin:{address}?{query}")
    })
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
