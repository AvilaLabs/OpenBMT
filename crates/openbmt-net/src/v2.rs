//! The encrypted transport of BIP324, as bytes in and bytes out (NR-80 to
//! NR-85), and the [`Transport`] that lets a session speak either it or the
//! old one.
//!
//! The key exchange and the packet ciphers are the `bip324` crate's; this
//! module writes none of them. It is the initiator's side only, and it has
//! no socket, no clock and no randomness: the caller passes the ephemeral
//! secret and the auxiliary randomness in a [`V2Entropy`].
//!
//! The initiator sends its 64-byte ElligatorSwift key and no garbage, and
//! on the peer's key sends the garbage terminator and the version packet.
//! A message handed to [`Transport::send`] before the peer's version packet
//! is authenticated waits and goes out right after it: the cipher does not
//! exist earlier. From the peer it accepts up to 4,095 bytes of garbage, up
//! to 8 decoy packets before the version packet, and decoy packets after
//! it.
//!
//! The `bip324` handshake type borrows the peer's garbage for as long as
//! the first packet has not been read, so it cannot be kept between two
//! calls. While the first packets are incomplete, the handshake is
//! rebuilt from the same entropy and the peer's key when enough bytes have
//! arrived for another try. That costs one more key exchange for each
//! packet of the handshake, and the number of those is limited.

use std::sync::OnceLock;

use bip324::{
    CipherSession, FillBytes, GarbageResult, Handshake, Initialized, OutboundCipher, PacketType,
    ReceivedGarbage, ReceivedKey, Role, SentVersion, VersionResult, NUM_LENGTH_BYTES,
};
use bitcoin::consensus::encode::serialize;
use bitcoin::p2p::message::NetworkMessage;
use bitcoin::p2p::Magic;
use bitcoin::secp256k1::{Secp256k1, VerifyOnly};

use crate::frame::{command_name, encode, Command, Frame};
use crate::{
    Drop, FrameReader, MAX_BLOCK_PAYLOAD, MAX_OUTGOING_BYTES, MAX_UNKNOWN_PAYLOAD, PUSH_SLACK,
};

/// Bytes of an ElligatorSwift key.
pub const KEY_LEN: usize = 64;
/// NR-83: most garbage bytes the peer may send before its terminator.
pub const MAX_GARBAGE: usize = 4_095;
/// NR-83: most decoy packets before the peer's version packet.
pub const MAX_HANDSHAKE_DECOYS: usize = 8;
/// NR-83: most bytes of contents in a packet of the handshake.
pub const MAX_HANDSHAKE_CONTENTS: usize = 4_096;
/// NR-83: most bytes of contents in a packet after the handshake: the
/// largest message payload of NR-02 and the 13 bytes that name the
/// command.
pub const MAX_CONTENTS: usize = MAX_BLOCK_PAYLOAD as usize + 13;
/// NR-83: most bytes of decoy packets, in all, after the handshake
/// **[to tune]**.
pub const MAX_DECOY_BYTES: usize = 1_000_000;
/// Bytes of a packet that are not contents: the header byte and the tag.
const OVERHEAD: usize = 17;
/// The bit of the header byte that marks a decoy.
const IGNORE_BIT: u8 = 0x80;
/// Messages that may wait for the cipher.
const MAX_PENDING: usize = 4;
/// The 12 bytes after the network magic of an old-transport `version`.
const V1_VERSION: &[u8; 12] = b"version\0\0\0\0\0";

/// The commands that have a one-byte ID in BIP324; the ID is the position.
/// Position 0 means "12 bytes of command follow".
const SHORT_IDS: [&str; 29] = [
    "",
    "addr",
    "block",
    "blocktxn",
    "cmpctblock",
    "feefilter",
    "filteradd",
    "filterclear",
    "filterload",
    "getblocks",
    "getblocktxn",
    "getdata",
    "getheaders",
    "headers",
    "inv",
    "mempool",
    "merkleblock",
    "notfound",
    "ping",
    "pong",
    "sendcmpct",
    "tx",
    "getcfilters",
    "cfilter",
    "getcfheaders",
    "cfheaders",
    "getcfcheckpt",
    "cfcheckpt",
    "addrv2",
];

/// The command a one-byte ID stands for, if there is one.
pub(crate) fn short_name(id: u8) -> Option<&'static str> {
    SHORT_IDS
        .get(usize::from(id))
        .copied()
        .filter(|n| !n.is_empty())
}

