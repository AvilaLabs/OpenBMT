//! The owner phone of a shop with its own wallet, pairing a Box with no Key
//! app (BX-18). `scripts/own-box-e2e-core.sh` runs it next to a real Box.
//!
//! ```text
//! own_box welcome <dir> <box addr>   pair the example counter, add the Box, send the welcome
//! own_box mailbox <box addr>         make the owner's mailbox on the Box (LK-20)
//! own_box key                        print the owner root's key in hex
//! own_box flow <dir> <box addr> <box dir>
//!                                    a Box added before any counter, then a
//!                                    first and a second counter (AP-12, OA-119 to OA-122)
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

use openbmt_core::approval::CounterConfirmation;
use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::Identity;
use openbmt_core::pairing_own::OwnCounterPairing;
use openbmt_core::pinned_wallet::PinnedWallet;
use openbmt_core::settings::SignedSettings;
use openbmt_link::{os_random, seal, Client, ClientConfig, ClientError, Kind, Refusal, Role, Route};
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
    text.parse()
        .map_err(|_| format!("{text} is not an address"))
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

/// The second counter of the flow below.
const SECOND_SECRET: [u8; 32] = [0x14; 32];

/// What the Box's relay says to a counter that asks for its mailbox: `ok`
/// for a device on the settings in force, `refused` for one that is not
/// (BX-22).
fn relay_says(addr: SocketAddr, counter: &Identity) -> Result<&'static str, String> {
    let mut client = Client::connect(&Route::Direct(addr), None, &ClientConfig::default())
        .map_err(|e| e.to_string())?;
    match client.make_mailbox(counter) {
        Ok(()) => Ok("ok"),
        Err(ClientError::Refused(Refusal::NotMember)) => Ok("refused"),
        Err(e) => Err(format!("the relay answered {e}")),
    }
}

/// The owner phone's message to the Box that carries settings (`OBMS` or
/// `OBM<`), numbered `number`.
fn send_to_box(
    addr: SocketAddr,
    box_key: &openbmt_core::identity::DeviceId,
    number: u64,
    bytes: &[u8],
) -> Result<(), String> {
    let sealed = seal(
        &owner()?,
        Role::Owner,
        box_key,
        Role::Box,
        number,
        Kind::Settings,
        bytes,
        &mut os_random,
    )
    .map_err(|e| format!("{e:?}"))?;
    Client::connect(&Route::Direct(addr), None, &ClientConfig::default())
        .map_err(|e| e.to_string())?
        .put(&sealed)
        .map(|_| ())
        .map_err(|e| format!("the Box did not take the message: {e}"))
}

/// The version of the settings the Box keeps in `dir/settings`. It stores
/// the signed settings of a confirmation, not the `OBM<` frame (BX-18).
fn box_settings(dir: &str) -> Option<u32> {
    let bytes = std::fs::read(PathBuf::from(dir).join("settings")).ok()?;
    SignedSettings::from_bytes(&bytes)
        .ok()
        .map(|s| s.settings.version)
}

