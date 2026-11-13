//! The payment request as a read-only NFC tag (`docs/TAP-RULES.md`).
//!
//! The counter phone answers as an NFC Forum Type 4 tag. [`tag_content`]
//! makes the bytes of the NDEF file for a request (TP-01 to TP-03) at the
//! moment the QR code's text is made; [`tap_answer`] answers one command of a
//! reader from those bytes (TP-10 to TP-15). The app passes bytes between
//! Android's card emulation and these two functions and builds, reads and
//! changes nothing of what they hold.
//!
//! The session is an opaque blob the app keeps between the commands of one
//! tap and drops when the reader goes away (TP-15): the state of the
//! selection and, for the NDEF file, the content as it was when the file was
//! selected (TP-05).

use crate::codec::{wire_record, wire_record_get};

/// The arguments of `tap_answer`, the op of [`tap_answer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapArgs {
    /// The session the last command returned; empty at the start of a tap.
    pub session: Vec<u8>,
    /// The tag's bytes while a sale waits on the payment screen.
    pub content: Option<Vec<u8>>,
    /// The command as the reader sent it.
    pub apdu: Vec<u8>,
}

wire_record!(TapArgs {
    session,
    content,
    apdu
});
wire_record_get!(TapArgs {
    session,
    content,
    apdu
});

/// The result of `tap_answer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapView {
    /// The session to pass with the next command of the tap.
    pub session: Vec<u8>,
    /// The response with its status word, to give to the reader.
    pub response: Vec<u8>,
}

wire_record!(TapView { session, response });

/// The op: [`tap_answer`] on the arguments of a call.
pub(crate) fn tap_op(args: TapArgs) -> TapView {
    let (session, response) = tap_answer(&args.session, args.content.as_deref(), &args.apdu);
    TapView { session, response }
}

/// The application ID of the tag (TP-10).
const AID: [u8; 7] = [0xD2, 0x76, 0x00, 0x00, 0x85, 0x01, 0x01];

/// The most bytes of one NDEF message (TP-03).
pub const MAX_MESSAGE: usize = 2048;

/// The most bytes of the NDEF file: the length field and the message.
const MAX_FILE: usize = MAX_MESSAGE + 2;

/// The most bytes one read answers (TP-12, TP-13).
const MAX_READ: usize = 255;

/// The longest command that is parsed: a header, `Lc`, 255 bytes and `Le`
/// (TP-14).
const MAX_COMMAND: usize = 261;

/// The capability file (TP-12): length 000F, mapping version 2.0, at most
/// 255 bytes read (`MLe`), 1 byte written (`MLc`), and the control TLV of the
/// NDEF file E104 of at most 2050 bytes, read access 00, write access FF.
const CAPABILITY: [u8; 15] = [
    0x00, 0x0F, 0x20, 0x00, 0xFF, 0x00, 0x01, 0x04, 0x06, 0xE1, 0x04, 0x08, 0x02, 0x00, 0xFF,
];

const OK: [u8; 2] = [0x90, 0x00];
const WRONG_LENGTH: [u8; 2] = [0x67, 0x00];
const NOT_ALLOWED: [u8; 2] = [0x69, 0x82];
const NOT_SELECTED: [u8; 2] = [0x69, 0x86];
const NOT_FOUND: [u8; 2] = [0x6A, 0x82];
const WRONG_P1P2: [u8; 2] = [0x6A, 0x86];
const BAD_OFFSET: [u8; 2] = [0x6B, 0x00];
const NO_INSTRUCTION: [u8; 2] = [0x6D, 0x00];
const NO_CLASS: [u8; 2] = [0x6E, 0x00];

