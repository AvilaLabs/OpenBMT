//! What can go wrong, each naming its rule.

use std::fmt;

use bitcoin::Txid;
use openbmt_core::currency::ShopCurrency;
use openbmt_core::identity::IdentityError;
use openbmt_core::wire::DecodeError;
use openbmt_session::{BindError, EncodeError};

/// What a counter's bundle still lacks (AS-72).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleGap {
    /// No finished pairing of the counter waits: the Key app has not
    /// approved it, or it was delivered already.
    Pairing,
    /// No settings signed by both roots list the counter above the
    /// pairing's version.
    Settings,
}

impl fmt::Display for BundleGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pairing => write!(f, "no finished pairing of the counter waits"),
            Self::Settings => write!(
                f,
                "no settings with both signatures list the counter above the pairing's version"
            ),
        }
    }
}

/// Why the text given as the Lightning node was refused (AS-76).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKeyFault {
    /// The text holds something other than hexadecimal digits, or is longer
    /// than a key, or is empty.
    NotAKey,
    /// The key is cut short or abbreviated: fewer than 66 digits, or digits
    /// with dots in them.
    ShortForm,
    /// 66 digits that do not start with 02 or 03, so not a compressed key.
    NotCompressed,
    /// 66 digits that start with 02 or 03 and are not a point on the curve.
    NotOnCurve,
    /// A node address whose part after the @ is not a host and a port.
    Address,
}

impl fmt::Display for NodeKeyFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAKey => write!(f, "it is not a node key"),
            Self::ShortForm => write!(f, "it is cut short or abbreviated"),
            Self::NotCompressed => write!(f, "it does not start with 02 or 03"),
            Self::NotOnCurve => write!(f, "it is not a point on the curve"),
            Self::Address => write!(f, "what follows the @ is not a host and a port"),
        }
    }
}

/// What the hand-over to a Box still lacks (AS-86).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxGap {
    /// No finished welcome of the Box waits: the Key app has not approved
    /// its claim, or it was handed over already.
    Welcome,
    /// No settings signed by both roots list the Box as a Box.
    Settings,
}

impl fmt::Display for BoxGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Welcome => write!(f, "no finished welcome of the Box waits"),
            Self::Settings => write!(f, "no settings with both signatures list the Box as a Box"),
        }
    }
}

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
    /// OC-11: a batch overlaps a range of receive indices that was issued
    /// already.
    BatchOverlaps {
        /// The first index of the batch.
        start: u32,
        /// How many indices the batch holds.
        count: u32,
    },
    /// OC-11: the most ranges that are kept as issued are kept already.
    IssuedRangesFull,
    /// AS-58: a pairing request is not one the Key app's offer accepts. The
    /// text says why.
    PairingRequest(String),
    /// AS-58: the bytes are not a counter's pairing request. The text is the
    /// decoder's.
    PairingNotRequest(String),
    /// AS-58: the request was made for another shop's offer or another
    /// network: its binding is not this owner's and Key app's offer.
    PairingOtherShop,
    /// AS-69: the counter is on a device list, in the queue or finished and
    /// waiting already.
    PairingKnown,
    /// AS-70: the owner phone holds no settings and was given no bitcoin
    /// price to start them from.
    NoSettingsRate,
    /// AS-72: a counter's bundle cannot be made yet: something it needs is
    /// not here.
    BundleNotReady(BundleGap),
    /// AS-74: the owner phone holds no settings, held or queued, to change.
    NoSettingsToChange,
    /// AS-75: a changed limit in the shop's currency (the one given) needs
    /// today's price of a bitcoin and the edit gave none.
    EditNeedsRate(ShopCurrency),
    /// AS-75: the price of a bitcoin given, in the currency given, is
    /// outside the band of that currency ($1,000 to $10,000,000 for
    /// dollars).
    PriceOutOfRange(ShopCurrency),
    /// AS-76: the text given as the Lightning node is not a node key or
    /// address.
    LightningNode(NodeKeyFault),
    /// AS-83: the bytes are not a Box's claim. The text is the decoder's.
    BoxNotClaim(String),
    /// AS-83: the claim names the owner phone's or the Key app's own key.
    BoxIsRoot,
    /// AS-83: the Box is on the settings held or queued, or its claim is
    /// queued or its welcome waits already.
    BoxKnown,
    /// AS-86: the Box cannot be handed its welcome yet: something it needs
    /// is not here.
    BoxNotReady(BoxGap),
    /// AS-87: no claim of this Box is queued and no welcome of it waits, so
    /// there is nothing to forget.
    BoxNothingToForget,
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
    /// AS-90 to AS-99: a step with a hardware signer was refused.
    Hardware(crate::hardware::HardwareRefusal),
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
            Self::BatchOverlaps { start, count } => write!(
                f,
                "OC-11: the batch of {count} indices from {start} overlaps a range issued already"
            ),
            Self::IssuedRangesFull => write!(
                f,
                "OC-11: the most issued ranges are kept already; no new batch is signed"
            ),
            Self::PairingRequest(why) => {
                write!(f, "AS-58: the pairing request is refused: {why}")
            }
            Self::PairingNotRequest(why) => {
                write!(f, "AS-58: this is not a counter's pairing request: {why}")
            }
            Self::PairingOtherShop => write!(
                f,
                "AS-58: the pairing request was made for another offer: another shop or network"
            ),
            Self::PairingKnown => write!(f, "AS-69: this counter is paired or waiting already"),
            Self::BoxNotClaim(why) => {
                write!(f, "AS-83: this is not a Box's claim: {why}")
            }
            Self::BoxIsRoot => write!(
                f,
                "AS-83: the claim names the key of the owner phone or of the Key app"
            ),
            Self::BoxKnown => write!(f, "AS-83: this Box is added or waiting already"),
            Self::BoxNotReady(gap) => write!(f, "AS-86: {gap}"),
            Self::BoxNothingToForget => {
                write!(
                    f,
                    "AS-87: no claim of this Box is queued and no welcome of it waits"
                )
            }
            Self::NoSettingsRate => write!(
                f,
                "AS-70: no settings are held and no bitcoin price was given to start them"
            ),
            Self::NoSettingsToChange => {
                write!(f, "AS-74: no settings are held or queued to change")
            }
            Self::EditNeedsRate(c) => write!(
                f,
                "AS-75: a limit in {} changed and no price of a bitcoin was given",
                c.plural()
            ),
            Self::PriceOutOfRange(c) => write!(
                f,
                "AS-75: the price of a bitcoin is outside the band of {}",
                c.plural()
            ),
            Self::LightningNode(why) => write!(f, "AS-76: the Lightning node is refused: {why}"),
            Self::BundleNotReady(gap) => write!(f, "AS-72: the bundle is not ready: {gap}"),
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
            Self::Hardware(h) => write!(f, "{h}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// A refusal, or the caller's store failing (AS-61, AS-64). In both cases
/// nothing was changed in memory; a storage error can leave disk uncertain.
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
