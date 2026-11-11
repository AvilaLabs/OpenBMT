//! The second call of the library: QR drawing and BC-UR sequences for the
//! Android apps, behind one framed request and one framed reply
//! (`docs/UR-RULES.md`, section 6). Safe code only; `src/jni.rs` carries the
//! bytes in and out.
//!
//! Request, all integers big-endian: [`REQUEST_MAGIC`], the version byte
//! [`VERSION`], the operation byte, then the fields of the operation and
//! nothing after them. Reply: [`REPLY_MAGIC`], [`VERSION`], a status byte
//! ([`OK`] or [`REFUSED`]) and then the body of the operation, or, for a
//! refusal, the rule code as five ASCII bytes (`UR-12`, `QR-01`).
//!
//! | Operation | Fields | Body of the reply |
//! |---|---|---|
//! | [`OP_QR_ENCODE`] | level (0 to 3 for L to H), length `u32`, text | side `u16`, the modules packed row by row, most significant bit first |
//! | [`OP_UR_ENCODE`] | type length `u8`, type, largest fragment `u16`, length `u32`, payload | count `u32`, then each part as a length `u32` and its upper-case text |
//! | [`OP_DECODER_NEW`] | none | handle `u32` |
//! | [`OP_DECODER_RECEIVE`] | handle `u32`, length `u32`, text | complete `u8`, received `u32`, expected `u32`, fraction in millionths `u32` |
//! | [`OP_DECODER_RESULT`] | handle `u32` | type length `u8`, type, length `u32`, payload |
//! | [`OP_DECODER_DROP`] | handle `u32` | none |
//! | [`OP_KEY_TEXT`] | type length `u8`, type, length `u32`, message | the key text, in ASCII |
//!
//! A refusal of the key itself (UR-23 to UR-29) is a reply with the status
//! [`REFUSED_WITH_WORDS`]: the rule code as five bytes, then the heading,
//! what happened and what to do, each as a length `u16` and UTF-8.
//!
//! The payload of `ur_encode` and of a result is the content itself: the
//! CBOR byte string of the types (UR-13, UR-14) is added and removed here.
//! A result of a type that `key_text` reads (`crypto-account`,
//! `crypto-hdkey` and the newer `hdkey`, `account-descriptor` and
//! `output-descriptor`, so that they can be refused with words) is its CBOR
//! as it came; `key_text` takes that, and for `bytes` the byte string around
//! the content.
//! A multi-part `ur_encode` answers with one full turn of the sequence, twice
//! its length, so that a screen can show the parts round and round; a whole
//! message answers with its one part.
//!
//! The only state is the table of decoders: at most [`MAX_DECODERS`] at a
//! time, each with the bounds of `openbmt-ur`, and a handle is never given
//! twice in the life of the process.

use std::sync::{Mutex, PoisonError};

use openbmt_qr::{encode_with, Ecc};
use openbmt_ur::cbor::{decode_bytes, encode_bytes};
use openbmt_ur::{
    check_type, psbt_to_ur_parts, ur_to_key_text, ur_to_psbt, Decoder, Encoder, KeyRefusal,
    MIN_FRAGMENT_LEN, TYPE_CRYPTO_PSBT, TYPE_PSBT,
};

/// The first four bytes of a request.
pub const REQUEST_MAGIC: [u8; 4] = *b"OBMo";
/// The first four bytes of a reply.
pub const REPLY_MAGIC: [u8; 4] = *b"OBMp";
/// The version byte of both.
pub const VERSION: u8 = 1;

/// A reply that holds the body of the operation.
pub const OK: u8 = 0;
/// A reply that holds a rule code.
pub const REFUSED: u8 = 1;
/// A reply that holds a rule code and the words for it (`key_text` only).
pub const REFUSED_WITH_WORDS: u8 = 2;

/// Operation: draw a text as a QR code.
pub const OP_QR_ENCODE: u8 = 1;
/// Operation: a message as the parts of a UR.
pub const OP_UR_ENCODE: u8 = 2;
/// Operation: open a decoder.
pub const OP_DECODER_NEW: u8 = 3;
/// Operation: give a decoder the text of one part.
pub const OP_DECODER_RECEIVE: u8 = 4;
/// Operation: take the message of a complete decoder.
pub const OP_DECODER_RESULT: u8 = 5;
/// Operation: close a decoder.
pub const OP_DECODER_DROP: u8 = 6;
/// Operation: the cosigner key text in a finished UR (UR-31).
pub const OP_KEY_TEXT: u8 = 7;

