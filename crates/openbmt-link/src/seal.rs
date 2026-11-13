//! The sealed message (LK-01 to LK-09).
//!
//! ```text
//! OBMN | version u8 | reader key 32 | sender key 32 | number u64 LE |
//! kind u8 | one-time public key 33 | ciphertext hash 32 | signature 64 |
//! ciphertext
//! ```
//!
//! Everything before the ciphertext hash is the *prefix*. The prefix is the
//! associated data of the AEAD, so a changed prefix does not decrypt. The
//! signature covers the prefix's fields (without the magic) and the hash of
//! the ciphertext, so a changed or swapped ciphertext does not verify. The
//! hash itself cannot be in the associated data, because it is made from the
//! ciphertext; the signature is what ties the two together.
//!
//! The ciphertext is the padded content under ChaCha20-Poly1305 with an
//! all-zero nonce and a key that is new for every message (LK-02, LK-03).

use std::fmt;
use std::sync::OnceLock;

use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::{All, Parity, PublicKey, Scalar, Secp256k1, SecretKey, XOnlyPublicKey};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use openbmt_core::identity::{DeviceId, Identity, Sig};
use openbmt_core::wire::{DecodeError, Reader, VERSION};

use crate::hkdf;

/// Magic of a sealed message.
pub const MAGIC_SEALED: [u8; 4] = *b"OBMN";
/// The tag of the sender's signature, and the salt of the key derivation.
pub const TAG_LINK: &str = "OpenBMT/link/v1";
/// The size steps of LK-07: the padded plaintext is exactly one of these.
pub const SIZE_STEPS: [usize; 5] = [256, 1024, 4096, 16384, 65536];
/// Bytes the AEAD adds to the plaintext.
pub const AEAD_TAG_LEN: usize = 16;
/// Bytes of the prefix: magic, version, two keys, number, kind and the
/// one-time public key.
pub const PREFIX_LEN: usize = 4 + 1 + 32 + 32 + 8 + 1 + 33;
/// Bytes before the ciphertext: the prefix, the hash and the signature.
pub const HEADER_LEN: usize = PREFIX_LEN + 32 + 64;
/// The longest sealed message: the header and the largest step.
pub const MAX_SEALED_LEN: usize = HEADER_LEN + 65536 + AEAD_TAG_LEN;
/// The longest content that fits a step: the largest step less the 4-byte
/// length.
pub const MAX_CONTENT_LEN: usize = 65536 - 4;

/// The most a number may be above the last one taken from its sender
/// (LK-06): 2^32. One message cannot move a sender's numbers out of reach.
pub const MAX_NUMBER_JUMP: u64 = 1 << 32;

/// What a device is to the shop (LK-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The owner's phone.
    Owner,
    /// A counter.
    Counter,
    /// The Box.
    Box,
    /// The owner phone's background watcher, with the watch key the signed
    /// settings name (LK-74). It sends heartbeats and nothing else.
    Watch,
}

/// What a message is (LK-08). The byte is signed but not encrypted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The owner's heartbeat to a counter.
    Heartbeat,
    /// An owner command to a counter.
    Command,
    /// Signed settings from the owner.
    Settings,
    /// A signed address batch from the owner.
    Batch,
    /// A signed device list from the owner.
    DeviceList,
    /// A counter's check-in.
    CheckIn,
    /// A counter's records.
    Records,
    /// A counter's signed "paid" to the Box.
    PaidNotice,
    /// A notice from the Box (a hint, CR-55).
    PosNotice,
    /// The owner telling a counter how many of its records it holds (LK-55).
    RecordsReceived,
    /// A signed index range from the owner, for a counter paired with the
    /// shop's own wallet (LK-77).
    Range,
    /// A switch from the starter wallet to a new one, from the owner to a
    /// counter paired with the starter wallet (LK-78).
    Switch,
    /// The starter line from the owner, to a counter paired with the starter
    /// wallet (LK-79).
    Starter,
    /// A write, read, list or remove of Shop Lightning's stored state, from
    /// the Lightning counter to the Box that keeps the second copy
    /// (LK-80). Byte 13 is the starter line's.
    Store,
    /// The Box's answer to a Store (LK-80).
    Stored,
    /// The Lightning counter's report to the owner (LK-84).
    LightningReport,
}

