//! BX-64: the files a host without a console sets the Box up with.

use std::net::SocketAddr;
use std::sync::Arc;

use openbmt_link::ServerConfig;
use openbmt_net_io::Clock;

use super::kit::*;
use super::runner::{pair_ok, start, start_in, TestClock};
use crate::files::BoxFiles;
use crate::setup_files::{take_setup_files, till_file_listen};
use crate::state::POS_TOKEN_FILE;
use crate::SetupStore;
use crate::{BoxRunner, DirFiles, PosView, Reading, POS_CONFIG_FILE};

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
    assert!(say(&lines, "setup file: pos-token.txt refused and deleted"));
    assert!(!exists(&dir, "pos-token.txt"));
    assert!(!exists(&dir, "pos-token.txt.refused"));
    assert_eq!(files_holding(&dir, b"pos-token-abc"), Vec::<String>::new());
    assert!(!exists(&dir, POS_TOKEN_FILE));
    // Paired, a token with a line break inside is still refused.
    pair_ok(&r);
    std::fs::write(dir.path().join("pos-token.txt"), "two\nlines\n").unwrap();
    let lines = take_setup_files(&r, dir.path());
    assert!(say(&lines, "setup file: pos-token.txt refused and deleted"));
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

/// The names of the files under `dir`, at any depth, that hold `needle`.
fn files_holding(dir: &TempDir, needle: &[u8]) -> Vec<String> {
    fn walk(path: &std::path::Path, needle: &[u8], out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, needle, out);
            } else if std::fs::read(&p).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle))
            {
                out.push(p.display().to_string());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir.path(), needle, &mut out);
    out
}

/// A Box started in `dir` with the setup files taken as it starts, and what
/// it said.
fn start_taking(dir: &TempDir, clock: &Arc<TestClock>) -> (BoxRunner, Vec<String>) {
    let data = dir.path().to_path_buf();
    BoxRunner::start_taking(
        dir.path(),
        "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        Arc::clone(clock) as Arc<dyn Clock + Send + Sync>,
        ServerConfig::default(),
        |store| take_setup_files(store, &data),
    )
    .unwrap()
}

#[test]
fn bx_64_a_pos_setup_txt_takes_effect_at_the_start_that_finds_it() {
    let dir = TempDir::new("sf-first-setup");
    std::fs::write(dir.path().join("pos-setup.txt"), format!("{SETUP}\n")).unwrap();
    let clock = TestClock::new(NOW);
    let (r, lines) = start_taking(&dir, &clock);
    assert!(say(&lines, "setup file: pos-setup.txt taken and deleted"));
    assert!(say(&lines, "pos-setup: square"), "{lines:?}");
    assert!(!exists(&dir, "pos-setup.txt"));
    // The POS side is there at this start, with no second one.
    match r.pos_view() {
        PosView::State(s) => assert_eq!(s.reading, Reading::Stopped),
        other => panic!("{other:?}"),
    }
    r.stop();
}

#[test]
fn bx_64_a_pos_token_txt_is_stored_at_the_start_that_finds_it() {
    let dir = TempDir::new("sf-first-token");
    let clock = TestClock::new(NOW);
    let r = start_in(&dir, &clock).unwrap();
    pair_ok(&r);
    r.stop();
    std::fs::write(dir.path().join("pos-token.txt"), "pos-token-abc\n").unwrap();
    std::fs::write(
        dir.path().join("pos-setup.txt"),
        format!("{SETUP}|curl=/nonexistent/curl\n"),
    )
    .unwrap();
    let (r, lines) = start_taking(&dir, &clock);
    assert!(say(&lines, "setup file: pos-token.txt taken and deleted"));
    assert!(say(&lines, "setup file: pos-setup.txt taken and deleted"));
    assert!(lines.iter().all(|l| !l.contains("pos-token-abc")));
    assert!(!exists(&dir, "pos-token.txt"));
    let stored = DirFiles::new(dir.path()).read(POS_TOKEN_FILE).unwrap();
    assert_eq!(stored.as_deref(), Some(&b"pos-token-abc"[..]));
    assert!(matches!(r.pos_view(), PosView::State(_)));
    r.stop();
}

#[test]
fn bx_64_a_refused_setup_file_at_start_is_renamed_and_the_pos_stays_off() {
    let dir = TempDir::new("sf-first-bad");
    std::fs::write(dir.path().join("pos-setup.txt"), "vendor=nope\n").unwrap();
    std::fs::write(dir.path().join("pos-token.txt"), "pos-token-abc\n").unwrap();
    let clock = TestClock::new(NOW);
    let (r, lines) = start_taking(&dir, &clock);
    assert!(say(
        &lines,
        "setup file: pos-setup.txt refused, renamed pos-setup.txt.refused"
    ));
    assert!(say(&lines, "setup file: pos-token.txt refused and deleted"));
    assert!(exists(&dir, "pos-setup.txt.refused"));
    assert!(!exists(&dir, "pos-token.txt.refused"));
    assert!(!exists(&dir, "pos-token.txt"));
    assert_eq!(files_holding(&dir, b"pos-token-abc"), Vec::<String>::new());
    assert!(!exists(&dir, POS_CONFIG_FILE));
    assert_eq!(r.pos_view(), PosView::NotInstalled);
    r.stop();
}

#[test]
fn bx_64_a_taken_file_that_cannot_be_deleted_is_said_and_not_applied_again() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("sf-nodelete");
    let (r, _clock) = start(&dir);
    pair_ok(&r);
    let file = dir.path().join("pos-setup.txt");
    std::fs::write(&file, format!("{SETUP}\n")).unwrap();
    // A store that, once it has stored, makes the folder one that cannot be
    // written to, so that the delete is refused.
    struct Locking<'a>(&'a BoxRunner, &'a std::path::Path);
    impl SetupStore for Locking<'_> {
        fn store_pos_token(&self, token: &str) -> Result<(), crate::BoxError> {
            self.0.store_pos_token(token)
        }
        fn store_pos_config(&self, line: &str) -> Result<crate::PosConfig, crate::PosConfigError> {
            let config = self.0.store_pos_config(line);
            std::fs::set_permissions(self.1, std::fs::Permissions::from_mode(0o500)).unwrap();
            config
        }
    }
    let lines = take_setup_files(&Locking(&r, dir.path()), dir.path());
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("setup file: pos-setup.txt taken, but it could not be deleted")),
        "{lines:?}"
    );
    // The change the console makes afterwards is not undone at the next start.
    r.store_pos_config("vendor=clover|location=LOCCL1|currency=USD|tenders=Bitcoin")
        .unwrap();
    let kept = DirFiles::new(dir.path()).read(POS_CONFIG_FILE).unwrap();
    let again = take_setup_files(&r, dir.path());
    assert!(again.is_empty(), "{again:?}");
    assert!(!exists(&dir, "pos-setup.txt"));
    assert_eq!(
        DirFiles::new(dir.path()).read(POS_CONFIG_FILE).unwrap(),
        kept
    );
    r.stop();
}
