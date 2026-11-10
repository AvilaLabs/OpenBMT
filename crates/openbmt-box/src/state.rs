//! The Box's state and what changes it (BX-01 to BX-24, BX-50, BX-51).
//!
//! [`BoxCore`] holds the Box's key, the open invitation, the pinned roots and
//! the settings in force. It opens no socket and reads no clock: the time and
//! the randomness are passed in, and storage is behind
//! [`BoxFiles`](crate::files::BoxFiles). A value is stored before it takes
//! effect (BX-50): when a write fails, the state in memory is as it was.
//!
//! The files, all in the data directory:
//!
//! | File | Holds |
//! |---|---|
//! | `box.key` | `OBMY`, 1, the 32 secret bytes of the Box's identity |
//! | `invitation` | `OBMI`, 1, the invitation (17), its nonce (32), the time it was entered (8, LE) |
//! | `roots` | `OBMF`, 1, owner root (32), approver root (32) |
//! | `settings` | the signed settings in force (`OBMS`) |
//! | `link-inbox`, `link-outbox` | the message numbers (LK-40) |
//! | `pos.token` | the Box's own POS sign-in (BX-41); erased by BX-23 |
//!
//! Paired means `roots` is there. It is the last file a welcome writes, so
//! a crash during pairing leaves the Box unpaired, never half paired.

use std::collections::BTreeMap;
use std::fmt;
use std::io;

use openbmt_core::batch::PinnedRoots;
use openbmt_core::boxpair::{Claim, Invitation, InviteTextError, Welcome};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::{DeviceRole, Settings, SettingsError, SignedSettings};
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_link::{Inbox, Outbox};

use crate::files::BoxFiles;

/// The Box's identity secret (BX-01).
pub const KEY_FILE: &str = "box.key";
/// The open invitation (BX-12).
pub const INVITATION_FILE: &str = "invitation";
/// The pinned roots (BX-16).
pub const ROOTS_FILE: &str = "roots";
/// The signed settings in force (BX-21).
pub const SETTINGS_FILE: &str = "settings";
/// The Box's own POS sign-in (BX-41), erased by BX-23.
pub const POS_TOKEN_FILE: &str = "pos.token";
/// Seconds an invitation is good for on the Box's clock (BX-12).
pub const INVITE_SECS: u64 = 30 * 60;
/// Most members the relay holds (LK-21), and so the most the device list
/// and the owner may come to.
pub const MAX_MEMBERS: usize = 64;

const MAGIC_KEY: [u8; 4] = *b"OBMY";
const MAGIC_INVITATION: [u8; 4] = *b"OBMI";
const MAGIC_ROOTS: [u8; 4] = *b"OBMF";

/// What the Box is doing, for the person at its console.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxStatus {
    /// No roots pinned and no invitation open.
    Unpaired,
    /// Not paired, and holding an invitation that has not run out.
    InvitationOpen,
    /// Paired, and listed by the settings in force.
    Paired,
    /// Paired, and the settings in force no longer list this Box (BX-23).
    TurnedOff,
}

impl BoxStatus {
    /// The words the console prints.
    pub fn text(self) -> &'static str {
        match self {
            Self::Unpaired => "unpaired",
            Self::InvitationOpen => "invitation open",
            Self::Paired => "paired",
            Self::TurnedOff => "turned off",
        }
    }
}

/// Why the Box refused or could not do something. Each names its rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoxError {
    /// A file that must be read is damaged or missing: the Box stops and
    /// says which (BX-51).
    Damaged {
        /// The file.
        file: &'static str,
        /// What is wrong with it.
        why: &'static str,
    },
    /// A file could not be read, written or removed (BX-50).
    Io {
        /// The file.
        file: &'static str,
        /// What was being done.
        op: &'static str,
        /// What the system said.
        kind: io::ErrorKind,
    },
    /// The system's randomness failed (BX-01).
    NoRandomness,
    /// The Box is paired: it takes no welcome and no invitation (BX-16).
    AlreadyPaired,
    /// The invitation text was refused (BX-10).
    Invitation(InviteTextError),
    /// No invitation is open (BX-12).
    NoInvitation,
    /// The invitation ran out (BX-12).
    InvitationExpired,
    /// The welcome is not a welcome (BX-15).
    BadWelcome(DecodeError),
    /// The welcome's MAC is not the one for the invitation (BX-15).
    WelcomeMac,
    /// The settings are not signed settings (BX-15, BX-21).
    BadSettings(DecodeError),
    /// The settings do not check under the pinned roots, or do not replace
    /// the ones in force (BX-15, BX-21).
    Settings(SettingsError),
    /// The first settings do not list this Box as a Box (BX-15).
    NotListed,
    /// The device list and the owner come to more members than the relay
    /// holds (BX-22).
    TooManyMembers,
    /// The Box is not paired, so there are no roots to check against
    /// (BX-21).
    NotPaired,
}