impl Kind {
    /// Every kind.
    pub const ALL: [Kind; 16] = [
        Kind::Heartbeat,
        Kind::Command,
        Kind::Settings,
        Kind::Batch,
        Kind::DeviceList,
        Kind::CheckIn,
        Kind::Records,
        Kind::PaidNotice,
        Kind::PosNotice,
        Kind::RecordsReceived,
        Kind::Range,
        Kind::Switch,
        Kind::Starter,
        Kind::Store,
        Kind::Stored,
        Kind::LightningReport,
    ];

    /// The byte that is signed.
    pub fn to_byte(self) -> u8 {
        match self {
            Kind::Heartbeat => 1,
            Kind::Command => 2,
            Kind::Settings => 3,
            Kind::Batch => 4,
            Kind::DeviceList => 5,
            Kind::CheckIn => 6,
            Kind::Records => 7,
            Kind::PaidNotice => 8,
            Kind::PosNotice => 9,
            Kind::RecordsReceived => 10,
            Kind::Range => 11,
            Kind::Switch => 12,
            Kind::Starter => 13,
            Kind::Store => 14,
            Kind::Stored => 15,
            Kind::LightningReport => 16,
        }
    }

    /// Whether messages of this kind take their numbers from a count of
    /// their own for each sender and reader pair (LK-81).
    pub fn counts_apart(self) -> bool {
        matches!(self, Kind::Store | Kind::Stored)
    }

    /// Whether this kind is one of Shop Lightning's (LK-80), which only the
    /// devices the settings name may send (see [`StoreParties`]).
    pub fn is_shop_lightning(self) -> bool {
        matches!(self, Kind::Store | Kind::Stored | Kind::LightningReport)
    }

    /// The kind for a byte; `None` for an unknown one (LK-08).
    pub fn from_byte(b: u8) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.to_byte() == b)
    }

    /// Whether a device in role `from` may send this kind to a device in
    /// role `to` (the table of LK-08).
    pub fn allowed(self, from: Role, to: Role) -> bool {
        use Role::{Box as TheBox, Counter, Owner, Watch};
        match self {
            Kind::Heartbeat => (from == Owner || from == Watch) && to == Counter,
            Kind::Command
            | Kind::Batch
            | Kind::RecordsReceived
            | Kind::Range
            | Kind::Switch
            | Kind::Starter => from == Owner && to == Counter,
            Kind::Settings | Kind::DeviceList => from == Owner && (to == Counter || to == TheBox),
            Kind::CheckIn | Kind::Records => from == Counter && to == Owner,
            Kind::PaidNotice => from == Counter && to == TheBox,
            Kind::PosNotice => from == TheBox && (to == Counter || to == Owner),
            Kind::Store => from == Counter && to == TheBox,
            Kind::Stored => from == TheBox && to == Counter,
            Kind::LightningReport => from == Counter && to == Owner,
        }
    }
}

/// The two devices Shop Lightning's kinds are for (LK-80), taken from the
/// settings in force: the Lightning counter and the Box that is its second
/// device. The role table lets any counter and any Box send these kinds; a
/// device that is not named here may not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreParties {
    /// The Lightning counter.
    pub counter: DeviceId,
    /// The Box that keeps its second copy.
    pub second_device: DeviceId,
}

impl StoreParties {
    /// The parties the settings name, or `None` with Shop Lightning off.
    pub fn from_settings(settings: &openbmt_core::settings::Settings) -> Option<Self> {
        settings.shop_lightning.as_ref().map(|sl| Self {
            counter: sl.counter,
            second_device: sl.second_device,
        })
    }

    /// Whether `sender` may send a message of `kind` to `reader`, as far as
    /// Shop Lightning goes (LK-80). A kind that is not Shop Lightning's is
    /// not this check's: it passes. The reader of a report is the owner,
    /// whom the role table names, so only its sender is checked.
    pub fn allows(
        parties: Option<&Self>,
        kind: Kind,
        sender: &DeviceId,
        reader: &DeviceId,
    ) -> bool {
        if !kind.is_shop_lightning() {
            return true;
        }
        let Some(p) = parties else {
            return false;
        };
        match kind {
            Kind::Store => *sender == p.counter && *reader == p.second_device,
            Kind::Stored => *sender == p.second_device && *reader == p.counter,
            _ => *sender == p.counter,
        }
    }
}

