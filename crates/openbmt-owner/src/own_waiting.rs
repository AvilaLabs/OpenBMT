//! The one change of a shop with its own wallet and a Box that waits for a
//! counter's confirmation (`docs/OWNER-APP-RULES.md` OA-119 to OA-121,
//! `docs/COUNTER-RULES.md` CR-239 to CR-242, `docs/APPROVAL-RULES.md`).
//!
//! Settings of class Loosen that the owner root signs are not put in force
//! on the owner phone: they are kept here, as the shop's one waiting change,
//! sent to the counters, and put in force only when a counter of the
//! settings in force has confirmed them and the confirmation is joined to
//! them as `OBM<`.
//!
//! The file: `signed at` u64 LE, the length of the `OBMS` frame (u32 LE) and
//! the frame of the settings, then the length (u32 LE) and the bytes of the
//! owner state that goes with the change, which are empty when the change
//! adds no counter. A counter that is added brings its address range and its
//! bundle with it: the file is written first, so a start that finds the
//! state without them completes it from the file (OA-119). It is read
//! strictly: what decodes encodes to the same bytes.

use openbmt_core::approval::{classify, confirm_words, ChangeClass, CounterConfirmation};
use openbmt_core::confirmed::ConfirmedSettings;
use openbmt_core::identity::DeviceId;
use openbmt_core::settings::{changed_fields, Change, Settings, SignedSettings};
use openbmt_core::wire::DecodeError;

/// How long a change waits before it is dropped: seven days from when it was
/// signed (OA-121).
pub const WAIT_SECS: u64 = 7 * 24 * 60 * 60;

/// The change that waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingChange {
    /// The settings, signed by the owner root alone.
    pub signed: SignedSettings,
    /// When the owner signed them, in seconds.
    pub signed_at: u64,
    /// The owner state as it is once the counter this change adds is paired
    /// (its range and its bundle), in the state file's own form; empty when
    /// the change adds no counter.
    pub state: Vec<u8>,
}

impl WaitingChange {
    /// The file's bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let frame = self.signed.to_bytes();
        let mut out = self.signed_at.to_le_bytes().to_vec();
        out.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        out.extend_from_slice(&frame);
        out.extend_from_slice(&(self.state.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.state);
        out
    }

    /// Read the file strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        fn take<'a>(rest: &mut &'a [u8], n: usize) -> Result<&'a [u8], DecodeError> {
            let (head, tail) = rest.split_at_checked(n).ok_or(DecodeError::Truncated)?;
            *rest = tail;
            Ok(head)
        }
        fn length(rest: &mut &[u8]) -> Result<usize, DecodeError> {
            let n = u32::from_le_bytes(
                take(rest, 4)?
                    .try_into()
                    .map_err(|_| DecodeError::Truncated)?,
            );
            usize::try_from(n).map_err(|_| DecodeError::Truncated)
        }
        let mut rest = bytes;
        let signed_at = u64::from_le_bytes(
            take(&mut rest, 8)?
                .try_into()
                .map_err(|_| DecodeError::Truncated)?,
        );
        let n = length(&mut rest)?;
        let frame = take(&mut rest, n)?;
        let signed = SignedSettings::from_bytes(frame)?;
        if signed.to_bytes() != frame {
            return Err(DecodeError::NonCanonical(
                "the waiting settings are not in the form they are written in",
            ));
        }
        let n = length(&mut rest)?;
        let state = take(&mut rest, n)?.to_vec();
        if !rest.is_empty() {
            return Err(DecodeError::NonCanonical(
                "the waiting change has bytes after its state",
            ));
        }
        Ok(Self {
            signed,
            signed_at,
            state,
        })
    }

    /// The hash the counters confirm (AP-04).
    pub fn hash(&self) -> [u8; 32] {
        self.signed.settings.hash()
    }

    /// The four words both screens show (AP-03).
    pub fn words(&self) -> [&'static str; 4] {
        confirm_words(&self.hash())
    }

    /// Whether the change has waited seven days at `now` (OA-121).
    pub fn expired(&self, now: u64) -> bool {
        now >= self.signed_at.saturating_add(WAIT_SECS)
    }

    /// The fields this change loosens, against the settings in force.
    pub fn loosened_fields(&self, current: &Settings) -> Vec<&'static str> {
        changed_fields(current, &self.signed.settings)
            .into_iter()
            .filter(|c| c.direction == Change::Loosening)
            .map(|c| c.field)
            .collect()
    }

    /// Whether the settings in force are these settings, so that the file is
    /// no longer needed.
    pub fn is_in_force(&self, current: Option<&SignedSettings>) -> bool {
        current.is_some_and(|c| c.settings.hash() == self.hash())
    }

    /// OA-120: join `confirmation` to the settings as `OBM<`, when it is for
    /// this change's hash and from a counter of `current`, the settings in
    /// force (AP-04, AP-05); `None` otherwise, and the change keeps waiting.
    pub fn join(
        &self,
        confirmation: CounterConfirmation,
        owner: &DeviceId,
        current: &Settings,
    ) -> Option<ConfirmedSettings> {
        let confirmed = ConfirmedSettings::new(self.signed.clone(), confirmation);
        confirmed.verify(owner, current).ok()?;
        Some(confirmed)
    }
}

/// Whether `new`, as a change from `current`, needs a counter's confirmation
/// for a shop that has a Box: its class is Loosen (AP-01, AP-12).
pub fn needs_confirmation(current: &Settings, new: &Settings) -> bool {
    classify(current, new) == ChangeClass::Loosen
}