impl fmt::Display for BoxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Damaged { file, why } => write!(
                f,
                "BX-51: {file} is {why}: the Box does not start and does not make a new one"
            ),
            Self::Io { file, op, kind } => write!(f, "BX-50: could not {op} {file}: {kind}"),
            Self::NoRandomness => write!(f, "BX-01: the system's randomness failed"),
            Self::AlreadyPaired => write!(
                f,
                "BX-16: this Box is paired: erase its data directory to pair it again"
            ),
            Self::Invitation(e) => write!(f, "{e}"),
            Self::NoInvitation => write!(f, "BX-12: no invitation is open"),
            Self::InvitationExpired => write!(f, "BX-12: the invitation ran out"),
            Self::BadWelcome(e) => write!(f, "BX-15: the welcome is not valid: {e}"),
            Self::WelcomeMac => write!(
                f,
                "BX-15: the welcome was not made for this invitation and this Box"
            ),
            Self::BadSettings(e) => write!(f, "BX-15: the settings are not valid: {e}"),
            Self::Settings(e) => write!(f, "BX-21: the settings are refused: {e}"),
            Self::NotListed => write!(f, "BX-15: the settings do not list this Box as a Box"),
            Self::TooManyMembers => write!(
                f,
                "BX-22: the device list is longer than the relay can hold"
            ),
            Self::NotPaired => write!(f, "BX-21: this Box is not paired"),
        }
    }
}

impl std::error::Error for BoxError {}

fn io_err(file: &'static str, op: &'static str) -> impl FnOnce(io::Error) -> BoxError {
    move |e| BoxError::Io {
        file,
        op,
        kind: e.kind(),
    }
}

/// What taking new settings did (BX-21, BX-23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTaken {
    /// The settings no longer list this Box: it is off.
    pub turned_off: bool,
    /// When the Box was turned off, why the POS token could not be erased,
    /// if it could not.
    pub token_error: Option<io::ErrorKind>,
}

/// 32 secret bytes that are overwritten when dropped.
struct Secret([u8; 32]);

impl Drop for Secret {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            *b = 0;
        }
        std::hint::black_box(&mut self.0);
    }
}

struct OpenInvitation {
    invitation: Invitation,
    nonce: [u8; 32],
    entered_at: u64,
}

struct Pairing {
    roots: PinnedRoots,
    signed: SignedSettings,
}

/// The Box's state.
pub struct BoxCore<F: BoxFiles> {
    files: F,
    secret: Secret,
    id: DeviceId,
    open: Option<OpenInvitation>,
    pairing: Option<Pairing>,
}

impl<F: BoxFiles> fmt::Debug for BoxCore<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BoxCore({:?})", self.id)
    }
}

/// Settings read back from disk are checked again, so that damage shows.
/// They were taken under the rules of BX-15 or BX-21 when they were stored
/// and may be a tightening that only the owner signed, so they are checked
/// as the change from themselves at the version before: the signatures and
/// `validate`, and nothing that needs the earlier settings.
fn check_stored(signed: &SignedSettings, roots: &PinnedRoots) -> Result<(), SettingsError> {
    let mut basis = signed.settings.clone();
    basis.version = basis.version.saturating_sub(1);
    signed.verify(roots, Some(&basis)).map(|_| ())
}

fn damaged(file: &'static str, why: &'static str) -> BoxError {
    BoxError::Damaged { file, why }
}

fn read_file<F: BoxFiles>(files: &F, file: &'static str) -> Result<Option<Vec<u8>>, BoxError> {
    files.read(file).map_err(io_err(file, "read"))
}

fn read_key(bytes: &[u8]) -> Result<[u8; 32], DecodeError> {
    let mut r = Reader::framed(bytes, &MAGIC_KEY)?;
    let secret = r.array::<32>()?;
    r.finish()?;
    Ok(secret)
}

