//! The Key app's own state and its byte form (AS-36, AS-38).
//!
//! ```text
//! "OBM3" | 1 | wallet ID (32) | owner root (32)
//!   | signer flag u8, then u32 length and the `OBMK` bytes if 1
//!   | receive mark u32 LE (used when this device holds no signer)
//!   | settings flag u8, then u32 length and the `OBMS` bytes if 1
//!   | log count u16 LE | that many session IDs (32 each), oldest first
//!   | version 2 only: invitation (17) | expires at u64 LE
//! ```
//!
//! The version is 1 until the Key app holds an open Box invitation (AS-80)
//! and 2 while it does: a state without one is written exactly as before.

use std::sync::OnceLock;

use bitcoin::bip32::{Xpriv, Xpub};
use bitcoin::secp256k1::{All, Secp256k1};
use openbmt_core::batch::{Approver as BatchApprover, BatchPolicy, HighWater, PinnedRoots};
use openbmt_core::boxpair::{Invitation, INVITATION_RANDOM_LEN, INVITATION_SECS};
use openbmt_core::identity::{DeviceId, Identity};
use openbmt_core::settings::SignedSettings;
use openbmt_core::wallet::WalletDescription;
use openbmt_core::wire::{self, DecodeError, Reader};
use openbmt_session::Request;
use openbmt_signer::SignerState;

use crate::error::{RefuseSession, RestoreError, SetupError, StateError};
use crate::session::Session;
use crate::{MAGIC_APPROVER_STATE, MAX_SESSION_LOG};

pub(crate) fn secp() -> &'static Secp256k1<All> {
    static CTX: OnceLock<Secp256k1<All>> = OnceLock::new();
    CTX.get_or_init(Secp256k1::new)
}

/// What the Key app keeps, apart from its identity, its wallet copy and
/// the pinned owner root. A session works on a copy of this and the Key app
/// takes the copy only after the store succeeded (AS-34).
#[derive(Clone)]
pub(crate) struct Kept {
    pub signer: Option<SignerState>,
    pub high_water: HighWater,
    pub settings: Option<SignedSettings>,
    pub seen: Vec<[u8; 32]>,
    /// The open Box invitation (AS-80).
    pub invitation: Option<OpenInvitation>,
}

/// A Box invitation the Key app made and has not spent (BX-11).
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct OpenInvitation {
    pub invitation: Invitation,
    /// When it runs out, in seconds on the Key app's clock.
    pub expires_at: u64,
}

/// An invitation the Key app holds, as the screen shows it (AS-80).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationView {
    /// Thirty characters in six groups of five (BX-10).
    pub text: String,
    /// When it runs out, in seconds on the Key app's clock.
    pub expires_at: u64,
}

impl Kept {
    /// The receive mark: the signer's when this device holds key 2 (SR-70),
    /// the Key app's own mark when it does not.
    pub(crate) fn receive_mark(&self) -> u32 {
        match &self.signer {
            Some(s) => s.receive_mark(),
            None => self.high_water.next_index(),
        }
    }
}

/// The Key app's side of the approval session.
pub struct Approver {
    pub(crate) identity: Identity,
    pub(crate) batch: BatchApprover,
    pub(crate) wallet: WalletDescription,
    pub(crate) wallet_id: [u8; 32],
    pub(crate) owner: DeviceId,
    pub(crate) key2: Option<Xpriv>,
    pub(crate) kept: Kept,
}

impl Drop for Approver {
    fn drop(&mut self) {
        // Best effort: the compiler and the operating system may keep
        // other copies.
        if let Some(k) = self.key2.as_mut() {
            k.private_key.non_secure_erase();
        }
    }
}

impl std::fmt::Debug for Approver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Approver({:?})", self.identity.device_id())
    }
}

impl Approver {
    /// A Key app with an empty state. `secret` is the 32 bytes of the
    /// approver identity from a cryptographic random source. `owner` is the
    /// pinned owner root. The Key app is an approver only until
    /// [`Approver::attach_signer`] is called.
    pub fn new(
        secret: &[u8; 32],
        wallet: WalletDescription,
        owner: DeviceId,
        policy: BatchPolicy,
    ) -> Result<Self, SetupError> {
        let identity = Identity::from_secret_bytes(secret).map_err(SetupError::Identity)?;
        let for_batches = Identity::from_secret_bytes(secret).map_err(SetupError::Identity)?;
        if identity.device_id() == owner {
            return Err(SetupError::SameRoot);
        }
        let wallet_id = wallet.checksum32();
        Ok(Self {
            identity,
            batch: BatchApprover::new(for_batches, policy),
            wallet,
            wallet_id,
            owner,
            key2: None,
            kept: Kept {
                signer: None,
                high_water: HighWater::new(),
                settings: None,
                seen: Vec::new(),
                invitation: None,
            },
        })
    }

