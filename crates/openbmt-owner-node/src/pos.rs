//! The owner's own matching of the till's orders to the counters' sales
//! (`docs/OWNER-COINS-RULES.md`, OC-30 to OC-37).
//!
//! The owner phone reads the till (the shop's POS) with a read-only sign-in
//! of its own and runs the matcher of `openbmt-pos` itself, so that what the
//! Box says about the till is never the only account (`docs/DESIGN.md`
//! 5.3). The pieces:
//!
//! - **The orders** come from the till's system through two steps the app
//!   makes: [`OwnerPos::request`] gives the method, address and body of the
//!   next page, and [`OwnerPos::reply`] takes the status and the bytes of
//!   the answer. The sending and the sign-in are the app's. This module has
//!   no network and never holds the sign-in.
//! - **The sales** come from the owner's verified copies of the counters'
//!   records ([`counter_facts`]), never from the Box's notices and never from
//!   what a counter says about itself outside its signed records.
//! - **What is kept** is the owner's own settings for the till
//!   ([`PosSetup`], `pos-setup.bin`, `OBMo`) and the ledger of the matcher
//!   (`pos-ledger.bin`, `OBMZ`). Each is written whole through the
//!   watcher's atomic write before it takes effect, and each is read back
//!   strictly. A file that does not read back leaves the till out of use
//!   until the owner removes the connection: the ledger is never dropped on
//!   its own.
//!
//! The signed settings of the shop carry no till setting, so the owner's own
//! are used: one vendor, one location (Square's location id, or Clover's
//! merchant id), the exact names of the Bitcoin tender and the currencies. All
//! the counters of the link serve that one location.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use openbmt_core::identity::DeviceId;
use openbmt_core::records::Chain;
use openbmt_core::say::{Tone, Who, Words};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::Currency;
use openbmt_courier::Post;
use openbmt_pos::{
    facts_from_chain, Alert, AlertKey, Cursor, Delta, FactsError, Ledger, LedgerError,
    LedgerWriteError, LocationCounters, ModelError, SaleFact, Settings, SettingsError, Summary,
    Text, MAX_COUNTERS_PER_LOCATION, MAX_CURRENCIES, MAX_PAGES, MAX_TENDER_NAMES,
};
use openbmt_pos_readers::{clover, square, ReadRefused, RequestRefused};

pub use openbmt_pos::{
    AlertKind, Id as PosId, SaleKey as PosSaleKey, Subject as PosSubject, TenderKey,
};

use crate::link::OwnerLink;
use crate::watcher::write_file;

/// The owner's settings for the till, in the node's folder.
pub const POS_SETUP_FILE: &str = "pos-setup.bin";
/// The matcher's ledger, in the node's folder.
pub const POS_LEDGER_FILE: &str = "pos-ledger.bin";
/// Magic of the owner's till settings (OC-31).
pub const MAGIC_POS_SETUP: [u8; 4] = *b"OBMo";
/// OC-33: orders asked for in one page.
pub const POS_PAGE_ORDERS: usize = 100;
/// OC-34: fewest seconds between the starts of two reads of the till.
pub const POS_MIN_POLL_SECS: u64 = 60;
/// OC-33: where Square's orders are read. **[verify, E5]**
pub const SQUARE_HOST: &str = "https://connect.squareup.com";
/// OC-33: where Clover's orders are read. **[verify, E6]**
pub const CLOVER_HOST: &str = "https://api.clover.com";
/// The largest ledger or settings file read back, in bytes.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Which vendor's till the owner reads (OC-31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosVendor {
    /// Square, read-only (`docs/DESIGN.md` 5.6).
    Square,
    /// Clover.
    Clover,
}

impl PosVendor {
    fn to_u8(self) -> u8 {
        match self {
            Self::Square => 1,
            Self::Clover => 2,
        }
    }

    fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Square),
            2 => Some(Self::Clover),
            _ => None,
        }
    }
}

/// How the app sends a request (OC-33).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosMethod {
    /// A GET with no body.
    Get,
    /// A POST of a JSON body.
    Post,
}

/// What the app sends for one page (OC-33). The sign-in is the app's to add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosRequest {
    /// The method.
    pub method: PosMethod,
    /// The whole address, with its query.
    pub url: String,
    /// The body; empty for a GET.
    pub body: Vec<u8>,
}

