//! What can go wrong, each naming its rule.

use std::fmt;

use bitcoin::Txid;
use openbmt_core::identity::IdentityError;
use openbmt_core::wire::DecodeError;
use openbmt_session::{BindError, EncodeError};

/// Why a call was refused. Nothing was changed and nothing was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// AS-50: a bound is reached. The text names what is full.
    Full(&'static str),
    /// AS-07: a text is empty, too long or holds a control character.
    BadText(&'static str),
    /// AS-51: the destination is not a script of a kind the signer allows.
    DestinationKind,
    /// AS-51: the amount is below the dust limit of the destination's kind,
    /// or is zero.
    BelowDust {
        /// The amount asked for.
        sats: u64,
        /// The dust limit of the destination.
        dust: u64,
    },
    /// AS-51: an amount above 21,000,000 BTC.
    AmountTooLarge,
    /// AS-52: the sale has no facts, so nothing is known to be received.
    UnknownSale,
    /// AS-52: the refunds of the sale would add up to more than was
    /// received.
    MoreThanReceived {
        /// What the sale's facts say was received.
        received: u64,
        /// What the refunds would add up to.
        wanted: u64,
    },
    /// AS-50, AS-52: the sale cannot be forgotten while refunds or paid
    /// refunds are counted against it.
    SaleInUse,
    /// There is no queue entry with this number.
    UnknownEntry(u32),
    /// AS-54, AS-66: the entry is in a stage that does not allow this call.
    WrongStage(&'static str),
    /// AS-58: a batch does not equal what the wallet derives for it.
    BatchNotDerived,
    /// AS-58: a pairing request is not one the Key app's offer accepts.
    PairingRequest(String),
    /// AS-59: settings that do not pass the settings rules.
    SettingsInvalid(String),
    /// AS-59: a version that is not above every version known.
    SettingsVersion {
        /// The highest version known.
        known: u32,
        /// The version offered.
        got: u32,
    },
    /// AS-61: the fee rate is 0.
    ZeroFeeRate,
    /// AS-61, AS-02: the request could not be built.
    Encode(EncodeError),
    /// AS-61: this session ID is already used by an open session.
    SessionIdUsed,
    /// AS-62: the file is not a response in canonical form.
    Frame(DecodeError),
    /// AS-62: no open session has this ID.
    UnknownSession,
    /// AS-13, AS-62: the response is not bound to the request.
    Bind(BindError),
    /// AS-64, AS-50: the finished objects waiting to be delivered would
    /// pass the bound.
    OutboxFull,
    /// AS-64: no transaction waiting has this ID.
    UnknownTransaction(Txid),
    /// AS-65, AS-50: the stored snapshot would be above its bound.
    SnapshotTooLarge,
    /// AS-62: the key-1 state is not for this wallet.
    Key1Wallet,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full(what) => write!(f, "AS-50: {what} is full"),
            Self::BadText(what) => write!(f, "AS-07: the {what} text is not allowed"),
            Self::DestinationKind => write!(
                f,
                "AS-51: the destination is not a P2WPKH, P2WSH, P2TR, P2PKH or P2SH script"
            ),
            Self::BelowDust { sats, dust } => write!(
                f,
                "AS-51: {sats} sat is below the dust limit of {dust} sat for this destination"
            ),
            Self::AmountTooLarge => write!(f, "AS-51: an amount is above 21,000,000 BTC"),
            Self::UnknownSale => write!(f, "AS-52: no facts are known for this sale"),
            Self::MoreThanReceived { received, wanted } => write!(
                f,
                "AS-52: the refunds of this sale would add up to {wanted} sat, and {received} sat \
                 was received"
            ),
            Self::SaleInUse => write!(
                f,
                "AS-52: refunds are counted against this sale; its facts stay"
            ),
            Self::UnknownEntry(n) => write!(f, "AS-54: the queue has no entry {n}"),
            Self::WrongStage(what) => write!(f, "AS-54: {what}"),
            Self::BatchNotDerived => write!(
                f,
                "AS-58: the batch is not what the wallet derives for that counter and range"
            ),
            Self::PairingRequest(why) => {
                write!(f, "AS-58: the pairing request is refused: {why}")
            }
            Self::SettingsInvalid(why) => {
                write!(f, "AS-59: the settings are refused: {why}")
            }
            Self::SettingsVersion { known, got } => write!(
                f,
                "AS-59: version {got} is not above the highest version known, {known}"
            ),
            Self::ZeroFeeRate => write!(f, "AS-61: the fee rate is 0"),
            Self::Encode(e) => write!(f, "AS-61: the request cannot be built: {e}"),
            Self::SessionIdUsed => write!(f, "AS-61: an open session has this ID"),
            Self::Frame(e) => write!(f, "AS-62: the response is not in canonical form: {e}"),
            Self::UnknownSession => write!(f, "AS-62: no open session has this ID"),
            Self::Bind(e) => write!(f, "AS-62: {e}"),
            Self::OutboxFull => write!(
                f,
                "AS-64: too many finished objects are waiting to be delivered"
            ),
            Self::UnknownTransaction(t) => {
                write!(f, "AS-64: no transaction {t} is waiting")
            }
            Self::SnapshotTooLarge => write!(f, "AS-65: the snapshot would be above its bound"),
            Self::Key1Wallet => write!(f, "AS-62: the key-1 state is for another wallet"),
        }
    }
}

impl std::error::Error for Refusal {}

/// A refusal, or the caller's store failing (AS-61, AS-64). In both cases
/// nothing was changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerError<E> {
    /// A rule refused.
    Refused(Refusal),
    /// The store failed. Nothing was changed in memory.
    Store(E),
}

impl<E> From<Refusal> for OwnerError<E> {
    fn from(r: Refusal) -> Self {
        Self::Refused(r)
    }
}

impl<E: fmt::Display> fmt::Display for OwnerError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::Store(e) => write!(f, "AS-64: the new state could not be stored: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for OwnerError<E> {}

/// Why the owner phone could not be set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    /// The owner secret is not a valid key.
    Identity(IdentityError),
    /// The owner and the Key app have the same root.
    SameRoot,
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(e) => write!(f, "AS-65: the owner key is not valid: {e}"),
            Self::SameRoot => write!(f, "AS-65: the owner and the Key app share a root"),
        }
    }
}

impl std::error::Error for SetupError {}

/// Why a stored snapshot was refused (AS-65).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// The set-up was refused.
    Setup(SetupError),
    /// The bytes are not a snapshot in canonical form.
    Decode(DecodeError),
    /// The snapshot is for another wallet.
    WrongWallet,
    /// The snapshot is for another owner.
    WrongOwner,
    /// The snapshot is for another Key app.
    WrongApprover,
    /// The snapshot decodes but contradicts itself. The text says how.
    Inconsistent(&'static str),
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup(e) => write!(f, "{e}"),
            Self::Decode(e) => write!(f, "AS-65: the snapshot is not in canonical form: {e}"),
            Self::WrongWallet => write!(f, "AS-65: the snapshot is for another wallet"),
            Self::WrongOwner => write!(f, "AS-65: the snapshot is for another owner"),
            Self::WrongApprover => write!(f, "AS-65: the snapshot is for another Key app"),
            Self::Inconsistent(why) => write!(f, "AS-65: the snapshot contradicts itself: {why}"),
        }
    }
}

impl std::error::Error for RestoreError {}
