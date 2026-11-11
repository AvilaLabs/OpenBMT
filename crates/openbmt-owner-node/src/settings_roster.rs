//! The devices each pending settings delivery must reach (LK-64, LK-66,
//! LK-76).
//!
//! Settings that remove a device are for that device too, so that it learns
//! it was removed. The list of devices to reach (the counters and Boxes of
//! the settings, and of those before them) is known when the settings are
//! handed to the link, and the app may be closed before every one of them
//! holds them. The file keeps that list with the delivery, so a start after
//! that reaches the same devices and does not count the delivery done while
//! one of them still lacks it.
//!
//! The file is written whole (a temporary file, a flush, a rename, a flush
//! of the directory) before the settings are sent and after every change,
//! and read back strictly: a file that does not read, that is not in the
//! order it is written in or that does not re-encode to the same bytes is
//! refused whole.
//!
//! Layout: the magic `OBM$`, the version byte 1, the number of entries (u32
//! LE), and for each entry the outbox number of the delivery (u32 LE), the
//! settings version (u32 LE), the number of devices (u32 LE) and for each
//! device its key (32 bytes) and a role byte (1 counter, 2 Box). Entries are
//! in the order of the outbox number, with none twice; devices in the order
//! of their keys, with none twice.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use openbmt_core::identity::DeviceId;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_link::Role;

use crate::watcher::write_file;

/// The file under the link's directory.
pub const SETTINGS_ROSTER_FILE: &str = "link-settings-roster";

/// Most deliveries the file holds at once (the most the link carries).
pub const MAX_ROSTER_ENTRIES: usize = 16;

/// Most devices one delivery lists.
pub const MAX_ROSTER_DEVICES: usize = 256;

const MAGIC: [u8; 4] = *b"OBM$";

/// One pending settings delivery and the devices it must reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterEntry {
    /// The outbox number of the delivery.
    pub id: u32,
    /// The version of the settings.
    pub version: u32,
    /// The counters and Boxes that must hold them.
    pub targets: BTreeMap<DeviceId, Role>,
}

/// Why the file could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterError {
    /// The file could not be read or written.
    Io(String),
    /// The file is not in the form it is written in.
    Corrupt(&'static str),
    /// More entries or devices than the file may hold.
    TooMany,
}

impl fmt::Display for RosterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "LK-76: the settings roster: {e}"),
            Self::Corrupt(why) => write!(f, "LK-76: the settings roster file is bad: {why}"),
            Self::TooMany => write!(f, "LK-76: the settings roster holds more than it may"),
        }
    }
}

impl std::error::Error for RosterError {}

fn corrupt(e: DecodeError) -> RosterError {
    match e {
        DecodeError::NonCanonical(why) => RosterError::Corrupt(why),
        DecodeError::TrailingBytes => RosterError::Corrupt("bytes follow the last entry"),
        _ => RosterError::Corrupt("it is cut short or not framed as it is written"),
    }
}

fn role_byte(role: Role) -> Option<u8> {
    match role {
        Role::Counter => Some(1),
        Role::Box => Some(2),
        Role::Owner | Role::Watch => None,
    }
}

/// The encoding of a list of entries, which must be in order. `None` if an
/// entry names a role a roster does not hold.
pub fn encode_roster(entries: &[RosterEntry]) -> Option<Vec<u8>> {
    let mut out = wire::start(&MAGIC, 8 + entries.len() * 64);
    out.extend_from_slice(&wire::len_u32(entries.len()).to_le_bytes());
    for entry in entries {
        out.extend_from_slice(&entry.id.to_le_bytes());
        out.extend_from_slice(&entry.version.to_le_bytes());
        out.extend_from_slice(&wire::len_u32(entry.targets.len()).to_le_bytes());
        // A map is in the order of its keys, which is that of their bytes.
        let mut devices: Vec<(&DeviceId, &Role)> = entry.targets.iter().collect();
        devices.sort_by_key(|(device, _)| device.to_bytes());
        for (device, role) in devices {
            out.extend_from_slice(&device.to_bytes());
            out.push(role_byte(*role)?);
        }
    }
    Some(out)
}

