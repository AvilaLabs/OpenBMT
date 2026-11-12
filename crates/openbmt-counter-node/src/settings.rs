//! The settings the owner sends after pairing (`docs/COUNTER-RULES.md`,
//! CR-160 to CR-166).
//!
//! The pairing record holds the settings the counter was paired with. Every
//! later change comes from the owner as a message of kind Settings through
//! the link; [`crate::node::Node::take_settings`] checks it and keeps it in
//! one file of the node's folder, [`crate::dirstore::SETTINGS_FILE`]. At the
//! next start [`effective`] reads the record and the file and gives the
//! newer of the two.
//!
//! The file holds the `OBMS` frame of the signed settings and nothing else:
//! at most [`MAX_SETTINGS_FILE_BYTES`], read strictly by [`decode_file`] (the
//! frame of `openbmt_core::settings`, no byte over, and the bytes written
//! again are the bytes read).
//!
//! The settings in the file are not checked as a change from the record: the
//! counter may have taken several versions since, and what a change from the
//! record's version is called depends on the versions in between. They are
//! checked as signed by both pinned roots when they carry both signatures
//! and by the owner root alone when they carry one, valid, and with a
//! version above the record's. The counter checked each version as a change
//! from the one in force before it was written (CR-160).

use std::fmt;

use openbmt_core::approval::ConfirmError;
use openbmt_core::confirmed::ConfirmedSettings;
use openbmt_core::settings::{Authority, Settings, SettingsError, SignedSettings};
use openbmt_core::wire::{DecodeError, MAGIC_CONFIRMED_SETTINGS};

use crate::dirstore::{DirStore, DirStoreError};
use crate::paired::PairingRecord;
use crate::paired_own::PairedRecord;

/// CR-161: the most the settings file holds, in bytes.
pub const MAX_SETTINGS_FILE_BYTES: usize = 64 * 1024;

/// CR-161: read the settings file strictly: no longer than
/// [`MAX_SETTINGS_FILE_BYTES`], one `OBMS` frame with no byte over, and
/// bytes that are the encoding of what they decode to.
pub fn decode_file(bytes: &[u8]) -> Result<SignedSettings, DecodeError> {
    if bytes.len() > MAX_SETTINGS_FILE_BYTES {
        return Err(DecodeError::TooLarge);
    }
    let signed = SignedSettings::from_bytes(bytes)?;
    if signed.to_bytes() != bytes {
        return Err(DecodeError::NonCanonical(
            "the settings are not in the form they are written in",
        ));
    }
    Ok(signed)
}

/// What the settings file or a message of kind Settings holds: the `OBMS`
/// frame, or the `OBM<` frame that carries a counter's confirmation as well
/// (CR-242).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsFrame {
    /// Signed settings (`OBMS`).
    Plain(SignedSettings),
    /// Signed settings and a counter's confirmation (`OBM<`).
    Confirmed(ConfirmedSettings),
}

impl SettingsFrame {
    /// The signed settings, whichever frame carried them.
    pub fn signed(&self) -> &SignedSettings {
        match self {
            Self::Plain(signed) => signed,
            Self::Confirmed(c) => &c.signed,
        }
    }

    /// The bytes of the frame, as they are kept and sent.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Plain(signed) => signed.to_bytes(),
            Self::Confirmed(c) => c.to_bytes(),
        }
    }
}

/// CR-160, CR-242: read a message of kind Settings, or the settings file, as
/// either frame, each as strictly as it is read alone: the `OBM<` frame by
/// its own magic, anything else as [`decode_file`] reads it.
pub fn decode_frame(bytes: &[u8]) -> Result<SettingsFrame, DecodeError> {
    if bytes.starts_with(&MAGIC_CONFIRMED_SETTINGS) {
        let c = ConfirmedSettings::from_bytes(bytes)?;
        if c.to_bytes() != bytes {
            return Err(DecodeError::NonCanonical(
                "the settings are not in the form they are written in",
            ));
        }
        return Ok(SettingsFrame::Confirmed(c));
    }
    decode_file(bytes).map(SettingsFrame::Plain)
}