fn read_roots(bytes: &[u8]) -> Result<PinnedRoots, BoxError> {
    let bad = |_| damaged(ROOTS_FILE, "not the two roots");
    let mut r = Reader::framed(bytes, &MAGIC_ROOTS).map_err(bad)?;
    let owner = r.device_id().map_err(bad)?;
    let approver = r.device_id().map_err(bad)?;
    r.finish().map_err(bad)?;
    PinnedRoots::new(owner, approver).map_err(|_| damaged(ROOTS_FILE, "two roots that are one key"))
}

fn read_invitation(bytes: &[u8]) -> Option<OpenInvitation> {
    let mut r = Reader::framed(bytes, &MAGIC_INVITATION).ok()?;
    let random = r.array::<17>().ok()?;
    let nonce = r.array::<32>().ok()?;
    let entered_at = r.u64_le().ok()?;
    r.finish().ok()?;
    Some(OpenInvitation {
        invitation: Invitation::from_random(&random),
        nonce,
        entered_at,
    })
}

/// The relay's member list for these settings (BX-22): the device list and
/// the owner's key.
fn member_list(settings: &Settings, owner: &DeviceId) -> Vec<DeviceId> {
    let mut all = settings.devices.clone();
    all.insert(*owner);
    all.into_iter().collect()
}

impl<F: BoxFiles> BoxCore<F> {
    /// Load the Box's state, making its key the first time (BX-01). A key,
    /// roots or settings file that is there and cannot be read stops the
    /// start and names the file (BX-51), and a missing key next to roots or
    /// settings does too: a new key is made only for a directory with none
    /// of them. `random` fills 32 bytes from the system's randomness.
    pub fn open(
        files: F,
        random: &mut impl FnMut(&mut [u8; 32]) -> io::Result<()>,
    ) -> Result<Self, BoxError> {
        let mut files = files;
        let roots_bytes = read_file(&files, ROOTS_FILE)?;
        let settings_bytes = read_file(&files, SETTINGS_FILE)?;
        let secret = match read_file(&files, KEY_FILE)? {
            Some(bytes) => read_key(&bytes).map_err(|_| damaged(KEY_FILE, "not a Box key"))?,
            None => {
                if roots_bytes.is_some() || settings_bytes.is_some() {
                    return Err(damaged(KEY_FILE, "missing"));
                }
                let mut secret = [0u8; 32];
                random(&mut secret).map_err(|_| BoxError::NoRandomness)?;
                let mut out = wire::start(&MAGIC_KEY, 32);
                out.extend_from_slice(&secret);
                files
                    .write(KEY_FILE, &out)
                    .map_err(io_err(KEY_FILE, "write"))?;
                secret
            }
        };
        let identity = Identity::from_secret_bytes(&secret)
            .map_err(|_| damaged(KEY_FILE, "not a valid key"))?;
        let id = identity.device_id();
        let pairing = match roots_bytes {
            None => None,
            Some(bytes) => {
                let roots = read_roots(&bytes)?;
                let bytes = settings_bytes.ok_or_else(|| damaged(SETTINGS_FILE, "missing"))?;
                let signed = SignedSettings::from_bytes(&bytes)
                    .map_err(|_| damaged(SETTINGS_FILE, "not signed settings"))?;
                check_stored(&signed, &roots)
                    .map_err(|_| damaged(SETTINGS_FILE, "not signed by the pinned roots"))?;
                Some(Pairing { roots, signed })
            }
        };
        // A damaged invitation is forgotten: it is only a 30 minute secret.
        let open = match (&pairing, read_file(&files, INVITATION_FILE)?) {
            (None, Some(bytes)) => match read_invitation(&bytes) {
                Some(open) => Some(open),
                None => {
                    let _ = files.remove(INVITATION_FILE);
                    None
                }
            },
            (Some(_), Some(_)) => {
                let _ = files.remove(INVITATION_FILE);
                None
            }
            (_, None) => None,
        };
        Ok(Self {
            files,
            secret: Secret(secret),
            id,
            open,
            pairing,
        })
    }

    /// The Box's identity key (BX-01).
    pub fn device_id(&self) -> DeviceId {
        self.id
    }

    /// The Box's identity, made from its stored secret, for the relay and
    /// for opening messages.
    pub fn identity(&self) -> Result<Identity, BoxError> {
        Identity::from_secret_bytes(&self.secret.0)
            .map_err(|_| damaged(KEY_FILE, "not a valid key"))
    }