/// The owner's settings for the till (OC-31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosSetup {
    /// The vendor.
    pub vendor: PosVendor,
    /// Square's location id, or Clover's merchant id.
    pub location: Text,
    /// The exact names of the Bitcoin tender (PS-10), 1 to 8.
    pub tender_names: Vec<Text>,
    /// The shop's currencies (PS-13), 1 to 4.
    pub currencies: Vec<Currency>,
}

impl PosSetup {
    /// Settings from what the owner typed. Refused unless every bound of
    /// PS-03, PS-10 and PS-13 holds, and, for Clover, the merchant id is 1 to
    /// 32 letters and digits (PS-97).
    pub fn from_text(
        vendor: PosVendor,
        location: &str,
        tender_names: &[String],
        currencies: &[String],
    ) -> Result<Self, PosRefused> {
        let location = Text::new(location.trim()).map_err(PosRefused::Model)?;
        let names = tender_names
            .iter()
            .map(|n| Text::new(n.trim()).map_err(PosRefused::Model))
            .collect::<Result<Vec<_>, _>>()?;
        let mut codes = Vec::new();
        for c in currencies {
            let bytes: [u8; 3] = c
                .trim()
                .as_bytes()
                .try_into()
                .map_err(|_| PosRefused::Setup(SettingsError::Currencies))?;
            codes.push(Currency::new(bytes).ok_or(PosRefused::Setup(SettingsError::Currencies))?);
        }
        Self::new(vendor, location, names, codes)
    }

    /// Settings from checked values.
    pub fn new(
        vendor: PosVendor,
        location: Text,
        tender_names: Vec<Text>,
        currencies: Vec<Currency>,
    ) -> Result<Self, PosRefused> {
        let setup = Self {
            vendor,
            location,
            tender_names,
            currencies,
        };
        let settings = setup.settings(&[]).map_err(PosRefused::Setup)?;
        if vendor == PosVendor::Clover {
            clover::Request::new(
                setup.location.as_str(),
                &settings,
                POS_PAGE_ORDERS,
                &Cursor::start(),
            )
            .map_err(PosRefused::Request)?;
        }
        Ok(setup)
    }

    /// The matcher's settings: this one location, served by `counters`
    /// (OC-31), with the standard timing.
    pub fn settings(&self, counters: &[DeviceId]) -> Result<Settings, SettingsError> {
        Settings::new(
            self.tender_names.clone(),
            vec![LocationCounters {
                location: self.location.clone(),
                counters: counters.to_vec(),
            }],
            self.currencies.clone(),
        )
    }

    /// The stored form: `OBMo`, version 1, the vendor, the location, the
    /// names and the currencies.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_POS_SETUP, 64);
        out.push(self.vendor.to_u8());
        put_text(&mut out, &self.location);
        out.push(u8::try_from(self.tender_names.len()).unwrap_or(u8::MAX));
        for n in &self.tender_names {
            put_text(&mut out, n);
        }
        out.push(u8::try_from(self.currencies.len()).unwrap_or(u8::MAX));
        for c in &self.currencies {
            out.extend_from_slice(&c.code());
        }
        out
    }

    /// Read the stored form. Strict: the exact length, the bounds of the
    /// settings, and the bytes that are accepted encode back to themselves.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::framed(bytes, &MAGIC_POS_SETUP)?;
        let vendor = PosVendor::from_u8(r.u8()?)
            .ok_or(DecodeError::NonCanonical("the vendor is not known"))?;
        let location = get_text(&mut r)?;
        let n = usize::from(r.u8()?);
        if n == 0 || n > MAX_TENDER_NAMES {
            return Err(DecodeError::NonCanonical("the tender names are out of bound"));
        }
        let mut names = Vec::with_capacity(n);
        for _ in 0..n {
            names.push(get_text(&mut r)?);
        }
        let n = usize::from(r.u8()?);
        if n == 0 || n > MAX_CURRENCIES {
            return Err(DecodeError::NonCanonical("the currencies are out of bound"));
        }
        let mut currencies = Vec::with_capacity(n);
        for _ in 0..n {
            currencies.push(
                Currency::new(r.array::<3>()?)
                    .ok_or(DecodeError::NonCanonical("a currency is not three capitals"))?,
            );
        }
        r.finish()?;
        let setup = Self::new(vendor, location, names, currencies)
            .map_err(|_| DecodeError::NonCanonical("the settings do not hold"))?;
        if setup.to_bytes() != bytes {
            return Err(DecodeError::NonCanonical("the bytes are not the canonical form"));
        }
        Ok(setup)
    }
}