/// The most decoders open at once.
pub const MAX_DECODERS: usize = 4;

/// The longest request, in bytes: the largest text a decoder reads (about
/// 2.1 million characters) and the largest message an encoder takes (1 MiB)
/// both fit.
pub const MAX_CALL: usize = 4 * 1024 * 1024;

/// UR-20: a type that the apps do not send, or a result of such a type.
pub const TYPE_NOT_USED: &str = "UR-20";
/// UR-21: a request that is not exactly the framing above.
pub const BAD_FRAMING: &str = "UR-21";
/// UR-22: a handle that is not open, a decoder table that is full, or a
/// result asked for before the message is complete.
pub const BAD_STATE: &str = "UR-22";

/// The type of OpenBMT's own framed objects.
const TYPE_BYTES: &str = "bytes";

/// A refusal: the rule code to give back, and the words when there are any.
struct Refusal {
    code: String,
    words: Option<KeyRefusal>,
}

impl Refusal {
    fn of(code: &str) -> Self {
        Self {
            code: code.to_string(),
            words: None,
        }
    }
}

type Done = Result<Vec<u8>, Refusal>;

/// The first word of a library error's text: its rule code.
fn code_of(text: &str) -> Refusal {
    Refusal::of(
        text.split(|c: char| c == ':' || c.is_whitespace())
            .next()
            .unwrap_or(BAD_FRAMING),
    )
}

/// A reader over the fields of a request: every take is checked.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Refusal> {
        let (head, rest) = self
            .0
            .split_at_checked(n)
            .ok_or_else(|| Refusal::of(BAD_FRAMING))?;
        self.0 = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, Refusal> {
        self.take(1)?
            .first()
            .copied()
            .ok_or_else(|| Refusal::of(BAD_FRAMING))
    }

    fn u16(&mut self) -> Result<u16, Refusal> {
        let b: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| Refusal::of(BAD_FRAMING))?;
        Ok(u16::from_be_bytes(b))
    }

    fn u32(&mut self) -> Result<u32, Refusal> {
        let b: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| Refusal::of(BAD_FRAMING))?;
        Ok(u32::from_be_bytes(b))
    }

    /// A `u32` length and that many bytes.
    fn sized(&mut self) -> Result<&'a [u8], Refusal> {
        let n = usize::try_from(self.u32()?).map_err(|_| Refusal::of(BAD_FRAMING))?;
        self.take(n)
    }

    /// Nothing may be left.
    fn end(&self) -> Result<(), Refusal> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(Refusal::of(BAD_FRAMING))
        }
    }
}

fn push_u32(out: &mut Vec<u8>, n: usize) -> Result<(), Refusal> {
    let n = u32::try_from(n).map_err(|_| Refusal::of(BAD_FRAMING))?;
    out.extend_from_slice(&n.to_be_bytes());
    Ok(())
}

/// The rule code of a refusal of the key and its three texts.
fn with_words(code: &str, why: &KeyRefusal) -> Vec<u8> {
    let words = why.words();
    let mut out = code.as_bytes().to_vec();
    for text in [words.heading, words.what, words.next] {
        // Each text is a short fixed sentence, far under 64 KiB.
        let len = u16::try_from(text.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(text.as_bytes());
    }
    out
}

fn framed(status: u8, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + body.len());
    out.extend_from_slice(&REPLY_MAGIC);
    out.push(VERSION);
    out.push(status);
    out.extend_from_slice(body);
    out
}

/// The decoders open now. A handle is never given twice.
pub struct Calls {
    table: Mutex<Table>,
}

struct Table {
    next: u32,
    open: Vec<(u32, Decoder)>,
}

impl Default for Calls {
    fn default() -> Self {
        Self::new()
    }
}

impl Calls {
    /// No decoder open; the first handle is 1.
    pub const fn new() -> Self {
        Self {
            table: Mutex::new(Table {
                next: 1,
                open: Vec::new(),
            }),
        }
    }

