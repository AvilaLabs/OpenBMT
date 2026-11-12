//! Signed settings that carry a counter's confirmation (`docs/COUNTER-RULES.md`
//! §24, CR-242).
//!
//! A shop that uses its own wallet has no Key app to co-sign a change that
//! loosens its settings. One counter, with its manager's PIN typed, confirms
//! the change instead ([`crate::approval`], AP-03 to AP-05). The owner phone
//! then sends the settings in the frame `OBM<`, version 1: the `OBMS` frame of
//! [`SignedSettings`], whole, then the confirming counter's id (32 bytes) and
//! its signature (64 bytes). The frame is read as strictly as `OBMS` is: the
//! settings inside must read and encode again to the same bytes, and the
//! frame encodes again to the input.

use std::collections::BTreeMap;
use std::fmt;

use crate::approval::{ChangeClass, ConfirmError, CounterConfirmation};
use crate::identity::DeviceId;
use crate::settings::{Authority, Change, Settings, SettingsError, SignedSettings};
use crate::wire::{self, DecodeError, Reader};

/// The most the signed settings inside the frame take, in bytes.
pub const MAX_INNER_BYTES: usize = 64 * 1024;

/// The bytes the confirming counter adds to the `OBMS` frame: its id and its
/// signature.
const CONFIRMATION_BYTES: usize = 32 + 64;

/// Why signed settings with a confirmation were refused (CR-242).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmedError {
    /// The settings themselves were refused, as for the `OBMS` frame.
    Settings(SettingsError),
    /// The confirmation is not one the shop's settings in force accept.
    Confirmation(ConfirmError),
}

impl fmt::Display for ConfirmedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Settings(e) => write!(f, "{e}"),
            Self::Confirmation(e) => write!(f, "CR-242: {e}"),
        }
    }
}

impl std::error::Error for ConfirmedError {}

/// Signed settings and the confirmation of one counter of the shop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedSettings {
    /// The settings, signed by the owner root.
    pub signed: SignedSettings,
    /// The confirming counter and its signature over the settings' hash.
    pub confirmation: CounterConfirmation,
}

impl ConfirmedSettings {
    /// Join a counter's confirmation to signed settings.
    pub fn new(signed: SignedSettings, confirmation: CounterConfirmation) -> Self {
        Self {
            signed,
            confirmation,
        }
    }

