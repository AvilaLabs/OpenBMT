//! The Box as a program.
//!
//! ```text
//! openbmt-box <data dir> <bind addr> [--invitation-file <path>] [--no-console]
//! ```
//!
//! It prints its key's four fingerprint words and its key in hex, then the
//! address it listens on. An invitation (BX-12) is read once from the file
//! named by `--invitation-file`, which is then deleted, or typed on standard
//! input as `invite <text>`. Other commands on standard input: `state`
//! (unpaired, invitation open, paired or turned off), `members`, `notes` and
//! `quit`. For the POS (BX-41): `pos-token` reads the POS sign-in from the
//! next line of standard input and stores it, and `pos` prints the state of
//! the POS side, never the token. `pos-setup` reads the next line of standard
//! input as the shop's POS vendor (BX-49) and stores it; the Box reads that
//! vendor's POS over the system's `curl` from its next start. The line is
//! `key=value` words separated by `|`: `vendor` (`square` or `clover`),
//! `location` (Square's location id or Clover's merchant id), `currency`,
//! `tenders` (the exact names of the Bitcoin tenders, separated by `;`) and,
//! if needed, `curl` (a path), `socks` (the Tor proxy, `host:port`) and `host`
//! (`https://` and a name). The program runs until the end of standard input
//! or `quit`. Without a `pos-setup` it reads no POS and keeps the counters'
//! notices no further than the Box's files.
//!
//! With `--no-console` the Box reads no standard input (a service has none)
//! and an invitation comes only from `--invitation-file`. It runs until the
//! process is ended by a signal. It installs no handler: SIGTERM and even
//! SIGKILL leave nothing half written, because every file is replaced whole
//! or appended with a tail that is cut at the next start (BX-52).

use std::io::{self, BufRead, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use openbmt_box::say::{
    invite_lines, members_lines, notes_lines, pos_lines, pos_setup_lines, pos_token_lines, render,
    state_lines, unknown_lines, ConsoleAnswer,
};
use openbmt_box::{read_invitation_file, BoxError, BoxRunner, PosConfigError};
use openbmt_link::ServerConfig;
use openbmt_net_io::SystemClock;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: openbmt-box <data dir> <bind addr> [--invitation-file <path>] [--no-console]"
    );
    ExitCode::from(2)
}

fn say(line: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn say_all(lines: Vec<String>) {
    for line in lines {
        say(&line);
    }
}

fn bad_arguments() -> ExitCode {
    complain(render(&ConsoleAnswer::BadArguments.words()));
    usage()
}

/// An answer for the error stream: the words, as they are given on the
/// output.
fn complain(lines: Vec<String>) {
    for line in lines {
        eprintln!("{line}");
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(bind)) = (args.next(), args.next()) else {
        return bad_arguments();
    };
    let mut invitation_file: Option<PathBuf> = None;
    let mut no_console = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--invitation-file" => match args.next() {
                Some(path) => invitation_file = Some(PathBuf::from(path)),
                None => return bad_arguments(),
            },
            "--no-console" => no_console = true,
            _ => return bad_arguments(),
        }
    }
    let Ok(bind_addr) = bind.parse::<SocketAddr>() else {
        complain(render(&ConsoleAnswer::BadAddress(bind).words()));
        return usage();
    };
    let runner = match BoxRunner::start(
        dir,
        bind_addr,
        Arc::new(SystemClock),
        ServerConfig::default(),
    ) {
        Ok(r) => r,
        Err(e) => {
            complain(render(&e.words()));
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
                Ok(()) => say_all(invite_lines(&Ok(()))),
                // The same words as the invite command gives.
                Err(e) => complain(invite_lines(&Err(e))),
            },
            Err(e) => complain(render(
                &ConsoleAnswer::InvitationFileUnreadable {
                    path: path.display().to_string(),
                    kind: e.kind(),
                }
                .words(),
            )),
        }
    }
    say_all(state_lines(runner.status()));
    if no_console {
        // The runner's threads do the work; this one waits to be ended.
        loop {
            std::thread::park();
        }
    }
    let mut lines = io::stdin().lock().lines();
    while let Some(line) = lines.next() {
        let Ok(line) = line else { break };
        let line = line.trim();
        let (command, rest) = line.split_once(' ').unwrap_or((line, ""));
        match command {
            "invite" => say_all(invite_lines(&runner.enter_invitation(rest.trim()))),
            "state" => say_all(state_lines(runner.status())),
            "pos-token" => match lines.next() {
                Some(Ok(token)) => say_all(pos_token_lines(runner.store_pos_token(&token))),
                // No line after the command is an empty token.
                _ => say_all(pos_token_lines(Err(BoxError::BadToken("empty")))),
            },
            "pos-setup" => match lines.next() {
                Some(Ok(setup)) => say_all(pos_setup_lines(&runner.store_pos_config(&setup))),
                // No line after the command is a setup with no words.
                _ => say_all(pos_setup_lines(&Err(PosConfigError::Missing("vendor")))),
            },
            "pos" => say_all(pos_lines(&runner.pos_view(), runner.status())),
            "members" => say_all(members_lines(&runner.members())),
            "notes" => say_all(notes_lines(&runner.notes())),
            "quit" => break,
            "" => {}
            other => say_all(unknown_lines(other)),
        }
    }
    runner.stop();
    ExitCode::SUCCESS
}
