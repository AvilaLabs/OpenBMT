//! The counter's own state: the "desk" (`OBMG`, CR-04, CR-14, CR-21, CR-40).
//!
//! The desk holds what is not a sale, a ledger or a batch: when addresses
//! were handed out in the last hour, whether the cancel alarm is up, the
//! number of the last owner command taken, the number of the last owner
//! heartbeat taken (CR-54), and the overrides inside the last 24 hours.
//!
//! ```text
//! OBMG | 1 | last_command_seq u64 | alarm flag
//!   | n_times u32 | n_times * u64            (non-decreasing)
//!   | n_overrides u32 | n_overrides * (role u8 | name_len u8 | name | at u64 | sats u64)
//!                                            (at non-decreasing)
//!
//! OBMG | 2 | last_command_seq u64 | alarm flag | last_heartbeat_seq u64
//!   | the rest as in version 1
//! ```
//!
//! Version 2 is written only when a heartbeat has been taken, and its
//! `last_heartbeat_seq` is then above zero. A desk that has taken none is
//! written as version 1, which is what every earlier build wrote and what
//! this one reads as "no heartbeat taken". Each desk has one spelling, so a
//! version 2 form with a zero number does not read back.

use openbmt_core::wire::{self, DecodeError, Reader};

use crate::{MAX_ADDRESSES_PER_HOUR, MAX_NAME_BYTES, MAX_OVERRIDE_ENTRIES, OVERRIDE_WINDOW_SECS};

/// The version byte of a desk that holds a heartbeat number (CR-54).
const DESK_VERSION_WITH_HEARTBEAT: u8 = 2;

/// Seconds in the window over which new addresses are counted (CR-04).
pub const ADDRESS_WINDOW_SECS: u64 = 3_600;

/// Who overrides (CR-21). The caller says who the person is and has already
/// checked the PIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// A cashier: held to the per-sale limit and the daily cap.
    Cashier = 1,
    /// A manager: held to the ceiling.
    Manager = 2,
}

impl Role {
    /// Read a role byte.
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Cashier),
            2 => Some(Self::Manager),
            _ => None,
        }
    }

    /// The word for the role.
    pub fn text(&self) -> &'static str {
        match self {
            Self::Cashier => "cashier",
            Self::Manager => "manager",
        }
    }
}

/// Whether a name may be written down: not empty and at most
/// [`MAX_NAME_BYTES`] bytes.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES
}

/// One override inside the window (CR-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideEntry {
    /// Who, by role.
    pub role: Role,
    /// Who, by name.
    pub name: String,
    /// When.
    pub at: u64,
    /// How much was put at risk, in satoshis.
    pub sats: u64,
}

/// The counter's own state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desk {
    address_times: Vec<u64>,
    alarm: bool,
    last_command_seq: u64,
    last_heartbeat_seq: u64,
    overrides: Vec<OverrideEntry>,
}

impl Desk {
    /// A new desk: no addresses handed out, no alarm, no command taken.
    pub fn new() -> Self {
        Self::default()
    }

    /// CR-14: the cancel alarm is up.
    pub fn alarm(&self) -> bool {
        self.alarm
    }

    /// The number of the last owner command taken (0 if none).
    pub fn last_command_seq(&self) -> u64 {
        self.last_command_seq
    }

    /// CR-54: the number of the last owner heartbeat taken (0 if none).
    pub fn last_heartbeat_seq(&self) -> u64 {
        self.last_heartbeat_seq
    }

    /// The overrides kept, oldest first.
    pub fn overrides(&self) -> &[OverrideEntry] {
        &self.overrides
    }

    /// Addresses handed out inside the hour before `now`.
    pub fn addresses_in_hour(&self, now: u64) -> usize {
        self.address_times
            .iter()
            .filter(|t| now.saturating_sub(**t) < ADDRESS_WINDOW_SECS)
            .count()
    }

    /// Sats a person has put at risk by override inside the window before
    /// `now` (CR-21). The sum saturates.
    pub fn override_total(&self, role: Role, name: &str, now: u64) -> u64 {
        self.overrides
            .iter()
            .filter(|e| e.role == role && e.name == name && in_override_window(e.at, now))
            .fold(0u64, |sum, e| sum.saturating_add(e.sats))
    }

    pub(crate) fn set_alarm(&mut self, alarm: bool) {
        self.alarm = alarm;
    }

    pub(crate) fn set_last_command_seq(&mut self, seq: u64) {
        self.last_command_seq = seq;
    }

    pub(crate) fn set_last_heartbeat_seq(&mut self, seq: u64) {
        self.last_heartbeat_seq = seq;
    }

    /// A copy with the address times and overrides older than their windows
    /// dropped.
    pub(crate) fn pruned(&self, now: u64) -> Self {
        let mut next = self.clone();
        next.address_times
            .retain(|t| now.saturating_sub(*t) < ADDRESS_WINDOW_SECS);
        next.overrides.retain(|e| in_override_window(e.at, now));
        next
    }

    /// Note that an address was handed out at `now`. The list stays in
    /// order even if the clock went backwards.
    pub(crate) fn push_address_time(&mut self, now: u64) {
        let at = self.address_times.partition_point(|t| *t <= now);
        self.address_times.insert(at, now);
    }