/// Why settings the owner sent were not taken. Nothing changed. Each names
/// its rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsRefused {
    /// CR-160: what came is not signed settings in the form they are sent.
    Bytes(DecodeError),
    /// CR-160: the settings are not signed as they must be, or are not
    /// valid, or do not go past the ones in force.
    Verify(SettingsError),
    /// CR-161: the settings could not be written to the folder, so they were
    /// not used.
    Store(DirStoreError),
    /// CR-242: the confirmation of a counter that came with the settings is
    /// not one the settings in force accept.
    Confirmation(ConfirmError),
    /// CR-242: a counter paired with the Key app takes no confirmation of a
    /// counter: its loosening carries the Key app's signature.
    NoConfirmationHere,
}

impl SettingsRefused {
    /// The rule the refusal belongs to.
    pub fn rule(&self) -> &'static str {
        match self {
            // A change that sends payments elsewhere is refused by CR-239.
            Self::Verify(SettingsError::ConfirmationRequired(_)) => "CR-239",
            Self::Bytes(_) | Self::Verify(_) => "CR-160",
            Self::Store(_) => "CR-161",
            Self::Confirmation(_) | Self::NoConfirmationHere => "CR-242",
        }
    }
}

impl fmt::Display for SettingsRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Bytes(e) => write!(f, "the owner's settings do not read: {e}"),
            Self::Verify(e) => write!(f, "the owner's settings are refused: {e}"),
            Self::Store(e) => write!(f, "the owner's settings were not written, so not used: {e}"),
            Self::Confirmation(e) => write!(f, "the confirmation that came with the settings: {e}"),
            Self::NoConfirmationHere => write!(
                f,
                "a counter paired with the Key app takes no counter's confirmation"
            ),
        }
    }
}

impl std::error::Error for SettingsRefused {}

/// Why the counter will not start from what it holds of the settings. Each
/// names its rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsFault {
    /// CR-163: the settings file could not be read.
    Read(DirStoreError),
    /// CR-163: the settings file does not read back.
    Decode(DecodeError),
    /// CR-163: the settings in the file are not signed as they must be, or
    /// are not valid.
    Verify(SettingsError),
    /// CR-163: the counter's own records name a settings version above the
    /// one the start would use: the file was put back from an older copy, or
    /// is gone.
    Rolled {
        /// The version the start would use.
        in_force: u32,
        /// The highest version a record names.
        recorded: u32,
    },
}

impl SettingsFault {
    /// The rule the fault belongs to.
    pub fn rule(&self) -> &'static str {
        "CR-163"
    }
}

impl fmt::Display for SettingsFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.rule())?;
        match self {
            Self::Read(e) => write!(f, "the settings file could not be read: {e}"),
            Self::Decode(e) => write!(f, "the settings file does not read back: {e}"),
            Self::Verify(e) => write!(f, "the settings file is not signed as it must be: {e}"),
            Self::Rolled { in_force, recorded } => write!(
                f,
                "the settings to use are version {in_force} but the counter's records name \
                 version {recorded}: the settings file was put back from an older copy, or is \
                 gone"
            ),
        }
    }
}

impl std::error::Error for SettingsFault {}

/// The settings a start uses (CR-163).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    /// The signed settings in force: the file's when it is newer than the
    /// pairing record's, else the record's.
    pub signed: SignedSettings,
    /// Whether they came from the file.
    pub from_file: bool,
    /// A line for the node's log when the file was not used though it is
    /// there: it holds settings no newer than the record's.
    pub note: Option<String>,
}

impl Effective {
    /// The settings in force.
    pub fn settings(&self) -> &Settings {
        &self.signed.settings
    }
}

