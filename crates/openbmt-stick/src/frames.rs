//! SK-52, SK-56: the payment's frames. QR texts read from the camera are
//! gathered into a PSBT (an animated UR `crypto-psbt` or a BBQr, whichever
//! the first frame is), and a signed PSBT is cut into frames in the same form.

use openbmt_bbqr::{Joiner, Kind};
use openbmt_qr::{encode_text, Ecc, QrCode};
use openbmt_ur::{psbt_to_ur_parts, ur_to_psbt, Decoder};

/// The largest UR fragment, as the apps use (`docs/UR-RULES.md`).
pub const UR_FRAGMENT: usize = 230;
/// The longest BBQr part, in characters, as the owner phone's writer uses.
pub const BBQR_PART_CHARS: usize = 400;

/// The form a payment came in, and goes back in (SK-56).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// An animated UR `crypto-psbt`.
    Ur,
    /// A BBQr of type P.
    Bbqr,
}

/// What one QR text did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feed {
    /// Parts held out of parts needed (`total` is 0 until known).
    Progress { have: usize, total: usize },
    /// All parts are in: the form and the PSBT's bytes.
    Done { form: Form, psbt: Vec<u8> },
    /// All parts are in and they are no PSBT.
    Bad,
}

/// Gathers frames until the PSBT is whole.
pub struct Collector {
    form: Option<Form>,
    ur: Decoder,
    bbqr: Joiner,
    have: usize,
    total: usize,
}

impl std::fmt::Debug for Collector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Collector")
            .field("have", &self.have)
            .field("total", &self.total)
            .finish()
    }
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        Self {
            form: None,
            ur: Decoder::new(),
            bbqr: Joiner::new(&[Kind::Psbt]),
            have: 0,
            total: 0,
        }
    }

    /// Parts held and parts needed (0 until the first part is read).
    pub fn progress(&self) -> (usize, usize) {
        (self.have, self.total)
    }

    /// One QR text. A text that is no part of the form the first frame set
    /// is ignored, as is a part the decoder refuses.
    pub fn feed(&mut self, text: &str) -> Feed {
        let lower = text.to_ascii_lowercase();
        let seen = if lower.starts_with("ur:") {
            Form::Ur
        } else if text.starts_with("B$") {
            Form::Bbqr
        } else {
            return self.now();
        };
        if self.form.is_some_and(|f| f != seen) {
            return self.now();
        }
        match seen {
            Form::Ur => {
                if let Ok(p) = self.ur.receive(text) {
                    self.form = Some(Form::Ur);
                    self.have = p.received_parts;
                    self.total = p.expected_parts;
                    if p.complete {
                        return match self.ur.result() {
                            Some((ty, cbor)) => match ur_to_psbt(&ty, &cbor) {
                                Ok(psbt) => Feed::Done {
                                    form: Form::Ur,
                                    psbt,
                                },
                                Err(_) => Feed::Bad,
                            },
                            None => Feed::Bad,
                        };
                    }
                }
            }
            Form::Bbqr => {
                if let Ok(p) = self.bbqr.add(text) {
                    self.form = Some(Form::Bbqr);
                    self.have = p.have;
                    self.total = p.total;
                    if p.complete() {
                        return match self.bbqr.finish() {
                            Ok(d) if d.kind == Kind::Psbt => Feed::Done {
                                form: Form::Bbqr,
                                psbt: d.bytes,
                            },
                            _ => Feed::Bad,
                        };
                    }
                }
            }
        }
        self.now()
    }

    fn now(&self) -> Feed {
        Feed::Progress {
            have: self.have,
            total: self.total,
        }
    }
}

/// The QR texts of a PSBT in `form`: a UR gives its parts once and then as
/// many mixed parts again (so a part missed is made up), a BBQr every part.
pub fn texts_of(form: Form, psbt: &[u8]) -> Option<Vec<String>> {
    match form {
        Form::Ur => {
            let mut e = psbt_to_ur_parts(psbt, UR_FRAGMENT).ok()?;
            let n = if e.is_single_part() {
                1
            } else {
                e.seq_len() * 2
            };
            Some((0..n).map(|_| e.next_part_upper()).collect())
        }
        Form::Bbqr => openbmt_bbqr::encode(Kind::Psbt, psbt, BBQR_PART_CHARS).ok(),
    }
}

/// The QR codes of a PSBT's frames in `form`, at error correction level L as
/// the owner phone draws them.
pub fn codes_of(form: Form, psbt: &[u8]) -> Option<Vec<QrCode>> {
    texts_of(form, psbt)?
        .iter()
        .map(|t| encode_text(t, Ecc::L).ok())
        .collect()
}