    /// Run one request and frame the answer. Never panics on any bytes.
    pub fn call(&self, request: &[u8]) -> Vec<u8> {
        match self.run(request) {
            Ok(body) => framed(OK, &body),
            Err(Refusal {
                code,
                words: Some(why),
            }) => framed(REFUSED_WITH_WORDS, &with_words(&code, &why)),
            Err(Refusal { code, words: None }) => framed(REFUSED, code.as_bytes()),
        }
    }

    /// The number of decoders open (for the tests).
    pub fn open_decoders(&self) -> usize {
        self.lock(|t| t.open.len())
    }

    fn lock<R>(&self, f: impl FnOnce(&mut Table) -> R) -> R {
        // A panic while the table was held cannot leave it half-changed:
        // each change is one push, one removal or one counter step.
        let mut guard = self.table.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut guard)
    }

    fn run(&self, request: &[u8]) -> Done {
        if request.len() > MAX_CALL {
            return Err(Refusal::of(BAD_FRAMING));
        }
        let mut r = Reader(request);
        if r.take(4)? != REQUEST_MAGIC || r.u8()? != VERSION {
            return Err(Refusal::of(BAD_FRAMING));
        }
        match r.u8()? {
            OP_QR_ENCODE => qr_encode(r),
            OP_UR_ENCODE => ur_encode(r),
            OP_KEY_TEXT => key_text(r),
            OP_DECODER_NEW => {
                r.end()?;
                self.decoder_new()
            }
            OP_DECODER_RECEIVE => {
                let handle = r.u32()?;
                let text = r.sized()?;
                r.end()?;
                self.decoder_receive(handle, text)
            }
            OP_DECODER_RESULT => {
                let handle = r.u32()?;
                r.end()?;
                self.decoder_result(handle)
            }
            OP_DECODER_DROP => {
                let handle = r.u32()?;
                r.end()?;
                self.decoder_drop(handle)
            }
            _ => Err(Refusal::of(BAD_FRAMING)),
        }
    }

    fn decoder_new(&self) -> Done {
        self.lock(|t| {
            if t.open.len() >= MAX_DECODERS || t.next == u32::MAX {
                return Err(Refusal::of(BAD_STATE));
            }
            let handle = t.next;
            t.next += 1;
            t.open.push((handle, Decoder::new()));
            Ok(handle.to_be_bytes().to_vec())
        })
    }

    fn decoder_receive(&self, handle: u32, text: &[u8]) -> Done {
        // Text that is not UTF-8 cannot be a UR (UR-07).
        let text = std::str::from_utf8(text)
            .map_err(|_| code_of("UR-07 the text is not the shape of a UR"))?;
        self.lock(|t| {
            let decoder = t
                .open
                .iter_mut()
                .find(|(h, _)| *h == handle)
                .map(|(_, d)| d)
                .ok_or_else(|| Refusal::of(BAD_STATE))?;
            let p = decoder.receive(text).map_err(|e| code_of(&e.to_string()))?;
            let mut out = Vec::with_capacity(13);
            out.push(u8::from(p.complete));
            push_u32(&mut out, p.received_parts)?;
            push_u32(&mut out, p.expected_parts)?;
            let millionths = (p.estimated_fraction.clamp(0.0, 1.0) * 1_000_000.0) as u32;
            out.extend_from_slice(&millionths.to_be_bytes());
            Ok(out)
        })
    }

    fn decoder_result(&self, handle: u32) -> Done {
        let (ur_type, cbor) = self.lock(|t| {
            let decoder = t
                .open
                .iter()
                .find(|(h, _)| *h == handle)
                .map(|(_, d)| d)
                .ok_or_else(|| Refusal::of(BAD_STATE))?;
            decoder.result().ok_or_else(|| Refusal::of(BAD_STATE))
        })?;
        let payload = match ur_type.as_str() {
            // The key types are read by `key_text`, which wants the CBOR.
            "crypto-account" | "crypto-hdkey" | "hdkey" | "account-descriptor"
            | "output-descriptor" => cbor,
            TYPE_BYTES => decode_bytes(&cbor)
                .map_err(|e| code_of(&e.to_string()))?
                .to_vec(),
            TYPE_CRYPTO_PSBT | TYPE_PSBT => {
                ur_to_psbt(&ur_type, &cbor).map_err(|e| code_of(&e.to_string()))?
            }
            _ => return Err(Refusal::of(TYPE_NOT_USED)),
        };
        let mut out = Vec::with_capacity(5 + ur_type.len() + payload.len());
        out.push(u8::try_from(ur_type.len()).map_err(|_| Refusal::of(BAD_FRAMING))?);
        out.extend_from_slice(ur_type.as_bytes());
        push_u32(&mut out, payload.len())?;
        out.extend_from_slice(&payload);
        Ok(out)
    }

    fn decoder_drop(&self, handle: u32) -> Done {
        self.lock(|t| {
            let at = t
                .open
                .iter()
                .position(|(h, _)| *h == handle)
                .ok_or_else(|| Refusal::of(BAD_STATE))?;
            t.open.remove(at);
            Ok(Vec::new())
        })
    }
}