/// The NDEF file for a request (TP-01 to TP-03): the length of the message
/// as two bytes, then one URI record, type `U`, identifier code 0, holding
/// `request_text` as it is. The caller passes the request in lower case, in
/// the form the app copies (CR-83). `None` when the message would be more
/// than 2048 bytes.
pub fn tag_content(request_text: &str) -> Option<Vec<u8>> {
    let payload_len = request_text.len().checked_add(1)?;
    let short = payload_len <= 255;
    let mut record = Vec::with_capacity(payload_len + 7);
    // MB and ME set, TNF 1; the short-record bit when the length is one byte.
    record.push(if short { 0xD1 } else { 0xC1 });
    // The type is one byte long.
    record.push(1);
    if short {
        record.push(u8::try_from(payload_len).ok()?);
    } else {
        record.extend_from_slice(&u32::try_from(payload_len).ok()?.to_be_bytes());
    }
    record.push(b'U');
    record.push(0);
    record.extend_from_slice(request_text.as_bytes());
    if record.len() > MAX_MESSAGE {
        return None;
    }
    let mut file = Vec::with_capacity(record.len() + 2);
    file.extend_from_slice(&u16::try_from(record.len()).ok()?.to_be_bytes());
    file.extend_from_slice(&record);
    Some(file)
}

/// What is selected.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Selected {
    /// Nothing: the start of a tap, or after the reader went away.
    Nothing,
    /// The application.
    Application,
    /// The capability file E103.
    Capability,
    /// The NDEF file E104, as it was when it was selected (TP-05).
    Ndef(Vec<u8>),
}

impl Selected {
    /// The blob the app keeps: one state byte, and for the NDEF file the
    /// length as two bytes and the snapshot.
    fn to_session(&self) -> Vec<u8> {
        match self {
            Self::Nothing => Vec::new(),
            Self::Application => vec![1],
            Self::Capability => vec![2],
            Self::Ndef(snapshot) => {
                let mut out = vec![3];
                // A snapshot is at most 2050 bytes, so the length fits.
                out.extend_from_slice(&u16::try_from(snapshot.len()).unwrap_or(0).to_be_bytes());
                out.extend_from_slice(snapshot);
                out
            }
        }
    }

    /// Reads a blob; one of another shape is the start state.
    fn from_session(session: &[u8]) -> Self {
        match session {
            [1] => Self::Application,
            [2] => Self::Capability,
            [3, hi, lo, snapshot @ ..]
                if snapshot.len() <= MAX_FILE
                    && usize::from(u16::from_be_bytes([*hi, *lo])) == snapshot.len() =>
            {
                Self::Ndef(snapshot.to_vec())
            }
            _ => Self::Nothing,
        }
    }
}

/// One command, split by its lengths.
struct Command<'a> {
    cla: u8,
    ins: u8,
    p1: u8,
    p2: u8,
    /// The command data, when the command has `Lc`.
    data: Option<&'a [u8]>,
    /// The expected length, 0 standing for 256, when the command has `Le`.
    le: Option<usize>,
}

/// Parses a short command: header, then optionally `Lc` and data, then
/// optionally `Le`. The lengths must agree with the bytes (TP-14). Extended
/// lengths are not parsed: a command that does not fit the short forms does
/// not agree.
fn parse(apdu: &[u8]) -> Option<Command<'_>> {
    if apdu.len() > MAX_COMMAND {
        return None;
    }
    let (&cla, rest) = apdu.split_first()?;
    let (&ins, rest) = rest.split_first()?;
    let (&p1, rest) = rest.split_first()?;
    let (&p2, rest) = rest.split_first()?;
    let (data, le) = match rest {
        [] => (None, None),
        // Case 2: only `Le`.
        [le] => (None, Some(if *le == 0 { 256 } else { usize::from(*le) })),
        [lc, body @ ..] => {
            let lc = usize::from(*lc);
            // `Lc` is 1 to 255 when it is there.
            if lc == 0 {
                return None;
            }
            match body.len().checked_sub(lc)? {
                0 => (body.get(..lc), None),
                1 => {
                    let le = body.get(lc).copied()?;
                    (
                        body.get(..lc),
                        Some(if le == 0 { 256 } else { usize::from(le) }),
                    )
                }
                _ => return None,
            }
        }
    };
    Some(Command {
        cla,
        ins,
        p1,
        p2,
        data,
        le,
    })
}

fn answer(data: &[u8], status: [u8; 2]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.extend_from_slice(&status);
    out
}