/// Wait for the Box's settings to reach `version`.
fn box_takes(dir: &str, version: u32) -> Option<u32> {
    for _ in 0..40 {
        if box_settings(dir) == Some(version) {
            return Some(version);
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    box_settings(dir)
}

fn pair(
    shop: &mut OwnShop,
    secret: [u8; 32],
    name: &str,
) -> Result<openbmt_owner::own_shop::Paired, String> {
    let counter = Identity::from_secret_bytes(&secret).map_err(|e| e.to_string())?;
    let made = shop.make_offer(now_secs()).map_err(|e| format!("{e:?}"))?;
    let (_, request) =
        OwnCounterPairing::begin(made.offer.clone(), &counter, name, secret, made.made_at + 1)
            .map_err(|e| format!("{e:?}"))?;
    shop.confirm(
        &made,
        made.made_at + 2,
        &request.to_bytes(),
        USD_PER_BTC,
        Vec::new(),
        None,
        |_: &[u8]| Ok::<(), String>(()),
    )
    .map_err(|e| format!("{e:?}"))
}

/// AP-12 and OA-119 to OA-122 against a real Box. The Box has just been
/// paired with the invitation in `dir` and holds nothing else. The shop adds
/// it before any counter; the first counter is then added as a Free change
/// the Box takes with the owner's signature alone, and the second as a
/// Loosen change, which waits: the Box's relay does not carry that counter
/// until the first counter's confirmation is joined and sent.
///
/// The counter's confirmation here is signed by the counter's key with the
/// core's `CounterConfirmation::sign`; `counter_regtest` has no counter that
/// is paired with an own wallet and a Box, so the real counter's own record of
/// it (CR-241) is not part of this run.
fn flow(dir: &str, addr: &str, box_dir: &str) -> Result<(), String> {
    let dirp = PathBuf::from(dir);
    let text =
        std::fs::read_to_string(dirp.join("invitation.txt")).map_err(|_| "no invitation is open")?;
    let invitation = Invitation::from_text(text.trim()).map_err(|e| e.to_string())?;
    let claim_bytes = std::fs::read(dirp.join("claim.bin")).map_err(|_| "no claim was kept")?;
    let claim = Claim::from_bytes(&claim_bytes).map_err(|e| e.to_string())?;
    let socket = address(addr)?;
    let wallet = PinnedWallet::from_text(WALLET).map_err(|e| format!("{e:?}"))?;
    let mut shop = OwnShop::new(owner()?, wallet);
    let first = Identity::from_secret_bytes(&COUNTER_SECRET).map_err(|e| e.to_string())?;
    let second = Identity::from_secret_bytes(&SECOND_SECRET).map_err(|e| e.to_string())?;

    // The Box is added before any counter: nothing is in force to confirm it.
    let settings = shop
        .add_box(claim.box_key, USD_PER_BTC, |_: &[u8]| Ok::<(), String>(()))
        .map_err(|e| format!("{e:?}"))?;
    let welcome = Welcome::make_own(&invitation, &claim, owner()?.device_id());
    Client::connect(
        &Route::Direct(socket),
        Some(&claim.box_key),
        &ClientConfig::default(),
    )
    .map_err(|e| e.to_string())?
    .welcome(&welcome.to_bytes(), &settings.to_bytes())
    .map_err(|e| format!("refused: {e}"))?;
    println!(
        "welcomed version {} counters {}",
        settings.settings.version,
        settings.settings.counters.len()
    );
    println!("relay first={}", relay_says(socket, &first)?);

    // The first counter: Free (AP-12), so it is in force at once and the Box
    // takes it with the owner's signature alone.
    let paired = pair(&mut shop, COUNTER_SECRET, "Front")?;
    let version = shop.known_settings().map_or(0, |k| k.settings.version);
    let known = shop.known_settings().ok_or("no settings")?.to_bytes();
    send_to_box(socket, &claim.box_key, 1, &known)?;
    let took = box_takes(box_dir, version);
    println!(
        "first waiting={} version={version} box_version={} relay first={}",
        paired.waiting,
        took.unwrap_or(0),
        relay_says(socket, &first)?
    );

    // The second counter: a Loosen change, which waits.
    let paired = pair(&mut shop, SECOND_SECRET, "Back")?;
    let held = shop
        .waiting_change()
        .ok_or("the second counter's change does not wait")?
        .clone();
    let held_version = held.signed.settings.version;
    send_to_box(socket, &claim.box_key, 2, &held.signed.to_bytes())?;
    std::thread::sleep(std::time::Duration::from_secs(2));
    println!(
        "second waiting={} held_version={held_version} in_force={} box_version={} relay second={}",
        paired.waiting,
        shop.known_settings().map_or(0, |k| k.settings.version),
        box_settings(box_dir).unwrap_or(0),
        relay_says(socket, &second)?
    );

    // The first counter confirms; the owner joins, puts it in force and sends
    // it to the Box.
    let confirmation = CounterConfirmation::sign(&first, &held.hash());
    let confirmed = shop
        .join_confirmation(confirmation)
        .ok_or("the confirmation does not join")?;
    shop.put_in_force(&confirmed, |_: &[u8]| Ok::<(), String>(()))
        .map_err(|e| format!("{e:?}"))?;
    send_to_box(socket, &claim.box_key, 3, &confirmed.to_bytes())?;
    let took = box_takes(box_dir, held_version);
    println!(
        "confirmed in_force={} waiting={} box_version={} relay second={}",
        shop.known_settings().map_or(0, |k| k.settings.version),
        shop.waiting_change().is_some(),
        took.unwrap_or(0),
        relay_says(socket, &second)?
    );
    Ok(())
}

fn mailbox(addr: &str) -> Result<(), String> {
    Client::connect(
        &Route::Direct(address(addr)?),
        None,
        &ClientConfig::default(),
    )
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
        "flow" => flow(arg(1)?, arg(2)?, arg(3)?),
        "key" => {
            let key = owner()?.device_id().to_bytes();
            println!(
                "{}",
                key.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
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