    /// AS-36: this device holds key 2. The signer's state must be for this
    /// wallet and `account_key` must be this device's key of it, in full.
    pub fn attach_signer(
        &mut self,
        state: SignerState,
        account_key: Xpriv,
    ) -> Result<(), SetupError> {
        if self.kept.signer.is_some() {
            return Err(SetupError::SignerAttached);
        }
        if state.wallet().checksum32() != self.wallet_id {
            return Err(SetupError::SignerWallet);
        }
        let slot = usize::from(state.own_slot()).checked_sub(1);
        let own = slot.and_then(|s| state.wallet().slots().get(s));
        match own {
            Some(slot) if slot.xpub == Xpub::from_priv(secp(), &account_key) => {}
            _ => return Err(SetupError::SignerKey),
        }
        self.kept.signer = Some(state);
        self.key2 = Some(account_key);
        Ok(())
    }

    /// The Key app's root.
    pub fn device_id(&self) -> DeviceId {
        self.identity.device_id()
    }

    /// The pinned owner root.
    pub fn owner(&self) -> DeviceId {
        self.owner
    }

    /// The wallet's ID.
    pub fn wallet_id(&self) -> &[u8; 32] {
        &self.wallet_id
    }

    /// The Key app's copy of the wallet description.
    pub fn wallet(&self) -> &WalletDescription {
        &self.wallet
    }

    /// The receive mark: one past the highest index co-signed in a batch.
    pub fn receive_mark(&self) -> u32 {
        self.kept.receive_mark()
    }

    /// The signer and its state, when this device holds key 2.
    pub fn signer(&self) -> Option<&SignerState> {
        self.kept.signer.as_ref()
    }

    /// The last settings the Key app co-signed.
    pub fn last_settings(&self) -> Option<&SignedSettings> {
        self.kept.settings.as_ref()
    }

    /// The finished sessions the Key app remembers, oldest first.
    pub fn seen_sessions(&self) -> &[[u8; 32]] {
        &self.kept.seen
    }

    /// AS-80: make an invitation from 17 random bytes the caller drew, good
    /// for 30 minutes from `now` (BX-11), and hand the new state to `persist`
    /// before it is returned. It replaces any invitation that was open.
    pub fn make_invitation<E>(
        &mut self,
        random: &[u8; INVITATION_RANDOM_LEN],
        now: u64,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<InvitationView, StateError<E>> {
        let open = OpenInvitation {
            invitation: Invitation::from_random(random),
            expires_at: now.saturating_add(INVITATION_SECS),
        };
        let view = InvitationView {
            text: open.invitation.to_text(),
            expires_at: open.expires_at,
        };
        let mut next = self.kept.clone();
        next.invitation = Some(open);
        persist(&self.snapshot_of(&next)).map_err(StateError::Store)?;
        self.kept = next;
        Ok(view)
    }

    /// AS-80: the invitation that is open and has not run out at `now`.
    pub fn open_invitation(&self, now: u64) -> Option<InvitationView> {
        self.kept
            .invitation
            .as_ref()
            .filter(|o| now < o.expires_at)
            .map(|o| InvitationView {
                text: o.invitation.to_text(),
                expires_at: o.expires_at,
            })
    }

    /// AS-80: forget the open invitation. Nothing is stored when none is held.
    pub fn cancel_invitation<E>(
        &mut self,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), StateError<E>> {
        if self.kept.invitation.is_none() {
            return Ok(());
        }
        let mut next = self.kept.clone();
        next.invitation = None;
        persist(&self.snapshot_of(&next)).map_err(StateError::Store)?;
        self.kept = next;
        Ok(())
    }

    /// AS-20: read a request and check it. Nothing is signed or stored.
    pub fn open(&mut self, bytes: &[u8], now: u64) -> Result<Session<'_>, RefuseSession> {
        if bytes.len() > openbmt_session::MAX_SESSION_BYTES {
            return Err(RefuseSession::TooLarge);
        }
        let request = Request::from_bytes(bytes).map_err(RefuseSession::Frame)?;
        if request.owner() != self.owner {
            return Err(RefuseSession::WrongOwner);
        }
        if request.verify(&self.owner).is_err() {
            return Err(RefuseSession::BadSignature);
        }
        if *request.wallet() != self.wallet_id {
            return Err(RefuseSession::WrongWallet);
        }
        if self.kept.seen.contains(request.session_id()) {
            return Err(RefuseSession::SeenBefore);
        }
        Ok(Session::new(self, request, now))
    }