fn put_text(out: &mut Vec<u8>, t: &Text) {
    out.push(u8::try_from(t.as_str().len()).unwrap_or(u8::MAX));
    out.extend_from_slice(t.as_str().as_bytes());
}

fn get_text(r: &mut Reader<'_>) -> Result<Text, DecodeError> {
    let len = usize::from(r.u8()?);
    let s = std::str::from_utf8(r.take(len)?)
        .map_err(|_| DecodeError::NonCanonical("text is not UTF-8"))?;
    Text::new(s).map_err(|_| DecodeError::NonCanonical("a text is not valid"))
}

/// Why the till was not read or changed (OC-30 to OC-37). Every refusal
/// leaves the ledger as it was, except that pages stored before a later one
/// failed stay stored (PS-74).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosRefused {
    /// No till is connected (OC-31).
    NotSetUp,
    /// The link has no counter to match against (OC-30).
    NoCounters,
    /// The till was read less than a minute ago (OC-34).
    TooSoon {
        /// Seconds until it may be read.
        wait_secs: u64,
    },
    /// An answer came that no request was waiting for (OC-33).
    NothingAsked,
    /// The till refused the sign-in (OC-33).
    SignInRefused {
        /// The status the till answered with.
        status: u16,
    },
    /// The till asked to be read less often (OC-33).
    RateLimited,
    /// The till answered with another status than 200 (OC-33).
    Unavailable {
        /// The status.
        status: u16,
    },
    /// The till's answer was refused whole (PS-90 to PS-98).
    Unreadable(ReadRefused),
    /// The request could not be made (PS-94, PS-97).
    Request(RequestRefused),
    /// The till says there is more and does not move on (PS-80).
    NoProgress,
    /// The settings were refused (PS-10, PS-13, PS-14).
    Setup(SettingsError),
    /// A text was refused (PS-03).
    Model(ModelError),
    /// The ledger refused the change (PS-72).
    Ledger(LedgerError),
    /// A counter's records give no sales (OC-30).
    Facts {
        /// The counter.
        counter: DeviceId,
        /// Why.
        error: FactsError,
    },
    /// A file could not be written or removed (OC-32).
    Store(String),
    /// A file does not read back as written (OC-32).
    Corrupt {
        /// The file.
        file: &'static str,
        /// What is wrong with it.
        why: String,
    },
}

impl fmt::Display for PosRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSetUp => write!(f, "OC-31: no till is connected to this phone"),
            Self::NoCounters => write!(
                f,
                "OC-30: the link has no counter, so there are no sales to match"
            ),
            Self::TooSoon { wait_secs } => write!(
                f,
                "OC-34: the till was read less than a minute ago; wait {wait_secs} seconds"
            ),
            Self::NothingAsked => {
                write!(f, "OC-33: an answer came that no request was waiting for")
            }
            Self::SignInRefused { status } => {
                write!(f, "OC-33: the till refused the sign-in (status {status})")
            }
            Self::RateLimited => write!(f, "OC-33: the till asked to be read less often"),
            Self::Unavailable { status } => {
                write!(f, "OC-33: the till answered with status {status}")
            }
            Self::Unreadable(e) => write!(f, "{e}"),
            Self::Request(e) => write!(f, "{e}"),
            Self::NoProgress => write!(f, "PS-80: the till says there is more and does not move on"),
            Self::Setup(e) => write!(f, "{e}"),
            Self::Model(e) => write!(f, "{e}"),
            Self::Ledger(e) => write!(f, "{e}"),
            Self::Facts { error, .. } => write!(f, "OC-30: {error}"),
            Self::Store(e) => write!(f, "OC-32: could not store the till's records: {e}"),
            Self::Corrupt { file, why } => {
                write!(f, "OC-32: {file} does not read back as written: {why}")
            }
        }
    }
}