/// CR-163: check `signed`, read from the settings file, against the pinned
/// roots and the pairing record's settings as the floor: the owner root
/// signed it, the Key app root signed it if it carries that signature, it is
/// valid and its version is above the record's. `Ok(false)` when the version
/// is not above the record's.
fn check_file(
    signed: &SignedSettings,
    authority: &Authority,
    floor: &Settings,
) -> Result<bool, SettingsFault> {
    signed
        .verify_signatures_with(authority)
        .map_err(SettingsFault::Verify)?;
    Ok(signed.settings.version > floor.version)
}

/// CR-163: the settings a start uses, from the pairing record and the
/// settings file of `store`'s folder. The record was checked by the caller
/// (`PairingRecord::check`). A file that does not read, or is not signed as
/// it must be, is a fault: the counter does not fall back to the record's
/// settings, which may be looser than what the owner last sent. A file with
/// settings no newer than the record's is left as it is and noted.
pub fn effective(store: &DirStore, record: &PairingRecord) -> Result<Effective, SettingsFault> {
    effective_under(store, &Authority::KeyApp(record.roots), &record.settings)
}

/// [`effective`] for a pairing record of either kind (CR-233): under the
/// owner root alone, a settings file that carries a Key app signature is a
/// fault (CR-236).
pub fn effective_of(store: &DirStore, record: &PairedRecord) -> Result<Effective, SettingsFault> {
    effective_under(store, &record.authority(), record.settings())
}

fn effective_under(
    store: &DirStore,
    authority: &Authority,
    record_settings: &SignedSettings,
) -> Result<Effective, SettingsFault> {
    let Some(bytes) = store.settings_file().map_err(SettingsFault::Read)? else {
        return Ok(Effective {
            signed: record_settings.clone(),
            from_file: false,
            note: None,
        });
    };
    let frame = decode_frame(&bytes).map_err(SettingsFault::Decode)?;
    if let SettingsFrame::Confirmed(c) = &frame {
        // CR-163: the frame of a confirmation is the one an own-wallet
        // counter writes; the counter's own signature is checked again, and
        // a counter paired with the Key app never wrote one.
        if matches!(authority, Authority::KeyApp(_)) {
            return Err(SettingsFault::Verify(SettingsError::Invalid(
                "CR-242: a counter paired with the Key app keeps no confirmation of a counter",
            )));
        }
        c.verify_signature().map_err(|_| {
            SettingsFault::Verify(SettingsError::Invalid(
                "CR-242: the confirmation of a counter in the settings file is not signed",
            ))
        })?;
    }
    let signed = frame.signed().clone();
    if check_file(&signed, authority, &record_settings.settings)? {
        return Ok(Effective {
            signed,
            from_file: true,
            note: None,
        });
    }
    Ok(Effective {
        note: Some(format!(
            "CR-163: the settings file is version {}, which is not above the pairing's version \
             {}: the pairing's settings are used",
            signed.settings.version, record_settings.settings.version
        )),
        signed: record_settings.clone(),
        from_file: false,
    })
}

/// CR-163: the check of the rollback guard, for the node: the counter's own
/// records name `recorded`, so settings of a lower version in force mean the
/// file was put back from an older copy.
pub fn check_not_rolled(version: u32, recorded: Option<u32>) -> Result<(), SettingsFault> {
    match recorded {
        Some(recorded) if recorded > version => Err(SettingsFault::Rolled {
            in_force: version,
            recorded,
        }),
        _ => Ok(()),
    }
}

/// What Home says about the settings besides the link's line: the one thing
/// the cashier or a manager must know. Its words are in [`crate::say`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsLine {
    /// CR-165: the settings no longer list this counter.
    Removed,
    /// CR-164: the settings change Tor or the Lightning node, which take
    /// effect when the node starts again.
    RestartNeeded,
}

impl SettingsLine {
    /// The line for the node's state: removal first, as it is the one that
    /// stops sales.
    pub fn of(removed: bool, restart_needed: bool) -> Option<Self> {
        if removed {
            Some(Self::Removed)
        } else if restart_needed {
            Some(Self::RestartNeeded)
        } else {
            None
        }
    }
}
