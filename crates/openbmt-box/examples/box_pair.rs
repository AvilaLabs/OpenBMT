//! The Key app and the owner phone, played with the core functions, for
//! pairing a Box and talking to it (`scripts/box-e2e.sh` runs it).
//!
//! ```text
//! box_pair invite   <dir>                       make an invitation (BX-10)
//! box_pair claim    <dir> <addr>                fetch the claim and check it (BX-13, BX-14)
//! box_pair welcome  <dir> <addr>                send the welcome and first settings (BX-15)
//! box_pair mailbox  <dir> <addr> <name>         make a device's mailbox
//! box_pair send     <dir> <addr> <from> <to> <text>
//! box_pair recv     <dir> <addr> <name>
//! box_pair settings <dir> <addr> <version> <drop-c2 | drop-box>
//! box_pair key      <dir> <name>                print a device's key in hex
//!
//! `drop-c2` takes counter 2 off the list. `drop-box` takes the Box off it
//! as well (and counter 2, so that it is still a tightening of what
//! `drop-c2` sent).
//! ```
//!
//! The devices are `owner`, `approver`, `c1`, `c2` and `stranger`. Their
//! keys are made from their names, so that two runs agree: for tests only.
//! State between the commands is in `<dir>`: `invitation.txt` and
//! `claim.bin`. Every command prints what it did and exits 0, or prints
//! why not and exits 1.

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use openbmt_core::batch::PinnedRoots;
use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::{tagged_hash, DeviceId, Identity};
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_link::{open, os_random, seal, Client, ClientConfig, ClientError, Kind, Role, Route};

fn identity(name: &str) -> Result<Identity, String> {
    let secret = tagged_hash("OpenBMT/example/box-pair/v1", name.as_bytes());
    Identity::from_secret_bytes(&secret).map_err(|e| e.to_string())
}

fn role_of(name: &str) -> Result<Role, String> {
    match name {
        "owner" => Ok(Role::Owner),
        "c1" | "c2" | "stranger" => Ok(Role::Counter),
        other => Err(format!("{other} is not a device of this example")),
    }
}