impl std::error::Error for PosRefused {}

impl PosRefused {
    /// What the owner is told (SC-01). A refusal that wraps another crate's
    /// has that crate's words.
    pub fn words(&self) -> Words {
        match self {
            Self::NotSetUp => Words::new(
                Tone::Info,
                "Till not connected",
                "No till system has been connected to this phone yet, so there is nothing to read.",
                Who::Owner,
                "Connect the till's system on the till screen, then read it again.",
            )
            .code("OC-31"),
            Self::NoCounters => Words::new(
                Tone::Stop,
                "No counter to match against",
                "This phone has no counter in its link, so it has no sales to match the till's orders to.",
                Who::Owner,
                "Set up the link to the counters, then read the till again.",
            )
            .code("OC-30"),
            Self::TooSoon { wait_secs } => Words::new(
                Tone::Info,
                "Till was just read",
                "This phone reads the till at most once a minute, and it was read a moment ago.",
                Who::Nobody,
                "Nothing to do. The till can be read again in a minute.",
            )
            .code("OC-34")
            .detail(format!("wait {wait_secs} seconds")),
            Self::NothingAsked => Words::new(
                Tone::Stop,
                "Till answer not expected",
                "An answer from the till arrived that this phone had not asked for, so it was not used.",
                Who::Owner,
                "Read the till again. If it repeats, report it to the OpenBMT project; see Details.",
            )
            .code("OC-33"),
            Self::SignInRefused { status } => Words::new(
                Tone::Stop,
                "Till refused the sign-in",
                "The till's system refused this phone's read-only sign-in: it was withdrawn or has run out.",
                Who::Owner,
                "Make a new read-only sign-in in the till's system, then connect the till again and paste it.",
            )
            .code("OC-33")
            .detail(format!("status {status}")),
            Self::RateLimited => Words::new(
                Tone::Info,
                "Till asks for a pause",
                "The till's system asked this phone to read from it less often.",
                Who::Nobody,
                "Nothing to do. This phone reads again after a minute.",
            )
            .code("OC-33"),
            Self::Unavailable { status } => Words::new(
                Tone::Stop,
                "Till gave an error",
                "The till's system answered with an error instead of its orders, so nothing was changed.",
                Who::Owner,
                "Try again later. If it repeats, check that the till's system is working.",
            )
            .code("OC-33")
            .detail(format!("status {status}")),
            Self::Unreadable(e) => e.words(),
            Self::Request(e) => e.words(),
            Self::NoProgress => openbmt_pos::SyncError::<String>::NoProgress.words(),
            Self::Setup(e) => e.words(),
            Self::Model(e) => e.words(),
            Self::Ledger(e) => e.words(),
            Self::Facts { error, .. } => error.words(),
            Self::Store(text) => Words::new(
                Tone::Stop,
                "Till records not saved",
                "This phone could not save what it read from the till, so the last change was not kept.",
                Who::Owner,
                "Try again. If it repeats, report it to the OpenBMT project; the system's text is under Details.",
            )
            .code("OC-32")
            .detail(clean(text)),
            Self::Corrupt { why, .. } => Words::new(
                Tone::Stop,
                "Till records unreadable",
                "A file this phone keeps about the till does not read back as it was written, so the till is not read.",
                Who::Owner,
                "Remove the till connection on the till screen and connect it again. The matching then starts over.",
            )
            .code("OC-32")
            .detail(clean(why)),
        }
    }
}

/// System text for the small print: one line, no control character.
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(200)
        .collect()
}

/// One alert that is open, with its words (OC-36, PS-49, PS-61).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAlert {
    /// Its identity (PS-50).
    pub key: AlertKey,
    /// When it was raised.
    pub raised_at: u64,
    /// Why it was raised and what to do next.
    pub words: Words,
}

impl OpenAlert {
    fn of(alert: &Alert) -> Self {
        Self {
            key: alert.key.clone(),
            raised_at: alert.raised_at,
            words: alert.words(),
        }
    }
}

