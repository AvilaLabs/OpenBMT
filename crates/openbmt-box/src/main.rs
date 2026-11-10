//! The Box as a program.
//!
//! ```text
//! openbmt-box <data dir> <bind addr> [--invitation-file <path>]
//! ```
//!
//! It prints its key's four fingerprint words and its key in hex, then the
//! address it listens on. An invitation (BX-12) is read once from the file
//! named by `--invitation-file`, which is then deleted, or typed on standard
//! input as `invite <text>`. Other commands on standard input: `state`
//! (unpaired, invitation open, paired or turned off), `members`, `notes` and
//! `quit`. The program runs until the end of standard input, `quit` or
//! SIGTERM. There is no POS code here yet.

use std::io::{self, BufRead, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use openbmt_box::{read_invitation_file, BoxRunner};
use openbmt_link::ServerConfig;
use openbmt_net_io::SystemClock;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn usage() -> ExitCode {
    eprintln!("usage: openbmt-box <data dir> <bind addr> [--invitation-file <path>]");
    ExitCode::from(2)
}

fn say(line: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(bind)) = (args.next(), args.next()) else {
        return usage();
    };
    let mut invitation_file: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match (arg.as_str(), args.next()) {
            ("--invitation-file", Some(path)) => invitation_file = Some(PathBuf::from(path)),
            _ => return usage(),
        }
    }
    let Ok(bind) = bind.parse::<SocketAddr>() else {
        eprintln!("{bind} is not an address and port");
        return usage();
    };
    let runner = match BoxRunner::start(dir, bind, Arc::new(SystemClock), ServerConfig::default()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("openbmt-box: {e}");
            return ExitCode::FAILURE;
        }
    };
    say(&format!(
        "fingerprint: {}",
        runner.fingerprint_words().join(" ")
    ));
    say(&format!("key: {}", hex(&runner.device_id().to_bytes())));
    say(&format!("listening: {}", runner.local_addr()));
    if let Some(path) = invitation_file {
        match read_invitation_file(&path) {
            Ok(text) => match runner.enter_invitation(&text) {
                Ok(()) => say("invitation: open for 30 minutes"),
                Err(e) => eprintln!("openbmt-box: the invitation was refused: {e}"),
            },
            Err(e) => eprintln!("openbmt-box: {}: {e}", path.display()),
        }
    }
    say(&format!("state: {}", runner.status().text()));
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
        match command {
            "invite" => match runner.enter_invitation(rest.trim()) {
                Ok(()) => say("invitation: open for 30 minutes"),
                Err(e) => say(&format!("invitation refused: {e}")),
            },
            "state" => say(&format!("state: {}", runner.status().text())),
            "members" => {
                let members = runner.members();
                say(&format!("members: {}", members.len()));
                for m in members {
                    say(&format!("member: {}", hex(&m.to_bytes())));
                }
            }
            "notes" => {
                for n in runner.notes() {
                    say(&format!("note: {n}"));
                }
            }
            "quit" => break,
            "" => {}
            other => say(&format!("unknown command: {other}")),
        }
    }
    runner.stop();
    ExitCode::SUCCESS
}
