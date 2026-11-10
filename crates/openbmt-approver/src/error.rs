//! What can go wrong, each naming its rule.

use std::fmt;

use openbmt_core::identity::IdentityError;
use openbmt_core::wire::DecodeError;
use openbmt_session::EncodeError;

/// Why a request was not opened (AS-20).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefuseSession {
    /// AS-02: the file is above the size limit.
    TooLarge,
    /// AS-01, AS-03, AS-04: the file is not a request in canonical form.
    Frame(DecodeError),
    /// AS-20: the owner key in the file is not the pinned owner root.
    WrongOwner,
    /// AS-05: the owner's signature does not verify.
    BadSignature,
    /// AS-20: the request is for another wallet.
    WrongWallet,
    /// AS-22: this session ID was finished before.
    SeenBefore,
}

impl fmt::Display for RefuseSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "AS-02: the request is above 8 MiB"),
            Self::Frame(e) => write!(f, "AS-03: the request is not in canonical form: {e}"),
            Self::WrongOwner => write!(f, "AS-20: the request is not from the pinned owner"),
            Self::BadSignature => write!(f, "AS-20: the owner's signature is not valid"),
            Self::WrongWallet => write!(f, "AS-20: the request is for another wallet"),
            Self::SeenBefore => write!(f, "AS-22: this session was finished before"),
        }
    }
}

impl std::error::Error for RefuseSession {}

/// Why a decision was not taken (AS-30).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecideError {
    /// The session was finished.
    Finished,
    /// The request has no item with this number.
    UnknownItem(u32),
    /// AS-23, AS-30: the line cannot be approved. The text is the failing
    /// rule.
    CannotApprove(String),
    /// AS-30: the reason is empty, over 160 bytes or holds a control
    /// character.
    BadReason,
}

impl fmt::Display for DecideError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Finished => write!(f, "AS-34: the session was finished"),
            Self::UnknownItem(n) => write!(f, "AS-30: the request has no item {n}"),
            Self::CannotApprove(why) => {
                write!(f, "AS-23: the line cannot be approved: {why}")
            }
            Self::BadReason => write!(f, "AS-30: the reason must be 1 to 160 bytes of text"),
        }
    }
}

impl std::error::Error for DecideError {}

/// Why a finish returned no response (AS-34, AS-37).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishError<E> {
    /// The session was finished already.
    Finished,
    /// AS-37: the response could not be built (for example it would be
    /// above 8 MiB). Nothing was stored.
    Response(EncodeError),
    /// AS-34: the store failed. Nothing is returned, nothing the Key app
    /// keeps has changed, and the session can be finished again.
    Store(E),
}

impl<E: fmt::Display> fmt::Display for FinishError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Finished => write!(f, "AS-34: the session was finished already"),
            Self::Response(e) => write!(f, "AS-37: the response cannot be built: {e}"),
            Self::Store(e) => write!(f, "AS-34: could not store the new state: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for FinishError<E> {}

/// Why the Key app could not be set up (AS-36, AS-38).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    /// The identity secret is not a valid key.
    Identity(IdentityError),
    /// The pinned owner root is the Key app's own root.
    SameRoot,
    /// AS-36: the signer is for another wallet.
    SignerWallet,
    /// AS-36: the account key is not this device's key of the wallet, in
    /// full.
    SignerKey,
    /// AS-36: a signer is attached already.
    SignerAttached,
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(e) => write!(f, "AS-38: identity: {e}"),
            Self::SameRoot => write!(f, "AS-38: the owner root is the Key app's own root"),
            Self::SignerWallet => write!(f, "AS-36: the signer is for another wallet"),
            Self::SignerKey => write!(f, "AS-36: the account key is not this device's key"),
            Self::SignerAttached => write!(f, "AS-36: a signer is attached already"),
        }
    }
}

impl std::error::Error for SetupError {}

/// Why a stored state was not taken back (AS-38).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// The bytes are not a snapshot in canonical form.
    Decode(DecodeError),
    /// The snapshot is for another wallet.
    WrongWallet,
    /// The snapshot is for another owner.
    WrongOwner,
    /// The snapshot holds signer state and no key 2 was given, or the other
    /// way round.
    SignerMismatch,
    /// The stored settings are not the Key app's own co-signed settings.
    BadSettings,
    /// The Key app could not be set up.
    Setup(SetupError),
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "AS-38: the snapshot is not in canonical form: {e}"),
            Self::WrongWallet => write!(f, "AS-38: the snapshot is for another wallet"),
            Self::WrongOwner => write!(f, "AS-38: the snapshot is for another owner"),
            Self::SignerMismatch => write!(
                f,
                "AS-38: the snapshot does not fit whether key 2 is on this device"
            ),
            Self::BadSettings => write!(
                f,
                "AS-38: the stored settings are not co-signed by this Key app"
            ),
            Self::Setup(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RestoreError {}

/// Why a change made on the Key app's own screen was not taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError<E> {
    /// The change was refused; the text says why.
    Refused(String),
    /// There is no signer on this device.
    NoSigner,
    /// The store failed. Nothing has changed.
    Store(E),
}

impl<E: fmt::Display> fmt::Display for StateError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(why) => write!(f, "{why}"),
            Self::NoSigner => write!(f, "AS-36: key 2 is not on this device"),
            Self::Store(e) => write!(f, "AS-34: could not store the new state: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for StateError<E> {}