/// What the screen shows of the till (OC-36).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosView {
    /// The connection, if there is one.
    pub setup: Option<PosSetup>,
    /// When a read of the till last finished in this run of the app.
    pub last_poll_at: Option<u64>,
    /// The open alerts, oldest first (PS-61).
    pub open: Vec<OpenAlert>,
    /// The counts (PS-61).
    pub summary: Summary,
}

/// What one reply did (OC-35).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosPolled {
    /// Pages taken in this read so far.
    pub pages: usize,
    /// Orders taken in this read so far.
    pub orders_read: usize,
    /// Another page is wanted now: ask for the next request.
    pub again: bool,
    /// The till has more than this read took (PS-80); the next read goes on.
    pub more_left: bool,
    /// What matching made new. Empty until the last page.
    pub delta: Delta,
}

/// The request that was made and is waiting for its answer.
#[derive(Debug, Clone)]
struct Asked {
    cursor: Cursor,
    settings: Settings,
    vendor: VendorRequest,
}

#[derive(Debug, Clone)]
enum VendorRequest {
    Square(square::Request),
    Clover(clover::Request),
}

/// The owner's matching of the till. See the module docs.
#[derive(Debug)]
pub struct OwnerPos {
    dir: PathBuf,
    setup: Option<PosSetup>,
    ledger: Ledger,
    broken: Option<PosRefused>,
    asked: Option<Asked>,
    in_sync: bool,
    pages: usize,
    orders_read: usize,
    last_started_at: Option<u64>,
    last_poll_at: Option<u64>,
}