fn qr_encode(mut r: Reader<'_>) -> Done {
    let ecc = match r.u8()? {
        0 => Ecc::L,
        1 => Ecc::M,
        2 => Ecc::Q,
        3 => Ecc::H,
        _ => return Err(Refusal::of(BAD_FRAMING)),
    };
    let text = r.sized()?;
    r.end()?;
    let code = encode_with(text, ecc, None, None).map_err(|e| code_of(&e.to_string()))?;
    let side = code.size();
    let mut out = Vec::with_capacity(2 + (side * side).div_ceil(8));
    out.extend_from_slice(
        &u16::try_from(side)
            .map_err(|_| Refusal::of(BAD_FRAMING))?
            .to_be_bytes(),
    );
    let mut byte = 0u8;
    let mut bits = 0u32;
    for y in 0..side {
        for x in 0..side {
            byte = (byte << 1) | u8::from(code.get(x, y));
            bits += 1;
            if bits == 8 {
                out.push(byte);
                byte = 0;
                bits = 0;
            }
        }
    }
    if bits > 0 {
        out.push(byte << (8 - bits));
    }
    Ok(out)
}

fn ur_encode(mut r: Reader<'_>) -> Done {
    let type_len = usize::from(r.u8()?);
    let ur_type = std::str::from_utf8(r.take(type_len)?).map_err(|_| Refusal::of(BAD_FRAMING))?;
    let max_fragment = usize::from(r.u16()?);
    let payload = r.sized()?;
    r.end()?;
    check_type(ur_type).map_err(|e| code_of(&e.to_string()))?;
    // The message is the payload itself (UR-02): the byte string around it
    // is never empty.
    if payload.is_empty() {
        return Err(Refusal::of("UR-02"));
    }
    let mut encoder: Encoder = match ur_type {
        TYPE_BYTES => Encoder::new(
            TYPE_BYTES,
            &encode_bytes(payload),
            max_fragment,
            MIN_FRAGMENT_LEN,
        ),
        TYPE_CRYPTO_PSBT => psbt_to_ur_parts(payload, max_fragment),
        _ => return Err(Refusal::of(TYPE_NOT_USED)),
    }
    .map_err(|e| code_of(&e.to_string()))?;
    let count = if encoder.is_single_part() {
        1
    } else {
        encoder.seq_len().saturating_mul(2)
    };
    let mut out = Vec::new();
    push_u32(&mut out, count)?;
    for _ in 0..count {
        let part = encoder.next_part_upper();
        push_u32(&mut out, part.len())?;
        out.extend_from_slice(part.as_bytes());
    }
    Ok(out)
}

/// UR-31: the key text of a finished UR of a type and its message.
fn key_text(mut r: Reader<'_>) -> Done {
    let type_len = usize::from(r.u8()?);
    let ur_type = std::str::from_utf8(r.take(type_len)?).map_err(|_| Refusal::of(BAD_FRAMING))?;
    let message = r.sized()?;
    r.end()?;
    check_type(ur_type).map_err(|e| code_of(&e.to_string()))?;
    match ur_to_key_text(ur_type, message) {
        Ok(key) => Ok(key.into_string().into_bytes()),
        Err(why) => Err(Refusal {
            code: why.rule().to_string(),
            words: Some(why),
        }),
    }
}

/// The table of the exported call: one for the process.
static CALLS: Calls = Calls::new();

/// Run one request against the process's decoders (`src/jni.rs`).
pub fn call(request: &[u8]) -> Vec<u8> {
    CALLS.call(request)
}