/// The one-byte ID of a command, if it has one.
pub(crate) fn short_id(name: &str) -> Option<u8> {
    let at = SHORT_IDS.iter().skip(1).position(|n| *n == name)?;
    u8::try_from(at.checked_add(1)?).ok()
}

/// The randomness for one encrypted connection (NR-80): the ephemeral
/// secret key and the 32 bytes that randomize the encoding of its public
/// key. The caller draws both from a source it trusts. They are overwritten
/// when the value is dropped, and they are not printed.
pub struct V2Entropy {
    secret: [u8; 32],
    aux: [u8; 32],
}

impl V2Entropy {
    /// Entropy drawn as 8 numbers: the secret first, then the auxiliary
    /// randomness, each as 4 numbers in little-endian bytes.
    pub fn from_words(mut next: impl FnMut() -> u64) -> Self {
        let mut entropy = Self {
            secret: [0; 32],
            aux: [0; 32],
        };
        for chunk in entropy
            .secret
            .chunks_mut(8)
            .chain(entropy.aux.chunks_mut(8))
        {
            for (slot, byte) in chunk.iter_mut().zip(next().to_le_bytes()) {
                *slot = byte;
            }
        }
        entropy
    }

    /// Entropy from these bytes. A secret of all zeros, or at or above the
    /// order of the curve, is refused when the connection starts.
    pub fn new(secret: [u8; 32], aux: [u8; 32]) -> Self {
        Self { secret, aux }
    }
}

impl std::fmt::Debug for V2Entropy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "V2Entropy(..)")
    }
}

impl std::ops::Drop for V2Entropy {
    fn drop(&mut self) {
        wipe(&mut self.secret);
        wipe(&mut self.aux);
    }
}

/// Overwrite bytes with zeros, and keep the compiler from seeing that the
/// write is not read.
pub(crate) fn wipe(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        *b = 0;
    }
    std::hint::black_box(&*bytes);
}

/// The two values the key exchange asks its random source for, in order.
struct Feed<'a> {
    entropy: &'a V2Entropy,
    drawn: u8,
}

impl FillBytes for Feed<'_> {
    fn fill_bytes(&mut self, dest: &mut [u8; 32]) {
        *dest = if self.drawn == 0 {
            self.entropy.secret
        } else {
            self.entropy.aux
        };
        self.drawn = self.drawn.saturating_add(1);
    }
}

fn context() -> &'static Secp256k1<VerifyOnly> {
    static CONTEXT: OnceLock<Secp256k1<VerifyOnly>> = OnceLock::new();
    CONTEXT.get_or_init(Secp256k1::verification_only)
}

fn initialized(magic: [u8; 4], entropy: &V2Entropy) -> Result<Handshake<Initialized>, Drop> {
    // The crate asserts that the secret is not zero; that is a refusal here.
    if entropy.secret.iter().all(|b| *b == 0) {
        return Err(Drop::V2Crypto);
    }
    let mut feed = Feed { entropy, drawn: 0 };
    Handshake::new_with_rng(magic, Role::Initiator, &mut feed, context())
        .map_err(|_| Drop::V2Crypto)
}

/// Our 64-byte key and no garbage.
fn our_key(magic: [u8; 4], entropy: &V2Entropy) -> Result<[u8; KEY_LEN], Drop> {
    let mut out = [0u8; KEY_LEN];
    initialized(magic, entropy)?
        .send_key(None, &mut out)
        .map_err(|_| Drop::V2Crypto)?;
    Ok(out)
}

