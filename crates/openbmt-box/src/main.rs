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
//! `quit`. For the POS (BX-41): `pos-token` reads the POS sign-in from the
//! next line of standard input and stores it, and `pos` prints the state of
//! the POS side, never the token. The program runs until the end of standard
//! input, `quit` or SIGTERM. It holds no vendor's connector, so on its own it
//! takes the counters' notices no further than the Box's files: a connector
//! is added by the program that links the library.

use std::io::{self, BufRead, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use openbmt_box::say::{
    invite_lines, members_lines, notes_lines, pos_lines, pos_token_lines, render, state_lines,
    unknown_lines, ConsoleAnswer,
};
use openbmt_box::{read_invitation_file, BoxError, BoxRunner};
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
    while let Some(arg) = args.next() {
        match (arg.as_str(), args.next()) {
            ("--invitation-file", Some(path)) => invitation_file = Some(PathBuf::from(path)),
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
