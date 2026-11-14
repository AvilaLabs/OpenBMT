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
//!
//! OBMG | 3 | last_command_seq u64 | alarm flag | last_heartbeat_seq u64
//!   | last_refund_no u64 | the rest as in version 1
//!
//! OBMG | 4 | last_command_seq u64 | alarm flag | last_heartbeat_seq u64
//!   | last_refund_no u64 | cashiers_listed u32
//!   | n_times u32 | n_times * u64            (non-decreasing)
//!   | n_overrides u32
//!   | n_overrides * (role u8 | name_len u8 | name | at u64 | sats u64 | height u32)
//!                                            (at non-decreasing; height 0 is unknown)
//! ```
//!
//! Version 2 is written only when a heartbeat has been taken, and its
//! `last_heartbeat_seq` is then above zero. A desk that has taken none is
//! written as version 1, which is what every earlier build wrote and what
//! this one reads as "no heartbeat taken". Version 3 is written only once a
//! refund request has been made (CR-61, CR-73): `last_refund_no` is then above
//! zero, and `last_heartbeat_seq` may be zero. Each desk has one spelling, so
//! a version 2 form with a zero number and a version 3 form with a zero
//! request number do not read back.
//!
//! Version 4 is written only when the number of cashiers listed (CR-146) is
//! above zero or an override holds the chain height the counter knew when it
//! was made (CR-21): the heartbeat and request numbers may then be zero. An
//! earlier form reads with no cashier listed and every height unknown. A build
//! from before this one refuses a version 4.

use openbmt_core::wire::{self, DecodeError, Reader};

use crate::refund::CAP_WINDOW_BLOCKS;
use crate::{MAX_ADDRESSES_PER_HOUR, MAX_NAME_BYTES, MAX_OVERRIDE_ENTRIES, OVERRIDE_WINDOW_SECS};

/// The version byte of a desk that holds a heartbeat number (CR-54).
const DESK_VERSION_WITH_HEARTBEAT: u8 = 2;
/// The version byte of a desk that holds a refund request number (CR-73).
const DESK_VERSION_WITH_REFUNDS: u8 = 3;
/// The version byte of a desk that holds the number of cashiers listed and the
/// chain height of each override (CR-21, CR-146).
const DESK_VERSION_WITH_STAFF: u8 = 4;

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
    /// The chain height the counter knew when the override was made, when it
    /// knew one (CR-21).
    pub height: Option<u32>,
}