/// The handshake after this side's version packet, and the bytes of this
/// side's garbage terminator and version packet. The same entropy and the same peer key
/// always give the same result.
fn sent_version(
    magic: [u8; 4],
    entropy: &V2Entropy,
    peer_key: [u8; KEY_LEN],
) -> Result<(Handshake<SentVersion>, Vec<u8>), Drop> {
    let mut key = [0u8; KEY_LEN];
    let received = initialized(magic, entropy)?
        .send_key(None, &mut key)
        .map_err(|_| Drop::V2Crypto)?
        .receive_key(peer_key)
        .map_err(|_| Drop::V2Crypto)?;
    let mut out = vec![0u8; Handshake::<ReceivedKey<'_>>::send_version_len(None)];
    let sent = received
        .send_version(&mut out, None)
        .map_err(|_| Drop::V2Crypto)?;
    Ok((sent, out))
}

/// NR-81: whether these bytes start an old-transport `version` message.
fn is_v1_version(magic: &[u8; 4], bytes: &[u8]) -> bool {
    bytes.get(..4) == Some(magic.as_slice()) && bytes.get(4..16) == Some(V1_VERSION.as_slice())
}

fn too_long(contents: usize, limit: usize) -> Drop {
    Drop::V2TooLong {
        len: u32::try_from(contents).unwrap_or(u32::MAX),
        limit: u32::try_from(limit).unwrap_or(u32::MAX),
    }
}

/// What a read of the peer's handshake packets came to.
enum Attempt {
    /// The version packet is authenticated: the session, and the number of
    /// bytes of the buffer used.
    Done(CipherSession, usize),
    /// The buffer must hold at least this many bytes before another try.
    Need(usize),
}

/// Read the peer's packets from `consumed` on: decoys, then the version
/// packet. `saw` is set once a packet has authenticated.
fn attempt(
    mut handshake: Handshake<ReceivedGarbage<'_>>,
    buf: &[u8],
    consumed: usize,
    saw: &mut bool,
) -> Result<Attempt, Drop> {
    let mut at = consumed;
    let mut decoys = 0usize;
    loop {
        let head_end = at.saturating_add(NUM_LENGTH_BYTES);
        let Some(head) = buf
            .get(at..)
            .and_then(|b| b.first_chunk::<NUM_LENGTH_BYTES>())
            .copied()
        else {
            return Ok(Attempt::Need(head_end));
        };
        let rest = handshake
            .decrypt_packet_len(head)
            .map_err(|_| Drop::V2Auth)?;
        let contents = rest.saturating_sub(OVERHEAD);
        if contents > MAX_HANDSHAKE_CONTENTS {
            return Err(too_long(contents, MAX_HANDSHAKE_CONTENTS));
        }
        let end = head_end.saturating_add(rest);
        let Some(body) = buf.get(head_end..end) else {
            return Ok(Attempt::Need(end));
        };
        let mut packet = body.to_vec();
        match handshake.receive_version(&mut packet) {
            Err(_) => return Err(Drop::V2Auth),
            Ok(VersionResult::Complete { cipher }) => {
                *saw = true;
                return Ok(Attempt::Done(cipher, end));
            }
            Ok(VersionResult::Decoy(next)) => {
                *saw = true;
                decoys = decoys.saturating_add(1);
                if decoys > MAX_HANDSHAKE_DECOYS {
                    return Err(Drop::V2Decoys);
                }
                handshake = next;
                at = end;
            }
        }
    }
}

/// The handshake while the peer's garbage and first packets are read.
struct Garbage {
    entropy: V2Entropy,
    peer_key: [u8; KEY_LEN],
    /// The handshake after this side's version packet; `None` while it has been
    /// used up by a read that stopped inside a packet.
    sent: Option<Handshake<SentVersion>>,
    /// Everything after the peer's key.
    buf: Vec<u8>,
    /// The buffer length at which another try is made.
    resume_at: usize,
}

/// One try at the peer's garbage and handshake packets.
fn step(
    g: &mut Garbage,
    magic: [u8; 4],
    saw: &mut bool,
) -> Result<Option<(CipherSession, usize)>, Drop> {
    if g.buf.len() < g.resume_at {
        return Ok(None);
    }
    let handshake = match g.sent.take() {
        Some(h) => h,
        None => sent_version(magic, &g.entropy, g.peer_key)?.0,
    };
    match handshake.receive_garbage(&g.buf) {
        Err(bip324::Error::NoGarbageTerminator) => Err(Drop::V2Garbage),
        Err(_) => Err(Drop::V2Crypto),
        Ok(GarbageResult::NeedMoreData(h)) => {
            g.sent = Some(h);
            g.resume_at = g.buf.len().saturating_add(1);
            Ok(None)
        }
        Ok(GarbageResult::FoundGarbage {
            handshake,
            consumed_bytes,
        }) => match attempt(handshake, &g.buf, consumed_bytes, saw)? {
            Attempt::Done(cipher, used) => Ok(Some((cipher, used))),
            Attempt::Need(n) => {
                g.resume_at = n;
                Ok(None)
            }
        },
    }
}

/// Packets after the handshake.
struct Open {
    cipher: CipherSession,
    buf: Vec<u8>,
    /// Bytes at the front of `buf` that have been handed out.
    start: usize,
    /// The length of the rest of the packet at `start`, once its 3 length
    /// bytes have been decrypted (which can be done once only).
    want: Option<usize>,
    decoy_bytes: usize,
}

/// A packet read.
enum Parsed {
    Decoy,
    Skipped,
    Frame(Frame),
}

/// The message in a packet's contents, as a frame (NR-02, NR-03, NR-13).
fn parse_contents(contents: &[u8], handshake_done: bool) -> Result<Parsed, Drop> {
    let malformed = Drop::Malformed {
        command: "v2 packet",
    };
    let (first, rest) = contents.split_first().ok_or(malformed)?;
    let (name, payload) = if *first == 0 {
        let (field, payload) = rest.split_first_chunk::<12>().ok_or(Drop::Malformed {
            command: "v2 packet",
        })?;
        (Some(command_name(field)?), payload)
    } else {
        (short_name(*first), rest)
    };
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    match name.and_then(Command::from_name) {
        Some(command) => {
            if !handshake_done && !command.belongs_to_handshake() {
                return Err(Drop::BeforeHandshake {
                    command: command.name(),
                });
            }
            if len > command.limit() {
                return Err(Drop::TooLong {
                    command: command.name(),
                    len,
                    limit: command.limit(),
                });
            }
            Ok(Parsed::Frame(Frame {
                command,
                payload: payload.to_vec(),
            }))
        }
        None if len > MAX_UNKNOWN_PAYLOAD => Err(Drop::UnknownTooLong { len }),
        None => Ok(Parsed::Skipped),
    }
}

impl Open {
    /// NR-83: the next message, or `None` when more bytes are needed.
    /// Decoys and commands the counter does not read are consumed.
    fn next_frame(&mut self, handshake_done: bool) -> Result<Option<Frame>, Drop> {
        loop {
            let rest = match self.want {
                Some(rest) => rest,
                None => {
                    let Some(head) = self
                        .buf
                        .get(self.start..)
                        .and_then(|b| b.first_chunk::<NUM_LENGTH_BYTES>())
                        .copied()
                    else {
                        return Ok(None);
                    };
                    let rest = self.cipher.inbound().decrypt_packet_len(head);
                    let contents = rest.saturating_sub(OVERHEAD);
                    if contents > MAX_CONTENTS {
                        return Err(too_long(contents, MAX_CONTENTS));
                    }
                    self.want = Some(rest);
                    rest
                }
            };
            let from = self.start.saturating_add(NUM_LENGTH_BYTES);
            let to = from.saturating_add(rest);
            let Some(body) = self.buf.get_mut(from..to) else {
                return Ok(None);
            };
            let (_, plain) = self
                .cipher
                .inbound()
                .decrypt_in_place(body, None)
                .map_err(|_| Drop::V2Auth)?;
            let parsed = match plain.split_first() {
                Some((header, _)) if header & IGNORE_BIT != 0 => Parsed::Decoy,
                Some((_, contents)) => parse_contents(contents, handshake_done)?,
                None => return Err(Drop::V2Auth),
            };
            self.start = to;
            self.want = None;
            if self.start >= self.buf.len() {
                self.start = 0;
                if self.buf.capacity() > PUSH_SLACK {
                    self.buf = Vec::new();
                } else {
                    self.buf.clear();
                }
            }
            match parsed {
                Parsed::Frame(frame) => return Ok(Some(frame)),
                Parsed::Skipped => {}
                Parsed::Decoy => {
                    let size = rest.saturating_add(NUM_LENGTH_BYTES);
                    self.decoy_bytes = self.decoy_bytes.saturating_add(size);
                    if self.decoy_bytes > MAX_DECOY_BYTES {
                        return Err(Drop::V2Decoys);
                    }
                }
            }
        }
    }
}

enum Phase {
    /// Our key is out; the peer's 64 bytes are awaited.
    Key {
        entropy: V2Entropy,
        buf: Vec<u8>,
    },
    Garbage(Garbage),
    Open(Open),
    Failed,
}

/// One connection's encrypted transport, initiator side.
pub struct V2 {
    magic: [u8; 4],
    phase: Phase,
    /// Contents of messages that wait for the cipher.
    pending: Vec<Vec<u8>>,
    /// An encrypted packet of the peer has authenticated (NR-81).
    proven: bool,
    session_id: Option<[u8; 32]>,
    handshake_done: bool,
}

impl std::fmt::Debug for V2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let phase = match self.phase {
            Phase::Key { .. } => "key",
            Phase::Garbage(_) => "garbage",
            Phase::Open(_) => "open",
            Phase::Failed => "failed",
        };
        f.debug_struct("V2")
            .field("phase", &phase)
            .field("proven", &self.proven)
            .finish_non_exhaustive()
    }
}

