//! The Key app and the owner phone, played with the core functions, for
//! pairing a Box and talking to it (`scripts/box-e2e.sh` runs it).
//!
//! ```text
//! box_pair invite   <dir>                       make an invitation (BX-10)
//! box_pair claim    <dir> <addr>                fetch the claim and check it (BX-13, BX-14)
//! box_pair welcome  <dir> <addr>                send the welcome and first settings (BX-15)
//! box_pair badwelcome <dir> <addr>              a stranger's welcome (garbage)
//! box_pair mailbox  <dir> <addr> <name>         make a device's mailbox
//! box_pair send     <dir> <addr> <from> <to> <text>
//! box_pair recv     <dir> <addr> <name>
//! box_pair fetch    <dir> <addr> <name>         read a mailbox without making it or acknowledging (LK-28)
//! box_pair settings <dir> <addr> <version> <drop-c2 | drop-box>
//! box_pair key      <dir> <name>                print a device's key in hex
//! box_pair notice   <dir> <addr> <counter> <index> <opened|paid|unpaid|final> <cents>
//!                                               a counter's paid notice to the Box (BX-40)
//! box_pair posrecv  <dir> <addr>                the owner reads the Box's POS notices (BX-42)
//! box_pair pos-order <pos dir> <id> <open|closed> <cents> <bitcoin|none>
//!                                               put an order in the test POS
//! box_pair pos-close-on <pos dir>               let the test POS close orders
//! box_pair pos-box  <box dir> <addr> <pos dir>  a Box with the test POS (BX-41)
//!
//! `drop-c2` takes counter 2 off the list. `drop-box` takes the Box off it
//! as well (and counter 2, so that it is still a tightening of what
//! `drop-c2` sent).
//! ```
//!
//! `pos-box` is what a program that links the library does: the Box with a
//! connector. Its connector is a directory, `<pos dir>`: `orders.txt` holds
//! one order per line, `<id> <open|closed> <cents> <unix seconds> <bitcoin|none>`
//! (location `Main`, currency USD; `bitcoin` is a paid tender named
//! `Bitcoin`), `can-close` (if present) lets it close, `closes.txt` gets a line
//! `<key hex> <order>` for each order it closed, `calls.txt` a line for each
//! call of `close` and `token.txt` says whether a sign-in was given. It finds
//! the one open order without a Bitcoin tender for the sale's amount. Its
//! console is the Box's (`invite <text>`, `state`, `pos`, `pos-token` and the
//! line after it, `notes`, `quit`). `pos-order` stamps the order with the
//! time now.
//!
//! The devices are `owner`, `approver`, `c1`, `c2` and `stranger`. Their
//! keys are made from their names, so that two runs agree: for tests only.
//! State between the commands is in `<dir>`: `invitation.txt` and
//! `claim.bin`. Every command prints what it did and exits 0, or prints
//! why not and exits 1.

