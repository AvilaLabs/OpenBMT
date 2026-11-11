//! What the Box keeps about the shop's POS vendor (BX-49,
//! [rules](../../../docs/BOX-RULES.md) section 4).
//!
//! [`PosConfig`] is the Box's own file `pos-config`, written whole (BX-50) and
//! read strictly: the vendor, the vendor's location or merchant id, the
//! currency, the exact names of the Bitcoin tenders, and three optional
//! things (the path of curl, the Tor proxy and the vendor's host). It holds
//! no token: that is `pos.token` (BX-41).
//!
//! ```text
//! "OBMo" | 1 | vendor (1) | currency (3)
//!        | location (u16 length, bytes)
//!        | tenders (1 count, then each u16 length, bytes)
//!        | curl path | Tor proxy | host     (each u16 length, bytes; 0 = none)
//! ```
//!
//! The console command `pos-setup` takes one line of `key=value` words
//! separated by `|` and makes a [`PosConfig`] of it.

use std::fmt;

use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_counter::Currency;
use openbmt_pos::{
    LocationCounters, Settings as PosSettings, SettingsError, Text, MAX_TENDER_NAMES,
};
use openbmt_pos_readers::clover::MAX_MERCHANT_BYTES;

use crate::https::{DEFAULT_CURL, DEFAULT_SOCKS, MAX_TEXT_BYTES};

/// The file, in the Box's data directory.
pub const POS_CONFIG_FILE: &str = "pos-config";

const MAGIC_POS_CONFIG: [u8; 4] = *b"OBMo";

/// The vendor's own host for Square.
pub const SQUARE_HOST: &str = "https://connect.squareup.com";
/// The vendor's own host for Clover (North America).
pub const CLOVER_HOST: &str = "https://api.clover.com";

/// The POS vendors the Box can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    /// Square (read-only: `docs/DESIGN.md` section 5.6).
    Square,
    /// Clover.
    Clover,
}

impl Vendor {
    /// The word the shop types.
    pub fn word(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Clover => "clover",
        }
    }

    fn to_byte(self) -> u8 {
        match self {
            Self::Square => 1,
            Self::Clover => 2,
        }
    }

    fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Square),
            2 => Some(Self::Clover),
            _ => None,
        }
    }

    /// The host the vendor's API is at when the config names none.
    pub fn default_host(self) -> &'static str {
        match self {
            Self::Square => SQUARE_HOST,
            Self::Clover => CLOVER_HOST,
        }
    }
}

/// Why a POS config was not taken, or did not read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosConfigError {
    /// The text has a word that is not `key=value`, a key it does not know,
    /// or a key twice.
    BadWord(String),
    /// A key the Box needs is not there.
    Missing(&'static str),
    /// The vendor is not `square` or `clover`.
    UnknownVendor(String),
    /// A value is not valid, with the key.
    BadValue(&'static str),
    /// The stored file does not read back.
    Damaged(String),
    /// The values make POS settings that are not valid.
    Settings(SettingsError),
    /// The file could not be written.
    Io(std::io::ErrorKind),
}

impl fmt::Display for PosConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadWord(w) => write!(f, "BX-49: \"{w}\" is not a key=value word the Box knows"),
            Self::Missing(k) => write!(f, "BX-49: the key {k} is missing"),
            Self::UnknownVendor(v) => {
                write!(
                    f,
                    "BX-49: \"{v}\" is not a vendor the Box reads (square or clover)"
                )
            }
            Self::BadValue(k) => write!(f, "BX-49: the value of {k} is not valid"),
            Self::Damaged(why) => write!(f, "BX-49: {POS_CONFIG_FILE} does not read back: {why}"),
            Self::Settings(e) => write!(f, "{e}"),
            Self::Io(kind) => write!(f, "BX-50: {POS_CONFIG_FILE} could not be written: {kind}"),
        }
    }
}

impl std::error::Error for PosConfigError {}

