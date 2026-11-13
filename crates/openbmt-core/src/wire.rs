//! Strict wire encodings.
//!
//! Every message that crosses a device boundary or is written to storage is
//! framed the same way:
//!
//! ```text
//! magic (4 ASCII bytes) | version u8 | payload
//! ```
//!
//! The magic names the type, so bytes meant for one type are never read as
//! another. The version is currently 1; any other value is refused. The
//! magics are `OBMB` (signed batch), `OBMR` (record), `OBMS` (signed
//! settings), `OBMH` (high-water mark), `OBMC` (counter batches), `OBME`
//! (exposure), `OBMP` (pause tracker) and `OBMK` (the Key app's signer
//! state, written by the `openbmt-signer` crate), `OBMO` (pair offer), `OBMJ`
//! (pair request), `OBMD` (signed pairing), `OBMs` (counter bundle), `OBMX` (owner command), `OBMV` (a
//! sale watch, written by the `openbmt-watch` crate), `OBMG` (the counter's
//! own state, "desk") and `OBML` (a stored sale), the last two written by the
//! `openbmt-counter` crate, and `OBMT` (the owner's signed watcher
//! heartbeat). An own-wallet pairing has `OBM-` (offer), `OBM.` (request),
//! `OBM/` (bundle), `OBM:` (the counter's record) and `OBM;` (the range
//! book), beside `OBM)` and `OBM*` of `pinned_wallet`, and `OBM<` carries signed settings
//! with a counter's confirmation (`confirmed`). The address book (`OBMA`) and the header store (`OBMW`) have
//! their own magics in the crates that write them.
//!
//! [`Reader`], [`start`], [`put_opt_u64`] and [`len_u32`] are public so that
//! other crates in the workspace write their snapshots the same way.
//!
//! Decoding is strict. A message decodes only if it is exactly what
//! `to_bytes` would have produced for the value it describes: no trailing
//! bytes, no alternative spellings (flags are 0 or 1, lists are in ascending
//! order without repeats, lengths are the canonical ones), and every length
//! is checked against its limit and against the bytes that are actually
//! present before anything is allocated. For all bytes `b` that decode,
//! `to_bytes(from_bytes(b)) == b`.
//!
//! Decoding only parses. It does not verify signatures and does not run
//! `validate` or `verify`; callers must still do that before trusting a
//! decoded value.

use std::fmt;

use crate::identity::{DeviceId, Sig};

/// The only wire version this crate reads and writes.
pub const VERSION: u8 = 1;

/// Magic of a [`SignedBatch`](crate::batch::SignedBatch).
pub const MAGIC_SIGNED_BATCH: [u8; 4] = *b"OBMB";
/// Magic of a [`Record`](crate::records::Record).
pub const MAGIC_RECORD: [u8; 4] = *b"OBMR";
/// Magic of a [`SignedSettings`](crate::settings::SignedSettings).
pub const MAGIC_SIGNED_SETTINGS: [u8; 4] = *b"OBMS";
/// Magic of a [`HighWater`](crate::batch::HighWater) snapshot.
pub const MAGIC_HIGH_WATER: [u8; 4] = *b"OBMH";
/// Magic of a [`CounterBatches`](crate::batch::CounterBatches) snapshot.
pub const MAGIC_COUNTER_BATCHES: [u8; 4] = *b"OBMC";
/// Magic of an [`Exposure`](crate::policy::Exposure) snapshot.
pub const MAGIC_EXPOSURE: [u8; 4] = *b"OBME";
/// Magic of a [`PauseTracker`](crate::policy::PauseTracker) snapshot.
pub const MAGIC_PAUSE_TRACKER: [u8; 4] = *b"OBMP";
/// Magic of the Key app's signer state snapshot (the `openbmt-signer` crate).
pub const MAGIC_SIGNER_STATE: [u8; 4] = *b"OBMK";