    /// The pinned roots, for the checks of the core crate.
    pub(crate) fn roots(&self) -> Option<PinnedRoots> {
        PinnedRoots::new(self.owner, self.identity.device_id()).ok()
    }

    // ---- the Key app's own screen -----------------------------------------

    /// A change made on the Key app's own screen to the signer's state
    /// (enrolling a destination, setting ceilings, raising the change
    /// mark). `f` works on a copy and may call any of the signer's own-screen
    /// calls with a `persist` that does nothing; the whole new snapshot is
    /// then stored, and only if that succeeds does the Key app take it. A
    /// call that leaves the snapshot's bytes as they are does not store.
    pub fn change_signer<E>(
        &mut self,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
        f: impl FnOnce(&mut SignerState) -> Result<(), String>,
    ) -> Result<(), StateError<E>> {
        let mut next = self.kept.clone();
        let signer = next.signer.as_mut().ok_or(StateError::NoSigner)?;
        f(signer).map_err(StateError::Refused)?;
        let bytes = self.snapshot_of(&next);
        if bytes == self.snapshot() {
            // The call changed nothing (a clock correction that finds the
            // clock fine, for one): what is stored is already this state.
            return Ok(());
        }
        persist(&bytes).map_err(StateError::Store)?;
        self.kept = next;
        Ok(())
    }

    /// SR-70: raise the receive mark, on this device only. It only goes up.
    pub fn raise_receive_mark<E>(
        &mut self,
        new_mark: u32,
        persist: impl FnOnce(&[u8]) -> Result<(), E>,
    ) -> Result<(), StateError<E>> {
        let mut next = self.kept.clone();
        match next.signer.as_mut() {
            Some(s) => s
                .raise_receive_mark(new_mark, |_| Ok::<(), std::convert::Infallible>(()))
                .map_err(|e| StateError::Refused(e.to_string()))?,
            None => {
                if new_mark > 1 << 31 || new_mark <= next.high_water.next_index() {
                    return Err(StateError::Refused(
                        "SR-70: the mark only goes up, to at most 2^31".to_string(),
                    ));
                }
                next.high_water = HighWater::from_next_index(new_mark);
            }
        }
        persist(&self.snapshot_of(&next)).map_err(StateError::Store)?;
        self.kept = next;
        Ok(())
    }

    // ---- snapshot -----------------------------------------------------------

    /// AS-38: the Key app's state as bytes.
    pub fn snapshot(&self) -> Vec<u8> {
        self.snapshot_of(&self.kept)
    }

    pub(crate) fn snapshot_of(&self, kept: &Kept) -> Vec<u8> {
        let mut out = wire::start(&MAGIC_APPROVER_STATE, 256);
        if kept.invitation.is_some() {
            if let Some(v) = out.get_mut(4) {
                *v = 2;
            }
        }
        out.extend_from_slice(&self.wallet_id);
        out.extend_from_slice(&self.owner.to_bytes());
        match &kept.signer {
            Some(s) => {
                out.push(1);
                let bytes = s.to_bytes();
                out.extend_from_slice(
                    &u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes(),
                );
                out.extend_from_slice(&bytes);
            }
            None => out.push(0),
        }
        out.extend_from_slice(&kept.high_water.next_index().to_le_bytes());
        match &kept.settings {
            Some(s) => {
                out.push(1);
                let bytes = s.to_bytes();
                out.extend_from_slice(
                    &u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_le_bytes(),
                );
                out.extend_from_slice(&bytes);
            }
            None => out.push(0),
        }
        let count = u16::try_from(kept.seen.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&count.to_le_bytes());
        for id in &kept.seen {
            out.extend_from_slice(id);
        }
        if let Some(open) = &kept.invitation {
            out.extend_from_slice(open.invitation.as_bytes());
            out.extend_from_slice(&open.expires_at.to_le_bytes());
        }
        out
    }