    /// Note an override. The list stays in order of time.
    pub(crate) fn push_override(&mut self, entry: OverrideEntry) {
        let at = self.overrides.partition_point(|e| e.at <= entry.at);
        self.overrides.insert(at, entry);
    }

    /// Whether another override fits.
    pub(crate) fn has_room_for_override(&self) -> bool {
        self.overrides.len() < MAX_OVERRIDE_ENTRIES
    }

    /// Whether another address may be handed out inside the hour (CR-04).
    pub(crate) fn has_room_for_address(&self) -> bool {
        self.address_times.len() < MAX_ADDRESSES_PER_HOUR
    }

    /// The stored form (see the module docs). Lengths are bounded by the
    /// caps, so the `u32` and `u8` fields cannot overflow.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = wire::start(
            &wire::MAGIC_COUNTER_DESK,
            25 + self.address_times.len() * 8 + self.overrides.len() * 40,
        );
        out.extend_from_slice(&self.last_command_seq.to_le_bytes());
        out.push(u8::from(self.alarm));
        if self.last_heartbeat_seq != 0 {
            if let Some(version) = out.get_mut(4) {
                *version = DESK_VERSION_WITH_HEARTBEAT;
            }
            out.extend_from_slice(&self.last_heartbeat_seq.to_le_bytes());
        }
        out.extend_from_slice(&len32(self.address_times.len()).to_le_bytes());
        for t in &self.address_times {
            out.extend_from_slice(&t.to_le_bytes());
        }
        out.extend_from_slice(&len32(self.overrides.len()).to_le_bytes());
        for e in &self.overrides {
            out.push(e.role as u8);
            out.push(u8::try_from(e.name.len()).unwrap_or(u8::MAX));
            out.extend_from_slice(e.name.as_bytes());
            out.extend_from_slice(&e.at.to_le_bytes());
            out.extend_from_slice(&e.sats.to_le_bytes());
        }
        out
    }

    /// Read the stored form strictly (see `openbmt_core::wire`). Besides the
    /// framing it checks what the live code guarantees: at most
    /// [`MAX_ADDRESSES_PER_HOUR`] times and [`MAX_OVERRIDE_ENTRIES`]
    /// overrides, times in order, a known role, and a name that is valid text
    /// of 1 to [`MAX_NAME_BYTES`] bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        // `Reader::framed` reads only version 1, and this form has two.
        let mut r = Reader::new(bytes);
        if r.take(4)? != wire::MAGIC_COUNTER_DESK {
            return Err(DecodeError::BadMagic);
        }
        let version = r.u8()?;
        if version != wire::VERSION && version != DESK_VERSION_WITH_HEARTBEAT {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        let last_command_seq = r.u64_le()?;
        let alarm = r.flag()?;
        let last_heartbeat_seq = if version == DESK_VERSION_WITH_HEARTBEAT {
            match r.u64_le()? {
                0 => {
                    return Err(DecodeError::NonCanonical(
                        "version 2 holds a heartbeat number of zero",
                    ))
                }
                n => n,
            }
        } else {
            0
        };
        let n = r.count(MAX_ADDRESSES_PER_HOUR, 8)?;
        let mut address_times = Vec::with_capacity(n);
        for _ in 0..n {
            let t = r.u64_le()?;
            if address_times.last().is_some_and(|prev| *prev > t) {
                return Err(DecodeError::NonCanonical("address times are not in order"));
            }
            address_times.push(t);
        }
        let n = r.count(MAX_OVERRIDE_ENTRIES, 19)?;
        let mut overrides: Vec<OverrideEntry> = Vec::with_capacity(n);
        for _ in 0..n {
            let role = Role::from_u8(r.u8()?)
                .ok_or(DecodeError::NonCanonical("an override role is not known"))?;
            let name_len = usize::from(r.u8()?);
            if name_len == 0 || name_len > MAX_NAME_BYTES {
                return Err(DecodeError::NonCanonical(
                    "an override name has a bad length",
                ));
            }
            let name = std::str::from_utf8(r.take(name_len)?)
                .map_err(|_| DecodeError::NonCanonical("an override name is not text"))?
                .to_string();
            let at = r.u64_le()?;
            let sats = r.u64_le()?;
            if overrides.last().is_some_and(|prev| prev.at > at) {
                return Err(DecodeError::NonCanonical("overrides are not in order"));
            }
            overrides.push(OverrideEntry {
                role,
                name,
                at,
                sats,
            });
        }
        r.finish()?;
        Ok(Self {
            address_times,
            alarm,
            last_command_seq,
            last_heartbeat_seq,
            overrides,
        })
    }
}

/// Whether an override made at `at` still counts at `now`. An entry stamped
/// in the future (the clock went back) counts.
pub fn in_override_window(at: u64, now: u64) -> bool {
    now.saturating_sub(at) < OVERRIDE_WINDOW_SECS
}

/// A length as the `u32` the stored forms carry. Every list is far below the
/// bound, so the saturation is never reached.
pub(crate) fn len32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