/// The contents of a packet for a message: the one-byte ID, or a zero and
/// the 12-byte command, and the payload.
pub(crate) fn contents_of(message: &NetworkMessage) -> Vec<u8> {
    let name = message.cmd();
    let payload = serialize(message);
    let mut out = Vec::with_capacity(payload.len().saturating_add(13));
    match short_id(name) {
        Some(id) => out.push(id),
        None => {
            out.push(0);
            let mut field = [0u8; 12];
            for (slot, byte) in field.iter_mut().zip(name.bytes()) {
                *slot = byte;
            }
            out.extend_from_slice(&field);
        }
    }
    out.extend_from_slice(&payload);
    out
}

/// Encrypt contents as one genuine packet and append it.
fn seal(cipher: &mut CipherSession, contents: &[u8], out: &mut Vec<u8>) -> Result<(), Drop> {
    let mut packet = vec![0u8; OutboundCipher::encryption_buffer_len(contents.len())];
    cipher
        .outbound()
        .encrypt(contents, &mut packet, PacketType::Genuine, None)
        .map_err(|_| Drop::V2Crypto)?;
    out.extend_from_slice(&packet);
    Ok(())
}

impl V2 {
    fn new(magic: Magic, entropy: V2Entropy, out: &mut Vec<u8>) -> Result<Self, Drop> {
        let magic = magic.to_bytes();
        out.extend_from_slice(&our_key(magic, &entropy)?);
        Ok(Self {
            magic,
            phase: Phase::Key {
                entropy,
                buf: Vec::new(),
            },
            pending: Vec::new(),
            proven: false,
            session_id: None,
            handshake_done: false,
        })
    }