/// Magic of a [`PairOffer`](crate::pairing::PairOffer).
pub const MAGIC_PAIR_OFFER: [u8; 4] = *b"OBMO";
/// Magic of a [`PairRequest`](crate::pairing::PairRequest).
pub const MAGIC_PAIR_REQUEST: [u8; 4] = *b"OBMJ";
/// Magic of a [`SignedPairing`](crate::pairing::SignedPairing).
pub const MAGIC_SIGNED_PAIRING: [u8; 4] = *b"OBMD";
/// Magic of a [`CounterBundle`](crate::bundle::CounterBundle).
pub const MAGIC_COUNTER_BUNDLE: [u8; 4] = *b"OBMs";
/// Magic of a [`SignedCommand`](crate::command::SignedCommand).
pub const MAGIC_OWNER_COMMAND: [u8; 4] = *b"OBMX";
/// Magic of a [`SignedHeartbeat`](crate::heartbeat::SignedHeartbeat).
pub const MAGIC_HEARTBEAT: [u8; 4] = *b"OBMT";
/// Magic of a sale watch snapshot (the `openbmt-watch` crate).
pub const MAGIC_SALE_WATCH: [u8; 4] = *b"OBMV";
/// Magic of the counter's own state: address times, the cancel alarm, the
/// last command number and override totals (the `openbmt-counter` crate).
pub const MAGIC_COUNTER_DESK: [u8; 4] = *b"OBMG";
/// Magic of a stored sale (the `openbmt-counter` crate).
pub const MAGIC_COUNTER_SALE: [u8; 4] = *b"OBML";
/// Magic of a Box's claim (BX-12).
pub const MAGIC_BOX_CLAIM: [u8; 4] = *b"OBM5";
/// Magic of the welcome a Box takes (BX-15).
pub const MAGIC_BOX_WELCOME: [u8; 4] = *b"OBMM";
/// Magic of an [`OwnOffer`](crate::pairing_own::OwnOffer) (CR-227).
pub const MAGIC_OWN_OFFER: [u8; 4] = *b"OBM-";
/// Magic of an [`OwnRequest`](crate::pairing_own::OwnRequest) (CR-229).
pub const MAGIC_OWN_REQUEST: [u8; 4] = *b"OBM.";
/// Magic of an [`OwnBundle`](crate::pairing_own::OwnBundle) (CR-230).
pub const MAGIC_OWN_BUNDLE: [u8; 4] = *b"OBM/";
/// Magic of the counter's record of an own-wallet pairing (CR-233, written
/// by the `openbmt-counter-node` crate).
pub const MAGIC_OWN_PAIRING_RECORD: [u8; 4] = *b"OBM:";
/// Magic of a counter's range book slot (CR-234).
pub const MAGIC_RANGE_BOOK: [u8; 4] = *b"OBM;";
/// Magic of signed settings that carry a counter's confirmation (CR-242).
pub const MAGIC_CONFIRMED_SETTINGS: [u8; 4] = *b"OBM<";
/// Magic of a [`SwitchOwn`](crate::pairing_own::SwitchOwn), the owner's
/// message that moves a counter from the starter wallet (CR-257).
pub const MAGIC_SWITCH_OWN: [u8; 4] = *b"OBM]";
/// Magic of a [`SignedRelayHint`](crate::relay::SignedRelayHint), the relay
/// for counters (CR-132).
pub const MAGIC_RELAY_FOR_COUNTERS: [u8; 4] = *b"OBMz";