/// Why a message was not sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SealError {
    /// The sender's role may not send this kind to the reader's role.
    KindNotAllowed,
    /// The content does not fit the largest step.
    TooLarge,
    /// Message numbers start at 1.
    NumberZero,
    /// The randomness failed.
    Randomness,
    /// The randomness gave bytes that are not a valid secret key.
    BadOneTimeKey,
    /// The reader's key could not be used.
    BadReaderKey,
    /// The cipher refused.
    Cipher,
}

impl fmt::Display for SealError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KindNotAllowed => write!(f, "LK-08: this role may not send this kind there"),
            Self::TooLarge => write!(f, "LK-07: content is larger than the largest size step"),
            Self::NumberZero => write!(f, "LK-06: message numbers start at 1"),
            Self::Randomness => write!(f, "LK-03: the randomness failed"),
            Self::BadOneTimeKey => write!(f, "LK-03: the randomness is not a valid key"),
            Self::BadReaderKey => write!(f, "LK-02: the reader's key cannot be used"),
            Self::Cipher => write!(f, "LK-02: the cipher refused"),
        }
    }
}

impl std::error::Error for SealError {}

/// Why a message was not opened. Nothing of the content comes with any of
/// them (LK-05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// Not a well-formed frame.
    Frame(DecodeError),
    /// The message is for another device.
    WrongReader,
    /// The sender is not on the reader's device list.
    SenderNotListed,
    /// The kind byte is not a known kind.
    UnknownKind(u8),
    /// The sender's role may not send this kind to this reader.
    KindNotAllowed(Kind),
    /// The signature is not the sender's over this message.
    BadSignature,
    /// The ciphertext is not the one the signature covers.
    CiphertextHash,
    /// The number is not above the last one taken from this sender.
    Replay {
        /// The last number taken.
        last: u64,
        /// The number in the message.
        got: u64,
    },
    /// The number is more than [`MAX_NUMBER_JUMP`] above the last one taken
    /// from this sender.
    NumberTooFar {
        /// The last number taken.
        last: u64,
        /// The number in the message.
        got: u64,
    },
    /// The ciphertext did not open.
    Decrypt,
    /// The padding is not the one LK-07 gives.
    Padding,
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(e) => write!(f, "LK-05: not a sealed message: {e}"),
            Self::WrongReader => write!(f, "LK-05: sealed for another device"),
            Self::SenderNotListed => write!(f, "LK-05: sender is not on the device list"),
            Self::UnknownKind(k) => write!(f, "LK-08: unknown kind {k}"),
            Self::KindNotAllowed(k) => {
                write!(f, "LK-08: {k:?} is not sent from this role to this role")
            }
            Self::BadSignature => write!(f, "LK-04: signature is not the sender's"),
            Self::CiphertextHash => write!(f, "LK-04: ciphertext is not the one signed"),
            Self::Replay { last, got } => {
                write!(f, "LK-06: number {got} is not above the last, {last}")
            }
            Self::NumberTooFar { last, got } => {
                write!(f, "LK-06: number {got} is too far above the last, {last}")
            }
            Self::Decrypt => write!(f, "LK-02: the ciphertext does not open"),
            Self::Padding => write!(f, "LK-07: the padding is not well formed"),
        }
    }
}

impl std::error::Error for OpenError {}

/// A message that opened.
#[derive(Clone, PartialEq, Eq)]
pub struct Opened {
    /// The device that sent it.
    pub sender: DeviceId,
    /// Its number (LK-06). Store it before the content is used.
    pub number: u64,
    /// What it is.
    pub kind: Kind,
    /// The content, without padding.
    pub content: Vec<u8>,
}

impl fmt::Debug for Opened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Opened({:?}, #{}, {:?}, {} bytes)",
            self.sender,
            self.number,
            self.kind,
            self.content.len()
        )
    }
}

/// 32 secret bytes that are overwritten when dropped. No `Debug`, no
/// `Clone`.
struct Secret32([u8; 32]);

impl Drop for Secret32 {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            *b = 0;
        }
        std::hint::black_box(&mut self.0);
    }
}