fn roots() -> Result<PinnedRoots, String> {
    PinnedRoots::new(
        identity("owner")?.device_id(),
        identity("approver")?.device_id(),
    )
    .map_err(|e| e.to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn connect(addr: &str, expected: Option<&DeviceId>) -> Result<Client, String> {
    let addr: SocketAddr = addr
        .parse()
        .map_err(|_| format!("{addr} is not an address"))?;
    Client::connect(&Route::Direct(addr), expected, &ClientConfig::default())
        .map_err(|e| e.to_string())
}

fn read_invitation(dir: &Path) -> Result<Invitation, String> {
    let text = fs::read_to_string(dir.join("invitation.txt"))
        .map_err(|_| "no invitation is open on the Key app (BX-11)".to_string())?;
    Invitation::from_text(text.trim()).map_err(|e| e.to_string())
}

fn read_claim(dir: &Path) -> Result<Claim, String> {
    let bytes = fs::read(dir.join("claim.bin")).map_err(|_| "no claim was kept".to_string())?;
    Claim::from_bytes(&bytes).map_err(|e| e.to_string())
}

/// First settings: two counters and the Box (BX-15, BX-20).
fn first_settings(box_key: &DeviceId, version: u32) -> Result<Settings, String> {
    let (c1, c2) = (identity("c1")?.device_id(), identity("c2")?.device_id());
    let mut s = Settings::defaults(1_000);
    s.version = version;
    s.devices = [c1, c2, *box_key].into_iter().collect();
    s.counters = [(c1, 2_500u16), (c2, 2_500u16)].into_iter().collect();
    s.boxes = [*box_key].into_iter().collect();
    Ok(s)
}

fn signed(s: Settings, both: bool) -> Result<SignedSettings, String> {
    Ok(SignedSettings {
        owner_sig: s.sign(&identity("owner")?),
        approver_sig: if both {
            Some(s.sign(&identity("approver")?))
        } else {
            None
        },
        settings: s,
    })
}

fn rng(buf: &mut [u8; 32]) -> std::io::Result<()> {
    os_random(buf)
}

/// The next number from `from` to `to`, kept in a file.
fn next_number(dir: &Path, from: &str, to: &str) -> Result<u64, String> {
    let path = dir.join(format!("seq-{from}-{to}"));
    let last: u64 = fs::read_to_string(&path)
        .ok()
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    let next = last + 1;
    fs::write(&path, next.to_string()).map_err(|e| e.to_string())?;
    Ok(next)
}

fn refused(e: &ClientError) -> String {
    format!("refused: {e}")
}

fn run(args: &[String]) -> Result<(), String> {
    let arg = |i: usize| args.get(i).map(String::as_str).ok_or("missing argument");
    let dir = PathBuf::from(arg(1)?);
    match arg(0)? {
        "invite" => {
            let mut random = [0u8; 32];
            os_random(&mut random).map_err(|e| e.to_string())?;
            let mut bytes = [0u8; 17];
            bytes.copy_from_slice(random.get(..17).ok_or("short random")?);
            let text = Invitation::from_random(&bytes).to_text();
            // One open invitation at a time: a new one replaces the old.
            fs::write(dir.join("invitation.txt"), &text).map_err(|e| e.to_string())?;
            println!("{text}");
        }
        "claim" => {
            let mut c = connect(arg(2)?, None)?;
            let bytes = c.claim().map_err(|e| refused(&e))?;
            let claim = Claim::from_bytes(&bytes).map_err(|e| e.to_string())?;
            let inv = read_invitation(&dir)?;
            // A claim that fails spends the invitation (BX-11).
            let _ = fs::remove_file(dir.join("invitation.txt"));
            if !claim.verify(&inv) {
                return Err("claim REJECTED: it was not made with this invitation".into());
            }
            fs::write(dir.join("claim.bin"), &bytes).map_err(|e| e.to_string())?;
            // The invitation is kept for the welcome until the approval.
            fs::write(dir.join("invitation.txt"), inv.to_text()).map_err(|e| e.to_string())?;
            println!("claim verified");
            println!(
                "box fingerprint: {}",
                claim.box_key.fingerprint_words().join(" ")
            );
            println!("box key: {}", hex(&claim.box_key.to_bytes()));
        }
        "welcome" => {
            let claim = read_claim(&dir)?;
            let inv = read_invitation(&dir)?;
            let _ = fs::remove_file(dir.join("invitation.txt"));
            let welcome = Welcome::make(&inv, &claim, roots()?);
            let settings = signed(first_settings(&claim.box_key, 1)?, true)?;
            let mut c = connect(arg(2)?, Some(&claim.box_key))?;
            c.welcome(&welcome.to_bytes(), &settings.to_bytes())
                .map_err(|e| refused(&e))?;
            println!("welcomed");
        }
        "key" => {
            println!("{}", hex(&identity(arg(2)?)?.device_id().to_bytes()));
        }
        "mailbox" => {
            let who = identity(arg(3)?)?;
            connect(arg(2)?, None)?
                .make_mailbox(&who)
                .map_err(|e| refused(&e))?;
            println!("mailbox made for {}", arg(3)?);
        }
        "send" => {
            let (from, to) = (arg(3)?, arg(4)?);
            let kind = if to == "owner" {
                Kind::CheckIn
            } else {
                Kind::Heartbeat
            };
            let n = next_number(&dir, from, to)?;
            let sealed = seal(
                &identity(from)?,
                role_of(from)?,
                &identity(to)?.device_id(),
                role_of(to)?,
                n,
                kind,
                arg(5)?.as_bytes(),
                &mut rng,
            )
            .map_err(|e| e.to_string())?;
            connect(arg(2)?, None)?
                .put(&sealed)
                .map_err(|e| refused(&e))?;
            println!("sent from {from} to {to}");
        }
        "recv" => {
            let name = arg(3)?;
            let me = identity(name)?;
            let mut c = connect(arg(2)?, None)?;
            c.make_mailbox(&me).map_err(|e| refused(&e))?;
            let got = c.fetch(&me, 0).map_err(|e| refused(&e))?;
            let known: Vec<(&str, Identity)> = ["owner", "c1", "c2"]
                .into_iter()
                .map(|n| identity(n).map(|i| (n, i)))
                .collect::<Result<_, _>>()?;
            let mut last = 0;
            for (id, sealed) in &got.items {
                let opened = open(
                    &me,
                    role_of(name)?,
                    sealed,
                    |d| {
                        known
                            .iter()
                            .find(|(_, i)| i.device_id() == *d)
                            .and_then(|(n, _)| role_of(n).ok())
                    },
                    |_| 0,
                )
                .map_err(|e| e.to_string())?;
                let from = known
                    .iter()
                    .find(|(_, i)| i.device_id() == opened.sender)
                    .map_or("?", |(n, _)| *n);
                println!(
                    "message from {from}: {}",
                    String::from_utf8_lossy(&opened.content)
                );
                last = *id;
            }
            if last > 0 {
                c.ack(&me, last).map_err(|e| refused(&e))?;
            }
            println!("received {}", got.items.len());
        }
        "settings" => {
            let version: u32 = arg(3)?.parse().map_err(|_| "version is a number")?;
            let claim = read_claim(&dir)?;
            let mut s = first_settings(&claim.box_key, version)?;
            let c2 = identity("c2")?.device_id();
            match arg(4)? {
                "drop-c2" => {
                    s.devices.remove(&c2);
                    s.counters.remove(&c2);
                }
                "drop-box" => {
                    s.devices.remove(&c2);
                    s.counters.remove(&c2);
                    s.devices.remove(&claim.box_key);
                    s.boxes.clear();
                }
                other => return Err(format!("unknown change {other}")),
            }
            // Both are tightenings: the owner's signature is enough.
            let settings = signed(s, false)?;
            let n = next_number(&dir, "owner", "box")?;
            let sealed = seal(
                &identity("owner")?,
                Role::Owner,
                &claim.box_key,
                Role::Box,
                n,
                Kind::Settings,
                &settings.to_bytes(),
                &mut rng,
            )
            .map_err(|e| e.to_string())?;
            // The Box makes its own mailbox when it starts reading; wait for it.
            let mut last = String::new();
            for _ in 0..100 {
                match connect(arg(2)?, Some(&claim.box_key))?.put(&sealed) {
                    Ok(_) => {
                        println!("settings version {version} sent");
                        return Ok(());
                    }
                    Err(e) => last = refused(&e),
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            return Err(last);
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            println!("{e}");
            ExitCode::FAILURE
        }
    }
}