    fn send(&mut self, message: &NetworkMessage, out: &mut Vec<u8>) -> Result<(), Drop> {
        let contents = contents_of(message);
        if contents.len() > MAX_CONTENTS {
            return Err(Drop::SendBacklog);
        }
        match &mut self.phase {
            Phase::Open(open) => seal(&mut open.cipher, &contents, out),
            Phase::Failed => Err(Drop::V2Crypto),
            Phase::Key { .. } | Phase::Garbage(_) => {
                let held: usize = self.pending.iter().map(Vec::len).sum();
                if self.pending.len() >= MAX_PENDING
                    || held.saturating_add(contents.len()) > MAX_OUTGOING_BYTES
                {
                    return Err(Drop::SendBacklog);
                }
                self.pending.push(contents);
                Ok(())
            }
        }
    }

    fn push(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> Result<(), Drop> {
        match std::mem::replace(&mut self.phase, Phase::Failed) {
            Phase::Key { entropy, mut buf } => {
                buf.extend_from_slice(bytes);
                if is_v1_version(&self.magic, &buf) {
                    return Err(Drop::V2NotSpoken);
                }
                let Some(key) = buf.first_chunk::<KEY_LEN>().copied() else {
                    self.phase = Phase::Key { entropy, buf };
                    return Ok(());
                };
                let (sent, ours) = sent_version(self.magic, &entropy, key)?;
                out.extend_from_slice(&ours);
                let rest = buf.get(KEY_LEN..).unwrap_or_default().to_vec();
                let garbage = Garbage {
                    entropy,
                    peer_key: key,
                    sent: Some(sent),
                    buf: rest,
                    resume_at: 0,
                };
                self.drive(garbage, out)
            }
            Phase::Garbage(mut garbage) => {
                garbage.buf.extend_from_slice(bytes);
                self.drive(garbage, out)
            }
            Phase::Open(mut open) => {
                open.buf.drain(..open.start.min(open.buf.len()));
                open.start = 0;
                let cap = NUM_LENGTH_BYTES
                    .saturating_add(open.want.unwrap_or(0))
                    .saturating_add(PUSH_SLACK);
                if open.buf.len().saturating_add(bytes.len()) > cap {
                    return Err(Drop::ReceiveBacklog);
                }
                open.buf.extend_from_slice(bytes);
                self.phase = Phase::Open(open);
                Ok(())
            }
            Phase::Failed => Ok(()),
        }
    }

    /// Try the garbage and the handshake packets; on success open the
    /// session and send what waited.
    fn drive(&mut self, mut garbage: Garbage, out: &mut Vec<u8>) -> Result<(), Drop> {
        match step(&mut garbage, self.magic, &mut self.proven)? {
            None => {
                self.phase = Phase::Garbage(garbage);
                Ok(())
            }
            Some((mut cipher, used)) => {
                self.session_id = Some(cipher.id().to_bytes());
                self.proven = true;
                for contents in std::mem::take(&mut self.pending) {
                    seal(&mut cipher, &contents, out)?;
                }
                let rest = garbage.buf.split_off(used.min(garbage.buf.len()));
                self.phase = Phase::Open(Open {
                    cipher,
                    buf: rest,
                    start: 0,
                    want: None,
                    decoy_bytes: 0,
                });
                Ok(())
            }
        }
    }

    fn next(&mut self) -> Result<Option<Frame>, Drop> {
        let handshake_done = self.handshake_done;
        match &mut self.phase {
            Phase::Open(open) => open.next_frame(handshake_done),
            _ => Ok(None),
        }
    }
}

/// How a session reads and writes: the old transport or the encrypted one
/// (NR-80). A session hands a message to it and gets bytes, and hands it
/// bytes and gets messages.
#[derive(Debug)]
pub enum Transport {
    /// The old transport (NR-01 to NR-05).
    V1 {
        /// The network's magic.
        magic: Magic,
        /// The reader that cuts the stream into messages.
        reader: FrameReader,
    },
    /// The encrypted transport of BIP324 (NR-80 to NR-85).
    V2(Box<V2>),
}

impl Transport {
    /// The old transport.
    pub fn v1(magic: Magic) -> Self {
        Self::V1 {
            magic,
            reader: FrameReader::new(magic),
        }
    }