/// Why bytes were not accepted as a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The input ends before the message does.
    Truncated,
    /// Bytes remain after the message.
    TrailingBytes,
    /// The first four bytes are not this type's magic.
    BadMagic,
    /// The version byte is not one this crate reads.
    UnsupportedVersion(u8),
    /// A length or count is above its limit.
    TooLarge,
    /// The bytes are not what `to_bytes` would produce; the text says which
    /// rule failed.
    NonCanonical(&'static str),
    /// A public key is not a point on the curve.
    InvalidKey,
    /// A record kind byte is not a known kind.
    UnknownKind(u8),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => write!(f, "message is cut short"),
            Self::TrailingBytes => write!(f, "message has bytes after its end"),
            Self::BadMagic => write!(f, "message is not of this type"),
            Self::UnsupportedVersion(v) => write!(f, "message version {v} is not supported"),
            Self::TooLarge => write!(f, "message length is above its limit"),
            Self::NonCanonical(why) => write!(f, "message is not in canonical form: {why}"),
            Self::InvalidKey => write!(f, "message holds an invalid public key"),
            Self::UnknownKind(k) => write!(f, "unknown record kind {k}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Start a message: magic and version.
pub fn start(magic: &[u8; 4], capacity: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(capacity + 5);
    out.extend_from_slice(magic);
    out.push(VERSION);
    out
}

/// Write a flag byte, then the u64 only when there is one.
pub fn put_opt_u64(out: &mut Vec<u8>, v: Option<u64>) {
    match v {
        Some(v) => {
            out.push(1);
            out.extend_from_slice(&v.to_le_bytes());
        }
        None => out.push(0),
    }
}

/// A list length as the u32 the wire form carries. Callers hold lists far
/// below that bound, so a longer one is a programming error.
pub fn len_u32(len: usize) -> u32 {
    u32::try_from(len).expect("list length fits the u32 wire field")
}

/// Cursor over the bytes being decoded.
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    /// Start reading `buf` as it is, with no magic or version. For a record
    /// body, which sits inside a framed record.
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    /// Start reading `buf` and check its magic and version.
    pub fn framed(buf: &'a [u8], magic: &[u8; 4]) -> Result<Self, DecodeError> {
        let mut r = Self { buf };
        if r.take(4)? != magic {
            return Err(DecodeError::BadMagic);
        }
        match r.u8()? {
            VERSION => Ok(r),
            v => Err(DecodeError::UnsupportedVersion(v)),
        }
    }

    /// Start reading `buf` and check its magic and that its version is one
    /// of `1..=latest`. Returns the reader and the version, for a frame that
    /// has grown: the older versions still read.
    pub fn framed_up_to(
        buf: &'a [u8],
        magic: &[u8; 4],
        latest: u8,
    ) -> Result<(Self, u8), DecodeError> {
        let mut r = Self { buf };
        if r.take(4)? != magic {
            return Err(DecodeError::BadMagic);
        }
        match r.u8()? {
            v @ 1.. if v <= latest => Ok((r, v)),
            v => Err(DecodeError::UnsupportedVersion(v)),
        }
    }

    /// The next `n` bytes, or `Truncated` if fewer remain.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if n > self.buf.len() {
            return Err(DecodeError::Truncated);
        }
        let (head, tail) = self.buf.split_at(n);
        self.buf = tail;
        Ok(head)
    }

    /// One byte.
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.array::<1>()?[0])
    }

    /// A little-endian u16.
    pub fn u16_le(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// A little-endian u32.
    pub fn u32_le(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// A little-endian u64.
    pub fn u64_le(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// The next `N` bytes as an array.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        <[u8; N]>::try_from(self.take(N)?).map_err(|_| DecodeError::Truncated)
    }

    /// A byte that must be 0 or 1.
    pub fn flag(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::NonCanonical("flag is not 0 or 1")),
        }
    }

    /// A flag byte, then a u64 only when the flag is 1.
    pub fn opt_u64(&mut self) -> Result<Option<u64>, DecodeError> {
        if self.flag()? {
            Ok(Some(self.u64_le()?))
        } else {
            Ok(None)
        }
    }

    /// A valid x-only public key.
    pub fn device_id(&mut self) -> Result<DeviceId, DecodeError> {
        DeviceId::from_bytes(&self.array()?).map_err(|_| DecodeError::InvalidKey)
    }

    /// A 64-byte signature. The bytes are not checked.
    pub fn sig(&mut self) -> Result<Sig, DecodeError> {
        Ok(Sig(self.array()?))
    }

    /// A u32 element count for a list whose elements take at least
    /// `min_elem` bytes each. A count above `max` is `TooLarge`; a count that
    /// cannot fit in the bytes left is `Truncated`. Either way it is refused
    /// before the caller reserves anything.
    pub fn count(&mut self, max: usize, min_elem: usize) -> Result<usize, DecodeError> {
        let n = usize::try_from(self.u32_le()?).map_err(|_| DecodeError::TooLarge)?;
        if n > max {
            return Err(DecodeError::TooLarge);
        }
        match n.checked_mul(min_elem) {
            Some(need) if need <= self.buf.len() => Ok(n),
            _ => Err(DecodeError::Truncated),
        }
    }

    /// Whether every byte was read.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Succeed only if every byte was read.
    pub fn finish(self) -> Result<(), DecodeError> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_reads_in_order_and_reports_short_input() {
        let bytes = [1u8, 2, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 9];
        let mut r = Reader { buf: &bytes };
        assert_eq!(r.u8(), Ok(1));
        assert_eq!(r.u16_le(), Ok(2));
        assert_eq!(r.u32_le(), Ok(3));
        assert_eq!(r.u64_le(), Ok(4));
        assert_eq!(r.u16_le(), Err(DecodeError::Truncated));
        assert_eq!(r.finish(), Err(DecodeError::TrailingBytes));
        assert_eq!(Reader { buf: &[] }.finish(), Ok(()));
    }

    #[test]
    fn framing_checks_magic_then_version() {
        let m = *b"TEST";
        assert!(Reader::framed(b"TEST\x01", &m).is_ok());
        assert_eq!(
            Reader::framed(b"TESX\x01", &m).err(),
            Some(DecodeError::BadMagic)
        );
        assert_eq!(
            Reader::framed(b"TEST\x00", &m).err(),
            Some(DecodeError::UnsupportedVersion(0))
        );
        assert_eq!(
            Reader::framed(b"TEST\x02", &m).err(),
            Some(DecodeError::UnsupportedVersion(2))
        );
        assert_eq!(
            Reader::framed(b"TES", &m).err(),
            Some(DecodeError::Truncated)
        );
        assert_eq!(
            Reader::framed(b"TEST", &m).err(),
            Some(DecodeError::Truncated)
        );
    }

    #[test]
    fn count_checks_limit_before_length() {
        let mut r = Reader {
            buf: &[255, 255, 255, 255],
        };
        assert_eq!(r.count(10, 4), Err(DecodeError::TooLarge));
        let mut r = Reader {
            buf: &[3, 0, 0, 0, 1, 2, 3],
        };
        assert_eq!(r.count(10, 4), Err(DecodeError::Truncated));
        let mut r = Reader {
            buf: &[3, 0, 0, 0, 1, 2, 3],
        };
        assert_eq!(r.count(10, 1), Ok(3));
    }

    #[test]
    fn flag_accepts_only_zero_and_one() {
        assert_eq!(Reader { buf: &[0] }.flag(), Ok(false));
        assert_eq!(Reader { buf: &[1] }.flag(), Ok(true));
        assert!(matches!(
            Reader { buf: &[2] }.flag(),
            Err(DecodeError::NonCanonical(_))
        ));
    }

    #[test]
    fn device_id_must_be_a_curve_point() {
        // An all-zero x coordinate is not on the curve.
        assert_eq!(
            Reader { buf: &[0u8; 32] }.device_id(),
            Err(DecodeError::InvalidKey)
        );
    }
}
