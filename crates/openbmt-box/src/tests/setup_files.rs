//! BX-64: the files a host without a console sets the Box up with.

use super::kit::*;
use super::runner::{pair_ok, start};
use crate::files::BoxFiles;
use crate::setup_files::{take_setup_files, till_file_listen};
use crate::state::POS_TOKEN_FILE;
use crate::{DirFiles, POS_CONFIG_FILE};

const SETUP: &str = "vendor=square|location=LOCSQ1|currency=USD|tenders=Bitcoin";

fn exists(dir: &TempDir, name: &str) -> bool {
    dir.path().join(name).exists()
}

fn say(lines: &[String], line: &str) -> bool {
    lines.iter().any(|l| l == line)
}

#[test]
fn bx_64_pos_setup_txt_is_stored_as_the_console_stores_it_and_deleted() {
    let dir = TempDir::new("sf-setup");
    let (r, _clock) = start(&dir);
    std::fs::write(dir.path().join("pos-setup.txt"), format!("{SETUP}\n")).unwrap();
    let lines = take_setup_files(&r, dir.path());
    assert!(say(&lines, "setup file: pos-setup.txt taken and deleted"));
    assert!(say(&lines, "pos-setup: square"), "{lines:?}");
    assert!(!exists(&dir, "pos-setup.txt"));
    assert!(!exists(&dir, "pos-setup.txt.refused"));
    // The same bytes the console's command stores.
    let by_file = DirFiles::new(dir.path()).read(POS_CONFIG_FILE).unwrap();
    r.store_pos_config(SETUP).unwrap();
    assert_eq!(
        DirFiles::new(dir.path()).read(POS_CONFIG_FILE).unwrap(),
        by_file
    );
    assert!(by_file.is_some());
    r.stop();
}

#[test]
fn bx_64_a_pos_setup_txt_that_is_refused_is_renamed_and_nothing_is_stored() {
    let dir = TempDir::new("sf-setup-bad");
    let (r, _clock) = start(&dir);
    std::fs::write(dir.path().join("pos-setup.txt"), "vendor=nope\n").unwrap();
    let lines = take_setup_files(&r, dir.path());
    assert!(say(
        &lines,
        "setup file: pos-setup.txt refused, renamed pos-setup.txt.refused"
    ));
    assert!(lines.iter().any(|l| l.contains("nope")), "{lines:?}");
    assert!(!exists(&dir, "pos-setup.txt"));
    assert!(exists(&dir, "pos-setup.txt.refused"));
    assert!(!exists(&dir, POS_CONFIG_FILE));
    // Text that is not text is refused the same way.
    std::fs::write(dir.path().join("pos-setup.txt"), [0xff, 0xfe, b'\n']).unwrap();
    let lines = take_setup_files(&r, dir.path());
    assert!(say(
        &lines,
        "setup file: pos-setup.txt refused, renamed pos-setup.txt.refused"
    ));
    assert!(!exists(&dir, POS_CONFIG_FILE));
    r.stop();
}

#[test]
fn bx_64_pos_token_txt_is_stored_as_the_console_stores_it_and_deleted() {
    let dir = TempDir::new("sf-token");
    let (r, _clock) = start(&dir);
    pair_ok(&r);
    std::fs::write(dir.path().join("pos-token.txt"), "pos-token-abc\r\n").unwrap();
    let lines = take_setup_files(&r, dir.path());
    assert!(say(&lines, "setup file: pos-token.txt taken and deleted"));
    assert!(!exists(&dir, "pos-token.txt"));
    assert!(!exists(&dir, "pos-token.txt.refused"));
    let stored = DirFiles::new(dir.path()).read(POS_TOKEN_FILE).unwrap();
    assert_eq!(stored.as_deref(), Some(&b"pos-token-abc"[..]));
    // The token is never said.
    assert!(lines.iter().all(|l| !l.contains("pos-token-abc")));
    r.stop();
}

#[test]
fn bx_64_pos_token_txt_on_an_unpaired_box_or_with_a_bad_token_is_refused() {
    let dir = TempDir::new("sf-token-bad");
    let (r, _clock) = start(&dir);
    std::fs::write(dir.path().join("pos-token.txt"), "pos-token-abc\n").unwrap();
    let lines = take_setup_files(&r, dir.path());
    assert!(say(
        &lines,
        "setup file: pos-token.txt refused, renamed pos-token.txt.refused"
    ));
    assert!(!exists(&dir, "pos-token.txt"));
    assert!(exists(&dir, "pos-token.txt.refused"));
    assert!(!exists(&dir, POS_TOKEN_FILE));
    // Paired, a token with a line break inside is still refused.
    pair_ok(&r);
    std::fs::write(dir.path().join("pos-token.txt"), "two\nlines\n").unwrap();
    let lines = take_setup_files(&r, dir.path());
    assert!(say(
        &lines,
        "setup file: pos-token.txt refused, renamed pos-token.txt.refused"
    ));
    assert!(!exists(&dir, POS_TOKEN_FILE));
    r.stop();
}

#[test]
fn bx_64_no_setup_file_says_nothing_and_changes_nothing() {
    let dir = TempDir::new("sf-none");
    let (r, _clock) = start(&dir);
    assert!(take_setup_files(&r, dir.path()).is_empty());
    assert!(!exists(&dir, POS_CONFIG_FILE));
    r.stop();
}

#[test]
fn bx_64_till_txt_names_the_listen_address_and_is_not_changed() {
    let dir = TempDir::new("sf-till");
    assert_eq!(till_file_listen(dir.path()), None);
    std::fs::write(
        dir.path().join("till.txt"),
        "# the till\nlisten=[::]:8091\n",
    )
    .unwrap();
    assert_eq!(till_file_listen(dir.path()).as_deref(), Some("[::]:8091"));
    std::fs::write(dir.path().join("till.txt"), "other=1\n").unwrap();
    assert_eq!(till_file_listen(dir.path()), None);
    assert!(exists(&dir, "till.txt"));
}