    /// The encrypted transport. Our key is appended to `out`. Fails when
    /// the entropy gives no key.
    pub fn v2(magic: Magic, entropy: V2Entropy, out: &mut Vec<u8>) -> Result<Self, Drop> {
        Ok(Self::V2(Box::new(V2::new(magic, entropy, out)?)))
    }

    /// Whether this is the encrypted transport.
    pub fn is_v2(&self) -> bool {
        matches!(self, Self::V2(_))
    }

    /// NR-84: the session ID, once the peer's version packet has
    /// authenticated.
    pub fn v2_session_id(&self) -> Option<[u8; 32]> {
        match self {
            Self::V2(v2) => v2.session_id,
            Self::V1 { .. } => None,
        }
    }

    /// NR-81: true while the encrypted transport is in use and no packet of
    /// the peer has authenticated yet. A close then is a peer that does not
    /// speak it.
    pub fn v2_unproven(&self) -> bool {
        matches!(self, Self::V2(v2) if !v2.proven)
    }

    /// Write a message: append its bytes to `out`, or hold it until the
    /// encrypted handshake is through.
    pub fn send(&mut self, message: &NetworkMessage, out: &mut Vec<u8>) -> Result<(), Drop> {
        match self {
            Self::V1 { magic, .. } => {
                out.extend_from_slice(&encode(*magic, message));
                Ok(())
            }
            Self::V2(v2) => v2.send(message, out),
        }
    }

    /// Add bytes received (in reads of at most [`PUSH_SLACK`]). Bytes the
    /// handshake answers with are appended to `out`.
    pub fn push(&mut self, bytes: &[u8], out: &mut Vec<u8>) -> Result<(), Drop> {
        match self {
            Self::V1 { reader, .. } => reader.push(bytes),
            Self::V2(v2) => v2.push(bytes, out),
        }
    }

    /// The next message of a command the counter reads, or `None` when more
    /// bytes are needed.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, Drop> {
        match self {
            Self::V1 { reader, .. } => reader.next(),
            Self::V2(v2) => v2.next(),
        }
    }

    /// NR-13: from now on the commands that belong after the handshake are
    /// read.
    pub fn set_handshake_done(&mut self) {
        match self {
            Self::V1 { reader, .. } => reader.set_handshake_done(),
            Self::V2(v2) => v2.handshake_done = true,
        }
    }
}