/// Read the file strictly (LK-76).
pub fn decode_roster(bytes: &[u8]) -> Result<Vec<RosterEntry>, RosterError> {
    let mut r = Reader::framed(bytes, &MAGIC).map_err(corrupt)?;
    let n = usize::try_from(r.u32_le().map_err(corrupt)?)
        .map_err(|_| RosterError::Corrupt("too many entries"))?;
    if n > MAX_ROSTER_ENTRIES {
        return Err(RosterError::TooMany);
    }
    let mut entries: Vec<RosterEntry> = Vec::with_capacity(n);
    for _ in 0..n {
        let id = r.u32_le().map_err(corrupt)?;
        let version = r.u32_le().map_err(corrupt)?;
        let count = usize::try_from(r.u32_le().map_err(corrupt)?)
            .map_err(|_| RosterError::Corrupt("too many devices"))?;
        if count > MAX_ROSTER_DEVICES {
            return Err(RosterError::TooMany);
        }
        let mut targets = BTreeMap::new();
        let mut last: Option<[u8; 32]> = None;
        for _ in 0..count {
            let device = r.device_id().map_err(corrupt)?;
            let role = match r.u8().map_err(corrupt)? {
                1 => Role::Counter,
                2 => Role::Box,
                _ => return Err(RosterError::Corrupt("a role is not a counter or a Box")),
            };
            let key = device.to_bytes();
            if last.is_some_and(|l| l >= key) {
                return Err(RosterError::Corrupt(
                    "the devices are not in order, or one is there twice",
                ));
            }
            last = Some(key);
            targets.insert(device, role);
        }
        if entries.last().is_some_and(|e| e.id >= id) {
            return Err(RosterError::Corrupt(
                "the entries are not in order, or one is there twice",
            ));
        }
        entries.push(RosterEntry {
            id,
            version,
            targets,
        });
    }
    r.finish().map_err(corrupt)?;
    if encode_roster(&entries).as_deref() != Some(bytes) {
        return Err(RosterError::Corrupt("it does not encode to the same bytes"));
    }
    Ok(entries)
}

/// The rosters of the pending deliveries, in memory and in their file.
#[derive(Debug)]
pub struct SettingsRoster {
    dir: PathBuf,
    entries: Vec<RosterEntry>,
}

impl SettingsRoster {
    /// Open the file under `dir`; no file is an empty roster.
    pub fn open(dir: &Path) -> Result<Self, RosterError> {
        let entries = match fs::read(dir.join(SETTINGS_ROSTER_FILE)) {
            Ok(bytes) => decode_roster(&bytes)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(RosterError::Io(format!("read: {e}"))),
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            entries,
        })
    }

    /// The entries, in the order of the file.
    pub fn entries(&self) -> &[RosterEntry] {
        &self.entries
    }

    /// The entry of delivery `id`, if it is for settings of `version`.
    pub fn get(&self, id: u32, version: u32) -> Option<&RosterEntry> {
        self.entries
            .iter()
            .find(|e| e.id == id && e.version == version)
    }

    /// Replace the entries with `next`, saved first. What is in memory
    /// changes only if the file was written.
    fn replace(&mut self, mut next: Vec<RosterEntry>) -> Result<(), RosterError> {
        next.sort_by_key(|e| e.id);
        if next.len() > MAX_ROSTER_ENTRIES
            || next.iter().any(|e| e.targets.len() > MAX_ROSTER_DEVICES)
        {
            return Err(RosterError::TooMany);
        }
        let bytes =
            encode_roster(&next).ok_or(RosterError::Corrupt("a role is not a counter or a Box"))?;
        write_file(&self.dir, SETTINGS_ROSTER_FILE, &bytes).map_err(RosterError::Io)?;
        self.entries = next;
        Ok(())
    }

    /// Keep this entry in place of the one for the same delivery. Saved
    /// before it returns, and not written again if nothing changed.
    pub fn set(&mut self, entry: RosterEntry) -> Result<(), RosterError> {
        if self.entries.contains(&entry) {
            return Ok(());
        }
        let mut next: Vec<RosterEntry> = self
            .entries
            .iter()
            .filter(|e| e.id != entry.id)
            .cloned()
            .collect();
        next.push(entry);
        self.replace(next)
    }

    /// Keep only the entries for which `keep` is true. Saved only if one
    /// was dropped.
    pub fn retain(
        &mut self,
        mut keep: impl FnMut(&RosterEntry) -> bool,
    ) -> Result<(), RosterError> {
        let next: Vec<RosterEntry> = self.entries.iter().filter(|e| keep(e)).cloned().collect();
        if next.len() != self.entries.len() {
            self.replace(next)?;
        }
        Ok(())
    }
}