    /// The storage, for tests.
    pub fn files(&self) -> &F {
        &self.files
    }

    /// The storage, to change it in a test.
    pub fn files_mut(&mut self) -> &mut F {
        &mut self.files
    }

    /// Give the storage back, as a stop does.
    pub fn into_files(self) -> F {
        self.files
    }

    /// The pinned roots, once paired (BX-16).
    pub fn roots(&self) -> Option<PinnedRoots> {
        self.pairing.as_ref().map(|p| p.roots)
    }

    /// The settings in force, once paired.
    pub fn settings(&self) -> Option<&Settings> {
        self.pairing.as_ref().map(|p| &p.signed.settings)
    }

    /// Whether the settings in force no longer list this Box (BX-23).
    pub fn turned_off(&self) -> bool {
        self.pairing
            .as_ref()
            .is_some_and(|p| !p.signed.settings.boxes.contains(&self.id))
    }

    fn expired(open: &OpenInvitation, now: u64) -> bool {
        now.saturating_sub(open.entered_at) >= INVITE_SECS
    }

    /// Forget an invitation that ran out (BX-12).
    fn expire(&mut self, now: u64) {
        if self.open.as_ref().is_some_and(|o| Self::expired(o, now)) {
            self.open = None;
            let _ = self.files.remove(INVITATION_FILE);
        }
    }

    /// What the Box is doing now.
    pub fn status(&mut self, now: u64) -> BoxStatus {
        self.expire(now);
        match &self.pairing {
            Some(_) if self.turned_off() => BoxStatus::TurnedOff,
            Some(_) => BoxStatus::Paired,
            None if self.open.is_some() => BoxStatus::InvitationOpen,
            None => BoxStatus::Unpaired,
        }
    }

    /// Whether an invitation is open at `now`.
    pub fn invitation_open(&mut self, now: u64) -> bool {
        self.expire(now);
        self.pairing.is_none() && self.open.is_some()
    }

    /// Take an invitation the merchant typed (BX-12). The text is read and
    /// its check characters are checked before anything is stored (BX-10),
    /// the box nonce is new, and any invitation held is replaced. A paired
    /// Box takes none (BX-16).
    pub fn enter_invitation(
        &mut self,
        text: &str,
        now: u64,
        nonce_random: [u8; 32],
    ) -> Result<(), BoxError> {
        if self.pairing.is_some() {
            return Err(BoxError::AlreadyPaired);
        }
        let invitation = Invitation::from_text(text).map_err(BoxError::Invitation)?;
        let mut out = wire::start(&MAGIC_INVITATION, 57);
        out.extend_from_slice(invitation.as_bytes());
        out.extend_from_slice(&nonce_random);
        out.extend_from_slice(&now.to_le_bytes());
        self.files
            .write(INVITATION_FILE, &out)
            .map_err(io_err(INVITATION_FILE, "write"))?;
        self.open = Some(OpenInvitation {
            invitation,
            nonce: nonce_random,
            entered_at: now,
        });
        Ok(())
    }

    /// The claim to offer (BX-12): while an invitation is open and has not
    /// run out, `OBM5` with the Box's key, the invitation's nonce and the
    /// MAC.
    pub fn claim(&mut self, now: u64) -> Option<Vec<u8>> {
        self.expire(now);
        if self.pairing.is_some() {
            return None;
        }
        let open = self.open.as_ref()?;
        Some(Claim::make(&open.invitation, self.id, open.nonce).to_bytes())
    }

