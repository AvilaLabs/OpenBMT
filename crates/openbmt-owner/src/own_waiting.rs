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
//! The file: `signed at` u64 LE, then the `OBMS` frame of the settings. It
//! is read strictly: what decodes encodes to the same bytes.

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
}

impl WaitingChange {
    /// The file's bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.signed_at.to_le_bytes().to_vec();
        out.extend_from_slice(&self.signed.to_bytes());
        out
    }

    /// Read the file strictly.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (at, rest) = bytes.split_at_checked(8).ok_or(DecodeError::Truncated)?;
        let signed_at = u64::from_le_bytes(at.try_into().map_err(|_| DecodeError::Truncated)?);
        let signed = SignedSettings::from_bytes(rest)?;
        if signed.to_bytes() != rest {
            return Err(DecodeError::NonCanonical(
                "the waiting settings are not in the form they are written in",
            ));
        }
        Ok(Self { signed, signed_at })
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
