//! BX-64: setting the Box up with files in its data folder, for a host
//! system that has no console.
//!
//! At start, after the data folder is read, [`take_setup_files`] reads and
//! then removes `pos-setup.txt` and `pos-token.txt`. Each holds what the
//! console command of the same name takes and is stored the same way. A file
//! that is refused is renamed with `.refused` at the end, and the words are
//! the console's. [`till_file_listen`] reads `till.txt`, which is kept.

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::pos_config::PosConfigError;
use crate::runner::BoxRunner;
use crate::say::{pos_setup_lines, pos_token_lines};
use crate::state::BoxError;

/// The file that holds what `pos-setup` takes.
pub const POS_SETUP_FILE_TXT: &str = "pos-setup.txt";
/// The file that holds what `pos-token` takes.
pub const POS_TOKEN_FILE_TXT: &str = "pos-token.txt";
/// The file that names the till listener's address; it is kept.
pub const TILL_FILE_TXT: &str = "till.txt";
/// The end of the name a refused file is given.
pub const REFUSED_END: &str = ".refused";

/// The most a setup file is read for; a longer one is refused as a token or
/// a setup line would be.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// The text of a file, with a final line break cut off, or `None` when it
/// is not text.
fn read_text(path: &Path) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_FILE_BYTES)
        .read_to_end(&mut bytes)?;
    Ok(String::from_utf8(bytes).ok().map(|mut text| {
        if text.ends_with('\n') {
            text.pop();
            if text.ends_with('\r') {
                text.pop();
            }
        }
        text
    }))
}

/// Read and remove one file. `store` takes its text and gives the console's
/// lines and whether it was taken. A file that is refused is renamed.
fn take_one(
    dir: &Path,
    name: &str,
    not_text: impl FnOnce() -> Vec<String>,
    store: impl FnOnce(&str) -> (Vec<String>, bool),
) -> Vec<String> {
    let path = dir.join(name);
    if !path.is_file() {
        return Vec::new();
    }
    let (lines, taken) = match read_text(&path) {
        Ok(Some(text)) => store(&text),
        Ok(None) => (not_text(), false),
        Err(e) => (
            vec![format!(
                "setup file: {name} could not be read ({})",
                e.kind()
            )],
            false,
        ),
    };
    let mut out = Vec::new();
    if taken {
        match fs::remove_file(&path) {
            Ok(()) => out.push(format!("setup file: {name} taken and deleted")),
            Err(e) => out.push(format!(
                "setup file: {name} taken, but it could not be deleted ({})",
                e.kind()
            )),
        }
    } else {
        let refused = dir.join(format!("{name}{REFUSED_END}"));
        match fs::rename(&path, &refused) {
            Ok(()) => out.push(format!(
                "setup file: {name} refused, renamed {name}{REFUSED_END}"
            )),
            Err(e) => out.push(format!(
                "setup file: {name} refused, and it could not be renamed ({})",
                e.kind()
            )),
        }
    }
    out.extend(lines);
    out
}

/// Read and remove `pos-setup.txt` and `pos-token.txt` in `dir` (BX-64) and
/// store what they hold as the console does (BX-49, BX-41). The lines are
/// what the Box says about each, in the console's words.
pub fn take_setup_files(runner: &BoxRunner, dir: &Path) -> Vec<String> {
    let mut lines = take_one(
        dir,
        POS_SETUP_FILE_TXT,
        || pos_setup_lines(&Err(PosConfigError::BadWord("(not text)".into()))),
        |text| {
            let result = runner.store_pos_config(text);
            (pos_setup_lines(&result), result.is_ok())
        },
    );
    lines.extend(take_one(
        dir,
        POS_TOKEN_FILE_TXT,
        || pos_token_lines(Err(BoxError::BadToken("not text"))),
        |text| {
            let result = runner.store_pos_token(text);
            let taken = result.is_ok();
            (pos_token_lines(result), taken)
        },
    ));
    lines
}

/// The address `till.txt` names with `listen=<addr:port>`, if the file is
/// there and has the word. The file is not changed.
pub fn till_file_listen(dir: &Path) -> Option<String> {
    let text = read_text(&dir.join(TILL_FILE_TXT)).ok().flatten()?;
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("listen="))
        .map(|addr| addr.trim().to_string())
        .next()
}