    /// Take a welcome and the first settings (BX-15), in this order: the
    /// invitation is spent, the MAC is checked, the roots are taken from
    /// the welcome, the settings are checked under them as first settings,
    /// they must list this Box as a Box, and then the settings, the two
    /// number files (LK-40) and last the roots are stored. Any failure
    /// leaves the Box unpaired with the invitation spent. A paired Box
    /// takes a welcome from nobody (BX-16).
    pub fn take_welcome(
        &mut self,
        welcome: &[u8],
        settings: &[u8],
        now: u64,
    ) -> Result<(), BoxError> {
        if self.pairing.is_some() {
            return Err(BoxError::AlreadyPaired);
        }
        let open = self.open.take().ok_or(BoxError::NoInvitation)?;
        // Spent from here on, whatever happens next (BX-15).
        self.files
            .remove(INVITATION_FILE)
            .map_err(io_err(INVITATION_FILE, "remove"))?;
        if Self::expired(&open, now) {
            return Err(BoxError::InvitationExpired);
        }
        let welcome = Welcome::from_bytes(welcome).map_err(BoxError::BadWelcome)?;
        let roots = welcome
            .verify(&open.invitation, &self.id, &open.nonce)
            .ok_or(BoxError::WelcomeMac)?;
        let signed = SignedSettings::from_bytes(settings).map_err(BoxError::BadSettings)?;
        signed.verify(&roots, None).map_err(BoxError::Settings)?;
        if !signed.settings.boxes.contains(&self.id) {
            return Err(BoxError::NotListed);
        }
        if member_list(&signed.settings, &roots.owner).len() > MAX_MEMBERS {
            return Err(BoxError::TooManyMembers);
        }
        // The roots go last: they are what makes the Box paired, so a stop
        // before them leaves it unpaired and one after them leaves it
        // paired with everything in place.
        self.files
            .write(SETTINGS_FILE, &signed.to_bytes())
            .map_err(io_err(SETTINGS_FILE, "write"))?;
        self.files
            .write(openbmt_courier::INBOX_FILE, &Inbox::default().to_bytes())
            .map_err(io_err(openbmt_courier::INBOX_FILE, "write"))?;
        self.files
            .write(openbmt_courier::OUTBOX_FILE, &Outbox::default().to_bytes())
            .map_err(io_err(openbmt_courier::OUTBOX_FILE, "write"))?;
        let mut out = wire::start(&MAGIC_ROOTS, 64);
        out.extend_from_slice(&roots.owner.to_bytes());
        out.extend_from_slice(&roots.approver.to_bytes());
        self.files
            .write(ROOTS_FILE, &out)
            .map_err(io_err(ROOTS_FILE, "write"))?;
        self.pairing = Some(Pairing { roots, signed });
        Ok(())
    }

    /// Take new signed settings (BX-21): they must check against the pinned
    /// roots and carry a higher version than the ones held. They are stored
    /// before they take effect. Settings that no longer list this Box turn
    /// it off and erase its POS token (BX-23).
    pub fn take_settings(&mut self, settings: &[u8]) -> Result<SettingsTaken, BoxError> {
        let pairing = self.pairing.as_ref().ok_or(BoxError::NotPaired)?;
        let signed = SignedSettings::from_bytes(settings).map_err(BoxError::BadSettings)?;
        signed
            .verify(&pairing.roots, Some(&pairing.signed.settings))
            .map_err(BoxError::Settings)?;
        let listed = signed.settings.boxes.contains(&self.id);
        if listed && member_list(&signed.settings, &pairing.roots.owner).len() > MAX_MEMBERS {
            return Err(BoxError::TooManyMembers);
        }
        self.files
            .write(SETTINGS_FILE, &signed.to_bytes())
            .map_err(io_err(SETTINGS_FILE, "write"))?;
        if let Some(p) = self.pairing.as_mut() {
            p.signed = signed;
        }
        let token_error = if listed {
            None
        } else {
            self.erase_pos_token().err().map(|e| e.kind())
        };
        Ok(SettingsTaken {
            turned_off: !listed,
            token_error,
        })
    }

    /// Erase the POS token from storage (BX-23). Nothing to do if there is
    /// none.
    pub fn erase_pos_token(&mut self) -> io::Result<()> {
        self.files.remove(POS_TOKEN_FILE)
    }

    /// The relay's member list (BX-22): the device list of the settings in
    /// force and the owner's key. Empty while the Box is not paired
    /// (BX-03) and when it is turned off (BX-23).
    pub fn members(&self) -> Vec<DeviceId> {
        match &self.pairing {
            Some(p) if !self.turned_off() => member_list(&p.signed.settings, &p.roots.owner),
            _ => Vec::new(),
        }
    }

    /// What `key` is to the shop under the settings in force (BX-24).
    pub fn role_of(&self, key: &DeviceId) -> Option<DeviceRole> {
        let p = self.pairing.as_ref()?;
        if self.turned_off() {
            return None;
        }
        p.signed.settings.role_of(&p.roots.owner, key)
    }

    /// Every device the Box talks to, with its role (BX-24), without the
    /// Box itself.
    pub fn roles(&self) -> BTreeMap<DeviceId, DeviceRole> {
        self.members()
            .into_iter()
            .filter(|d| *d != self.id)
            .filter_map(|d| self.role_of(&d).map(|r| (d, r)))
            .collect()
    }
}