/// The smallest size step that holds `content_len` bytes and their 4-byte
/// length; `None` if none does.
pub fn step_for(content_len: usize) -> Option<usize> {
    let need = content_len.checked_add(4)?;
    SIZE_STEPS.into_iter().find(|s| *s >= need)
}

/// The padded plaintext of LK-07: length u32 LE, content, zeros.
fn pad(content: &[u8]) -> Result<Vec<u8>, SealError> {
    let step = step_for(content.len()).ok_or(SealError::TooLarge)?;
    let len = u32::try_from(content.len()).map_err(|_| SealError::TooLarge)?;
    let mut out = Vec::with_capacity(step);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(content);
    out.resize(step, 0);
    Ok(out)
}

/// Undo [`pad`], refusing any padding that is not exactly what `pad` makes.
fn unpad(plain: &[u8]) -> Result<Vec<u8>, OpenError> {
    let head: [u8; 4] = plain
        .get(..4)
        .and_then(|h| h.try_into().ok())
        .ok_or(OpenError::Padding)?;
    let len = usize::try_from(u32::from_le_bytes(head)).map_err(|_| OpenError::Padding)?;
    let end = len.checked_add(4).ok_or(OpenError::Padding)?;
    let content = plain.get(4..end).ok_or(OpenError::Padding)?;
    if step_for(len) != Some(plain.len()) {
        return Err(OpenError::Padding);
    }
    if plain
        .get(end..)
        .is_none_or(|rest| rest.iter().any(|b| *b != 0))
    {
        return Err(OpenError::Padding);
    }
    Ok(content.to_vec())
}

fn secp() -> &'static Secp256k1<All> {
    static CTX: OnceLock<Secp256k1<All>> = OnceLock::new();
    CTX.get_or_init(Secp256k1::new)
}

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    sha256::Hash::hash(bytes).to_byte_array()
}

/// What the sender signs (LK-04).
pub(crate) fn signed_bytes(
    reader: &[u8; 32],
    sender: &[u8; 32],
    number: u64,
    kind: u8,
    one_time: &[u8; 33],
    ct_hash: &[u8; 32],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(139);
    out.push(VERSION);
    out.extend_from_slice(reader);
    out.extend_from_slice(sender);
    out.extend_from_slice(&number.to_le_bytes());
    out.push(kind);
    out.extend_from_slice(one_time);
    out.extend_from_slice(ct_hash);
    out
}

/// The message key (LK-02).
fn message_key(
    shared_x: &[u8; 32],
    one_time: &[u8; 33],
    reader: &[u8; 32],
    sender: &[u8; 32],
) -> Secret32 {
    let mut info = Vec::with_capacity(97);
    info.extend_from_slice(one_time);
    info.extend_from_slice(reader);
    info.extend_from_slice(sender);
    Secret32(hkdf::derive32(TAG_LINK.as_bytes(), shared_x, &info))
}

fn lift(reader: &DeviceId) -> Option<PublicKey> {
    let x = XOnlyPublicKey::from_slice(&reader.to_bytes()).ok()?;
    Some(PublicKey::from_x_only_public_key(x, Parity::Even))
}

fn cipher(key: &Secret32) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(&Key::from(key.0))
}