/// The shop's POS vendor, as the Box keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PosConfig {
    /// The vendor.
    pub vendor: Vendor,
    /// Square's location id, or Clover's merchant id.
    pub location: String,
    /// The currency of the shop's sales.
    pub currency: Currency,
    /// The exact names of the Bitcoin tenders (PS-10).
    pub tenders: Vec<String>,
    /// The path of curl; none means [`DEFAULT_CURL`] from `PATH`.
    pub curl: Option<String>,
    /// The Tor proxy, `host:port`; none means [`DEFAULT_SOCKS`].
    pub socks: Option<String>,
    /// The vendor's host, `https://` and a name; none means the vendor's own.
    pub host: Option<String>,
}

fn printable(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control)
}

fn host_ok(host: &str) -> bool {
    host.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() <= 253
            && rest
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
    })
}

fn socks_ok(proxy: &str) -> bool {
    proxy.rsplit_once(':').is_some_and(|(host, port)| {
        !host.is_empty()
            && host.len() <= 253
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
            && port.parse::<u16>().is_ok_and(|p| p > 0)
    })
}

impl PosConfig {
    /// Check every value: what [`PosConfig::parse`] and the file's reader
    /// both end with.
    pub fn validate(&self) -> Result<(), PosConfigError> {
        let location_ok = match self.vendor {
            Vendor::Square => printable(&self.location, openbmt_pos::MAX_TEXT_BYTES),
            Vendor::Clover => {
                !self.location.is_empty()
                    && self.location.len() <= MAX_MERCHANT_BYTES
                    && self.location.bytes().all(|b| b.is_ascii_alphanumeric())
            }
        };
        if !location_ok {
            return Err(PosConfigError::BadValue("location"));
        }
        if self.tenders.is_empty()
            || self.tenders.len() > MAX_TENDER_NAMES
            || self
                .tenders
                .iter()
                .any(|t| t.trim().is_empty() || Text::new(t).is_err())
        {
            return Err(PosConfigError::BadValue("tenders"));
        }
        if let Some(curl) = &self.curl {
            if !printable(curl, MAX_TEXT_BYTES) {
                return Err(PosConfigError::BadValue("curl"));
            }
        }
        if self.socks.as_deref().is_some_and(|s| !socks_ok(s)) {
            return Err(PosConfigError::BadValue("socks"));
        }
        if self.host.as_deref().is_some_and(|h| !host_ok(h)) {
            return Err(PosConfigError::BadValue("host"));
        }
        self.settings(Vec::new()).map(|_| ())
    }

    /// Read the one line of `key=value` words `pos-setup` takes: `vendor`,
    /// `location`, `currency` and `tenders` (names separated by `;`) are
    /// needed; `curl`, `socks` and `host` may be given. The words are
    /// separated by `|`.
    pub fn parse(line: &str) -> Result<Self, PosConfigError> {
        let mut vendor = None;
        let mut location = None;
        let mut currency = None;
        let mut tenders = None;
        let mut curl = None;
        let mut socks = None;
        let mut host = None;
        for word in line.trim().split('|') {
            let (key, value) = word
                .split_once('=')
                .ok_or_else(|| PosConfigError::BadWord(word.trim().to_string()))?;
            let (key, value) = (key.trim(), value.trim());
            let slot = match key {
                "vendor" => &mut vendor,
                "location" => &mut location,
                "currency" => &mut currency,
                "tenders" => &mut tenders,
                "curl" => &mut curl,
                "socks" => &mut socks,
                "host" => &mut host,
                _ => return Err(PosConfigError::BadWord(word.trim().to_string())),
            };
            if slot.replace(value.to_string()).is_some() {
                return Err(PosConfigError::BadWord(word.trim().to_string()));
            }
        }
        let vendor = match vendor.ok_or(PosConfigError::Missing("vendor"))?.as_str() {
            "square" => Vendor::Square,
            "clover" => Vendor::Clover,
            other => return Err(PosConfigError::UnknownVendor(other.to_string())),
        };
        let currency = currency.ok_or(PosConfigError::Missing("currency"))?;
        let code: [u8; 3] = currency
            .as_bytes()
            .try_into()
            .map_err(|_| PosConfigError::BadValue("currency"))?;
        let config = Self {
            vendor,
            location: location.ok_or(PosConfigError::Missing("location"))?,
            currency: Currency::new(code).ok_or(PosConfigError::BadValue("currency"))?,
            tenders: tenders
                .ok_or(PosConfigError::Missing("tenders"))?
                .split(';')
                .map(|t| t.trim().to_string())
                .collect(),
            curl,
            socks,
            host,
        };
        config.validate()?;
        Ok(config)
    }