use std::fs;
use std::io::{self, BufRead, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use openbmt_box::say::{
    invite_lines, notes_lines, pos_lines, pos_token_lines, state_lines, unknown_lines,
};
use openbmt_box::{BoxError, BoxRunner, PosSetup};
use openbmt_core::batch::PinnedRoots;
use openbmt_core::boxpair::{Claim, Invitation, Welcome};
use openbmt_core::identity::{tagged_hash, DeviceId, Identity};
use openbmt_core::settings::{Settings, SignedSettings};
use openbmt_counter::{Amount, Currency};
use openbmt_link::{
    open, os_random, seal, Client, ClientConfig, ClientError, Kind, Role, Route, ServerConfig,
};
use openbmt_net_io::SystemClock;
use openbmt_pos::{
    CloseOutcome, CloseRequest, Connector, Cursor, Id, LocationCounters, NoticeDraft, NoticeState,
    OrderId, OrderState, Page, PaidNotice, PosError, PosNotice, PosOrder, PosReader, PosTender,
    SaleFact, Settings as PosSettings, TenderKind, TenderState, Text,
};

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

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn usd() -> Result<Currency, String> {
    Currency::new(*b"USD").ok_or_else(|| "USD is not a currency".to_string())
}

/// The shop's POS settings for `pos-box`: location `Main`, served by `c1`.
fn pos_settings() -> Result<PosSettings, String> {
    PosSettings::new(
        vec![Text::new("Bitcoin").map_err(|e| e.to_string())?],
        vec![LocationCounters {
            location: Text::new("Main").map_err(|e| e.to_string())?,
            counters: vec![identity("c1")?.device_id()],
        }],
        vec![usd()?],
    )
    .map_err(|e| e.to_string())
}

/// The test POS: the directory described at the top of this file.
struct FileConnector {
    dir: PathBuf,
}

impl FileConnector {
    fn lines(&self) -> Vec<String> {
        fs::read_to_string(self.dir.join("orders.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn orders(&self) -> Vec<PosOrder> {
        self.lines().iter().filter_map(|l| parse_order(l)).collect()
    }

    fn append(&self, file: &str, line: &str) -> Result<(), PosError> {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(file))
            .map_err(|_| PosError::Unavailable)?;
        writeln!(f, "{line}").map_err(|_| PosError::Unavailable)
    }
}

fn parse_order(line: &str) -> Option<PosOrder> {
    let f: Vec<&str> = line.split_whitespace().collect();
    let [id, state, cents, at, tender] = f[..] else {
        return None;
    };
    let (cents, at): (u64, u64) = (cents.parse().ok()?, at.parse().ok()?);
    let closed = state == "closed";
    let tenders = if tender == "bitcoin" {
        vec![PosTender {
            id: Id::new(&format!("t-{id}")).ok()?,
            kind: TenderKind::Other,
            name: Text::new("Bitcoin").ok()?,
            amount_minor: cents,
            tip_minor: 0,
            state: TenderState::Paid,
            at,
            reference: None,
        }]
    } else {
        Vec::new()
    };
    Some(PosOrder {
        id: Id::new(id).ok()?,
        location: Text::new("Main").ok()?,
        state: if closed {
            OrderState::Closed
        } else {
            OrderState::Open
        },
        currency: Currency::new(*b"USD")?,
        total_minor: cents,
        tenders,
        opened_at: at,
        closed_at: closed.then_some(at),
        updated_at: at,
        reference: None,
    })
}

impl PosReader for FileConnector {
    fn poll(&mut self, cursor: &Cursor) -> Result<Page, PosError> {
        Ok(Page {
            orders: self.orders(),
            next: cursor.clone(),
            more: false,
        })
    }
}

impl Connector for FileConnector {
    fn set_token(&mut self, token: Option<&str>) {
        let line = if token.is_some() { "set" } else { "none" };
        let _ = fs::write(self.dir.join("token.txt"), line);
    }

    fn can_close(&self) -> bool {
        self.dir.join("can-close").exists()
    }

    fn find_order(&mut self, sale: &SaleFact) -> Result<Option<OrderId>, PosError> {
        let open: Vec<PosOrder> = self
            .orders()
            .into_iter()
            .filter(|o| {
                o.state == OrderState::Open
                    && o.tenders.is_empty()
                    && o.total_minor == sale.amount.0
            })
            .collect();
        Ok(match open.as_slice() {
            [one] => Some(one.id.clone()),
            _ => None,
        })
    }

    fn close(&mut self, request: &CloseRequest) -> Result<CloseOutcome, PosError> {
        let key = request.key.to_hex();
        self.append("calls.txt", &key)?;
        let done = fs::read_to_string(self.dir.join("closes.txt")).unwrap_or_default();
        if done.lines().any(|l| l.starts_with(&key)) {
            return Ok(CloseOutcome::AlreadyClosed);
        }
        self.append("closes.txt", &format!("{key} {}", request.order))?;
        // The order now carries a paid Bitcoin tender and is closed.
        let at = now_secs();
        let lines: Vec<String> = self
            .lines()
            .into_iter()
            .map(|l| match l.split_whitespace().next() {
                Some(id) if id == request.order.as_str() => {
                    format!("{id} closed {} {at} bitcoin", request.amount_minor)
                }
                _ => l,
            })
            .collect();
        fs::write(self.dir.join("orders.txt"), lines.join("\n") + "\n")
            .map_err(|_| PosError::Unavailable)?;
        Ok(CloseOutcome::Closed)
    }
}

/// A Box with the test POS, and the Box's console on standard input.
fn pos_box(data: &Path, addr: &str, pos_dir: &Path) -> Result<(), String> {
    let bind: SocketAddr = addr
        .parse()
        .map_err(|_| format!("{addr} is not an address"))?;
    fs::create_dir_all(pos_dir).map_err(|e| e.to_string())?;
    let setup = PosSetup::new(
        pos_settings()?,
        Box::new(FileConnector {
            dir: pos_dir.to_path_buf(),
        }),
    );
    let runner = BoxRunner::start_with(
        data,
        bind,
        Arc::new(SystemClock),
        ServerConfig::default(),
        Some(setup),
    )
    .map_err(|e| e.to_string())?;
    let say = |line: &str| {
        let mut out = io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    };
    say(&format!(
        "fingerprint: {}",
        runner.fingerprint_words().join(" ")
    ));
    say(&format!("key: {}", hex(&runner.device_id().to_bytes())));
    say(&format!("listening: {}", runner.local_addr()));
    for line in state_lines(runner.status()) {
        say(&line);
    }
    let answer = |lines: Vec<String>| {
        for line in lines {
            say(&line);
        }
    };
    let mut lines = io::stdin().lock().lines();
    while let Some(Ok(line)) = lines.next() {
        let line = line.trim().to_string();
        let (command, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        match command {
            "invite" => answer(invite_lines(&runner.enter_invitation(rest.trim()))),
            "state" => answer(state_lines(runner.status())),
            "pos-token" => match lines.next() {
                Some(Ok(token)) => answer(pos_token_lines(runner.store_pos_token(&token))),
                _ => answer(pos_token_lines(Err(BoxError::BadToken("empty")))),
            },
            "pos" => answer(pos_lines(&runner.pos_view(), runner.status())),
            "notes" => answer(notes_lines(&runner.notes())),
            "quit" => break,
            "" => {}
            other => answer(unknown_lines(other)),
        }
    }
    runner.stop();
    Ok(())
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
        "badwelcome" => {
            // A device that holds no invitation: garbage in both places.
            match connect(arg(2)?, None)?.welcome(&[7; 101], &[8; 50]) {
                Ok(()) => return Err("ACCEPTED a stranger's welcome".into()),
                Err(e) => println!("{}", refused(&e)),
            }
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
        "fetch" => {
            // LK-28: a removed device reads its leaving mailbox without making one or acknowledging.
            let name = arg(3)?;
            let me = identity(name)?;
            let mut c = connect(arg(2)?, None)?;
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
            let _ = last;
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
        "notice" => {
            // A counter's paid notice, sealed to the Box. The notice's own
            // number and the message's number are kept apart.
            let (from, index) = (arg(3)?, arg(4)?);
            let state = match arg(5)? {
                "opened" => NoticeState::Opened,
                "paid" => NoticeState::Paid,
                "unpaid" => NoticeState::Unpaid,
                "final" => NoticeState::Final,
                other => return Err(format!("unknown state {other}")),
            };
            let cents: u64 = arg(6)?.parse().map_err(|_| "cents is a number")?;
            let claim = read_claim(&dir)?;
            let who = identity(from)?;
            let now = now_secs();
            // A sale keeps the time it opened at in every notice about it.
            let opened_file = dir.join(format!("opened-{from}-{index}"));
            let opened_at: u64 = match fs::read_to_string(&opened_file) {
                Ok(t) => t.trim().parse().map_err(|_| "a damaged opening time")?,
                Err(_) => {
                    fs::write(&opened_file, now.to_string()).map_err(|e| e.to_string())?;
                    now
                }
            };
            let draft = NoticeDraft {
                index: index.parse().map_err(|_| "index is a number")?,
                amount: Amount(cents),
                currency: usd()?,
                opened_at,
                state,
                state_at: now,
                till_sale: None,
            };
            let number = next_number(&dir, from, "notice")?;
            let notice = PaidNotice::sign(&who, number, &draft).ok_or("the notice was not made")?;
            let n = next_number(&dir, from, "box")?;
            let sealed = seal(
                &who,
                role_of(from)?,
                &claim.box_key,
                Role::Box,
                n,
                Kind::PaidNotice,
                &notice.to_bytes(),
                &mut rng,
            )
            .map_err(|e| e.to_string())?;
            connect(arg(2)?, Some(&claim.box_key))?
                .put(&sealed)
                .map_err(|e| refused(&e))?;
            println!("notice {number} sent: sale {index} {}", state.name());
        }
        "posrecv" => {
            let claim = read_claim(&dir)?;
            let me = identity("owner")?;
            let mut c = connect(arg(2)?, None)?;
            c.make_mailbox(&me).map_err(|e| refused(&e))?;
            let got = c.fetch(&me, 0).map_err(|e| refused(&e))?;
            let box_key = claim.box_key;
            let mut last = 0;
            for (id, sealed) in &got.items {
                let opened = open(
                    &me,
                    Role::Owner,
                    sealed,
                    |d| (*d == box_key).then_some(Role::Box),
                    |_| 0,
                )
                .map_err(|e| e.to_string())?;
                if opened.kind == Kind::PosNotice {
                    let n = PosNotice::from_bytes(&opened.content).map_err(|e| e.to_string())?;
                    let ok = n.from == box_key && n.verify();
                    println!(
                        "pos notice {} from the Box ({}): {}",
                        n.number,
                        if ok {
                            "signature good"
                        } else {
                            "SIGNATURE BAD"
                        },
                        n.text()
                    );
                }
                last = *id;
            }
            if last > 0 {
                c.ack(&me, last).map_err(|e| refused(&e))?;
            }
            println!("received {}", got.items.len());
        }
        "pos-order" => {
            // `dir` is the test POS's directory; the order gets the time now.
            let (id, state, cents, tender) = (arg(2)?, arg(3)?, arg(4)?, arg(5)?);
            if !matches!(state, "open" | "closed") || !matches!(tender, "bitcoin" | "none") {
                return Err("state is open or closed, tender is bitcoin or none".into());
            }
            let cents: u64 = cents.parse().map_err(|_| "cents is a number")?;
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let mut f = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("orders.txt"))
                .map_err(|e| e.to_string())?;
            writeln!(f, "{id} {state} {cents} {} {tender}", now_secs())
                .map_err(|e| e.to_string())?;
            println!("order {id} put in the test POS");
        }
        "pos-close-on" => {
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            fs::write(dir.join("can-close"), "").map_err(|e| e.to_string())?;
            println!("the test POS can close orders");
        }
        "pos-box" => return pos_box(&dir, arg(2)?, Path::new(arg(3)?)),
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