    /// AS-38: a Key app from its stored state. `key2` is given when this
    /// device holds key 2, and then the snapshot must hold signer state, and
    /// the other way round.
    pub fn restore(
        secret: &[u8; 32],
        wallet: WalletDescription,
        owner: DeviceId,
        policy: BatchPolicy,
        key2: Option<Xpriv>,
        bytes: &[u8],
    ) -> Result<Self, RestoreError> {
        let mut approver = Self::new(secret, wallet, owner, policy).map_err(RestoreError::Setup)?;
        let d = RestoreError::Decode;
        let mut r = Reader::new(bytes);
        if r.take(4).map_err(d)? != MAGIC_APPROVER_STATE {
            return Err(d(DecodeError::BadMagic));
        }
        let version = match r.u8().map_err(d)? {
            v @ (1 | 2) => v,
            v => return Err(d(DecodeError::UnsupportedVersion(v))),
        };
        if r.array::<32>().map_err(d)? != approver.wallet_id {
            return Err(RestoreError::WrongWallet);
        }
        if r.array::<32>().map_err(d)? != approver.owner.to_bytes() {
            return Err(RestoreError::WrongOwner);
        }
        let signer_bytes = if r.flag().map_err(d)? {
            let n = usize::try_from(r.u32_le().map_err(d)?)
                .map_err(|_| RestoreError::Decode(DecodeError::TooLarge))?;
            Some(r.take(n).map_err(d)?)
        } else {
            None
        };
        let high_water = r.u32_le().map_err(d)?;
        if signer_bytes.is_some() && high_water != 0 {
            return Err(RestoreError::Decode(DecodeError::NonCanonical(
                "a receive mark is kept next to the signer's own",
            )));
        }
        let settings = if r.flag().map_err(d)? {
            let n = usize::try_from(r.u32_le().map_err(d)?)
                .map_err(|_| RestoreError::Decode(DecodeError::TooLarge))?;
            Some(SignedSettings::from_bytes(r.take(n).map_err(d)?).map_err(d)?)
        } else {
            None
        };
        let count = usize::from(r.u16_le().map_err(d)?);
        if count > MAX_SESSION_LOG {
            return Err(RestoreError::Decode(DecodeError::TooLarge));
        }
        let mut seen: Vec<[u8; 32]> = Vec::with_capacity(count);
        for _ in 0..count {
            let id = r.array::<32>().map_err(d)?;
            if seen.contains(&id) {
                return Err(RestoreError::Decode(DecodeError::NonCanonical(
                    "a session ID is listed twice",
                )));
            }
            seen.push(id);
        }
        let invitation = if version == 2 {
            let raw = r.array::<INVITATION_RANDOM_LEN>().map_err(d)?;
            if raw.last().is_some_and(|b| b & 0xfc != 0) {
                return Err(d(DecodeError::NonCanonical("an invitation's spare bits")));
            }
            let expires_at = r.u64_le().map_err(d)?;
            Some(OpenInvitation {
                invitation: Invitation::from_random(&raw),
                expires_at,
            })
        } else {
            None
        };
        r.finish().map_err(d)?;
        if let Some(s) = &settings {
            let roots = approver.roots().ok_or(RestoreError::BadSettings)?;
            if s.approver_sig.is_none() || s.verify(&roots, None).is_err() {
                return Err(RestoreError::BadSettings);
            }
        }
        match (signer_bytes, key2) {
            (Some(sb), Some(key)) => {
                let xpub = Xpub::from_priv(secp(), &key);
                let state =
                    SignerState::from_bytes(sb, approver.wallet.network(), &xpub).map_err(d)?;
                approver
                    .attach_signer(state, key)
                    .map_err(RestoreError::Setup)?;
            }
            (None, None) => {}
            _ => return Err(RestoreError::SignerMismatch),
        }
        approver.kept.high_water = HighWater::from_next_index(high_water);
        approver.kept.settings = settings;
        approver.kept.seen = seen;
        approver.kept.invitation = invitation;
        Ok(approver)
    }
}