    /// The path curl is run from.
    pub fn curl_path(&self) -> &str {
        self.curl.as_deref().unwrap_or(DEFAULT_CURL)
    }

    /// The Tor proxy that is used when Tor is on.
    pub fn socks_addr(&self) -> &str {
        self.socks.as_deref().unwrap_or(DEFAULT_SOCKS)
    }

    /// The vendor's host.
    pub fn host_url(&self) -> &str {
        self.host
            .as_deref()
            .unwrap_or_else(|| self.vendor.default_host())
    }

    /// The POS settings of `openbmt-pos` these values make, with `counters`
    /// serving the one location (BX-49).
    pub fn settings(
        &self,
        counters: Vec<openbmt_core::identity::DeviceId>,
    ) -> Result<PosSettings, PosConfigError> {
        let text = |t: &str| Text::new(t).map_err(|_| PosConfigError::BadValue("tenders"));
        let tenders = self
            .tenders
            .iter()
            .map(|t| text(t))
            .collect::<Result<Vec<_>, _>>()?;
        let location =
            Text::new(&self.location).map_err(|_| PosConfigError::BadValue("location"))?;
        PosSettings::new(
            tenders,
            vec![LocationCounters { location, counters }],
            vec![self.currency],
        )
        .map_err(PosConfigError::Settings)
    }

    /// The bytes of the file.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_POS_CONFIG, 64 + self.location.len());
        out.push(self.vendor.to_byte());
        out.extend_from_slice(&self.currency.code());
        put_text(&mut out, &self.location);
        out.push(u8::try_from(self.tenders.len()).unwrap_or(u8::MAX));
        for t in &self.tenders {
            put_text(&mut out, t);
        }
        for opt in [&self.curl, &self.socks, &self.host] {
            put_text(&mut out, opt.as_deref().unwrap_or(""));
        }
        out
    }

    /// Read the file back: strict, and every value checked as a typed one
    /// is.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PosConfigError> {
        let damaged = |e: DecodeError| PosConfigError::Damaged(e.to_string());
        let mut r = Reader::framed(bytes, &MAGIC_POS_CONFIG).map_err(damaged)?;
        let vendor = Vendor::from_byte(r.u8().map_err(damaged)?)
            .ok_or_else(|| PosConfigError::Damaged("the vendor is not known".into()))?;
        let currency = Currency::new(r.array::<3>().map_err(damaged)?)
            .ok_or_else(|| PosConfigError::Damaged("the currency is not valid".into()))?;
        let location = get_text(&mut r).map_err(damaged)?;
        let count = usize::from(r.u8().map_err(damaged)?);
        if count > MAX_TENDER_NAMES {
            return Err(PosConfigError::Damaged("too many tender names".into()));
        }
        let mut tenders = Vec::with_capacity(count);
        for _ in 0..count {
            tenders.push(get_text(&mut r).map_err(damaged)?);
        }
        let mut opts = Vec::with_capacity(3);
        for _ in 0..3 {
            let t = get_text(&mut r).map_err(damaged)?;
            opts.push((!t.is_empty()).then_some(t));
        }
        r.finish().map_err(damaged)?;
        let mut opts = opts.into_iter();
        let config = Self {
            vendor,
            location,
            currency,
            tenders,
            curl: opts.next().flatten(),
            socks: opts.next().flatten(),
            host: opts.next().flatten(),
        };
        config
            .validate()
            .map_err(|e| PosConfigError::Damaged(e.to_string()))?;
        // Strict: what reads is what would be written.
        if config.to_bytes() != bytes {
            return Err(PosConfigError::Damaged(
                "it is not in its canonical form".into(),
            ));
        }
        Ok(config)
    }
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    let len = u16::try_from(bytes.len()).unwrap_or(u16::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes.get(..usize::from(len)).unwrap_or(bytes));
}

fn get_text(r: &mut Reader<'_>) -> Result<String, DecodeError> {
    let len = usize::from(r.u16_le()?);
    String::from_utf8(r.take(len)?.to_vec())
        .map_err(|_| DecodeError::NonCanonical("a text is not UTF-8"))
}