/// Seal already padded plaintext with the given one-time secret. Internal:
/// [`seal`] pads and draws the secret; the tests use this to build messages
/// `seal` never would.
pub(crate) fn build(
    sender: &Identity,
    reader: &DeviceId,
    number: u64,
    kind: Kind,
    plain: &[u8],
    mut one_time: SecretKey,
) -> Result<Vec<u8>, SealError> {
    let secp = secp();
    let one_time_pub = one_time.public_key(secp).serialize();
    let result = (|| {
        let reader_point = lift(reader).ok_or(SealError::BadReaderKey)?;
        let scalar =
            Scalar::from_be_bytes(one_time.secret_bytes()).map_err(|_| SealError::BadOneTimeKey)?;
        let product = reader_point
            .mul_tweak(secp, &scalar)
            .map_err(|_| SealError::BadReaderKey)?;
        let full = product.serialize();
        let mut shared = Secret32([0; 32]);
        shared
            .0
            .copy_from_slice(full.get(1..33).ok_or(SealError::BadReaderKey)?);
        let reader_bytes = reader.to_bytes();
        let sender_bytes = sender.device_id().to_bytes();
        let key = message_key(&shared.0, &one_time_pub, &reader_bytes, &sender_bytes);

        let mut prefix = Vec::with_capacity(PREFIX_LEN);
        prefix.extend_from_slice(&MAGIC_SEALED);
        prefix.push(VERSION);
        prefix.extend_from_slice(&reader_bytes);
        prefix.extend_from_slice(&sender_bytes);
        prefix.extend_from_slice(&number.to_le_bytes());
        prefix.push(kind.to_byte());
        prefix.extend_from_slice(&one_time_pub);

        let ct = cipher(&key)
            .encrypt(
                &Nonce::from([0u8; 12]),
                Payload {
                    msg: plain,
                    aad: &prefix,
                },
            )
            .map_err(|_| SealError::Cipher)?;
        let ct_hash = sha256_of(&ct);
        let signed = signed_bytes(
            &reader_bytes,
            &sender_bytes,
            number,
            kind.to_byte(),
            &one_time_pub,
            &ct_hash,
        );
        let sig = sender.sign_tagged(TAG_LINK, &signed);

        let mut out = prefix;
        out.extend_from_slice(&ct_hash);
        out.extend_from_slice(&sig.0);
        out.extend_from_slice(&ct);
        Ok(out)
    })();
    one_time.non_secure_erase();
    result
}

/// Seal `content` for `reader` (LK-01 to LK-08).
///
/// `random` fills 32 bytes with a fresh one-time secret; if it fails, so
/// does the seal (LK-03). `number` is the next number for this reader from
/// an [`Outbox`](crate::Outbox) and must already be stored (LK-40).
#[allow(clippy::too_many_arguments)]
pub fn seal<E>(
    sender: &Identity,
    sender_role: Role,
    reader: &DeviceId,
    reader_role: Role,
    number: u64,
    kind: Kind,
    content: &[u8],
    random: &mut impl FnMut(&mut [u8; 32]) -> Result<(), E>,
) -> Result<Vec<u8>, SealError> {
    if !kind.allowed(sender_role, reader_role) {
        return Err(SealError::KindNotAllowed);
    }
    if number == 0 {
        return Err(SealError::NumberZero);
    }
    let plain = pad(content)?;
    let mut bytes = Secret32([0; 32]);
    random(&mut bytes.0).map_err(|_| SealError::Randomness)?;
    let one_time = SecretKey::from_slice(&bytes.0).map_err(|_| SealError::BadOneTimeKey)?;
    build(sender, reader, number, kind, &plain, one_time)
}

/// The parts of a sealed message, checked as far as the frame goes.
struct Parsed<'a> {
    prefix: &'a [u8],
    reader: DeviceId,
    sender: DeviceId,
    number: u64,
    kind: u8,
    one_time: [u8; 33],
    ct_hash: [u8; 32],
    sig: Sig,
    ciphertext: &'a [u8],
}

fn parse(sealed: &[u8]) -> Result<Parsed<'_>, OpenError> {
    let frame = |e: DecodeError| OpenError::Frame(e);
    let mut r = Reader::framed(sealed, &MAGIC_SEALED).map_err(frame)?;
    let reader = r.device_id().map_err(frame)?;
    let sender = r.device_id().map_err(frame)?;
    let number = r.u64_le().map_err(frame)?;
    let kind = r.u8().map_err(frame)?;
    let one_time: [u8; 33] = r.array().map_err(frame)?;
    PublicKey::from_slice(&one_time).map_err(|_| frame(DecodeError::InvalidKey))?;
    let ct_hash: [u8; 32] = r.array().map_err(frame)?;
    let sig = r.sig().map_err(frame)?;
    let ciphertext = r
        .take(sealed.len().saturating_sub(HEADER_LEN))
        .map_err(frame)?;
    r.finish().map_err(frame)?;
    let plain_len = ciphertext
        .len()
        .checked_sub(AEAD_TAG_LEN)
        .ok_or(OpenError::Frame(DecodeError::Truncated))?;
    if !SIZE_STEPS.contains(&plain_len) {
        return Err(OpenError::Frame(DecodeError::NonCanonical(
            "ciphertext is not a size step",
        )));
    }
    let prefix = sealed
        .get(..PREFIX_LEN)
        .ok_or(OpenError::Frame(DecodeError::Truncated))?;
    Ok(Parsed {
        prefix,
        reader,
        sender,
        number,
        kind,
        one_time,
        ct_hash,
        sig,
        ciphertext,
    })
}