/// The counter's own state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desk {
    address_times: Vec<u64>,
    alarm: bool,
    last_command_seq: u64,
    last_heartbeat_seq: u64,
    last_refund_no: u64,
    cashiers_listed: u32,
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

    /// CR-61: the number of the last refund request made (0 if none).
    pub fn last_refund_no(&self) -> u64 {
        self.last_refund_no
    }

    /// CR-146: how many cashiers are listed on the counter (0 if none). With
    /// none, a cashier's daily caps are shared by every name.
    pub fn cashiers_listed(&self) -> u32 {
        self.cashiers_listed
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

    /// Sats put at risk by override inside the window before `now` (CR-21).
    /// The window is a day by the clock, and also a day of blocks when the
    /// heights are known (as CR-120 has it for refunds). A manager is counted
    /// by name. A cashier is counted by name while cashiers are listed, and
    /// as one bucket of every cashier entry when none is (CR-146), so that a
    /// new name does not bring a new cap. The sum saturates.
    pub fn override_total(&self, role: Role, name: &str, now: u64, now_height: Option<u32>) -> u64 {
        let by_role = role == Role::Cashier && self.cashiers_listed == 0;
        self.overrides
            .iter()
            .filter(|e| e.role == role && (by_role || e.name == name))
            .filter(|e| counts_in_window(e.at, e.height, now, now_height))
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

    pub(crate) fn set_last_refund_no(&mut self, no: u64) {
        self.last_refund_no = no;
    }

    pub(crate) fn set_cashiers_listed(&mut self, n: u32) {
        self.cashiers_listed = n;
    }

    /// A copy with the address times older than their window dropped. The
    /// overrides are left as they are.
    pub(crate) fn pruned_addresses(&self, now: u64) -> Self {
        let mut next = self.clone();
        next.address_times
            .retain(|t| now.saturating_sub(*t) < ADDRESS_WINDOW_SECS);
        next
    }

    /// A copy with the overrides older than their window dropped, the chain
    /// being at `now_height` (see [`Desk::override_total`]).
    pub(crate) fn pruned_overrides(&self, now: u64, now_height: Option<u32>) -> Self {
        let mut next = self.clone();
        next.overrides
            .retain(|e| counts_in_window(e.at, e.height, now, now_height));
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
            41 + self.address_times.len() * 8 + self.overrides.len() * 44,
        );
        out.extend_from_slice(&self.last_command_seq.to_le_bytes());
        out.push(u8::from(self.alarm));
        let staff_form =
            self.cashiers_listed != 0 || self.overrides.iter().any(|e| e.height.is_some());
        if staff_form || self.last_heartbeat_seq != 0 || self.last_refund_no != 0 {
            let version = if staff_form {
                DESK_VERSION_WITH_STAFF
            } else if self.last_refund_no != 0 {
                DESK_VERSION_WITH_REFUNDS
            } else {
                DESK_VERSION_WITH_HEARTBEAT
            };
            if let Some(byte) = out.get_mut(4) {
                *byte = version;
            }
            out.extend_from_slice(&self.last_heartbeat_seq.to_le_bytes());
            if staff_form || self.last_refund_no != 0 {
                out.extend_from_slice(&self.last_refund_no.to_le_bytes());
            }
            if staff_form {
                out.extend_from_slice(&self.cashiers_listed.to_le_bytes());
            }
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
            if staff_form {
                out.extend_from_slice(&e.height.unwrap_or(0).to_le_bytes());
            }
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
        if !matches!(
            version,
            wire::VERSION
                | DESK_VERSION_WITH_HEARTBEAT
                | DESK_VERSION_WITH_REFUNDS
                | DESK_VERSION_WITH_STAFF
        ) {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        let last_command_seq = r.u64_le()?;
        let alarm = r.flag()?;
        let last_heartbeat_seq = match version {
            DESK_VERSION_WITH_HEARTBEAT => match r.u64_le()? {
                0 => {
                    return Err(DecodeError::NonCanonical(
                        "version 2 holds a heartbeat number of zero",
                    ))
                }
                n => n,
            },
            DESK_VERSION_WITH_REFUNDS | DESK_VERSION_WITH_STAFF => r.u64_le()?,
            _ => 0,
        };
        let last_refund_no = match version {
            DESK_VERSION_WITH_REFUNDS => match r.u64_le()? {
                0 => {
                    return Err(DecodeError::NonCanonical(
                        "version 3 holds a request number of zero",
                    ))
                }
                n => n,
            },
            DESK_VERSION_WITH_STAFF => r.u64_le()?,
            _ => 0,
        };
        let cashiers_listed = if version == DESK_VERSION_WITH_STAFF {
            r.u32_le()?
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
        let mut any_height = false;
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
            let height = if version == DESK_VERSION_WITH_STAFF {
                match r.u32_le()? {
                    0 => None,
                    h => Some(h),
                }
            } else {
                None
            };
            any_height |= height.is_some();
            if overrides.last().is_some_and(|prev| prev.at > at) {
                return Err(DecodeError::NonCanonical("overrides are not in order"));
            }
            overrides.push(OverrideEntry {
                role,
                name,
                at,
                sats,
                height,
            });
        }
        r.finish()?;
        if version == DESK_VERSION_WITH_STAFF && cashiers_listed == 0 && !any_height {
            return Err(DecodeError::NonCanonical(
                "version 4 holds no cashier and no height",
            ));
        }
        Ok(Self {
            address_times,
            alarm,
            last_command_seq,
            last_heartbeat_seq,
            last_refund_no,
            cashiers_listed,
            overrides,
        })
    }
}

/// Whether an override made at `at` still counts at `now`. An entry stamped
/// in the future (the clock went back) counts.
pub fn in_override_window(at: u64, now: u64) -> bool {
    now.saturating_sub(at) < OVERRIDE_WINDOW_SECS
}

/// Whether an entry made at `at`, with the chain at `height` then, still
/// counts at `now` with the chain at `now_height` (CR-21, CR-120): it is from
/// the last 24 hours by the clock, or its time is later than the clock, or it
/// was made fewer than [`CAP_WINDOW_BLOCKS`] blocks ago when both heights are
/// known.
pub fn counts_in_window(at: u64, height: Option<u32>, now: u64, now_height: Option<u32>) -> bool {
    in_override_window(at, now)
        || at > now
        || matches!(
            (height, now_height),
            (Some(then), Some(now_h)) if now_h.saturating_sub(then) < CAP_WINDOW_BLOCKS
        )
}

/// A length as the `u32` the stored forms carry. Every list is far below the
/// bound, so the saturation is never reached.
pub(crate) fn len32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