fn read_bounded(dir: &Path, name: &'static str) -> Result<Option<Vec<u8>>, PosRefused> {
    let path = dir.join(name);
    match fs::metadata(&path) {
        Ok(m) if m.len() > MAX_FILE_BYTES => Err(PosRefused::Corrupt {
            file: name,
            why: "the file is too large".to_string(),
        }),
        Ok(_) => fs::read(&path)
            .map(Some)
            .map_err(|e| PosRefused::Store(format!("{name}: {e}"))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(PosRefused::Store(format!("{name}: {e}"))),
    }
}

impl OwnerPos {
    /// The till's records in `dir`, read back strictly. A file that does not
    /// read back does not stop the node: every call but [`Self::remove`]
    /// then refuses with the file named (OC-32).
    pub fn open(dir: &Path) -> Self {
        let mut pos = Self {
            dir: dir.to_path_buf(),
            setup: None,
            ledger: Ledger::new(),
            broken: None,
            asked: None,
            in_sync: false,
            pages: 0,
            orders_read: 0,
            last_started_at: None,
            last_poll_at: None,
        };
        if let Err(e) = pos.read_back() {
            pos.setup = None;
            pos.ledger = Ledger::new();
            pos.broken = Some(e);
        }
        pos
    }

    fn read_back(&mut self) -> Result<(), PosRefused> {
        if let Some(bytes) = read_bounded(&self.dir, POS_SETUP_FILE)? {
            self.setup = Some(PosSetup::from_bytes(&bytes).map_err(|e| PosRefused::Corrupt {
                file: POS_SETUP_FILE,
                why: e.to_string(),
            })?);
        }
        if let Some(bytes) = read_bounded(&self.dir, POS_LEDGER_FILE)? {
            self.ledger = Ledger::from_bytes(&bytes).map_err(|e| PosRefused::Corrupt {
                file: POS_LEDGER_FILE,
                why: e.to_string(),
            })?;
        }
        Ok(())
    }

    fn ready(&self) -> Result<(), PosRefused> {
        match &self.broken {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    /// The connection, if there is one.
    pub fn setup(&self) -> Option<&PosSetup> {
        self.setup.as_ref()
    }

    /// The matcher's ledger, for reading.
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// OC-31: connect the till, or change its settings. A change of vendor
    /// or of location starts an empty ledger (the cursor belongs to one
    /// vendor's till); any other change keeps the ledger. The new ledger is
    /// stored before the settings, so a stop between the two leaves the old
    /// settings with a ledger that starts over, which reads the same orders
    /// again and raises nothing twice (PS-74).
    pub fn set_setup(&mut self, setup: PosSetup) -> Result<(), PosRefused> {
        self.ready()?;
        let same_till = self
            .setup
            .as_ref()
            .is_some_and(|s| s.vendor == setup.vendor && s.location == setup.location);
        let fresh = Ledger::new();
        if !same_till && self.ledger != fresh {
            write_file(&self.dir, POS_LEDGER_FILE, &fresh.to_bytes()).map_err(PosRefused::Store)?;
            self.ledger = fresh;
        }
        write_file(&self.dir, POS_SETUP_FILE, &setup.to_bytes()).map_err(PosRefused::Store)?;
        self.setup = Some(setup);
        self.asked = None;
        self.in_sync = false;
        self.pages = 0;
        self.orders_read = 0;
        self.last_poll_at = None;
        Ok(())
    }

    /// OC-31: remove the connection and everything kept about the till. The
    /// only call that works on a till whose files do not read back.
    pub fn remove(&mut self) -> Result<(), PosRefused> {
        for name in [POS_LEDGER_FILE, POS_SETUP_FILE] {
            match fs::remove_file(self.dir.join(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(PosRefused::Store(format!("{name}: {e}"))),
            }
        }
        fs::File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(|e| PosRefused::Store(e.to_string()))?;
        self.setup = None;
        self.ledger = Ledger::new();
        self.broken = None;
        self.asked = None;
        self.in_sync = false;
        self.pages = 0;
        self.orders_read = 0;
        self.last_started_at = None;
        self.last_poll_at = None;
        Ok(())
    }

    /// OC-33, OC-34: the request for the next page. `counters` are the
    /// counters of the link (OC-30), `now` is in seconds. The start of a read
    /// is refused less than [`POS_MIN_POLL_SECS`] after the start of the one
    /// before; the next page of a read that is going on is not.
    pub fn request(&mut self, now: u64, counters: &[DeviceId]) -> Result<PosRequest, PosRefused> {
        self.ready()?;
        let setup = self.setup.clone().ok_or(PosRefused::NotSetUp)?;
        if counters.is_empty() {
            return Err(PosRefused::NoCounters);
        }
        if counters.len() > MAX_COUNTERS_PER_LOCATION {
            return Err(PosRefused::Setup(SettingsError::Locations));
        }
        if !self.in_sync {
            if let Some(t) = self.last_started_at {
                let next = t.saturating_add(POS_MIN_POLL_SECS);
                if now < next {
                    return Err(PosRefused::TooSoon {
                        wait_secs: next - now,
                    });
                }
            }
        }
        let settings = setup.settings(counters).map_err(PosRefused::Setup)?;
        let cursor = self.ledger.cursor().clone();
        let (vendor, request) = match setup.vendor {
            PosVendor::Square => {
                let r = square::Request::new(&settings, POS_PAGE_ORDERS, &cursor)
                    .map_err(PosRefused::Request)?;
                let request = PosRequest {
                    method: PosMethod::Post,
                    url: format!("{SQUARE_HOST}{}", square::SEARCH_PATH),
                    body: r.body().into_bytes(),
                };
                (VendorRequest::Square(r), request)
            }
            PosVendor::Clover => {
                let r = clover::Request::new(
                    setup.location.as_str(),
                    &settings,
                    POS_PAGE_ORDERS,
                    &cursor,
                )
                .map_err(PosRefused::Request)?;
                let request = PosRequest {
                    method: PosMethod::Get,
                    url: format!("{CLOVER_HOST}{}", r.path_and_query()),
                    body: Vec::new(),
                };
                (VendorRequest::Clover(r), request)
            }
        };
        if !self.in_sync {
            self.last_started_at = Some(now);
            self.pages = 0;
            self.orders_read = 0;
        }
        self.in_sync = true;
        self.asked = Some(Asked {
            cursor,
            settings,
            vendor,
        });
        Ok(request)
    }

    /// OC-33 to OC-35: take the till's answer to the request just made. A
    /// page is stored with its cursor as soon as it is read (PS-74); after
    /// the last page the matching is worked out once, on what is stored, and
    /// stored as a last write. `facts` gives the counters' sales and is
    /// called only then. Any refusal ends the read; what was stored stays.
    pub fn reply(
        &mut self,
        status: u16,
        body: &[u8],
        now: u64,
        facts: impl FnOnce() -> Result<Vec<SaleFact>, PosRefused>,
    ) -> Result<PosPolled, PosRefused> {
        self.ready()?;
        let asked = self.asked.take().ok_or(PosRefused::NothingAsked)?;
        let out = self.take_reply(asked, status, body, now, facts);
        if !out.as_ref().is_ok_and(|p| p.again) {
            self.in_sync = false;
            self.pages = 0;
            self.orders_read = 0;
        }
        out
    }

    fn take_reply(
        &mut self,
        asked: Asked,
        status: u16,
        body: &[u8],
        now: u64,
        facts: impl FnOnce() -> Result<Vec<SaleFact>, PosRefused>,
    ) -> Result<PosPolled, PosRefused> {
        match status {
            200 => {}
            401 | 403 => return Err(PosRefused::SignInRefused { status }),
            429 => return Err(PosRefused::RateLimited),
            _ => return Err(PosRefused::Unavailable { status }),
        }
        let page = match &asked.vendor {
            VendorRequest::Square(r) => r.parse(body, None),
            VendorRequest::Clover(r) => r.parse(body),
        }
        .map_err(PosRefused::Unreadable)?;
        if page.more && page.next == asked.cursor {
            return Err(PosRefused::NoProgress);
        }
        let dir = self.dir.clone();
        let persist = |bytes: &[u8]| write_file(&dir, POS_LEDGER_FILE, bytes);
        let read = page.orders.len();
        self.ledger
            .absorb_stored(&page.orders, page.next, &asked.settings, persist)
            .map_err(ledger_refusal)?;
        self.pages += 1;
        self.orders_read += read;
        let more_left = page.more;
        if more_left && self.pages < MAX_PAGES {
            return Ok(PosPolled {
                pages: self.pages,
                orders_read: self.orders_read,
                again: true,
                more_left: true,
                delta: Delta::default(),
            });
        }
        let sales = facts()?;
        let delta = self
            .ledger
            .reconcile_stored(&sales, &asked.settings, now, persist)
            .map_err(ledger_refusal)?;
        self.last_poll_at = Some(now);
        Ok(PosPolled {
            pages: self.pages,
            orders_read: self.orders_read,
            again: false,
            more_left,
            delta,
        })
    }

    /// OC-36, PS-52: the owner acknowledges an open alert. It is a change of
    /// this phone's own ledger, stored before it takes effect. No signed
    /// command is made, so it reaches no other device.
    pub fn acknowledge(&mut self, key: &AlertKey, now: u64) -> Result<(), PosRefused> {
        self.ready()?;
        let dir = self.dir.clone();
        self.ledger
            .acknowledge_stored(key, now, |bytes| write_file(&dir, POS_LEDGER_FILE, bytes))
            .map_err(ledger_refusal)
    }

    /// OC-36, PS-61: what the screen shows.
    pub fn view(&self) -> Result<PosView, PosRefused> {
        self.ready()?;
        Ok(PosView {
            setup: self.setup.clone(),
            last_poll_at: self.last_poll_at,
            open: self.ledger.open_alerts().into_iter().map(OpenAlert::of).collect(),
            summary: self.ledger.summary(),
        })
    }
}

fn ledger_refusal(e: LedgerWriteError<String>) -> PosRefused {
    match e {
        LedgerWriteError::Refused(e) => PosRefused::Ledger(e),
        LedgerWriteError::Persist(e) => PosRefused::Store(e),
    }
}

/// OC-30, PS-05: the sales of every counter of the link, folded from the
/// owner's verified copy of its records. A copy that does not verify, or a
/// record that is not a sale event, gives no sales at all: a partial list
/// would raise alerts for sales that were only missing.
pub fn counter_facts<R: Post>(link: &OwnerLink<R>) -> Result<Vec<SaleFact>, PosRefused> {
    let mut all = Vec::new();
    for counter in link.counters() {
        let Some(copy) = link.copy(counter) else {
            continue;
        };
        let chain = Chain::from_records(copy.records().to_vec());
        let facts = facts_from_chain(&chain, link.devices(), counter).map_err(|error| {
            PosRefused::Facts {
                counter: *counter,
                error,
            }
        })?;
        all.extend(facts);
    }
    Ok(all)
}