impl Parsed<'_> {
    /// LK-04: the ciphertext is the one signed, and the signature is the
    /// sender's.
    fn check_signature(&self) -> Result<(), OpenError> {
        let signed = signed_bytes(
            &self.reader.to_bytes(),
            &self.sender.to_bytes(),
            self.number,
            self.kind,
            &self.one_time,
            &self.ct_hash,
        );
        if !self.sender.verify_tagged(TAG_LINK, &signed, &self.sig) {
            return Err(OpenError::BadSignature);
        }
        if sha256_of(self.ciphertext) != self.ct_hash {
            return Err(OpenError::CiphertextHash);
        }
        Ok(())
    }
}

/// What a relay may see of a message it has checked (LK-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    /// The reader.
    pub reader: DeviceId,
    /// The sender.
    pub sender: DeviceId,
    /// The sender's number.
    pub number: u64,
    /// The kind.
    pub kind: Kind,
    /// The size step the content is padded to.
    pub step: usize,
}

/// What a relay does with a message (LK-20): the frame, the kind byte, the
/// signature and the hash, and no decryption. Nothing about the sender or
/// the number is checked against any list.
pub fn inspect(sealed: &[u8]) -> Result<Seen, OpenError> {
    let p = parse(sealed)?;
    let kind = Kind::from_byte(p.kind).ok_or(OpenError::UnknownKind(p.kind))?;
    p.check_signature()?;
    Ok(Seen {
        reader: p.reader,
        sender: p.sender,
        number: p.number,
        kind,
        step: p.ciphertext.len().saturating_sub(AEAD_TAG_LEN),
    })
}

/// Open a sealed message (LK-05), checking in this order and stopping at the
/// first failure: the frame; the reader; the sender on the list (`allowed`
/// gives its role); the kind (known, and sent by that role to this one: this
/// is LK-08's early refusal, before the signature and the decryption); the
/// signature and hash; the number above `last_number` for that sender and no more than
/// [`MAX_NUMBER_JUMP`] above it; the
/// ciphertext; the padding.
///
/// Opening does not store the number. Use [`Inbox::open`](crate::Inbox::open),
/// which stores it before it returns the content (LK-06).
pub fn open(
    reader: &Identity,
    reader_role: Role,
    sealed: &[u8],
    allowed: impl Fn(&DeviceId) -> Option<Role>,
    last_number: impl Fn(&DeviceId) -> u64,
) -> Result<Opened, OpenError> {
    let p = parse(sealed)?;
    if p.reader != reader.device_id() {
        return Err(OpenError::WrongReader);
    }
    let sender_role = allowed(&p.sender).ok_or(OpenError::SenderNotListed)?;
    let kind = Kind::from_byte(p.kind).ok_or(OpenError::UnknownKind(p.kind))?;
    if !kind.allowed(sender_role, reader_role) {
        return Err(OpenError::KindNotAllowed(kind));
    }
    p.check_signature()?;
    let last = last_number(&p.sender);
    if p.number <= last {
        return Err(OpenError::Replay {
            last,
            got: p.number,
        });
    }
    if p.number.saturating_sub(last) > MAX_NUMBER_JUMP {
        return Err(OpenError::NumberTooFar {
            last,
            got: p.number,
        });
    }
    let eph = PublicKey::from_slice(&p.one_time)
        .map_err(|_| OpenError::Frame(DecodeError::InvalidKey))?;
    let mut shared = Secret32(reader.shared_x(&eph).ok_or(OpenError::Decrypt)?);
    let key = message_key(
        &shared.0,
        &p.one_time,
        &p.reader.to_bytes(),
        &p.sender.to_bytes(),
    );
    shared.0 = [0; 32];
    let plain = cipher(&key)
        .decrypt(
            &Nonce::from([0u8; 12]),
            Payload {
                msg: p.ciphertext,
                aad: p.prefix,
            },
        )
        .map_err(|_| OpenError::Decrypt)?;
    let content = unpad(&plain)?;
    Ok(Opened {
        sender: p.sender,
        number: p.number,
        kind,
        content,
    })
}