/// A usable NDEF file: the length field and a message of at most 2048 bytes.
fn usable(content: &[u8]) -> bool {
    (2..=MAX_FILE).contains(&content.len())
}

/// Answers one command of a reader.
///
/// `session` is what this function returned for the last command of the tap
/// (empty at the start); `content` is the NDEF file of the sale waiting now,
/// or `None` when no sale is waiting on the payment screen; `apdu` is the
/// command as the reader sent it. Returns the new session and the response
/// with its status word.
///
/// With no content every command is answered 6A82 and the session is
/// dropped (TP-04), so the snapshot of a sale that ended is never read: that
/// is how reads of the old content are refused when the sale ends (TP-05).
/// While content is there, a snapshot taken at the selection of the NDEF file
/// is read as it was, even when the content has changed since.
pub fn tap_answer(session: &[u8], content: Option<&[u8]>, apdu: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let Some(content) = content.filter(|c| usable(c)) else {
        return (Vec::new(), answer(&[], NOT_FOUND));
    };
    let state = Selected::from_session(session);
    // A refused command changes nothing.
    let keep = |state: &Selected, response: Vec<u8>| (state.to_session(), response);
    let Some(cmd) = parse(apdu) else {
        return keep(&state, answer(&[], WRONG_LENGTH));
    };
    if cmd.cla != 0x00 {
        return keep(&state, answer(&[], NO_CLASS));
    }
    match cmd.ins {
        0xA4 => select(state, &cmd, content),
        0xB0 => {
            let response = read(&state, &cmd);
            keep(&state, response)
        }
        // No reader writes to the tag.
        0xD6 => keep(&state, answer(&[], NOT_ALLOWED)),
        _ => keep(&state, answer(&[], NO_INSTRUCTION)),
    }
}

/// SELECT by name of the application, or by file ID of E103 and E104.
fn select(state: Selected, cmd: &Command<'_>, content: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let refuse = |status: [u8; 2]| (state.to_session(), answer(&[], status));
    let Some(data) = cmd.data else {
        return refuse(WRONG_LENGTH);
    };
    if !matches!(cmd.p2, 0x00 | 0x0C) {
        return refuse(WRONG_P1P2);
    }
    match cmd.p1 {
        0x04 => {
            if data.len() != AID.len() {
                return refuse(WRONG_LENGTH);
            }
            if data != AID {
                return refuse(NOT_FOUND);
            }
            // Selecting the application again forgets the file (TP-13).
            (Selected::Application.to_session(), answer(&[], OK))
        }
        0x00 => {
            if data.len() != 2 {
                return refuse(WRONG_LENGTH);
            }
            if state == Selected::Nothing {
                return refuse(NOT_FOUND);
            }
            let next = match data {
                [0xE1, 0x03] => Selected::Capability,
                [0xE1, 0x04] => Selected::Ndef(content.to_vec()),
                _ => return refuse(NOT_FOUND),
            };
            (next.to_session(), answer(&[], OK))
        }
        _ => refuse(WRONG_P1P2),
    }
}

/// READ BINARY of the file selected.
fn read(state: &Selected, cmd: &Command<'_>) -> Vec<u8> {
    let (Some(le), None) = (cmd.le, cmd.data) else {
        return answer(&[], WRONG_LENGTH);
    };
    let file: &[u8] = match state {
        Selected::Nothing | Selected::Application => return answer(&[], NOT_SELECTED),
        Selected::Capability => &CAPABILITY,
        Selected::Ndef(snapshot) => snapshot,
    };
    // The top bit of P1 names a file by a short ID, which is not offered.
    if cmd.p1 & 0x80 != 0 {
        return answer(&[], WRONG_P1P2);
    }
    let offset = usize::from(u16::from_be_bytes([cmd.p1, cmd.p2]));
    let Some(rest) = file.get(offset..) else {
        return answer(&[], BAD_OFFSET);
    };
    let take = le.min(MAX_READ).min(rest.len());
    answer(rest.get(..take).unwrap_or(&[]), OK)
}