    /// The wire form: `OBM<`, version 1, the `OBMS` frame, the counter's id
    /// and its signature. Call it on settings that passed `validate`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let inner = self.signed.to_bytes();
        let mut out = wire::start(
            &wire::MAGIC_CONFIRMED_SETTINGS,
            inner.len() + CONFIRMATION_BYTES,
        );
        out.extend_from_slice(&inner);
        out.extend_from_slice(&self.confirmation.counter.to_bytes());
        out.extend_from_slice(&self.confirmation.sig.0);
        out
    }

    /// Read the wire form strictly: the magic and version, an `OBMS` frame of
    /// at most [`MAX_INNER_BYTES`] that reads and encodes again to the same
    /// bytes, a valid counter id and a signature, and nothing after. No
    /// signature is checked here.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        // The frame is magic and version, so what is left after the five
        // bytes is the settings and the confirmation.
        Reader::framed(
            bytes.get(..5).ok_or(DecodeError::Truncated)?,
            &wire::MAGIC_CONFIRMED_SETTINGS,
        )?
        .finish()?;
        let rest = bytes.get(5..).ok_or(DecodeError::Truncated)?;
        let inner_len = rest
            .len()
            .checked_sub(CONFIRMATION_BYTES)
            .ok_or(DecodeError::Truncated)?;
        if inner_len > MAX_INNER_BYTES {
            return Err(DecodeError::TooLarge);
        }
        let (inner, tail) = rest.split_at(inner_len);
        let signed = SignedSettings::from_bytes(inner)?;
        if signed.to_bytes() != inner {
            return Err(DecodeError::NonCanonical(
                "the settings are not in the form they are written in",
            ));
        }
        let mut t = Reader::new(tail);
        let counter = t.device_id()?;
        let sig = t.sig()?;
        t.finish()?;
        Ok(Self {
            signed,
            confirmation: CounterConfirmation { counter, sig },
        })
    }

    /// CR-242: check these settings as a change from `current`, the settings
    /// now in force, under the owner root `owner`. Every check of the
    /// `OBMS` frame applies (the owner's signature, valid values, a version
    /// above the one in force); a Redirect change is refused; and a change
    /// that is not Free needs this confirmation: from a counter of `current`
    /// (AP-05), signed for these settings' hash (AP-04). A confirmation that
    /// is carried must be right whether or not the change needed it.
    pub fn verify(&self, owner: &DeviceId, current: &Settings) -> Result<Change, ConfirmedError> {
        let authority = Authority::OwnWallet { owner: *owner };
        let change = match self.signed.verify_with(&authority, Some(current)) {
            Ok(change) => change,
            Err(SettingsError::ConfirmationRequired(ChangeClass::Loosen)) => Change::Loosening,
            Err(e) => return Err(ConfirmedError::Settings(e)),
        };
        self.confirmation
            .verify(&self.signed.settings.hash(), &current.counters)
            .map_err(ConfirmedError::Confirmation)?;
        Ok(change)
    }

    /// CR-163: check that the confirmation is the counter's own signature
    /// over these settings' hash, for settings that were taken earlier, when
    /// the settings in force then are not known. The counter's place among
    /// them was checked when the settings were taken.
    pub fn verify_signature(&self) -> Result<(), ConfirmError> {
        let mut only = BTreeMap::new();
        only.insert(self.confirmation.counter, 0u16);
        self.confirmation
            .verify(&self.signed.settings.hash(), &only)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Limit;
    use crate::testutil::identity;

    fn owner() -> crate::identity::Identity {
        identity(1)
    }

    fn counter() -> crate::identity::Identity {
        identity(2)
    }

    fn base() -> Settings {
        let mut s = Settings::defaults(1_000);
        s.version = 3;
        s.devices.insert(counter().device_id());
        s.counters.insert(counter().device_id(), 10_000);
        s
    }

    fn loosened() -> Settings {
        let mut s = base();
        s.version = 4;
        s.light_per_sale = Limit::from_usd(s.light_per_sale.display_usd + 5, 1_000);
        s
    }

    fn signed(s: Settings) -> SignedSettings {
        let owner_sig = s.sign(&owner());
        SignedSettings {
            settings: s,
            owner_sig,
            approver_sig: None,
        }
    }

    fn confirmed() -> ConfirmedSettings {
        let s = signed(loosened());
        let c = CounterConfirmation::sign(&counter(), &s.settings.hash());
        ConfirmedSettings::new(s, c)
    }

    #[test]
    fn cr_242_the_frame_is_the_obms_frame_then_the_counter_and_its_signature() {
        let c = confirmed();
        let bytes = c.to_bytes();
        assert_eq!(&bytes[..5], b"OBM<\x01");
        let inner = c.signed.to_bytes();
        assert_eq!(&bytes[5..5 + inner.len()], &inner[..]);
        assert_eq!(
            &bytes[5 + inner.len()..5 + inner.len() + 32],
            &counter().device_id().to_bytes()
        );
        assert_eq!(bytes.len(), 5 + inner.len() + 96);
        assert_eq!(ConfirmedSettings::from_bytes(&bytes), Ok(c));
    }

    #[test]
    fn cr_242_the_frame_is_read_strictly() {
        let c = confirmed();
        let bytes = c.to_bytes();
        // Every cut is refused, a byte left over is refused, other magic and
        // versions are refused.
        for n in 0..bytes.len() {
            assert!(ConfirmedSettings::from_bytes(&bytes[..n]).is_err(), "{n}");
        }
        let mut long = bytes.clone();
        long.push(0);
        assert!(ConfirmedSettings::from_bytes(&long).is_err());
        let mut magic = bytes.clone();
        magic[3] = b'S';
        assert_eq!(
            ConfirmedSettings::from_bytes(&magic),
            Err(DecodeError::BadMagic)
        );
        let mut version = bytes.clone();
        version[4] = 2;
        assert_eq!(
            ConfirmedSettings::from_bytes(&version),
            Err(DecodeError::UnsupportedVersion(2))
        );
        // A plain `OBMS` frame is not an `OBM<` frame.
        assert!(ConfirmedSettings::from_bytes(&c.signed.to_bytes()).is_err());
        // The settings inside are read as strictly as `OBMS` reads them.
        let mut inner_long = c.signed.to_bytes();
        inner_long.push(0);
        let mut framed = wire::start(&wire::MAGIC_CONFIRMED_SETTINGS, 0);
        framed.extend_from_slice(&inner_long);
        framed.extend_from_slice(&c.confirmation.counter.to_bytes());
        framed.extend_from_slice(&c.confirmation.sig.0);
        assert!(ConfirmedSettings::from_bytes(&framed).is_err());
        // Bytes that decode encode again to the same bytes.
        let again = ConfirmedSettings::from_bytes(&bytes).unwrap().to_bytes();
        assert_eq!(again, bytes);
        // A counter id that is not a key is refused.
        let mut bad_key = bytes.clone();
        let at = bytes.len() - 96;
        bad_key[at..at + 32].fill(0xff);
        assert_eq!(
            ConfirmedSettings::from_bytes(&bad_key),
            Err(DecodeError::InvalidKey)
        );
    }

    #[test]
    fn cr_242_a_valid_confirmation_from_a_counter_in_force_takes_a_loosening() {
        let c = confirmed();
        assert_eq!(
            c.verify(&owner().device_id(), &base()),
            Ok(Change::Loosening)
        );
    }

    #[test]
    fn cr_242_a_counter_not_in_force_another_hash_or_a_bad_signature_is_refused() {
        let other = identity(3);
        let mut c = confirmed();
        // A counter that is not in the settings now in force (AP-05), even
        // when it is in the new ones.
        let mut new = loosened();
        new.counters.insert(counter().device_id(), 9_000);
        new.counters.insert(other.device_id(), 1_000);
        new.devices.insert(other.device_id());
        let s = signed(new);
        let conf = CounterConfirmation::sign(&other, &s.settings.hash());
        let d = ConfirmedSettings::new(s, conf);
        assert_eq!(
            d.verify(&owner().device_id(), &base()),
            Err(ConfirmedError::Confirmation(ConfirmError::NotACounter))
        );
        // The confirmation of other settings.
        c.confirmation = CounterConfirmation::sign(&counter(), &[9u8; 32]);
        assert_eq!(
            c.verify(&owner().device_id(), &base()),
            Err(ConfirmedError::Confirmation(ConfirmError::BadSignature))
        );
        // A changed signature.
        let mut bad = confirmed();
        bad.confirmation.sig.0[3] ^= 1;
        assert_eq!(
            bad.verify(&owner().device_id(), &base()),
            Err(ConfirmedError::Confirmation(ConfirmError::BadSignature))
        );
        // The refusal names the rule.
        assert!(ConfirmedError::Confirmation(ConfirmError::NotACounter)
            .to_string()
            .starts_with("CR-242"));
    }

    #[test]
    fn cr_242_the_checks_of_the_obms_frame_still_apply() {
        // The owner's signature, the version and a Redirect.
        let mut c = confirmed();
        c.signed.owner_sig = Settings::defaults(1).sign(&owner());
        assert_eq!(
            c.verify(&owner().device_id(), &base()),
            Err(ConfirmedError::Settings(
                SettingsError::OwnerSignatureInvalid
            ))
        );
        let c = confirmed();
        let mut forward = base();
        forward.version = 4;
        assert!(matches!(
            c.verify(&owner().device_id(), &forward),
            Err(ConfirmedError::Settings(SettingsError::StaleVersion { .. }))
        ));
        let mut redirect = loosened();
        redirect.lightning_node = Some([2u8; crate::settings::LIGHTNING_NODE_BYTES]);
        let s = signed(redirect);
        let conf = CounterConfirmation::sign(&counter(), &s.settings.hash());
        assert_eq!(
            ConfirmedSettings::new(s, conf).verify(&owner().device_id(), &base()),
            Err(ConfirmedError::Settings(
                SettingsError::ConfirmationRequired(ChangeClass::Redirect)
            ))
        );
        // Another owner root.
        assert!(confirmed()
            .verify(&identity(9).device_id(), &base())
            .is_err());
    }

    #[test]
    fn cr_163_a_kept_frame_is_checked_for_the_counters_own_signature() {
        let c = confirmed();
        assert_eq!(c.verify_signature(), Ok(()));
        let mut bad = c;
        bad.confirmation.sig.0[0] ^= 1;
        assert_eq!(bad.verify_signature(), Err(ConfirmError::BadSignature));
    }
}
