//! The owner phone of a shop with its own wallet, pairing a Box with no Key
//! app (BX-18). `scripts/own-box-e2e-core.sh` runs it next to a real Box.
//!
//! ```text
//! own_box welcome <dir> <box addr>   pair the example counter, add the Box, send the welcome
//! own_box mailbox <box addr>         make the owner's mailbox on the Box (LK-20)
//! own_box key                        print the owner root's key in hex
//! ```
//!
//! `<dir>` holds the invitation and the claim that `box_pair invite` and
//! `box_pair claim` kept. The shop is the owner library's own shop
//! (`OwnShop`): the counter of `counter_regtest` is paired by the shop's
//! offer and request first, so that the first settings list it, then the Box
//! is added with `OwnShop::add_box` (settings signed by the owner root alone)
//! and the welcome is the own-wallet one (`Welcome::make_own`). The owner root
//! is the fixed key of `owner_regtest`, so that example can follow the Box
//! afterwards as the shop's owner.
//!
//! **Test only, never use.** The owner root and the wallet are fixed.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::OwnCounterPairing;
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_link::{Client, ClientConfig, Route};
use openbmt_owner::own_shop::OwnShop;

/// TEST ONLY, NEVER USE: the secret bytes of the owner root of `owner_regtest`.
const OWNER_SECRET: [u8; 32] = [0x11; 32];
/// The secret bytes of the counter of `counter_regtest`.
const COUNTER_SECRET: [u8; 32] = [0x13; 32];
/// The key maker's test wallet, single key, as the owner library's tests use.
const WALLET: &str = "wpkh([818bc9b6/84h/0h/0h]xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8/<0;1>/*)#96yh9yhx";
/// The price the first settings start at, in cents for a bitcoin.
const USD_PER_BTC: u64 = 10_000_000;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn owner() -> Result<Identity, String> {
    Identity::from_secret_bytes(&OWNER_SECRET).map_err(|e| e.to_string())
}

fn address(text: &str) -> Result<SocketAddr, String> {
    text.parse().map_err(|_| format!("{text} is not an address"))
}

fn welcome(dir: &str, addr: &str) -> Result<(), String> {
    let dir = PathBuf::from(dir);
    let text =
        std::fs::read_to_string(dir.join("invitation.txt")).map_err(|_| "no invitation is open")?;
    let invitation = Invitation::from_text(text.trim()).map_err(|e| e.to_string())?;
    let claim_bytes = std::fs::read(dir.join("claim.bin")).map_err(|_| "no claim was kept")?;
    let claim = Claim::from_bytes(&claim_bytes).map_err(|e| e.to_string())?;
    if !claim.verify(&invitation) {
        return Err("the claim was not made with this invitation".to_string());
    }
    let wallet = PinnedWallet::from_text(WALLET).map_err(|e| format!("{e:?}"))?;
    let mut shop = OwnShop::new(owner()?, wallet);
    let store = |_: &[u8]| Ok::<(), String>(());

    // The counter first, so that the first settings list it.
    let counter = Identity::from_secret_bytes(&COUNTER_SECRET).map_err(|e| e.to_string())?;
    let made = shop.make_offer(now_secs()).map_err(|e| format!("{e:?}"))?;
    let (_, request) = OwnCounterPairing::begin(
        made.offer.clone(),
        &counter,
        "Front",
        [0x13; 32],
        made.made_at + 1,
    )
    .map_err(|e| format!("{e:?}"))?;
    shop.confirm(
        &made,
        made.made_at + 2,
        &request.to_bytes(),
        USD_PER_BTC,
        Vec::new(),
        None,
        store,
    )
    .map_err(|e| format!("{e:?}"))?;

    // The Box: settings signed by the owner root alone, and the own-wallet welcome.
    let settings = shop
        .add_box(claim.box_key, 0, store)
        .map_err(|e| format!("{e:?}"))?;
    let welcome = Welcome::make_own(&invitation, &claim, owner()?.device_id());
    let mut client = Client::connect(
        &Route::Direct(address(addr)?),
        Some(&claim.box_key),
        &ClientConfig::default(),
    )
    .map_err(|e| e.to_string())?;
    client
        .welcome(&welcome.to_bytes(), &settings.to_bytes())
        .map_err(|e| format!("refused: {e}"))?;
    println!(
        "welcomed settings version {} counters {} approver {}",
        settings.settings.version,
        settings.settings.counters.len(),
        if settings.approver_sig.is_some() {
            "signed"
        } else {
            "none"
        }
    );
    Ok(())
}

fn mailbox(addr: &str) -> Result<(), String> {
    Client::connect(&Route::Direct(address(addr)?), None, &ClientConfig::default())
        .map_err(|e| e.to_string())?
        .make_mailbox(&owner()?)
        .map_err(|e| format!("refused: {e}"))?;
    println!("mailbox made for the owner");
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let arg = |i: usize| args.get(i).map(String::as_str).ok_or("missing argument");
    match arg(0)? {
        "welcome" => welcome(arg(1)?, arg(2)?),
        "mailbox" => mailbox(arg(1)?),
        "key" => {
            let key = owner()?.device_id().to_bytes();
            println!("{}", key.iter().map(|b| format!("{b:02x}")).collect::<String>());
            Ok(())
        }
        other => Err(format!("{other} is not a command")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
